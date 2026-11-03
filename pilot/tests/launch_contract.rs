//! Launch `pilot` exactly as Playwright 1.65 and Playwright MCP launch
//! Chromium (argv captured from real launches in `tests/fixtures/`), then
//! drive it over Chromium's pipe transport: commands on file descriptor 3,
//! replies on 4, each message NUL-terminated JSON.
#![cfg(unix)]

use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use serde_json::{Value, json};

const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Playwright sends `Browser.close` with this id and ignores the reply.
const PLAYWRIGHT_BROWSER_CLOSE_ID: i64 = -9999;

const FIXTURES: [(&str, &str); 5] = [
    (
        "launch-headless",
        include_str!("fixtures/playwright-1.65/launch-headless.argv"),
    ),
    (
        "launch-headed",
        include_str!("fixtures/playwright-1.65/launch-headed.argv"),
    ),
    (
        "persistent-headless",
        include_str!("fixtures/playwright-1.65/persistent-headless.argv"),
    ),
    (
        "persistent-headed",
        include_str!("fixtures/playwright-1.65/persistent-headed.argv"),
    ),
    (
        "mcp-default",
        include_str!("fixtures/playwright-1.65/mcp-default.argv"),
    ),
];

struct PipedPilot {
    child: Child,
    commands: Option<File>,
    replies: mpsc::Receiver<Value>,
    profile: PathBuf,
}

impl Drop for PipedPilot {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            // Only a failed test needs this fallback; success must observe a
            // natural exit first.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

impl PipedPilot {
    fn send(&mut self, message: Value) {
        let commands = self.commands.as_mut().expect("command pipe is open");
        let mut frame = serde_json::to_vec(&message).expect("command serializes");
        frame.push(0);
        commands
            .write_all(&frame)
            .expect("command pipe accepts a frame");
        commands.flush().expect("command pipe flushes");
    }

    fn reply_to(&self, id: i64, fixture: &str) -> Value {
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = self
                .replies
                .recv_timeout(remaining)
                .unwrap_or_else(|error| {
                    panic!("{fixture}: no reply to command {id} on fd 4: {error}")
                });
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    fn wait_for_exit(&mut self, fixture: &str) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("child status is readable") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "{fixture}: pilot did not exit within {EXIT_TIMEOUT:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Creates a pipe whose descriptors sit at 10 or above and are close-on-exec,
/// so the child only inherits the copies placed at 3 and 4.
fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    // SAFETY: `pipe` writes two new descriptors into the array.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe failed");
    let [read, write] = fds.map(|fd| {
        // SAFETY: `fd` was just created by `pipe` and is owned here.
        let moved = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        assert!(moved >= 10, "fcntl(F_DUPFD_CLOEXEC) failed");
        // SAFETY: `fd` is still open and has no other owner.
        unsafe { libc::close(fd) };
        // SAFETY: `moved` is a fresh descriptor with no other owner.
        unsafe { OwnedFd::from_raw_fd(moved) }
    });
    (read, write)
}

fn fresh_profile_dir(fixture: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock is after the epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "pilot-launch-{fixture}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("profile dir is creatable");
    dir
}

fn playwright_args(argv: &str, profile: &std::path::Path) -> Vec<String> {
    argv.lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.replace("{USER_DATA_DIR}", &profile.to_string_lossy()))
        .collect()
}

fn singleton_lock_target(profile: &std::path::Path) -> Option<String> {
    std::fs::read_link(profile.join("SingletonLock"))
        .ok()
        .map(|target| target.to_string_lossy().into_owned())
}

/// Launches pilot as Playwright does, without its window, so the suite
/// opens no windows on a desktop.
fn spawn_like_playwright(fixture: &str, argv: &str) -> PipedPilot {
    spawn_with_window(fixture, argv, false)
}

fn spawn_with_window(fixture: &str, argv: &str, window: bool) -> PipedPilot {
    let profile = fresh_profile_dir(fixture);
    let args = playwright_args(argv, &profile);
    let (child_input, parent_commands) = pipe();
    let (parent_replies, child_output) = pipe();
    let input_fd = child_input.as_raw_fd();
    let output_fd = child_output.as_raw_fd();
    let mut command = Command::new(env!("CARGO_BIN_EXE_pilot"));
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !window {
        command.env(pilot::headful::NO_WINDOW_ENV, "1");
    }
    // SAFETY: only async-signal-safe `dup2` calls run between fork and exec.
    // Both sources are at 10 or above, so placing them at 3 and 4 cannot
    // clobber one another; `dup2` clears close-on-exec on the copies.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(input_fd, 3) < 0 || libc::dup2(output_fd, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().expect("pilot should spawn");
    drop(child_input);
    drop(child_output);

    let (sender, replies) = mpsc::channel();
    let mut reader = BufReader::new(File::from(parent_replies));
    thread::spawn(move || {
        loop {
            let mut frame = Vec::new();
            match reader.read_until(0, &mut frame) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if frame.last() == Some(&0) {
                        frame.pop();
                    }
                    let message = serde_json::from_slice(&frame).expect("reply frame is JSON");
                    if sender.send(message).is_err() {
                        return;
                    }
                }
            }
        }
    });

    PipedPilot {
        child,
        commands: Some(File::from(parent_commands)),
        replies,
        profile,
    }
}

#[test]
fn pilot_accepts_every_playwright_launch_command_line_and_closes_like_chromium() {
    for (fixture, argv) in FIXTURES {
        let mut pilot = spawn_like_playwright(fixture, argv);

        pilot.send(json!({ "id": 1, "method": "Browser.getVersion" }));
        let reply = pilot.reply_to(1, fixture);
        let product = reply["result"]["product"].as_str().unwrap_or_default();
        assert!(
            product.contains('/'),
            "{fixture}: Browser.getVersion product must be name/version, got {reply}"
        );

        // Playwright MCP reads this to tell whether the profile is in use.
        let lock = singleton_lock_target(&pilot.profile)
            .unwrap_or_else(|| panic!("{fixture}: SingletonLock is held while pilot runs"));
        assert!(
            lock.ends_with(&format!("-{}", pilot.child.id())),
            "{fixture}: SingletonLock must name pilot's pid after the last '-': {lock}"
        );

        pilot.send(json!({ "id": PLAYWRIGHT_BROWSER_CLOSE_ID, "method": "Browser.close" }));
        let status = pilot.wait_for_exit(fixture);
        assert!(
            status.success(),
            "{fixture}: Browser.close must exit cleanly, got {status}"
        );
        assert_eq!(
            singleton_lock_target(&pilot.profile),
            None,
            "{fixture}: a clean exit releases SingletonLock"
        );
    }
}

#[test]
fn a_second_pilot_on_a_profile_in_use_fails_like_chromium() {
    let (fixture, argv) = FIXTURES[4];
    let mut first = spawn_like_playwright(fixture, argv);
    first.send(json!({ "id": 1, "method": "Browser.getVersion" }));
    first.reply_to(1, fixture);

    let second = Command::new(env!("CARGO_BIN_EXE_pilot"))
        .args(playwright_args(argv, &first.profile))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("second pilot runs");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert_eq!(
        second.status.code(),
        Some(21),
        "Chromium exits 21 when the profile is in use; stderr: {stderr}"
    );
    assert!(
        stderr.contains("Failed to create a ProcessSingleton"),
        "Playwright recognises Chromium's ProcessSingleton message: {stderr}"
    );

    first.send(json!({ "id": PLAYWRIGHT_BROWSER_CLOSE_ID, "method": "Browser.close" }));
    assert!(first.wait_for_exit(fixture).success());
}

// 2026-11-03: with the watch-the-agent window open, pilot answered and closed
// the browser on Browser.close but never exited: the runtime went down with
// the browser thread and cancelled the window's controller before it could
// tell the window the browser had stopped. A headed launch opens a real
// window here (on a desktop; without a display pilot runs headless).
#[test]
fn a_headed_launch_with_its_window_still_closes_like_chromium() {
    for (fixture, argv) in [FIXTURES[1], FIXTURES[4]] {
        let mut pilot = spawn_with_window(fixture, argv, true);
        pilot.send(json!({ "id": 1, "method": "Browser.getVersion" }));
        assert!(pilot.reply_to(1, fixture)["result"]["product"].is_string());
        pilot.send(json!({ "id": PLAYWRIGHT_BROWSER_CLOSE_ID, "method": "Browser.close" }));
        assert!(
            pilot.wait_for_exit(fixture).success(),
            "{fixture}: pilot exits cleanly"
        );
    }
}
