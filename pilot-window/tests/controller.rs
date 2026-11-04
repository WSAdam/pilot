//! The window controller against a real in-process browser: it follows the
//! agent's page, shows its screencast, and forwards a human's navigation and
//! input to it.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use futures_util::SinkExt;
use moli_core::{
    LayoutPolicy, OptionalResourceFetchMask,
    runtime::{NavigationRuntimeConfig, storage_partition::StoragePartitionState},
};
use moli_fetch::FetchConfig;
use moli_protocol_server::{
    BrowserInstanceControl, LocalCdpClient, LocalCdpConnector, ProtocolServer, ServerConfig,
};
use pilot_window::{
    agent::ACTIVE_FOR,
    controller::{Command, Update, run_controller},
    geometry::{ChromeLayout, FrameMetadata, Rect},
    input::{self, Key, Modifiers, MouseButton},
    text::TextRenderer,
    view::View,
};
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};

const WAIT: Duration = Duration::from_secs(20);

const BUTTON_PAGE: &str = "data:text/html,<title>waiting</title><button style='position:fixed;inset:0;width:100%25;height:100%25' onclick='document.title=%22clicked%22'>take over</button>";
const INPUT_PAGE: &str = "data:text/html,<title>form</title><input id=field style='position:fixed;inset:0;width:100%25;height:100%25;font-size:40px'>";

async fn start_browser() -> (
    LocalCdpConnector,
    SocketAddr,
    JoinHandle<anyhow::Result<()>>,
) {
    let server = ProtocolServer::new_with_storage_partition_and_runtime_config(
        ServerConfig {
            host: "127.0.0.1".to_owned(),
            port: 0,
            ..ServerConfig::default()
        },
        Arc::new(StoragePartitionState::open(None).unwrap()),
        NavigationRuntimeConfig::new(
            FetchConfig::default(),
            OptionalResourceFetchMask::NONE,
            true,
            LayoutPolicy::Live,
        ),
    );
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (bound_tx, bound_rx) = tokio::sync::oneshot::channel();
    let serving = tokio::spawn(async move {
        server
            .serve_with_control_bound_hook_and_local_clients(
                BrowserInstanceControl::new(),
                move |address| {
                    let _ = bound_tx.send(address);
                },
                move |connector| {
                    let _ = ready_tx.send(connector);
                },
            )
            .await
    });
    let address = bound_rx.await.unwrap();
    (ready_rx.await.unwrap(), address, serving)
}

/// The agent's side: a plain CDP client that waits for its own replies.
struct Agent {
    client: LocalCdpClient,
    next_id: u64,
    replies: HashMap<u64, Value>,
}

impl Agent {
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let mut command = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            command["sessionId"] = json!(session);
        }
        assert!(self.client.sender().send(command.to_string()).await);
        tokio::time::timeout(WAIT, async {
            loop {
                if let Some(reply) = self.replies.remove(&id) {
                    return reply;
                }
                let message: Value =
                    serde_json::from_str(&self.client.recv().await.unwrap()).unwrap();
                if let Some(reply_id) = message["id"].as_u64() {
                    self.replies.insert(reply_id, message);
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{method} replies"))
    }
}

/// A browser, the agent's page on `url`, and the window controller showing it.
struct Harness {
    agent: Agent,
    session: String,
    target_id: String,
    first_frame: FrameMetadata,
    address: SocketAddr,
    commands: mpsc::UnboundedSender<Command>,
    updates: mpsc::UnboundedReceiver<Update>,
    serving: JoinHandle<anyhow::Result<()>>,
    controller: JoinHandle<anyhow::Result<()>>,
    /// What the window shows, from every update received so far.
    view: View,
}

impl Harness {
    async fn start(url: &str) -> Self {
        let (connector, address, serving) = start_browser().await;
        let mut agent = Agent {
            client: connector.connect().await.unwrap(),
            next_id: 0,
            replies: HashMap::new(),
        };
        let created = agent
            .call("Target.createTarget", json!({"url": url}), None)
            .await;
        let target_id = created["result"]["targetId"].as_str().unwrap().to_owned();
        let session = agent
            .call(
                "Target.attachToTarget",
                json!({"targetId": target_id, "flatten": true}),
                None,
            )
            .await["result"]["sessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (update_tx, updates) = mpsc::unbounded_channel();
        let controller = tokio::spawn(run_controller(connector, command_rx, move |update| {
            let _ = update_tx.send(update);
        }));
        let mut harness = Self {
            agent,
            session,
            target_id,
            first_frame: FrameMetadata {
                device_width: 0.0,
                device_height: 0.0,
            },
            address,
            commands,
            updates,
            serving,
            controller,
            view: View::default(),
        };
        let target_id = harness.target_id.clone();
        let Update::Tabs { tabs, active } = harness
            .next_update(|update| {
                matches!(update, Update::Tabs { tabs, .. } if tabs.iter().any(|tab| tab.target_id == target_id))
            })
            .await
        else {
            unreachable!()
        };
        assert_eq!(
            active.map(|index| tabs[index].target_id.as_str()),
            Some(harness.target_id.as_str()),
            "the window shows the page the agent opened"
        );
        let Update::Frame { frame, metadata } = harness.next_frame().await else {
            unreachable!()
        };
        assert!(frame.width > 0 && frame.height > 0);
        assert!(metadata.device_width > 0.0 && metadata.device_height > 0.0);
        harness.first_frame = metadata;
        harness
    }

    fn send(&self, command: Command) {
        self.commands.send(command).unwrap();
    }

    async fn next_update(&mut self, mut wanted: impl FnMut(&Update) -> bool) -> Update {
        tokio::time::timeout(WAIT, async {
            loop {
                let update = self
                    .updates
                    .recv()
                    .await
                    .expect("the controller is running");
                self.view.apply(update.clone());
                if wanted(&update) {
                    return update;
                }
            }
        })
        .await
        .expect("the controller sends the update")
    }

    async fn next_frame(&mut self) -> Update {
        self.next_update(|update| matches!(update, Update::Frame { .. }))
            .await
    }

    async fn evaluate(&mut self, expression: &str) -> Value {
        let session = self.session.clone();
        self.agent
            .call(
                "Runtime.evaluate",
                json!({"expression": expression, "returnByValue": true}),
                Some(&session),
            )
            .await["result"]["result"]["value"]
            .clone()
    }

    /// Polls the agent's page until `expression` evaluates to `expected`.
    async fn wait_for(&mut self, expression: &str, expected: Value) {
        let mut last = Value::Null;
        for _ in 0..100 {
            last = self.evaluate(expression).await;
            if last == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("{expression} stayed {last}, expected {expected}");
    }

    fn click_page_center(&self) {
        let (x, y) = (
            self.first_frame.device_width / 2.0,
            self.first_frame.device_height / 2.0,
        );
        for pressed in [true, false] {
            self.send(Command::Mouse(input::mouse_button(
                pressed,
                x,
                y,
                MouseButton::Left,
                u32::from(pressed),
                1,
                Modifiers::default(),
            )));
        }
    }

    /// Closing the window stops the browser, and the controller ends with it.
    async fn close(self) {
        self.send(Command::Close);
        tokio::time::timeout(WAIT, self.serving)
            .await
            .expect("closing the window stops the browser")
            .unwrap()
            .unwrap();
        tokio::time::timeout(WAIT, self.controller)
            .await
            .expect("the controller ends with the browser")
            .unwrap()
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_window_follows_the_agent_shows_its_page_and_forwards_a_takeover_click() {
    let mut harness = Harness::start(BUTTON_PAGE).await;
    // Like Chromium, a frame is captured again only when the page changes;
    // it reaches the window because the window acknowledged the last one.
    harness
        .evaluate("document.body.style.background = 'red'")
        .await;
    harness.next_frame().await;

    harness.click_page_center();
    harness.wait_for("document.title", json!("clicked")).await;
    harness.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_window_navigates_the_shown_tab_and_types_into_it() {
    let mut harness = Harness::start(BUTTON_PAGE).await;
    harness.send(Command::Navigate(INPUT_PAGE.to_owned()));
    harness.wait_for("location.href", json!(INPUT_PAGE)).await;

    harness.click_page_center();
    for character in ["h", "i"] {
        for pressed in [true, false] {
            harness.send(Command::Key(input::key(
                Key::Character(character),
                pressed,
                Modifiers::default(),
            )));
        }
    }
    harness.wait_for("field.value", json!("hi")).await;

    harness.send(Command::Back);
    harness.wait_for("location.href", json!(BUTTON_PAGE)).await;
    harness.send(Command::Forward);
    harness.wait_for("location.href", json!(INPUT_PAGE)).await;
    harness.evaluate("window.marker = 1").await;
    harness.send(Command::Reload);
    harness
        .wait_for("typeof window.marker", json!("undefined"))
        .await;
    harness.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_screencast_pauses_while_the_window_is_hidden() {
    let mut harness = Harness::start(BUTTON_PAGE).await;
    harness.send(Command::SetVisible(false));
    // Let any frame already in flight arrive, then change the page.
    tokio::time::sleep(Duration::from_millis(500)).await;
    while harness.updates.try_recv().is_ok() {}
    harness
        .evaluate("document.body.style.background = 'blue'")
        .await;
    let hidden = tokio::time::timeout(
        Duration::from_millis(1500),
        harness.next_update(|update| matches!(update, Update::Frame { .. })),
    )
    .await;
    assert!(hidden.is_err(), "no frames while the window is hidden");

    harness.send(Command::SetVisible(true));
    harness.next_frame().await;
    harness.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_indicator_lights_while_the_agent_sends_commands() {
    let mut harness = Harness::start(BUTTON_PAGE).await;
    // A real agent connects over the websocket, as Playwright MCP's
    // --cdp-endpoint mode does; only its commands count.
    let url = format!("ws://{}/devtools/browser/moli-browser", harness.address);
    let stream = tokio::net::TcpStream::connect(harness.address)
        .await
        .unwrap();
    let (mut socket, _) = tokio_tungstenite::client_async(url, stream).await.unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::text(
            json!({"id": 1, "method": "Browser.getVersion"}).to_string(),
        ))
        .await
        .unwrap();
    harness
        .next_update(|update| matches!(update, Update::AgentActive(true)))
        .await;
    let quiet = tokio::time::Instant::now();
    harness
        .next_update(|update| matches!(update, Update::AgentActive(false)))
        .await;
    assert!(
        quiet.elapsed() >= ACTIVE_FOR / 2,
        "the indicator stays on for a while"
    );
    drop(socket);
    harness.close().await;
}

const SOLID_PAGE: &str =
    "data:text/html,<title>solid</title><body style='margin:0;background:rgb(0,96,200)'>";
const SOLID: (u8, u8, u8) = (0, 96, 200);
const BADGE_IDLE: u32 = 0xf1f3f4;
const BADGE_ACTIVE: u32 = 0x1a73e8;

/// Whether `word` is `color`, give or take JPEG noise.
fn near(word: u32, (red, green, blue): (u8, u8, u8)) -> bool {
    let close = |shift: u32, wanted: u8| ((word >> shift) & 0xff).abs_diff(u32::from(wanted)) <= 12;
    close(16, red) && close(8, green) && close(0, blue)
}

/// Pixels in `area` dark enough to be label text.
fn dark_pixels(buffer: &[u32], width: usize, area: Rect) -> usize {
    (area.y as usize..area.bottom() as usize)
        .flat_map(|y| (area.x as usize..area.right() as usize).map(move |x| y * width + x))
        .filter(|&index| {
            let word = buffer[index];
            [word >> 16, word >> 8, word]
                .iter()
                .all(|channel| (*channel & 0xff) < 128)
        })
        .count()
}

/// What a window of `layout`'s size would show for `view`.
fn compose(view: &View, layout: &ChromeLayout, text: Option<&TextRenderer>) -> Vec<u32> {
    let (width, height) = (
        layout.tab_strip.width as usize,
        layout.content.bottom() as usize,
    );
    let mut buffer = vec![0; width * height];
    view.compose(layout, text, &mut buffer, width, height);
    buffer
}

impl Harness {
    /// Takes updates until the window's view satisfies `ready`.
    async fn view_until(&mut self, mut ready: impl FnMut(&View) -> bool) -> &View {
        let reached = tokio::time::timeout(WAIT, async {
            while !ready(&self.view) {
                let update = self
                    .updates
                    .recv()
                    .await
                    .expect("the controller is running");
                self.view.apply(update);
            }
        })
        .await;
        assert!(
            reached.is_ok(),
            "the window never got there; it shows tabs {:?} (active {:?}), agent active {}, a frame: {}",
            self.view.tabs,
            self.view.active,
            self.view.agent_active,
            self.view.frame.is_some()
        );
        &self.view
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_composed_window_shows_the_agents_page_its_title_and_the_agent_badge() {
    let mut harness = Harness::start(SOLID_PAGE).await;
    let layout = ChromeLayout::new(800, 600, 1.0);
    let width = layout.tab_strip.width as usize;
    let text = TextRenderer::system();
    assert!(
        text.is_some() || !cfg!(target_os = "macos"),
        "macOS always has a system UI font"
    );
    let shows_page = |view: &View| {
        view.frame.as_ref().is_some_and(|(frame, _)| {
            near(
                frame.pixels
                    [(frame.height as usize / 2) * frame.width as usize + frame.width as usize / 2],
                SOLID,
            )
        })
    };

    let view = harness
        .view_until(|view| {
            shows_page(view) && view.active_tab().is_some_and(|tab| tab.title == "solid")
        })
        .await;
    let buffer = compose(view, &layout, text.as_ref());

    let (shown, _) = view.shown_frame(&layout).unwrap();
    assert!(
        shown.width * shown.height >= layout.content.width * layout.content.height * 0.5,
        "the page fills most of the page area: {shown:?} in {:?}",
        layout.content
    );
    for row in 1..10 {
        for column in 1..10 {
            let (x, y) = (
                shown.x + shown.width * f64::from(column) / 10.0,
                shown.y + shown.height * f64::from(row) / 10.0,
            );
            let word = buffer[y as usize * width + x as usize];
            assert!(near(word, SOLID), "{word:06x} at ({x}, {y}) is the page");
        }
    }
    let tab = layout.tab(view.active.unwrap(), view.tabs.len());
    if text.is_some() {
        assert!(dark_pixels(&buffer, width, tab) > 20, "the title is drawn");
    }
    let badge = |buffer: &[u32]| {
        let (x, y) = (
            layout.agent_badge.x + 4.0,
            layout.agent_badge.y + layout.agent_badge.height / 2.0,
        );
        buffer[y as usize * width + x as usize]
    };
    assert_eq!(badge(&buffer), BADGE_IDLE);

    // An agent connects as Playwright MCP's --cdp-endpoint mode does.
    let url = format!("ws://{}/devtools/browser/moli-browser", harness.address);
    let stream = tokio::net::TcpStream::connect(harness.address)
        .await
        .unwrap();
    let (mut socket, _) = tokio_tungstenite::client_async(url, stream).await.unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::text(
            json!({"id": 1, "method": "Browser.getVersion"}).to_string(),
        ))
        .await
        .unwrap();
    let view = harness.view_until(|view| view.agent_active).await;
    assert_eq!(badge(&compose(view, &layout, text.as_ref())), BADGE_ACTIVE);
    drop(socket);
    harness.close().await;
}
