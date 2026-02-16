//! QEMU disk image operations
//!
//! Provides wrappers around qemu-img for disk creation and format detection.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Convert a path to a string, returning an error if the path contains invalid UTF-8
fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Path contains invalid UTF-8: {:?}", path))
}

/// Create a new qcow2 disk image
pub fn create_disk(path: &Path, size: &str) -> Result<()> {
    let path_str = path_to_str(path)?;
    let output = Command::new("qemu-img")
        .args(["create", "-f", "qcow2", path_str, size])
        .output()
        .context("Failed to run qemu-img create")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to create disk: {}", stderr);
    }

    Ok(())
}

/// Convert a disk image from one format to another (e.g., DMG to qcow2)
#[allow(dead_code)]
pub fn convert_disk(source: &Path, dest: &Path, dest_format: &str) -> Result<()> {
    let source_str = path_to_str(source)?;
    let dest_str = path_to_str(dest)?;
    let output = Command::new("qemu-img")
        .args(["convert", "-O", dest_format, source_str, dest_str])
        .output()
        .context("Failed to run qemu-img convert")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to convert disk: {}", stderr);
    }

    Ok(())
}

/// Detect the format of a disk image (returns format string like "qcow2", "raw", etc.)
pub fn detect_disk_format(path: &Path) -> Option<String> {
    let path_str = path_to_str(path).ok()?;
    let output = Command::new("qemu-img")
        .args(["info", "--output=json", path_str])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).ok()?;

    json["format"].as_str().map(|s| s.to_string())
}
