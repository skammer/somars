//! Logging initialization and configuration
//!
//! This module sets up structured logging using the `tracing` crate.
//!
//! Logs go to `~/.config/somars/somars.log` to avoid corrupting the TUI.
//! To see more detailed logs, set RUST_LOG.

use std::{fs::OpenOptions, path::PathBuf, sync::Mutex};
use tracing_subscriber::EnvFilter;

/// Initialize the tracing subscriber for logging
///
/// This sets up structured logging with:
/// - File output (to avoid interfering with TUI stdout/stderr)
/// - Default level: ERROR only (quiet by default)
/// - Configurable log level via RUST_LOG environment variable
/// - Clean formatting without targets (for better readability)
///
/// # Environment Variables
///
/// - `RUST_LOG`: Set the log level (e.g., `info`, `debug`, `warn`, `error`)
///   - `error` - Error messages only (default)
///   - `warn` - Warnings and errors
///   - `info` - General informational messages
///   - `debug` - Detailed debugging information
///   - Module-specific filtering: `somars::audio=debug,somars=info`
///
/// # Example
///
/// ```no_run
/// # use somars::logging::init_logging;
/// init_logging();
/// ```
///
/// # Log File
///
/// Logs are written to:
///
/// ```bash
/// ~/.config/somars/somars.log
/// ```
pub fn init_logging() {
    // Read log level from environment, defaulting to ERROR (quiet by default)
    // Users can set RUST_LOG=debug to see more detailed logs
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("somars=error"));

    if let Some(log_file) = open_log_file() {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .with_writer(Mutex::new(log_file))
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .with_writer(std::io::sink)
            .init();
    }
}

fn open_log_file() -> Option<std::fs::File> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn log_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".config").join("somars").join("somars.log"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_init_logging() {
        // Note: This test will fail if a subscriber is already initialized
        // In normal usage, init_logging should only be called once at startup
        // For testing purposes, we just verify the function exists and compiles
        // Actual testing would require a more complex setup
    }
}
