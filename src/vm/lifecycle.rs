use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::discovery::DiscoveredVm;
use super::qemu_config::BootMode;
use crate::hardware::UsbVersion;

/// Result of a VM launch attempt
#[derive(Debug)]
pub struct LaunchResult {
    /// Whether the launch appeared successful (no immediate errors)
    pub success: bool,
    /// Error message if the launch failed
    pub error: Option<String>,
    /// VM display name for status messages
    pub vm_name: String,
}

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
    pub usb_version: UsbVersion,
}

impl UsbPassthrough {
    /// Generate QEMU device arguments for this USB device
    /// If `bus` is provided, attach to that specific bus (e.g., "xhci.0" for USB 3.0)
    pub fn to_qemu_args(&self, bus: Option<&str>) -> Vec<String> {
        let device_spec = if let Some(bus_name) = bus {
            format!(
                "usb-host,bus={},vendorid=0x{:04x},productid=0x{:04x}",
                bus_name, self.vendor_id, self.product_id
            )
        } else {
            format!(
                "usb-host,vendorid=0x{:04x},productid=0x{:04x}",
                self.vendor_id, self.product_id
            )
        };
        vec!["-device".to_string(), device_spec]
    }

    /// Check if this device is USB 3.0 or higher
    pub fn is_usb3(&self) -> bool {
        self.usb_version.is_usb3()
    }
}

/// Launch a VM and monitor for immediate errors
///
/// This function spawns the VM process and monitors stderr for a brief period
/// to catch any immediate startup errors (like missing files, invalid arguments, etc.)
/// If the process exits with an error within the monitoring window, we capture it.
pub fn launch_vm_with_error_check(vm: &DiscoveredVm, options: &LaunchOptions) -> LaunchResult {
    let vm_name = vm.display_name();

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
                return LaunchResult {
                    success: false,
                    error: Some(format!("ISO file not found: {}", iso_path.display())),
                    vm_name,
                };
            }
            if !iso_path.is_file() {
                return LaunchResult {
                    success: false,
                    error: Some(format!("ISO path is not a file: {}", iso_path.display())),
                    vm_name,
                };
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
    if !options.usb_devices.is_empty() {
        args.push("-usb".to_string());

        // Check if any USB 3.0 devices are present
        let has_usb3 = options.usb_devices.iter().any(|d| d.is_usb3());

        // Add xHCI controller if USB 3.0 devices are present
        if has_usb3 {
            args.push("-device".to_string());
            args.push("qemu-xhci,id=xhci".to_string());
        }

        // Add each USB device, attaching USB 3.0 devices to xHCI controller
        for usb in &options.usb_devices {
            let bus = if usb.is_usb3() { Some("xhci.0") } else { None };
            args.extend(usb.to_qemu_args(bus));
        }
    }

    cmd.args(&args);

    // Capture stderr to detect errors, but let stdout go to null
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return LaunchResult {
                success: false,
                error: Some(format!("Failed to start VM process: {}", e)),
                vm_name,
            };
        }
    };

    // Take stderr handle for monitoring
    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            return LaunchResult {
                success: true,
                error: None,
                vm_name,
            };
        }
    };

    // Create a channel to receive error output
    let (tx, rx) = mpsc::channel();

    // Spawn a thread to read ALL stderr output
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut all_lines = Vec::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                // Capture all stderr output - we'll filter later if needed
                if !line.trim().is_empty() {
                    all_lines.push(line);
                }
            }
        }

        let _ = tx.send(all_lines);
    });

    // Wait for QEMU to either start successfully or fail
    // QEMU typically fails fast if there's a configuration error
    thread::sleep(Duration::from_millis(800));

    // Check if the process has already exited (indicating an error)
    match child.try_wait() {
        Ok(Some(status)) => {
            // Process exited - this usually means an error for QEMU
            // (successful QEMU keeps running until the VM shuts down)

            // Wait a bit more for stderr to be fully captured
            thread::sleep(Duration::from_millis(300));

            // Try to get error output
            let stderr_lines = rx.recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();

            // Filter for error-related lines for display
            let error_lines: Vec<&String> = stderr_lines.iter()
                .filter(|line| {
                    let lower = line.to_lowercase();
                    lower.contains("error")
                        || lower.contains("failed")
                        || lower.contains("cannot")
                        || lower.contains("unable")
                        || lower.contains("not found")
                        || lower.contains("no such")
                        || lower.contains("invalid")
                        || lower.contains("is not a valid")
                        || lower.contains("could not")
                        || lower.contains("qemu-system")
                        || lower.contains("permission denied")
                })
                .collect();

            let error_msg = if !error_lines.is_empty() {
                error_lines.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
            } else if !stderr_lines.is_empty() {
                // Show all stderr if no specific errors found
                stderr_lines.join("\n")
            } else {
                format!("VM process exited with code: {}", status)
            };

            return LaunchResult {
                success: false,
                error: Some(error_msg),
                vm_name,
            };
        }
        Ok(None) => {
            // Process still running - this is the expected success case
        }
        Err(e) => {
            return LaunchResult {
                success: false,
                error: Some(format!("Failed to check VM status: {}", e)),
                vm_name,
            };
        }
    }

    LaunchResult {
        success: true,
        error: None,
        vm_name,
    }
}

/// Launch a VM synchronously (legacy function for compatibility)
pub fn launch_vm_sync(vm: &DiscoveredVm, options: &LaunchOptions) -> Result<()> {
    let result = launch_vm_with_error_check(vm, options);

    if result.success {
        Ok(())
    } else {
        bail!("{}", result.error.unwrap_or_else(|| "Unknown error".to_string()))
    }
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

                // Find a unique name in trash (append timestamp if needed)
                let mut trash_path = trash_dir.join(&vm.id);
                if trash_path.exists() {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    trash_path = trash_dir.join(format!("{}-{}", vm.id, timestamp));
                }

                std::fs::rename(&vm.path, &trash_path)
                    .context("Failed to move VM to trash")?;
            }
        }
    }

    Ok(())
}

/// Rename a VM by updating its display name in vm-curator.toml
pub fn rename_vm(vm: &DiscoveredVm, new_name: &str) -> Result<()> {
    let metadata_path = vm.path.join("vm-curator.toml");

    // Read existing metadata or create new
    let os_profile = if metadata_path.exists() {
        // Parse existing file to preserve os_profile
        let content = std::fs::read_to_string(&metadata_path)
            .context("Failed to read VM metadata")?;

        // Simple extraction of os_profile
        content.lines()
            .find(|line| line.trim().starts_with("os_profile"))
            .and_then(|line| {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let value = parts[1].trim();
                    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                        Some(value[1..value.len()-1].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
    } else {
        // No existing file, use VM's id as fallback profile
        Some(vm.id.clone())
    };

    // Write updated metadata
    let mut content = String::new();
    content.push_str("# VM Curator metadata\n\n");
    content.push_str(&format!("display_name = \"{}\"\n", new_name.replace('"', "\\\"")));

    if let Some(profile) = os_profile {
        content.push_str(&format!("os_profile = \"{}\"\n", profile));
    }

    std::fs::write(&metadata_path, content)
        .context("Failed to write VM metadata")?;

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

    // Check if any USB 3.0 devices are present
    let has_usb3 = devices.iter().any(|d| d.is_usb3());

    // Add xHCI controller if USB 3.0 devices are present
    if has_usb3 {
        section.push_str(" -device qemu-xhci,id=xhci");
    }

    // Add each USB device, attaching USB 3.0 devices to xHCI controller
    for device in devices {
        if device.is_usb3() {
            section.push_str(&format!(
                " -device usb-host,bus=xhci.0,vendorid=0x{:04x},productid=0x{:04x}",
                device.vendor_id, device.product_id
            ));
        } else {
            section.push_str(&format!(
                " -device usb-host,vendorid=0x{:04x},productid=0x{:04x}",
                device.vendor_id, device.product_id
            ));
        }
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

    // Find ALL QEMU commands in the script (there may be multiple in a case statement)
    // Each entry is (start_idx, end_idx) for a QEMU command
    let mut qemu_commands: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_qemu_line = (trimmed.starts_with("qemu-system-")
            || trimmed.starts_with("exec qemu-system-")
            || trimmed.starts_with("\"$QEMU\"")
            || trimmed.starts_with("$QEMU "))
            && !trimmed.starts_with('#');

        if is_qemu_line {
            let start_idx = i;
            // Find the end of this QEMU command (the line without trailing \)
            while i < lines.len() {
                let line_trimmed = lines[i].trim();
                if !line_trimmed.ends_with('\\') {
                    break;
                }
                i += 1;
            }
            let end_idx = i;
            qemu_commands.push((start_idx, end_idx));
        }
        i += 1;
    }

    // Track which end lines we need to modify
    let qemu_end_indices: std::collections::HashSet<usize> = qemu_commands.iter().map(|(_, end)| *end).collect();

    // Get the first QEMU command start for inserting the section
    let first_qemu_start = qemu_commands.first().map(|(start, _)| *start);

    for (i, line) in lines.iter().enumerate() {
        // Insert USB section before the FIRST qemu command
        if Some(i) == first_qemu_start && !inserted_section {
            result.push_str(usb_section);
            result.push('\n');
            inserted_section = true;
        }

        // Modify ALL QEMU command endings to include $USB_PASSTHROUGH_ARGS
        if qemu_end_indices.contains(&i) {
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
            // Or with xHCI: USB_PASSTHROUGH_ARGS="-usb -device qemu-xhci,id=xhci -device usb-host,bus=xhci.0,vendorid=0x1234,productid=0x5678 ..."
            for part in line.split("-device usb-host,") {
                if part.contains("vendorid=") && part.contains("productid=") {
                    if let (Some(vid), Some(pid)) = (
                        extract_hex_value(part, "vendorid="),
                        extract_hex_value(part, "productid="),
                    ) {
                        // Detect USB version from bus assignment
                        // If attached to xhci.0, it's USB 3.0; otherwise default to USB 2.0
                        let usb_version = if part.contains("bus=xhci") {
                            UsbVersion::Usb3
                        } else {
                            UsbVersion::Usb2
                        };
                        devices.push(UsbPassthrough {
                            vendor_id: vid,
                            product_id: pid,
                            usb_version,
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

// PCI Passthrough section markers
const PCI_MARKER_START: &str = "# >>> PCI Passthrough (managed by vm-curator) >>>";
const PCI_MARKER_END: &str = "# <<< PCI Passthrough <<<";

/// Load PCI passthrough configuration from the VM's launch.sh
/// Returns a vector of individual QEMU args (e.g., ["-device", "vfio-pci,host=..."])
pub fn load_pci_passthrough(vm: &DiscoveredVm) -> Vec<String> {
    let content = match std::fs::read_to_string(&vm.launch_script) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    parse_pci_section(&content)
}

fn parse_pci_section(content: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut in_pci_section = false;

    for line in content.lines() {
        if line.trim() == PCI_MARKER_START {
            in_pci_section = true;
            continue;
        }
        if line.trim() == PCI_MARKER_END {
            in_pci_section = false;
            continue;
        }
        if in_pci_section && line.contains("PCI_PASSTHROUGH_ARGS=") {
            // Parse the PCI args line
            // Format: PCI_PASSTHROUGH_ARGS="-device vfio-pci,host=0000:01:00.0 -device vfio-pci,host=0000:01:00.1"
            // Extract each -device vfio-pci,host=... segment

            // Find the value inside quotes
            if let Some(start) = line.find('"') {
                if let Some(end) = line.rfind('"') {
                    if end > start {
                        let value = &line[start + 1..end];
                        // Split by -device and reconstruct
                        for part in value.split("-device ") {
                            let part = part.trim();
                            if part.starts_with("vfio-pci,host=") {
                                // Extract the host address (ends at space or end of string)
                                let arg = format!("-device {}", part.split_whitespace().next().unwrap_or(part));
                                args.push(arg);
                            }
                        }
                    }
                }
            }
        }
    }

    args
}
