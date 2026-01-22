use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use super::discovery::DiscoveredVm;
use super::qemu_config::BootMode;

/// Convert a path to a string, returning an error if the path contains invalid UTF-8
fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Path contains invalid UTF-8: {:?}", path))
}

/// Launch options for starting a VM
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub boot_mode: BootMode,
    pub extra_args: Vec<String>,
    pub usb_devices: Vec<UsbPassthrough>,
}

/// USB device for passthrough
#[derive(Debug, Clone)]
pub struct UsbPassthrough {
    pub vendor_id: u16,
    pub product_id: u16,
}

impl UsbPassthrough {
    pub fn to_qemu_args(&self) -> Vec<String> {
        vec![
            "-device".to_string(),
            format!(
                "usb-host,vendorid=0x{:04x},productid=0x{:04x}",
                self.vendor_id, self.product_id
            ),
        ]
    }
}

/// Launch a VM synchronously
pub fn launch_vm_sync(vm: &DiscoveredVm, options: &LaunchOptions) -> Result<()> {
    let mut cmd = Command::new("bash");
    cmd.current_dir(&vm.path);

    let mut args = vec![vm.launch_script.to_string_lossy().to_string()];

    match &options.boot_mode {
        BootMode::Normal => {}
        BootMode::Install => {
            args.push("--install".to_string());
        }
        BootMode::Cdrom(iso_path) => {
            // Validate ISO path exists before attempting to launch
            if !iso_path.exists() {
                bail!("ISO file not found: {:?}", iso_path);
            }
            if !iso_path.is_file() {
                bail!("ISO path is not a file: {:?}", iso_path);
            }
            args.push("--cdrom".to_string());
            args.push(iso_path.to_string_lossy().to_string());
        }
        BootMode::Network => {
            args.push("--netboot".to_string());
        }
    }

    args.extend(options.extra_args.clone());
    cmd.args(&args);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let _child = cmd.spawn().context("Failed to launch VM")?;

    Ok(())
}

/// Reset a VM by recreating its disk from a backing file or template
pub fn reset_vm(vm: &DiscoveredVm) -> Result<()> {
    // Find the primary disk
    let disk = vm.config.primary_disk()
        .context("VM has no disk configured")?;

    let disk_path = &disk.path;

    // Check for a backing file or template
    let info = super::snapshot::get_disk_info(disk_path)
        .context("Failed to get disk info")?;

    if let Some(backing) = &info.backing_file {
        // Recreate from backing file
        let backing_path = Path::new(backing);
        if !backing_path.exists() {
            bail!("Backing file not found: {}", backing);
        }

        // Remove old disk
        std::fs::remove_file(disk_path)
            .context("Failed to remove old disk")?;

        // Create new disk with backing file
        let disk_str = path_to_str(disk_path)?;
        let output = Command::new("qemu-img")
            .args([
                "create",
                "-f", "qcow2",
                "-F", "qcow2",
                "-b", backing,
                disk_str,
            ])
            .output()
            .context("Failed to create disk from backing file")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to recreate disk: {}", stderr);
        }
    } else {
        // Look for a template or fresh install snapshot
        let snapshots = super::snapshot::list_snapshots(disk_path)?;

        // Try to find a "fresh" or "clean" snapshot
        let fresh_snapshot = snapshots.iter().find(|s| {
            let name = s.name.to_lowercase();
            name.contains("fresh") || name.contains("clean") || name.contains("initial")
        });

        if let Some(snapshot) = fresh_snapshot {
            super::snapshot::restore_snapshot(disk_path, &snapshot.name)?;
        } else {
            bail!("No backing file or fresh snapshot found. Cannot reset VM.");
        }
    }

    Ok(())
}

/// Delete a VM (move to trash or permanently delete)
pub fn delete_vm(vm: &DiscoveredVm, permanent: bool) -> Result<()> {
    if permanent {
        std::fs::remove_dir_all(&vm.path)
            .context("Failed to delete VM directory")?;
    } else {
        // Move to trash using trash-cli if available
        let result = Command::new("trash-put")
            .arg(&vm.path)
            .output();

        match result {
            Ok(output) if output.status.success() => {}
            _ => {
                // Fall back to moving to a .trash directory
                let trash_dir = vm.path.parent()
                    .unwrap_or(Path::new("."))
                    .join(".trash");
                std::fs::create_dir_all(&trash_dir)
                    .context("Failed to create trash directory")?;

                let trash_path = trash_dir.join(&vm.id);
                std::fs::rename(&vm.path, &trash_path)
                    .context("Failed to move VM to trash")?;
            }
        }
    }

    Ok(())
}

/// Check if a VM is currently running (basic check via process list)
pub fn is_vm_running(vm: &DiscoveredVm) -> bool {
    // Try to find a QEMU process with this VM's disk
    if let Some(disk) = vm.config.primary_disk() {
        let disk_name = disk.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if let Ok(output) = Command::new("pgrep")
            .args(["-f", &format!("qemu.*{}", disk_name)])
            .output()
        {
            return output.status.success();
        }
    }
    false
}
