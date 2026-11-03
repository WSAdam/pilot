//! The window controller against a real in-process browser: it follows the
//! agent's page, shows its screencast, and forwards a human's input to it.

use std::{collections::HashMap, sync::Arc, time::Duration};

use moli_core::{
    LayoutPolicy, OptionalResourceFetchMask,
    runtime::{NavigationRuntimeConfig, storage_partition::StoragePartitionState},
};
use moli_fetch::FetchConfig;
use moli_protocol_server::{
    BrowserInstanceControl, LocalCdpClient, LocalCdpConnector, ProtocolServer, ServerConfig,
};
use pilot_window::{
    controller::{Command, Update, run_controller},
    input::{self, Modifiers, MouseButton},
};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(20);

const BUTTON_PAGE: &str = "data:text/html,<title>waiting</title><button style='position:fixed;inset:0;width:100%25;height:100%25' onclick='document.title=%22clicked%22'>take over</button>";

async fn start_browser() -> (
    LocalCdpConnector,
    tokio::task::JoinHandle<anyhow::Result<()>>,
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
    let serving = tokio::spawn(async move {
        server
            .serve_with_control_bound_hook_and_local_clients(
                BrowserInstanceControl::new(),
                |_| {},
                move |connector| {
                    let _ = ready_tx.send(connector);
                },
            )
            .await
    });
    (ready_rx.await.unwrap(), serving)
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

async fn next_update(
    updates: &mut mpsc::UnboundedReceiver<Update>,
    mut wanted: impl FnMut(&Update) -> bool,
) -> Update {
    tokio::time::timeout(WAIT, async {
        loop {
            let update = updates.recv().await.expect("the controller is running");
            if wanted(&update) {
                return update;
            }
        }
    })
    .await
    .expect("the controller sends the update")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_window_follows_the_agent_shows_its_page_and_forwards_a_takeover_click() {
    let (connector, serving) = start_browser().await;
    let mut agent = Agent {
        client: connector.connect().await.unwrap(),
        next_id: 0,
        replies: HashMap::new(),
    };
    let created = agent
        .call("Target.createTarget", json!({"url": BUTTON_PAGE}), None)
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

    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (update_tx, mut updates) = mpsc::unbounded_channel();
    let controller = tokio::spawn(run_controller(
        connector.clone(),
        command_rx,
        move |update| {
            let _ = update_tx.send(update);
        },
    ));

    let Update::Tabs { tabs, active } = next_update(&mut updates, |update| {
        matches!(update, Update::Tabs { tabs, .. } if tabs.iter().any(|tab| tab.target_id == target_id))
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(
        active.map(|index| tabs[index].target_id.as_str()),
        Some(target_id.as_str()),
        "the window shows the page the agent opened"
    );

    let Update::Frame { frame, metadata } = next_update(&mut updates, |update| {
        matches!(update, Update::Frame { .. })
    })
    .await
    else {
        unreachable!()
    };
    assert!(frame.width > 0 && frame.height > 0);
    assert!(metadata.device_width > 0.0 && metadata.device_height > 0.0);
    // Like Chromium, a frame is captured again only when the page changes;
    // it reaches the window because the window acknowledged the last one.
    agent
        .call(
            "Runtime.evaluate",
            json!({"expression": "document.body.style.background = 'red'"}),
            Some(&session),
        )
        .await;
    next_update(&mut updates, |update| {
        matches!(update, Update::Frame { .. })
    })
    .await;

    // A human clicks the middle of the page in the window.
    let (x, y) = (metadata.device_width / 2.0, metadata.device_height / 2.0);
    for pressed in [true, false] {
        command_tx
            .send(Command::Mouse(input::mouse_button(
                pressed,
                x,
                y,
                MouseButton::Left,
                u32::from(pressed),
                1,
                Modifiers::default(),
            )))
            .unwrap();
    }
    let mut title = String::new();
    for _ in 0..50 {
        title = agent
            .call(
                "Runtime.evaluate",
                json!({"expression": "document.title", "returnByValue": true}),
                Some(&session),
            )
            .await["result"]["result"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if title == "clicked" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(title, "clicked", "the takeover click reached the page");

    command_tx.send(Command::Close).unwrap();
    tokio::time::timeout(WAIT, serving)
        .await
        .expect("closing the window stops the browser")
        .unwrap()
        .unwrap();
    tokio::time::timeout(WAIT, controller)
        .await
        .expect("the controller ends with the browser")
        .unwrap()
        .unwrap();
}
