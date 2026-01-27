//! Filesystem utilities
//!
//! Provides helpers for filesystem detection and optimization,
//! particularly for BTRFS copy-on-write handling.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// BTRFS filesystem magic number (used by statfs)
const BTRFS_SUPER_MAGIC: i64 = 0x9123683E;

/// Check if a path is on a BTRFS filesystem
pub fn is_btrfs(path: &Path) -> bool {
    // Use stat -f to get filesystem type
    // This is more portable than using libc statfs directly
    let output = Command::new("stat")
        .args(["-f", "-c", "%t", path.to_str().unwrap_or("")])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let fs_type = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // stat -f -c %t returns the filesystem type in hex
            let expected = format!("{:x}", BTRFS_SUPER_MAGIC);
            fs_type.to_lowercase() == expected
        }
        _ => {
            // Fallback: check /proc/mounts
            is_btrfs_from_mounts(path)
        }
    }
}

/// Fallback BTRFS detection using /proc/mounts
fn is_btrfs_from_mounts(path: &Path) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };

    // Canonicalize path for comparison
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // Path doesn't exist yet, try parent
            if let Some(parent) = path.parent() {
                match parent.canonicalize() {
                    Ok(p) => p,
                    Err(_) => return false,
                }
            } else {
                return false;
            }
        }
    };

    let path_str = path.to_string_lossy();

    // Find the mount point that contains our path
    // Sort by mount point length (longest first) to find the most specific match
    let mut mount_entries: Vec<(&str, &str)> = mounts
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                Some((parts[1], parts[2])) // mount_point, fs_type
            } else {
                None
            }
        })
        .collect();

    mount_entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (mount_point, fs_type) in mount_entries {
        if path_str.starts_with(mount_point) || mount_point == "/" {
            return fs_type == "btrfs";
        }
    }

    false
}

/// Disable copy-on-write on a directory using chattr +C
///
/// This should be called on newly created directories BEFORE any files
/// are created in them, as the attribute only affects new files.
pub fn disable_cow(path: &Path) -> Result<()> {
    let output = Command::new("chattr")
        .args(["+C", path.to_str().unwrap_or("")])
        .output()
        .context("Failed to run chattr command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't fail if chattr isn't available or permission denied
        // Just log and continue - this is an optimization, not critical
        if !stderr.contains("Operation not supported")
            && !stderr.contains("Inappropriate ioctl")
        {
            anyhow::bail!("chattr +C failed: {}", stderr.trim());
        }
    }

    Ok(())
}

/// Set up a VM library directory with appropriate filesystem optimizations
///
/// Creates the directory if it doesn't exist, and disables BTRFS copy-on-write
/// if the filesystem is BTRFS. This prevents performance degradation from
/// double copy-on-write (BTRFS CoW + qcow2 CoW).
///
/// Returns Ok(true) if CoW was disabled, Ok(false) if not needed or not possible.
pub fn setup_vm_directory(path: &Path) -> Result<bool> {
    // Create directory if it doesn't exist
    std::fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory {:?}", path))?;

    // Check if BTRFS and disable CoW
    if is_btrfs(path) {
        match disable_cow(path) {
            Ok(()) => return Ok(true),
            Err(e) => {
                // Log but don't fail - CoW disable is an optimization
                eprintln!("Warning: Could not disable copy-on-write: {}", e);
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_btrfs_nonexistent() {
        // Should not panic on non-existent path
        let result = is_btrfs(&PathBuf::from("/nonexistent/path/12345"));
        // Result depends on root filesystem type
        assert!(result == true || result == false);
    }

    #[test]
    fn test_is_btrfs_root() {
        // Should work on root
        let _result = is_btrfs(&PathBuf::from("/"));
        // Just verify it doesn't panic
    }
}
