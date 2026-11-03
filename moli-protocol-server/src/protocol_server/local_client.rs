//! pilot: in-process CDP clients of the browser a [`super::ProtocolServer`]
//! serves.
//!
//! pilot's headful window watches the page the agent drives. It is a second
//! browser-level CDP client, like a websocket client of `/devtools/browser`,
//! but it lives in the same process and speaks CDP over channels, so it needs
//! no socket even when the agent uses `--remote-debugging-pipe`.

use anyhow::Result;
use axum::extract::ws::Message;
use moli_protocol::ParsedCdpCommand;
use tokio::sync::mpsc;

use super::{BrowserInstanceControl, InstanceCommand, cdp_owner::SharedCdpOwnerRegistry};
use crate::{
    cdp_frontend::CdpFrontendEndpoint,
    cdp_writer::{CdpSocketSink, spawn_socket_sink},
};

/// Opens in-process CDP connections to one running browser.
#[derive(Clone)]
pub struct LocalCdpConnector {
    owners: SharedCdpOwnerRegistry,
    control: BrowserInstanceControl,
}

impl LocalCdpConnector {
    pub(super) fn new(owners: SharedCdpOwnerRegistry, control: BrowserInstanceControl) -> Self {
        Self { owners, control }
    }

    /// Attaches a new browser-level frontend.
    pub async fn connect(&self) -> Result<LocalCdpClient> {
        let endpoint = self.owners.shared_owner()?;
        let (message_tx, messages) = mpsc::unbounded_channel();
        let writer = futures_util::sink::unfold(message_tx, |message_tx, message| async move {
            if let Message::Text(text) = message {
                let _ = message_tx.send(text.to_string());
            }
            Ok::<_, std::io::Error>(message_tx)
        });
        let (sink, _finished) = spawn_socket_sink(Box::pin(writer));
        let frontend_id = endpoint.attach_browser(sink.clone()).await?;
        Ok(LocalCdpClient {
            sender: LocalCdpSender {
                endpoint,
                frontend_id,
            },
            sink,
            messages,
        })
    }

    /// Commands the browser has received from its other clients (the agent's
    /// pipe or websocket), never from local clients.
    pub fn agent_command_count(&self) -> u64 {
        self.control.agent_command_count()
    }

    /// Stops the browser, as `Browser.close` does.
    pub fn request_shutdown(&self) {
        self.control.request(InstanceCommand::Shutdown);
    }
}

/// One in-process CDP connection. Dropping it detaches the frontend.
pub struct LocalCdpClient {
    sender: LocalCdpSender,
    sink: CdpSocketSink,
    messages: mpsc::UnboundedReceiver<String>,
}

impl LocalCdpClient {
    pub fn sender(&self) -> LocalCdpSender {
        self.sender.clone()
    }

    /// The next reply or event, as CDP JSON text; `None` once the browser is
    /// gone.
    pub async fn recv(&mut self) -> Option<String> {
        self.messages.recv().await
    }
}

impl Drop for LocalCdpClient {
    fn drop(&mut self) {
        self.sender.endpoint.detach_browser(self.sender.frontend_id);
        self.sink.close();
    }
}

/// Sends commands on a [`LocalCdpClient`]'s connection.
#[derive(Clone)]
pub struct LocalCdpSender {
    endpoint: CdpFrontendEndpoint,
    frontend_id: u64,
}

impl LocalCdpSender {
    /// Sends one CDP command (JSON text). Returns `false` once the browser is
    /// shutting down.
    pub async fn send(&self, command: String) -> bool {
        self.endpoint
            .command(self.frontend_id, ParsedCdpCommand::parse_str(command))
            .await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{os::fd::OwnedFd, time::Duration};

    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixStream,
        task::JoinHandle,
    };

    use super::*;
    use crate::{ProtocolServer, ServerConfig};

    const WAIT: Duration = Duration::from_secs(10);

    /// The agent: a CDP client on Chromium's pipe, as Playwright launches it.
    struct PipeAgent {
        input: UnixStream,
        output: BufReader<UnixStream>,
    }

    impl PipeAgent {
        async fn send(&mut self, command: Value) {
            self.input
                .write_all(format!("{command}\0").as_bytes())
                .await
                .unwrap();
        }

        async fn reply(&mut self, id: u64) -> Value {
            tokio::time::timeout(WAIT, async {
                loop {
                    let mut frame = Vec::new();
                    self.output.read_until(0, &mut frame).await.unwrap();
                    frame.pop();
                    let message: Value = serde_json::from_slice(&frame).unwrap();
                    if message["id"] == id {
                        return message;
                    }
                }
            })
            .await
            .expect("the agent's reply arrives")
        }
    }

    fn pipe_pair() -> (OwnedFd, UnixStream) {
        let (server_end, client_end) = std::os::unix::net::UnixStream::pair().unwrap();
        client_end.set_nonblocking(true).unwrap();
        (
            OwnedFd::from(server_end),
            UnixStream::from_std(client_end).unwrap(),
        )
    }

    async fn serve_with_pipe_agent() -> (PipeAgent, LocalCdpConnector, JoinHandle<Result<()>>) {
        let (server_input, agent_input) = pipe_pair();
        let (server_output, agent_output) = pipe_pair();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = ProtocolServer::new(ServerConfig::default());
        let serving = tokio::spawn(async move {
            server
                .serve_pipe_with_local_clients(server_input, server_output, move |connector| {
                    let _ = ready_tx.send(connector);
                })
                .await
        });
        let connector = tokio::time::timeout(WAIT, ready_rx).await.unwrap().unwrap();
        let agent = PipeAgent {
            input: agent_input,
            output: BufReader::new(agent_output),
        };
        (agent, connector, serving)
    }

    async fn next_matching(local: &mut LocalCdpClient, matches: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(WAIT, async {
            loop {
                let text = local.recv().await.expect("the browser is still running");
                let message: Value = serde_json::from_str(&text).unwrap();
                if matches(&message) {
                    return message;
                }
            }
        })
        .await
        .expect("the local client sees the message")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_local_client_watches_the_browser_the_pipe_agent_drives() {
        let (mut agent, connector, serving) = serve_with_pipe_agent().await;
        let mut local = connector.connect().await.unwrap();
        let sender = local.sender();
        assert!(
            sender
                .send(
                    json!({"id": 1, "method": "Target.setDiscoverTargets", "params": {"discover": true}})
                        .to_string()
                )
                .await
        );
        next_matching(&mut local, |message| message["id"] == 1).await;

        // Both clients use id 1: each reply reaches only its own client.
        agent
            .send(
                json!({"id": 1, "method": "Target.createTarget", "params": {"url": "about:blank"}}),
            )
            .await;
        let created = agent.reply(1).await;
        let target_id = created["result"]["targetId"]
            .as_str()
            .expect("the agent gets its own createTarget reply")
            .to_owned();
        let event = next_matching(&mut local, |message| {
            message["method"] == "Target.targetCreated"
                && message["params"]["targetInfo"]["targetId"] == target_id.as_str()
        })
        .await;
        assert_eq!(event["params"]["targetInfo"]["type"], "page");
        assert_eq!(
            connector.agent_command_count(),
            1,
            "the agent's commands count, the local client's do not"
        );

        drop(local);
        let mut watcher = connector.connect().await.unwrap();
        agent
            .send(json!({"id": 2, "method": "Browser.getVersion"}))
            .await;
        assert!(
            agent.reply(2).await["result"]["product"].is_string(),
            "the browser keeps serving the agent after the local client leaves"
        );

        connector.request_shutdown();
        tokio::time::timeout(WAIT, serving)
            .await
            .expect("a shutdown request stops the browser")
            .unwrap()
            .unwrap();
        // pilot's window quits when its client's stream ends.
        tokio::time::timeout(WAIT, async { while watcher.recv().await.is_some() {} })
            .await
            .expect("a local client sees the browser stop");
    }
}
