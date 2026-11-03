//! `pilot`: start Moli from a Chromium command line.
//!
//! Playwright launches this binary exactly as it would launch Chromium. The
//! command line is translated into Moli's configuration, the profile is
//! claimed the way Chromium claims it, and Moli serves CDP in-process.

use anyhow::{Context, Result};
use pilot::argv::ChromiumLaunch;
use std::process::ExitCode;

fn run() -> Result<ExitCode> {
    moli_process_signal::install_immediate_exit_handlers()?;
    let launch = ChromiumLaunch::parse(std::env::args_os().skip(1));
    let config = pilot::config::moli_config(&launch)?;
    moli::telemetry::init(&config.log_filter);
    if !launch.ignored_switches.is_empty() {
        tracing::debug!(switches = ?launch.ignored_switches, "🔍 ignoring Chromium switches");
    }

    #[cfg(unix)]
    let _profile_lock = match launch.user_data_dir.as_deref() {
        Some(user_data_dir) => {
            std::fs::create_dir_all(user_data_dir)
                .with_context(|| format!("❌ cannot create {}", user_data_dir.display()))?;
            match pilot::profile::SingletonLock::acquire(user_data_dir) {
                Ok(lock) => Some(lock),
                Err(pilot::profile::LockError::InUse { holder }) => {
                    eprintln!("{}", pilot::profile::PROFILE_IN_USE_MESSAGE);
                    tracing::warn!(%holder, "❌ profile already in use");
                    return Ok(ExitCode::from(pilot::profile::PROFILE_IN_USE_EXIT_CODE));
                }
                Err(pilot::profile::LockError::Io(error)) => {
                    return Err(error).context("❌ cannot claim the profile's SingletonLock");
                }
            }
        }
        None => None,
    };

    let runtime_thread_budget = moli::runtime_thread_budget::tokio_runtime_thread_budget();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(runtime_thread_budget)
        .max_blocking_threads(runtime_thread_budget)
        .enable_all()
        .build()?;
    if pilot::headful::wants_window(&launch, std::env::var_os(pilot::headful::NO_WINDOW_ENV)) {
        tracing::info!("🚀 pilot starting with its watch-the-agent window");
        pilot::headful::run(launch, config, runtime)?;
    } else {
        runtime.block_on(pilot::serve::run(&launch, &config))?;
    }
    tracing::info!("✅ pilot exited cleanly");
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            let stderr = std::io::stderr();
            let _ = moli::app::write_error_report(&mut stderr.lock(), &error);
            moli::app::error_exit_code(&error)
        }
    }
}
