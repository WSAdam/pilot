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
    /// Both switches: Chromium serves the pipe and the port, one browser
    /// behind both, and announces the port. Playwright waits for that
    /// announcement when a launch names a port.
    PipeAndPort { address: String, port: u16 },
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
    /// `--proxy-server`: where Playwright's `proxy` launch option points.
    pub proxy_server: Option<String>,
    /// `--proxy-bypass-list`: hosts that skip the proxy, `;`-separated.
    pub proxy_bypass_list: Option<String>,
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
        let mut proxy_server = None;
        let mut proxy_bypass_list = None;
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
                "proxy-server" => proxy_server = value.map(str::to_owned),
                "proxy-bypass-list" => proxy_bypass_list = value.map(str::to_owned),
                _ => ignored_switches.push(name.to_owned()),
            }
        }

        let address = address.unwrap_or_else(|| DEFAULT_DEBUGGING_ADDRESS.to_owned());
        let transport = match (pipe, port) {
            (true, None) => DebuggingTransport::Pipe,
            (true, Some(port)) => DebuggingTransport::PipeAndPort { address, port },
            (false, port) => DebuggingTransport::Port {
                address,
                port: port.unwrap_or(DEFAULT_DEBUGGING_PORT),
            },
        };
        Self {
            headless,
            user_data_dir,
            transport,
            start_url: positional.into_iter().next(),
            no_startup_window,
            proxy_server,
            proxy_bypass_list,
            ignored_switches,
        }
    }

    /// The `moli serve` command line (including the program name) that runs
    /// this launch. Real layout is always on: Playwright's actionability
    /// checks and coordinate input need real geometry.
    pub fn moli_serve_args(&self) -> Vec<OsString> {
        // Images and fonts change layout, so agents need them; audio, video
        // and text tracks stay off to keep pilot lean.
        let mut args: Vec<OsString> = ["moli", "serve", "--layout", "--image", "--font"]
            .map(OsString::from)
            .into();
        if let Some(user_data_dir) = &self.user_data_dir {
            args.push("--profile-dir".into());
            args.push(user_data_dir.into());
        }
        match &self.transport {
            DebuggingTransport::Pipe => args.push("--remote-debugging-pipe".into()),
            // `moli serve` takes one transport; pilot's own serve loop runs the
            // pipe beside this port (serve.rs).
            DebuggingTransport::Port { address, port }
            | DebuggingTransport::PipeAndPort { address, port } => {
                args.push("--host".into());
                args.push(address.into());
                args.push("--port".into());
                args.push(port.to_string().into());
            }
        }
        if let Some(proxy_server) = &self.proxy_server {
            args.push("--http-proxy".into());
            args.push(proxy_server.into());
        }
        if let Some(no_proxy) = self.proxy_bypass_list.as_deref().and_then(moli_no_proxy) {
            args.push("--http-no-proxy".into());
            args.push(no_proxy.into());
        }
        args
    }
}

/// Chromium's `--proxy-bypass-list` as moli's `--http-no-proxy` list.
/// Chromium-only rules such as `<-loopback>` (which Playwright always adds,
/// meaning "proxy loopback too") and `<local>` are dropped; `*.example.com` and
/// `.example.com` become `example.com`, which moli matches with its subdomains;
/// a `:port` suffix is dropped because moli matches hosts only.
fn moli_no_proxy(bypass_list: &str) -> Option<String> {
    let hosts = bypass_list
        .split([';', ','])
        .map(str::trim)
        .filter(|rule| !rule.is_empty() && !rule.starts_with('<'))
        .map(|rule| {
            let rule = rule
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            let rule = rule.strip_prefix("*.").unwrap_or(rule);
            let rule = rule.strip_prefix('.').unwrap_or(rule);
            match rule.rsplit_once(':') {
                Some((host, port)) if !host.contains(':') && port.parse::<u16>().is_ok() => host,
                _ => rule,
            }
        })
        .filter(|host| !host.is_empty())
        .collect::<Vec<_>>();
    (!hosts.is_empty()).then(|| hosts.join(","))
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
                "--image",
                "--font",
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
                "--image",
                "--font",
                "--host",
                DEFAULT_DEBUGGING_ADDRESS,
                "--port",
                "0"
            ]
            .map(OsString::from)
        );
    }

    fn string_args(launch: &ChromiumLaunch) -> Vec<String> {
        launch
            .moli_serve_args()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    // 2026-11-10: a launch naming a port beside Playwright's pipe served only
    // the pipe and never announced the port, so Playwright's launch hung
    // waiting for "DevTools listening on" (Playwright MCP http.spec).
    #[test]
    fn a_pipe_launch_that_names_a_port_serves_both() {
        let launch = ChromiumLaunch::parse([
            "--headless",
            "--remote-debugging-pipe",
            "--remote-debugging-port=9411",
        ]);
        assert_eq!(
            launch.transport,
            DebuggingTransport::PipeAndPort {
                address: DEFAULT_DEBUGGING_ADDRESS.to_owned(),
                port: 9411,
            }
        );
        let args = string_args(&launch);
        assert!(args.windows(2).any(|pair| pair == ["--port", "9411"]));
        assert!(!args.iter().any(|arg| arg == "--remote-debugging-pipe"));
        Cli::try_parse_from(args).expect("moli accepts the translated command line");
    }

    // 2026-11-10: --proxy-server was dropped as an unknown switch, so a
    // browser launched with Playwright's proxy option went direct (Playwright
    // MCP launch.spec --proxy-server).
    #[test]
    fn proxy_switches_reach_moli_as_its_proxy_settings() {
        let launch = ChromiumLaunch::parse([
            "--remote-debugging-pipe",
            "--proxy-server=http://127.0.0.1:3128",
            "--proxy-bypass-list=<-loopback>;*.example.com;.corp.test;intranet:8080",
        ]);
        assert!(
            launch.ignored_switches.is_empty(),
            "{:?}",
            launch.ignored_switches
        );
        let args = string_args(&launch);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--http-proxy", "http://127.0.0.1:3128"]),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--http-no-proxy", "example.com,corp.test,intranet"]),
            "{args:?}"
        );
        let cli = Cli::try_parse_from(args).expect("moli accepts the translated command line");
        AppConfig::from_cli(&cli).expect("moli builds a configuration with the proxy");
    }

    #[test]
    fn playwrights_loopback_rule_alone_bypasses_nothing() {
        let launch = ChromiumLaunch::parse([
            "--proxy-server=socks5://127.0.0.1:1080",
            "--proxy-bypass-list=<-loopback>",
        ]);
        let args = string_args(&launch);
        assert!(!args.iter().any(|arg| arg == "--http-no-proxy"), "{args:?}");
    }
}
