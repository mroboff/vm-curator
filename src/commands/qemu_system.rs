use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::vm::{DiscoveredVm, LaunchOptions, QemuConfig, BootMode};

/// Build QEMU command line arguments from config
pub fn build_qemu_args(config: &QemuConfig, options: &LaunchOptions) -> Vec<String> {
    let mut args = Vec::new();

    // Memory
    args.push("-m".to_string());
    args.push(format!("{}M", config.memory_mb));

    // CPU
    args.push("-smp".to_string());
    args.push(format!("{}", config.cpu_cores));

    if let Some(ref model) = config.cpu_model {
        args.push("-cpu".to_string());
        args.push(model.clone());
    }

    // Machine type
    if let Some(ref machine) = config.machine {
        args.push("-M".to_string());
        args.push(machine.clone());
    }

    // KVM acceleration
    if config.enable_kvm {
        args.push("-enable-kvm".to_string());
    }

    // VGA
    args.push("-vga".to_string());
    args.push(match &config.vga {
        crate::vm::VgaType::Std => "std".to_string(),
        crate::vm::VgaType::Cirrus => "cirrus".to_string(),
        crate::vm::VgaType::Vmware => "vmware".to_string(),
        crate::vm::VgaType::Qxl => "qxl".to_string(),
        crate::vm::VgaType::Virtio => "virtio".to_string(),
        crate::vm::VgaType::None => "none".to_string(),
        crate::vm::VgaType::Other(s) => s.clone(),
    });

    // Disks
    for (i, disk) in config.disks.iter().enumerate() {
        let hd = match i {
            0 => "-hda",
            1 => "-hdb",
            2 => "-hdc",
            3 => "-hdd",
            _ => continue,
        };
        args.push(hd.to_string());
        args.push(disk.path.to_string_lossy().to_string());
    }

    // Network
    if let Some(ref net) = config.network {
        if net.user_net {
            args.push("-net".to_string());
            args.push(format!("nic,model={}", net.model));
            args.push("-net".to_string());
            args.push("user".to_string());
        }
    }

    // Boot mode adjustments
    match &options.boot_mode {
        BootMode::Normal => {}
        BootMode::Install => {
            // Would typically add boot order adjustment
            args.push("-boot".to_string());
            args.push("d".to_string());
        }
        BootMode::Cdrom(iso) => {
            args.push("-cdrom".to_string());
            args.push(iso.to_string_lossy().to_string());
            args.push("-boot".to_string());
            args.push("d".to_string());
        }
        BootMode::Network => {
            args.push("-boot".to_string());
            args.push("n".to_string());
        }
    }

    // USB passthrough devices
    if !options.usb_devices.is_empty() {
        args.push("-usb".to_string());
        for usb in &options.usb_devices {
            args.extend(usb.to_qemu_args());
        }
    }

    // Extra args from config
    args.extend(config.extra_args.clone());

    // Extra args from options
    args.extend(options.extra_args.clone());

    args
}

/// Launch QEMU directly (not using launch.sh)
pub fn launch_qemu_direct(config: &QemuConfig, options: &LaunchOptions) -> Result<()> {
    let args = build_qemu_args(config, options);

    let mut cmd = Command::new(config.emulator.command());
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    cmd.spawn().context("Failed to launch QEMU")?;

    Ok(())
}

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

/// Get KVM module info
pub fn get_kvm_info() -> Option<String> {
    if !is_kvm_available() {
        return None;
    }

    let output = Command::new("lsmod")
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("kvm_intel") || line.starts_with("kvm_amd") {
            return Some(line.split_whitespace().next()?.to_string());
        }
    }

    Some("kvm".to_string())
}
