//! VM Creation Logic
//!
//! This module handles creating new VMs: directory creation, disk image
//! generation, and launch script generation.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Shell-escape a string for safe interpolation in bash scripts.
/// This handles special characters that could cause command injection.
fn shell_escape(s: &str) -> String {
    // If the string contains only safe characters, return as-is
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return s.to_string();
    }

    // Otherwise, wrap in single quotes and escape any existing single quotes
    // In shell: replace ' with '\'' (end quote, escaped quote, start quote)
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

use crate::commands::qemu_img;
use crate::vm::qemu_config::{PortForward, PortProtocol};
use crate::wizard_types::{CreateWizardState, DiskAction, WizardQemuConfig};

/// Install media type for QEMU command generation
pub enum InstallMedia<'a> {
    /// No install media
    None,
    /// ISO mounted as CD-ROM; None = $ISO variable, Some = custom path expression
    Iso(Option<&'a str>),
    /// Recovery image (DMG) mounted as IDE drive; None = $RECOVERY_IMG variable, Some = custom path
    RecoveryImage(Option<&'a str>),
}

/// Generate a random UUID for SMBIOS
fn generate_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use time-based pseudo-random generation
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // Simple LCG for pseudo-random bytes
    let mut state = seed as u64;
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 33) as u8;
    }

    // Set version 4 (random) and variant 1
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Generate a consumer-like serial number (not corporate format)
fn generate_serial() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // Mix in process ID and thread for more entropy
    let seed = seed ^ (std::process::id() as u128) ^ (seed >> 64);

    let chars: Vec<char> = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ".chars().collect();
    let mut state = seed as u64;
    let mut serial = String::with_capacity(12);

    for _ in 0..12 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let idx = ((state >> 33) as usize) % chars.len();
        serial.push(chars[idx]);
    }

    serial
}

/// A matched OVMF firmware pair: read-only CODE plus its VARS template.
///
/// CODE and VARS must agree in both size (2M vs 4M) and on-disk format
/// (raw vs qcow2); mixing them produces a VM that either won't boot or fails
/// to expose Secure Boot / TPM 2.0 correctly. Selecting them as a pair (rather
/// than via two independent searches) guarantees they always match.
struct OvmfFirmware {
    /// Read-only OVMF_CODE path.
    code: String,
    /// OVMF_VARS template to copy into the VM directory.
    vars_template: String,
    /// On-disk image format for the QEMU `-drive ...,format=` flag.
    format: &'static str,
}

/// Secure Boot OVMF pairs `(code, vars, format)` in priority order.
///
/// 4M variants are listed first: Fedora's 2M `OVMF_CODE.secboot.fd` does not
/// expose TPM 2.0 correctly to the Windows 11 installer (issue #42), so we must
/// prefer the 4M build — including Fedora's qcow2-format firmware — when present.
const OVMF_SECBOOT_PAIRS: &[(&str, &str, &str)] = &[
    // Fedora/RHEL — 4M qcow2 (Fedora 40+/44 ship firmware as qcow2)
    (
        "/usr/share/edk2/ovmf/OVMF_CODE_4M.secboot.qcow2",
        "/usr/share/edk2/ovmf/OVMF_VARS_4M.secboot.qcow2",
        "qcow2",
    ),
    // Fedora/RHEL — 4M raw
    (
        "/usr/share/edk2/ovmf/OVMF_CODE_4M.secboot.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS_4M.secboot.fd",
        "raw",
    ),
    // Arch Linux — 4M
    (
        "/usr/share/edk2/x64/OVMF_CODE.secboot.4m.fd",
        "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
        "raw",
    ),
    (
        "/usr/share/OVMF/x64/OVMF_CODE.secboot.4m.fd",
        "/usr/share/OVMF/x64/OVMF_VARS.4m.fd",
        "raw",
    ),
    // Debian/Ubuntu — 4M (pre-enrolled Microsoft keys)
    (
        "/usr/share/OVMF/OVMF_CODE_4M.secboot.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.ms.fd",
        "raw",
    ),
    (
        "/usr/share/OVMF/OVMF_CODE_4M.ms.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.ms.fd",
        "raw",
    ),
    // --- 2M fallbacks (last resort) ---
    // Fedora/RHEL — 2M
    (
        "/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd",
        "raw",
    ),
    // Debian/Ubuntu — 2M
    (
        "/usr/share/OVMF/OVMF_CODE.secboot.fd",
        "/usr/share/OVMF/OVMF_VARS.ms.fd",
        "raw",
    ),
    // Arch Linux — legacy 2M
    (
        "/usr/share/edk2-ovmf/x64/OVMF_CODE.secboot.fd",
        "/usr/share/edk2-ovmf/x64/OVMF_VARS.fd",
        "raw",
    ),
    // Generic
    (
        "/usr/share/ovmf/OVMF_CODE.secboot.fd",
        "/usr/share/ovmf/OVMF_VARS.fd",
        "raw",
    ),
];

/// Non-Secure-Boot OVMF pairs `(code, vars, format)` in priority order (4M first).
const OVMF_PAIRS: &[(&str, &str, &str)] = &[
    // Fedora/RHEL — 4M qcow2
    (
        "/usr/share/edk2/ovmf/OVMF_CODE_4M.qcow2",
        "/usr/share/edk2/ovmf/OVMF_VARS_4M.qcow2",
        "qcow2",
    ),
    // Fedora/RHEL — 4M raw
    (
        "/usr/share/edk2/ovmf/OVMF_CODE_4M.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS_4M.fd",
        "raw",
    ),
    // Arch Linux — 4M
    (
        "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
        "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
        "raw",
    ),
    (
        "/usr/share/edk2-ovmf/x64/OVMF_CODE.4m.fd",
        "/usr/share/edk2-ovmf/x64/OVMF_VARS.4m.fd",
        "raw",
    ),
    (
        "/usr/share/OVMF/x64/OVMF_CODE.4m.fd",
        "/usr/share/OVMF/x64/OVMF_VARS.4m.fd",
        "raw",
    ),
    // Debian/Ubuntu — 4M
    (
        "/usr/share/OVMF/OVMF_CODE_4M.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.fd",
        "raw",
    ),
    // --- 2M fallbacks (last resort) ---
    // Fedora/RHEL — 2M
    (
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS.fd",
        "raw",
    ),
    // Debian/Ubuntu — 2M
    (
        "/usr/share/OVMF/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_VARS.fd",
        "raw",
    ),
    // Arch Linux — legacy 2M
    (
        "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
        "/usr/share/edk2-ovmf/x64/OVMF_VARS.fd",
        "raw",
    ),
    (
        "/usr/share/edk2/x64/OVMF_CODE.fd",
        "/usr/share/edk2/x64/OVMF_VARS.fd",
        "raw",
    ),
    // NixOS
    (
        "/run/libvirt/nix-ovmf/OVMF_CODE.fd",
        "/run/libvirt/nix-ovmf/OVMF_VARS.fd",
        "raw",
    ),
    // Generic
    (
        "/usr/share/ovmf/OVMF_CODE.fd",
        "/usr/share/ovmf/OVMF_VARS.fd",
        "raw",
    ),
    (
        "/usr/share/qemu/OVMF_CODE.fd",
        "/usr/share/qemu/OVMF_VARS.fd",
        "raw",
    ),
];

/// Fallback firmware pair used when no known CODE+VARS pair exists on disk.
fn default_ovmf_firmware() -> OvmfFirmware {
    OvmfFirmware {
        code: "/usr/share/OVMF/OVMF_CODE.fd".to_string(),
        vars_template: "/usr/share/OVMF/OVMF_VARS.fd".to_string(),
        format: "raw",
    }
}

/// Select a matched OVMF CODE+VARS firmware pair, preferring 4M builds.
///
/// Both the CODE and VARS file of a candidate pair must exist on disk before it
/// is chosen, so the returned CODE and VARS always agree in size and format.
fn find_ovmf_firmware(secboot: bool) -> Option<OvmfFirmware> {
    let table = if secboot {
        OVMF_SECBOOT_PAIRS
    } else {
        OVMF_PAIRS
    };
    for &(code, vars, format) in table {
        if Path::new(code).exists() && Path::new(vars).exists() {
            return Some(OvmfFirmware {
                code: code.to_string(),
                vars_template: vars.to_string(),
                format,
            });
        }
    }
    None
}

/// Result of creating a new VM
#[derive(Debug)]
pub struct CreatedVm {
    /// Path to the VM directory - reserved for future use
    #[allow(dead_code)]
    pub path: PathBuf,
    /// Path to the launch script
    pub launch_script: PathBuf,
    /// Path to the disk image - reserved for future use
    #[allow(dead_code)]
    pub disk_image: PathBuf,
}

/// Create a new VM from wizard state
pub fn create_vm(library_path: &Path, state: &CreateWizardState) -> Result<CreatedVm> {
    // Validate inputs
    if state.vm_name.trim().is_empty() {
        bail!("VM name cannot be empty");
    }
    if state.folder_name.is_empty() {
        bail!("Folder name cannot be empty");
    }

    // Validate disk configuration
    if state.use_existing_disk {
        if state.existing_disk_path.is_none() {
            bail!("No existing disk selected");
        }
        let path = state.existing_disk_path.as_ref().unwrap();
        if !path.exists() {
            bail!("Selected disk does not exist: {}", path.display());
        }
    } else if state.disk_size_gb == 0 {
        bail!("Disk size must be greater than 0");
    }

    // Create VM directory
    let vm_dir = create_vm_directory(library_path, &state.folder_name)?;

    // Create or copy/move disk image
    let disk_filename = format!("{}.qcow2", state.folder_name);
    let disk_path = if state.use_existing_disk {
        handle_existing_disk(
            &vm_dir,
            &disk_filename,
            state.existing_disk_path.as_ref().unwrap(),
            &state.existing_disk_action,
        )?
    } else {
        create_disk_image(&vm_dir, &disk_filename, state.disk_size_gb)?
    };

    // Copy BIOS/ROM file to VM directory if provided
    let mut qemu_config = state.qemu_config.clone();
    if let Some(ref rom_path) = state.bios_rom_path {
        let rom_filename = rom_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "rom.bin".to_string());
        let dest = vm_dir.join(&rom_filename);
        fs::copy(rom_path, &dest).with_context(|| {
            format!(
                "Failed to copy ROM file from {} to {}",
                rom_path.display(),
                dest.display()
            )
        })?;
        qemu_config.bios_path = Some(PathBuf::from(&rom_filename));
    }

    // Generate and write launch script with OS-awareness
    let script_content = generate_launch_script_with_os(
        &state.vm_name,
        &disk_filename,
        state.iso_path.as_deref(),
        state.is_recovery_image,
        &qemu_config,
        state.selected_os.as_deref(),
        state.floppy_path.as_deref(),
    );
    let launch_script_path = write_launch_script(&vm_dir, &script_content)?;

    // Write VM metadata file with custom display name
    write_vm_metadata(&vm_dir, &state.vm_name, state.selected_os.as_deref(), None)?;

    Ok(CreatedVm {
        path: vm_dir,
        launch_script: launch_script_path,
        disk_image: disk_path,
    })
}

/// Handle an existing disk by copying or moving it to the VM directory
fn handle_existing_disk(
    vm_dir: &Path,
    filename: &str,
    source: &Path,
    action: &DiskAction,
) -> Result<PathBuf> {
    let dest = vm_dir.join(filename);

    match action {
        DiskAction::Copy => {
            fs::copy(source, &dest).with_context(|| {
                format!(
                    "Failed to copy disk from {} to {}",
                    source.display(),
                    dest.display()
                )
            })?;
        }
        DiskAction::Move => {
            // Try rename first (works if on same filesystem)
            if fs::rename(source, &dest).is_err() {
                // Rename failed (likely different filesystem), fall back to copy+delete
                fs::copy(source, &dest).with_context(|| {
                    format!(
                        "Failed to copy disk from {} to {}",
                        source.display(),
                        dest.display()
                    )
                })?;
                fs::remove_file(source).with_context(|| {
                    format!(
                        "Failed to remove original disk after copying: {}",
                        source.display()
                    )
                })?;
            }
        }
    }

    Ok(dest)
}

/// Write VM metadata file (vm-curator.toml)
pub fn write_vm_metadata(
    vm_dir: &Path,
    display_name: &str,
    os_profile: Option<&str>,
    notes: Option<&str>,
) -> Result<()> {
    let metadata_path = vm_dir.join("vm-curator.toml");

    let mut content = String::new();
    content.push_str("# VM Curator metadata\n\n");
    content.push_str(&format!(
        "display_name = \"{}\"\n",
        display_name.replace('"', "\\\"")
    ));

    if let Some(profile) = os_profile {
        content.push_str(&format!("os_profile = \"{}\"\n", profile));
    }

    if let Some(notes_text) = notes {
        if notes_text.contains('\n') {
            // Multi-line: use TOML literal string
            content.push_str(&format!("notes = '''\n{}'''\n", notes_text));
        } else {
            content.push_str(&format!(
                "notes = \"{}\"\n",
                notes_text.replace('"', "\\\"")
            ));
        }
    }

    fs::write(&metadata_path, content)
        .with_context(|| format!("Failed to write VM metadata: {}", metadata_path.display()))?;

    Ok(())
}

/// Create the VM directory
pub fn create_vm_directory(library_path: &Path, folder_name: &str) -> Result<PathBuf> {
    let vm_dir = library_path.join(folder_name);

    if vm_dir.exists() {
        bail!("VM directory already exists: {}", vm_dir.display());
    }

    fs::create_dir_all(&vm_dir)
        .with_context(|| format!("Failed to create VM directory: {}", vm_dir.display()))?;

    Ok(vm_dir)
}

/// Create a new qcow2 disk image
pub fn create_disk_image(vm_dir: &Path, filename: &str, size_gb: u32) -> Result<PathBuf> {
    let disk_path = vm_dir.join(filename);
    let size_str = format!("{}G", size_gb);

    qemu_img::create_disk(&disk_path, &size_str)
        .with_context(|| format!("Failed to create disk image: {}", disk_path.display()))?;

    Ok(disk_path)
}

/// Check if an OS profile is Windows 10 or 11
fn is_windows_10_or_11(os_profile: Option<&str>) -> bool {
    matches!(os_profile, Some("windows-10") | Some("windows-11"))
}

/// Check if an OS profile is Windows 11 specifically
fn is_windows_11(os_profile: Option<&str>) -> bool {
    matches!(os_profile, Some("windows-11"))
}

/// Check if an OS profile is an Intel (x86_64) macOS
fn is_intel_macos(os_profile: Option<&str>, emulator: &str) -> bool {
    if !emulator.contains("x86_64") {
        return false;
    }
    os_profile.is_some_and(|p| p.starts_with("macos-") || p.starts_with("mac-osx-"))
}

/// Check if an OS profile is a modern macOS that requires OpenCore
#[allow(dead_code)]
fn is_modern_macos(os_profile: Option<&str>) -> bool {
    matches!(
        os_profile,
        Some("macos-big-sur")
            | Some("macos-monterey")
            | Some("macos-ventura")
            | Some("macos-sonoma")
            | Some("macos-sequoia")
            | Some("macos-tahoe")
    )
}

/// Generate SMBIOS options for Windows to avoid corporate machine detection
fn generate_smbios_options() -> String {
    let uuid = generate_uuid();
    let system_serial = generate_serial();
    let board_serial = generate_serial();

    // Consumer-style SMBIOS that doesn't trigger corporate machine detection
    // Type 1: System Information
    // Type 2: Baseboard Information
    format!(
        r#"# SMBIOS configuration (unique per VM to avoid Windows corporate detection)
SMBIOS_OPTS=(
    -smbios "type=1,manufacturer=QEMU,product=Standard PC,version=1.0,serial={system_serial},uuid={uuid}"
    -smbios "type=2,manufacturer=QEMU,product=Standard PC,version=1.0,serial={board_serial}"
)
"#,
        system_serial = system_serial,
        uuid = uuid,
        board_serial = board_serial,
    )
}

/// Generate TPM setup functions for Windows 11
fn generate_tpm_functions() -> String {
    r#"# TPM 2.0 emulation functions (required for Windows 11)
TPM_DIR="$VM_DIR/tpm"

init_tpm() {
    if [[ ! -d "$TPM_DIR" ]]; then
        echo "Initializing TPM state directory..."
        mkdir -p "$TPM_DIR"

        # Ensure a per-user swtpm CA config exists so EK/platform certificate
        # creation does not require write access to the system-wide
        # /var/lib/swtpm-localca statedir (issue #42). skip-if-exist never
        # clobbers an existing user config.
        if [[ ! -f "$HOME/.config/swtpm-localca.conf" ]]; then
            swtpm_setup --create-config-files skip-if-exist 2>/dev/null || true
        fi

        # Try a full setup with certificates; fall back to a certificate-less
        # setup if cert creation still fails. Windows 11 detects TPM 2.0 without
        # an EK certificate, so this keeps VM creation working regardless.
        if ! swtpm_setup --tpmstate "$TPM_DIR" \
            --tpm2 \
            --create-ek-cert \
            --create-platform-cert \
            --allow-signing \
            --decryption \
            --overwrite 2>/dev/null; then
            echo "swtpm certificate creation failed; retrying without certificates..."
            swtpm_setup --tpmstate "$TPM_DIR" --tpm2 --overwrite
        fi
    fi
}

start_tpm() {
    # Initialize TPM if needed
    init_tpm

    # Kill any existing swtpm for this VM
    pkill -f "swtpm.*$TPM_DIR" 2>/dev/null || true
    sleep 0.5

    echo "Starting TPM emulator..."
    swtpm socket \
        --tpmstate dir="$TPM_DIR" \
        --ctrl type=unixio,path="$TPM_DIR/swtpm-sock" \
        --tpm2 \
        --daemon
    sleep 1

    if [[ ! -S "$TPM_DIR/swtpm-sock" ]]; then
        echo "Error: TPM socket not created"
        exit 1
    fi
}

stop_tpm() {
    pkill -f "swtpm.*$TPM_DIR" 2>/dev/null || true
}

# Cleanup TPM on exit
cleanup() {
    stop_tpm
}
trap cleanup EXIT

"#
    .to_string()
}

/// Generate OVMF variables setup for UEFI.
///
/// Copies the VARS template from the same firmware pair the QEMU command uses
/// (via [`find_ovmf_firmware`]) so CODE and VARS always match in size/format.
/// The writable copy's extension mirrors the firmware format (`.qcow2` vs
/// `.fd`) and the QEMU `-drive ...,format=` flag is derived from the same pair.
fn generate_ovmf_vars_setup(needs_secboot: bool) -> String {
    let firmware = find_ovmf_firmware(needs_secboot).unwrap_or_else(default_ovmf_firmware);
    let vars_ext = if firmware.format == "qcow2" {
        "qcow2"
    } else {
        "fd"
    };

    format!(
        r#"# UEFI variables (writable copy per VM)
OVMF_VARS_TEMPLATE="{template}"
OVMF_VARS="$VM_DIR/OVMF_VARS.{ext}"

# Create a writable copy of OVMF_VARS if it doesn't exist
if [[ ! -f "$OVMF_VARS" ]]; then
    if [[ -f "$OVMF_VARS_TEMPLATE" ]]; then
        echo "Creating UEFI variables file..."
        cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS"
    else
        echo "Warning: OVMF_VARS template not found at $OVMF_VARS_TEMPLATE"
        echo "UEFI variables may not persist across reboots"
    fi
fi

"#,
        template = firmware.vars_template,
        ext = vars_ext
    )
}

/// Generate the launch.sh script content with OS profile awareness
pub fn generate_launch_script_with_os(
    vm_name: &str,
    disk_filename: &str,
    iso_path: Option<&Path>,
    is_recovery_image: bool,
    config: &WizardQemuConfig,
    os_profile: Option<&str>,
    floppy_path: Option<&Path>,
) -> String {
    let mut script = String::new();

    let is_windows = is_windows_10_or_11(os_profile);
    let is_intel_macos_vm = is_intel_macos(os_profile, &config.emulator);
    let needs_tpm = config.tpm || is_windows_11(os_profile);
    let needs_uefi = config.uefi || is_windows_11(os_profile);

    // Shebang and header
    script.push_str("#!/bin/bash\n\n");
    script.push_str(&format!("# {} VM Launch Script\n", vm_name));
    script.push_str(&format!(
        "# {} CPUs, {}MB RAM, {} graphics, {} disk interface\n",
        config.cpu_cores, config.memory_mb, config.vga, config.disk_interface
    ));
    if is_windows {
        script.push_str("# Windows VM with unique SMBIOS identifiers\n");
    }
    if is_intel_macos_vm {
        script.push_str("# macOS VM with Apple SMC emulation\n");
    }
    if needs_tpm {
        script.push_str("# TPM 2.0 enabled (requires swtpm package)\n");
    }
    if needs_tpm && needs_uefi {
        script.push_str("# Secure Boot enabled (OVMF secboot + SMM)\n");
    }
    script.push_str("# Generated by vm-curator\n\n");

    // Variables
    script.push_str("VM_DIR=\"$(dirname \"$(readlink -f \"$0\")\")\"\n");
    script.push_str(&format!("DISK=\"$VM_DIR/{}\"\n", disk_filename));

    if is_recovery_image {
        // Recovery image (DMG) variable
        if let Some(path) = iso_path {
            script.push_str(&format!(
                "RECOVERY_IMG={}\n",
                shell_escape(&path.display().to_string())
            ));
        } else {
            script.push_str("RECOVERY_IMG=\"\"\n");
        }
    } else {
        // ISO variable
        if let Some(iso) = iso_path {
            script.push_str(&format!(
                "ISO={}\n",
                shell_escape(&iso.display().to_string())
            ));
        } else {
            script.push_str("ISO=\"\"\n");
        }
    }

    // Floppy image variable
    if let Some(floppy) = floppy_path {
        script.push_str(&format!(
            "FLOPPY={}\n",
            shell_escape(&floppy.display().to_string())
        ));
    }

    // BIOS/ROM file variable
    if let Some(ref bios_path) = config.bios_path {
        let filename = bios_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| bios_path.display().to_string());
        script.push_str(&format!("ROM=\"$VM_DIR/{}\"\n", filename));
    }

    script.push('\n');

    // macOS OpenCore bootloader verification
    if is_intel_macos_vm && needs_uefi && config.bios_path.is_some() {
        script.push_str(
            r#"# Verify OpenCore bootloader exists
if [[ ! -f "$ROM" ]]; then
    echo "Error: OpenCore bootloader not found at $ROM"
    echo "Download from: https://github.com/kholia/OSX-KVM"
    echo "Place OpenCore.qcow2 in: $VM_DIR/"
    exit 1
fi

"#,
        );
    }

    // Windows-specific: SMBIOS options
    if is_windows {
        script.push_str(&generate_smbios_options());
    }

    // UEFI setup with writable OVMF_VARS
    if needs_uefi {
        script.push_str(&generate_ovmf_vars_setup(needs_tpm));
    }

    // TPM functions
    if needs_tpm {
        script.push_str(&generate_tpm_functions());
    }

    // Help function
    script.push_str("show_help() {\n");
    script.push_str("    echo \"Usage: $0 [OPTIONS]\"\n");
    script.push_str("    echo \"\"\n");
    script.push_str("    echo \"Options:\"\n");
    script.push_str("    echo \"  --install        Boot from installation media\"\n");
    script.push_str("    echo \"  --cdrom <iso>    Boot with specified ISO as CD-ROM\"\n");
    script.push_str("    echo \"  --recovery <dmg> Boot with recovery image (DMG)\"\n");
    script.push_str("    echo \"  --floppy <img>   Boot with specified floppy image\"\n");
    script.push_str("    echo \"  (no options)     Normal boot from hard disk\"\n");
    script.push_str("}\n\n");

    // Build QEMU commands with OS-awareness
    let floppy_ref = if floppy_path.is_some() {
        Some("\"$FLOPPY\"")
    } else {
        None
    };
    let base_cmd = build_qemu_command_with_os(
        config,
        disk_filename,
        &InstallMedia::None,
        os_profile,
        floppy_ref,
    );

    let install_cmd = if is_recovery_image {
        build_qemu_command_with_os(
            config,
            disk_filename,
            &InstallMedia::RecoveryImage(None),
            os_profile,
            floppy_ref,
        )
    } else {
        build_qemu_command_with_os(
            config,
            disk_filename,
            &InstallMedia::Iso(None),
            os_profile,
            floppy_ref,
        )
    };

    // Main script logic
    script.push_str("case \"$1\" in\n");
    script.push_str("    --install)\n");

    if is_recovery_image {
        script.push_str(
            "        if [[ -z \"$RECOVERY_IMG\" ]] || [[ ! -f \"$RECOVERY_IMG\" ]]; then\n",
        );
        script.push_str("            echo \"Error: Recovery image not found at $RECOVERY_IMG\"\n");
        script.push_str(
            "            echo \"Please edit this script to set the path or use --recovery\"\n",
        );
        script.push_str("            exit 1\n");
        script.push_str("        fi\n");
        script.push_str("        echo \"Booting from recovery image...\"\n");
    } else {
        script.push_str("        if [[ -z \"$ISO\" ]] || [[ ! -f \"$ISO\" ]]; then\n");
        script.push_str("            echo \"Error: Installation ISO not found at $ISO\"\n");
        script.push_str(
            "            echo \"Please edit this script to set the ISO path or use --cdrom\"\n",
        );
        script.push_str("            exit 1\n");
        script.push_str("        fi\n");
        script.push_str("        echo \"Booting from installation ISO...\"\n");
    }

    // Start TPM before QEMU if needed
    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    script.push_str(&format!("        {}\n", install_cmd));
    script.push_str("        ;;\n");

    // --cdrom option (always available)
    script.push_str("    --cdrom)\n");
    script.push_str("        if [[ -z \"$2\" ]] || [[ ! -f \"$2\" ]]; then\n");
    script.push_str("            echo \"Error: Please specify a valid ISO file\"\n");
    script.push_str("            exit 1\n");
    script.push_str("        fi\n");
    script.push_str("        echo \"Booting with CD-ROM: $2\"\n");

    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    let cdrom_cmd = build_qemu_command_with_os(
        config,
        disk_filename,
        &InstallMedia::Iso(Some("\"$2\"")),
        os_profile,
        floppy_ref,
    );
    script.push_str(&format!("        {}\n", cdrom_cmd));
    script.push_str("        ;;\n");

    // --recovery option (always available)
    script.push_str("    --recovery)\n");
    script.push_str("        if [[ -z \"$2\" ]] || [[ ! -f \"$2\" ]]; then\n");
    script.push_str("            echo \"Error: Please specify a valid DMG file\"\n");
    script.push_str("            exit 1\n");
    script.push_str("        fi\n");
    script.push_str("        echo \"Booting with recovery image: $2\"\n");

    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    let recovery_cmd = build_qemu_command_with_os(
        config,
        disk_filename,
        &InstallMedia::RecoveryImage(Some("\"$2\"")),
        os_profile,
        floppy_ref,
    );
    script.push_str(&format!("        {}\n", recovery_cmd));
    script.push_str("        ;;\n");

    // --floppy option
    script.push_str("    --floppy)\n");
    script.push_str("        if [[ -z \"$2\" ]] || [[ ! -f \"$2\" ]]; then\n");
    script.push_str("            echo \"Error: Please specify a valid floppy image file\"\n");
    script.push_str("            exit 1\n");
    script.push_str("        fi\n");
    script.push_str("        echo \"Booting with floppy: $2\"\n");

    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    let floppy_cmd = build_qemu_command_with_os(
        config,
        disk_filename,
        &InstallMedia::None,
        os_profile,
        Some("\"$2\""),
    );
    script.push_str(&format!("        {}\n", floppy_cmd));
    script.push_str("        ;;\n");

    script.push_str("    --help|-h)\n");
    script.push_str("        show_help\n");
    script.push_str("        exit 0\n");
    script.push_str("        ;;\n");
    script.push_str("    \"\")\n");
    script.push_str(&format!("        echo \"Booting {}...\"\n", vm_name));

    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    script.push_str(&format!("        {}\n", base_cmd));
    script.push_str("        ;;\n");
    script.push_str("    *)\n");
    script.push_str("        echo \"Unknown option: $1\"\n");
    script.push_str("        show_help\n");
    script.push_str("        exit 1\n");
    script.push_str("        ;;\n");
    script.push_str("esac\n");

    script
}

/// SPICE guest-agent channel — enables clipboard sharing (copy/paste) between
/// host and guest. Requires `spice-vdagent` running inside the guest. These exact
/// strings are the single source of truth shared by command generation, in-place
/// editing (`set_spice_agent_args`), and the dbus-display strip in `lifecycle.rs`.
pub(crate) const SPICE_AGENT_ARGS: &[&str] = &[
    "-device virtio-serial-pci",
    "-chardev spicevmc,id=spicechannel0,name=vdagent",
    "-device virtserialport,chardev=spicechannel0,name=com.redhat.spice.0",
];

/// Build the QEMU command string with OS-awareness
fn build_qemu_command_with_os(
    config: &WizardQemuConfig,
    _disk_filename: &str,
    install_media: &InstallMedia,
    os_profile: Option<&str>,
    floppy_path: Option<&str>,
) -> String {
    let mut args: Vec<String> = Vec::new();

    let is_windows = is_windows_10_or_11(os_profile);
    let is_intel_macos_vm = is_intel_macos(os_profile, &config.emulator);
    let needs_tpm = config.tpm || is_windows_11(os_profile);
    let needs_uefi = config.uefi || is_windows_11(os_profile);

    // Emulator
    args.push(config.emulator.clone());

    // KVM acceleration
    if config.enable_kvm {
        args.push("-enable-kvm".to_string());
    }

    // BIOS/ROM file (skip for macOS UEFI — OpenCore is handled as an AHCI drive)
    if config.bios_path.is_some() && !(is_intel_macos_vm && needs_uefi) {
        args.push("-bios \"$ROM\"".to_string());
    }

    // Machine type (escaped to prevent injection)
    if let Some(ref machine) = config.machine {
        let safe_machine = shell_escape(machine);
        let needs_secboot = needs_tpm && needs_uefi;
        let mut machine_opts = vec![safe_machine.to_string()];
        if config.enable_kvm {
            machine_opts.push("accel=kvm".to_string());
        }
        if needs_secboot {
            machine_opts.push("smm=on".to_string());
        }
        args.push(format!("-machine {}", machine_opts.join(",")));
    }

    // CPU (escaped to prevent injection)
    if let Some(ref cpu_model) = config.cpu_model {
        args.push(format!("-cpu {}", shell_escape(cpu_model)));
    }

    // SMP (CPU cores)
    args.push(format!(
        "-smp {},sockets=1,cores={},threads=1",
        config.cpu_cores, config.cpu_cores
    ));

    // Memory
    args.push(format!("-m {}M", config.memory_mb));

    // SMBIOS options for Windows (reference the variable defined in script)
    if is_windows {
        args.push("\"${SMBIOS_OPTS[@]}\"".to_string());
    }

    // Apple SMC and SMBIOS for Intel macOS
    if is_intel_macos_vm {
        args.push("-device \"isa-applesmc,osk=ourhardworkbythesewordsguardedpleasedontsteal(c)AppleComputerInc\"".to_string());
        args.push("-smbios type=2".to_string());
    }

    // UEFI boot with writable OVMF_VARS. The CODE path and format come from the
    // same firmware pair that `generate_ovmf_vars_setup` copies VARS from (both
    // call `find_ovmf_firmware` with the same flag), so CODE and VARS always
    // agree in size and on-disk format (raw vs qcow2).
    if needs_uefi {
        let needs_secboot = needs_tpm;
        let firmware = find_ovmf_firmware(needs_secboot).unwrap_or_else(default_ovmf_firmware);
        // OVMF_CODE is read-only
        args.push(format!(
            "-drive if=pflash,format={},readonly=on,file={}",
            firmware.format, firmware.code
        ));
        // OVMF_VARS is writable (uses variable set up in script)
        args.push(format!(
            "-drive if=pflash,format={},file=\"$OVMF_VARS\"",
            firmware.format
        ));

        // Secure Boot requires secure pflash protection
        if needs_secboot {
            args.push("-global driver=cfi.pflash01,property=secure,value=on".to_string());
        }
    }

    // Disk (interface escaped to prevent injection)
    // Map "sata" to "ide" for backwards compatibility — QEMU doesn't support if=sata,
    // but on Q35 machines, if=ide routes through the AHCI controller (giving SATA behavior)
    let machine_name = config.machine.as_deref().unwrap_or("");
    match machine_name {
        "q800" => {
            // q800: explicit SCSI device attachment for built-in ESP controller
            args.push("-drive file=\"$DISK\",format=qcow2,if=none,id=hd0".to_string());
            args.push("-device scsi-hd,drive=hd0".to_string());
        }
        "mac99" => {
            // mac99: explicit IDE device attachment with bus specification
            args.push("-drive file=\"$DISK\",format=qcow2,if=none,id=hd0".to_string());
            args.push("-device ide-hd,drive=hd0,bus=ide.0".to_string());
        }
        _ => {
            if is_intel_macos_vm && needs_uefi {
                // Explicit AHCI controller with predictable bus addressing for macOS
                args.push("-device ich9-ahci,id=sata".to_string());
                // OpenCore bootloader as sata.0 (if provided via bios_rom)
                if config.bios_path.is_some() {
                    args.push("-drive file=\"$ROM\",format=qcow2,if=none,id=oc".to_string());
                    args.push("-device ide-hd,drive=oc,bus=sata.0".to_string());
                }
                // Main disk (sata.1 with OpenCore, sata.0 without)
                let disk_bus = if config.bios_path.is_some() {
                    "sata.1"
                } else {
                    "sata.0"
                };
                args.push("-drive file=\"$DISK\",format=qcow2,if=none,id=maindisk".to_string());
                args.push(format!("-device ide-hd,drive=maindisk,bus={}", disk_bus));
            } else {
                let disk_if = if config.disk_interface == "sata" {
                    "ide"
                } else {
                    &config.disk_interface
                };
                args.push(format!(
                    "-drive file=\"$DISK\",format=qcow2,if={},index=0,media=disk",
                    shell_escape(disk_if)
                ));
            }
        }
    }

    // Install media (CD-ROM or recovery image)
    match install_media {
        InstallMedia::None => {}
        InstallMedia::Iso(custom_path) => {
            let iso_ref = custom_path.unwrap_or("\"$ISO\"");
            match machine_name {
                "q800" => {
                    args.push(format!(
                        "-drive file={},format=raw,if=none,id=cd0,media=cdrom",
                        iso_ref
                    ));
                    args.push("-device scsi-cd,drive=cd0".to_string());
                }
                "mac99" => {
                    args.push(format!(
                        "-drive file={},format=raw,if=none,id=cd0,media=cdrom",
                        iso_ref
                    ));
                    args.push("-device ide-cd,drive=cd0,bus=ide.1".to_string());
                }
                _ => {
                    if is_intel_macos_vm && needs_uefi {
                        // macOS UEFI: attach ISO on AHCI bus
                        let iso_bus = if config.bios_path.is_some() {
                            "sata.3"
                        } else {
                            "sata.2"
                        };
                        args.push(format!(
                            "-drive file={},format=raw,if=none,id=cd0,media=cdrom",
                            iso_ref
                        ));
                        args.push(format!("-device ide-cd,drive=cd0,bus={}", iso_bus));
                        // No -boot d for macOS (OpenCore handles boot)
                    } else {
                        args.push(format!("-drive file={},media=cdrom,index=1", iso_ref));
                        // Boot from CD-ROM
                        args.push("-boot d".to_string());
                    }
                }
            }
            if !is_intel_macos_vm || !needs_uefi {
                // Boot from CD-ROM (non-macOS or non-UEFI paths that didn't already add it)
                if machine_name == "q800" || machine_name == "mac99" {
                    args.push("-boot d".to_string());
                }
            }
        }
        InstallMedia::RecoveryImage(custom_path) => {
            let dmg_ref = custom_path.unwrap_or("\"$RECOVERY_IMG\"");
            if is_intel_macos_vm && needs_uefi {
                // macOS UEFI: attach recovery image on AHCI bus
                // No format= specified — QEMU auto-detects DMG vs qcow2
                let recovery_bus = if config.bios_path.is_some() {
                    "sata.2"
                } else {
                    "sata.1"
                };
                args.push(format!(
                    "-drive file={},snapshot=on,if=none,id=recovery",
                    dmg_ref
                ));
                args.push(format!(
                    "-device ide-hd,drive=recovery,bus={}",
                    recovery_bus
                ));
            } else {
                // Non-macOS UEFI: use legacy IDE attachment
                args.push(format!(
                    "-drive file={},snapshot=on,format=dmg,if=ide,index=2,media=disk",
                    dmg_ref
                ));
            }
            // No -boot d: OpenCore/UEFI bootloader handles boot selection
        }
    }

    // Floppy disk image
    if let Some(floppy_ref) = floppy_path {
        args.push(format!("-fda {}", floppy_ref));
        // When floppy is present with ISO, boot from floppy (which accesses CD)
        if matches!(install_media, InstallMedia::Iso(_)) {
            // Replace the -boot d we just added with -boot a
            if let Some(pos) = args.iter().position(|a| a == "-boot d") {
                args[pos] = "-boot a".to_string();
            }
        }
    }

    // VGA / Graphics (escaped to prevent injection)
    if config.gl_acceleration && config.vga == "virtio" {
        // Use virtio-vga-gl for 3D acceleration
        args.push("-device virtio-vga-gl".to_string());
    } else {
        args.push(format!("-vga {}", shell_escape(&config.vga)));
    }

    // Display (with GL if enabled, escaped to prevent injection)
    if config.gl_acceleration {
        args.push(format!("-display {},gl=on", shell_escape(&config.display)));
    } else {
        args.push(format!("-display {}", shell_escape(&config.display)));
    }

    // Audio backend (must be declared before devices that use it)
    if !config.audio.is_empty() {
        if config.display == "spice-app" {
            args.push("-audiodev spice,id=audio0".to_string());
        } else {
            args.push("-audiodev pa,id=audio0".to_string());
        }
    }

    // SPICE guest-agent channel for clipboard sharing (needs spice-vdagent in the guest)
    if config.display == "spice-app" {
        for a in SPICE_AGENT_ARGS {
            args.push((*a).to_string());
        }
    }

    // Audio devices (known safe values from profiles, but escape for safety)
    for audio in &config.audio {
        match audio.as_str() {
            "intel-hda" => args.push("-device intel-hda".to_string()),
            "hda-duplex" | "hda-output" | "hda-micro" => {
                // HDA codec devices must reference the audiodev
                args.push(format!("-device {},audiodev=audio0", shell_escape(audio)));
            }
            "ac97" => args.push("-device AC97,audiodev=audio0".to_string()),
            "sb16" => args.push("-device sb16,audiodev=audio0".to_string()),
            "screamer" => {
                // Screamer is built into the mac99 machine; no -device line needed.
                // The -audiodev backend declared above is sufficient.
            }
            _ => {
                // Unknown audio device - escape it
                args.push(format!("-device {},audiodev=audio0", shell_escape(audio)));
            }
        }
    }

    // Network (escaped to prevent injection)
    if config.network_model != "none" {
        // Map short network model names to QEMU device names
        let net_device = match config.network_model.as_str() {
            "virtio" => "virtio-net-pci".to_string(),
            other => shell_escape(other),
        };

        let mac_suffix = config
            .mac_address
            .as_deref()
            .filter(|m| crate::vm::mac::is_valid_mac(m))
            .map(|m| format!(",mac={}", m))
            .unwrap_or_default();

        match config.network_backend.as_str() {
            "none" => {
                // No networking backend (different from network_model "none")
            }
            "passt" => {
                args.push("-netdev passt,id=net0".to_string());
                args.push(format!("-device {},netdev=net0{}", net_device, mac_suffix));
            }
            "bridge" => {
                let br = config.bridge_name.as_deref().unwrap_or("qemubr0");
                args.push(format!("-netdev bridge,id=net0,br={}", shell_escape(br)));
                args.push(format!("-device {},netdev=net0{}", net_device, mac_suffix));
            }
            _ => {
                // User/SLIRP (default)
                let mut netdev = "-netdev user,id=net0".to_string();
                for pf in &config.port_forwards {
                    let proto = match pf.protocol {
                        PortProtocol::Tcp => "tcp",
                        PortProtocol::Udp => "udp",
                    };
                    netdev.push_str(&format!(
                        ",hostfwd={}::{}-:{}",
                        proto, pf.host_port, pf.guest_port
                    ));
                }
                args.push(netdev);
                args.push(format!("-device {},netdev=net0{}", net_device, mac_suffix));
            }
        }
    }

    // USB tablet for mouse (+ keyboard for macOS)
    if config.usb_tablet {
        args.push("-usb".to_string());
        if is_intel_macos_vm {
            args.push("-device usb-kbd".to_string());
        }
        args.push("-device usb-tablet".to_string());
    }

    // RTC local time (for Windows)
    if config.rtc_localtime {
        args.push("-rtc base=localtime".to_string());
    }

    // TPM 2.0 (if enabled, uses socket set up by start_tpm function)
    if needs_tpm {
        args.push("-chardev socket,id=chrtpm,path=\"$TPM_DIR/swtpm-sock\"".to_string());
        args.push("-tpmdev emulator,id=tpm0,chardev=chrtpm".to_string());
        args.push("-device tpm-tis,tpmdev=tpm0".to_string());
    }

    // Extra args - these come from QEMU profiles and are considered trusted
    // They may contain complex argument structures that shouldn't be escaped
    // (e.g., "-device virtio-vga-gl" or "-display sdl,gl=on")
    for arg in &config.extra_args {
        args.push(arg.clone());
    }

    // QMP monitor socket — enables pause/resume and live monitoring
    args.push("-qmp".to_string());
    args.push("unix:$VM_DIR/qemu.sock,server=on,wait=off".to_string());

    args.join(" \\\n        ")
}

/// Write the launch script to disk and make it executable
pub fn write_launch_script(vm_dir: &Path, content: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let script_path = vm_dir.join("launch.sh");

    fs::write(&script_path, content)
        .with_context(|| format!("Failed to write launch script: {}", script_path.display()))?;

    // Make executable (chmod +x)
    let mut perms = fs::metadata(&script_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms)
        .with_context(|| format!("Failed to set permissions on: {}", script_path.display()))?;

    Ok(script_path)
}

/// Update network arguments in an existing launch.sh script
pub fn update_network_in_script(
    vm_path: &Path,
    model: &str,
    backend: &str,
    bridge_name: Option<&str>,
    port_forwards: &[PortForward],
    mac_address: Option<&str>,
) -> Result<()> {
    let script_path = vm_path.join("launch.sh");
    let content = std::fs::read_to_string(&script_path)
        .with_context(|| format!("Failed to read launch script: {}", script_path.display()))?;

    // Build new network arguments
    let new_net_args =
        generate_network_args(model, backend, bridge_name, port_forwards, mac_address);

    // Remove existing network lines and replace
    let mut new_lines = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut replaced = false;

    // Detect whether a given trimmed line is a network arg (-netdev or a
    // network -device). Used to identify contiguous network blocks within
    // each branch of the case statement.
    fn is_network_line(trimmed: &str) -> bool {
        let is_netdev = trimmed.contains("-netdev ")
            || trimmed.contains("-net user")
            || trimmed.contains("-net bridge");
        let is_net_device = (trimmed.contains("-device ") && trimmed.contains("netdev=net0"))
            || (trimmed.contains("-device ")
                && (trimmed.contains("e1000")
                    || trimmed.contains("virtio-net")
                    || trimmed.contains("rtl8139")
                    || trimmed.contains("ne2k_pci")
                    || trimmed.contains("pcnet"))
                && !trimmed.contains("vga")
                && !trimmed.contains("audio"));
        is_netdev || is_net_device
    }

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Skip comment lines
        if trimmed.starts_with('#') {
            new_lines.push(line.to_string());
            i += 1;
            continue;
        }

        if is_network_line(trimmed) {
            // Consume the contiguous run of network-arg lines (each network
            // arg is one physical line in the script). Issue #38: insert the
            // replacement at every occurrence so all five case branches —
            // --install, --cdrom, --recovery, --floppy, normal boot — keep a
            // network device.
            while i < lines.len() && is_network_line(lines[i].trim()) {
                i += 1;
            }
            for arg in &new_net_args {
                new_lines.push(arg.clone());
            }
            replaced = true;
        } else {
            new_lines.push(line.to_string());
            i += 1;
        }
    }

    // If no network lines were found but we have new args, insert before the last non-empty line
    if !replaced && !new_net_args.is_empty() {
        // Find the last continuation line sequence and insert before it
        let insert_pos = new_lines.len().saturating_sub(2);
        for (j, arg) in new_net_args.iter().enumerate() {
            new_lines.insert(insert_pos + j, arg.clone());
        }
    }

    let new_content = new_lines.join("\n");
    // Ensure trailing newline
    let new_content = if new_content.ends_with('\n') {
        new_content
    } else {
        format!("{}\n", new_content)
    };

    std::fs::write(&script_path, new_content)
        .with_context(|| format!("Failed to write launch script: {}", script_path.display()))?;

    Ok(())
}

/// True if `line` is one of the managed SPICE guest-agent channel args
/// (ignoring indentation and a trailing line-continuation backslash).
fn is_spice_agent_line(line: &str) -> bool {
    let mut t = line.trim();
    if let Some(stripped) = t.strip_suffix('\\') {
        t = stripped.trim_end();
    }
    SPICE_AGENT_ARGS.contains(&t)
}

/// Add or remove the SPICE guest-agent channel lines in an existing launch.sh.
///
/// Idempotent: any existing channel lines are stripped first, then re-added (right
/// after each `-display` continuation line) only when `enable` is true. This lets the
/// management screen toggle clipboard support as the display backend is switched to or
/// away from `spice-app`, without duplicating or orphaning args. Indentation and the
/// trailing-backslash continuation style are matched to the surrounding `-display` line.
pub fn set_spice_agent_args(content: &str, enable: bool) -> String {
    let ends_with_newline = content.ends_with('\n');

    // Pass 1: strip any existing agent lines so repeated calls are idempotent.
    let stripped: Vec<String> = content
        .lines()
        .filter(|line| !is_spice_agent_line(line))
        .map(|s| s.to_string())
        .collect();

    let lines: Vec<String> = if enable {
        let mut out: Vec<String> = Vec::with_capacity(stripped.len() + SPICE_AGENT_ARGS.len());
        for line in stripped {
            let t = line.trim_start();
            let is_display = !t.starts_with('#') && t.starts_with("-display ");
            if !is_display {
                out.push(line);
                continue;
            }

            let indent_len = line.len() - line.trim_start().len();
            let indent = line[..indent_len].to_string();
            if line.trim_end().ends_with('\\') {
                // Mid-command display line: keep it, append continued agent lines.
                out.push(line);
                for a in SPICE_AGENT_ARGS {
                    out.push(format!("{}{} \\", indent, a));
                }
            } else {
                // Display was the command's last line: extend the continuation and
                // leave the final agent line without a trailing backslash.
                out.push(format!("{} \\", line.trim_end()));
                let last = SPICE_AGENT_ARGS.len() - 1;
                for (idx, a) in SPICE_AGENT_ARGS.iter().enumerate() {
                    if idx == last {
                        out.push(format!("{}{}", indent, a));
                    } else {
                        out.push(format!("{}{} \\", indent, a));
                    }
                }
            }
        }
        out
    } else {
        stripped
    };

    let mut s = lines.join("\n");
    if ends_with_newline {
        s.push('\n');
    }
    s
}

/// Generate network argument lines for a launch script
fn generate_network_args(
    model: &str,
    backend: &str,
    bridge_name: Option<&str>,
    port_forwards: &[PortForward],
    mac_address: Option<&str>,
) -> Vec<String> {
    if model == "none" {
        return Vec::new();
    }

    let net_device = match model {
        "virtio" => "virtio-net-pci".to_string(),
        other => shell_escape(other),
    };

    let mac_suffix = mac_address
        .filter(|m| crate::vm::mac::is_valid_mac(m))
        .map(|m| format!(",mac={}", m))
        .unwrap_or_default();

    let mut args = Vec::new();

    match backend {
        "none" => {
            // No networking backend
        }
        "passt" => {
            args.push("        -netdev passt,id=net0 \\".to_string());
            args.push(format!(
                "        -device {},netdev=net0{} \\",
                net_device, mac_suffix
            ));
        }
        "bridge" => {
            let br = bridge_name.unwrap_or("qemubr0");
            args.push(format!(
                "        -netdev bridge,id=net0,br={} \\",
                shell_escape(br)
            ));
            args.push(format!(
                "        -device {},netdev=net0{} \\",
                net_device, mac_suffix
            ));
        }
        _ => {
            // User/SLIRP
            let mut netdev = "        -netdev user,id=net0".to_string();
            for pf in port_forwards {
                let proto = match pf.protocol {
                    PortProtocol::Tcp => "tcp",
                    PortProtocol::Udp => "udp",
                };
                netdev.push_str(&format!(
                    ",hostfwd={}::{}-:{}",
                    proto, pf.host_port, pf.guest_port
                ));
            }
            netdev.push_str(" \\");
            args.push(netdev);
            args.push(format!(
                "        -device {},netdev=net0{} \\",
                net_device, mac_suffix
            ));
        }
    }

    args
}

#[cfg(test)]
#[path = "tests/create.rs"]
mod tests;
