//! Audio module types and errors

use crate::error::AppError;
use std::fmt;

/// Audio playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioState {
    Stopped,
    Playing,
    Paused,
    Loading,
}

/// Audio-specific errors
#[derive(Debug)]
pub enum AudioError {
    /// Failed to initialize audio output
    InitializationFailed(String),
    /// Failed to connect to stream
    StreamConnectionFailed(String),
    /// Stream decoding error
    DecodeError(String),
    /// Audio sink mutex poisoned
    SinkPoisoned,
    /// Invalid URL
    InvalidUrl(String),
    /// Network error
    Network(String),
    /// Generic error
    Other(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::InitializationFailed(msg) => write!(f, "Audio initialization failed: {}", msg),
            AudioError::StreamConnectionFailed(msg) => write!(f, "Stream connection failed: {}", msg),
            AudioError::DecodeError(msg) => write!(f, "Decode error: {}", msg),
            AudioError::SinkPoisoned => write!(f, "Audio sink mutex poisoned"),
            AudioError::InvalidUrl(url) => write!(f, "Invalid URL: {}", url),
            AudioError::Network(msg) => write!(f, "Network error: {}", msg),
            AudioError::Other(msg) => write!(f, "Audio error: {}", msg),
        }
    }
}

impl std::error::Error for AudioError {}

impl From<AudioError> for AppError {
    fn from(err: AudioError) -> Self {
        AppError::Audio(err.to_string())
    }
}

/// Result type for audio operations
pub type AudioResult<T> = Result<T, AudioError>;
