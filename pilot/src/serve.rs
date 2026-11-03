//! Runs Moli's protocol server over the transport a Chromium command line
//! asked for, announcing a port endpoint the way Chromium does.

use std::{net::SocketAddr, path::Path, sync::Arc};

use anyhow::{Context, Result};
use moli::config::AppConfig;
use moli_core::runtime::{NavigationRuntimeConfig, storage_partition::StoragePartitionState};
use moli_protocol_server::{BrowserInstanceControl, LocalCdpConnector, ProtocolServer};

use crate::argv::{ChromiumLaunch, DebuggingTransport};

/// Path of the browser-level CDP endpoint Moli serves.
pub const BROWSER_ENDPOINT_PATH: &str = "/devtools/browser/moli-browser";

pub async fn run(launch: &ChromiumLaunch, config: &AppConfig) -> Result<()> {
    run_with_local_clients(launch, config, |_| {}).await
}

/// Like [`run`], and hands `on_ready` a connector for in-process CDP
/// clients of the browser (the headful window).
pub async fn run_with_local_clients(
    launch: &ChromiumLaunch,
    config: &AppConfig,
    on_ready: impl FnOnce(LocalCdpConnector),
) -> Result<()> {
    let storage_partition = Arc::new(StoragePartitionState::open(config.browser.profile_dir())?);
    let server = ProtocolServer::new_with_storage_partition_and_runtime_config(
        config.server.clone(),
        storage_partition,
        NavigationRuntimeConfig::from(&config.browser),
    );
    match &launch.transport {
        DebuggingTransport::Pipe => serve_pipe(&server, on_ready).await,
        DebuggingTransport::Port { .. } => {
            let user_data_dir = launch.user_data_dir.clone();
            server
                .serve_with_control_bound_hook_and_local_clients(
                    BrowserInstanceControl::new(),
                    move |address| announce(address, user_data_dir.as_deref()),
                    on_ready,
                )
                .await
                .context("protocol server failed")
        }
    }
}

/// Chromium's announcement: a `DevTools listening on` line on stderr, which
/// Puppeteer and Playwright wait for, and `DevToolsActivePort` in the profile.
fn announce(address: SocketAddr, user_data_dir: Option<&Path>) {
    eprintln!("DevTools listening on ws://{address}{BROWSER_ENDPOINT_PATH}");
    tracing::info!(%address, "🚀 pilot serving CDP");
    #[cfg(unix)]
    if let Some(dir) = user_data_dir {
        let contents =
            crate::profile::devtools_active_port_contents(address.port(), BROWSER_ENDPOINT_PATH);
        if let Err(error) = std::fs::write(dir.join(crate::profile::DEVTOOLS_ACTIVE_PORT), contents)
        {
            tracing::warn!(%error, "⚠️ could not write DevToolsActivePort");
        }
    }
}

#[cfg(unix)]
async fn serve_pipe(
    server: &ProtocolServer,
    on_ready: impl FnOnce(LocalCdpConnector),
) -> Result<()> {
    let (input, output) = take_inherited_pipe_descriptors()?;
    server
        .serve_pipe_with_local_clients(input, output, on_ready)
        .await
        .context("protocol pipe failed")
}

#[cfg(not(unix))]
async fn serve_pipe(
    _server: &ProtocolServer,
    _on_ready: impl FnOnce(LocalCdpConnector),
) -> Result<()> {
    anyhow::bail!("--remote-debugging-pipe is only supported on Unix")
}

/// Claims descriptors 3 and 4, which Chromium's pipe protocol makes the parent
/// pass to the child, as the CDP input and output. (Same check as
/// `moli serve --remote-debugging-pipe`.)
#[cfg(unix)]
fn take_inherited_pipe_descriptors() -> Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::{FromRawFd, OwnedFd};

    for fd in [3, 4] {
        // SAFETY: `fstat` only writes to the zeroed buffer it is given.
        let mut status: libc::stat = unsafe { std::mem::zeroed() };
        let kind =
            (unsafe { libc::fstat(fd, &mut status) } == 0).then_some(status.st_mode & libc::S_IFMT);
        anyhow::ensure!(
            matches!(kind, Some(libc::S_IFIFO | libc::S_IFSOCK)),
            "--remote-debugging-pipe needs the parent to pass pipes as file descriptors 3 and 4"
        );
    }
    // SAFETY: both descriptors are open pipes or sockets (checked above) and
    // this runs once per process, so each gets exactly one owner.
    Ok(unsafe { (OwnedFd::from_raw_fd(3), OwnedFd::from_raw_fd(4)) })
}
