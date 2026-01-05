//! Audio management module
//!
//! This module handles all audio playback functionality including:
//! - Stream downloading and buffering
//! - Audio playback control
//! - ICY metadata extraction
//! - Volume control
//!
//! The main entry point is the [`AudioManager`] struct which provides
//! a high-level API for audio operations.

pub mod manager;
pub mod playback;
pub mod metadata;
pub mod stream;
pub mod types;

pub use manager::{AudioApp, AudioManager};
pub use types::{AudioError, AudioResult, AudioState};

// Re-export common types
pub use playback::PlaybackHandle;
pub use metadata::MetadataEvent;
