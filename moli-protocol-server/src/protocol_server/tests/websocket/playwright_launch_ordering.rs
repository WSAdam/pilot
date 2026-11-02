use super::*;

/// The per-page setup Playwright 1.65 pipelines when it attaches to a page
/// (`FrameSession._initialize`), in the order it sends it.
const FRAME_SESSION_SETUP: [(u64, &str); 4] = [
    (10, "Page.enable"),
    (11, "Page.getFrameTree"),
    (12, "Log.enable"),
    (13, "Page.setLifecycleEventsEnabled"),
];

// 2026-11-02: launchPersistentContext attaches to the start page, pipelines
// the setup above, ignores lifecycle events for frames it has not yet learned
// from Page.getFrameTree, then waits for `load`. Moli answered
// Page.setLifecycleEventsEnabled, and replayed DOMContentLoaded/load, before
// the earlier Page.getFrameTree, so a persistent launch never finished.
#[tokio::test]
async fn websocket_cdp_lifecycle_replay_follows_the_earlier_frame_tree_reply() {
    let (cdp_addr, protocol_server) = spawn_test_protocol_server().await;
    let (mut socket, _) = connect_async(format!(
        "ws://{cdp_addr}/devtools/browser/{DEFAULT_BROWSER_ID}"
    ))
    .await
    .expect("connect to cdp websocket");
    socket
        .send(WsMessage::Text(
            json!({
                "id": 1_u64,
                "method": "Target.setAutoAttach",
                "params": {"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true},
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send Target.setAutoAttach");
    let attached = recv_until_match(&mut socket, |message| {
        message["method"] == json!("Target.attachedToTarget")
            && message["params"]["targetInfo"]["type"] == json!("page")
    })
    .await;
    let session_id = attached
        .last()
        .and_then(|message| message["params"]["sessionId"].as_str())
        .expect("the start page is auto-attached with a session")
        .to_owned();

    for (id, method) in FRAME_SESSION_SETUP {
        let params = if method == "Page.setLifecycleEventsEnabled" {
            json!({"enabled": true})
        } else {
            json!({})
        };
        socket
            .send(WsMessage::Text(
                json!({"id": id, "method": method, "sessionId": session_id, "params": params})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("send frame session setup command");
    }

    let mut lifecycle_replied = false;
    let mut load_replayed = false;
    let messages = recv_until_match(&mut socket, |message| {
        lifecycle_replied |= message["id"] == json!(13_u64);
        load_replayed |= message["sessionId"].as_str() == Some(session_id.as_str())
            && message["method"] == json!("Page.lifecycleEvent")
            && message["params"]["name"] == json!("load");
        lifecycle_replied && load_replayed
    })
    .await;
    let frame_tree_reply = messages
        .iter()
        .position(|message| message["id"] == json!(11_u64))
        .unwrap_or_else(|| {
            panic!("Page.getFrameTree must reply before the lifecycle replay: {messages:#?}")
        });
    let first_lifecycle_event = messages
        .iter()
        .position(|message| {
            message["sessionId"].as_str() == Some(session_id.as_str())
                && message["method"] == json!("Page.lifecycleEvent")
        })
        .expect("the lifecycle replay was received");
    assert!(
        frame_tree_reply < first_lifecycle_event,
        "Page.getFrameTree must reply before any lifecycle event it describes: {messages:#?}"
    );

    let _ = socket.close(None).await;
    protocol_server.abort();
}
