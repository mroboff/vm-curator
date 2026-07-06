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
        BootMode::Recovery(dmg_path) => {
            // Validate DMG path exists before attempting to launch
            if !dmg_path.exists() {
                return LaunchResult {
                    success: false,
                    error: Some(format!("Recovery image not found: {}", dmg_path.display())),
                    vm_name,
                };
            }
            if !dmg_path.is_file() {
                return LaunchResult {
                    success: false,
                    error: Some(format!(
                        "Recovery image path is not a file: {}",
                        dmg_path.display()
                    )),
                    vm_name,
                };
            }
            args.push("--recovery".to_string());
            args.push(dmg_path.to_string_lossy().to_string());
        }
        BootMode::Floppy(floppy_path) => {
            if !floppy_path.exists() {
                return LaunchResult {
                    success: false,
                    error: Some(format!("Floppy image not found: {}", floppy_path.display())),
                    vm_name,
                };
            }
            if !floppy_path.is_file() {
                return LaunchResult {
                    success: false,
                    error: Some(format!(
                        "Floppy path is not a file: {}",
                        floppy_path.display()
                    )),
                    vm_name,
                };
            }
            args.push("--floppy".to_string());
            args.push(floppy_path.to_string_lossy().to_string());
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
            let stderr_lines = rx
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();

            // Filter for error-related lines for display
            let error_lines: Vec<&String> = stderr_lines
                .iter()
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
                error_lines
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
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
    if let Err(e) = ensure_qmp_in_script(&vm.path) {
        log::warn!("launch_vm_sync: could not patch QMP into launch.sh: {e}");
    }
    let result = launch_vm_with_error_check(vm, options);

    if result.success {
        Ok(())
    } else {
        bail!(
            "{}",
            result.error.unwrap_or_else(|| "Unknown error".to_string())
        )
    }
}

/// Launch VM with QEMU D-Bus display for GUI embedding.
/// Rewrites the launch script in memory to replace any existing -display flag
/// with -display dbus (session bus). Returns the child process PID on success.
#[allow(dead_code)]
pub fn launch_vm_dbus(vm: &DiscoveredVm) -> Result<u32> {
    let tmp = vm.path.join(".launch_dbus_tmp.sh");
    let _ = std::fs::remove_file(&tmp); // stale temp script from prior crash
    let _ = std::fs::remove_file(vm.path.join("qemu.sock")); // stale socket from unclean shutdown

    if let Err(e) = ensure_qmp_in_script(&vm.path) {
        log::warn!("launch_vm_dbus: could not patch QMP into launch.sh: {e}");
    }

    let script_path = vm.path.join("launch.sh");
    let content = std::fs::read_to_string(&script_path).context("Failed to read launch.sh")?;

    let modified = replace_display_for_dbus(&content, "-display dbus");

    std::fs::write(&tmp, &modified).context("Failed to write temp launch script")?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));

    let mut child = Command::new("bash")
        .arg(&tmp)
        .current_dir(&vm.path)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn QEMU")?;
    let pid = child.id();

    // Brief poll to catch immediate QEMU startup failures (missing session D-Bus,
    // bad flag, missing library) before returning Ok(pid) to the caller.
    thread::sleep(Duration::from_millis(300));
    if let Ok(Some(status)) = child.try_wait() {
        use std::io::Read;
        let stderr = child
            .stderr
            .take()
            .map(|mut s| {
                let mut b = String::new();
                s.read_to_string(&mut b).ok();
                b
            })
            .unwrap_or_default();
        bail!("QEMU exited immediately ({}): {}", status, stderr.trim());
    }
    // child intentionally dropped — QEMU process continues running

    let t = tmp.to_owned();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(4));
        let _ = std::fs::remove_file(t);
    });

    Ok(pid)
}

/// Replace every `-display <backend>` argument in a bash launch script with `replacement`.
/// Scripts with multiple case branches each get their own replacement.
/// Also strips SPICE-specific lines that are incompatible with dbus display.
#[allow(dead_code)]
fn replace_display_for_dbus(content: &str, replacement: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        let is_display =
            !t.starts_with('#') && (t.starts_with("-display ") || t.contains(" -display "));
        let bare = t.trim_end_matches('\\').trim_end();
        let is_spice = t.starts_with("-spice ")
            || (t.starts_with("-device virtio-serial") && t.contains("spice"))
            || (t.starts_with("-device virtserialport") && t.contains("com.redhat.spice"))
            || t.starts_with("-chardev spice")
            // SPICE clipboard channel (e.g. `-device virtio-serial-pci`) — incompatible
            // with the dbus display used for single-GPU passthrough.
            || crate::vm::create::SPICE_AGENT_ARGS.contains(&bare);
        if is_display {
            let indent_len = line.len() - line.trim_start().len();
            let indent = &line[..indent_len];
            if line.trim_end().ends_with('\\') {
                out.push(format!("{}{} \\", indent, replacement));
            } else {
                out.push(format!("{}{}", indent, replacement));
            }
        } else if !is_spice {
            out.push(line.to_string());
        }
    }
    let mut s = out.join("\n");
    if content.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Reset a VM by recreating its disk from a backing file or template
pub fn reset_vm(vm: &DiscoveredVm) -> Result<()> {
    // Find the primary disk
    let disk = vm
        .config
        .primary_disk()
        .context("VM has no disk configured")?;

    let disk_path = &disk.path;

    // Check for a backing file or template
    let info = super::snapshot::get_disk_info(disk_path).context("Failed to get disk info")?;

    if let Some(backing) = &info.backing_file {
        // Recreate from backing file
        let backing_path = Path::new(backing);
        if !backing_path.exists() {
            bail!("Backing file not found: {}", backing);
        }

        // Remove old disk
        std::fs::remove_file(disk_path).context("Failed to remove old disk")?;

        // Create new disk with backing file
        let disk_str = path_to_str(disk_path)?;
        let output = Command::new("qemu-img")
            .args([
                "create", "-f", "qcow2", "-F", "qcow2", "-b", backing, disk_str,
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
        std::fs::remove_dir_all(&vm.path).context("Failed to delete VM directory")?;
    } else {
        // Move to trash using trash-cli if available
        let result = Command::new("trash-put").arg(&vm.path).output();

        match result {
            Ok(output) if output.status.success() => {}
            _ => {
                // Fall back to moving to a .trash directory
                let trash_dir = vm.path.parent().unwrap_or(Path::new(".")).join(".trash");
                std::fs::create_dir_all(&trash_dir).context("Failed to create trash directory")?;

                // Find a unique name in trash (append timestamp if needed)
                let mut trash_path = trash_dir.join(&vm.id);
                if trash_path.exists() {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    trash_path = trash_dir.join(format!("{}-{}", vm.id, timestamp));
                }

                std::fs::rename(&vm.path, &trash_path).context("Failed to move VM to trash")?;
            }
        }
    }

    Ok(())
}

/// Rename a VM by updating its display name in vm-curator.toml
pub fn rename_vm(vm: &DiscoveredVm, new_name: &str) -> Result<()> {
    // Preserve existing os_profile and notes
    let os_profile = vm.os_profile.as_deref().or(Some(&vm.id));
    let notes = vm.notes.as_deref();

    crate::vm::create::write_vm_metadata(&vm.path, new_name, os_profile, notes)
        .context("Failed to write VM metadata")?;

    Ok(())
}

/// Save (or clear) the notes for a VM, preserving display_name and os_profile.
#[allow(dead_code)]
pub fn save_notes(vm: &DiscoveredVm, notes: Option<&str>) -> Result<()> {
    let display_name = vm.display_name();
    let os_profile = vm.os_profile.as_deref().or(Some(&vm.id));
    crate::vm::create::write_vm_metadata(&vm.path, &display_name, os_profile, notes)
        .context("Failed to write VM notes")?;
    Ok(())
}

/// A running QEMU process with its PID, command line, and working directory.
pub struct QemuProcess {
    pub pid: u32,
    pub cmdline: String,
    /// The working directory of the process (from `/proc/<pid>/cwd`)
    pub cwd: Option<std::path::PathBuf>,
}

/// Detect all running QEMU processes.
/// Returns process info including the working directory read from /proc.
pub fn detect_qemu_processes() -> Vec<QemuProcess> {
    let output = match Command::new("pgrep").args(["-a", "qemu-system"]).output() {
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
    let content = std::fs::read_to_string(script_path).context("Failed to read launch.sh")?;

    // Remove existing USB passthrough section if present
    let content = remove_usb_section(&content);

    // Generate new USB passthrough section
    let usb_section = generate_usb_section(devices);

    // Find where to insert the USB section
    // We want to add it before the final qemu-system command or at the end
    let new_content = insert_usb_section(&content, &usb_section);

    // Write back
    std::fs::write(script_path, new_content).context("Failed to write launch.sh")?;

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
    let qemu_end_indices: std::collections::HashSet<usize> =
        qemu_commands.iter().map(|(_, end)| *end).collect();

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
    let end = rest.find([',', ' ', '"', '\'']).unwrap_or(rest.len());

    let hex_str = &rest[..end];

    // Handle 0x prefix
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    u16::from_str_radix(hex_str, 16).ok()
}

// Shared Folders section markers
const SHARED_FOLDERS_MARKER_START: &str = "# >>> Shared Folders (managed by vm-curator) >>>";
const SHARED_FOLDERS_MARKER_END: &str = "# <<< Shared Folders <<<";

/// A shared folder configuration for virtio-9p host-to-guest file sharing
#[derive(Debug, Clone, PartialEq, Eq)]
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
    let content = std::fs::read_to_string(script_path).context("Failed to read launch.sh")?;

    // Remove existing shared folders section if present
    let content = remove_shared_folders_section(&content);

    // Determine device name based on architecture
    let device_name = if vm.config.emulator.command().contains("aarch64")
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
        let end = rest.find([',', ' ', '"']).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// Extract a simple key=value from a string
fn extract_simple_value(s: &str, prefix: &str) -> Option<String> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];
    let end = rest.find([',', ' ', '"', '\'']).unwrap_or(rest.len());
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
                                let arg = format!(
                                    "-device {}",
                                    part.split_whitespace().next().unwrap_or(part)
                                );
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

#[allow(dead_code)]
pub fn save_pci_passthrough(
    vm: &DiscoveredVm,
    devices: &[crate::hardware::PciDevice],
) -> Result<()> {
    let content =
        std::fs::read_to_string(&vm.launch_script).context("Failed to read launch script")?;

    // Strip existing section and $PCI_PASSTHROUGH_ARGS variable references
    let mut cleaned = String::new();
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
        if !in_pci_section {
            let cleaned_line = line
                .replace(" $PCI_PASSTHROUGH_ARGS", "")
                .replace("$PCI_PASSTHROUGH_ARGS ", "")
                .replace("$PCI_PASSTHROUGH_ARGS", "");
            cleaned.push_str(&cleaned_line);
            cleaned.push('\n');
        }
    }
    while cleaned.ends_with("\n\n") {
        cleaned.pop();
    }

    if devices.is_empty() {
        std::fs::write(&vm.launch_script, cleaned).context("Failed to write launch script")?;
        return Ok(());
    }

    // Build the PCI passthrough section with VFIO bind/restore helpers
    let args = crate::hardware::generate_passthrough_args(devices);
    let mut section = String::new();
    section.push_str(PCI_MARKER_START);
    section.push('\n');
    section.push_str("PCI_PASSTHROUGH_ARGS=\"");
    section.push_str(&args.join(" "));
    section.push_str("\"\n");
    section.push_str("PCI_DEVICES=(");
    for (i, dev) in devices.iter().enumerate() {
        if i > 0 {
            section.push(' ');
        }
        section.push_str(&format!("\"{}\"", dev.address));
    }
    section.push_str(")\n");
    section.push_str("declare -A PCI_ORIG_DRIVERS\n\n");

    section.push_str(r#"_pci_elevated() {
    if [[ $EUID -eq 0 ]]; then sh -c "$1"
    elif command -v pkexec >/dev/null 2>&1; then pkexec sh -c "$1"
    elif command -v sudo >/dev/null 2>&1; then sudo sh -c "$1"
    else echo "Error: root required to bind PCI devices"; return 1; fi
}
bind_vfio() {
    local bind_cmds=""
    for dev in "${PCI_DEVICES[@]}"; do
        local dev_path="/sys/bus/pci/devices/$dev"
        local driver_link="$dev_path/driver"
        [[ ! -d "$dev_path" ]] && { echo "Warning: $dev not found"; continue; }
        if [[ -L "$driver_link" ]]; then
            local current; current=$(basename "$(readlink "$driver_link")")
            PCI_ORIG_DRIVERS[$dev]="$current"
            [[ "$current" == "vfio-pci" ]] && { echo "$dev already bound"; continue; }
            bind_cmds+="echo '$dev' > '$driver_link/unbind' 2>/dev/null; sleep 0.1; "
        fi
        bind_cmds+="echo 'vfio-pci' > '$dev_path/driver_override'; "
        bind_cmds+="echo '$dev' > /sys/bus/pci/drivers_probe; "
    done
    [[ -n "$bind_cmds" ]] && { echo "Binding PCI devices to vfio-pci..."; _pci_elevated "$bind_cmds" || return 1; }
    sleep 0.5
}
restore_pci() {
    local restore_cmds=""
    for dev in "${PCI_DEVICES[@]}"; do
        local dev_path="/sys/bus/pci/devices/$dev"
        local orig="${PCI_ORIG_DRIVERS[$dev]:-}"
        [[ -z "$orig" || "$orig" == "vfio-pci" ]] && continue
        echo "Restoring $dev to $orig..."
        restore_cmds+="echo '$dev' > '$dev_path/driver/unbind' 2>/dev/null; "
        restore_cmds+="echo '' > '$dev_path/driver_override'; "
        restore_cmds+="echo '$dev' > /sys/bus/pci/drivers_probe; "
    done
    if [[ -n "$restore_cmds" ]]; then
        if [[ $EUID -eq 0 ]]; then sh -c "$restore_cmds"
        elif sudo -n true 2>/dev/null; then sudo sh -c "$restore_cmds"
        elif command -v pkexec >/dev/null 2>&1; then pkexec sh -c "$restore_cmds" 2>/dev/null
        else echo "Warning: could not restore PCI devices (reboot to restore)"; fi
    fi
}
if declare -f cleanup >/dev/null 2>&1; then
    eval "$(declare -f cleanup | sed '1s/cleanup/_pci_pre_cleanup/')"
    cleanup() { restore_pci; _pci_pre_cleanup; }
else
    trap 'restore_pci' EXIT
fi
bind_vfio || exit 1
"#);

    section.push_str(PCI_MARKER_END);
    section.push('\n');

    let new_content = insert_args_section(&cleaned, &section, "$PCI_PASSTHROUGH_ARGS");
    std::fs::write(&vm.launch_script, new_content).context("Failed to write launch script")?;
    Ok(())
}

// ── QMP (QEMU Machine Protocol) ─────────────────────────────────────────────
//
// The items below are `#[allow(dead_code)]` infrastructure for planned
// runtime VM control (pause/resume, GUI embedding via D-Bus). They are wired up
// but not yet surfaced in the UI — intentional, not dead.

#[allow(dead_code)]
const QMP_ARG: &str = "        -qmp unix:$VM_DIR/qemu.sock,server=on,wait=off";

/// Patch an existing launch.sh to include a QMP socket if not already present.
/// Idempotent — safe to call before every launch.
#[allow(dead_code)]
pub fn ensure_qmp_in_script(vm_path: &Path) -> Result<()> {
    let script_path = vm_path.join("launch.sh");
    let content =
        std::fs::read_to_string(&script_path).context("Failed to read launch.sh for QMP patch")?;

    if content.contains("qemu.sock") {
        return Ok(());
    }

    // Generated scripts always end each QEMU invocation block with a line containing
    // no trailing `\` followed immediately by `        ;;`. Walk line-by-line and insert
    // the QMP arg (with a continuation `\`) before each closing `;;` that follows a
    // non-continuation QEMU arg line.
    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::with_capacity(content.len() + 256);
    let mut in_qemu_block = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_end();

        if trimmed.contains("qemu-system") {
            in_qemu_block = true;
        }

        // Detect the last arg of a QEMU block: no trailing `\`, next non-empty line is `;;`
        if in_qemu_block
            && !trimmed.ends_with('\\')
            && !trimmed.is_empty()
            && lines.get(i + 1).map(|l| l.trim()) == Some(";;")
        {
            result.push_str(line);
            result.push_str(" \\\n");
            result.push_str(QMP_ARG);
            result.push('\n');
            in_qemu_block = false;
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    std::fs::write(&script_path, result).context("Failed to write patched launch.sh")?;
    Ok(())
}

/// Send a raw QMP command to a running VM's monitor socket.
#[allow(dead_code)]
fn qmp_send(vm_path: &Path, command: &str) -> Result<String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let sock = vm_path.join("qemu.sock");
    let stream = UnixStream::connect(&sock)
        .with_context(|| format!("QMP socket not available: {}", sock.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    // Read server greeting
    let mut line = String::new();
    reader.read_line(&mut line)?;

    // Negotiate capabilities (required before any command)
    writer.write_all(b"{\"execute\":\"qmp_capabilities\"}\n")?;
    line.clear();
    reader.read_line(&mut line)?;

    // Send the actual command
    writer.write_all(format!("{{\"execute\":\"{}\"}}\n", command).as_bytes())?;
    line.clear();
    reader.read_line(&mut line)?;
    Ok(line)
}

/// Pause a running VM (suspends guest execution, state preserved in memory).
#[allow(dead_code)]
pub fn pause_vm(vm_path: &Path) -> Result<()> {
    qmp_send(vm_path, "stop").map(|_| ())
}

/// Resume a paused VM.
#[allow(dead_code)]
pub fn resume_vm(vm_path: &Path) -> Result<()> {
    qmp_send(vm_path, "cont").map(|_| ())
}

/// Returns true if the VM is currently paused (QMP `query-status` → `"paused"`).
/// Returns false if not running, not reachable, or in any other state.
#[allow(dead_code)]
pub fn is_vm_paused(vm_path: &Path) -> bool {
    qmp_send(vm_path, "query-status")
        .map(|resp| resp.contains("\"paused\""))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "tests/lifecycle.rs"]
mod tests;
