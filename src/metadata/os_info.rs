use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Information about an operating system
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OsInfo {
    /// Full display name for VM list (with trademarks, e.g., "Microsoft® Windows 95")
    #[serde(default)]
    pub display_name: Option<String>,
    /// Short name for info panel headers (e.g., "Windows 95")
    pub name: String,
    /// Publisher/developer
    pub publisher: String,
    /// Release date (YYYY-MM-DD format)
    pub release_date: String,
    /// CPU architecture
    pub architecture: String,
    /// Blurb information
    #[serde(default)]
    pub blurb: OsBlurb,
    /// Fun facts
    #[serde(default)]
    pub fun_facts: Vec<String>,
    /// Installation steps for multi-step installs
    #[serde(default)]
    pub install_steps: Vec<InstallStep>,
}

/// OS description blurbs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OsBlurb {
    /// Short description (1-2 sentences)
    pub short: String,
    /// Long description (2-5 paragraphs)
    #[serde(default)]
    pub long: String,
}

/// Installation step for multi-step VMs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallStep {
    /// Step number
    pub number: u32,
    /// Step title
    pub title: String,
    /// Instructions
    pub instructions: String,
    /// Boot mode for this step
    #[serde(default)]
    pub boot_mode: String,
    /// Hint for completion
    #[serde(default)]
    pub completion_hint: String,
}

/// Metadata store for all OS info
#[derive(Debug, Clone, Default)]
pub struct MetadataStore {
    pub entries: HashMap<String, OsInfo>,
}

impl MetadataStore {
    /// Load metadata from a directory of TOML files
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let mut store = Self::default();

        if !dir.exists() {
            return Ok(store);
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "toml").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(info) = toml::from_str::<OsInfo>(&content) {
                        let id = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        store.entries.insert(id, info);
                    }
                }
            }
        }

        Ok(store)
    }

    /// Load metadata from embedded assets
    pub fn load_embedded() -> Self {
        let mut store = Self::default();

        // Embedded default metadata
        let defaults = include_str!("../../assets/metadata/defaults.toml");
        if let Ok(entries) = toml::from_str::<HashMap<String, OsInfo>>(defaults) {
            store.entries = entries;
        }

        store
    }

    /// Get info for a VM by ID
    pub fn get(&self, id: &str) -> Option<&OsInfo> {
        self.entries.get(id)
    }

    /// Merge user overrides with embedded defaults
    pub fn merge(&mut self, overrides: MetadataStore) {
        for (id, info) in overrides.entries {
            self.entries.insert(id, info);
        }
    }
}

/// Create default OS info from VM ID
pub fn default_os_info(vm_id: &str) -> OsInfo {
    let display_name = vm_id
        .replace('-', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ");

    let (publisher, architecture) = guess_os_details(vm_id);

    OsInfo {
        display_name: None, // Will fall back to name
        name: display_name,
        publisher,
        architecture,
        release_date: String::new(),
        blurb: OsBlurb::default(),
        fun_facts: Vec::new(),
        install_steps: Vec::new(),
    }
}

/// Guess OS details from VM ID
fn guess_os_details(vm_id: &str) -> (String, String) {
    let id = vm_id.to_lowercase();

    if id.contains("windows") {
        let arch = if id.contains("11") || id.contains("10") || id.contains("8") || id.contains("7") {
            "x86_64"
        } else {
            "i386"
        };
        ("Microsoft Corporation".to_string(), arch.to_string())
    } else if id.contains("mac") || id.contains("osx") {
        let arch = if id.contains("tiger") || id.contains("leopard") {
            "PowerPC"
        } else if id.contains("system") || id.contains("os-9") {
            "Motorola 68k"
        } else {
            "x86_64"
        };
        ("Apple Inc.".to_string(), arch.to_string())
    } else if id.contains("linux") || id.contains("fedora") || id.contains("ubuntu") {
        ("Various".to_string(), "x86_64".to_string())
    } else if id.contains("dos") {
        ("Microsoft Corporation".to_string(), "i386".to_string())
    } else {
        ("Unknown".to_string(), "Unknown".to_string())
    }
}
