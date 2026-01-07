//! Audio module types and errors

#![allow(dead_code)]

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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Audio buffer underrun - playback can be recovered
    AudioUnderrun,
    /// Stream temporarily unavailable - retryable
    StreamRetryable(String),
    /// Permanent stream error - should not retry
    StreamPermanent(String),
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
            AudioError::AudioUnderrun => write!(f, "Audio buffer underrun - attempting recovery"),
            AudioError::StreamRetryable(msg) => write!(f, "Stream temporarily unavailable: {}", msg),
            AudioError::StreamPermanent(msg) => write!(f, "Permanent stream error: {}", msg),
            AudioError::Other(msg) => write!(f, "Audio error: {}", msg),
        }
    }
}

impl std::error::Error for AudioError {}

impl AudioError {
    /// Check if this error is retryable (transient)
    pub fn is_retryable(&self) -> bool {
        match self {
            AudioError::AudioUnderrun => true,
            AudioError::StreamRetryable(_) => true,
            AudioError::Network(_) => true,
            AudioError::StreamConnectionFailed(_) => true,
            // These are permanent errors
            AudioError::StreamPermanent(_) => false,
            AudioError::SinkPoisoned => false,
            AudioError::InvalidUrl(_) => false,
            AudioError::InitializationFailed(_) => false,
            AudioError::DecodeError(_) => false,
            AudioError::Other(_) => false,
        }
    }

    /// Get the maximum number of retry attempts for this error
    pub fn max_retries(&self) -> u32 {
        match self {
            AudioError::AudioUnderrun => 10,
            AudioError::StreamRetryable(_) => 5,
            AudioError::Network(_) => 3,
            AudioError::StreamConnectionFailed(_) => 3,
            _ => 0,
        }
    }
}

impl From<AudioError> for AppError {
    fn from(err: AudioError) -> Self {
        AppError::Audio(err.to_string())
    }
}

/// Result type for audio operations
pub type AudioResult<T> = Result<T, AudioError>;
