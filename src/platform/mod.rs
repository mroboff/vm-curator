//! Platform abstraction layer.
//!
//! Centralizes the handful of operations that differ between operating systems
//! (hardware acceleration, default display backend, firmware discovery, opening
//! URLs) so the rest of the codebase can stay platform-agnostic. Each function
//! has a `#[cfg(target_os = "…")]` body; callers just use `platform::…`.
//!
//! The two supported targets are Linux (KVM, GTK, distro OVMF) and macOS
//! (HVF, Cocoa, Homebrew/MacPorts edk2). Other Unix targets fall back to the
//! Linux behavior for everything except acceleration.

use std::path::PathBuf;
use std::process::Command;

/// Name of the QEMU hardware-acceleration accelerator for this host
/// (`kvm` on Linux, `hvf` on macOS). Used in `-machine …,accel=<name>`.
pub fn acceleration_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "hvf"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "kvm"
    }
}

/// The standalone QEMU flag that enables acceleration, if any.
///
/// On Linux this is the idiomatic `-enable-kvm`; on macOS it is `-accel hvf`
/// (QEMU has no `-enable-hvf` shorthand). Returned as a single argument string
/// to match how the launch-script generator pushes other multi-token args.
pub fn acceleration_flag() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "-accel hvf"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "-enable-kvm"
    }
}

/// Whether hardware acceleration is available on this host.
///
/// Linux: the `/dev/kvm` device exists. macOS: `sysctl kern.hv_support` reports
/// `1` (Hypervisor.framework supported and not disabled by policy).
pub fn is_acceleration_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        Command::new("sysctl")
            .args(["-n", "kern.hv_support"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::path::Path::new("/dev/kvm").exists()
    }
}

/// A short human-readable accelerator label for display, or `None` when
/// acceleration is unavailable (mirrors the old `get_kvm_info`).
pub fn acceleration_info() -> Option<String> {
    if !is_acceleration_available() {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        Some("hvf".to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Report the loaded KVM variant (kvm_intel / kvm_amd) when we can.
        if let Ok(output) = Command::new("lsmod").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.starts_with("kvm_intel") || line.starts_with("kvm_amd") {
                    if let Some(name) = line.split_whitespace().next() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        Some("kvm".to_string())
    }
}

/// Whether PCI / GPU passthrough is supportable on this host.
///
/// Passthrough requires Linux VFIO/IOMMU; it does not exist on macOS (or other
/// non-Linux targets). The UI uses this to hide passthrough entries entirely.
pub fn passthrough_supported() -> bool {
    cfg!(target_os = "linux")
}

/// Default display backend for newly created VMs.
pub fn default_display() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cocoa"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "gtk"
    }
}

/// Preferred ordering of display backends, most-preferred first. Only backends
/// that QEMU actually reports as supported are surfaced to the user; this just
/// controls the order. The host-native windowing backend leads the list.
pub fn preferred_display_order() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["cocoa", "sdl", "spice-app", "vnc", "none"]
    }
    #[cfg(not(target_os = "macos"))]
    {
        &["gtk", "sdl", "spice-app", "vnc", "none"]
    }
}

/// Extra OVMF/edk2 `*_CODE` firmware paths to probe beyond the built-in Linux
/// distro list. Empty on Linux; on macOS these are the Homebrew/MacPorts edk2
/// files shipped alongside QEMU.
pub fn ovmf_code_candidates() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        firmware_files(&["edk2-x86_64-code.fd", "edk2-aarch64-code.fd"])
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Extra OVMF/edk2 `*_VARS` writable-variable template paths to probe beyond the
/// built-in Linux distro list. Empty on Linux; macOS Homebrew/MacPorts edk2 vars.
pub fn ovmf_vars_candidates() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        // edk2-i386-vars.fd pairs with the x86_64 CODE image; edk2-arm-vars.fd
        // with the aarch64 image.
        firmware_files(&["edk2-i386-vars.fd", "edk2-arm-vars.fd"])
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Build candidate firmware paths by joining each filename onto the QEMU data
/// directories that Homebrew and MacPorts use on macOS.
#[cfg(target_os = "macos")]
fn firmware_files(names: &[&str]) -> Vec<String> {
    const QEMU_SHARE_DIRS: &[&str] = &[
        "/opt/homebrew/share/qemu", // Homebrew on Apple Silicon
        "/usr/local/share/qemu",    // Homebrew on Intel
        "/opt/local/share/qemu",    // MacPorts
    ];

    let mut out = Vec::new();
    for dir in QEMU_SHARE_DIRS {
        for name in names {
            out.push(format!("{dir}/{name}"));
        }
    }
    out
}

/// Open a URL in the user's default browser, trying the host-native opener first.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    // Host-native opener first, then cross-platform fallbacks.
    #[cfg(target_os = "macos")]
    let primary: &[&str] = &["open"];
    #[cfg(not(target_os = "macos"))]
    let primary: &[&str] = &["xdg-open"];

    for opener in primary
        .iter()
        .chain(["firefox", "chromium", "google-chrome", "open"].iter())
    {
        if Command::new(opener).arg(url).spawn().is_ok() {
            return Ok(());
        }
    }
    anyhow::bail!("No browser found. Please visit: {}", url)
}

/// Best-effort default QEMU emulator for this host's native architecture, used
/// as a fallback when a profile does not pin one. Apple Silicon accelerates
/// aarch64 guests; Intel Macs and Linux x86 hosts accelerate x86_64.
#[allow(dead_code)]
pub fn native_emulator() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "qemu-system-aarch64"
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        "qemu-system-x86_64"
    }
}

/// Directories returned for firmware probing — exposed for completeness; not all
/// callers need it.
#[allow(dead_code)]
pub fn qemu_data_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        [
            "/opt/homebrew/share/qemu",
            "/usr/local/share/qemu",
            "/opt/local/share/qemu",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
