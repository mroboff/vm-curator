use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// A snapshot of a VM disk
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Snapshot ID from qemu-img - reserved for future use
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub size: String,
    pub date: String,
    /// VM clock time - reserved for display in future UI
    #[allow(dead_code)]
    pub vm_clock: String,
}

/// JSON output from qemu-img info --output=json
#[derive(Debug, Deserialize)]
struct QemuImgInfo {
    #[serde(default)]
    format: String,
    #[serde(rename = "virtual-size", default)]
    virtual_size: u64,
    #[serde(rename = "actual-size", default)]
    actual_size: Option<u64>,
    #[serde(rename = "cluster-size", default)]
    cluster_size: Option<u64>,
    #[serde(rename = "backing-filename")]
    backing_filename: Option<String>,
    #[serde(default)]
    snapshots: Vec<QemuSnapshot>,
}

/// Snapshot info from qemu-img JSON output
#[derive(Debug, Deserialize)]
struct QemuSnapshot {
    id: String,
    name: String,
    #[serde(rename = "vm-state-size", default)]
    vm_state_size: u64,
    #[serde(rename = "date-sec", default)]
    date_sec: i64,
    #[serde(rename = "date-nsec", default)]
    date_nsec: i64,
    #[serde(rename = "vm-clock-sec", default)]
    vm_clock_sec: i64,
    #[serde(rename = "vm-clock-nsec", default)]
    vm_clock_nsec: i64,
}

/// Convert a path to a string, returning an error if the path contains invalid UTF-8
fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Path contains invalid UTF-8: {:?}", path))
}

/// Validate and sanitize a snapshot name
/// Returns the sanitized name or an error if the name is invalid
pub fn validate_snapshot_name(name: &str) -> Result<String> {
    // Check for empty or whitespace-only names
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Snapshot name cannot be empty");
    }

    // Check length (qemu-img has a limit)
    if trimmed.len() > 128 {
        bail!("Snapshot name too long (max 128 characters)");
    }

    // Only allow safe characters: alphanumeric, dash, underscore, dot
    // This prevents command injection and qemu-img parsing issues
    let sanitized: String = trimmed
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_' // Replace unsafe characters with underscore
            }
        })
        .collect();

    // Ensure name doesn't start with a dash (could be interpreted as option)
    let sanitized = if sanitized.starts_with('-') {
        format!("_{}", sanitized)
    } else {
        sanitized
    };

    Ok(sanitized)
}

/// List snapshots for a qcow2 disk image using JSON output
pub fn list_snapshots(disk_path: &Path) -> Result<Vec<Snapshot>> {
    let disk_str = path_to_str(disk_path)?;
    let output = Command::new("qemu-img")
        .args(["info", "--output=json", disk_str])
        .output()
        .context("Failed to run qemu-img info")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("qemu-img info failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let info: QemuImgInfo = serde_json::from_str(&stdout)
        .context("Failed to parse qemu-img JSON output")?;

    let snapshots = info
        .snapshots
        .into_iter()
        .map(|s| {
            // Convert size to human-readable format
            let size = format_size(s.vm_state_size);

            // Convert timestamp to date string
            let date = format_timestamp(s.date_sec, s.date_nsec);

            // Convert VM clock to readable format
            let vm_clock = format_vm_clock(s.vm_clock_sec, s.vm_clock_nsec);

            Snapshot {
                id: s.id,
                name: s.name,
                size,
                date,
                vm_clock,
            }
        })
        .collect();

    Ok(snapshots)
}

/// Format bytes to human-readable size
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// Format Unix timestamp to date string
fn format_timestamp(secs: i64, _nsecs: i64) -> String {
    use chrono::{Local, TimeZone};
    if let Some(dt) = Local.timestamp_opt(secs, 0).single() {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        "unknown".to_string()
    }
}

/// Format VM clock seconds/nanoseconds to readable string
fn format_vm_clock(secs: i64, nsecs: i64) -> String {
    let total_secs = secs as f64 + (nsecs as f64 / 1_000_000_000.0);
    let hours = (total_secs / 3600.0) as u64;
    let minutes = ((total_secs % 3600.0) / 60.0) as u64;
    let seconds = total_secs % 60.0;
    format!("{:02}:{:02}:{:06.3}", hours, minutes, seconds)
}

/// Create a new snapshot
pub fn create_snapshot(disk_path: &Path, name: &str) -> Result<()> {
    let disk_str = path_to_str(disk_path)?;
    let sanitized_name = validate_snapshot_name(name)?;
    let output = Command::new("qemu-img")
        .args(["snapshot", "-c", &sanitized_name, disk_str])
        .output()
        .context("Failed to run qemu-img snapshot -c")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to create snapshot: {}", stderr);
    }

    Ok(())
}

/// Restore (apply) a snapshot
pub fn restore_snapshot(disk_path: &Path, name: &str) -> Result<()> {
    let disk_str = path_to_str(disk_path)?;
    let sanitized_name = validate_snapshot_name(name)?;
    let output = Command::new("qemu-img")
        .args(["snapshot", "-a", &sanitized_name, disk_str])
        .output()
        .context("Failed to run qemu-img snapshot -a")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to restore snapshot: {}", stderr);
    }

    Ok(())
}

/// Delete a snapshot
pub fn delete_snapshot(disk_path: &Path, name: &str) -> Result<()> {
    let disk_str = path_to_str(disk_path)?;
    let sanitized_name = validate_snapshot_name(name)?;
    let output = Command::new("qemu-img")
        .args(["snapshot", "-d", &sanitized_name, disk_str])
        .output()
        .context("Failed to run qemu-img snapshot -d")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to delete snapshot: {}", stderr);
    }

    Ok(())
}

/// Get information about a disk image using JSON output
#[allow(dead_code)]
pub fn get_disk_info(disk_path: &Path) -> Result<DiskInfo> {
    let disk_str = path_to_str(disk_path)?;
    let output = Command::new("qemu-img")
        .args(["info", "--output=json", disk_str])
        .output()
        .context("Failed to run qemu-img info")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to get disk info: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let info: QemuImgInfo = serde_json::from_str(&stdout)
        .context("Failed to parse qemu-img JSON output")?;

    Ok(DiskInfo {
        format: info.format,
        virtual_size: format_size(info.virtual_size),
        disk_size: info.actual_size.map(format_size).unwrap_or_else(|| "unknown".to_string()),
        cluster_size: info.cluster_size.map(format_size),
        backing_file: info.backing_filename,
    })
}

/// Disk image information - reserved for future disk info display
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiskInfo {
    pub format: String,
    pub virtual_size: String,
    pub disk_size: String,
    pub cluster_size: Option<String>,
    pub backing_file: Option<String>,
}

#[cfg(test)]
#[path = "tests/snapshot.rs"]
mod tests;
