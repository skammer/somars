use crate::{App, MessageType, PlaybackState, HistoryMessage, t, control::ControlCommand};
use crate::audio;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;
use std::time::Instant;
use tracing::{info, debug};

/// Parse a key event and return the corresponding command if applicable
pub fn parse_key_event(key: KeyCode) -> Option<ControlCommand> {
    match key {
        KeyCode::Char('q') => Some(ControlCommand::Quit),
        KeyCode::Enter => Some(ControlCommand::Play),
        KeyCode::Char(' ') => Some(ControlCommand::TogglePause),
        KeyCode::Up => Some(ControlCommand::SelectUp),
        KeyCode::Down => Some(ControlCommand::SelectDown),
        KeyCode::Char('+') | KeyCode::Char('=') => Some(ControlCommand::VolumeUp),
        KeyCode::Char('-') => Some(ControlCommand::VolumeDown),
        KeyCode::Char('?') => Some(ControlCommand::ToggleHelp),
        KeyCode::Char('j') => Some(ControlCommand::ScrollHistoryUp),
        KeyCode::Char('k') => Some(ControlCommand::ScrollHistoryDown),
        _ => None
    }
}

/// Handle a key event by parsing it and executing the corresponding command
pub fn handle_key_event(
    key: KeyEvent,
    app: &mut App,
    log_tx: &Sender<HistoryMessage>,
    _last_tick: &mut Instant
) -> bool {
    // Handle Ctrl+C for graceful shutdown
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        info!("Ctrl+C detected, initiating graceful shutdown");
        app.should_quit = true;
        return true;
    }

    if let Some(command) = parse_key_event(key.code) {
        execute_command(command, app, log_tx);
        true
    } else {
        false
    }
}

/// Execute a control command
pub fn execute_command(
    command: ControlCommand,
    app: &mut App,
    log_tx: &Sender<HistoryMessage>
) {
    match command {
        ControlCommand::Quit => {
            app.should_quit = true;
        },
        ControlCommand::Play => {
            handle_play(app, log_tx);
        },
        ControlCommand::Stop => {
            handle_stop(app);
        },
        ControlCommand::TogglePause => {
            match app.playback_state {
                PlaybackState::Playing => {
                    handle_pause(app);
                }
                PlaybackState::Stopped => {
                    // Do nothing, let handle_play be called explicitly
                }
                PlaybackState::Paused => {
                    handle_resume(app, log_tx);
                }
            }
        },
        ControlCommand::VolumeUp => {
            handle_volume_up(app);
        },
        ControlCommand::VolumeDown => {
            handle_volume_down(app);
        },
        ControlCommand::SetVolume(level) => {
            app.volume = level.clamp(0.0, 2.0);
            if let Some(sink) = &app.sink {
                if let Ok(sink) = sink.lock() {
                    sink.set_volume(app.volume);
                }
            }
        },
        ControlCommand::Tune(station_id) => {
            if let Some(index) = app.stations.iter().position(|s| s.id == station_id) {
                app.selected_station.select(Some(index));
                handle_play(app, log_tx);
            }
        },
        ControlCommand::TuneNext => {
            if !app.stations.is_empty() {
                let current = app.selected_station.selected().unwrap_or(0);
                let new_index = if current == app.stations.len() - 1 {
                    0
                } else {
                    current + 1
                };
                app.selected_station.select(Some(new_index));
                handle_play(app, log_tx);
            }
        },
        ControlCommand::TunePrev => {
            if !app.stations.is_empty() {
                let current = app.selected_station.selected().unwrap_or(0);
                let new_index = if current == 0 {
                    app.stations.len() - 1
                } else {
                    current - 1
                };
                app.selected_station.select(Some(new_index));
                handle_play(app, log_tx);
            }
        },
        ControlCommand::SelectUp => {
            handle_up(app);
        },
        ControlCommand::SelectDown => {
            handle_down(app);
        },
        ControlCommand::Toggle => {
            if matches!(app.playback_state, PlaybackState::Stopped) {
                handle_play(app, log_tx);
            } else {
                handle_stop(app);
            }
        },
        ControlCommand::ToggleHelp => {
            app.show_help = !app.show_help;
        },
        ControlCommand::ScrollHistoryUp => {
            handle_history_scroll_up(app);
        },
        ControlCommand::ScrollHistoryDown => {
            handle_history_scroll_down(app);
        },
    }
}

pub fn handle_play(app: &mut App, log_tx: &Sender<HistoryMessage>) {
    debug!("handle_play called");
    if let Some(index) = app.selected_station.selected() {
        if let Some(station) = app.stations.get(index).cloned() {
            info!(station_id = %station.id, station_title = %station.title, "Starting playback");
            if let Some(original_sink) = &app.sink {
                app.active_station = Some(index);
                let current_time = std::time::Instant::now();
                app.playback_start_time = Some(current_time);
                app.playback_start_time_for_underrun = Some(current_time);
                app.station_loading = true;
                if let Some(pause_time) = app.last_pause_time.take() {
                    if let Some(start) = app.playback_start_time {
                        app.total_played += pause_time.duration_since(start);
                    }
                }

                // Stop any existing playback before starting new stream
                if let Ok(locked_sink) = original_sink.lock() {
                    locked_sink.stop();
                }

                let sink = original_sink.clone();
                let log_tx_clone = log_tx.clone();
                let metadata_tx = app.metadata_tx.clone();
                let station_title_for_completion = station.title.clone();
                let volume = app.volume;

                // Use the audio module's start_playback function
                let handle = tokio::spawn(async move {
                    match audio::start_playback(
                        &station,
                        sink,
                        metadata_tx,
                        log_tx_clone.clone(),
                        volume,
                    ).await {
                        Ok(inner_handle) => {
                            // Wait for the inner playback task to complete
                            match inner_handle.await {
                                Ok(Ok(())) => Ok(()),
                                Ok(Err(e)) => Err(e),
                                Err(join_error) => Err(audio::AudioError::Other(format!("Join error: {}", join_error))),
                            }
                        }
                        Err(e) => Err(e),
                    }
                });

                let log_tx_clone = log_tx.clone();
                app.playback_state = PlaybackState::Playing;

                tokio::spawn(async move {
                    let log_tx_clone_2 = log_tx_clone.clone();
                    match handle.await {
                        Ok(Ok(())) => {
                            let _ = log_tx_clone_2.send(HistoryMessage {
                                message: t("starting-playback").replace("{$station}", &station_title_for_completion),
                                message_type: MessageType::System,
                                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            }).await;
                            let _ = log_tx_clone_2.send(HistoryMessage {
                                message: t("connecting-to-stream"),
                                message_type: MessageType::System,
                                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            }).await;
                            let _ = log_tx_clone_2.send(HistoryMessage {
                                message: "CLEAR_STATION_LOADING".to_string(),
                                message_type: MessageType::Background,
                                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            }).await;
                        }
                        Ok(Err(e)) => {
                            let _ = log_tx_clone_2.send(HistoryMessage {
                                message: format!("Playback error: {}", e),
                                message_type: MessageType::Error,
                                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            }).await;
                        }
                        Err(e) => {
                            let _ = log_tx_clone_2.send(HistoryMessage {
                                message: t("playback-error").replace("{$error}", &e.to_string()),
                                message_type: MessageType::Error,
                                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            }).await;
                        }
                    }
                });
            }
        }
    }
}

pub fn handle_stop(app: &mut App) {
    debug!("handle_stop called");
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            match app.playback_state {
                PlaybackState::Playing => {
                    sink.stop();
                    sink.empty();
                    app.playback_state = PlaybackState::Stopped;
                    // Add elapsed time to total played time
                    if let Some(start) = app.playback_start_time.take() {
                        app.total_played += start.elapsed();
                    }
                    app.last_pause_time = None; // Clear pause time when fully stopping
                }
                PlaybackState::Paused => {
                    sink.stop();
                    sink.empty();
                    app.playback_state = PlaybackState::Stopped;
                    // When paused, time is already added to total_played
                    app.last_pause_time = None; // Clear pause time when fully stopping
                }
                PlaybackState::Stopped => {}
            }
        }
    }

    // Reset restart attempts when user manually stops playback
    app.restart_attempts = 0;
    app.last_restart_time = None;
}

pub fn handle_pause(app: &mut App) {
    debug!("handle_pause called");
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            match app.playback_state {
                PlaybackState::Playing => {
                    sink.pause();
                    app.playback_state = PlaybackState::Paused;
                    // Add elapsed time to total played time
                    if let Some(start) = app.playback_start_time.take() {
                        app.total_played += start.elapsed();
                    }
                    // Store pause time for potential resume
                    app.last_pause_time = Some(std::time::Instant::now());
                }
                _ => {}
            }
        }
    }

    // Reset restart attempts when user pauses playback
    app.restart_attempts = 0;
    app.last_restart_time = None;
}

pub fn handle_resume(app: &mut App, log_tx: &Sender<HistoryMessage>) {
    debug!("handle_resume called");
    // First, check if we need to fallback to handle_play
    let needs_fallback = match app.playback_state {
        PlaybackState::Paused => false,
        _ => true,
    };
    
    if needs_fallback {
        // If not paused, just start playing (fallback)
        handle_play(app, log_tx);
        return;
    }
    
    // We're in the paused state, so handle resuming
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            sink.play();
            app.playback_state = PlaybackState::Playing;
            // Reset start time for new playing session
            app.playback_start_time = Some(std::time::Instant::now());
            // Clear pause time as we're now playing
            app.last_pause_time = None;
        }
    }
}


pub fn handle_up(app: &mut App) {
    if let Some(selected) = app.selected_station.selected() {
        if selected > 0 {
            app.selected_station.select(Some(selected - 1));
        }
    } else if !app.stations.is_empty() {
        app.selected_station.select(Some(0));
    }
}

pub fn handle_down(app: &mut App) {
    if !app.loading {
        if let Some(selected) = app.selected_station.selected() {
            if selected < app.stations.len() - 1 {
                app.selected_station.select(Some(selected + 1));
            }
        } else if !app.stations.is_empty() {
            app.selected_station.select(Some(0));
        }
    }
}

pub fn handle_volume_up(app: &mut App) {
    app.volume = (app.volume + 0.05).min(2.0); // 5% increments, max 200%
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            sink.set_volume(app.volume);
        }
    }
}

pub fn handle_volume_down(app: &mut App) {
    app.volume = (app.volume - 0.05).max(0.0); // 5% decrements, min 0%
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            sink.set_volume(app.volume);
        }
    }
}

fn handle_history_scroll_down(app: &mut App) {
    if !app.history.is_empty() {
        let i = app.history_scroll_state.selected().unwrap_or(0);
        if i < app.history.len() - 1 {
            app.history_scroll_state.select(Some(i + 1));
        }
    }
}

fn handle_history_scroll_up(app: &mut App) {
    if !app.history.is_empty() {
        if let Some(i) = app.history_scroll_state.selected() {
            if i > 0 {
                app.history_scroll_state.select(Some(i - 1));
            }
        } else {
            app.history_scroll_state.select(Some(0));
        }
    }
}
