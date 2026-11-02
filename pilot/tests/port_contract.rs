//! `pilot --remote-debugging-port`: announce the endpoint the way Chromium
//! does, so clients that launch with a port (Puppeteer's default, Playwright's
//! channel lookup) can find it.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

use serde_json::Value;

struct RunningPilot {
    child: Child,
    profile: PathBuf,
}

impl Drop for RunningPilot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

fn fresh_profile_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock is after the epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pilot-port-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("profile dir is creatable");
    dir
}

fn http_get_json(address: &str, path: &str) -> Value {
    let mut stream = TcpStream::connect(address).expect("connect to the debugging port");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .expect("send request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP response has a body");
    serde_json::from_str(body).unwrap_or_else(|error| panic!("{path} is JSON ({error}): {body}"))
}

#[test]
fn port_mode_prints_devtools_listening_and_writes_devtools_active_port() {
    let profile = fresh_profile_dir();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pilot"))
        .args([
            "--headless",
            "--remote-debugging-port=0",
            &format!("--user-data-dir={}", profile.display()),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("pilot should spawn");
    let stderr = child.stderr.take().expect("stderr is piped");
    let pilot = RunningPilot { child, profile };

    let (sender, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    let announced = loop {
        let line = lines
            .recv_timeout(Duration::from_secs(15))
            .expect("pilot announces its endpoint on stderr");
        if let Some(url) = line.strip_prefix("DevTools listening on ") {
            break url.to_owned();
        }
    };
    let address = announced
        .strip_prefix("ws://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(address, _)| address.to_owned())
        .unwrap_or_else(|| panic!("announced endpoint is a ws:// URL: {announced}"));

    let version = http_get_json(&address, "/json/version/");
    assert_eq!(
        version["webSocketDebuggerUrl"].as_str(),
        Some(announced.as_str()),
        "the announced endpoint is the browser's WebSocket URL"
    );

    let port = address.rsplit(':').next().expect("address has a port");
    let browser_path = announced
        .strip_prefix(&format!("ws://{address}"))
        .expect("announced URL starts with its address");
    let active_port = std::fs::read_to_string(pilot.profile.join("DevToolsActivePort"))
        .expect("DevToolsActivePort is written into the profile");
    assert_eq!(active_port, format!("{port}\n{browser_path}"));
}
