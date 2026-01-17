//! Audio playback control
//!
//! This module handles the actual audio playback operations.

#![allow(dead_code)]

use super::metadata::MetadataEvent;
use super::stream::{create_icy_client, calculate_prefetch_bytes, parse_bitrate_with_fallback, parse_url, StreamConfig};
use super::types::{AudioError, AudioResult};
use super::recovery::{RecoveryConfig, retry_with_backoff};
use crate::i18n::t;
use crate::MessageType;
use rodio::{Decoder, Sink};
use std::sync::Arc;
use std::sync::Mutex;
use stream_download::http::HttpStream;
use stream_download::StreamDownload;
use stream_download::storage::bounded::BoundedStorageProvider;
use stream_download::storage::memory::MemoryStorageProvider;
use tokio::sync::mpsc;

/// Handle for managing an active audio playback
pub struct PlaybackHandle {
    pub sink: Arc<Mutex<Sink>>,
    pub volume: f32,
}

impl PlaybackHandle {
    /// Create a new playback handle
    pub fn new(sink: Arc<Mutex<Sink>>, volume: f32) -> Self {
        Self { sink, volume }
    }

    /// Set the volume
    pub fn set_volume(&self, level: f32) -> AudioResult<()> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        sink.set_volume(level.clamp(0.0, 2.0));
        Ok(())
    }

    /// Get current volume
    pub fn volume(&self) -> AudioResult<f32> {
        // Note: rodio's Sink doesn't expose get_volume, so we track it separately
        Ok(self.volume)
    }

    /// Pause playback
    pub fn pause(&self) -> AudioResult<()> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        sink.pause();
        Ok(())
    }

    /// Resume playback
    pub fn resume(&self) -> AudioResult<()> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        sink.play();
        Ok(())
    }

    /// Stop playback and clear queue
    pub fn stop(&self) -> AudioResult<()> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        sink.stop();
        sink.empty();
        Ok(())
    }

    /// Check if sink is empty
    pub fn is_empty(&self) -> AudioResult<bool> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        Ok(sink.empty())
    }

    /// Get queue length
    pub fn len(&self) -> AudioResult<usize> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        Ok(sink.len())
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> AudioResult<bool> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        Ok(!sink.is_paused())
    }

    /// Get current playback position
    pub fn position(&self) -> AudioResult<std::time::Duration> {
        let sink = self.sink.lock()
            .map_err(|_| AudioError::SinkPoisoned)?;
        Ok(sink.get_pos())
    }
}

/// Start playback of a station
///
/// # Arguments
/// * `station` - The station to play
/// * `sink` - The audio sink
/// * `metadata_tx` - Channel for metadata events
/// * `log_tx` - Channel for log messages
/// * `volume` - Initial volume level
///
/// # Returns
/// A tokio task JoinHandle for the playback task
///
/// # Error Recovery
/// This function will automatically retry transient network failures using
/// exponential backoff. Permanent errors (like invalid URLs) are returned
/// immediately without retrying.
pub async fn start_playback(
    station: &crate::station::Station,
    sink: Arc<Mutex<Sink>>,
    metadata_tx: mpsc::Sender<MetadataEvent>,
    log_tx: mpsc::Sender<crate::HistoryMessage>,
    volume: f32,
) -> AudioResult<tokio::task::JoinHandle<AudioResult<()>>> {
    use std::num::NonZeroUsize;

    let station_url = station.url.clone();
    let station_title = station.title.clone();
    let log_tx_for_stream = log_tx.clone();

    // Use retry logic for stream connection
    let (stream, icy_headers, bitrate) = retry_with_backoff(
        || {
            let station_url = station_url.clone();
            let log_tx = log_tx_for_stream.clone();
            async move {
                // Create HTTP client with ICY metadata support
                let client = create_icy_client()?;

                // Parse URL
                let url = parse_url(&station_url)?;

                // Create stream
                let stream = HttpStream::new(client, url).await
                    .map_err(|e| AudioError::StreamRetryable(format!("Failed to create HTTP stream: {}", e)))?;

                // Parse ICY headers
                let icy_headers = icy_metadata::IcyHeaders::parse_from_headers(stream.headers());

                // Calculate prefetch bytes
                let config = StreamConfig::default();
                let bitrate = parse_bitrate_with_fallback(icy_headers.bitrate(), &config);

                // Send log about bitrate
                let _ = log_tx.send(crate::HistoryMessage {
                    message: t("bit-rate").replace("{$rate}", &format!("{:?}", bitrate)),
                    message_type: MessageType::System,
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                }).await;

                Ok((stream, icy_headers, bitrate))
            }
        },
        RecoveryConfig {
            max_retries: 5,
            initial_backoff: std::time::Duration::from_millis(500),
            max_backoff: std::time::Duration::from_secs(10),
            backoff_multiplier: 2.0,
        },
    ).await?;

    let prefetch_bytes = calculate_prefetch_bytes(bitrate, StreamConfig::default().prefetch_seconds);

    // Create stream download reader
    let reader = StreamDownload::from_stream(
        stream,
        BoundedStorageProvider::new(
            MemoryStorageProvider,
            NonZeroUsize::new(1024 * 1024)
                .expect("1024 * 1024 is guaranteed to be non-zero"),
        ),
        stream_download::Settings::default().prefetch_bytes(prefetch_bytes),
    )
    .await
    .map_err(|e| AudioError::DecodeError(format!("Failed to create stream reader: {}", e)))?;

    // Send log about getting response
    let _ = log_tx.send(crate::HistoryMessage {
        message: t("got-response"),
        message_type: MessageType::Background,
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
    }).await;

    // Clone log_tx for use in the async block
    let log_tx_clone = log_tx.clone();

    // Spawn playback task
    let handle = tokio::spawn(async move {
        // Create channel for metadata updates
        let (inner_metadata_tx, mut metadata_rx) = mpsc::channel(32);

        // Spawn decoder in blocking thread
        let decoder_result = tokio::task::spawn_blocking(move || {
            Decoder::new(icy_metadata::IcyMetadataReader::new(
                reader,
                icy_headers.metadata_interval(),
                move |metadata| {
                    if let Ok(metadata) = metadata {
                        if let Some(title) = metadata.stream_title() {
                            let _ = inner_metadata_tx.blocking_send(title.to_string());
                        }
                    }
                }
            ))
        }).await;

        let decoder_result = match decoder_result {
            Ok(result) => result,
            Err(join_error) => {
                let _ = log_tx_clone.send(crate::HistoryMessage {
                    message: t("failed-decoder-construction").replace("{$error}", &join_error.to_string()),
                    message_type: MessageType::Error,
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                }).await;
                return Err(AudioError::DecodeError(join_error.to_string()));
            }
        };

        // Clone for metadata task
        let log_tx_for_metadata = log_tx_clone.clone();
        let station_title_for_metadata = station_title.clone();

        // Spawn metadata handler task
        tokio::spawn(async move {
            while let Some(title) = metadata_rx.recv().await {
                let _ = metadata_tx.send(MetadataEvent::Track {
                    station: station_title_for_metadata.clone(),
                    title: title.clone(),
                }).await;

                // Also send to history log
                let _ = log_tx_for_metadata.send(crate::HistoryMessage {
                    message: format!("{} :: {}", station_title_for_metadata, title),
                    message_type: MessageType::Playback,
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                }).await;
            }
        });

        // Start playback
        if let Ok(audio_decoder) = decoder_result {
            // Scope to ensure MutexGuard is dropped before await
            let sink_op_result = {
                let lock_result = sink.lock();
                match lock_result {
                    Ok(locked_sink) => {
                        locked_sink.append(audio_decoder);
                        locked_sink.set_volume(volume);
                        locked_sink.play();
                        Ok(())
                    }
                    Err(_) => Err(AudioError::SinkPoisoned),
                }
            };

            // Handle any lock errors
            if let Err(e) = sink_op_result {
                let _ = log_tx_clone.send(crate::HistoryMessage {
                    message: format!("Failed to lock audio sink: {}", e),
                    message_type: MessageType::Error,
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                }).await;
                return Err(e);
            }

            let _ = log_tx_clone.send(crate::HistoryMessage {
                message: t("playback-started"),
                message_type: MessageType::System,
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            }).await;

            Ok(())
        } else {
            let _ = log_tx_clone.send(crate::HistoryMessage {
                message: t("failed-decoder-construction"),
                message_type: MessageType::Error,
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            }).await;
            Err(AudioError::DecodeError("Failed to construct decoder".to_string()))
        }
    });

    Ok(handle)
}
