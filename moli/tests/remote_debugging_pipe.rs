//! End-to-end checks for `moli serve --remote-debugging-pipe`, the transport
//! Playwright uses when it launches a Chromium-compatible browser: commands
//! arrive on file descriptor 3, replies leave on descriptor 4, and each message
//! is UTF-8 JSON terminated by a NUL byte.
#![cfg(unix)]

use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Playwright sends `Browser.close` with this id and ignores the reply.
const PLAYWRIGHT_BROWSER_CLOSE_ID: i64 = -9999;

struct PipedChild {
    child: Child,
    commands: Option<File>,
    replies: mpsc::Receiver<Value>,
}

impl Drop for PipedChild {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            // Only a failed test needs this fallback; success must observe a
            // natural exit first.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl PipedChild {
    fn send(&mut self, message: Value) {
        let commands = self.commands.as_mut().expect("command pipe is open");
        let mut frame = serde_json::to_vec(&message).expect("command serializes");
        frame.push(0);
        commands
            .write_all(&frame)
            .expect("command pipe accepts a frame");
        commands.flush().expect("command pipe flushes");
    }

    fn reply_to(&self, id: i64) -> Value {
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = self
                .replies
                .recv_timeout(remaining)
                .unwrap_or_else(|error| panic!("no reply to command {id} on fd 4: {error}"));
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    fn close_command_pipe(&mut self) {
        self.commands.take();
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("child status is readable") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "moli did not exit within {EXIT_TIMEOUT:?}"
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

fn spawn_pipe_child() -> PipedChild {
    let (child_input, parent_commands) = pipe();
    let (parent_replies, child_output) = pipe();
    let input_fd = child_input.as_raw_fd();
    let output_fd = child_output.as_raw_fd();
    let mut command = Command::new(env!("CARGO_BIN_EXE_moli"));
    command
        .args(["serve", "--remote-debugging-pipe", "--http-no-proxy", "*"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
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
    let child = command
        .spawn()
        .expect("moli serve --remote-debugging-pipe should spawn");
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

    PipedChild {
        child,
        commands: Some(File::from(parent_commands)),
        replies,
    }
}

fn assert_browser_version(child: &mut PipedChild) {
    child.send(json!({ "id": 1, "method": "Browser.getVersion" }));
    let reply = child.reply_to(1);
    let product = reply["result"]["product"].as_str().unwrap_or_default();
    assert!(
        product.contains('/'),
        "Browser.getVersion product must be name/version for Playwright, got {reply}"
    );
}

#[test]
fn pipe_session_answers_browser_get_version_and_exits_on_browser_close() {
    let mut child = spawn_pipe_child();
    assert_browser_version(&mut child);

    child.send(json!({ "id": PLAYWRIGHT_BROWSER_CLOSE_ID, "method": "Browser.close" }));
    let status = child.wait_for_exit();
    assert!(
        status.success(),
        "Browser.close must exit cleanly, got {status}"
    );
}

#[test]
fn pipe_session_exits_when_the_command_pipe_reaches_eof() {
    let mut child = spawn_pipe_child();
    assert_browser_version(&mut child);

    child.close_command_pipe();
    let status = child.wait_for_exit();
    assert!(
        status.success(),
        "EOF on fd 3 must exit cleanly, got {status}"
    );
}
