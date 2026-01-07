//! Action enum for command pattern
//!
//! Actions represent all possible commands and state changes in the application.
//! They are sent through channels for decoupled execution.

use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, PartialEq, Display, Serialize, Deserialize)]
pub enum Action {
    // System events
    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ClearScreen,
    Error(String),

    // Playback control
    Play,
    Stop,
    TogglePause,
    Pause,
    ResumePlayback,

    // Navigation
    StationUp,
    StationDown,
    SelectStation(usize),
    ScrollHistoryUp,
    ScrollHistoryDown,

    // Station selection
    TuneStation(String),
    TuneNext,
    TunePrev,

    // Volume
    VolumeUp,
    VolumeDown,
    SetVolume(f32),

    // UI
    ToggleHelp,
    Help,

    // Metadata
    MetadataUpdate(String),

    // State update (for components)
    UpdateStations(Vec<crate::station::Station>),
    SetActiveStation(Option<usize>),
    SetPlaybackState(crate::PlaybackState),
    SetSelectedStation(Option<crate::station::Station>),
    SetTotalPlayed(std::time::Duration),
    StartTrackingPlayTime,
    StopTrackingPlayTime,
}
