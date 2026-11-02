//! Chromium command-line parsing for the switches Playwright passes.

use std::{ffi::OsString, path::PathBuf};

/// Address used when `--remote-debugging-address` is absent, as in Chromium.
pub const DEFAULT_DEBUGGING_ADDRESS: &str = "127.0.0.1";
/// Port used when no debugging transport is requested, matching `moli serve`.
pub const DEFAULT_DEBUGGING_PORT: u16 = 9222;

/// Where the browser serves CDP.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebuggingTransport {
    /// `--remote-debugging-pipe`: commands on file descriptor 3, replies on 4.
    Pipe,
    /// `--remote-debugging-port` (0 picks a free port), bound to
    /// `--remote-debugging-address`.
    Port { address: String, port: u16 },
}

/// The parts of a Chromium command line that pilot acts on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChromiumLaunch {
    pub headless: bool,
    pub user_data_dir: Option<PathBuf>,
    pub transport: DebuggingTransport,
    /// First positional argument; Playwright passes `about:blank` for a
    /// persistent context.
    pub start_url: Option<String>,
    pub no_startup_window: bool,
    /// Switches pilot accepts but does not act on, kept for diagnostics.
    pub ignored_switches: Vec<String>,
}

impl ChromiumLaunch {
    /// Parses a command line without the program name. Unknown switches are
    /// ignored, never rejected: Playwright passes many Chromium-only switches.
    pub fn parse<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut headless = false;
        let mut user_data_dir = None;
        let mut pipe = false;
        let mut port = None;
        let mut address = None;
        let mut positional = Vec::new();
        let mut no_startup_window = false;
        let mut ignored_switches = Vec::new();
        let mut switches_ended = false;

        for arg in args {
            let arg = arg.into().to_string_lossy().into_owned();
            if switches_ended || !arg.starts_with('-') || arg == "-" {
                positional.push(arg);
                continue;
            }
            if arg == "--" {
                switches_ended = true;
                continue;
            }
            let (name, value) = match arg.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (arg.as_str(), None),
            };
            // Chromium accepts both `--switch` and `-switch` on POSIX.
            match name.trim_start_matches('-') {
                "headless" => headless = true,
                "user-data-dir" => user_data_dir = value.map(PathBuf::from),
                "remote-debugging-pipe" => pipe = true,
                // Chromium ignores a port it cannot parse; so does pilot.
                "remote-debugging-port" => port = value.and_then(|value| value.parse().ok()),
                "remote-debugging-address" => address = value.map(str::to_owned),
                "no-startup-window" => no_startup_window = true,
                _ => ignored_switches.push(name.to_owned()),
            }
        }

        let transport = if pipe {
            DebuggingTransport::Pipe
        } else {
            DebuggingTransport::Port {
                address: address.unwrap_or_else(|| DEFAULT_DEBUGGING_ADDRESS.to_owned()),
                port: port.unwrap_or(DEFAULT_DEBUGGING_PORT),
            }
        };
        Self {
            headless,
            user_data_dir,
            transport,
            start_url: positional.into_iter().next(),
            no_startup_window,
            ignored_switches,
        }
    }

    /// The `moli serve` command line (including the program name) that runs
    /// this launch. Real layout is always on: Playwright's actionability
    /// checks and coordinate input need real geometry.
    pub fn moli_serve_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["moli".into(), "serve".into(), "--layout".into()];
        if let Some(user_data_dir) = &self.user_data_dir {
            args.push("--profile-dir".into());
            args.push(user_data_dir.into());
        }
        match &self.transport {
            DebuggingTransport::Pipe => args.push("--remote-debugging-pipe".into()),
            DebuggingTransport::Port { address, port } => {
                args.push("--host".into());
                args.push(address.into());
                args.push("--port".into());
                args.push(port.to_string().into());
            }
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use moli::{
        cli::{Cli, Commands},
        config::AppConfig,
    };

    const MCP_DEFAULT: &str = include_str!("../tests/fixtures/playwright-1.65/mcp-default.argv");
    const LAUNCH_HEADLESS: &str =
        include_str!("../tests/fixtures/playwright-1.65/launch-headless.argv");
    const PERSISTENT_HEADLESS: &str =
        include_str!("../tests/fixtures/playwright-1.65/persistent-headless.argv");

    fn fixture_args(fixture: &str) -> Vec<&str> {
        fixture.lines().filter(|line| !line.is_empty()).collect()
    }

    fn port(address: &str, port: u16) -> DebuggingTransport {
        DebuggingTransport::Port {
            address: address.to_owned(),
            port,
        }
    }

    #[test]
    fn playwright_mcp_default_launch_is_a_headed_pipe_session_with_a_profile() {
        let launch = ChromiumLaunch::parse(fixture_args(MCP_DEFAULT));

        assert!(!launch.headless);
        assert_eq!(launch.transport, DebuggingTransport::Pipe);
        assert_eq!(launch.user_data_dir, Some(PathBuf::from("{USER_DATA_DIR}")));
        assert_eq!(launch.start_url.as_deref(), Some("about:blank"));
        assert!(!launch.no_startup_window);
        assert!(
            launch
                .ignored_switches
                .iter()
                .any(|switch| switch == "--disable-blink-features"),
            "MCP's automation switch is accepted and ignored: {:?}",
            launch.ignored_switches
        );
    }

    #[test]
    fn playwright_headless_launch_opens_no_start_page() {
        let launch = ChromiumLaunch::parse(fixture_args(LAUNCH_HEADLESS));

        assert!(launch.headless);
        assert_eq!(launch.transport, DebuggingTransport::Pipe);
        assert!(launch.no_startup_window);
        assert_eq!(launch.start_url, None);
    }

    #[test]
    fn playwright_persistent_headless_launch_starts_on_about_blank() {
        let launch = ChromiumLaunch::parse(fixture_args(PERSISTENT_HEADLESS));

        assert!(launch.headless);
        assert_eq!(launch.start_url.as_deref(), Some("about:blank"));
        assert!(!launch.no_startup_window);
    }

    #[test]
    fn headless_accepts_chromium_mode_values() {
        for switch in ["--headless", "--headless=new", "--headless=old"] {
            assert!(ChromiumLaunch::parse([switch]).headless, "{switch}");
        }
    }

    #[test]
    fn unknown_switches_are_ignored_not_rejected() {
        let launch = ChromiumLaunch::parse([
            "--some-future-switch=1",
            "--enable-features=Foo,Bar",
            "-single-dash-switch",
            "--headless",
            "about:blank",
        ]);

        assert!(launch.headless);
        assert_eq!(launch.start_url.as_deref(), Some("about:blank"));
        assert_eq!(
            launch.ignored_switches,
            [
                "--some-future-switch",
                "--enable-features",
                "-single-dash-switch"
            ]
        );
    }

    #[test]
    fn remote_debugging_port_and_address_select_a_listener() {
        let launch = ChromiumLaunch::parse([
            "--remote-debugging-port=0",
            "--remote-debugging-address=0.0.0.0",
        ]);
        assert_eq!(launch.transport, port("0.0.0.0", 0));

        let launch = ChromiumLaunch::parse(["--remote-debugging-port=9333"]);
        assert_eq!(launch.transport, port(DEFAULT_DEBUGGING_ADDRESS, 9333));
    }

    #[test]
    fn a_bare_launch_serves_on_the_default_port() {
        let launch = ChromiumLaunch::parse(Vec::<String>::new());
        assert_eq!(
            launch.transport,
            port(DEFAULT_DEBUGGING_ADDRESS, DEFAULT_DEBUGGING_PORT)
        );
    }

    #[test]
    fn arguments_after_a_double_dash_are_positional() {
        let launch = ChromiumLaunch::parse(["--", "--not-a-switch"]);
        assert_eq!(launch.start_url.as_deref(), Some("--not-a-switch"));
        assert!(launch.ignored_switches.is_empty());
    }

    #[test]
    fn moli_serve_args_forward_profile_and_pipe_and_always_enable_layout() {
        let launch = ChromiumLaunch::parse(fixture_args(MCP_DEFAULT));
        let args = launch.moli_serve_args();
        assert_eq!(
            args,
            [
                "moli",
                "serve",
                "--layout",
                "--profile-dir",
                "{USER_DATA_DIR}",
                "--remote-debugging-pipe"
            ]
            .map(OsString::from)
        );

        let cli = Cli::try_parse_from(args).expect("moli accepts the translated command line");
        assert!(matches!(cli.command, Commands::Serve(_)));
        let config = AppConfig::from_cli(&cli).expect("moli builds a config");
        assert!(config.browser.layout_policy().uses_real_layout());
    }

    #[test]
    fn moli_serve_args_bind_the_requested_port() {
        let launch = ChromiumLaunch::parse(["--remote-debugging-port=0"]);
        assert_eq!(
            launch.moli_serve_args(),
            [
                "moli",
                "serve",
                "--layout",
                "--host",
                DEFAULT_DEBUGGING_ADDRESS,
                "--port",
                "0"
            ]
            .map(OsString::from)
        );
    }
}
