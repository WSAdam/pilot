//! `pilot`: start Moli from a Chromium command line.
//!
//! Playwright launches this binary exactly as it would launch Chromium. The
//! command line is translated into `moli serve` and run in-process.

use anyhow::{Context, Result};
use clap::Parser;
use pilot::argv::ChromiumLaunch;
use std::process::ExitCode;

fn run() -> Result<()> {
    moli_process_signal::install_immediate_exit_handlers()?;
    let launch = ChromiumLaunch::parse(std::env::args_os().skip(1));
    let cli = moli::cli::Cli::try_parse_from(launch.moli_serve_args())
        .context("❌ pilot could not translate the Chromium command line for moli")?;
    let runtime_thread_budget = moli::runtime_thread_budget::tokio_runtime_thread_budget();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(runtime_thread_budget)
        .max_blocking_threads(runtime_thread_budget)
        .enable_all()
        .build()?
        .block_on(moli::app::run_cli(&mut std::io::stdout(), cli))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let stderr = std::io::stderr();
            let _ = moli::app::write_error_report(&mut stderr.lock(), &error);
            moli::app::error_exit_code(&error)
        }
    }
}
