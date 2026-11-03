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
    geometry::FrameMetadata,
    input::{self, Key, Modifiers, MouseButton},
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
