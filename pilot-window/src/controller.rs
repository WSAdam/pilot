//! The window's CDP side: follows the agent's tabs, shows the active tab's
//! screencast, and sends a human's navigation and input to it.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::Result;
use moli_protocol_server::{LocalCdpConnector, LocalCdpSender};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::{
    agent::AgentActivity,
    frame::Frame,
    geometry::FrameMetadata,
    tabs::{Tab, Tabs},
};

/// What the window asks of the browser.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    ActivateTab(usize),
    Navigate(String),
    Back,
    Forward,
    Reload,
    /// `Input.dispatchMouseEvent` parameters ([`crate::input`]).
    Mouse(Value),
    /// `Input.dispatchKeyEvent` parameters ([`crate::input`]).
    Key(Value),
    /// The screencast runs only while the window is visible.
    SetVisible(bool),
    /// The largest frame worth sending: the page area in physical pixels.
    SetFrameLimit {
        width: u32,
        height: u32,
    },
    /// The window closed: stop the browser.
    Close,
    /// The window could not open: leave the browser running without it.
    Detach,
}

/// What the window shows.
#[derive(Clone, Debug)]
pub enum Update {
    Tabs {
        tabs: Vec<Tab>,
        active: Option<usize>,
    },
    Frame {
        frame: Frame,
        metadata: FrameMetadata,
    },
    AgentActive(bool),
}

const AGENT_POLL: Duration = Duration::from_millis(250);
const FRAME_QUALITY: u8 = 80;

/// Runs until the browser stops or the window sends [`Command::Close`] (or
/// drops its command sender), which stops the browser.
pub async fn run_controller(
    connector: LocalCdpConnector,
    mut commands: mpsc::UnboundedReceiver<Command>,
    updates: impl FnMut(Update) + Send,
) -> Result<()> {
    let mut client = connector.connect().await?;
    let mut controller = Controller {
        sender: client.sender(),
        next_id: 0,
        pending: HashMap::new(),
        tabs: Tabs::default(),
        session: None,
        attaching: None,
        visible: true,
        frame_limit: None,
        updates,
    };
    controller
        .call(
            "Target.setDiscoverTargets",
            json!({"discover": true}),
            false,
            Pending::Ignore,
        )
        .await;
    let mut activity = AgentActivity::default();
    let mut agent_active = false;
    let mut poll = tokio::time::interval(AGENT_POLL);
    loop {
        tokio::select! {
            message = client.recv() => {
                let Some(message) = message else {
                    tracing::info!("✅ browser stopped; window controller done");
                    return Ok(());
                };
                match serde_json::from_str::<Value>(&message) {
                    Ok(message) => controller.handle_message(message).await,
                    Err(error) => tracing::warn!(%error, "⚠️ window got a non-JSON CDP message"),
                }
            }
            command = commands.recv() => match command {
                None | Some(Command::Close) => {
                    tracing::info!("🚀 window closed; stopping the browser");
                    connector.request_shutdown();
                    return Ok(());
                }
                Some(Command::Detach) => return Ok(()),
                Some(command) => controller.handle_command(command).await,
            },
            _ = poll.tick() => {
                let active = activity.observe(connector.agent_command_count(), Instant::now());
                if active != agent_active {
                    agent_active = active;
                    (controller.updates)(Update::AgentActive(active));
                }
            }
        }
    }
}

enum Pending {
    Attach {
        target_id: String,
    },
    /// `Page.getNavigationHistory`, then go `offset` entries from the current.
    History {
        offset: i64,
    },
    Ignore,
}

struct Session {
    target_id: String,
    session_id: String,
    screencasting: bool,
}

struct Controller<F> {
    sender: LocalCdpSender,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    tabs: Tabs,
    session: Option<Session>,
    attaching: Option<String>,
    visible: bool,
    frame_limit: Option<(u32, u32)>,
    updates: F,
}

impl<F: FnMut(Update)> Controller<F> {
    /// Sends a command, on the active tab's session when `on_session`.
    async fn call(&mut self, method: &str, params: Value, on_session: bool, pending: Pending) {
        let session_id = match (&self.session, on_session) {
            (Some(session), true) => Some(session.session_id.clone()),
            (None, true) => return,
            (_, false) => None,
        };
        self.next_id += 1;
        let mut command = json!({"id": self.next_id, "method": method, "params": params});
        if let Some(session_id) = session_id {
            command["sessionId"] = json!(session_id);
        }
        if !matches!(pending, Pending::Ignore) {
            self.pending.insert(self.next_id, pending);
        }
        if !self.sender.send(command.to_string()).await {
            tracing::warn!(
                method,
                "⚠️ browser is shutting down; window command dropped"
            );
        }
    }

    async fn handle_message(&mut self, message: Value) {
        if let Some(id) = message["id"].as_u64() {
            match self.pending.remove(&id) {
                Some(Pending::Attach { target_id }) => self.attached(target_id, &message).await,
                Some(Pending::History { offset }) => {
                    self.go_to_history_entry(offset, &message).await
                }
                Some(Pending::Ignore) | None => {}
            }
            return;
        }
        match message["method"].as_str() {
            Some("Page.screencastFrame") => self.frame(&message).await,
            Some("Target.detachedFromTarget") => {
                let session_id = message["params"]["sessionId"].as_str();
                if self
                    .session
                    .as_ref()
                    .map(|session| session.session_id.as_str())
                    == session_id
                {
                    self.session = None;
                    self.sync_session().await;
                }
            }
            Some(method) if method.starts_with("Target.") && self.tabs.apply(&message) => {
                self.send_tabs();
                self.sync_session().await;
            }
            _ => {}
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::ActivateTab(index) => {
                if self.tabs.activate(index) {
                    self.send_tabs();
                    self.sync_session().await;
                }
            }
            Command::Navigate(url) => {
                self.call("Page.navigate", json!({"url": url}), true, Pending::Ignore)
                    .await;
            }
            Command::Back | Command::Forward => {
                let offset = if command == Command::Back { -1 } else { 1 };
                self.call(
                    "Page.getNavigationHistory",
                    json!({}),
                    true,
                    Pending::History { offset },
                )
                .await;
            }
            Command::Reload => {
                self.call("Page.reload", json!({}), true, Pending::Ignore)
                    .await;
            }
            Command::Mouse(params) => {
                self.call("Input.dispatchMouseEvent", params, true, Pending::Ignore)
                    .await;
            }
            Command::Key(params) => {
                self.call("Input.dispatchKeyEvent", params, true, Pending::Ignore)
                    .await;
            }
            Command::SetVisible(visible) => {
                if visible != self.visible {
                    self.visible = visible;
                    if visible {
                        self.start_screencast().await;
                    } else {
                        self.stop_screencast().await;
                    }
                }
            }
            Command::SetFrameLimit { width, height } => {
                if self.frame_limit != Some((width, height)) {
                    self.frame_limit = Some((width, height));
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.screencasting)
                    {
                        self.stop_screencast().await;
                        self.start_screencast().await;
                    }
                }
            }
            Command::Close | Command::Detach => {}
        }
    }

    fn send_tabs(&mut self) {
        let update = Update::Tabs {
            tabs: self.tabs.tabs().to_vec(),
            active: self.tabs.active_index(),
        };
        (self.updates)(update);
    }

    /// Attaches to the active tab, leaving any other one.
    async fn sync_session(&mut self) {
        let active = self.tabs.active().map(|tab| tab.target_id.clone());
        if let Some(session) = &self.session
            && Some(&session.target_id) != active.as_ref()
        {
            let session_id = session.session_id.clone();
            self.session = None;
            self.call(
                "Target.detachFromTarget",
                json!({"sessionId": session_id}),
                false,
                Pending::Ignore,
            )
            .await;
        }
        if let Some(target_id) = active
            && self.session.is_none()
            && self.attaching.as_ref() != Some(&target_id)
        {
            self.attaching = Some(target_id.clone());
            self.call(
                "Target.attachToTarget",
                json!({"targetId": target_id, "flatten": true}),
                false,
                Pending::Attach { target_id },
            )
            .await;
        }
    }

    async fn attached(&mut self, target_id: String, reply: &Value) {
        if self.attaching.as_ref() == Some(&target_id) {
            self.attaching = None;
        }
        let Some(session_id) = reply["result"]["sessionId"].as_str() else {
            tracing::warn!(%target_id, error = %reply["error"], "⚠️ window could not attach to the tab");
            return;
        };
        let still_active = self
            .tabs
            .active()
            .is_some_and(|tab| tab.target_id == target_id);
        if !still_active || self.session.is_some() {
            self.call(
                "Target.detachFromTarget",
                json!({"sessionId": session_id}),
                false,
                Pending::Ignore,
            )
            .await;
            self.sync_session().await;
            return;
        }
        tracing::info!(%target_id, "🔍 window watching tab");
        self.session = Some(Session {
            target_id,
            session_id: session_id.to_owned(),
            screencasting: false,
        });
        self.start_screencast().await;
    }

    async fn start_screencast(&mut self) {
        if !self.visible
            || self
                .session
                .as_ref()
                .is_none_or(|session| session.screencasting)
        {
            return;
        }
        let mut params = json!({"format": "jpeg", "quality": FRAME_QUALITY, "everyNthFrame": 1});
        if let Some((width, height)) = self.frame_limit {
            params["maxWidth"] = json!(width);
            params["maxHeight"] = json!(height);
        }
        self.call("Page.startScreencast", params, true, Pending::Ignore)
            .await;
        if let Some(session) = &mut self.session {
            session.screencasting = true;
        }
    }

    async fn stop_screencast(&mut self) {
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.screencasting)
        {
            self.call("Page.stopScreencast", json!({}), true, Pending::Ignore)
                .await;
            if let Some(session) = &mut self.session {
                session.screencasting = false;
            }
        }
    }

    async fn frame(&mut self, message: &Value) {
        let session_id = message["sessionId"].as_str();
        if self
            .session
            .as_ref()
            .map(|session| session.session_id.as_str())
            != session_id
        {
            return;
        }
        let params = &message["params"];
        // Acknowledge first, so the next frame is captured while this one is
        // decoded and drawn.
        self.call(
            "Page.screencastFrameAck",
            json!({"sessionId": params["sessionId"]}),
            true,
            Pending::Ignore,
        )
        .await;
        let (Some(data), Some(metadata)) = (
            params["data"].as_str(),
            FrameMetadata::from_event(&params["metadata"]),
        ) else {
            tracing::warn!("⚠️ screencast frame without data or page size");
            return;
        };
        match Frame::decode(data) {
            Ok(frame) => (self.updates)(Update::Frame { frame, metadata }),
            Err(error) => tracing::warn!(%error, "⚠️ could not decode a screencast frame"),
        }
    }

    async fn go_to_history_entry(&mut self, offset: i64, reply: &Value) {
        let result = &reply["result"];
        let (Some(current), Some(entries)) = (
            result["currentIndex"].as_i64(),
            result["entries"].as_array(),
        ) else {
            return;
        };
        let Some(entry) = usize::try_from(current + offset)
            .ok()
            .and_then(|index| entries.get(index))
        else {
            return;
        };
        self.call(
            "Page.navigateToHistoryEntry",
            json!({"entryId": entry["id"]}),
            true,
            Pending::Ignore,
        )
        .await;
    }
}
