//! QEMU system emulator utilities
//!
//! Provides utilities for checking QEMU availability and capabilities.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

    parse_display_help(&text)
}

/// Parse the output of `<emulator> -display help`.
///
/// QEMU prints a header line ending in ":", a list of backend names (one per
/// line), a blank line, and then a usage paragraph. Only the names between the
/// header and the blank line are real backends, and each name is a single
/// lowercase token like `gtk` or `spice-app`.
fn parse_display_help(text: &str) -> Vec<String> {
    let mut displays = Vec::new();
    let mut found_header = false;
    for line in text.lines() {
        let trimmed = line.trim();

        if !found_header {
            // The header is the line introducing the list, e.g.
            // "Available display backend types:".
            if trimmed.starts_with("Available") && trimmed.ends_with(':') {
                found_header = true;
            }
            continue;
        }

        // The list ends at the first blank line; anything after is help text.
        if trimmed.is_empty() {
            break;
        }

        if is_valid_display_backend(trimmed) {
            displays.push(trimmed.to_string());
        }
    }

    displays
}

/// Check whether a string looks like a QEMU display backend name.
///
/// Backend names are lowercase identifiers, optionally containing digits or
/// hyphens (e.g., `gtk`, `spice-app`, `egl-headless`).
fn is_valid_display_backend(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase())
            .unwrap_or(false)
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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

/// Information about available network backends
#[derive(Debug, Clone)]
pub struct NetworkCapabilities {
    pub passt_available: bool,
    pub bridge_helper_path: Option<PathBuf>,
    pub bridge_helper_configured: bool,
    pub system_bridges: Vec<String>,
}

/// Detect all available networking capabilities
pub fn detect_network_capabilities() -> NetworkCapabilities {
    let passt_available = is_passt_available();
    let bridge_helper_path = find_bridge_helper();
    let bridge_helper_configured = bridge_helper_path
        .as_ref()
        .map(|p| is_bridge_helper_configured(p))
        .unwrap_or(false);
    let system_bridges = list_system_bridges();

    NetworkCapabilities {
        passt_available,
        bridge_helper_path,
        bridge_helper_configured,
        system_bridges,
    }
}

/// Check if passt binary is available
fn is_passt_available() -> bool {
    Command::new("passt")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Find qemu-bridge-helper binary
fn find_bridge_helper() -> Option<PathBuf> {
    let paths = [
        "/usr/lib/qemu/qemu-bridge-helper",
        "/usr/libexec/qemu-bridge-helper",
        "/usr/libexec/qemu/qemu-bridge-helper",
    ];

    for path in paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Check if bridge helper has setuid or CAP_NET_ADMIN
fn is_bridge_helper_configured(path: &Path) -> bool {
    // Check setuid bit
    if let Ok(metadata) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o4000 != 0 {
            return true;
        }
    }

    // Check capabilities via getcap
    if let Ok(output) = Command::new("getcap")
        .arg(path)
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("cap_net_admin") {
            return true;
        }
    }

    false
}

/// List bridges currently on the system
fn list_system_bridges() -> Vec<String> {
    let output = match Command::new("ip")
        .args(["-o", "link", "show", "type", "bridge"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut bridges = Vec::new();
    for line in stdout.lines() {
        // Format: "N: bridgename: <FLAGS> ..."
        if let Some(name) = line.split(':').nth(1) {
            let name = name.trim();
            if !name.is_empty() {
                bridges.push(name.to_string());
            }
        }
    }
    bridges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_display_help_extracts_only_backend_names() {
        // Real output from `qemu-system-x86_64 -display help` on QEMU 10.x.
        // The list is followed by a usage paragraph that previously got
        // captured as bogus backends ("Some", "-display", "For"). See #27.
        let raw = "\
Available display backend types:
none
gtk
sdl
egl-headless
curses
spice-app
dbus

Some display backends support suboptions, which can be set with
   -display backend,option=value,option=value...
For a short list of the suboptions for each display, see the top-level -help output; more detail is in the documentation.
";
        let parsed = parse_display_help(raw);
        assert_eq!(
            parsed,
            vec!["none", "gtk", "sdl", "egl-headless", "curses", "spice-app", "dbus"]
        );
    }

    #[test]
    fn parse_display_help_handles_empty_output() {
        assert!(parse_display_help("").is_empty());
    }

    #[test]
    fn parse_display_help_returns_empty_when_header_missing() {
        // If QEMU output is unrecognizable, return nothing so the caller
        // falls back to the default list.
        let raw = "gtk\nsdl\nspice-app\n";
        assert!(parse_display_help(raw).is_empty());
    }

    #[test]
    fn is_valid_display_backend_accepts_known_names() {
        assert!(is_valid_display_backend("gtk"));
        assert!(is_valid_display_backend("spice-app"));
        assert!(is_valid_display_backend("egl-headless"));
        assert!(is_valid_display_backend("vnc"));
        assert!(is_valid_display_backend("none"));
    }

    #[test]
    fn is_valid_display_backend_rejects_help_text() {
        assert!(!is_valid_display_backend("Some"));
        assert!(!is_valid_display_backend("For"));
        assert!(!is_valid_display_backend("-display"));
        assert!(!is_valid_display_backend("display backend,option"));
        assert!(!is_valid_display_backend(""));
    }
}
