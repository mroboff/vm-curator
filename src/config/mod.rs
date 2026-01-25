use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Path to VM library directory
    pub vm_library_path: PathBuf,
    /// Path to user metadata overrides
    pub metadata_path: PathBuf,
    /// Path to user ASCII art overrides
    pub ascii_art_path: PathBuf,
    /// Default snapshot name prefix
    pub snapshot_prefix: String,

    // === VM Creation Defaults ===
    /// Default memory for new VMs (MB)
    pub default_memory_mb: u32,
    /// Default CPU cores for new VMs
    pub default_cpu_cores: u32,
    /// Default disk size for new VMs (GB)
    pub default_disk_size_gb: u32,
    /// Default display backend (gtk, sdl, spice)
    pub default_display: String,
    /// Enable KVM acceleration by default
    pub default_enable_kvm: bool,

    // === Behavior ===
    /// Show confirmation dialog before launching VMs
    pub confirm_before_launch: bool,
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

            // VM Creation Defaults
            default_memory_mb: 4096,
            default_cpu_cores: 2,
            default_disk_size_gb: 64,
            default_display: "gtk".to_string(),
            default_enable_kvm: true,

            // Behavior
            confirm_before_launch: true,
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
}
