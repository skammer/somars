//! ICY metadata handling
//!
//! This module handles extraction of metadata from audio streams,
//! such as track titles and artist information embedded in ICY streams.

#![allow(dead_code)]

/// Metadata event extracted from audio stream
#[derive(Debug, Clone)]
pub enum MetadataEvent {
    /// New track information (title - artist format)
    Track { station: String, title: String },
    /// Bitrate information
    Bitrate(u64),
    /// Stream started
    StreamStarted(String),
    /// Stream error
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_event_creation() {
        let event = MetadataEvent::Track {
            station: "Test Station".to_string(),
            title: "Test Song".to_string(),
        };
        assert!(matches!(event, MetadataEvent::Track { .. }));
    }
}
