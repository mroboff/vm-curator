//! QEMU system emulator utilities
//!
//! Provides utilities for checking QEMU availability and capabilities.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Get QEMU version information
pub fn get_qemu_version(emulator: &str) -> Result<String> {
    let output = Command::new(emulator)
        .arg("--version")
        .output()
        .context("Failed to get QEMU version")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().next().unwrap_or("Unknown").to_string())
}

/// Check if QEMU emulator is available
pub fn is_emulator_available(emulator: &str) -> bool {
    Command::new("which")
        .arg(emulator)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List available QEMU emulators on the system
pub fn list_available_emulators() -> Vec<String> {
    let emulators = [
        "qemu-system-x86_64",
        "qemu-system-i386",
        "qemu-system-ppc",
        "qemu-system-m68k",
        "qemu-system-arm",
        "qemu-system-aarch64",
    ];

    emulators
        .iter()
        .filter(|e| is_emulator_available(e))
        .map(|e| e.to_string())
        .collect()
}

/// Check KVM availability
pub fn is_kvm_available() -> bool {
    Path::new("/dev/kvm").exists()
}

/// Get supported display backends for a QEMU emulator
///
/// Runs `<emulator> -display help` and parses the output to get
/// the list of supported display backends (e.g., gtk, sdl, spice-app, vnc).
pub fn get_supported_displays(emulator: &str) -> Vec<String> {
    let output = match Command::new(emulator)
        .args(["-display", "help"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    // QEMU prints display backends to stdout (or sometimes stderr)
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };

    let mut displays = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        // Skip empty lines and header lines
        if trimmed.is_empty() || trimmed.starts_with("Available") || trimmed.contains(':') {
            continue;
        }
        // Each display backend is typically listed on its own line
        let backend = trimmed.split_whitespace().next().unwrap_or("");
        if !backend.is_empty() {
            displays.push(backend.to_string());
        }
    }

    displays
}

/// Check if a SPICE viewer application is available in PATH
///
/// Checks for `remote-viewer` (from virt-viewer package) or `virt-viewer`.
pub fn is_spice_viewer_available() -> bool {
    for viewer in &["remote-viewer", "virt-viewer"] {
        if Command::new("which")
            .arg(viewer)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// Get KVM module info
pub fn get_kvm_info() -> Option<String> {
    if !is_kvm_available() {
        return None;
    }

    let output = Command::new("lsmod").output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("kvm_intel") || line.starts_with("kvm_amd") {
            return Some(line.split_whitespace().next()?.to_string());
        }
    }

    Some("kvm".to_string())
}
