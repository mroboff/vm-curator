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
            args.push("qemu-xhci,id=xhci,p2=8,p3=8".to_string());
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

        for line in reader.lines().map_while(Result::ok) {
            // Capture all stderr output - we'll filter later if needed
            if !line.trim().is_empty() {
                all_lines.push(line);
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

/// A running QEMU process with its PID, command line, and working directory.
pub struct QemuProcess {
    pub pid: u32,
    pub cmdline: String,
    /// The working directory of the process (from /proc/<pid>/cwd)
    pub cwd: Option<std::path::PathBuf>,
}

/// Detect all running QEMU processes.
/// Returns process info including the working directory read from /proc.
pub fn detect_qemu_processes() -> Vec<QemuProcess> {
    let output = match Command::new("pgrep")
        .args(["-a", "qemu-system"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Each line: "12345 qemu-system-x86_64 -m 4096 ..."
        if let Some(space_pos) = line.find(' ') {
            if let Ok(pid) = line[..space_pos].parse::<u32>() {
                let cmdline = line[space_pos + 1..].to_string();
                // Read the process working directory from /proc
                let cwd = std::fs::read_link(format!("/proc/{}/cwd", pid)).ok();
                processes.push(QemuProcess { pid, cmdline, cwd });
            }
        }
    }
    processes
}

/// Send SIGTERM to a QEMU process (triggers ACPI shutdown in modern QEMU).
pub fn stop_vm_by_pid(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .context("Failed to send SIGTERM")?;
    if !status.success() {
        bail!("kill returned non-zero exit code");
    }
    Ok(())
}

/// Force-kill a QEMU process with SIGKILL.
pub fn force_stop_vm(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .context("Failed to send SIGKILL")?;
    if !status.success() {
        bail!("kill -9 returned non-zero exit code");
    }
    Ok(())
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
        section.push_str(" -device qemu-xhci,id=xhci,p2=8,p3=8");
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
    insert_args_section(content, usb_section, "$USB_PASSTHROUGH_ARGS")
}

/// Generic function to insert a variable-definition section into a launch script
/// and append `$VAR_NAME` to all QEMU command endings.
///
/// The section is inserted at the top-level scope: before a `case` statement if
/// one exists, otherwise before the first QEMU command. This ensures the variable
/// is visible to all branches.
pub fn insert_args_section(content: &str, section: &str, var_ref: &str) -> String {
    if section.is_empty() {
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

    // Determine where to insert the section: before a `case` statement if present
    // (so the variable is in top-level scope), otherwise before the first QEMU command.
    let case_line = lines.iter().position(|l| {
        let trimmed = l.trim();
        trimmed.starts_with("case ") && !trimmed.starts_with('#')
    });
    let first_qemu_start = qemu_commands.first().map(|(start, _)| *start);
    let insert_before = case_line.or(first_qemu_start);

    for (i, line) in lines.iter().enumerate() {
        // Insert section at the chosen insertion point
        if Some(i) == insert_before && !inserted_section {
            result.push_str(section);
            result.push('\n');
            inserted_section = true;
        }

        // Modify ALL QEMU command endings to include the variable reference
        if qemu_end_indices.contains(&i) {
            let trimmed = line.trim_end();
            if let Some(comment_pos) = trimmed.find(" #") {
                let (cmd, comment) = trimmed.split_at(comment_pos);
                result.push_str(cmd);
                result.push(' ');
                result.push_str(var_ref);
                result.push_str(comment);
            } else {
                result.push_str(trimmed);
                result.push(' ');
                result.push_str(var_ref);
            }
            result.push('\n');
            continue;
        }

        result.push_str(line);
        if i < lines.len() - 1 {
            result.push('\n');
        }
    }

    // If we didn't find a suitable insertion point, append at the end
    if !inserted_section {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(section);
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
    let end = rest.find([',', ' ', '"', '\''])
        .unwrap_or(rest.len());

    let hex_str = &rest[..end];

    // Handle 0x prefix
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    u16::from_str_radix(hex_str, 16).ok()
}

// Shared Folders section markers
const SHARED_FOLDERS_MARKER_START: &str = "# >>> Shared Folders (managed by vm-curator) >>>";
const SHARED_FOLDERS_MARKER_END: &str = "# <<< Shared Folders <<<";

/// A shared folder configuration for virtio-9p host-to-guest file sharing
#[derive(Debug, Clone)]
pub struct SharedFolder {
    pub host_path: String,
    pub mount_tag: String,
}

/// Escape a string for safe use in shell scripts
fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return s.to_string();
    }
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

/// Save shared folders configuration to the VM's launch.sh
pub fn save_shared_folders(vm: &DiscoveredVm, folders: &[SharedFolder]) -> Result<()> {
    let script_path = &vm.launch_script;
    let content =
        std::fs::read_to_string(script_path).context("Failed to read launch.sh")?;

    // Remove existing shared folders section if present
    let content = remove_shared_folders_section(&content);

    // Determine device name based on architecture
    let device_name = if vm
        .config
        .emulator
        .command()
        .contains("aarch64")
        || vm.config.emulator.command().contains("arm")
    {
        "virtio-9p-device"
    } else {
        "virtio-9p-pci"
    };

    // Generate new shared folders section
    let section = generate_shared_folders_section(folders, device_name);

    // Insert into script
    let new_content = insert_shared_folders_section(&content, &section);

    std::fs::write(script_path, new_content).context("Failed to write launch.sh")?;

    Ok(())
}

/// Load shared folders configuration from the VM's launch.sh
pub fn load_shared_folders(vm: &DiscoveredVm) -> Vec<SharedFolder> {
    let content = match std::fs::read_to_string(&vm.launch_script) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    parse_shared_folders_section(&content)
}

fn remove_shared_folders_section(content: &str) -> String {
    let mut result = String::new();
    let mut in_section = false;

    for line in content.lines() {
        if line.trim() == SHARED_FOLDERS_MARKER_START {
            in_section = true;
            continue;
        }
        if line.trim() == SHARED_FOLDERS_MARKER_END {
            in_section = false;
            continue;
        }
        if !in_section {
            let cleaned_line = line
                .replace(" $SHARED_FOLDERS_ARGS", "")
                .replace("$SHARED_FOLDERS_ARGS ", "")
                .replace("$SHARED_FOLDERS_ARGS", "");
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

fn generate_shared_folders_section(folders: &[SharedFolder], device_name: &str) -> String {
    if folders.is_empty() {
        return String::new();
    }

    let mut section = String::new();
    section.push_str(SHARED_FOLDERS_MARKER_START);
    section.push('\n');
    section.push_str("SHARED_FOLDERS_ARGS=\"");

    for (i, folder) in folders.iter().enumerate() {
        let id = format!("fsdev{}", i);
        let escaped_path = shell_escape(&folder.host_path);

        if i > 0 {
            section.push(' ');
        }
        section.push_str(&format!(
            "-fsdev local,id={},path={},security_model=mapped-xattr -device {},fsdev={},mount_tag={}",
            id, escaped_path, device_name, id, folder.mount_tag
        ));
    }

    section.push_str("\"\n");
    section.push_str(SHARED_FOLDERS_MARKER_END);
    section.push('\n');

    section
}

fn insert_shared_folders_section(content: &str, section: &str) -> String {
    insert_args_section(content, section, "$SHARED_FOLDERS_ARGS")
}

fn parse_shared_folders_section(content: &str) -> Vec<SharedFolder> {
    let mut folders = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        if line.trim() == SHARED_FOLDERS_MARKER_START {
            in_section = true;
            continue;
        }
        if line.trim() == SHARED_FOLDERS_MARKER_END {
            in_section = false;
            continue;
        }
        if in_section && line.contains("SHARED_FOLDERS_ARGS=") {
            // Parse -fsdev local,id=...,path=...,security_model=... -device ...,mount_tag=...
            // Split on "-fsdev " to get each folder pair
            for part in line.split("-fsdev ") {
                if !part.contains("path=") {
                    continue;
                }
                let host_path = extract_path_value(part);
                let mount_tag = extract_simple_value(part, "mount_tag=");

                if let (Some(path), Some(tag)) = (host_path, mount_tag) {
                    folders.push(SharedFolder {
                        host_path: path,
                        mount_tag: tag,
                    });
                }
            }
        }
    }

    folders
}

/// Extract a path value from a -fsdev argument, handling shell quoting
fn extract_path_value(s: &str) -> Option<String> {
    let start = s.find("path=")? + 5;
    let rest = &s[start..];

    if let Some(inner) = rest.strip_prefix('\'') {
        // Single-quoted path: find matching closing quote (handle escaped quotes)
        let mut result = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\'' {
                // Check for escaped quote pattern: '\''
                if chars.as_str().starts_with("\\''") {
                    result.push('\'');
                    chars.next(); // skip backslash
                    chars.next(); // skip first quote
                } else {
                    break;
                }
            } else {
                result.push(c);
            }
        }
        Some(result)
    } else {
        // Unquoted path: ends at comma or space
        let end = rest
            .find([',', ' ', '"'])
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// Extract a simple key=value from a string
fn extract_simple_value(s: &str, prefix: &str) -> Option<String> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];
    let end = rest
        .find([',', ' ', '"', '\''])
        .unwrap_or(rest.len());
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_shared_folders_section_empty() {
        let section = generate_shared_folders_section(&[], "virtio-9p-pci");
        assert!(section.is_empty());
    }

    #[test]
    fn test_generate_shared_folders_section_single() {
        let folders = vec![SharedFolder {
            host_path: "/home/user/Documents".to_string(),
            mount_tag: "host_documents".to_string(),
        }];
        let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
        assert!(section.contains(SHARED_FOLDERS_MARKER_START));
        assert!(section.contains(SHARED_FOLDERS_MARKER_END));
        assert!(section.contains("path=/home/user/Documents"));
        assert!(section.contains("mount_tag=host_documents"));
        assert!(section.contains("virtio-9p-pci"));
        assert!(section.contains("fsdev0"));
    }

    #[test]
    fn test_generate_shared_folders_section_multiple() {
        let folders = vec![
            SharedFolder {
                host_path: "/home/user/Documents".to_string(),
                mount_tag: "host_documents".to_string(),
            },
            SharedFolder {
                host_path: "/home/user/Downloads".to_string(),
                mount_tag: "host_downloads".to_string(),
            },
        ];
        let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
        assert!(section.contains("fsdev0"));
        assert!(section.contains("fsdev1"));
        assert!(section.contains("mount_tag=host_documents"));
        assert!(section.contains("mount_tag=host_downloads"));
    }

    #[test]
    fn test_generate_shared_folders_section_arm() {
        let folders = vec![SharedFolder {
            host_path: "/tmp/share".to_string(),
            mount_tag: "host_share".to_string(),
        }];
        let section = generate_shared_folders_section(&folders, "virtio-9p-device");
        assert!(section.contains("virtio-9p-device"));
        assert!(!section.contains("virtio-9p-pci"));
    }

    #[test]
    fn test_generate_shared_folders_section_path_with_spaces() {
        let folders = vec![SharedFolder {
            host_path: "/home/user/My Documents".to_string(),
            mount_tag: "host_my_documents".to_string(),
        }];
        let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
        assert!(section.contains("'/home/user/My Documents'"));
    }

    #[test]
    fn test_parse_shared_folders_section() {
        let content = format!(
            "{}\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path=/home/user/docs,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=host_docs\"\n{}\n",
            SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
        );
        let folders = parse_shared_folders_section(&content);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].host_path, "/home/user/docs");
        assert_eq!(folders[0].mount_tag, "host_docs");
    }

    #[test]
    fn test_parse_shared_folders_section_quoted_path() {
        let content = format!(
            "{}\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path='/home/user/My Documents',security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=host_my_documents\"\n{}\n",
            SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
        );
        let folders = parse_shared_folders_section(&content);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].host_path, "/home/user/My Documents");
        assert_eq!(folders[0].mount_tag, "host_my_documents");
    }

    #[test]
    fn test_parse_shared_folders_section_multiple() {
        let content = format!(
            "{}\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path=/home/a,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=tag_a -fsdev local,id=fsdev1,path=/home/b,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev1,mount_tag=tag_b\"\n{}\n",
            SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
        );
        let folders = parse_shared_folders_section(&content);
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].host_path, "/home/a");
        assert_eq!(folders[0].mount_tag, "tag_a");
        assert_eq!(folders[1].host_path, "/home/b");
        assert_eq!(folders[1].mount_tag, "tag_b");
    }

    #[test]
    fn test_remove_shared_folders_section() {
        let content = "#!/bin/bash\n# >>> Shared Folders (managed by vm-curator) >>>\nSHARED_FOLDERS_ARGS=\"...\"\n# <<< Shared Folders <<<\nqemu-system-x86_64 $SHARED_FOLDERS_ARGS\n";
        let result = remove_shared_folders_section(content);
        assert!(!result.contains("SHARED_FOLDERS"));
        assert!(!result.contains(">>> Shared Folders"));
        assert!(result.contains("qemu-system-x86_64"));
    }

    #[test]
    fn test_insert_shared_folders_section_simple() {
        let content = "#!/bin/bash\nqemu-system-x86_64 -m 2048\n";
        let section = "# >>> Shared Folders (managed by vm-curator) >>>\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path=/tmp,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=host_tmp\"\n# <<< Shared Folders <<<\n";
        let result = insert_shared_folders_section(content, section);
        assert!(result.contains(SHARED_FOLDERS_MARKER_START));
        assert!(result.contains("$SHARED_FOLDERS_ARGS"));
        // Section should appear before QEMU command
        let marker_pos = result.find(SHARED_FOLDERS_MARKER_START).unwrap();
        let qemu_pos = result.find("qemu-system-x86_64").unwrap();
        assert!(marker_pos < qemu_pos);
    }

    #[test]
    fn test_insert_section_before_case_statement() {
        // Scripts with case statements need the variable defined BEFORE the case,
        // not inside a branch (otherwise other branches can't see it).
        let content = "#!/bin/bash\nVM_DIR=\".\"\ncase \"$1\" in\n    --install)\n        qemu-system-x86_64 -m 2048\n        ;;\n    \"\")\n        qemu-system-x86_64 -m 2048\n        ;;\nesac\n";
        let section = "# >>> Shared Folders (managed by vm-curator) >>>\nSHARED_FOLDERS_ARGS=\"test\"\n# <<< Shared Folders <<<\n";
        let result = insert_shared_folders_section(content, section);

        // Section must appear before the case statement
        let marker_pos = result.find(SHARED_FOLDERS_MARKER_START).unwrap();
        let case_pos = result.find("case \"$1\"").unwrap();
        assert!(marker_pos < case_pos, "Section must be before case statement, got marker at {} and case at {}", marker_pos, case_pos);

        // Both QEMU commands should have $SHARED_FOLDERS_ARGS appended
        let count = result.matches("$SHARED_FOLDERS_ARGS").count();
        assert_eq!(count, 2, "Expected 2 appended refs (one per QEMU command), got {}", count);
    }

    #[test]
    fn test_roundtrip_shared_folders() {
        let folders = vec![
            SharedFolder {
                host_path: "/home/user/Documents".to_string(),
                mount_tag: "host_documents".to_string(),
            },
            SharedFolder {
                host_path: "/home/user/My Pictures".to_string(),
                mount_tag: "host_my_pictures".to_string(),
            },
        ];
        let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
        let parsed = parse_shared_folders_section(&section);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].host_path, "/home/user/Documents");
        assert_eq!(parsed[0].mount_tag, "host_documents");
        assert_eq!(parsed[1].host_path, "/home/user/My Pictures");
        assert_eq!(parsed[1].mount_tag, "host_my_pictures");
    }

    #[test]
    fn test_shell_escape_safe() {
        assert_eq!(shell_escape("/home/user/docs"), "/home/user/docs");
        assert_eq!(shell_escape("my-file_name.txt"), "my-file_name.txt");
    }

    #[test]
    fn test_shell_escape_special() {
        assert_eq!(shell_escape("/home/user/My Documents"), "'/home/user/My Documents'");
        assert_eq!(shell_escape("path with spaces"), "'path with spaces'");
    }

    #[test]
    fn test_detect_qemu_processes_parsing() {
        // Simulate pgrep -a output parsing logic (same as detect_qemu_processes but without /proc)
        let output = "12345 qemu-system-x86_64 -m 4096 -drive file=disk.qcow2\n\
                       67890 qemu-system-aarch64 -m 2048 -hda test.img\n";
        let mut pids = Vec::new();
        let mut cmdlines = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            if let Some(space_pos) = line.find(' ') {
                if let Ok(pid) = line[..space_pos].parse::<u32>() {
                    pids.push(pid);
                    cmdlines.push(line[space_pos + 1..].to_string());
                }
            }
        }
        assert_eq!(pids.len(), 2);
        assert_eq!(pids[0], 12345);
        assert!(cmdlines[0].contains("disk.qcow2"));
        assert_eq!(pids[1], 67890);
        assert!(cmdlines[1].contains("test.img"));
    }
}
