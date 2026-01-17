//! Stream downloading and buffering
//!
//! This module handles HTTP stream downloads with ICY metadata support.

use icy_metadata::RequestIcyMetadata;
use super::types::{AudioError, AudioResult};
use reqwest::Url;
use stream_download::http::reqwest::Client;

/// Stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Buffer size in bytes (default: 1MB)
    pub buffer_size: u64,
    /// Prefetch bytes based on bitrate (5 seconds worth)
    pub prefetch_seconds: u64,
    /// Default bitrate if not available in ICY headers (kbps)
    pub default_bitrate: u64,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1024 * 1024, // 1MB
            prefetch_seconds: 5,
            default_bitrate: 128,
        }
    }
}

/// Creates an HTTP client with ICY metadata support
pub fn create_icy_client() -> AudioResult<Client> {
    Client::builder()
        .request_icy_metadata()
        .build()
        .map_err(|e| AudioError::InitializationFailed(format!("Failed to create HTTP client: {}", e)))
}

/// Parses bitrate from ICY headers with fallback
pub fn parse_bitrate_with_fallback(
    bitrate: Option<u32>,
    config: &StreamConfig,
) -> u64 {
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
