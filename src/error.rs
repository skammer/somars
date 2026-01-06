//! Error types for the somars application
//!
//! This module provides error handling with integration to color-eyre for rich error reporting.

use thiserror::Error;

/// Application-specific error type
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Station error: {0}")]
    Station(String),

    #[error("UDP error: {0}")]
    Udp(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Generic error: {0}")]
    Generic(String),
}

impl From<crate::config::ConfigError> for AppError {
    fn from(err: crate::config::ConfigError) -> Self {
        AppError::Config(err.to_string())
    }
}

/// Result type alias for backwards compatibility
pub type AppResult<T> = std::result::Result<T, AppError>;

// For new code using color-eyre
// Use color_eyre::eyre::Result<T> directly or
// use crate::error::Result<T> alias if we want to provide one

// Example: When migrating to color-eyre in main.rs:
// fn main() -> color_eyre::eyre::Result<()> {
//     color_eyre::install()?;
//     // ... rest of code
// }