//! Shared Folders Help Text
//!
//! Loads mount instruction text for the Shared Folders screen from embedded assets
//! and user overrides. Each tier provides OS-specific mounting instructions.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Help text entry for a shared folders tier
#[derive(Debug, Clone, Deserialize)]
pub struct SharedFoldersHelpEntry {
    /// Title for the help panel
    pub title: String,
    /// Description text (may contain {TAG} placeholder)
    pub description: String,
}

/// Store for shared folders help text
#[derive(Debug, Clone, Default)]
pub struct SharedFoldersHelpStore {
    entries: HashMap<String, SharedFoldersHelpEntry>,
}

impl SharedFoldersHelpStore {
    /// Load help text from embedded assets
    pub fn load_embedded() -> Self {
        let mut store = Self::default();

        let content = include_str!("../../assets/metadata/shared_folders_help.toml");
        if let Ok(entries) = toml::from_str::<HashMap<String, SharedFoldersHelpEntry>>(content) {
            store.entries = entries;
        }

        store
    }

    /// Load user overrides from a file
    pub fn load_user_overrides(&mut self, path: &Path) {
        if !path.exists() {
            return;
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(entries) = toml::from_str::<HashMap<String, SharedFoldersHelpEntry>>(&content)
            {
                for (key, value) in entries {
                    self.entries.insert(key, value);
                }
            }
        }
    }

    /// Get help text or return default values
    pub fn get_or_default(&self, key: &str) -> (&str, &str) {
        self.entries
            .get(key)
            .map(|e| (e.title.as_str(), e.description.as_str()))
            .or_else(|| {
                self.entries
                    .get("unknown")
                    .map(|e| (e.title.as_str(), e.description.as_str()))
            })
            .unwrap_or((
                "Mount Instructions",
                "Mount command varies by OS. Check your OS documentation for 9p support.",
            ))
    }
}
