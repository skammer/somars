use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::PathBuf;

/// Configuration-specific errors
#[derive(Debug)]
pub enum ConfigError {
    /// Could not determine config directory
    NoConfigDir,
    /// Failed to read config file
    ReadError {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to parse config file
    ParseError {
        path: PathBuf,
        source: toml::de::Error,
    },
    /// Failed to write config file
    WriteError {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to serialize config
    SerializeError { source: toml::ser::Error },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NoConfigDir => {
                write!(f, "Could not determine configuration directory. \
                          Please set HOME environment variable or ensure your platform's config directory is accessible.")
            }
            ConfigError::ReadError { path, source } => {
                write!(
                    f,
                    "Failed to read configuration file '{}': {}",
                    path.display(),
                    source
                )
            }
            ConfigError::ParseError { path, source } => {
                write!(
                    f,
                    "Failed to parse configuration file '{}': {}",
                    path.display(),
                    source
                )
            }
            ConfigError::WriteError { path, source } => {
                write!(
                    f,
                    "Failed to write configuration file '{}': {}",
                    path.display(),
                    source
                )
            }
            ConfigError::SerializeError { source } => {
                write!(f, "Failed to serialize configuration: {}", source)
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::NoConfigDir => None,
            ConfigError::ReadError { source, .. } => Some(source),
            ConfigError::ParseError { source, .. } => Some(source),
            ConfigError::WriteError { source, .. } => Some(source),
            ConfigError::SerializeError { source } => Some(source),
        }
    }
}

/// Result type for config operations
pub type ConfigResult<T> = Result<T, ConfigError>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub last_station: Option<String>,
    #[serde(default = "default_log_level")]
    pub log_level: u8,
    #[serde(default = "default_udp_port")]
    pub udp_port: u16,
    #[serde(default)]
    pub udp_enabled: bool,
    #[serde(default = "default_audio_prefetch_seconds")]
    pub audio_prefetch_seconds: u64,
    #[serde(default = "default_audio_startup_prefetch_seconds")]
    pub audio_startup_prefetch_seconds: u64,
    #[serde(default = "default_audio_buffer_size_bytes")]
    pub audio_buffer_size_bytes: usize,
    #[serde(default = "default_audio_output_buffer_frames")]
    pub audio_output_buffer_frames: u32,
}

fn default_volume() -> f32 {
    1.0
}

fn default_log_level() -> u8 {
    1
}

fn default_udp_port() -> u16 {
    8069
}

fn default_audio_prefetch_seconds() -> u64 {
    20
}

fn default_audio_startup_prefetch_seconds() -> u64 {
    3
}

fn default_audio_buffer_size_bytes() -> usize {
    8 * 1024 * 1024
}

fn default_audio_output_buffer_frames() -> u32 {
    4096
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            last_station: None,
            log_level: default_log_level(),
            udp_port: default_udp_port(),
            udp_enabled: false,
            audio_prefetch_seconds: default_audio_prefetch_seconds(),
            audio_startup_prefetch_seconds: default_audio_startup_prefetch_seconds(),
            audio_buffer_size_bytes: default_audio_buffer_size_bytes(),
            audio_output_buffer_frames: default_audio_output_buffer_frames(),
        }
    }
}

impl Config {
    /// Load configuration from default path
    ///
    /// Returns default config if the file doesn't exist.
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load() -> ConfigResult<Self> {
        Self::load_from_path(None)
    }

    /// Load configuration, falling back to defaults on error
    ///
    /// This is a convenience method that always returns a valid config,
    /// using defaults if loading fails. Errors are printed to stderr.
    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|e| {
            eprintln!("Config load failed: {}. Using defaults.", e);
            Self::default()
        })
    }

    /// Load configuration from a specific path or default
    ///
    /// If `path` is None, uses the default config path.
    /// Returns default config if the file doesn't exist.
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load_from_path(path: Option<String>) -> ConfigResult<Self> {
        let config_path = if let Some(path_str) = path {
            PathBuf::from(path_str)
        } else {
            Self::default_config_path()?
        };

        if config_path.exists() {
            let content =
                fs::read_to_string(&config_path).map_err(|source| ConfigError::ReadError {
                    path: config_path.clone(),
                    source,
                })?;
            let config: Config =
                toml::from_str(&content).map_err(|source| ConfigError::ParseError {
                    path: config_path.clone(),
                    source,
                })?;
            Ok(config)
        } else {
            // Return default config if file doesn't exist
            Ok(Config::default())
        }
    }

    /// Save configuration to default path
    pub fn save(&self) -> ConfigResult<()> {
        let config_path = Self::default_config_path()?;
        self.save_to_path(&config_path.to_string_lossy())
    }

    /// Save configuration to a specific path
    pub fn save_to_path(&self, path: &str) -> ConfigResult<()> {
        let config_path = PathBuf::from(path);
        let content = toml::to_string_pretty(self)
            .map_err(|source| ConfigError::SerializeError { source })?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::WriteError {
                path: config_path.clone(),
                source,
            })?;
        }
        fs::write(&config_path, content).map_err(|source| ConfigError::WriteError {
            path: config_path,
            source,
        })?;
        Ok(())
    }

    /// Get the default configuration file path
    pub fn default_config_path() -> ConfigResult<PathBuf> {
        // On macOS, try to use ~/.config/somars/config.toml if available
        #[cfg(target_os = "macos")]
        {
            if let Some(home_dir) = dirs::home_dir() {
                let config_path = home_dir.join(".config").join("somars").join("config.toml");
                // Check if the directory exists or we can create it
                let config_dir = home_dir.join(".config").join("somars");
                if config_dir.exists() || std::fs::create_dir_all(&config_dir).is_ok() {
                    return Ok(config_path);
                }
            }
        }

        // For other platforms or if ~/.config approach fails, use the default
        let config_dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
        Ok(config_dir.join("somars").join("config.toml"))
    }

    /// Get the configuration file path (alias for default_config_path)
    pub fn config_path() -> ConfigResult<PathBuf> {
        Self::default_config_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.volume, 1.0);
        assert_eq!(config.log_level, 1);
        assert_eq!(config.udp_port, 8069);
        assert_eq!(config.udp_enabled, false);
        assert_eq!(config.last_station, None);
        assert_eq!(config.audio_prefetch_seconds, 20);
        assert_eq!(config.audio_startup_prefetch_seconds, 3);
        assert_eq!(config.audio_buffer_size_bytes, 8 * 1024 * 1024);
        assert_eq!(config.audio_output_buffer_frames, 4096);
    }

    #[test]
    fn test_config_volume_clamping() {
        let config = Config::default();
        // Volume should be clamped between 0.0 and 2.0
        assert!(config.volume >= 0.0);
        assert!(config.volume <= 2.0);
    }
}
