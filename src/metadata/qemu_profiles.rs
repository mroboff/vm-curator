//! QEMU configuration profiles for different operating systems
//!
//! This module provides OS-specific QEMU defaults that are used
//! when creating new VMs through the creation wizard.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Embedded QEMU profiles from assets/metadata/qemu_profiles.toml
const EMBEDDED_PROFILES: &str = include_str!("../../assets/metadata/qemu_profiles.toml");

/// A QEMU configuration profile for a specific operating system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QemuProfile {
    /// Human-readable display name
    pub display_name: String,

    /// Category (windows, linux, bsd, unix, classic-mac, alternative, retro, macos)
    pub category: String,

    /// QEMU emulator command (e.g., qemu-system-x86_64)
    pub emulator: String,

    /// Default RAM in megabytes
    pub memory_mb: u32,

    /// Default CPU cores
    pub cpu_cores: u32,

    /// CPU model (host, qemu64, pentium, etc.)
    #[serde(default)]
    pub cpu_model: Option<String>,

    /// Machine type (q35, pc, etc.)
    #[serde(default)]
    pub machine: Option<String>,

    /// Graphics adapter (qxl, virtio, std, cirrus, vmware, none)
    pub vga: String,

    /// Audio devices (e.g., ["intel-hda", "hda-duplex"])
    #[serde(default)]
    pub audio: Vec<String>,

    /// Network adapter model (virtio, e1000, rtl8139, ne2k_pci, pcnet, none)
    pub network_model: String,

    /// Disk interface (virtio, ide, sata, scsi, sd)
    pub disk_interface: String,

    /// Default disk size in gigabytes
    pub disk_size_gb: u32,

    /// Enable KVM acceleration
    #[serde(default)]
    pub enable_kvm: bool,

    /// Boot in UEFI mode
    #[serde(default)]
    pub uefi: bool,

    /// Enable TPM emulation
    #[serde(default)]
    pub tpm: bool,

    /// Set RTC to local time (for Windows)
    #[serde(default)]
    pub rtc_localtime: bool,

    /// Use USB tablet for mouse
    #[serde(default)]
    pub usb_tablet: bool,

    /// Display output (gtk, sdl, spice, vnc)
    #[serde(default = "default_display")]
    pub display: String,

    /// Additional QEMU arguments
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Download URL for free/open-source OSes
    #[serde(default)]
    pub iso_url: Option<String>,

    /// Tips/notes for this OS
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_display() -> String {
    "gtk".to_string()
}

impl Default for QemuProfile {
    fn default() -> Self {
        Self {
            display_name: "Unknown OS".to_string(),
            category: "alternative".to_string(),
            emulator: "qemu-system-x86_64".to_string(),
            memory_mb: 2048,
            cpu_cores: 2,
            cpu_model: Some("host".to_string()),
            machine: Some("q35".to_string()),
            vga: "std".to_string(),
            audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
            network_model: "e1000".to_string(),
            disk_interface: "ide".to_string(),
            disk_size_gb: 32,
            enable_kvm: true,
            uefi: false,
            tpm: false,
            rtc_localtime: false,
            usb_tablet: true,
            display: "gtk".to_string(),
            extra_args: vec![],
            iso_url: None,
            notes: None,
        }
    }
}

impl QemuProfile {
    /// Check if this profile supports free ISO download
    #[allow(dead_code)]
    pub fn has_free_iso(&self) -> bool {
        self.iso_url.is_some()
    }

    /// Check if this profile uses x86 architecture
    #[allow(dead_code)]
    pub fn is_x86(&self) -> bool {
        self.emulator.contains("x86_64") || self.emulator.contains("i386")
    }

    /// Check if this profile uses 64-bit x86
    #[allow(dead_code)]
    pub fn is_x86_64(&self) -> bool {
        self.emulator.contains("x86_64")
    }

    /// Get a short summary for display in the wizard
    pub fn summary(&self) -> String {
        let mut parts = vec![];

        // Memory
        if self.memory_mb >= 1024 {
            parts.push(format!("{}GB RAM", self.memory_mb / 1024));
        } else {
            parts.push(format!("{}MB RAM", self.memory_mb));
        }

        // Disk
        parts.push(format!("{}GB", self.disk_size_gb));

        // Notable features
        if self.uefi {
            parts.push("UEFI".to_string());
        }
        if self.disk_interface == "virtio" {
            parts.push("virtio".to_string());
        }

        parts.join(", ")
    }
}

/// Store for QEMU profiles with support for user overrides
#[derive(Debug, Default)]
pub struct QemuProfileStore {
    profiles: HashMap<String, QemuProfile>,
}

impl QemuProfileStore {
    /// Create a new empty profile store
    pub fn new() -> Self {
        Self {
            profiles: HashMap::new(),
        }
    }

    /// Load the embedded profiles from compile-time data
    pub fn load_embedded() -> Self {
        let mut store = Self::new();

        match toml::from_str::<HashMap<String, QemuProfile>>(EMBEDDED_PROFILES) {
            Ok(profiles) => {
                store.profiles = profiles;
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse embedded QEMU profiles: {}", e);
            }
        }

        store
    }

    /// Load user override profiles from a file
    pub fn load_user_overrides(&mut self, path: &Path) {
        if !path.exists() {
            return;
        }

        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<HashMap<String, QemuProfile>>(&content) {
                Ok(user_profiles) => {
                    // Merge user profiles (override existing)
                    for (id, profile) in user_profiles {
                        self.profiles.insert(id, profile);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse user QEMU profiles: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Warning: Failed to read user QEMU profiles: {}", e);
            }
        }
    }

    /// Get a profile by OS ID
    pub fn get(&self, os_id: &str) -> Option<&QemuProfile> {
        self.profiles.get(os_id)
    }

    /// Get a profile by OS ID, or return the default profile
    #[allow(dead_code)]
    pub fn get_or_default(&self, os_id: &str) -> QemuProfile {
        self.profiles
            .get(os_id)
            .cloned()
            .unwrap_or_else(QemuProfile::default)
    }

    /// List all profiles
    #[allow(dead_code)]
    pub fn list_all(&self) -> Vec<(&String, &QemuProfile)> {
        let mut profiles: Vec<_> = self.profiles.iter().collect();
        profiles.sort_by(|a, b| a.1.display_name.cmp(&b.1.display_name));
        profiles
    }

    /// List profiles by category
    pub fn list_by_category(&self, category: &str) -> Vec<(&String, &QemuProfile)> {
        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .filter(|(_, p)| p.category == category)
            .collect();
        profiles.sort_by(|a, b| a.1.display_name.cmp(&b.1.display_name));
        profiles
    }

    /// Get all unique categories
    #[allow(dead_code)]
    pub fn categories(&self) -> Vec<String> {
        let mut categories: Vec<String> = self
            .profiles
            .values()
            .map(|p| p.category.clone())
            .collect();
        categories.sort();
        categories.dedup();
        categories
    }

    /// Get category display name
    pub fn category_display_name(category: &str) -> &'static str {
        match category {
            "windows" => "Windows",
            "linux" => "Linux",
            "bsd" => "BSD",
            "unix" => "Unix",
            "classic-mac" => "Classic Mac",
            "macos" => "macOS",
            "alternative" => "Alternative",
            "retro" => "Retro",
            "mobile" => "Mobile / Android",
            "infrastructure" => "Infrastructure",
            "utilities" => "Utilities",
            _ => "Other",
        }
    }

    /// Get profiles that support free ISO download
    #[allow(dead_code)]
    pub fn list_with_free_iso(&self) -> Vec<(&String, &QemuProfile)> {
        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .filter(|(_, p)| p.iso_url.is_some())
            .collect();
        profiles.sort_by(|a, b| a.1.display_name.cmp(&b.1.display_name));
        profiles
    }

    /// Get profiles that are x86/x86_64 (supported in V1.0)
    #[allow(dead_code)]
    pub fn list_x86_profiles(&self) -> Vec<(&String, &QemuProfile)> {
        let mut profiles: Vec<_> = self.profiles.iter().filter(|(_, p)| p.is_x86()).collect();
        profiles.sort_by(|a, b| a.1.display_name.cmp(&b.1.display_name));
        profiles
    }

    /// Search profiles by name
    #[allow(dead_code)]
    pub fn search(&self, query: &str) -> Vec<(&String, &QemuProfile)> {
        let query_lower = query.to_lowercase();
        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .filter(|(id, p)| {
                id.to_lowercase().contains(&query_lower)
                    || p.display_name.to_lowercase().contains(&query_lower)
            })
            .collect();
        profiles.sort_by(|a, b| a.1.display_name.cmp(&b.1.display_name));
        profiles
    }

    /// Get the count of profiles
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Check if the store is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Get a generic profile based on category
    #[allow(dead_code)]
    pub fn generic_profile_for_category(category: &str) -> &'static str {
        match category {
            "windows" => "generic-windows",
            "linux" => "generic-linux",
            "bsd" => "generic-bsd",
            _ => "generic-other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_embedded_profiles() {
        let store = QemuProfileStore::load_embedded();
        assert!(!store.is_empty(), "Should have loaded some profiles");

        // Check that some expected profiles exist
        assert!(store.get("windows-10").is_some(), "Should have Windows 10");
        assert!(store.get("linux-debian").is_some(), "Should have Debian");
        assert!(store.get("freebsd").is_some(), "Should have FreeBSD");
    }

    #[test]
    fn test_profile_summary() {
        let profile = QemuProfile {
            display_name: "Test OS".to_string(),
            memory_mb: 4096,
            disk_size_gb: 64,
            uefi: true,
            disk_interface: "virtio".to_string(),
            ..Default::default()
        };

        let summary = profile.summary();
        assert!(summary.contains("4GB RAM"));
        assert!(summary.contains("64GB"));
        assert!(summary.contains("UEFI"));
        assert!(summary.contains("virtio"));
    }

    #[test]
    fn test_categories() {
        let store = QemuProfileStore::load_embedded();
        let categories = store.categories();

        assert!(categories.contains(&"windows".to_string()));
        assert!(categories.contains(&"linux".to_string()));
        assert!(categories.contains(&"bsd".to_string()));
    }

    #[test]
    fn test_search() {
        let store = QemuProfileStore::load_embedded();

        let results = store.search("windows");
        assert!(!results.is_empty(), "Should find Windows profiles");

        let results = store.search("debian");
        assert!(!results.is_empty(), "Should find Debian profiles");
    }

    #[test]
    fn test_free_iso_profiles() {
        let store = QemuProfileStore::load_embedded();
        let free_profiles = store.list_with_free_iso();

        // Should have at least some free/open-source OSes
        assert!(
            !free_profiles.is_empty(),
            "Should have profiles with free ISOs"
        );

        // Check that a known free OS is in the list
        let has_debian = free_profiles.iter().any(|(id, _)| *id == "linux-debian");
        assert!(has_debian, "Debian should have a free ISO URL");
    }
}
