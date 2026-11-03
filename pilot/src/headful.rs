//! Headful pilot: the browser on a background runtime, the watch-the-agent
//! window on the main thread (which macOS requires for windows).

use anyhow::{Context, Result};
use moli::config::AppConfig;

use crate::argv::ChromiumLaunch;

/// Set to any value to run without a window even when `--headless` is
/// absent, for automated runs that must not open windows on a desktop.
pub const NO_WINDOW_ENV: &str = "PILOT_NO_WINDOW";

/// Whether this launch opens the window: whenever `--headless` is absent,
/// as Chromium opens one, unless [`NO_WINDOW_ENV`] is set.
pub fn wants_window(launch: &ChromiumLaunch, no_window_env: Option<std::ffi::OsString>) -> bool {
    !launch.headless && no_window_env.is_none()
}

/// Runs the browser as a task on `runtime` and the window on this thread
/// until the browser stops, then returns the browser's result. Without a
/// display the browser keeps running headless.
pub fn run(
    launch: ChromiumLaunch,
    config: AppConfig,
    runtime: tokio::runtime::Runtime,
) -> Result<()> {
    let (connector_tx, connector_rx) = std::sync::mpsc::channel();
    // The runtime outlives the window, so the window's controller can see
    // the browser stop and close the window.
    let browser = runtime.spawn(async move {
        crate::serve::run_with_local_clients(&launch, &config, move |connector| {
            let _ = connector_tx.send(connector);
        })
        .await
    });
    if let Ok(connector) = connector_rx.recv()
        && let Err(error) = pilot_window::app::run_window(connector, runtime.handle().clone())
    {
        tracing::warn!(error = %format!("{error:#}"), "⚠️ no window; pilot keeps running headless");
    }
    runtime
        .block_on(browser)
        .context("❌ the browser task panicked")?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(switches: &[&str]) -> ChromiumLaunch {
        ChromiumLaunch::parse(switches.iter().copied())
    }

    #[test]
    fn a_window_opens_unless_headless_or_turned_off() {
        assert!(wants_window(&launch(&["--remote-debugging-pipe"]), None));
        assert!(!wants_window(
            &launch(&["--headless", "--remote-debugging-pipe"]),
            None
        ));
        assert!(!wants_window(
            &launch(&["--remote-debugging-pipe"]),
            Some("1".into())
        ));
    }
}
