//! Multi-GPU Passthrough Support
//!
//! Provides utilities for Looking Glass integration in multi-GPU passthrough scenarios.

use std::path::PathBuf;
use std::process::Command;

/// Looking Glass configuration utilities
pub struct LookingGlassConfig;

impl LookingGlassConfig {
    /// Find Looking Glass client in common locations
    pub fn find_client() -> Option<PathBuf> {
        let candidates = [
            "/usr/bin/looking-glass-client",
            "/usr/local/bin/looking-glass-client",
            "/opt/looking-glass/bin/looking-glass-client",
        ];

        for path in candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }

        // Try to find via which
        if let Ok(output) = Command::new("which").arg("looking-glass-client").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }

        None
    }
}
