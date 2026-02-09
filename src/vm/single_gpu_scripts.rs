//! Single GPU Passthrough Script Generation
//!
//! Generates scripts for single GPU passthrough scenarios where the user's primary
//! GPU is passed to a VM, requiring the display manager to be stopped.
//!
//! For single-GPU passthrough, Looking Glass is NOT used because the display
//! goes directly to physical monitors connected to the passed-through GPU.

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// Lazy-compiled regexes for QEMU command extraction/modification
// These are used in extract_qemu_command_for_passthrough() and compiled once

/// Regex to match -name "..." arguments
static RE_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"-name\s+["'][^"']+["']"#).expect("Invalid regex: RE_NAME")
});

/// Regex to match -display arguments
static RE_DISPLAY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-display\s+\S+(,\S+)*").expect("Invalid regex: RE_DISPLAY")
});

/// Regex to match -vga arguments
static RE_VGA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-vga\s+\S+").expect("Invalid regex: RE_VGA")
});

/// Regex to match -audiodev arguments
static RE_AUDIODEV: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-audiodev\s+\S+(,\S+)*").expect("Invalid regex: RE_AUDIODEV")
});

/// Regex to match sound/audio device arguments
static RE_SOUNDHW: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-device\s+(intel-hda|ich9-intel-hda|hda-duplex|hda-micro|hda-output|AC97|sb16)[,\s]?[^\s\\]*")
        .expect("Invalid regex: RE_SOUNDHW")
});

/// Regex to match -cdrom arguments
static RE_CDROM: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"-cdrom\s+("[^"]+"|'[^']+'|\$\w+|\S+)"#).expect("Invalid regex: RE_CDROM")
});

/// Regex to match -drive with media=cdrom
static RE_DRIVE_CDROM: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-drive\s+[^\s\\]*media=cdrom[^\s\\]*").expect("Invalid regex: RE_DRIVE_CDROM")
});

/// Regex to match -drive with $ISO variable
static RE_DRIVE_ISO: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"-drive\s+[^\s\\]*file="\$ISO"[^\s\\]*"#).expect("Invalid regex: RE_DRIVE_ISO")
});

/// Regex to match empty continuation lines
static RE_EMPTY_CONT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\\\n\s*\\\n").expect("Invalid regex: RE_EMPTY_CONT")
});

/// Regex to match -cpu host
static RE_CPU_HOST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-cpu\s+host\b").expect("Invalid regex: RE_CPU_HOST")
});

/// Regex to match -machine arguments
static RE_MACHINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-machine\s+(\S+)").expect("Invalid regex: RE_MACHINE")
});

/// Regex to match -boot order=d
static RE_BOOT_D: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-boot\s+order=d\b").expect("Invalid regex: RE_BOOT_D")
});

use crate::hardware::SingleGpuConfig;
use crate::vm::lifecycle::{load_pci_passthrough, load_usb_passthrough};
use crate::vm::DiscoveredVm;

/// Generated scripts for single GPU passthrough
#[derive(Debug)]
pub struct GeneratedScripts {
    /// Path to the start script
    pub start_script: PathBuf,
    /// Path to the restore script
    pub restore_script: PathBuf,
}

/// Components extracted from the VM's launch.sh script
#[derive(Debug, Default)]
struct LaunchScriptComponents {
    /// DISK variable definition (e.g., DISK="$VM_DIR/disk.qcow2")
    disk_var: Option<String>,
    /// ISO variable definition
    iso_var: Option<String>,
    /// OVMF_CODE path
    ovmf_code: Option<String>,
    /// OVMF_VARS path (editable copy)
    ovmf_vars: Option<String>,
    /// TPM directory path
    tpm_dir: Option<String>,
    /// Whether TPM is enabled
    has_tpm: bool,
    /// Whether UEFI is enabled
    has_uefi: bool,
    /// SMBIOS_OPTS array definition
    smbios_opts: Option<String>,
}

/// Parse the launch.sh script to extract important components
fn parse_launch_script(content: &str) -> LaunchScriptComponents {
    let mut components = LaunchScriptComponents::default();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Check for DISK variable
        if trimmed.starts_with("DISK=") {
            components.disk_var = Some(trimmed.to_string());
        }

        // Check for ISO variable
        if trimmed.starts_with("ISO=") {
            components.iso_var = Some(trimmed.to_string());
        }

        // Check for OVMF paths
        if trimmed.starts_with("OVMF_CODE=") {
            if let Some(path) = extract_quoted_value(trimmed, "OVMF_CODE=") {
                components.ovmf_code = Some(path);
            }
        }
        if trimmed.starts_with("OVMF_VARS=") {
            if let Some(path) = extract_quoted_value(trimmed, "OVMF_VARS=") {
                components.ovmf_vars = Some(path);
            }
        }

        // Check for TPM
        if trimmed.starts_with("TPM_DIR=") {
            if let Some(path) = extract_quoted_value(trimmed, "TPM_DIR=") {
                components.tpm_dir = Some(path);
            }
            components.has_tpm = true;
        }
        if trimmed.contains("-tpmdev") || trimmed.contains("swtpm") {
            components.has_tpm = true;
        }

        // Check for UEFI
        if trimmed.contains("OVMF") || trimmed.contains("pflash") {
            components.has_uefi = true;
        }

        // Check for SMBIOS_OPTS (may be multi-line array)
        if trimmed.starts_with("SMBIOS_OPTS=(") {
            // Multi-line array - collect until closing )
            let mut smbios_block = String::new();
            smbios_block.push_str(lines[i]);
            smbios_block.push('\n');

            // Check if array closes on same line
            if !trimmed.ends_with(')') || trimmed == "SMBIOS_OPTS=(" {
                i += 1;
                while i < lines.len() {
                    smbios_block.push_str(lines[i]);
                    smbios_block.push('\n');
                    if lines[i].trim().ends_with(')') {
                        break;
                    }
                    i += 1;
                }
            }
            components.smbios_opts = Some(smbios_block);
        } else if trimmed.starts_with("SMBIOS_OPTS=") {
            // Single-line assignment
            components.smbios_opts = Some(trimmed.to_string());
        }

        i += 1;
    }

    components
}

/// Extract a quoted value from a variable assignment
fn extract_quoted_value(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    let rest = rest.trim();
    if rest.starts_with('"') && rest.len() > 1 {
        let end = rest[1..].find('"').map(|i| i + 1)?;
        Some(rest[1..end].to_string())
    } else if rest.starts_with('\'') && rest.len() > 1 {
        let end = rest[1..].find('\'').map(|i| i + 1)?;
        Some(rest[1..end].to_string())
    } else {
        Some(rest.split_whitespace().next()?.to_string())
    }
}

/// Generate all single GPU passthrough scripts for a VM
pub fn generate_single_gpu_scripts(
    vm: &DiscoveredVm,
    config: &SingleGpuConfig,
) -> Result<GeneratedScripts> {
    let vm_dir = &vm.path;

    // Save the config so it can be loaded later for regeneration
    crate::hardware::save_config(vm_dir, config)
        .with_context(|| "Failed to save single-GPU config")?;

    // Generate the start script
    let start_content = generate_start_script(vm, config)?;
    let start_path = vm_dir.join("single-gpu-start.sh");
    write_executable_script(&start_path, &start_content)?;

    // Generate the restore script
    let restore_content = generate_restore_script(vm, config);
    let restore_path = vm_dir.join("single-gpu-restore.sh");
    write_executable_script(&restore_path, &restore_content)?;

    // Note: System setup (VFIO modules, initramfs) is done once via Settings,
    // not per-VM. The setup functionality is in run_system_setup().

    Ok(GeneratedScripts {
        start_script: start_path,
        restore_script: restore_path,
    })
}

/// Write a script file and make it executable
fn write_executable_script(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)
        .with_context(|| format!("Failed to write script: {:?}", path))?;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;

    Ok(())
}

/// Generate the main start script
fn generate_start_script(vm: &DiscoveredVm, config: &SingleGpuConfig) -> Result<String> {
    let gpu_addr = &config.gpu.address;
    let audio_addr = config.audio.as_ref().map(|a| a.address.as_str()).unwrap_or("");
    let original_driver = config.original_driver.module_name();
    let display_manager = config.display_manager.service_name();
    let vm_dir = vm.path.display();
    let vm_name = vm.display_name();

    // Read and parse the launch script
    let launch_script = fs::read_to_string(&vm.launch_script)
        .with_context(|| format!("Failed to read launch script: {:?}", vm.launch_script))?;
    let components = parse_launch_script(&launch_script);

    // Get dependent modules to unload
    let modules_to_unload = config.original_driver.dependent_modules();
    let unload_modules_cmd = if !modules_to_unload.is_empty() {
        modules_to_unload
            .iter()
            .map(|m| format!("    modprobe -r {} 2>/dev/null || true", m))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "    # No additional modules to unload".to_string()
    };

    // Load USB passthrough from launch.sh
    let usb_devices = load_usb_passthrough(vm);
    let usb_passthrough_args = generate_usb_passthrough_args(&usb_devices);

    // Load PCI passthrough from launch.sh (network cards, USB controllers, etc.)
    let pci_passthrough_args = load_pci_passthrough(vm);

    // Extract extra PCI addresses for binding (NICs, USB controllers, NVMe, etc.)
    let extra_pci_addrs = extract_pci_addresses(&pci_passthrough_args);
    let extra_pci_addrs_str = if extra_pci_addrs.is_empty() {
        "EXTRA_PCI_ADDRS=()".to_string()
    } else {
        format!(
            "EXTRA_PCI_ADDRS=({})",
            extra_pci_addrs
                .iter()
                .map(|a| format!("\"{}\"", a))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    // Build QEMU command from existing launch.sh
    let qemu_command = extract_qemu_command_for_passthrough(vm, config, &components, &usb_passthrough_args, &pci_passthrough_args)?;

    // Generate variable definitions
    let variable_defs = generate_variable_definitions(vm, &components);

    // Generate TPM functions if TPM is enabled
    let tpm_functions = if components.has_tpm {
        generate_tpm_functions(&components)
    } else {
        String::new()
    };

    // Generate TPM start call
    let tpm_start = if components.has_tpm {
        r#"
# Start TPM emulator
start_tpm
"#.to_string()
    } else {
        String::new()
    };

    // Generate cleanup for NVIDIA module loading
    let nvidia_module_load = if original_driver == "nvidia" {
        r#"
    # Load NVIDIA modules in dependency order
    modprobe nvidia 2>/dev/null || true
    sleep 1
    modprobe nvidia_modeset 2>/dev/null || true
    sleep 0.5
    modprobe nvidia_drm 2>/dev/null || true
    modprobe nvidia_uvm 2>/dev/null || true
    sleep 1"#.to_string()
    } else {
        format!(r#"
    modprobe "{}" 2>/dev/null || true
    sleep 1"#, original_driver)
    };

    let script = format!(
        r#"#!/bin/bash
# Single GPU Passthrough Start Script
# Generated by vm-curator for: {vm_name}
#
# This script must be run from a TTY (Ctrl+Alt+F3), not from a graphical terminal.
# It will stop your display manager, pass your GPU to the VM, and restore
# the display when the VM exits.
#
# For single-GPU passthrough, the VM's display goes directly to physical monitors.

set -e

# ============================================================================
# Configuration
# ============================================================================
VM_DIR="{vm_dir}"
VM_NAME="{vm_name}"
GPU_ADDR="{gpu_addr}"
AUDIO_ADDR="{audio_addr}"
ORIGINAL_DRIVER="{original_driver}"
DISPLAY_MANAGER="{display_manager}"
{extra_pci_addrs}
{variable_defs}
# ============================================================================
# Safety Checks
# ============================================================================

# Must run as root or with sudo
if [[ $EUID -ne 0 ]]; then
    echo "This script must be run as root (use sudo)"
    exit 1
fi

# Must run from TTY, not graphical terminal
if [[ -n "$DISPLAY" ]] || [[ -n "$WAYLAND_DISPLAY" ]]; then
    echo "ERROR: This script must be run from a TTY (Ctrl+Alt+F3), not a graphical terminal"
    echo ""
    echo "Instructions:"
    echo "  1. Press Ctrl+Alt+F3 to switch to TTY3"
    echo "  2. Log in with your username"
    echo "  3. Run: sudo $0"
    exit 1
fi

# Check that VFIO modules are available
if ! modinfo vfio_pci &>/dev/null; then
    echo "ERROR: vfio_pci module not available"
    echo "Run System Setup from vm-curator Settings > Single GPU Passthrough"
    exit 1
fi
{tpm_functions}
# ============================================================================
# Cleanup Function
# ============================================================================

cleanup() {{
    local exit_code=$?
    echo ""
    echo "Cleaning up and restoring display..."

    # Kill any lingering QEMU processes for this VM
    pkill -f "qemu.*$VM_NAME" 2>/dev/null || true
{tpm_cleanup}
    # Unbind from vfio-pci using PCI remove+rescan pattern
    if [[ -e "/sys/bus/pci/devices/$GPU_ADDR" ]]; then
        echo "Removing GPU from PCI bus..."
        echo 1 > /sys/bus/pci/devices/$GPU_ADDR/remove 2>/dev/null || true
    fi
    if [[ -n "$AUDIO_ADDR" ]] && [[ -e "/sys/bus/pci/devices/$AUDIO_ADDR" ]]; then
        echo 1 > /sys/bus/pci/devices/$AUDIO_ADDR/remove 2>/dev/null || true
    fi

    # Remove extra PCI devices from bus (will be re-bound on rescan)
    for addr in "${{EXTRA_PCI_ADDRS[@]}}"; do
        if [[ -e "/sys/bus/pci/devices/$addr" ]]; then
            echo 1 > /sys/bus/pci/devices/$addr/remove 2>/dev/null || true
        fi
    done
    sleep 2

    # Rescan PCI bus
    echo "Rescanning PCI bus..."
    echo 1 > /sys/bus/pci/rescan
    sleep 3

    # Unload VFIO modules
    modprobe -r vfio_pci 2>/dev/null || true
    modprobe -r vfio_iommu_type1 2>/dev/null || true
    modprobe -r vfio 2>/dev/null || true
    sleep 1
{nvidia_module_load}

    # Manual bind fallback if GPU doesn't auto-bind
    if [[ -e "/sys/bus/pci/devices/$GPU_ADDR" ]] && [[ ! -e "/sys/bus/pci/devices/$GPU_ADDR/driver" ]]; then
        echo "Manual bind to $ORIGINAL_DRIVER..."
        echo "$GPU_ADDR" > /sys/bus/pci/drivers/$ORIGINAL_DRIVER/bind 2>/dev/null || true
    fi

    # Restart display manager
    echo "Starting display manager..."
    systemctl start "$DISPLAY_MANAGER" 2>/dev/null || true

    echo "Display restored."
    exit $exit_code
}}

trap cleanup EXIT INT TERM

# ============================================================================
# Stop Display Manager
# ============================================================================

echo "Stopping display manager ($DISPLAY_MANAGER)..."
systemctl stop "$DISPLAY_MANAGER"
sleep 2

# For NVIDIA, stop persistence daemon
if [[ "$ORIGINAL_DRIVER" == "nvidia" ]]; then
    systemctl stop nvidia-persistenced 2>/dev/null || true
    sleep 1
fi

# ============================================================================
# Unload GPU Driver
# ============================================================================

echo "Unloading GPU driver modules..."

# Kill any processes using the GPU (try common ones)
for proc in Xorg Xwayland gnome-shell kwin_wayland plasmashell sway hyprland; do
    pkill -9 "$proc" 2>/dev/null || true
done
sleep 2

# Unload driver modules
{unload_modules_cmd}

# Verify driver is unloaded
if [[ -e "/sys/bus/pci/devices/$GPU_ADDR/driver" ]]; then
    current_driver=$(basename $(readlink /sys/bus/pci/devices/$GPU_ADDR/driver))
    if [[ "$current_driver" != "vfio-pci" ]]; then
        echo "$GPU_ADDR" > /sys/bus/pci/drivers/$current_driver/unbind 2>/dev/null || true
    fi
fi

if [[ -n "$AUDIO_ADDR" ]] && [[ -e "/sys/bus/pci/devices/$AUDIO_ADDR/driver" ]]; then
    current_driver=$(basename $(readlink /sys/bus/pci/devices/$AUDIO_ADDR/driver))
    if [[ "$current_driver" != "vfio-pci" ]]; then
        echo "$AUDIO_ADDR" > /sys/bus/pci/drivers/$current_driver/unbind 2>/dev/null || true
    fi
fi

# ============================================================================
# Bind to VFIO
# ============================================================================

echo "Binding GPU to vfio-pci..."

modprobe vfio_pci

# Set driver override and bind
echo "vfio-pci" > /sys/bus/pci/devices/$GPU_ADDR/driver_override
echo "$GPU_ADDR" > /sys/bus/pci/drivers/vfio-pci/bind

if [[ -n "$AUDIO_ADDR" ]]; then
    echo "vfio-pci" > /sys/bus/pci/devices/$AUDIO_ADDR/driver_override
    echo "$AUDIO_ADDR" > /sys/bus/pci/drivers/vfio-pci/bind
fi

# Bind extra PCI devices (network cards, USB controllers, NVMe, etc.)
for addr in "${{EXTRA_PCI_ADDRS[@]}}"; do
    echo "Binding $addr to vfio-pci..."
    # Unbind from current driver if bound
    if [[ -e "/sys/bus/pci/devices/$addr/driver" ]]; then
        current_driver=$(basename $(readlink /sys/bus/pci/devices/$addr/driver))
        if [[ "$current_driver" != "vfio-pci" ]]; then
            echo "$addr" > /sys/bus/pci/drivers/$current_driver/unbind 2>/dev/null || true
        fi
    fi
    echo "vfio-pci" > /sys/bus/pci/devices/$addr/driver_override
    echo "$addr" > /sys/bus/pci/drivers/vfio-pci/bind
done

# Verify binding
if [[ ! -e "/sys/bus/pci/drivers/vfio-pci/$GPU_ADDR" ]]; then
    echo "ERROR: Failed to bind GPU to vfio-pci"
    exit 1
fi

echo "GPU successfully bound to vfio-pci"
{tpm_start}
# ============================================================================
# Start VM
# ============================================================================

echo ""
echo "============================================"
echo "Starting VM: $VM_NAME"
echo "============================================"
echo "Note: Display will appear on your physical monitor(s)"
echo ""

# Run QEMU (no 'exec' so cleanup trap runs)
cd "$VM_DIR"
{qemu_command}

echo ""
echo "VM has exited."
# Cleanup will run via trap
"#,
        vm_name = vm_name,
        vm_dir = vm_dir,
        gpu_addr = gpu_addr,
        audio_addr = audio_addr,
        original_driver = original_driver,
        display_manager = display_manager,
        extra_pci_addrs = extra_pci_addrs_str,
        variable_defs = variable_defs,
        tpm_functions = tpm_functions,
        tpm_cleanup = if components.has_tpm {
            r#"
    # Kill TPM emulator
    if [[ -n "$TPM_DIR" ]]; then
        pkill -f "swtpm.*$TPM_DIR" 2>/dev/null || true
    fi
"#
        } else { "" },
        nvidia_module_load = nvidia_module_load,
        unload_modules_cmd = unload_modules_cmd,
        tpm_start = tpm_start,
        qemu_command = qemu_command,
    );

    Ok(script)
}

/// Generate variable definitions from launch.sh components
fn generate_variable_definitions(vm: &DiscoveredVm, components: &LaunchScriptComponents) -> String {
    let mut vars = Vec::new();

    // Add disk variable
    if let Some(ref disk_var) = components.disk_var {
        vars.push(disk_var.clone());
    } else if let Some(disk) = vm.config.disks.first() {
        vars.push(format!("DISK=\"{}\"", disk.path.display()));
    }

    // Note: ISO variable intentionally NOT included - single-GPU passthrough is for
    // running installed VMs, not installation. Use standard launch.sh for installation.

    // Add OVMF paths if UEFI
    if components.has_uefi {
        if let Some(ref ovmf_code) = components.ovmf_code {
            vars.push(format!("OVMF_CODE=\"{}\"", ovmf_code));
        }
        if let Some(ref ovmf_vars) = components.ovmf_vars {
            vars.push(format!("OVMF_VARS=\"{}\"", ovmf_vars));
        }
    }

    // Add TPM directory if TPM enabled
    if components.has_tpm {
        if let Some(ref tpm_dir) = components.tpm_dir {
            vars.push(format!("TPM_DIR=\"{}\"", tpm_dir));
        } else {
            vars.push(format!("TPM_DIR=\"{}/tpm\"", vm.path.display()));
        }
    }

    // Add SMBIOS opts if present
    if let Some(ref smbios) = components.smbios_opts {
        vars.push(smbios.clone());
    }

    if vars.is_empty() {
        String::new()
    } else {
        vars.join("\n") + "\n"
    }
}

/// Generate TPM initialization and start functions
fn generate_tpm_functions(components: &LaunchScriptComponents) -> String {
    let tpm_dir = components.tpm_dir.as_deref().unwrap_or("$VM_DIR/tpm");

    format!(r#"
# ============================================================================
# TPM Functions
# ============================================================================

init_tpm() {{
    if [[ ! -d "{tpm_dir}" ]]; then
        echo "Initializing TPM state directory..."
        mkdir -p "{tpm_dir}"
        swtpm_setup --tpmstate "{tpm_dir}" --tpm2 --create-ek-cert --create-platform-cert
    fi
}}

start_tpm() {{
    init_tpm
    echo "Starting TPM emulator..."
    swtpm socket \
        --tpmstate dir="{tpm_dir}" \
        --ctrl type=unixio,path="{tpm_dir}/swtpm-sock" \
        --tpm2 \
        --daemon
    sleep 1
}}
"#, tpm_dir = tpm_dir)
}

/// Extract PCI addresses from passthrough args (e.g., "-device vfio-pci,host=0000:47:00.0")
fn extract_pci_addresses(pci_args: &[String]) -> Vec<String> {
    pci_args
        .iter()
        .filter_map(|arg| {
            // Format: "-device vfio-pci,host=0000:XX:XX.X"
            arg.split("host=").nth(1).map(|s| {
                s.split(|c| c == ',' || c == ' ')
                    .next()
                    .unwrap_or(s)
                    .to_string()
            })
        })
        .collect()
}

/// Generate USB passthrough arguments
fn generate_usb_passthrough_args(devices: &[crate::vm::UsbPassthrough]) -> String {
    if devices.is_empty() {
        return String::new();
    }

    let mut args = vec!["-usb".to_string()];

    // Check if any USB 3.0 devices are present
    let has_usb3 = devices.iter().any(|d| d.is_usb3());

    // Add xHCI controller if USB 3.0 devices are present
    if has_usb3 {
        args.push("-device qemu-xhci,id=xhci,p2=8,p3=8".to_string());
    }

    // Add each USB device, attaching USB 3.0 devices to xHCI controller
    for dev in devices {
        if dev.is_usb3() {
            args.push(format!(
                "-device usb-host,bus=xhci.0,vendorid=0x{:04x},productid=0x{:04x}",
                dev.vendor_id, dev.product_id
            ));
        } else {
            args.push(format!(
                "-device usb-host,vendorid=0x{:04x},productid=0x{:04x}",
                dev.vendor_id, dev.product_id
            ));
        }
    }
    args.join(" \\\n    ")
}

/// Generate the emergency restore script
fn generate_restore_script(vm: &DiscoveredVm, config: &SingleGpuConfig) -> String {
    let gpu_addr = &config.gpu.address;
    let audio_addr = config
        .audio
        .as_ref()
        .map(|a| a.address.as_str())
        .unwrap_or("");
    let original_driver = config.original_driver.module_name();
    let display_manager = config.display_manager.service_name();
    let vm_name = vm.display_name();
    let vm_dir = vm.path.display();

    // Check if TPM is used
    let launch_script = fs::read_to_string(&vm.launch_script).unwrap_or_default();
    let components = parse_launch_script(&launch_script);

    // Load PCI passthrough for extra devices
    let pci_passthrough_args = load_pci_passthrough(vm);
    let extra_pci_addrs = extract_pci_addresses(&pci_passthrough_args);
    let extra_pci_addrs_str = if extra_pci_addrs.is_empty() {
        "EXTRA_PCI_ADDRS=()".to_string()
    } else {
        format!(
            "EXTRA_PCI_ADDRS=({})",
            extra_pci_addrs
                .iter()
                .map(|a| format!("\"{}\"", a))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    let tpm_cleanup = if components.has_tpm {
        let tpm_dir = components.tpm_dir.as_deref().unwrap_or("$VM_DIR/tpm");
        format!(r#"
# Kill TPM emulator if running
TPM_DIR="{tpm_dir}"
pkill -f "swtpm.*$TPM_DIR" 2>/dev/null || true
"#, tpm_dir = tpm_dir)
    } else {
        String::new()
    };

    let nvidia_module_load = if original_driver == "nvidia" {
        r#"
    # Load NVIDIA modules in dependency order
    echo "Loading NVIDIA modules..."
    modprobe nvidia 2>/dev/null || true
    sleep 1
    modprobe nvidia_modeset 2>/dev/null || true
    sleep 0.5
    modprobe nvidia_drm 2>/dev/null || true
    modprobe nvidia_uvm 2>/dev/null || true
    sleep 1"#.to_string()
    } else {
        format!(r#"
    echo "Loading {} driver..."
    modprobe "{}" 2>/dev/null || true
    sleep 2"#, original_driver, original_driver)
    };

    format!(
        r#"#!/bin/bash
# Single GPU Passthrough Restore Script
# Generated by vm-curator for: {vm_name}
#
# Use this script to restore your display if something goes wrong.
# Can be run from SSH or a recovery boot.

set -e

VM_DIR="{vm_dir}"
VM_NAME="{vm_name}"
GPU_ADDR="{gpu_addr}"
AUDIO_ADDR="{audio_addr}"
ORIGINAL_DRIVER="{original_driver}"
DISPLAY_MANAGER="{display_manager}"
{extra_pci_addrs}

# Must run as root
if [[ $EUID -ne 0 ]]; then
    echo "This script must be run as root (use sudo)"
    exit 1
fi

echo "Restoring display..."

# Kill any lingering QEMU processes
echo "Killing any QEMU processes..."
pkill -f "qemu.*$VM_NAME" 2>/dev/null || true
{tpm_cleanup}
# Remove and rescan PCI devices (more reliable than unbind)
echo "Removing GPU from PCI bus..."
if [[ -e "/sys/bus/pci/devices/$GPU_ADDR" ]]; then
    echo 1 > /sys/bus/pci/devices/$GPU_ADDR/remove 2>/dev/null || true
fi
if [[ -n "$AUDIO_ADDR" ]] && [[ -e "/sys/bus/pci/devices/$AUDIO_ADDR" ]]; then
    echo 1 > /sys/bus/pci/devices/$AUDIO_ADDR/remove 2>/dev/null || true
fi

# Remove extra PCI devices from bus (will be re-bound on rescan)
for addr in "${{EXTRA_PCI_ADDRS[@]}}"; do
    if [[ -e "/sys/bus/pci/devices/$addr" ]]; then
        echo "Removing $addr from PCI bus..."
        echo 1 > /sys/bus/pci/devices/$addr/remove 2>/dev/null || true
    fi
done
sleep 2

# Rescan PCI bus
echo "Rescanning PCI bus..."
echo 1 > /sys/bus/pci/rescan
sleep 3

# Unload VFIO modules
echo "Unloading VFIO modules..."
modprobe -r vfio_pci 2>/dev/null || true
modprobe -r vfio_iommu_type1 2>/dev/null || true
modprobe -r vfio 2>/dev/null || true
sleep 1

# Load original driver
if [[ -n "$ORIGINAL_DRIVER" ]] && [[ "$ORIGINAL_DRIVER" != "vfio-pci" ]]; then
{nvidia_module_load}

    # Manual bind fallback if GPU doesn't auto-bind
    if [[ -e "/sys/bus/pci/devices/$GPU_ADDR" ]] && [[ ! -e "/sys/bus/pci/devices/$GPU_ADDR/driver" ]]; then
        echo "Manual bind to $ORIGINAL_DRIVER..."
        echo "$GPU_ADDR" > /sys/bus/pci/drivers/$ORIGINAL_DRIVER/bind 2>/dev/null || true
    fi
fi

# Restart display manager
echo "Starting display manager..."
systemctl start "$DISPLAY_MANAGER" 2>/dev/null || true

echo ""
echo "Restore complete!"
echo "If display still doesn't work, try rebooting."
"#,
        vm_name = vm_name,
        vm_dir = vm_dir,
        gpu_addr = gpu_addr,
        audio_addr = audio_addr,
        original_driver = original_driver,
        display_manager = display_manager,
        extra_pci_addrs = extra_pci_addrs_str,
        tpm_cleanup = tpm_cleanup,
        nvidia_module_load = nvidia_module_load,
    )
}

/// Result of running system setup
#[derive(Debug)]
pub enum SystemSetupResult {
    /// Setup launched in terminal window
    Launched,
    /// No terminal emulator found
    NoTerminal,
    /// Setup failed with error
    Error(String),
}

/// Run the one-time system setup for single GPU passthrough.
/// This launches an interactive terminal window with the setup script,
/// allowing the user to see output and provide input (e.g., for mkinitcpio).
pub fn run_system_setup(gpu_driver: &str) -> SystemSetupResult {
    use std::process::Command;

    // Generate the setup script
    let script_content = generate_interactive_setup_script(gpu_driver);
    let script_path = "/tmp/vm-curator-vfio-setup.sh";

    // Write the script
    if let Err(e) = std::fs::write(script_path, &script_content) {
        return SystemSetupResult::Error(format!("Failed to write setup script: {}", e));
    }

    // Make it executable
    if let Err(e) = std::fs::set_permissions(
        script_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    ) {
        return SystemSetupResult::Error(format!("Failed to make script executable: {}", e));
    }

    // Find a terminal emulator and launch the script
    // Try each terminal in order of preference
    let terminals: &[(&str, &[&str])] = &[
        ("alacritty", &["-e", "sudo", script_path]),
        ("kitty", &["sudo", script_path]),
        ("ghostty", &["-e", "sudo", script_path]),
        ("gnome-terminal", &["--", "sudo", script_path]),
        ("konsole", &["-e", "sudo", script_path]),
        ("xfce4-terminal", &["-x", "sudo", script_path]),
        ("xterm", &["-e", "sudo", script_path]),
    ];

    for (term, args) in terminals {
        if Command::new("which")
            .arg(term)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            match Command::new(term).args(*args).spawn() {
                Ok(_) => return SystemSetupResult::Launched,
                Err(_) => continue,
            }
        }
    }

    SystemSetupResult::NoTerminal
}

/// Generate an interactive setup script that shows progress and waits for user
fn generate_interactive_setup_script(gpu_driver: &str) -> String {
    let softdep_line = format!("softdep {} pre: vfio-pci", gpu_driver);

    format!(
        r#"#!/bin/bash
# Single GPU Passthrough System Setup
# Generated by vm-curator
#
# This script configures your system for single GPU passthrough.

set -e

echo "========================================"
echo "  Single GPU Passthrough System Setup"
echo "========================================"
echo ""
echo "This will:"
echo "  1. Create /etc/modules-load.d/vfio.conf"
echo "  2. Create /etc/modprobe.d/vfio.conf"
echo "  3. Regenerate initramfs (if applicable)"
echo ""
echo "Press Enter to continue or Ctrl+C to cancel..."
read

echo ""
echo "[1/3] Creating VFIO modules configuration..."

cat > /etc/modules-load.d/vfio.conf << 'EOF'
# VFIO modules for GPU passthrough
vfio
vfio_iommu_type1
vfio_pci
EOF

echo "  Created /etc/modules-load.d/vfio.conf"

echo ""
echo "[2/3] Creating modprobe configuration..."

cat > /etc/modprobe.d/vfio.conf << 'EOF'
# Load VFIO before GPU driver
{softdep_line}
options vfio_pci disable_vga=1
EOF

echo "  Created /etc/modprobe.d/vfio.conf"

echo ""
echo "[3/3] Updating boot configuration..."
echo ""

# Detect bootloader and initramfs tool
INITRAMFS_UPDATED=false

if command -v mkinitcpio &>/dev/null; then
    echo "Detected mkinitcpio - regenerating initramfs..."
    echo ""
    mkinitcpio -P
    INITRAMFS_UPDATED=true
elif command -v booster &>/dev/null; then
    echo "Detected booster - regenerating initramfs..."
    echo ""
    booster build --force
    INITRAMFS_UPDATED=true
elif command -v update-initramfs &>/dev/null; then
    echo "Detected update-initramfs - regenerating initramfs..."
    echo ""
    update-initramfs -u -k all
    INITRAMFS_UPDATED=true
elif command -v dracut &>/dev/null; then
    echo "Detected dracut - regenerating initramfs..."
    echo ""
    dracut -f --regenerate-all
    INITRAMFS_UPDATED=true
fi

# Check for Limine bootloader
if [[ -f /boot/limine.cfg ]] || [[ -f /boot/limine/limine.cfg ]] || [[ -f /boot/EFI/BOOT/limine.cfg ]]; then
    echo ""
    echo "Detected Limine bootloader."
    if [[ "$INITRAMFS_UPDATED" == "true" ]]; then
        echo "Initramfs has been updated. Limine should pick up changes on reboot."
    else
        echo ""
        echo "If you use an initramfs with Limine, please regenerate it manually."
        echo "If you boot without an initramfs, the module configs will be loaded"
        echo "by systemd-modules-load.service after boot."
    fi
fi

# Check for systemd-boot
if [[ -d /boot/loader ]] || bootctl is-installed &>/dev/null 2>&1; then
    if [[ "$INITRAMFS_UPDATED" == "false" ]]; then
        echo ""
        echo "Detected systemd-boot. Please ensure your initramfs is regenerated"
        echo "if you use one, or the modules will load via systemd-modules-load."
    fi
fi

if [[ "$INITRAMFS_UPDATED" == "false" ]]; then
    echo ""
    echo "No standard initramfs tool detected."
    echo "The VFIO modules will be loaded by systemd-modules-load.service on boot."
    echo "This should work for most setups, but if you use a custom initramfs,"
    echo "please regenerate it manually to include the VFIO modules."
fi

echo ""
echo "========================================"
echo "  Setup Complete!"
echo "========================================"
echo ""
echo "You must REBOOT your system for changes to take effect."
echo ""
echo "After reboot, to use single GPU passthrough:"
echo "  1. Press Ctrl+Alt+F3 to switch to TTY3"
echo "  2. Log in with your username"
echo "  3. Run: sudo ./single-gpu-start.sh (in your VM directory)"
echo ""
echo "Press Enter to close this window..."
read
"#,
        softdep_line = softdep_line,
    )
}

/// Extract and modify QEMU command from the VM's launch script for GPU passthrough
fn extract_qemu_command_for_passthrough(
    vm: &DiscoveredVm,
    config: &SingleGpuConfig,
    components: &LaunchScriptComponents,
    usb_passthrough_args: &str,
    pci_passthrough_args: &[String],
) -> Result<String> {
    let launch_script = fs::read_to_string(&vm.launch_script)
        .with_context(|| format!("Failed to read launch script: {:?}", vm.launch_script))?;

    // Find the QEMU command in the script
    let mut qemu_lines = Vec::new();
    let mut in_qemu_command = false;
    let mut found_qemu = false;

    for line in launch_script.lines() {
        let trimmed = line.trim();

        // Check if this is the start of a QEMU command
        if !in_qemu_command {
            // Skip 'exec' prefix - we don't want exec because it prevents cleanup trap
            if (trimmed.starts_with("qemu-system-")
                || trimmed.starts_with("\"$QEMU\"")
                || trimmed.starts_with("$QEMU "))
                && !trimmed.starts_with('#')
            {
                in_qemu_command = true;
                found_qemu = true;
            } else if trimmed.starts_with("exec qemu-system-") && !trimmed.starts_with('#') {
                // Remove 'exec' prefix
                in_qemu_command = true;
                found_qemu = true;
                let without_exec = trimmed.strip_prefix("exec ").unwrap_or(trimmed);
                qemu_lines.push(without_exec.to_string());
                if !trimmed.ends_with('\\') {
                    break;
                }
                continue;
            }
        }

        if in_qemu_command {
            qemu_lines.push(line.to_string());

            // Check if this is the last line of the command
            if !trimmed.ends_with('\\') {
                break;
            }
        }
    }

    if !found_qemu {
        // Fallback: generate a basic QEMU command
        return Ok(generate_basic_qemu_command(vm, config, components, usb_passthrough_args, pci_passthrough_args));
    }

    // Build the modified QEMU command
    let mut qemu_cmd = qemu_lines.join("\n");

    // Replace any hardcoded -name with $VM_NAME variable (for cleanup to work correctly)
    qemu_cmd = RE_NAME.replace_all(&qemu_cmd, r#"-name "$VM_NAME""#).to_string();

    // Remove existing -display if present (we'll use the GPU's display)
    qemu_cmd = RE_DISPLAY.replace_all(&qemu_cmd, "").to_string();

    // Remove existing -vga if present
    qemu_cmd = RE_VGA.replace_all(&qemu_cmd, "").to_string();

    // Remove existing audio devices (no user session available for audio)
    qemu_cmd = RE_AUDIODEV.replace_all(&qemu_cmd, "").to_string();
    qemu_cmd = RE_SOUNDHW.replace_all(&qemu_cmd, "").to_string();

    // Remove CD-ROM/ISO arguments - single-GPU passthrough is for running installed VMs,
    // not installation. Use standard launch.sh for installation.
    qemu_cmd = RE_CDROM.replace_all(&qemu_cmd, "").to_string();
    qemu_cmd = RE_DRIVE_CDROM.replace_all(&qemu_cmd, "").to_string();
    qemu_cmd = RE_DRIVE_ISO.replace_all(&qemu_cmd, "").to_string();

    // Clean up empty continuation lines
    while RE_EMPTY_CONT.is_match(&qemu_cmd) {
        qemu_cmd = RE_EMPTY_CONT.replace_all(&qemu_cmd, "\\\n").to_string();
    }

    // Build passthrough arguments
    let mut passthrough_args = Vec::new();

    // GPU passthrough (no x-vga=on - incompatible with modern NVIDIA GPUs)
    passthrough_args.push(format!(
        "-device vfio-pci,host={},multifunction=on",
        config.gpu.address
    ));

    // Audio passthrough (if present)
    if let Some(ref audio) = config.audio {
        passthrough_args.push(format!(
            "-device vfio-pci,host={}",
            audio.address
        ));
    }

    // Display settings - none (output goes to physical GPU)
    passthrough_args.push("-display none".to_string());
    passthrough_args.push("-vga none".to_string());

    // Add virtio-rng for entropy
    passthrough_args.push("-device virtio-rng-pci".to_string());

    // USB passthrough (from launch.sh config)
    if !usb_passthrough_args.is_empty() {
        passthrough_args.push(usb_passthrough_args.to_string());
    }

    // PCI passthrough (from launch.sh config - network cards, USB controllers, etc.)
    for pci_arg in pci_passthrough_args {
        passthrough_args.push(pci_arg.clone());
    }

    // Add NVIDIA CPU flags if NVIDIA GPU
    let nvidia_cpu_flags = if config.gpu.is_nvidia() {
        // These flags help with NVIDIA driver compatibility
        Some("-cpu host,kvm=off,hv_vendor_id=AuthenticAMD,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time".to_string())
    } else {
        None
    };

    // Add passthrough args to the QEMU command
    let passthrough_str = passthrough_args.join(" \\\n    ");

    // Find the end of the QEMU command and insert our args
    if let Some(last_backslash) = qemu_cmd.rfind('\\') {
        let (before, _) = qemu_cmd.split_at(last_backslash);
        qemu_cmd = format!(
            "{} \\\n    {}",
            before.trim_end(),
            passthrough_str
        );
    } else {
        // No continuation, just append
        qemu_cmd = format!("{} \\\n    {}", qemu_cmd.trim_end(), passthrough_str);
    }

    // Replace -cpu host with NVIDIA flags if needed
    if let Some(flags) = nvidia_cpu_flags {
        if RE_CPU_HOST.is_match(&qemu_cmd) {
            qemu_cmd = RE_CPU_HOST.replace(&qemu_cmd, flags.as_str()).to_string();
        }
    }

    // Add kernel_irqchip=on to machine options if not present
    if !qemu_cmd.contains("kernel_irqchip") {
        if let Some(caps) = RE_MACHINE.captures(&qemu_cmd) {
            let machine_opts = caps.get(1).unwrap().as_str();
            if !machine_opts.contains("kernel_irqchip") {
                let new_opts = if machine_opts.contains(',') {
                    format!("{},kernel_irqchip=on", machine_opts)
                } else {
                    format!("{},kernel_irqchip=on", machine_opts)
                };
                qemu_cmd = RE_MACHINE.replace(&qemu_cmd, format!("-machine {}", new_opts).as_str()).to_string();
            }
        }
    }

    // Fix boot order if it's set to 'd' (CD-ROM first) - use 'c' (disk first) for normal boot
    qemu_cmd = RE_BOOT_D.replace(&qemu_cmd, "-boot order=c").to_string();

    Ok(qemu_cmd)
}

/// Generate a basic QEMU command when the launch script can't be parsed
fn generate_basic_qemu_command(
    vm: &DiscoveredVm,
    config: &SingleGpuConfig,
    components: &LaunchScriptComponents,
    usb_passthrough_args: &str,
    pci_passthrough_args: &[String],
) -> String {
    let qemu_emulator = vm.config.emulator.command();
    let memory = vm.config.memory_mb;
    let cpu_cores = vm.config.cpu_cores;

    // Use NVIDIA CPU flags if NVIDIA GPU
    let cpu_flags = if config.gpu.is_nvidia() {
        "host,kvm=off,hv_vendor_id=AuthenticAMD,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time"
    } else {
        "host"
    };

    let mut cmd = format!(
        r#"{} \
    -name "$VM_NAME" \
    -machine q35,accel=kvm,kernel_irqchip=on \
    -cpu {} \
    -m {} \
    -smp {} \
    -enable-kvm"#,
        qemu_emulator,
        cpu_flags,
        memory,
        cpu_cores
    );

    // Add SMBIOS options if present (for Windows to avoid corporate machine detection)
    if components.smbios_opts.is_some() {
        cmd.push_str(r#" \
    "${SMBIOS_OPTS[@]}""#);
    }

    // Add UEFI if present
    if components.has_uefi {
        cmd.push_str(r#" \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$OVMF_VARS""#);
    }

    // Add TPM if present
    if components.has_tpm {
        cmd.push_str(r#" \
    -chardev socket,id=chrtpm,path="$TPM_DIR/swtpm-sock" \
    -tpmdev emulator,id=tpm0,chardev=chrtpm \
    -device tpm-tis,tpmdev=tpm0"#);
    }

    // Add disk
    if let Some(disk) = vm.config.disks.first() {
        let format_str = match &disk.format {
            crate::vm::qemu_config::DiskFormat::Qcow2 => "qcow2",
            crate::vm::qemu_config::DiskFormat::Raw => "raw",
            crate::vm::qemu_config::DiskFormat::Vmdk => "vmdk",
            crate::vm::qemu_config::DiskFormat::Vdi => "vdi",
            crate::vm::qemu_config::DiskFormat::Other(s) => s.as_str(),
        };
        cmd.push_str(&format!(
            r#" \
    -drive file="$DISK",format={},if=virtio"#,
            format_str
        ));
    }

    // Add GPU passthrough (no x-vga=on - incompatible with modern NVIDIA GPUs)
    cmd.push_str(&format!(
        r#" \
    -device vfio-pci,host={},multifunction=on"#,
        config.gpu.address
    ));

    // Add audio (if present)
    if let Some(ref audio) = config.audio {
        cmd.push_str(&format!(
            r#" \
    -device vfio-pci,host={}"#,
            audio.address
        ));
    }

    // Display settings - none (output goes to physical GPU)
    cmd.push_str(
        r#" \
    -display none \
    -vga none"#,
    );

    // Add virtio-rng for entropy
    cmd.push_str(
        r#" \
    -device virtio-rng-pci"#,
    );

    // Add USB passthrough
    if !usb_passthrough_args.is_empty() {
        cmd.push_str(&format!(
            r#" \
    {}"#,
            usb_passthrough_args
        ));
    } else {
        // Placeholder for USB - user needs to configure
        cmd.push_str(
            r#" \
    # USB passthrough - configure via USB Passthrough in VM Management
    -usb"#,
        );
    }

    // Add PCI passthrough (network cards, USB controllers, etc.)
    for pci_arg in pci_passthrough_args {
        cmd.push_str(&format!(
            r#" \
    {}"#,
            pci_arg
        ));
    }

    // Boot from disk
    cmd.push_str(
        r#" \
    -boot order=c"#,
    );

    cmd
}

/// Delete single GPU scripts for a VM
pub fn delete_scripts(vm_path: &Path) -> Result<()> {
    let scripts = [
        "single-gpu-start.sh",
        "single-gpu-restore.sh",
    ];

    for script in scripts {
        let path = vm_path.join(script);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete script: {:?}", path))?;
        }
    }

    Ok(())
}

/// Regenerate single-GPU scripts if they exist
/// Returns Ok(true) if scripts were regenerated, Ok(false) if no scripts exist
pub fn regenerate_if_exists(vm: &DiscoveredVm, config: &SingleGpuConfig) -> Result<bool> {
    use crate::hardware::scripts_exist;

    if !scripts_exist(&vm.path) {
        return Ok(false);
    }

    // Scripts exist, regenerate them
    generate_single_gpu_scripts(vm, config)?;
    Ok(true)
}

/// Regenerate single-GPU scripts using saved config from file
/// This is used when the app doesn't have single_gpu_config in memory (e.g., after restart)
/// Returns Ok(true) if scripts were regenerated, Ok(false) if no scripts or config exist
pub fn regenerate_from_saved_config(vm: &DiscoveredVm) -> Result<bool> {
    use crate::hardware::{load_config, scripts_exist};

    if !scripts_exist(&vm.path) {
        return Ok(false);
    }

    // Try to load saved config
    let config = load_config(&vm.path)
        .ok_or_else(|| anyhow::anyhow!("No saved single-GPU config found"))?;

    // Regenerate scripts
    generate_single_gpu_scripts(vm, &config)?;
    Ok(true)
}
