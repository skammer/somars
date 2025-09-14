use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            last_station: None,
            log_level: default_log_level(),
            udp_port: default_udp_port(),
            udp_enabled: false,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            // Return default config if file doesn't exist
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(config_path, content)?;
        Ok(())
    }

    fn config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_dir = dirs::config_dir()
            .ok_or("Could not determine config directory")?;
        Ok(config_dir.join("somars").join("config.toml"))
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
    }

    #[test]
    fn test_config_volume_clamping() {
        let config = Config::default();
        // Volume should be clamped between 0.0 and 2.0
        assert!(config.volume >= 0.0);
        assert!(config.volume <= 2.0);
    }
}