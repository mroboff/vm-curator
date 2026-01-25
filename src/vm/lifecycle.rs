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

    // Add USB passthrough arguments
    // These are passed as extra args that the launch.sh script forwards to QEMU
    if !options.usb_devices.is_empty() {
        // Add USB controller if passing through devices
        args.push("-usb".to_string());
        for usb in &options.usb_devices {
            args.extend(usb.to_qemu_args());
        }
    }

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

// USB Passthrough configuration markers
const USB_MARKER_START: &str = "# >>> USB Passthrough (managed by vm-curator) >>>";
const USB_MARKER_END: &str = "# <<< USB Passthrough <<<";

/// Save USB passthrough configuration to the VM's launch.sh
pub fn save_usb_passthrough(vm: &DiscoveredVm, devices: &[UsbPassthrough]) -> Result<()> {
    let script_path = &vm.launch_script;
    let content = std::fs::read_to_string(script_path)
        .context("Failed to read launch.sh")?;

    // Remove existing USB passthrough section if present
    let content = remove_usb_section(&content);

    // Generate new USB passthrough section
    let usb_section = generate_usb_section(devices);

    // Find where to insert the USB section
    // We want to add it before the final qemu-system command or at the end
    let new_content = insert_usb_section(&content, &usb_section);

    // Write back
    std::fs::write(script_path, new_content)
        .context("Failed to write launch.sh")?;

    Ok(())
}

/// Load USB passthrough configuration from the VM's launch.sh
pub fn load_usb_passthrough(vm: &DiscoveredVm) -> Vec<UsbPassthrough> {
    let script_path = &vm.launch_script;
    let content = match std::fs::read_to_string(script_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    parse_usb_section(&content)
}

fn remove_usb_section(content: &str) -> String {
    let mut result = String::new();
    let mut in_usb_section = false;

    for line in content.lines() {
        if line.trim() == USB_MARKER_START {
            in_usb_section = true;
            continue;
        }
        if line.trim() == USB_MARKER_END {
            in_usb_section = false;
            continue;
        }
        if !in_usb_section {
            // Also remove any $USB_PASSTHROUGH_ARGS references from qemu command lines
            let cleaned_line = remove_usb_args_from_line(line);
            result.push_str(&cleaned_line);
            result.push('\n');
        }
    }

    // Remove trailing empty lines that may have accumulated
    while result.ends_with("\n\n") {
        result.pop();
    }

    result
}

fn remove_usb_args_from_line(line: &str) -> String {
    // Remove $USB_PASSTHROUGH_ARGS from a line (with various formats)
    line.replace(" $USB_PASSTHROUGH_ARGS", "")
        .replace("$USB_PASSTHROUGH_ARGS ", "")
        .replace("$USB_PASSTHROUGH_ARGS", "")
}

fn generate_usb_section(devices: &[UsbPassthrough]) -> String {
    if devices.is_empty() {
        return String::new();
    }

    let mut section = String::new();
    section.push_str(USB_MARKER_START);
    section.push('\n');
    section.push_str("USB_PASSTHROUGH_ARGS=\"-usb");

    for device in devices {
        section.push_str(&format!(
            " -device usb-host,vendorid=0x{:04x},productid=0x{:04x}",
            device.vendor_id, device.product_id
        ));
    }

    section.push_str("\"\n");
    section.push_str(USB_MARKER_END);
    section.push('\n');

    section
}

fn insert_usb_section(content: &str, usb_section: &str) -> String {
    if usb_section.is_empty() {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();
    let mut inserted_section = false;
    let mut modified_qemu_cmd = false;

    // First pass: find the qemu command start line index
    let mut qemu_start_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_qemu_line = (trimmed.starts_with("qemu-system-")
            || trimmed.starts_with("exec qemu-system-")
            || trimmed.starts_with("\"$QEMU\"")
            || trimmed.starts_with("$QEMU "))
            && !trimmed.starts_with('#');

        if is_qemu_line {
            qemu_start_idx = Some(i);
            break;
        }
    }

    // Find the last line of the qemu command (the one without trailing \)
    let mut qemu_end_idx: Option<usize> = None;
    if let Some(start) = qemu_start_idx {
        for i in start..lines.len() {
            let trimmed = lines[i].trim();
            if !trimmed.ends_with('\\') {
                qemu_end_idx = Some(i);
                break;
            }
        }
        // If all lines end with \, use the last line
        if qemu_end_idx.is_none() {
            qemu_end_idx = Some(lines.len() - 1);
        }
    }

    for (i, line) in lines.iter().enumerate() {
        // Insert USB section before the qemu command
        if Some(i) == qemu_start_idx && !inserted_section {
            result.push_str(usb_section);
            result.push('\n');
            inserted_section = true;
        }

        // Modify the last line of the qemu command to include $USB_PASSTHROUGH_ARGS
        if Some(i) == qemu_end_idx && !modified_qemu_cmd {
            let trimmed = line.trim_end();
            // Add $USB_PASSTHROUGH_ARGS before any trailing comment
            if let Some(comment_pos) = trimmed.find(" #") {
                let (cmd, comment) = trimmed.split_at(comment_pos);
                result.push_str(cmd);
                result.push_str(" $USB_PASSTHROUGH_ARGS");
                result.push_str(comment);
            } else {
                result.push_str(trimmed);
                result.push_str(" $USB_PASSTHROUGH_ARGS");
            }
            result.push('\n');
            modified_qemu_cmd = true;
            continue;
        }

        result.push_str(line);
        if i < lines.len() - 1 {
            result.push('\n');
        }
    }

    // If we didn't find a qemu line, append at the end
    if !inserted_section {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(usb_section);
    }

    // Ensure file ends with newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    result
}

fn parse_usb_section(content: &str) -> Vec<UsbPassthrough> {
    let mut devices = Vec::new();
    let mut in_usb_section = false;

    for line in content.lines() {
        if line.trim() == USB_MARKER_START {
            in_usb_section = true;
            continue;
        }
        if line.trim() == USB_MARKER_END {
            in_usb_section = false;
            continue;
        }
        if in_usb_section && line.contains("USB_PASSTHROUGH_ARGS=") {
            // Parse the USB args line
            // Format: USB_PASSTHROUGH_ARGS="-usb -device usb-host,vendorid=0x1234,productid=0x5678 ..."
            for part in line.split("-device usb-host,") {
                if part.contains("vendorid=") && part.contains("productid=") {
                    if let (Some(vid), Some(pid)) = (
                        extract_hex_value(part, "vendorid="),
                        extract_hex_value(part, "productid="),
                    ) {
                        devices.push(UsbPassthrough {
                            vendor_id: vid,
                            product_id: pid,
                        });
                    }
                }
            }
        }
    }

    devices
}

fn extract_hex_value(s: &str, prefix: &str) -> Option<u16> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];

    // Find end of hex value (comma, space, quote, or end of string)
    let end = rest.find(|c: char| c == ',' || c == ' ' || c == '"' || c == '\'')
        .unwrap_or(rest.len());

    let hex_str = &rest[..end];

    // Handle 0x prefix
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    u16::from_str_radix(hex_str, 16).ok()
}
