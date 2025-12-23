use tokio::sync::mpsc::Sender;

use crate::{App, HistoryMessage};

// Helper function to restart playback when underrun is detected
pub fn restart_playback(app: &mut App, log_tx: &Sender<HistoryMessage>) {
    // Store the currently selected station before we release the sink
    let selected_station_index = app.selected_station.selected();

    // Stop current playback
    if let Some(ref sink) = app.sink {
        if let Ok(sink_guard) = sink.lock() {
            sink_guard.stop();
            sink_guard.empty();
        }
    }

    // Update the playback state to Stopped
    app.playback_state = crate::PlaybackState::Stopped;

    // Attempt to restart playback from the currently selected station
    if let Some(station_index) = selected_station_index {
        if station_index < app.stations.len() {
            // Call handle_play to restart
            crate::keyboard::handle_play(app, log_tx);
        }
    }
}