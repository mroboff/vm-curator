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

    // === Multi-GPU Passthrough ===
    /// Enable multi-GPU passthrough features in the UI
    pub enable_multi_gpu_passthrough: bool,
    /// Default IVSHMEM size in MB for Looking Glass
    pub default_ivshmem_size_mb: u32,
    /// Show GPU passthrough warnings
    pub show_gpu_warnings: bool,

    // === Single GPU Passthrough ===
    /// Enable single GPU passthrough options (for systems with only one GPU)
    pub single_gpu_enabled: bool,
    /// Experimental: Auto switch to TTY and back (requires additional setup)
    pub single_gpu_auto_tty: bool,
    /// Override auto-detected display manager (gdm, sddm, lightdm)
    pub single_gpu_dm_override: Option<String>,
    /// Path to Looking Glass client executable
    pub looking_glass_client_path: Option<PathBuf>,
    /// Auto-launch Looking Glass client when VM starts
    pub looking_glass_auto_launch: bool,
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

            // Multi-GPU Passthrough
            enable_multi_gpu_passthrough: false,
            default_ivshmem_size_mb: 64,
            show_gpu_warnings: true,

            // Single GPU Passthrough
            single_gpu_enabled: false,
            single_gpu_auto_tty: false,
            single_gpu_dm_override: None,
            looking_glass_client_path: None,
            looking_glass_auto_launch: true,
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
