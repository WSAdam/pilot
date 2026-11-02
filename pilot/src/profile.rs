//! The profile markers Chromium clients look for in `--user-data-dir`.
//!
//! - `SingletonLock` is a symlink to `<hostname>-<pid>`. Playwright MCP reads
//!   it to tell whether a profile is in use (a live pid means in use), and
//!   Chromium refuses a second instance on a live lock.
//! - `DevToolsActivePort` holds the debugging port and browser endpoint path,
//!   written when CDP is served on a port.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Name of Chromium's profile lock symlink.
pub const SINGLETON_LOCK: &str = "SingletonLock";
/// Name of the file announcing the debugging port.
pub const DEVTOOLS_ACTIVE_PORT: &str = "DevToolsActivePort";
/// What Chromium prints when another instance holds the profile. Playwright
/// recognises "ProcessSingleton" and MCP turns it into "Browser is already in
/// use".
pub const PROFILE_IN_USE_MESSAGE: &str = "Failed to create a ProcessSingleton for your profile directory. This means that the profile is already in use by another instance of Chromium.";
/// Chromium's exit code for a profile held by another instance.
pub const PROFILE_IN_USE_EXIT_CODE: u8 = 21;

#[derive(Debug)]
pub enum LockError {
    /// A live process holds the profile.
    InUse {
        holder: String,
    },
    Io(io::Error),
}

/// A held `SingletonLock`; dropping it removes the symlink if it is still ours.
#[derive(Debug)]
pub struct SingletonLock {
    path: PathBuf,
    target: String,
}

impl SingletonLock {
    /// Claims `<user_data_dir>/SingletonLock` for this process. A lock whose
    /// pid is no longer alive (left by a crash) is replaced.
    pub fn acquire(user_data_dir: &Path) -> Result<Self, LockError> {
        let path = user_data_dir.join(SINGLETON_LOCK);
        match std::fs::read_link(&path) {
            Ok(existing) => {
                let holder = existing.to_string_lossy().into_owned();
                if holder_pid(&holder).is_some_and(process_is_alive) {
                    return Err(LockError::InUse { holder });
                }
                std::fs::remove_file(&path).map_err(LockError::Io)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            // Something that is not a symlink sits at the lock path.
            Err(_) => std::fs::remove_file(&path).map_err(LockError::Io)?,
        }
        let target = format!("{}-{}", hostname(), std::process::id());
        std::os::unix::fs::symlink(&target, &path).map_err(LockError::Io)?;
        Ok(Self { path, target })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

impl Drop for SingletonLock {
    fn drop(&mut self) {
        // Leave a lock another process has since taken over.
        if std::fs::read_link(&self.path)
            .is_ok_and(|target| target.as_os_str() == self.target.as_str())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// The pid after the last '-', as Chromium and Playwright MCP read it.
fn holder_pid(holder: &str) -> Option<libc::pid_t> {
    holder.rsplit('-').next()?.parse().ok()
}

fn process_is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists and may be signalled.
    pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0
            || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

fn hostname() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is writable for its full length.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return "localhost".to_owned();
    }
    let end = buffer
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// Contents of `DevToolsActivePort`: the port, then the browser endpoint path.
pub fn devtools_active_port_contents(port: u16, browser_path: &str) -> String {
    format!("{port}\n{browser_path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn fresh_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pilot-profile-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A pid that has certainly exited: a child that was already reaped.
    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().expect("spawn true");
        let pid = child.id();
        child.wait().expect("reap true");
        pid
    }

    #[test]
    fn acquire_points_the_symlink_at_hostname_and_pid_and_drop_removes_it() {
        let dir = fresh_dir("acquire");
        let lock = SingletonLock::acquire(&dir).expect("a fresh profile is free");
        let target = std::fs::read_link(dir.join(SINGLETON_LOCK)).expect("lock is a symlink");
        let target = target.to_string_lossy().into_owned();
        assert_eq!(target, lock.target());
        assert!(
            target.ends_with(&format!("-{}", std::process::id())),
            "Playwright MCP reads the pid after the last '-': {target}"
        );
        drop(lock);
        assert!(std::fs::symlink_metadata(dir.join(SINGLETON_LOCK)).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_live_holder_keeps_the_profile() {
        let dir = fresh_dir("live");
        let _held = SingletonLock::acquire(&dir).expect("first claim");
        match SingletonLock::acquire(&dir) {
            Err(LockError::InUse { holder }) => {
                assert!(holder.ends_with(&format!("-{}", std::process::id())))
            }
            other => panic!("a live holder must block a second claim, got {other:?}"),
        }
    }

    #[test]
    fn a_lock_left_by_a_dead_process_is_replaced() {
        let dir = fresh_dir("stale");
        std::os::unix::fs::symlink(
            format!("crashed-host-{}", dead_pid()),
            dir.join(SINGLETON_LOCK),
        )
        .unwrap();
        let lock = SingletonLock::acquire(&dir).expect("a stale lock is replaced");
        assert!(lock.target().ends_with(&format!("-{}", std::process::id())));
        drop(lock);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drop_leaves_a_lock_that_someone_else_now_holds() {
        let dir = fresh_dir("taken-over");
        let lock = SingletonLock::acquire(&dir).expect("first claim");
        std::fs::remove_file(lock.path()).unwrap();
        std::os::unix::fs::symlink("other-host-1", lock.path()).unwrap();
        drop(lock);
        assert_eq!(
            std::fs::read_link(dir.join(SINGLETON_LOCK)).unwrap(),
            PathBuf::from("other-host-1")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn devtools_active_port_is_the_port_then_the_browser_path() {
        assert_eq!(
            devtools_active_port_contents(9333, "/devtools/browser/moli-browser"),
            "9333\n/devtools/browser/moli-browser"
        );
    }
}
