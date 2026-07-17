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
    create_disk_with_format(path, "qcow2", size)
}

/// Create a new disk image in the requested qemu-img format
pub fn create_disk_with_format(path: &Path, format: &str, size: &str) -> Result<()> {
    let path_str = path_to_str(path)?;
    let output = Command::new("qemu-img")
        .args(["create", "-f", format, path_str, size])
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
    parse_format_from_info_json(&stdout)
}

/// Extract the `format` field from the JSON emitted by `qemu-img info --output=json`.
///
/// Returns `None` if the JSON is malformed or has no string `format` field. Kept
/// separate from [`detect_disk_format`] so the parsing logic is unit-testable
/// without invoking `qemu-img`.
fn parse_format_from_info_json(stdout: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(stdout).ok()?;
    json["format"].as_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_format_qcow2() {
        let json = r#"{"virtual-size":42949672960,"filename":"disk.qcow2","format":"qcow2","actual-size":200704}"#;
        assert_eq!(parse_format_from_info_json(json), Some("qcow2".to_string()));
    }

    #[test]
    fn parse_format_raw() {
        let json = r#"{"format":"raw","virtual-size":1048576}"#;
        assert_eq!(parse_format_from_info_json(json), Some("raw".to_string()));
    }

    #[test]
    fn parse_format_missing_field() {
        let json = r#"{"virtual-size":1048576,"filename":"disk.img"}"#;
        assert_eq!(parse_format_from_info_json(json), None);
    }

    #[test]
    fn parse_format_non_string_field() {
        let json = r#"{"format":123}"#;
        assert_eq!(parse_format_from_info_json(json), None);
    }

    #[test]
    fn parse_format_malformed_json() {
        assert_eq!(parse_format_from_info_json("not json at all"), None);
        assert_eq!(parse_format_from_info_json(""), None);
    }

    #[test]
    fn path_to_str_valid_utf8() {
        let path = PathBuf::from("/tmp/disk.qcow2");
        assert_eq!(path_to_str(&path).unwrap(), "/tmp/disk.qcow2");
    }

    #[cfg(unix)]
    #[test]
    fn path_to_str_invalid_utf8_errors() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // 0xFF is not valid UTF-8.
        let path = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff.img"));
        assert!(path_to_str(&path).is_err());
    }
}
