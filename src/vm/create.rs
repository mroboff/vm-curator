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
    if s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/') {
        return s.to_string();
    }

    // Otherwise, wrap in single quotes and escape any existing single quotes
    // In shell: replace ' with '\'' (end quote, escaped quote, start quote)
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

use crate::app::{CreateWizardState, DiskAction, WizardQemuConfig};
use crate::commands::qemu_img;
use crate::vm::qemu_config::{PortForward, PortProtocol};

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
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let idx = ((state >> 33) as usize) % chars.len();
        serial.push(chars[idx]);
    }

    serial
}

/// Known OVMF firmware paths across different Linux distributions
const OVMF_SEARCH_PATHS: &[&str] = &[
    // Arch Linux (current naming with .4m suffix for 4MB variant)
    "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
    "/usr/share/edk2-ovmf/x64/OVMF_CODE.4m.fd",
    "/usr/share/OVMF/x64/OVMF_CODE.4m.fd",
    "/usr/share/ovmf/x64/OVMF_CODE.4m.fd",
    // Arch Linux (legacy naming without .4m)
    "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
    "/usr/share/edk2/x64/OVMF_CODE.fd",
    // Debian/Ubuntu
    "/usr/share/OVMF/OVMF_CODE.fd",
    "/usr/share/OVMF/OVMF_CODE_4M.fd",
    // Fedora/RHEL/CentOS
    "/usr/share/edk2/ovmf/OVMF_CODE.fd",
    "/usr/share/edk2/ovmf/OVMF_CODE.cc.fd",
    // openSUSE
    "/usr/share/qemu/ovmf-x86_64.bin",
    "/usr/share/qemu/ovmf-x86_64-code.bin",
    // NixOS
    "/run/libvirt/nix-ovmf/OVMF_CODE.fd",
    // Generic/fallback paths
    "/usr/share/ovmf/OVMF_CODE.fd",
    "/usr/share/qemu/OVMF_CODE.fd",
    "/usr/share/ovmf/x64/OVMF_CODE.fd",
];

/// Find the OVMF_CODE.fd firmware file by checking known paths
fn find_ovmf_code_path() -> Option<String> {
    for path in OVMF_SEARCH_PATHS {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Known OVMF Secure Boot firmware paths across different Linux distributions
const OVMF_SECBOOT_SEARCH_PATHS: &[&str] = &[
    // Arch Linux
    "/usr/share/edk2/x64/OVMF_CODE.secboot.4m.fd",
    "/usr/share/OVMF/x64/OVMF_CODE.secboot.4m.fd",
    "/usr/share/ovmf/x64/OVMF_CODE.secboot.4m.fd",
    "/usr/share/edk2-ovmf/x64/OVMF_CODE.secboot.fd",
    // Debian/Ubuntu
    "/usr/share/OVMF/OVMF_CODE_4M.secboot.fd",
    "/usr/share/OVMF/OVMF_CODE_4M.ms.fd",
    "/usr/share/OVMF/OVMF_CODE.secboot.fd",
    "/usr/share/OVMF/OVMF_CODE.ms.fd",
    // Fedora/RHEL
    "/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd",
    // Generic/fallback
    "/usr/share/ovmf/OVMF_CODE.secboot.fd",
    "/usr/share/qemu/OVMF_CODE.secboot.fd",
];

/// Find the OVMF Secure Boot firmware file by checking known paths
fn find_ovmf_secboot_code_path() -> Option<String> {
    for path in OVMF_SECBOOT_SEARCH_PATHS {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Find OVMF_VARS template with pre-enrolled Secure Boot keys (Microsoft keys)
fn find_ovmf_secboot_vars_template() -> Option<String> {
    let search_paths = [
        // Debian/Ubuntu (pre-enrolled Microsoft keys)
        "/usr/share/OVMF/OVMF_VARS_4M.ms.fd",
        "/usr/share/OVMF/OVMF_VARS.ms.fd",
        // Fedora/RHEL
        "/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd",
    ];

    for path in search_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    // Fall back to standard VARS template
    find_ovmf_vars_template()
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

    // Generate and write launch script with OS-awareness
    let script_content = generate_launch_script_with_os(
        &state.vm_name,
        &disk_filename,
        state.iso_path.as_deref(),
        &state.qemu_config,
        state.selected_os.as_deref(),
    );
    let launch_script_path = write_launch_script(&vm_dir, &script_content)?;

    // Write VM metadata file with custom display name
    write_vm_metadata(&vm_dir, &state.vm_name, state.selected_os.as_deref())?;

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
            fs::copy(source, &dest)
                .with_context(|| format!(
                    "Failed to copy disk from {} to {}",
                    source.display(),
                    dest.display()
                ))?;
        }
        DiskAction::Move => {
            // Try rename first (works if on same filesystem)
            if fs::rename(source, &dest).is_err() {
                // Rename failed (likely different filesystem), fall back to copy+delete
                fs::copy(source, &dest)
                    .with_context(|| format!(
                        "Failed to copy disk from {} to {}",
                        source.display(),
                        dest.display()
                    ))?;
                fs::remove_file(source)
                    .with_context(|| format!(
                        "Failed to remove original disk after copying: {}",
                        source.display()
                    ))?;
            }
        }
    }

    Ok(dest)
}

/// Write VM metadata file (vm-curator.toml)
pub fn write_vm_metadata(vm_dir: &Path, display_name: &str, os_profile: Option<&str>) -> Result<()> {
    let metadata_path = vm_dir.join("vm-curator.toml");

    let mut content = String::new();
    content.push_str("# VM Curator metadata\n\n");
    content.push_str(&format!("display_name = \"{}\"\n", display_name.replace('"', "\\\"")));

    if let Some(profile) = os_profile {
        content.push_str(&format!("os_profile = \"{}\"\n", profile));
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

/// Generate SMBIOS options for Windows to avoid corporate machine detection
fn generate_smbios_options() -> String {
    let uuid = generate_uuid();
    let system_serial = generate_serial();
    let board_serial = generate_serial();

    // Consumer-style SMBIOS that doesn't trigger corporate machine detection
    // Type 1: System Information
    // Type 2: Baseboard Information
    format!(r#"# SMBIOS configuration (unique per VM to avoid Windows corporate detection)
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
        swtpm_setup --tpmstate "$TPM_DIR" \
            --tpm2 \
            --create-ek-cert \
            --create-platform-cert \
            --allow-signing \
            --decryption \
            --overwrite
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

"#.to_string()
}

/// Generate OVMF variables setup for UEFI
fn generate_ovmf_vars_setup(needs_secboot: bool) -> String {
    // Find OVMF_VARS template (prefer secboot variant with pre-enrolled keys when needed)
    let ovmf_vars_template = if needs_secboot {
        find_ovmf_secboot_vars_template()
            .unwrap_or_else(|| "/usr/share/OVMF/OVMF_VARS.fd".to_string())
    } else {
        find_ovmf_vars_template()
            .unwrap_or_else(|| "/usr/share/OVMF/OVMF_VARS.fd".to_string())
    };

    format!(r#"# UEFI variables (writable copy per VM)
OVMF_VARS_TEMPLATE="{template}"
OVMF_VARS="$VM_DIR/OVMF_VARS.fd"

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

"#, template = ovmf_vars_template)
}

/// Find OVMF_VARS template path
fn find_ovmf_vars_template() -> Option<String> {
    let search_paths = [
        // Arch Linux (4M variant for modern UEFI)
        "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
        "/usr/share/edk2-ovmf/x64/OVMF_VARS.4m.fd",
        "/usr/share/OVMF/x64/OVMF_VARS.4m.fd",
        // Arch Linux (legacy)
        "/usr/share/edk2-ovmf/x64/OVMF_VARS.fd",
        "/usr/share/edk2/x64/OVMF_VARS.fd",
        // Debian/Ubuntu
        "/usr/share/OVMF/OVMF_VARS.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.fd",
        // Fedora/RHEL
        "/usr/share/edk2/ovmf/OVMF_VARS.fd",
        // Generic
        "/usr/share/ovmf/OVMF_VARS.fd",
        "/usr/share/qemu/OVMF_VARS.fd",
    ];

    for path in search_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Generate the launch.sh script content with OS profile awareness
pub fn generate_launch_script_with_os(
    vm_name: &str,
    disk_filename: &str,
    iso_path: Option<&Path>,
    config: &WizardQemuConfig,
    os_profile: Option<&str>,
) -> String {
    let mut script = String::new();

    let is_windows = is_windows_10_or_11(os_profile);
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

    if let Some(iso) = iso_path {
        // Shell-escape the ISO path to prevent command injection
        script.push_str(&format!("ISO={}\n", shell_escape(&iso.display().to_string())));
    } else {
        script.push_str("ISO=\"\"\n");
    }
    script.push('\n');

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
    script.push_str("    echo \"  (no options)     Normal boot from hard disk\"\n");
    script.push_str("}\n\n");

    // Build QEMU commands with OS-awareness
    let base_cmd = build_qemu_command_with_os(config, disk_filename, false, None, os_profile);
    let install_cmd = build_qemu_command_with_os(config, disk_filename, true, None, os_profile);

    // Main script logic
    script.push_str("case \"$1\" in\n");
    script.push_str("    --install)\n");
    script.push_str("        if [[ -z \"$ISO\" ]] || [[ ! -f \"$ISO\" ]]; then\n");
    script.push_str("            echo \"Error: Installation ISO not found at $ISO\"\n");
    script.push_str("            echo \"Please edit this script to set the ISO path or use --cdrom\"\n");
    script.push_str("            exit 1\n");
    script.push_str("        fi\n");
    script.push_str("        echo \"Booting from installation ISO...\"\n");

    // Start TPM before QEMU if needed
    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    script.push_str(&format!("        {}\n", install_cmd));
    script.push_str("        ;;\n");
    script.push_str("    --cdrom)\n");
    script.push_str("        if [[ -z \"$2\" ]] || [[ ! -f \"$2\" ]]; then\n");
    script.push_str("            echo \"Error: Please specify a valid ISO file\"\n");
    script.push_str("            exit 1\n");
    script.push_str("        fi\n");
    script.push_str("        echo \"Booting with CD-ROM: $2\"\n");

    if needs_tpm {
        script.push_str("        start_tpm\n");
    }

    // Build command for custom ISO (will substitute $2)
    let cdrom_cmd = build_qemu_command_with_os(config, disk_filename, true, Some("\"$2\""), os_profile);
    script.push_str(&format!("        {}\n", cdrom_cmd));
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

/// Build the QEMU command string with OS-awareness
fn build_qemu_command_with_os(
    config: &WizardQemuConfig,
    _disk_filename: &str,
    with_cdrom: bool,
    custom_iso: Option<&str>,
    os_profile: Option<&str>,
) -> String {
    let mut args: Vec<String> = Vec::new();

    let is_windows = is_windows_10_or_11(os_profile);
    let needs_tpm = config.tpm || is_windows_11(os_profile);
    let needs_uefi = config.uefi || is_windows_11(os_profile);

    // Emulator
    args.push(config.emulator.clone());

    // KVM acceleration
    if config.enable_kvm {
        args.push("-enable-kvm".to_string());
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

    // UEFI boot with writable OVMF_VARS
    if needs_uefi {
        let needs_secboot = needs_tpm;
        let ovmf_code = if needs_secboot {
            find_ovmf_secboot_code_path()
                .or_else(find_ovmf_code_path)
                .unwrap_or_else(|| "/usr/share/OVMF/OVMF_CODE.fd".to_string())
        } else {
            find_ovmf_code_path()
                .unwrap_or_else(|| "/usr/share/OVMF/OVMF_CODE.fd".to_string())
        };
        // OVMF_CODE is read-only
        args.push(format!(
            "-drive if=pflash,format=raw,readonly=on,file={}",
            ovmf_code
        ));
        // OVMF_VARS is writable (uses variable set up in script)
        args.push("-drive if=pflash,format=raw,file=\"$OVMF_VARS\"".to_string());

        // Secure Boot requires secure pflash protection
        if needs_secboot {
            args.push("-global driver=cfi.pflash01,property=secure,value=on".to_string());
        }
    }

    // Disk (interface escaped to prevent injection)
    // Map "sata" to "ide" for backwards compatibility — QEMU doesn't support if=sata,
    // but on Q35 machines, if=ide routes through the AHCI controller (giving SATA behavior)
    let disk_if = if config.disk_interface == "sata" {
        "ide"
    } else {
        &config.disk_interface
    };
    args.push(format!(
        "-drive file=\"$DISK\",format=qcow2,if={},index=0,media=disk",
        shell_escape(disk_if)
    ));

    // CD-ROM (for install mode)
    if with_cdrom {
        let iso_ref = custom_iso.unwrap_or("\"$ISO\"");
        args.push(format!(
            "-drive file={},media=cdrom,index=1",
            iso_ref
        ));
        // Boot from CD-ROM
        args.push("-boot d".to_string());
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
        args.push("-audiodev pa,id=audio0".to_string());
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

        match config.network_backend.as_str() {
            "none" => {
                // No networking backend (different from network_model "none")
            }
            "passt" => {
                args.push("-netdev passt,id=net0".to_string());
                args.push(format!("-device {},netdev=net0", net_device));
            }
            "bridge" => {
                let br = config.bridge_name.as_deref().unwrap_or("qemubr0");
                args.push(format!("-netdev bridge,id=net0,br={}", shell_escape(br)));
                args.push(format!("-device {},netdev=net0", net_device));
            }
            _ => {
                // User/SLIRP (default)
                let mut netdev = "-netdev user,id=net0".to_string();
                for pf in &config.port_forwards {
                    let proto = match pf.protocol {
                        PortProtocol::Tcp => "tcp",
                        PortProtocol::Udp => "udp",
                    };
                    netdev.push_str(&format!(",hostfwd={}::{}-:{}", proto, pf.host_port, pf.guest_port));
                }
                args.push(netdev);
                args.push(format!("-device {},netdev=net0", net_device));
            }
        }
    }

    // USB tablet for mouse
    if config.usb_tablet {
        args.push("-usb".to_string());
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
) -> Result<()> {
    let script_path = vm_path.join("launch.sh");
    let content = std::fs::read_to_string(&script_path)
        .with_context(|| format!("Failed to read launch script: {}", script_path.display()))?;

    // Build new network arguments
    let new_net_args = generate_network_args(model, backend, bridge_name, port_forwards);

    // Remove existing network lines and replace
    let mut new_lines = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut replaced = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Skip comment lines
        if trimmed.starts_with('#') {
            new_lines.push(line.to_string());
            i += 1;
            continue;
        }

        // Check if this line contains network args
        let is_netdev = trimmed.contains("-netdev ") || trimmed.contains("-net user") || trimmed.contains("-net bridge");
        let is_net_device = (trimmed.contains("-device ") && trimmed.contains("netdev=net0"))
            || (trimmed.contains("-device ") && (trimmed.contains("e1000") || trimmed.contains("virtio-net") || trimmed.contains("rtl8139") || trimmed.contains("ne2k_pci") || trimmed.contains("pcnet")) && !trimmed.contains("vga") && !trimmed.contains("audio"));

        if is_netdev || is_net_device {
            // Skip this line (and continuation lines with backslash)
            while i < lines.len() && lines[i].trim_end().ends_with('\\') {
                i += 1;
            }
            i += 1; // skip the last line of this group

            // Insert replacement on first network line removal
            if !replaced {
                for arg in &new_net_args {
                    new_lines.push(arg.clone());
                }
                replaced = true;
            }
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

/// Generate network argument lines for a launch script
fn generate_network_args(
    model: &str,
    backend: &str,
    bridge_name: Option<&str>,
    port_forwards: &[PortForward],
) -> Vec<String> {
    if model == "none" {
        return Vec::new();
    }

    let net_device = match model {
        "virtio" => "virtio-net-pci".to_string(),
        other => shell_escape(other),
    };

    let mut args = Vec::new();

    match backend {
        "none" => {
            // No networking backend
        }
        "passt" => {
            args.push("        -netdev passt,id=net0 \\".to_string());
            args.push(format!("        -device {},netdev=net0 \\", net_device));
        }
        "bridge" => {
            let br = bridge_name.unwrap_or("qemubr0");
            args.push(format!("        -netdev bridge,id=net0,br={} \\", shell_escape(br)));
            args.push(format!("        -device {},netdev=net0 \\", net_device));
        }
        _ => {
            // User/SLIRP
            let mut netdev = "        -netdev user,id=net0".to_string();
            for pf in port_forwards {
                let proto = match pf.protocol {
                    PortProtocol::Tcp => "tcp",
                    PortProtocol::Udp => "udp",
                };
                netdev.push_str(&format!(",hostfwd={}::{}-:{}", proto, pf.host_port, pf.guest_port));
            }
            netdev.push_str(" \\");
            args.push(netdev);
            args.push(format!("        -device {},netdev=net0 \\", net_device));
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::CreateWizardState;

    #[test]
    fn test_shell_escape_safe_strings() {
        // Safe strings should pass through unchanged
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("path/to/file.iso"), "path/to/file.iso");
        assert_eq!(shell_escape("my-vm_name.qcow2"), "my-vm_name.qcow2");
    }

    #[test]
    fn test_shell_escape_unsafe_strings() {
        // Strings with spaces
        assert_eq!(shell_escape("hello world"), "'hello world'");
        // Strings with quotes
        assert_eq!(shell_escape("it's a test"), "'it'\\''s a test'");
        // Strings with shell metacharacters
        assert_eq!(shell_escape("test; echo pwned"), "'test; echo pwned'");
        assert_eq!(shell_escape("$(whoami)"), "'$(whoami)'");
        assert_eq!(shell_escape("`whoami`"), "'`whoami`'");
        assert_eq!(shell_escape("test\"; echo pwned; echo \""), "'test\"; echo pwned; echo \"'");
    }

    #[test]
    fn test_generate_folder_name() {
        assert_eq!(CreateWizardState::generate_folder_name("Windows 10"), "windows-10");
        assert_eq!(CreateWizardState::generate_folder_name("Debian GNU/Linux"), "debian-gnu-linux");
        assert_eq!(CreateWizardState::generate_folder_name("MS-DOS 6.22"), "ms-dos-6-22");
        assert_eq!(CreateWizardState::generate_folder_name("  Spaced  Out  "), "spaced-out");
    }

    #[test]
    fn test_generate_launch_script() {
        let config = WizardQemuConfig::default();
        let script = generate_launch_script_with_os(
            "Test VM",
            "test.qcow2",
            Some(Path::new("/tmp/test.iso")),
            &config,
            None,
        );

        assert!(script.contains("#!/bin/bash"));
        assert!(script.contains("Test VM"));
        assert!(script.contains("test.qcow2"));
        assert!(script.contains("/tmp/test.iso"));
        assert!(script.contains("--install"));
        assert!(script.contains("--cdrom"));
    }

    #[test]
    fn test_build_qemu_command_basic() {
        let config = WizardQemuConfig {
            emulator: "qemu-system-x86_64".to_string(),
            memory_mb: 2048,
            cpu_cores: 2,
            cpu_model: Some("host".to_string()),
            machine: Some("q35".to_string()),
            vga: "std".to_string(),
            audio: vec![],
            network_model: "e1000".to_string(),
            disk_interface: "ide".to_string(),
            enable_kvm: true,
            uefi: false,
            tpm: false,
            rtc_localtime: false,
            usb_tablet: true,
            display: "gtk".to_string(),
            gl_acceleration: false,
            network_backend: "user".to_string(),
            port_forwards: vec![],
            bridge_name: None,
            extra_args: vec![],
        };

        let cmd = build_qemu_command_with_os(&config, "disk.qcow2", false, None, None);

        assert!(cmd.contains("qemu-system-x86_64"));
        assert!(cmd.contains("-enable-kvm"));
        assert!(cmd.contains("-m 2048M"));
        assert!(cmd.contains("-smp 2"));
        assert!(cmd.contains("-vga std"));
        assert!(cmd.contains("-display gtk"));
        assert!(cmd.contains("-device e1000"));
        assert!(cmd.contains("-usb"));
        assert!(cmd.contains("-device usb-tablet"));
    }

    #[test]
    fn test_build_qemu_command_with_cdrom() {
        let config = WizardQemuConfig::default();
        let cmd = build_qemu_command_with_os(&config, "disk.qcow2", true, None, None);

        assert!(cmd.contains("-drive file=\"$ISO\",media=cdrom"));
        assert!(cmd.contains("-boot d"));
    }

    #[test]
    fn test_generate_network_args_user_with_portfwd() {
        let forwards = vec![
            PortForward { protocol: PortProtocol::Tcp, host_port: 2222, guest_port: 22 },
            PortForward { protocol: PortProtocol::Tcp, host_port: 8080, guest_port: 80 },
        ];
        let args = generate_network_args("e1000", "user", None, &forwards);
        assert_eq!(args.len(), 2);
        assert!(args[0].contains("hostfwd=tcp::2222-:22"));
        assert!(args[0].contains("hostfwd=tcp::8080-:80"));
        assert!(args[1].contains("e1000,netdev=net0"));
    }

    #[test]
    fn test_generate_network_args_passt() {
        let args = generate_network_args("virtio", "passt", None, &[]);
        assert_eq!(args.len(), 2);
        assert!(args[0].contains("-netdev passt,id=net0"));
        assert!(args[1].contains("virtio-net-pci,netdev=net0"));
    }

    #[test]
    fn test_generate_network_args_bridge() {
        let args = generate_network_args("e1000", "bridge", Some("virbr0"), &[]);
        assert_eq!(args.len(), 2);
        assert!(args[0].contains("-netdev bridge,id=net0,br=virbr0"));
    }

    #[test]
    fn test_generate_network_args_none() {
        let args = generate_network_args("none", "user", None, &[]);
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_qemu_command_with_audio() {
        let config = WizardQemuConfig {
            audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
            ..Default::default()
        };

        let cmd = build_qemu_command_with_os(&config, "disk.qcow2", false, None, None);

        assert!(cmd.contains("-audiodev pa,id=audio0"));
        assert!(cmd.contains("-device intel-hda"));
        assert!(cmd.contains("-device hda-duplex,audiodev=audio0"));
    }
}
