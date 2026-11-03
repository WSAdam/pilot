//! pilot: Moli launched the way Playwright launches Chromium.
//!
//! Playwright and Playwright MCP start a browser with Chromium's command line:
//! dozens of feature switches, `--user-data-dir`, `--remote-debugging-pipe`
//! and a start URL. [`argv::ChromiumLaunch`] reads that command line, keeps the
//! switches pilot acts on, ignores the rest, and turns it into the
//! `moli serve` configuration that runs the browser.

pub mod argv;
pub mod config;
pub mod headful;
#[cfg(unix)]
pub mod profile;
pub mod serve;
