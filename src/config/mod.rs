use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to VM library directory
    pub vm_library_path: PathBuf,
    /// Path to user metadata overrides
    pub metadata_path: PathBuf,
    /// Path to user ASCII art overrides
    pub ascii_art_path: PathBuf,
    /// Default snapshot name prefix
    pub snapshot_prefix: String,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("vm-curator");

        Self {
            vm_library_path: home.join("vm-space"),
            metadata_path: config_dir.join("metadata"),
            ascii_art_path: config_dir.join("ascii"),
            snapshot_prefix: "snapshot".to_string(),
        }
    }
}

impl Config {
    /// Load configuration from file or create default
    pub fn load() -> Result<Self> {
        let config_path = Self::config_file_path();

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config from {:?}", config_path))?;
            toml::from_str(&content)
                .with_context(|| format!("Failed to parse config from {:?}", config_path))
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }

        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config to {:?}", config_path))?;

        Ok(())
    }

    /// Get the configuration file path
    pub fn config_file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("vm-curator")
            .join("config.toml")
    }

    /// Ensure all required directories exist
    pub fn ensure_directories(&self) -> Result<()> {
        std::fs::create_dir_all(&self.metadata_path)
            .with_context(|| format!("Failed to create metadata directory {:?}", self.metadata_path))?;
        std::fs::create_dir_all(&self.ascii_art_path)
            .with_context(|| format!("Failed to create ASCII art directory {:?}", self.ascii_art_path))?;
        Ok(())
    }
}
