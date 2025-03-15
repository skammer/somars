use crate::{App, MessageType, PlaybackState, HistoryMessage};
use crossterm::event::KeyCode;
use stream_download::http::reqwest::Client as StreamClient;
use icy_metadata::RequestIcyMetadata;
use tokio::sync::mpsc::Sender;
use std::time::Instant;

pub fn handle_key_event(
    key: KeyCode, 
    app: &mut App, 
    log_tx: &Sender<HistoryMessage>,
    _last_tick: &mut Instant
) -> bool {
    match key {
        KeyCode::Char('q') => {
            app.should_quit = true;
            true
        },
        KeyCode::Enter => {
            handle_play(app, log_tx);
            true
        },
        KeyCode::Char('s') => {
            handle_stop(app, false);
            true
        },
        KeyCode::Char(' ') => {
            // handle_stop(app, true);
            handle_pause(app);
            true
        },
        KeyCode::Up => {
            handle_up(app);
            true
        },
        KeyCode::Down => {
            handle_down(app);
            true
        },
        KeyCode::Char('+') | KeyCode::Char('=') => {
            handle_volume_up(app);
            true
        },
        KeyCode::Char('-') => {
            handle_volume_down(app);
            true
        },
        KeyCode::Char('?') => {
            app.show_help = !app.show_help;
            true
        },
        KeyCode::Char('k') => {
            handle_history_scroll_down(app);
            true
        },
        KeyCode::Char('j') => {
            handle_history_scroll_up(app);
            true
        },
        _ => false
    }
}

pub fn handle_play(app: &mut App, log_tx: &Sender<HistoryMessage>) {
    if let Some(index) = app.selected_station.selected() {
        if let Some(station) = app.stations.get(index).cloned() {
            if let Some(original_sink) = &app.sink {
                app.active_station = Some(index);
                app.playback_start_time = Some(std::time::Instant::now());
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
                let station_url = station.url.clone();
                let station_title = station.title.clone();
                let station_title_error = station_title.clone();
                let volume = app.volume;

                let handle: tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> = tokio::spawn(async move {
                    // Spawn a new task to handle audio playback
                    let add_log = {
                        let log_tx_clone = log_tx_clone.clone();
                        move |msg: String, msg_type: MessageType| {
                            let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
                            let log_tx_clone = log_tx_clone.clone();
                            async move {
                                let history_message = HistoryMessage {
                                    message: msg,
                                    message_type: msg_type,
                                    timestamp,
                                };
                                let _ = log_tx_clone.send(history_message).await;
                            }
                        }
                    };

                    add_log(format!("Initializing stream from: {}", &station_url), MessageType::System).await;
                    // We need to add a header to tell the Icecast server that we can parse the metadata embedded
                    // within the stream itself.
                    let client = StreamClient::builder().request_icy_metadata().build()?;

                    let stream = stream_download::http::HttpStream::new(client, station_url.to_string().parse()?).await?;

                    let icy_headers = icy_metadata::IcyHeaders::parse_from_headers(stream.headers());

                    // buffer 10 seconds of audio
                    // bitrate (in kilobits) / bits per byte * bytes per kilobyte * 10 seconds
                    let prefetch_bytes = icy_headers.bitrate().unwrap() / 8 * 1024 * 10;

                    let reader = match stream_download::StreamDownload::from_stream(
                        stream,
                        stream_download::storage::bounded::BoundedStorageProvider::new(
                            stream_download::storage::memory::MemoryStorageProvider,
                            std::num::NonZeroUsize::new(512 * 1024).unwrap(),
                        ),
                        stream_download::Settings::default().prefetch_bytes(prefetch_bytes as u64),
                    )
                    .await {
                        Ok(reader) => {
                            add_log("Got response, starting stream...".to_string(), MessageType::Background).await;
                            Ok(reader)
                        },
                        Err(e) => {
                            add_log(format!("Error: {}", e), MessageType::Error).await;
                            Err(e)
                        }
                    };

                    add_log(format!("Bit rate: {:?}kbps", icy_headers.bitrate().unwrap()), MessageType::System).await;

                    // Start new playback
                    let playback_success = match reader {
                        Ok(reader) => {
                            // Clone add_log for use in the metadata handler
                            let _add_log_clone = add_log.clone();

                            // Create a channel for metadata updates
                            let (metadata_tx, mut metadata_rx) = tokio::sync::mpsc::channel(32);

                            let decoder = tokio::task::spawn_blocking(move || {
                                rodio::Decoder::new_mp3(icy_metadata::IcyMetadataReader::new(
                                    reader,
                                    icy_headers.metadata_interval(),
                                    move |metadata| {
                                        if let Ok(metadata) = metadata {
                                            if let Some(title) = metadata.stream_title() {
                                                let _ = metadata_tx.blocking_send(title.to_string());
                                            }
                                        }
                                    }
                                ))
                            }).await?;

                            // Spawn a task to handle metadata updates
                            tokio::spawn({
                                let add_log = add_log.clone();
                                async move {
                                    while let Some(title) = metadata_rx.recv().await {
                                        add_log(format!("{} :: {}", station_title, title), MessageType::Playback).await;
                                    }
                                }
                            });

                            // Start playback with the new decoder
                            {
                                let locked_sink = sink.lock().unwrap();
                                locked_sink.append(decoder.unwrap());
                                locked_sink.set_volume(volume);
                                locked_sink.play();
                            }
                            true
                        },
                        Err(_) => {
                            let _ = add_log("Failed to start playback".to_string(), MessageType::Error).await;
                            false
                        },
                    };

                    if playback_success {
                        add_log("Playback started".to_string(), MessageType::System).await;
                    } else {
                        add_log("Failed to lock audio sink".to_string(), MessageType::Error).await;
                    }

                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                });

                let log_tx_clone = log_tx.clone();
                app.playback_state = PlaybackState::Playing;

                tokio::spawn(async move {
                    let log_tx_clone_2 = log_tx_clone.clone();
                    if let Err(e) = handle.await {
                        let _ = log_tx_clone_2.send(HistoryMessage {
                            message: format!("Playback error: {}", e),
                            message_type: MessageType::Error,
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        }).await;
                    } else {
                        let _ = log_tx_clone_2.send(HistoryMessage {
                            message: format!("Starting playback of {}", &station_title_error),
                            message_type: MessageType::System,
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        }).await;
                        let _ = log_tx_clone_2.send(HistoryMessage {
                            message: "Connecting to stream...".to_string(),
                            message_type: MessageType::System,
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        }).await;
                    }
                });
            }
        }
    }
}

pub fn handle_stop(app: &mut App, soft_stop: bool) {
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            // Keep total_played duration but reset timing state
            // app.playback_start_time = None;
            // app.last_pause_time = None;
            //

            match app.playback_state {
                PlaybackState::Playing => {
                    sink.stop();
                    sink.empty();
                    app.playback_state = PlaybackState::Stopped;
                    if let Some(start) = app.playback_start_time.take() {
                        app.total_played += start.elapsed();
                    }
                    app.last_pause_time = Some(std::time::Instant::now());
                }
                PlaybackState::Stopped => {
                    if soft_stop {
                        sink.play();
                        app.playback_state = PlaybackState::Playing;
                        app.playback_start_time = Some(std::time::Instant::now());
                    }
                }
                PlaybackState::Paused => {}
            }
        }
    }
}

pub fn handle_pause(app: &mut App) {
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            match app.playback_state {
                PlaybackState::Playing => {
                    sink.pause();
                    app.playback_state = PlaybackState::Paused;
                    if let Some(start) = app.playback_start_time.take() {
                        app.total_played += start.elapsed();
                    }
                    app.last_pause_time = Some(std::time::Instant::now());
                }
                PlaybackState::Paused => {
                    sink.play();
                    app.playback_state = PlaybackState::Playing;
                    app.playback_start_time = Some(std::time::Instant::now());
                }
                PlaybackState::Stopped => {}
            }
        }
    }
}

fn handle_up(app: &mut App) {
    if let Some(selected) = app.selected_station.selected() {
        if selected > 0 {
            app.selected_station.select(Some(selected - 1));
        }
    } else if !app.stations.is_empty() {
        app.selected_station.select(Some(0));
    }
}

fn handle_down(app: &mut App) {
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
    if app.volume < 2.0 {
        app.volume += 0.1;
        if let Some(sink) = &app.sink {
            if let Ok(sink) = sink.lock() {
                sink.set_volume(app.volume);
            }
        }
    }
}

pub fn handle_volume_down(app: &mut App) {
    if app.volume > 0.0 {
        app.volume -= 0.1;
        if let Some(sink) = &app.sink {
            if let Ok(sink) = sink.lock() {
                sink.set_volume(app.volume);
            }
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
