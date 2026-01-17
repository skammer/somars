//! Error recovery logic for audio playback
//!
//! This module provides automatic retry logic and exponential backoff
//! for recoverable audio errors.

#![allow(dead_code)]

use super::types::{AudioError, AudioResult};
use std::time::Duration;
use tokio::time::sleep;

/// Configuration for error recovery behavior
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier (exponential)
    pub backoff_multiplier: f64,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

/// Recovery state tracker
#[derive(Debug, Clone)]
pub struct RecoveryState {
    /// Current retry attempt
    pub attempt: u32,
    /// Last error encountered
    pub last_error: Option<AudioError>,
    /// Whether recovery is exhausted
    pub exhausted: bool,
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self {
            attempt: 0,
            last_error: None,
            exhausted: false,
        }
    }
}

impl RecoveryState {
    /// Create a new recovery state
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a retry attempt
    pub fn record_attempt(&mut self, error: AudioError) {
        self.attempt = self.attempt.saturating_add(1);
        self.last_error = Some(error.clone());
        self.exhausted = self.attempt >= error.max_retries();
    }

    /// Reset the recovery state
    pub fn reset(&mut self) {
        self.attempt = 0;
        self.last_error = None;
        self.exhausted = false;
    }

    /// Check if should retry based on the error
    pub fn should_retry(&self, error: &AudioError) -> bool {
        error.is_retryable() && self.attempt < error.max_retries()
    }

    /// Calculate the backoff duration for this attempt
    pub fn backoff_duration(&self, config: &RecoveryConfig) -> Duration {
        let exponential = config.backoff_multiplier.powi(self.attempt as i32 - 1);
        let duration = config.initial_backoff.mul_f64(exponential);
        duration.min(config.max_backoff)
    }
}

/// Retry a fallible operation with exponential backoff
///
/// # Arguments
/// * `operation` - The operation to retry
/// * `config` - Recovery configuration
///
/// # Returns
/// The result of the operation, or the last error if all retries failed
///
/// # Example
/// ```no_run
/// # use somars::audio::recovery::retry_with_backoff;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let result = retry_with_backoff(
///     || async { Ok::<(), somars::audio::AudioError>(()) },
///     Default::default(),
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn retry_with_backoff<F, Fut, T>(
    mut operation: F,
    config: RecoveryConfig,
) -> AudioResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AudioResult<T>>,
{
    let mut state = RecoveryState::new();

    loop {
        match operation().await {
            Ok(result) => {
                // Operation succeeded, return result
                return Ok(result);
            }
            Err(error) => {
                state.record_attempt(error.clone());

                if !state.should_retry(&error) {
                    return Err(error);
                }

                let backoff = state.backoff_duration(&config);
                // Sleep before retry
                sleep(backoff).await;
            }
        }
    }
}

/// Classify an error based on its recoverability
pub fn classify_error(error: &AudioError) -> ErrorClass {
    if error.is_retryable() {
        ErrorClass::Transient
    } else {
        ErrorClass::Permanent
    }
}

/// Error classification for recovery purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Temporary error that can be retried
    Transient,
    /// Permanent error that should not be retried
    Permanent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_state_default() {
        let state = RecoveryState::new();
        assert_eq!(state.attempt, 0);
        assert!(state.last_error.is_none());
        assert!(!state.exhausted);
    }

    #[test]
    fn test_recovery_state_record_attempt() {
        let mut state = RecoveryState::new();
        let error = AudioError::AudioUnderrun;

        state.record_attempt(error.clone());

        assert_eq!(state.attempt, 1);
        assert_eq!(state.last_error, Some(error));
        assert!(!state.exhausted); // 1 < 10 (max retries for AudioUnderrun)
    }

    #[test]
    fn test_recovery_state_exhausted() {
        let mut state = RecoveryState::new();
        let error = AudioError::Network("test".to_string());

        // Record 3 attempts (max for Network errors)
        for _ in 0..3 {
            state.record_attempt(error.clone());
        }

        assert_eq!(state.attempt, 3);
        assert!(state.exhausted);
        assert!(!state.should_retry(&error));
    }

    #[test]
    fn test_recovery_state_reset() {
        let mut state = RecoveryState::new();
        state.record_attempt(AudioError::AudioUnderrun);

        state.reset();

        assert_eq!(state.attempt, 0);
        assert!(state.last_error.is_none());
        assert!(!state.exhausted);
    }

    #[test]
    fn test_backoff_duration() {
        let config = RecoveryConfig {
            initial_backoff: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(10),
            ..Default::default()
        };

        let mut state = RecoveryState::new();

        // Attempt 1: 100ms
        state.attempt = 1;
        assert_eq!(state.backoff_duration(&config), Duration::from_millis(100));

        // Attempt 2: 200ms
        state.attempt = 2;
        assert_eq!(state.backoff_duration(&config), Duration::from_millis(200));

        // Attempt 3: 400ms
        state.attempt = 3;
        assert_eq!(state.backoff_duration(&config), Duration::from_millis(400));

        // Large attempt should cap at max_backoff
        state.attempt = 20;
        assert_eq!(state.backoff_duration(&config), Duration::from_secs(10));
    }

    #[test]
    fn test_classify_error() {
        assert_eq!(
            classify_error(&AudioError::AudioUnderrun),
            ErrorClass::Transient
        );
        assert_eq!(
            classify_error(&AudioError::StreamRetryable("test".to_string())),
            ErrorClass::Transient
        );
        assert_eq!(
            classify_error(&AudioError::Network("test".to_string())),
            ErrorClass::Transient
        );
        assert_eq!(
            classify_error(&AudioError::StreamPermanent("test".to_string())),
            ErrorClass::Permanent
        );
        assert_eq!(
            classify_error(&AudioError::SinkPoisoned),
            ErrorClass::Permanent
        );
        assert_eq!(
            classify_error(&AudioError::InvalidUrl("test".to_string())),
            ErrorClass::Permanent
        );
    }

    #[tokio::test]
    async fn test_retry_with_backoff_success() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let result = retry_with_backoff(
            move || {
                let attempts = attempts_clone.clone();
                async move {
                    let current = attempts.fetch_add(1, Ordering::SeqCst);
                    if current < 2 {
                        Err(AudioError::Network("Temporary failure".to_string()))
                    } else {
                        Ok::<(), AudioError>(())
                    }
                }
            },
            RecoveryConfig {
                initial_backoff: Duration::from_millis(10),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_with_backoff_permanent_error() {
        let result = retry_with_backoff(
            || async {
                Err::<(), AudioError>(AudioError::InvalidUrl("bad url".to_string()))
            },
            Default::default(),
        )
        .await;

        assert!(result.is_err());
        match result {
            Err(AudioError::InvalidUrl(msg)) => assert_eq!(msg, "bad url"),
            _ => panic!("Expected InvalidUrl error"),
        }
    }

    #[tokio::test]
    async fn test_retry_with_backoff_exhausted() {
        let result = retry_with_backoff(
            || async {
                Err::<(), AudioError>(AudioError::Network("persistent".to_string()))
            },
            RecoveryConfig {
                max_retries: 2,
                initial_backoff: Duration::from_millis(10),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_err());
        match result {
            Err(AudioError::Network(msg)) => assert_eq!(msg, "persistent"),
            _ => panic!("Expected Network error"),
        }
    }
}
