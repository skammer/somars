//! Audio manager - High-level audio API
//!
//! Provides a centralized interface for audio playback control.

use super::playback::{start_playback, PlaybackHandle};
use super::types::{AudioError, AudioResult, AudioState};
use crate::station::Station;
use rodio::Sink;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Audio manager - Centralized control for audio playback
pub struct AudioManager {
    handle: Option<tokio::task::JoinHandle<AudioResult<()>>>,
    state: AudioState,
    pub(crate) current_station: Option<String>,
}

impl AudioManager {
    /// Create a new AudioManager (for internal use)
    pub(crate) fn new() -> Self {
        Self {
            handle: None,
            state: AudioState::Stopped,
            current_station: None,
        }
    }

    /// Get current audio state
    pub fn state(&self) -> AudioState {
        self.state
    }

    /// Get current station ID if playing
    pub fn current_station(&self) -> Option<&str> {
        self.current_station.as_deref()
    }

    /// Set the audio state
    pub(crate) fn set_state(&mut self, state: AudioState) {
        self.state = state;
    }

    /// Set the current station
    pub(crate) fn set_current_station(&mut self, station: String) {
        self.current_station = Some(station);
    }

    /// Clear the current station
    pub(crate) fn clear_current_station(&mut self) {
        self.current_station = None;
    }

    /// Set the playback handle
    pub(crate) fn set_handle(&mut self, handle: tokio::task::JoinHandle<AudioResult<()>>) {
        self.handle = Some(handle);
    }

    /// Take the playback handle (consuming it)
    pub(crate) fn take_handle(&mut self) -> Option<tokio::task::JoinHandle<AudioResult<()>>> {
        self.handle.take()
    }

    /// Check if there's an active playback
    pub fn is_active(&self) -> bool {
        self.handle.is_some()
    }
}

/// Extension trait for integrating AudioManager with existing App
pub trait AudioApp {
    /// Get the audio sink
    fn sink(&self) -> Option<Arc<Mutex<Sink>>>;

    /// Get the audio manager
    fn audio_manager(&self) -> &AudioManager;

    /// Get mutable audio manager
    fn audio_manager_mut(&mut self) -> &mut AudioManager;

    /// Stop current playback
    fn stop_audio(&mut self) -> AudioResult<()>;

    /// Pause playback
    fn pause_audio(&mut self) -> AudioResult<()>;

    /// Resume playback
    fn resume_audio(&mut self) -> AudioResult<()>;

    /// Set volume
    fn set_audio_volume(&mut self, level: f32) -> AudioResult<()>;

    /// Get queue length
    fn audio_queue_length(&self) -> Option<usize>;
}

// Note: The full integration will require modifying the App struct
// For now, this is a simplified interface that can be expanded

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_manager_creation() {
        let manager = AudioManager::new();
        assert_eq!(manager.state(), AudioState::Stopped);
        assert!(manager.current_station().is_none());
        assert!(!manager.is_active());
    }

    #[test]
    fn test_audio_state_transitions() {
        let mut manager = AudioManager::new();
        manager.set_state(AudioState::Playing);
        assert_eq!(manager.state(), AudioState::Playing);

        manager.set_current_station("test".to_string());
        assert_eq!(manager.current_station(), Some("test"));
    }
}
