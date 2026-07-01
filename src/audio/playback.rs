//! Audio playback control
//!
//! This module supervises streaming playback and rebuilds the decoder path when
//! the network stalls or the download layer reconnects mid-stream.

#![allow(dead_code)]

use super::metadata::MetadataEvent;
use super::recovery::{retry_with_backoff, RecoveryConfig};
use super::stream::{
    calculate_prefetch_bytes, create_icy_client, parse_bitrate_with_fallback, parse_url,
    StreamConfig,
};
use super::types::{AudioError, AudioResult};
use crate::action::Action;
use crate::i18n::t;
use crate::{HistoryMessage, MessageType, PlaybackState};
use rodio::{Decoder, Sink, Source};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use stream_download::http::HttpStream;
use stream_download::storage::bounded::BoundedStorageProvider;
use stream_download::storage::memory::MemoryStorageProvider;
use stream_download::{Settings, StreamDownload};
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
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        sink.set_volume(level.clamp(0.0, 2.0));
        Ok(())
    }

    /// Get current volume
    pub fn volume(&self) -> AudioResult<f32> {
        Ok(self.volume)
    }

    /// Pause playback
    pub fn pause(&self) -> AudioResult<()> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        sink.pause();
        Ok(())
    }

    /// Resume playback
    pub fn resume(&self) -> AudioResult<()> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        sink.play();
        Ok(())
    }

    /// Stop playback and clear queue
    pub fn stop(&self) -> AudioResult<()> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        sink.stop();
        sink.clear();
        Ok(())
    }

    /// Check if sink is empty
    pub fn is_empty(&self) -> AudioResult<bool> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        Ok(sink.empty())
    }

    /// Get queue length
    pub fn len(&self) -> AudioResult<usize> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        Ok(sink.len())
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> AudioResult<bool> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        Ok(!sink.is_paused())
    }

    /// Get current playback position
    pub fn position(&self) -> AudioResult<std::time::Duration> {
        let sink = self.sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
        Ok(sink.get_pos())
    }
}

#[derive(Debug)]
struct PlaybackBufferStats {
    queued_samples: AtomicUsize,
    starving: AtomicBool,
    finished: AtomicBool,
}

impl PlaybackBufferStats {
    fn new() -> Self {
        Self {
            queued_samples: AtomicUsize::new(0),
            starving: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }
    }
}

struct BufferedStreamSource {
    rx: Receiver<Vec<f32>>,
    current_chunk: Vec<f32>,
    current_index: usize,
    channels: rodio::ChannelCount,
    sample_rate: rodio::SampleRate,
    stats: Arc<PlaybackBufferStats>,
}

impl BufferedStreamSource {
    fn from_source<S>(source: S, config: &StreamConfig) -> (Self, Arc<PlaybackBufferStats>)
    where
        S: Source + Send + 'static,
    {
        let stats = Arc::new(PlaybackBufferStats::new());
        let (tx, rx) = sync_channel(config.pcm_buffer_chunks.max(1));
        let channels = source.channels();
        let sample_rate = source.sample_rate();
        let stats_for_thread = stats.clone();
        let chunk_size = config.pcm_chunk_samples.max(channels as usize * 1024);

        thread::spawn(move || produce_samples(source, tx, stats_for_thread, chunk_size));

        (
            Self {
                rx,
                current_chunk: Vec::new(),
                current_index: 0,
                channels,
                sample_rate,
                stats: stats.clone(),
            },
            stats,
        )
    }
}

impl Iterator for BufferedStreamSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_index >= self.current_chunk.len() {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    self.stats
                        .queued_samples
                        .fetch_sub(chunk.len(), Ordering::SeqCst);
                    self.current_chunk = chunk;
                    self.current_index = 0;
                    self.stats.starving.store(false, Ordering::SeqCst);
                }
                Err(TryRecvError::Empty) => {
                    if self.stats.finished.load(Ordering::SeqCst) {
                        self.stats.starving.store(false, Ordering::SeqCst);
                        return None;
                    }
                    self.stats.starving.store(true, Ordering::SeqCst);
                    return Some(0.0);
                }
                Err(TryRecvError::Disconnected) => {
                    self.stats.finished.store(true, Ordering::SeqCst);
                    self.stats.starving.store(false, Ordering::SeqCst);
                    return None;
                }
            }
        }

        let sample = self.current_chunk[self.current_index];
        self.current_index += 1;
        self.stats.starving.store(false, Ordering::SeqCst);
        Some(sample)
    }
}

impl Source for BufferedStreamSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestartReason {
    Reconnect,
    Stall,
    StreamEnded,
}

struct PreparedAttempt {
    source: BufferedStreamSource,
    stats: Arc<PlaybackBufferStats>,
    reconnect_requested: Arc<AtomicBool>,
}

/// Start playback of a station in a supervised task.
pub fn start_playback(
    station: crate::station::Station,
    sink: Arc<Mutex<Sink>>,
    metadata_tx: mpsc::Sender<MetadataEvent>,
    log_tx: mpsc::Sender<HistoryMessage>,
    action_tx: mpsc::UnboundedSender<Action>,
    volume: f32,
    config: StreamConfig,
) -> tokio::task::JoinHandle<AudioResult<()>> {
    tokio::spawn(async move {
        let station_url = station.url.clone();
        let station_title = station.title.clone();
        let mut restart_attempts = 0;

        loop {
            let prepared = match prepare_attempt(
                &station_url,
                &station_title,
                metadata_tx.clone(),
                log_tx.clone(),
                config.clone(),
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    if try_restart_after_error(
                        &log_tx,
                        &action_tx,
                        &config,
                        &mut restart_attempts,
                        error.clone(),
                    )
                    .await?
                    {
                        continue;
                    }
                    return Err(error);
                }
            };

            let startup_samples = config
                .startup_buffer_samples(prepared.source.sample_rate(), prepared.source.channels());

            if let Err(error) = wait_for_startup_buffer(
                &prepared.stats,
                startup_samples,
                &prepared.reconnect_requested,
            )
            .await
            {
                if try_restart_after_error(
                    &log_tx,
                    &action_tx,
                    &config,
                    &mut restart_attempts,
                    error.clone(),
                )
                .await?
                {
                    continue;
                }
                return Err(error);
            }

            {
                let sink = sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
                sink.stop();
                sink.append(prepared.source);
                sink.set_volume(volume);
                sink.play();
            }

            let _ = send_log(&log_tx, t("playback-started"), MessageType::System).await;
            let _ = log_tx.send(clear_station_loading_message()).await;

            let reason = match monitor_playback_attempt(
                &sink,
                &prepared.stats,
                &prepared.reconnect_requested,
                &config,
            )
            .await
            {
                Ok(reason) => reason,
                Err(error) => {
                    if try_restart_after_error(
                        &log_tx,
                        &action_tx,
                        &config,
                        &mut restart_attempts,
                        error.clone(),
                    )
                    .await?
                    {
                        continue;
                    }
                    return Err(error);
                }
            };

            match reason {
                Some(reason) => {
                    restart_attempts += 1;
                    if restart_attempts > config.max_restart_attempts {
                        let error = AudioError::AudioUnderrun;
                        let _ = action_tx.send(Action::SetPlaybackState(PlaybackState::Stopped));
                        let _ = action_tx.send(Action::Error(error.to_string()));
                        return Err(error);
                    }

                    let message = match reason {
                        RestartReason::Reconnect => {
                            "Stream reconnected; rebuilding decoder and rebuffering..."
                        }
                        RestartReason::Stall => "Playback buffer starved; rebuffering stream...",
                        RestartReason::StreamEnded => "Stream ended unexpectedly; reconnecting...",
                    };
                    let _ = send_log(&log_tx, message.to_string(), MessageType::Background).await;
                    tokio::time::sleep(config.restart_backoff).await;
                }
                None => {
                    let _ = action_tx.send(Action::SetPlaybackState(PlaybackState::Stopped));
                    return Ok(());
                }
            }
        }
    })
}

async fn try_restart_after_error(
    log_tx: &mpsc::Sender<HistoryMessage>,
    action_tx: &mpsc::UnboundedSender<Action>,
    config: &StreamConfig,
    restart_attempts: &mut u32,
    error: AudioError,
) -> AudioResult<bool> {
    if error.is_retryable() && *restart_attempts < config.max_restart_attempts {
        *restart_attempts += 1;
        let _ = send_log(
            log_tx,
            format!(
                "Recoverable playback error: {}. Restarting stream...",
                error
            ),
            MessageType::Background,
        )
        .await;
        tokio::time::sleep(config.restart_backoff).await;
        return Ok(true);
    }

    let _ = action_tx.send(Action::SetPlaybackState(PlaybackState::Stopped));
    let _ = action_tx.send(Action::Error(error.to_string()));
    Ok(false)
}

async fn prepare_attempt(
    station_url: &str,
    station_title: &str,
    metadata_tx: mpsc::Sender<MetadataEvent>,
    log_tx: mpsc::Sender<HistoryMessage>,
    config: StreamConfig,
) -> AudioResult<PreparedAttempt> {
    let station_url = station_url.to_string();
    let station_title = station_title.to_string();
    let log_tx_for_stream = log_tx.clone();
    let config_for_connect = config.clone();

    let (stream, icy_headers, bitrate) = retry_with_backoff(
        || {
            let station_url = station_url.clone();
            let log_tx = log_tx_for_stream.clone();
            let config = config_for_connect.clone();
            async move {
                let client = create_icy_client()?;
                let url = parse_url(&station_url)?;
                let stream = HttpStream::new(client, url).await.map_err(|e| {
                    AudioError::StreamRetryable(format!("Failed to create HTTP stream: {}", e))
                })?;
                let icy_headers = icy_metadata::IcyHeaders::parse_from_headers(stream.headers());
                let bitrate = parse_bitrate_with_fallback(icy_headers.bitrate(), &config);

                let _ = log_tx
                    .send(HistoryMessage {
                        message: t("bit-rate").replace("{$rate}", &format!("{:?}", bitrate)),
                        message_type: MessageType::System,
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                    })
                    .await;

                Ok((stream, icy_headers, bitrate))
            }
        },
        RecoveryConfig {
            max_retries: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        },
    )
    .await?;

    let prefetch_bytes = calculate_prefetch_bytes(bitrate, config.startup_prefetch_seconds);
    let storage_size = config
        .buffer_size
        .max((prefetch_bytes as usize).saturating_mul(2))
        .max(512 * 1024);
    let reconnect_requested = Arc::new(AtomicBool::new(false));
    let reconnect_signal = reconnect_requested.clone();

    let reader = StreamDownload::from_stream(
        stream,
        BoundedStorageProvider::new(
            MemoryStorageProvider,
            NonZeroUsize::new(storage_size).expect("storage_size is clamped to non-zero"),
        ),
        Settings::default()
            .prefetch_bytes(prefetch_bytes)
            .retry_timeout(config.retry_timeout)
            .on_reconnect(move |_stream, cancellation_token| {
                reconnect_signal.store(true, Ordering::SeqCst);
                cancellation_token.cancel();
            }),
    )
    .await
    .map_err(|e| AudioError::DecodeError(format!("Failed to create stream reader: {}", e)))?;

    let _ = send_log(&log_tx, t("got-response"), MessageType::Background).await;

    let (inner_metadata_tx, mut metadata_rx) = mpsc::channel::<String>(32);
    let log_tx_for_metadata = log_tx.clone();
    let station_title_for_metadata = station_title.clone();
    tokio::spawn(async move {
        while let Some(title) = metadata_rx.recv().await {
            let _ = metadata_tx
                .send(MetadataEvent::Track {
                    station: station_title_for_metadata.clone(),
                    title: title.clone(),
                })
                .await;
            let _ = log_tx_for_metadata
                .send(HistoryMessage {
                    message: format!("{} :: {}", station_title_for_metadata, title),
                    message_type: MessageType::Playback,
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                })
                .await;
        }
    });

    let config_for_decoder = config.clone();
    let source_result = tokio::task::spawn_blocking(move || {
        let decoder = Decoder::new(icy_metadata::IcyMetadataReader::new(
            reader,
            icy_headers.metadata_interval(),
            move |metadata| {
                if let Ok(metadata) = metadata {
                    if let Some(title) = metadata.stream_title() {
                        let _ = inner_metadata_tx.blocking_send(title.to_string());
                    }
                }
            },
        ))
        .map_err(|e| AudioError::DecodeError(format!("Failed to construct decoder: {}", e)))?;

        Ok::<_, AudioError>(BufferedStreamSource::from_source(
            decoder,
            &config_for_decoder,
        ))
    })
    .await
    .map_err(|e| AudioError::DecodeError(e.to_string()))?;

    let (source, stats) = source_result?;

    Ok(PreparedAttempt {
        source,
        stats,
        reconnect_requested,
    })
}

async fn wait_for_startup_buffer(
    stats: &PlaybackBufferStats,
    startup_samples: usize,
    reconnect_requested: &AtomicBool,
) -> AudioResult<()> {
    let target = startup_samples.max(1);
    loop {
        if reconnect_requested.load(Ordering::SeqCst) {
            return Err(AudioError::AudioUnderrun);
        }

        let queued = stats.queued_samples.load(Ordering::SeqCst);
        if queued >= target || (stats.finished.load(Ordering::SeqCst) && queued > 0) {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn monitor_playback_attempt(
    sink: &Arc<Mutex<Sink>>,
    stats: &PlaybackBufferStats,
    reconnect_requested: &AtomicBool,
    config: &StreamConfig,
) -> AudioResult<Option<RestartReason>> {
    let mut starving_since = None;

    loop {
        let paused = {
            let sink = sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
            sink.is_paused()
        };

        if reconnect_requested.load(Ordering::SeqCst) {
            reset_sink(sink)?;
            return Ok(Some(RestartReason::Reconnect));
        }

        if !paused && stats.starving.load(Ordering::SeqCst) {
            let started = starving_since.get_or_insert_with(std::time::Instant::now);
            if started.elapsed() >= config.stall_grace_period {
                reset_sink(sink)?;
                return Ok(Some(RestartReason::Stall));
            }
        } else {
            starving_since = None;
        }

        let queued = stats.queued_samples.load(Ordering::SeqCst);
        let finished = stats.finished.load(Ordering::SeqCst);
        if finished && queued == 0 {
            reset_sink(sink)?;
            return Ok(Some(RestartReason::StreamEnded));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn produce_samples<S>(
    source: S,
    tx: SyncSender<Vec<f32>>,
    stats: Arc<PlaybackBufferStats>,
    chunk_size: usize,
) where
    S: Source + Send + 'static,
{
    let mut chunk = Vec::with_capacity(chunk_size);

    for sample in source {
        chunk.push(sample);
        if chunk.len() >= chunk_size && !send_chunk(&tx, &stats, &mut chunk, chunk_size) {
            return;
        }
    }

    if !chunk.is_empty() {
        let len = chunk.len();
        stats.queued_samples.fetch_add(len, Ordering::SeqCst);
        if tx.send(std::mem::take(&mut chunk)).is_err() {
            stats.queued_samples.fetch_sub(len, Ordering::SeqCst);
            return;
        }
    }

    stats.finished.store(true, Ordering::SeqCst);
    stats.starving.store(false, Ordering::SeqCst);
}

fn send_chunk(
    tx: &SyncSender<Vec<f32>>,
    stats: &PlaybackBufferStats,
    chunk: &mut Vec<f32>,
    chunk_size: usize,
) -> bool {
    let len = chunk.len();
    stats.queued_samples.fetch_add(len, Ordering::SeqCst);
    if tx.send(std::mem::take(chunk)).is_err() {
        stats.queued_samples.fetch_sub(len, Ordering::SeqCst);
        return false;
    }
    *chunk = Vec::with_capacity(chunk_size);
    true
}

fn reset_sink(sink: &Arc<Mutex<Sink>>) -> AudioResult<()> {
    let sink = sink.lock().map_err(|_| AudioError::SinkPoisoned)?;
    sink.stop();
    Ok(())
}

async fn send_log(
    log_tx: &mpsc::Sender<HistoryMessage>,
    message: String,
    message_type: MessageType,
) -> Result<(), mpsc::error::SendError<HistoryMessage>> {
    log_tx
        .send(HistoryMessage {
            message,
            message_type,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        })
        .await
}

fn clear_station_loading_message() -> HistoryMessage {
    HistoryMessage {
        message: "CLEAR_STATION_LOADING".to_string(),
        message_type: MessageType::Background,
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
    }
}
