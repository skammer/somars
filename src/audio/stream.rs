//! Stream downloading and buffering
//!
//! This module handles HTTP stream downloads with ICY metadata support.

#![allow(dead_code)]

use super::types::{AudioError, AudioResult};
use icy_metadata::RequestIcyMetadata;
use reqwest::Url;
use std::time::Duration;
use stream_download::http::reqwest::Client;

/// Stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Buffer size in bytes for the downloaded compressed stream.
    pub buffer_size: usize,
    /// Prefetch bytes based on bitrate before decoding starts.
    pub prefetch_seconds: u64,
    /// Smaller startup prefetch so playback begins before the full jitter buffer fills.
    pub startup_prefetch_seconds: u64,
    /// Default bitrate if not available in ICY headers (kbps)
    pub default_bitrate: u64,
    /// Time without new network data before the downloader attempts reconnect.
    pub retry_timeout: Duration,
    /// Amount of decoded PCM to queue before handing the source to rodio.
    pub startup_buffer_seconds: u64,
    /// How long decoded PCM starvation may last before we rebuild the pipeline.
    pub stall_grace_period: Duration,
    /// Delay before rebuilding after reconnect or starvation.
    pub restart_backoff: Duration,
    /// Maximum number of restarts before giving up.
    pub max_restart_attempts: u32,
    /// Number of decoded samples per PCM chunk.
    pub pcm_chunk_samples: usize,
    /// Number of PCM chunks buffered between decoder and output.
    pub pcm_buffer_chunks: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 8 * 1024 * 1024,
            prefetch_seconds: 20,
            startup_prefetch_seconds: 3,
            default_bitrate: 128,
            retry_timeout: Duration::from_secs(15),
            startup_buffer_seconds: 1,
            stall_grace_period: Duration::from_secs(2),
            restart_backoff: Duration::from_secs(1),
            max_restart_attempts: 10,
            pcm_chunk_samples: 8192,
            pcm_buffer_chunks: 64,
        }
    }
}

impl StreamConfig {
    pub fn from_app_config(config: &crate::config::Config) -> Self {
        Self {
            buffer_size: config.audio_buffer_size_bytes,
            prefetch_seconds: config.audio_prefetch_seconds,
            startup_prefetch_seconds: config.audio_startup_prefetch_seconds,
            ..Self::default()
        }
    }

    pub fn startup_buffer_samples(&self, sample_rate: u32, channels: u16) -> usize {
        let per_second = sample_rate as usize * channels as usize;
        per_second.saturating_mul(self.startup_buffer_seconds as usize)
    }
}

/// Creates an HTTP client with ICY metadata support
pub fn create_icy_client() -> AudioResult<Client> {
    Client::builder()
        .request_icy_metadata()
        .build()
        .map_err(|e| {
            AudioError::InitializationFailed(format!("Failed to create HTTP client: {}", e))
        })
}

/// Parses bitrate from ICY headers with fallback
pub fn parse_bitrate_with_fallback(bitrate: Option<u32>, config: &StreamConfig) -> u64 {
    bitrate.map(|b| b as u64).unwrap_or(config.default_bitrate)
}

/// Calculates prefetch bytes based on bitrate
///
/// # Arguments
/// * `bitrate` - Bitrate in kilobits per second
/// * `seconds` - Number of seconds to buffer
///
/// # Returns
/// Number of bytes to prefetch
pub fn calculate_prefetch_bytes(bitrate: u64, seconds: u64) -> u64 {
    // bitrate (in kilobits) / bits per byte * bytes per kilobyte * seconds
    bitrate / 8 * 1024 * seconds
}

/// Validates and parses a URL
pub fn parse_url(url: &str) -> AudioResult<Url> {
    url.parse()
        .map_err(|e| AudioError::InvalidUrl(format!("Failed to parse URL '{}': {}", url, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_prefetch_bytes() {
        // 128 kbit/s for 5 seconds = 128 / 8 * 1024 * 5 = 81920 bytes
        assert_eq!(calculate_prefetch_bytes(128, 5), 81920);
    }

    #[test]
    fn test_parse_bitrate_with_fallback() {
        let config = StreamConfig::default();
        assert_eq!(parse_bitrate_with_fallback(Some(256), &config), 256);
        assert_eq!(parse_bitrate_with_fallback(None, &config), 128);
    }

    #[test]
    fn test_parse_url_valid() {
        let url = parse_url("http://example.com/stream.mp3").unwrap();
        assert_eq!(url.as_str(), "http://example.com/stream.mp3");
    }

    #[test]
    fn test_parse_url_invalid() {
        assert!(parse_url("not a url").is_err());
    }
}
