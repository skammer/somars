use crate::{action::Action, station::Station, PlaybackState};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct MediaSessionHandle;

impl MediaSessionHandle {
    pub fn start(_action_tx: mpsc::UnboundedSender<Action>, _volume: f32) -> Self {
        Self
    }

    pub fn set_playback_state(&self, _state: PlaybackState) {}

    pub fn set_station(&self, _station: Station) {}

    pub fn set_track_title(&self, _station: Station, _title: String) {}

    pub fn set_volume(&self, _volume: f32) {}
}
