use std::{
    io::{self, Read, Write},
    os::fd::OwnedFd,
    pin::Pin,
    task::{Context as TaskContext, Poll, ready},
};

use anyhow::{Context, Result};
use axum::extract::ws::Message;
use moli_protocol::ParsedCdpCommand;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
    ReadBuf,
};

use super::{
    AppState, BrowserInstanceControl, InstanceCommand, LocalCdpConnector, ProtocolServer,
    SHUTDOWN_BUDGET,
};
use crate::{cdp_frontend::CdpFrontendEndpoint, cdp_writer::spawn_socket_sink};

/// Largest inbound CDP message, excluding its NUL terminator. This is the
/// WebSocket transport's inbound limit (axum's default maximum message size).
const MAX_INBOUND_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

struct PipeFd(tokio::io::unix::AsyncFd<std::fs::File>);

impl PipeFd {
    fn new(file: std::fs::File) -> io::Result<Self> {
        socket2::SockRef::from(&file).set_nonblocking(true)?;
        Ok(Self(tokio::io::unix::AsyncFd::new(file)?))
    }
}

impl AsyncRead for PipeFd {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let mut guard = ready!(self.0.poll_read_ready(cx))?;
            match guard.try_io(|fd| fd.get_ref().read(buf.initialize_unfilled())) {
                Ok(Ok(length)) => {
                    buf.advance(length);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }
}

impl AsyncWrite for PipeFd {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = ready!(self.0.poll_write_ready(cx))?;
            match guard.try_io(|fd| fd.get_ref().write(buf)) {
                Ok(result) => return Poll::Ready(result),
                Err(_) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl ProtocolServer {
    /// Serves one browser-level CDP session over the Chromium pipe protocol:
    /// NUL-terminated JSON messages are read from `input` and written to
    /// `output`. No socket is bound. The caller hands over ownership of both
    /// descriptors, which are closed when the session ends.
    pub async fn serve_pipe(&self, input: OwnedFd, output: OwnedFd) -> Result<()> {
        self.serve_pipe_with_local_clients(input, output, |_| {})
            .await
    }

    /// pilot: like [`Self::serve_pipe`], and hands `on_ready` a connector for
    /// in-process CDP clients of this browser.
    pub async fn serve_pipe_with_local_clients(
        &self,
        input: OwnedFd,
        output: OwnedFd,
        on_ready: impl FnOnce(LocalCdpConnector),
    ) -> Result<()> {
        self.serve_pipe_io_with_local_clients(
            PipeFd::new(std::fs::File::from(input))?,
            PipeFd::new(std::fs::File::from(output))?,
            on_ready,
        )
        .await
    }

    /// pilot: serves one browser over both the Chromium pipe and a TCP port,
    /// as Chromium does when launched with `--remote-debugging-pipe` and
    /// `--remote-debugging-port`. Both transports reach the same pages;
    /// `Browser.close` or EOF on the pipe, or `Browser.close` over the port,
    /// ends the instance for both.
    pub async fn serve_pipe_and_port_with_local_clients(
        &self,
        input: OwnedFd,
        output: OwnedFd,
        on_bound: impl FnOnce(std::net::SocketAddr),
        on_ready: impl FnOnce(LocalCdpConnector),
    ) -> Result<()> {
        self.serve_pipe_io_and_port_with_local_clients(
            PipeFd::new(std::fs::File::from(input))?,
            PipeFd::new(std::fs::File::from(output))?,
            on_bound,
            on_ready,
        )
        .await
    }

    async fn serve_pipe_io_and_port_with_local_clients(
        &self,
        input: impl AsyncRead + Unpin,
        output: impl AsyncWrite + Unpin + Send + 'static,
        on_bound: impl FnOnce(std::net::SocketAddr),
        on_ready: impl FnOnce(LocalCdpConnector),
    ) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(self.config.bind_target())
            .await
            .with_context(|| {
                format!(
                    "failed to bind protocol server to {}:{}",
                    self.config.host, self.config.port
                )
            })?;
        let addr = listener
            .local_addr()
            .context("failed to read bound protocol socket address")?;
        tracing::info!(%addr, "🚀 protocol server listening beside the CDP pipe");
        on_bound(addr);

        let control = BrowserInstanceControl::new();
        let app_state = AppState::new_with_storage_partition_and_runtime_config(
            addr,
            self.storage_partition.clone(),
            self.navigation_runtime_config.clone(),
            self.config.screencast_interval_ms,
            control.clone(),
        )?;
        let endpoint = app_state.cdp_owner_registry.shared_owner()?;
        on_ready(LocalCdpConnector::new(
            app_state.cdp_owner_registry.clone(),
            control.clone(),
        ));
        // The listener owns shutdown (CDP owner, Classic sessions, storage
        // flush); the pipe only asks for it when its client goes away.
        let pipe = async {
            let result = run_pipe(input, output, endpoint, control.clone()).await;
            control.request(InstanceCommand::Shutdown);
            result
        };
        let (served, piped) = tokio::join!(super::serve_listener(listener, app_state), pipe);
        match (piped, served) {
            (Err(error), Err(served)) => {
                Err(error.context(format!("port also failed: {served:#}")))
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[cfg(test)]
    async fn serve_pipe_io(
        &self,
        input: impl AsyncRead + Unpin,
        output: impl AsyncWrite + Unpin + Send + 'static,
    ) -> Result<()> {
        self.serve_pipe_io_with_local_clients(input, output, |_| {})
            .await
    }

    async fn serve_pipe_io_with_local_clients(
        &self,
        input: impl AsyncRead + Unpin,
        output: impl AsyncWrite + Unpin + Send + 'static,
        on_ready: impl FnOnce(LocalCdpConnector),
    ) -> Result<()> {
        let control = BrowserInstanceControl::new();
        let state = AppState::new_with_storage_partition_and_runtime_config(
            ([127, 0, 0, 1], 0).into(),
            self.storage_partition.clone(),
            self.navigation_runtime_config.clone(),
            self.config.screencast_interval_ms,
            control.clone(),
        )?;
        let endpoint = state.cdp_owner_registry.shared_owner()?;
        on_ready(LocalCdpConnector::new(
            state.cdp_owner_registry.clone(),
            control.clone(),
        ));
        let result = run_pipe(input, output, endpoint, control.clone()).await;
        control.request(InstanceCommand::Shutdown);
        // Like `serve_listener`: stop the CDP owner, then flush storage and
        // report a failed flush instead of leaving it to `Drop`.
        let storage_partition = state.storage_partition.clone();
        let stopped = tokio::time::timeout(SHUTDOWN_BUDGET, async {
            state.cdp_owner_registry.shutdown().await;
            control.drained().await;
            tokio::task::spawn_blocking(move || storage_partition.shutdown())
                .await
                .context("storage shutdown worker stopped")?
        })
        .await
        .context("protocol pipe shutdown timed out; storage may be incomplete")
        .and_then(|stopped| stopped);
        match (result, stopped) {
            (Err(error), Err(stop)) => {
                Err(error.context(format!("shutdown also failed: {stop:#}")))
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(()), stopped) => stopped,
        }
    }
}

async fn run_pipe(
    input: impl AsyncRead + Unpin,
    output: impl AsyncWrite + Unpin + Send + 'static,
    endpoint: CdpFrontendEndpoint,
    control: BrowserInstanceControl,
) -> Result<()> {
    let writer = futures_util::sink::unfold(output, |mut output, message| async move {
        if let Message::Text(text) = message {
            output.write_all(text.as_bytes()).await?;
            output.write_all(&[0]).await?;
            output.flush().await?;
        }
        Ok::<_, std::io::Error>(output)
    });
    let (sink, mut finished) = spawn_socket_sink(Box::pin(writer));
    let frontend_id = endpoint.attach_browser(sink.clone()).await?;
    let mut reader = BufReader::new(input);
    let result = async {
        let mut frame = Vec::new();
        loop {
            let finished_frame = tokio::select! {
                _ = control.shutdown_requested() => break,
                _ = endpoint.wait_for_shutdown() => break,
                _ = &mut finished => break,
                result = read_frame(&mut reader, &mut frame, MAX_INBOUND_MESSAGE_BYTES) => result?,
            };
            if !finished_frame {
                break;
            }
            let text = String::from_utf8(frame.clone()).context("CDP pipe message is not UTF-8")?;
            control.record_agent_command();
            if !endpoint
                .command(frontend_id, ParsedCdpCommand::parse_str(text))
                .await
            {
                break;
            }
        }
        Ok(())
    }
    .await;
    endpoint.detach_browser(frontend_id);
    sink.close();
    result
}

/// Reads one NUL-terminated message into `frame` (terminator removed). Returns
/// `false` at a clean EOF between messages. A message longer than `max` bytes
/// is rejected as soon as it exceeds the limit, before it is buffered in full.
async fn read_frame(
    reader: &mut (impl AsyncBufRead + Unpin),
    frame: &mut Vec<u8>,
    max: usize,
) -> Result<bool> {
    frame.clear();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let count = reader
        .take(limit)
        .read_until(0, frame)
        .await
        .context("CDP pipe read failed")?;
    if count == 0 {
        return Ok(false);
    }
    if frame.last() != Some(&0) {
        anyhow::ensure!(frame.len() <= max, "CDP pipe message exceeds {max} bytes");
        anyhow::bail!("unterminated CDP pipe message");
    }
    frame.pop();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerConfig;
    use serde_json::Value;
    use tokio::io::AsyncReadExt;

    async fn exercise_pipe(close_command: bool) {
        let (client_input, server_input) = std::os::unix::net::UnixStream::pair().unwrap();
        let (server_output, client_output) = std::os::unix::net::UnixStream::pair().unwrap();
        let server_input = PipeFd::new(std::fs::File::from(std::os::fd::OwnedFd::from(
            server_input,
        )))
        .unwrap();
        let server_output = PipeFd::new(std::fs::File::from(std::os::fd::OwnedFd::from(
            server_output,
        )))
        .unwrap();
        client_input.set_nonblocking(true).unwrap();
        client_output.set_nonblocking(true).unwrap();
        let client_input = tokio::net::UnixStream::from_std(client_input).unwrap();
        let client_output = tokio::net::UnixStream::from_std(client_output).unwrap();
        let server = ProtocolServer::new(ServerConfig::default());
        let serving =
            tokio::spawn(async move { server.serve_pipe_io(server_input, server_output).await });
        let mut input = client_input;
        let mut output = BufReader::new(client_output);
        input
            .write_all(b"{\"id\":1,\"method\":\"Browser.")
            .await
            .unwrap();
        input.write_all(b"getVersion\"}\0{\"id\":2,\"method\":\"Target.createTarget\",\"params\":{\"url\":\"about:blank\"}}\0").await.unwrap();
        let mut replies = Vec::new();
        while replies.len() < 2 {
            let mut frame = Vec::new();
            output.read_until(0, &mut frame).await.unwrap();
            assert_eq!(frame.pop(), Some(0));
            let response: Value = serde_json::from_slice(&frame).unwrap();
            if response.get("id").is_some() {
                replies.push(response);
            }
        }
        assert!(
            replies
                .iter()
                .any(|r| r["id"] == 1 && r["result"]["product"].is_string())
        );
        assert!(
            replies
                .iter()
                .any(|r| r["id"] == 2 && r["result"]["targetId"].is_string())
        );
        if close_command {
            input
                .write_all(b"{\"id\":3,\"method\":\"Browser.close\"}\0")
                .await
                .unwrap();
        } else {
            input.shutdown().await.unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let mut tail = Vec::new();
        output.read_to_end(&mut tail).await.unwrap();
    }
    async fn frame_result(input: &[u8], max: usize) -> Result<Option<Vec<u8>>> {
        let mut reader = BufReader::new(input);
        let mut frame = Vec::new();
        Ok(read_frame(&mut reader, &mut frame, max)
            .await?
            .then_some(frame))
    }

    #[tokio::test]
    async fn read_frame_enforces_the_inbound_limit_before_buffering_the_message() {
        assert_eq!(
            frame_result(b"{}\0", 2).await.unwrap(),
            Some(b"{}".to_vec())
        );
        assert_eq!(frame_result(b"", 2).await.unwrap(), None);
        let oversize = frame_result(&[b'a'; 4096], 2).await.unwrap_err();
        assert!(
            oversize.to_string().contains("exceeds 2 bytes"),
            "{oversize:#}"
        );
        let oversize_terminated = frame_result(b"abc\0", 2).await.unwrap_err();
        assert!(
            oversize_terminated.to_string().contains("exceeds 2 bytes"),
            "{oversize_terminated:#}"
        );
        let unterminated = frame_result(b"ab", 2).await.unwrap_err();
        assert!(
            unterminated.to_string().contains("unterminated"),
            "{unterminated:#}"
        );
    }

    #[tokio::test]
    async fn pipe_routes_fragmented_commands_and_stops_on_eof() {
        exercise_pipe(false).await;
    }

    #[tokio::test]
    async fn pipe_browser_close_stops_with_input_still_open() {
        exercise_pipe(true).await;
    }

    async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    // 2026-11-10 (pilot): given --remote-debugging-pipe and
    // --remote-debugging-port, pilot served only the pipe and never announced
    // the port, so Playwright's launch hung waiting for "DevTools listening on"
    // (Playwright MCP http.spec relaunches a crashed shared browser).
    #[tokio::test]
    async fn pipe_and_port_serve_one_browser_and_close_together() {
        let (client_input, server_input) = std::os::unix::net::UnixStream::pair().unwrap();
        let (server_output, client_output) = std::os::unix::net::UnixStream::pair().unwrap();
        let server_input = PipeFd::new(std::fs::File::from(std::os::fd::OwnedFd::from(
            server_input,
        )))
        .unwrap();
        let server_output = PipeFd::new(std::fs::File::from(std::os::fd::OwnedFd::from(
            server_output,
        )))
        .unwrap();
        client_input.set_nonblocking(true).unwrap();
        client_output.set_nonblocking(true).unwrap();
        let mut input = tokio::net::UnixStream::from_std(client_input).unwrap();
        let mut output = BufReader::new(tokio::net::UnixStream::from_std(client_output).unwrap());
        let server = ProtocolServer::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            port: 0,
            ..ServerConfig::default()
        });
        let (bound_tx, bound_rx) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(async move {
            server
                .serve_pipe_io_and_port_with_local_clients(
                    server_input,
                    server_output,
                    move |addr| {
                        let _ = bound_tx.send(addr);
                    },
                    |_| {},
                )
                .await
        });
        let addr = bound_rx
            .await
            .expect("the port should be bound and announced");

        input
            .write_all(b"{\"id\":1,\"method\":\"Target.createTarget\",\"params\":{\"url\":\"about:blank\"}}\0")
            .await
            .unwrap();
        let target_id = loop {
            let mut frame = Vec::new();
            output.read_until(0, &mut frame).await.unwrap();
            assert_eq!(frame.pop(), Some(0));
            let response: Value = serde_json::from_slice(&frame).unwrap();
            if response["id"] == 1 {
                break response["result"]["targetId"]
                    .as_str()
                    .expect("createTarget over the pipe returns a target id")
                    .to_owned();
            }
        };

        let version = http_get(addr, "/json/version").await;
        assert!(version.starts_with("HTTP/1.1 200"), "{version}");
        assert!(version.contains("webSocketDebuggerUrl"), "{version}");
        let list = http_get(addr, "/json/list").await;
        assert!(
            list.contains(&target_id),
            "the port lists the page the pipe created (one browser): {list}"
        );

        input.shutdown().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), serving)
            .await
            .expect("EOF on the pipe ends the port too")
            .unwrap()
            .unwrap();
        assert!(
            tokio::net::TcpStream::connect(addr).await.is_err(),
            "the port stops accepting once the pipe ends"
        );
    }
}
