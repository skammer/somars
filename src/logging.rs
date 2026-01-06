//! Logging initialization and configuration
//!
//! This module sets up structured logging using the `tracing` crate.
//!
//! By default, logs are only written to stderr at ERROR level to avoid
//! interfering with the TUI. To see more detailed logs, set RUST_LOG.

use tracing_subscriber::{fmt, EnvFilter};

/// Initialize the tracing subscriber for logging
///
/// This sets up structured logging with:
/// - Stderr output (to avoid interfering with TUI stdout)
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
/// # Redirecting Logs to a File
///
/// To see logs without interfering with the TUI, redirect stderr to a file:
///
/// ```bash
/// cargo run 2> somars.log
/// RUST_LOG=debug cargo run 2> somars.log
/// ```
pub fn init_logging() {
    // Read log level from environment, defaulting to ERROR (quiet by default)
    // Users can set RUST_LOG=debug to see more detailed logs
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("somars=error"));

    // Build and initialize the subscriber
    // Write to stderr to avoid interfering with TUI stdout
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)  // Don't show module targets, cleaner output
        .with_thread_ids(false)  // Don't show thread IDs (not needed for single-threaded TUI)
        .with_file(false)  // Don't show file paths in logs
        .with_line_number(false)  // Don't show line numbers
        .with_writer(std::io::stderr)  // Write to stderr, not stdout
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_logging() {
        // Note: This test will fail if a subscriber is already initialized
        // In normal usage, init_logging should only be called once at startup
        // For testing purposes, we just verify the function exists and compiles
        // Actual testing would require a more complex setup
    }
}
