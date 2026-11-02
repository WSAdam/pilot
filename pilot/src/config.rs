//! Moli's configuration for a pilot launch.

use crate::argv::ChromiumLaunch;
use anyhow::{Context, Result};
use clap::Parser;
use moli::config::AppConfig;
use moli_core::LayoutPolicy;

/// Builds the `moli serve` configuration for a Chromium command line.
///
/// pilot always lays pages out, and with the `Live` policy: Playwright reads
/// geometry after the page changed (a button added by a click, a restyle on
/// hover) and must see the current layout, not the first one Moli built.
pub fn moli_config(launch: &ChromiumLaunch) -> Result<AppConfig> {
    let cli = moli::cli::Cli::try_parse_from(launch.moli_serve_args())
        .context("❌ pilot could not translate the Chromium command line for moli")?;
    let mut config =
        AppConfig::from_cli(&cli).context("❌ pilot could not build the moli configuration")?;
    config.browser.set_layout_policy(LayoutPolicy::Live);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_lays_pages_out_with_the_live_policy() {
        for switches in [
            &["--headless", "--remote-debugging-pipe"][..],
            &["--remote-debugging-port=0"],
        ] {
            let launch = ChromiumLaunch::parse(switches.iter().copied());
            let config = moli_config(&launch).unwrap();
            assert_eq!(
                config.browser.layout_policy(),
                LayoutPolicy::Live,
                "{switches:?}"
            );
        }
    }
}
