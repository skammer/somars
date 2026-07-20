//! Audio management module
//!
//! This module handles all audio playback functionality including:
//! - Stream downloading and buffering
//! - Audio playback control
//! - ICY metadata extraction
//! - Volume control
//! - Error recovery and retry logic
//!
//! The main entry point is the [`AudioManager`] struct which provides
//! a high-level API for audio operations.

mod icy_reader;
pub mod manager;
pub mod metadata;
pub mod playback;
pub mod recovery;
pub mod stream;
pub mod types;

pub use manager::AudioManager;

// Re-export common types and functions
pub use metadata::MetadataEvent;
pub use playback::start_playback;
