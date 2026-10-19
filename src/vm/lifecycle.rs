use anyhow::{bail, Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    /// USB passthrough is persisted in the VM's launch script. Runtime devices
    /// are ignored so QEMU flags are not passed to the script as shell options.
    pub usb_devices: Vec<UsbPassthrough>,
    pub window_size: Option<WindowSize>,
}

/// Requested VM display size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let (width, height) = value.split_once('x').or_else(|| value.split_once('X'))?;
        let width = width.trim().parse::<u32>().ok()?;
        let height = height.trim().parse::<u32>().ok()?;

        if (320..=16384).contains(&width) && (200..=16384).contains(&height) {
            Some(Self { width, height })
        } else {
            None
        }
    }
}

impl fmt::Display for WindowSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl Serialize for WindowSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for WindowSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value)
            .ok_or_else(|| serde::de::Error::custom("expected window size in WIDTHxHEIGHT format"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VideoOverrideSpec {
    default_flag: &'static str,
    default_value: &'static str,
    override_device: &'static str,
}

/// USB device for passthrough
#[derive(Debug, Clone)]
pub struct UsbPassthrough {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usb_version: UsbVersion,
    /// Firmware boot priority (lower boots first); None leaves boot order alone
    pub bootindex: Option<u32>,
}

impl UsbPassthrough {
    /// Check if this device is USB 3.0 or higher
    pub fn is_usb3(&self) -> bool {
        self.usb_version.is_usb3()
    }
}

/// A physical disk passed through to the guest (whole block device)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskPassthrough {
    /// Device path, preferably a stable /dev/disk/by-id link
    pub path: String,
    /// Firmware boot priority (lower boots first); None leaves boot order alone
    pub bootindex: Option<u32>,
}

/// Fail fast when the VM uses a bridge network whose bridge does not exist.
fn check_bridge_exists(vm: &DiscoveredVm) -> Result<(), String> {
    use crate::vm::qemu_config::NetworkBackend;
    for network in &vm.config.networks {
        let NetworkBackend::Bridge(bridge) = &network.backend else {
            continue;
        };
        if std::path::Path::new("/sys/class/net").join(bridge).exists() {
            continue;
        }
        if bridge.starts_with(crate::vnet::BRIDGE_PREFIX) {
            return Err(format!(
                "Managed network bridge '{}' is not running.\n\
                 Start it from the Networks screen ([n] on the main menu) and try again.",
                bridge
            ));
        } else {
            return Err(format!(
                "Bridge '{}' does not exist on the host. Create it (or pick another \
                 network backend in Network Settings) and try again.",
                bridge
            ));
        }
    }
    Ok(())
}

/// Per-VM log of the last launch: the exact command, then everything
/// `launch.sh` and QEMU wrote to stdout/stderr, streamed live.
///
/// QEMU's stderr used to be captured only in memory and shown only if the
/// process died within the first second — warnings from a VM that *ran* (GL,
/// virgl, audio, USB) were unrecoverable. The log lives next to the VM so it
/// is trivial to find and to attach to a bug report.
pub const QEMU_LOG_FILE: &str = "qemu.log";
/// The previous launch's log, kept for comparison.
pub const QEMU_LOG_PREVIOUS: &str = "qemu.log.old";

/// Path of the VM's launch log.
pub fn qemu_log_path(vm_path: &Path) -> PathBuf {
    vm_path.join(QEMU_LOG_FILE)
}

/// An open launch log: the file both child streams are pointed at, plus
/// where it lives (the VM directory, or a temp-dir fallback if that failed).
struct QemuLog {
    file: std::fs::File,
    path: PathBuf,
}

/// Rotate the previous log aside and open a fresh one with a launch header.
///
/// The child's stdout and stderr are redirected straight into this file, so
/// logging does not depend on the launcher staying alive: `vm-curator launch`
/// returns within a second while QEMU keeps writing for hours. If the VM
/// directory is not writable the log falls back to the system temp dir.
fn open_qemu_log(vm_path: &Path, invocation: &LaunchInvocation) -> std::io::Result<QemuLog> {
    let path = qemu_log_path(vm_path);
    let (mut file, path) = match rotate_and_create(&path, &vm_path.join(QEMU_LOG_PREVIOUS)) {
        Ok(file) => (file, path),
        Err(e) => {
            log::warn!(
                "could not open {}: {e}; logging to temp dir",
                path.display()
            );
            let name = vm_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "vm".to_string());
            let fallback = std::env::temp_dir().join(format!("vm-curator-{name}.log"));
            let file = rotate_and_create(&fallback, &fallback.with_extension("log.old"))?;
            (file, fallback)
        }
    };
    writeln!(
        file,
        "# vm-curator launch {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z")
    )?;
    writeln!(
        file,
        "# command: {} {}",
        invocation.program,
        invocation.args.join(" ")
    )?;
    for (name, value) in &invocation.env {
        if let Some(value) = value {
            writeln!(file, "# env: {name}={value}")?;
        }
    }
    writeln!(file, "# --- stdout/stderr of launch.sh and QEMU ---")?;
    file.flush()?;
    Ok(QemuLog { file, path })
}

fn rotate_and_create(path: &Path, previous: &Path) -> std::io::Result<std::fs::File> {
    if path.exists() {
        let _ = std::fs::rename(path, previous);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Point the child's stdout and stderr at the log (both handles share one
/// append-mode file description, so lines interleave in write order).
fn redirect_output_to_log(cmd: &mut Command, log: &QemuLog) {
    match (log.file.try_clone(), log.file.try_clone()) {
        (Ok(out), Ok(err)) => {
            cmd.stdout(Stdio::from(out)).stderr(Stdio::from(err));
        }
        _ => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
}

/// The process output recorded in the log so far (header lines excluded).
fn logged_output_lines(log: &QemuLog) -> Vec<String> {
    std::fs::read_to_string(&log.path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("# ") && !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchInvocation {
    program: String,
    current_dir: PathBuf,
    args: Vec<String>,
    env: Vec<(String, Option<String>)>,
}

impl LaunchInvocation {
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.current_dir(&self.current_dir).args(&self.args);
        for (name, value) in &self.env {
            if let Some(value) = value {
                cmd.env(name, value);
            } else {
                cmd.env_remove(name);
            }
        }
        cmd
    }
}

fn build_launch_invocation(
    vm: &DiscoveredVm,
    options: &LaunchOptions,
) -> std::result::Result<LaunchInvocation, String> {
    let mut invocation = LaunchInvocation {
        program: "bash".to_string(),
        current_dir: vm.path.clone(),
        args: vec![vm.launch_script.to_string_lossy().to_string()],
        env: vec![(
            "VM_CURATOR_WINDOW_SIZE".to_string(),
            options.window_size.map(|size| size.to_string()),
        )],
    };

    match &options.boot_mode {
        BootMode::Normal => {}
        BootMode::Install => {
            invocation.args.push("--install".to_string());
        }
        BootMode::Cdrom(iso_path) => {
            if !iso_path.exists() {
                return Err(format!("ISO file not found: {}", iso_path.display()));
            }
            if !iso_path.is_file() {
                return Err(format!("ISO path is not a file: {}", iso_path.display()));
            }
            invocation.args.push("--cdrom".to_string());
            invocation.args.push(iso_path.to_string_lossy().to_string());
        }
        BootMode::Recovery(dmg_path) => {
            if !dmg_path.exists() {
                return Err(format!("Recovery image not found: {}", dmg_path.display()));
            }
            if !dmg_path.is_file() {
                return Err(format!(
                    "Recovery image path is not a file: {}",
                    dmg_path.display()
                ));
            }
            invocation.args.push("--recovery".to_string());
            invocation.args.push(dmg_path.to_string_lossy().to_string());
        }
        BootMode::Floppy(floppy_path) => {
            if !floppy_path.exists() {
                return Err(format!("Floppy image not found: {}", floppy_path.display()));
            }
            if !floppy_path.is_file() {
                return Err(format!(
                    "Floppy path is not a file: {}",
                    floppy_path.display()
                ));
            }
            invocation.args.push("--floppy".to_string());
            invocation
                .args
                .push(floppy_path.to_string_lossy().to_string());
        }
        BootMode::Network => {
            invocation.args.push("--netboot".to_string());
        }
    }

    invocation.args.extend(options.extra_args.clone());

    if !options.usb_devices.is_empty() {
        log::warn!(
            "Ignoring LaunchOptions::usb_devices; save USB passthrough to launch.sh before launching"
        );
    }

    Ok(invocation)
}

/// Launch a VM and monitor for immediate errors
///
/// This function spawns the VM process and monitors stderr for a brief period
/// to catch any immediate startup errors (like missing files, invalid arguments, etc.)
/// If the process exits with an error within the monitoring window, we capture it.
pub fn launch_vm_with_error_check(vm: &DiscoveredVm, options: &LaunchOptions) -> LaunchResult {
    let vm_name = vm.display_name();

    // Preflight: a bridge backend needs its bridge to exist NOW, otherwise
    // QEMU fails with a cryptic bridge-helper error. Managed networks
    // (issue #53) get a pointer to the Networks screen.
    if let Err(error) = check_bridge_exists(vm) {
        return LaunchResult {
            success: false,
            error: Some(error),
            vm_name,
        };
    }

    if let Err(e) = ensure_qmp_in_script(&vm.path) {
        log::warn!("launch_vm_with_error_check: could not patch QMP into launch.sh: {e}");
    }
    if let Err(e) = ensure_name_in_script(&vm.path) {
        log::warn!("launch_vm_with_error_check: could not patch -name into launch.sh: {e}");
    }
    if let Err(e) = ensure_ovmf_vars_writable(&vm.path) {
        log::warn!("launch_vm_with_error_check: could not make OVMF_VARS writable: {e}");
    }

    let invocation = match build_launch_invocation(vm, options) {
        Ok(invocation) => invocation,
        Err(error) => {
            return LaunchResult {
                success: false,
                error: Some(error),
                vm_name,
            };
        }
    };

    if options.window_size.is_some() {
        if let Err(e) = ensure_window_size_override_in_script(&vm.launch_script) {
            log::warn!(
                "launch_vm_with_error_check: could not patch window-size override into launch.sh: {e}"
            );
        }
    }

    // Per-VM log: both child streams go straight to the file, so the log is
    // complete no matter how soon this launcher returns.
    let log = match open_qemu_log(&vm.path, &invocation) {
        Ok(log) => log,
        Err(e) => {
            return LaunchResult {
                success: false,
                error: Some(format!("Failed to open launch log: {e}")),
                vm_name,
            };
        }
    };
    let log_hint = format!("\nFull log: {}", log.path.display());

    let mut cmd = invocation.command();
    cmd.stdin(Stdio::null());
    redirect_output_to_log(&mut cmd, &log);

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

    // Wait for QEMU to either start successfully or fail
    // QEMU typically fails fast if there's a configuration error
    thread::sleep(Duration::from_millis(800));

    // Check if the process has already exited (indicating an error)
    match child.try_wait() {
        Ok(Some(status)) => {
            // Process exited - this usually means an error for QEMU
            // (successful QEMU keeps running until the VM shuts down)

            // Everything the process wrote is already in the log file.
            let stderr_lines = logged_output_lines(&log);

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
                error: Some(format!("{error_msg}{log_hint}")),
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
    let tmp = vm.path.join(".launch_dbus_override.sh");
    let _ = std::fs::remove_file(&tmp); // stale override script from prior crash
    let _ = std::fs::remove_file(vm.path.join("qemu.sock")); // stale socket from unclean shutdown

    if let Err(e) = ensure_qmp_in_script(&vm.path) {
        log::warn!("launch_vm_dbus: could not patch QMP into launch.sh: {e}");
    }
    if let Err(e) = ensure_name_in_script(&vm.path) {
        log::warn!("launch_vm_dbus: could not patch -name into launch.sh: {e}");
    }
    if let Err(e) = ensure_ovmf_vars_writable(&vm.path) {
        log::warn!("launch_vm_dbus: could not make OVMF_VARS writable: {e}");
    }

    let script_path = vm.path.join("launch.sh");
    let content = std::fs::read_to_string(&script_path).context("Failed to read launch.sh")?;

    let modified = replace_display_for_dbus(&content, "-display dbus");

    std::fs::write(&tmp, &modified).context("Failed to write D-Bus launch override script")?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));

    let invocation = LaunchInvocation {
        program: "bash".to_string(),
        current_dir: vm.path.clone(),
        args: vec![tmp.to_string_lossy().to_string()],
        env: Vec::new(),
    };
    let log = open_qemu_log(&vm.path, &invocation).context("Failed to open launch log")?;

    let mut cmd = invocation.command();
    redirect_output_to_log(&mut cmd, &log);
    let mut child = cmd.spawn().context("Failed to spawn QEMU")?;
    let pid = child.id();

    // Brief poll to catch immediate QEMU startup failures (missing session D-Bus,
    // bad flag, missing library) before returning Ok(pid) to the caller.
    thread::sleep(Duration::from_millis(300));
    if let Ok(Some(status)) = child.try_wait() {
        let output = logged_output_lines(&log).join("\n");
        bail!(
            "QEMU exited immediately ({}): {}\nFull log: {}",
            status,
            output.trim(),
            log.path.display()
        );
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

    if disk.is_physical_device() {
        bail!(
            "Cannot reset a physical passthrough disk ({}). \
             Resetting would destroy data on the host device.",
            disk.path.display()
        );
    }

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
        if let Some(bootindex) = device.bootindex {
            section.push_str(&format!(",bootindex={}", bootindex));
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
                        let bootindex = part.split("bootindex=").nth(1).and_then(|rest| {
                            let digits: String =
                                rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                            digits.parse::<u32>().ok()
                        });
                        devices.push(UsbPassthrough {
                            vendor_id: vid,
                            product_id: pid,
                            usb_version,
                            bootindex,
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

// Disk Passthrough configuration markers
const DISK_PASSTHROUGH_MARKER_START: &str = "# >>> Disk Passthrough (managed by vm-curator) >>>";
const DISK_PASSTHROUGH_MARKER_END: &str = "# <<< Disk Passthrough <<<";

/// Save physical disk passthrough configuration to the VM's launch.sh
pub fn save_disk_passthrough(vm: &DiscoveredVm, disks: &[DiskPassthrough]) -> Result<()> {
    let script_path = &vm.launch_script;
    let content = std::fs::read_to_string(script_path).context("Failed to read launch.sh")?;

    let content = remove_disk_passthrough_section(&content);
    let section = generate_disk_passthrough_section(disks);
    let new_content = insert_args_section(&content, &section, "$DISK_PASSTHROUGH_ARGS");

    std::fs::write(script_path, new_content).context("Failed to write launch.sh")?;
    Ok(())
}

/// Load physical disk passthrough configuration from the VM's launch.sh
pub fn load_disk_passthrough(vm: &DiscoveredVm) -> Vec<DiskPassthrough> {
    let script_path = &vm.launch_script;
    match std::fs::read_to_string(script_path) {
        Ok(content) => parse_disk_passthrough_section(&content),
        Err(_) => Vec::new(),
    }
}

fn remove_disk_passthrough_section(content: &str) -> String {
    let mut result = String::new();
    let mut in_section = false;

    for line in content.lines() {
        if line.trim() == DISK_PASSTHROUGH_MARKER_START {
            in_section = true;
            continue;
        }
        if line.trim() == DISK_PASSTHROUGH_MARKER_END {
            in_section = false;
            continue;
        }
        if !in_section {
            let cleaned = line
                .replace(" $DISK_PASSTHROUGH_ARGS", "")
                .replace("$DISK_PASSTHROUGH_ARGS ", "")
                .replace("$DISK_PASSTHROUGH_ARGS", "");
            result.push_str(&cleaned);
            result.push('\n');
        }
    }

    while result.ends_with("\n\n") {
        result.pop();
    }

    result
}

fn generate_disk_passthrough_section(disks: &[DiskPassthrough]) -> String {
    if disks.is_empty() {
        return String::new();
    }

    let mut section = String::new();
    section.push_str(DISK_PASSTHROUGH_MARKER_START);
    section.push('\n');
    section.push_str(
        "# WARNING: passes the raw physical disks below to the guest — all data on them\n\
         # is writable by the VM\n",
    );

    // Preflight: refuse to start if a disk is missing, inaccessible, or mounted
    section.push_str("for _pdisk in");
    for disk in disks {
        section.push_str(&format!(" '{}'", disk.path));
    }
    section.push_str("; do\n");
    section.push_str(
        "    if [[ ! -b \"$_pdisk\" ]]; then\n\
         \x20       echo \"Error: passthrough disk not found: $_pdisk\"; exit 1\n\
         \x20   fi\n\
         \x20   if [[ ! -r \"$_pdisk\" || ! -w \"$_pdisk\" ]]; then\n\
         \x20       echo \"Error: no read/write access to $_pdisk\"\n\
         \x20       echo \"Fix (pick one):\"\n\
         \x20       echo \"  sudo usermod -aG disk $USER   # then log out and back in\"\n\
         \x20       echo \"  or add a udev rule granting your user access to this disk\"\n\
         \x20       exit 1\n\
         \x20   fi\n\
         \x20   if command -v lsblk >/dev/null 2>&1 \\\n\
         \x20      && lsblk -no MOUNTPOINTS \"$(readlink -f \"$_pdisk\")\" 2>/dev/null | grep -q .; then\n\
         \x20       echo \"Error: $_pdisk has mounted partitions; unmount them first\"; exit 1\n\
         \x20   fi\n\
         done\n",
    );

    section.push_str("DISK_PASSTHROUGH_ARGS=\"");
    for (i, disk) in disks.iter().enumerate() {
        if i > 0 {
            section.push(' ');
        }
        section.push_str(&format!(
            "-drive file={},format=raw,if=none,id=pdisk{},cache=none -device virtio-blk-pci,drive=pdisk{}",
            disk.path, i, i
        ));
        if let Some(bootindex) = disk.bootindex {
            section.push_str(&format!(",bootindex={}", bootindex));
        }
    }
    // Let the firmware offer a boot menu so passthrough disks can be chosen
    section.push_str(" -boot menu=on");
    section.push_str("\"\n");
    section.push_str(DISK_PASSTHROUGH_MARKER_END);
    section.push('\n');

    section
}

fn parse_disk_passthrough_section(content: &str) -> Vec<DiskPassthrough> {
    let mut disks = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        if line.trim() == DISK_PASSTHROUGH_MARKER_START {
            in_section = true;
            continue;
        }
        if line.trim() == DISK_PASSTHROUGH_MARKER_END {
            in_section = false;
            continue;
        }
        if in_section && line.contains("DISK_PASSTHROUGH_ARGS=") {
            // Paths come from "-drive file=<path>,..."; bootindex (if any) from
            // the paired "-device virtio-blk-pci,drive=pdiskN[,bootindex=N]".
            // Drives and devices are emitted in the same order, so zip by index.
            let mut paths = Vec::new();
            for part in line.split("-drive file=").skip(1) {
                if let Some(path) = part.split(',').next() {
                    paths.push(path.to_string());
                }
            }
            let mut bootindexes = Vec::new();
            for part in line.split("-device virtio-blk-pci,").skip(1) {
                let bootindex = part.split("bootindex=").nth(1).and_then(|rest| {
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    digits.parse::<u32>().ok()
                });
                bootindexes.push(bootindex);
            }
            for (i, path) in paths.into_iter().enumerate() {
                disks.push(DiskPassthrough {
                    path,
                    bootindex: bootindexes.get(i).copied().flatten(),
                });
            }
        }
    }

    disks
}

// Shared Folders section markers
const SHARED_FOLDERS_MARKER_START: &str = "# >>> Shared Folders (managed by vm-curator) >>>";
const SHARED_FOLDERS_MARKER_END: &str = "# <<< Shared Folders <<<";
const SHARED_FOLDERS_ARGS_REF: &str = "\"${SHARED_FOLDERS_ARGS[@]}\"";

/// How a shared folder is exposed to the guest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShareMode {
    /// virtio-9p filesystem device, mounted in the guest by `mount_tag`.
    #[default]
    NineP,
    /// QEMU's built-in SMB server on the user-mode (SLIRP) netdev. The guest
    /// reaches it at `\\10.0.2.4\qemu` with no driver install (#101). Needs
    /// `smbd` (samba) on the host and a `user` network backend; QEMU exports
    /// a single directory per netdev, so at most one folder may use this mode.
    Smb,
}

/// UNC path at which the guest sees an SMB-mode shared folder (SLIRP's default
/// SMB server address and QEMU's fixed share name).
pub const SMB_GUEST_SHARE: &str = "\\\\10.0.2.4\\qemu";

/// A shared folder configuration for host-to-guest file sharing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFolder {
    pub host_path: String,
    /// 9p mount tag. Kept for SMB-mode folders too so switching modes is lossless.
    pub mount_tag: String,
    pub mode: ShareMode,
}

impl SharedFolder {
    /// A virtio-9p folder with an explicit mount tag.
    pub fn nine_p(host_path: impl Into<String>, mount_tag: impl Into<String>) -> Self {
        Self {
            host_path: host_path.into(),
            mount_tag: mount_tag.into(),
            mode: ShareMode::NineP,
        }
    }

    /// An SMB folder; the mount tag is derived from the path in case the user
    /// later switches it to 9p.
    pub fn smb(host_path: impl Into<String>) -> Self {
        let host_path = host_path.into();
        let mount_tag = generate_mount_tag(&host_path);
        Self {
            host_path,
            mount_tag,
            mode: ShareMode::Smb,
        }
    }
}

/// Generate a 9p mount tag from a host directory path (`/home/u/My Docs` → `host_my_docs`).
pub fn generate_mount_tag(path: &str) -> String {
    let folder_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("shared");
    let sanitized: String = folder_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let tag = sanitized
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!(
        "host_{}",
        if tag.is_empty() {
            "shared".to_string()
        } else {
            tag
        }
    )
}

/// Find the id of the user-mode (SLIRP) netdev in a launch script, e.g. `net0`
/// from `-netdev user,id=net0,hostfwd=...`. Looks at the last QEMU invocation
/// (the default boot branch in vm-curator's generated scripts) first, then the
/// whole script, so a `--install` branch with a different setup can't mislead it.
pub fn find_user_netdev_id(content: &str) -> Option<String> {
    fn scan(text: &str) -> Option<String> {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            let Some(pos) = trimmed.find("-netdev") else {
                continue;
            };
            let after = trimmed[pos + "-netdev".len()..].trim_start();
            // The netdev spec is the next shell word; it may be quoted.
            let spec = after
                .trim_start_matches(['"', '\''])
                .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '\\')
                .next()
                .unwrap_or("");
            if !(spec.starts_with("user,") || spec == "user" || spec.starts_with("user ")) {
                continue;
            }
            for opt in spec.split(',') {
                if let Some(id) = opt.strip_prefix("id=") {
                    if !id.is_empty() {
                        return Some(id.to_string());
                    }
                }
            }
        }
        None
    }

    // Prefer the default boot branch: the last qemu-system-* block.
    let lines: Vec<&str> = content.lines().collect();
    let mut last_start = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if !t.starts_with('#')
            && (t.starts_with("qemu-system-") || t.starts_with("exec qemu-system-"))
        {
            last_start = Some(i);
        }
    }
    if let Some(start) = last_start {
        let mut end = start;
        while end + 1 < lines.len() && lines[end].trim_end().ends_with('\\') {
            end += 1;
        }
        if let Some(id) = scan(&lines[start..=end].join("\n")) {
            return Some(id);
        }
    }
    scan(content)
}

/// Where QEMU expects the host `smbd` binary for its built-in SMB server.
/// Distro builds compile in `/usr/sbin/smbd`; fall back to a PATH lookup.
pub fn smbd_available() -> bool {
    if std::path::Path::new("/usr/sbin/smbd").exists() {
        return true;
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join("smbd").is_file()))
        .unwrap_or(false)
}

/// Validate an SMB-mode folder set against the launch script before saving.
/// Returns the user netdev id to attach the share to.
fn validate_smb_folders(folders: &[SharedFolder], content: &str) -> Result<Option<String>> {
    let smb_count = folders.iter().filter(|f| f.mode == ShareMode::Smb).count();
    if smb_count == 0 {
        return Ok(None);
    }
    if smb_count > 1 {
        anyhow::bail!(
            "QEMU's built-in SMB server exports a single directory; only one folder can use SMB mode (share a parent directory instead)"
        );
    }
    match find_user_netdev_id(content) {
        Some(id) => Ok(Some(id)),
        None => anyhow::bail!(
            "SMB shares need user-mode networking (the `user` backend); this VM's launch.sh has no `-netdev user` — change it in Network Settings or use 9p mode"
        ),
    }
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

    // SMB folders piggyback on the user-mode netdev; make sure there is one.
    let smb_netdev = validate_smb_folders(folders, &content)?;

    // Determine device name based on architecture
    let device_name = if vm.config.emulator.command().contains("aarch64")
        || vm.config.emulator.command().contains("arm")
    {
        "virtio-9p-device"
    } else {
        "virtio-9p-pci"
    };

    // Generate new shared folders section
    let section = generate_shared_folders_section(folders, device_name, smb_netdev.as_deref());

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
            let cleaned_line = remove_shared_folders_arg_ref(line);
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

fn remove_shared_folders_arg_ref(line: &str) -> String {
    let mut cleaned = line.to_string();
    for var_ref in [
        " \"${SHARED_FOLDERS_ARGS[@]}\"",
        "\"${SHARED_FOLDERS_ARGS[@]}\" ",
        "\"${SHARED_FOLDERS_ARGS[@]}\"",
        " ${SHARED_FOLDERS_ARGS[@]}",
        "${SHARED_FOLDERS_ARGS[@]} ",
        "${SHARED_FOLDERS_ARGS[@]}",
        " $SHARED_FOLDERS_ARGS",
        "$SHARED_FOLDERS_ARGS ",
        "$SHARED_FOLDERS_ARGS",
    ] {
        cleaned = cleaned.replace(var_ref, "");
    }
    cleaned
}

/// Build the managed `SHARED_FOLDERS_ARGS` block.
///
/// 9p folders become `-fsdev`/`-device` pairs. An SMB folder becomes
/// `-set netdev.<id>.smb=<path>`, which QEMU applies to the already-defined
/// user netdev (`-set` runs after all `-netdev` options on the command line,
/// which is where this block is expanded). Keeping it here rather than on the
/// `-netdev` line itself means the network generator/parser stay untouched,
/// and commas in the path need no escaping (unlike inline `smb=`).
fn generate_shared_folders_section(
    folders: &[SharedFolder],
    device_name: &str,
    smb_netdev: Option<&str>,
) -> String {
    if folders.is_empty() {
        return String::new();
    }

    let mut section = String::new();
    section.push_str(SHARED_FOLDERS_MARKER_START);
    section.push('\n');
    section.push_str("SHARED_FOLDERS_ARGS=(\n");

    for (i, folder) in folders.iter().enumerate() {
        if folder.mode == ShareMode::Smb {
            let netdev = smb_netdev.unwrap_or("net0");
            let set_arg = format!("netdev.{}.smb={}", netdev, folder.host_path);
            section.push_str("    -set\n");
            section.push_str("    ");
            section.push_str(&shell_escape(&set_arg));
            section.push('\n');
            continue;
        }

        let id = format!("fsdev{}", i);
        let fsdev_arg = format!(
            "local,id={},path={},security_model=mapped-xattr",
            id, folder.host_path
        );
        let device_arg = format!(
            "{},fsdev={},mount_tag={}",
            device_name, id, folder.mount_tag
        );

        section.push_str("    -fsdev\n");
        section.push_str("    ");
        section.push_str(&shell_escape(&fsdev_arg));
        section.push('\n');
        section.push_str("    -device\n");
        section.push_str("    ");
        section.push_str(&shell_escape(&device_arg));
        section.push('\n');
    }

    section.push_str(")\n");
    section.push_str(SHARED_FOLDERS_MARKER_END);
    section.push('\n');

    section
}

fn insert_shared_folders_section(content: &str, section: &str) -> String {
    insert_args_section(content, section, SHARED_FOLDERS_ARGS_REF)
}

fn parse_shared_folders_section(content: &str) -> Vec<SharedFolder> {
    let mut folders = Vec::new();
    let mut in_section = false;
    let mut in_array = false;
    let mut array_lines = Vec::new();

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
            continue;
        }

        let trimmed = line.trim();
        if in_array {
            if let Some(before_end) = strip_array_closing_paren(line) {
                if !before_end.trim().is_empty() {
                    array_lines.push(before_end.trim().to_string());
                }
                folders.extend(parse_shared_folders_array_lines(&array_lines));
                array_lines.clear();
                in_array = false;
            } else {
                array_lines.push(line.to_string());
            }
            continue;
        }

        if let Some(after_start) = trimmed.strip_prefix("SHARED_FOLDERS_ARGS=(") {
            let inline = after_start.trim();
            if let Some(before_end) = strip_array_closing_paren(inline) {
                if !before_end.trim().is_empty() {
                    array_lines.push(before_end.trim().to_string());
                }
                folders.extend(parse_shared_folders_array_lines(&array_lines));
                array_lines.clear();
            } else {
                if !inline.is_empty() {
                    array_lines.push(inline.to_string());
                }
                in_array = true;
            }
            continue;
        }

        if line.contains("SHARED_FOLDERS_ARGS=") {
            folders.extend(parse_shared_folders_legacy_line(line));
        }
    }

    if in_array {
        folders.extend(parse_shared_folders_array_lines(&array_lines));
    }

    folders
}

fn strip_array_closing_paren(line: &str) -> Option<&str> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let line = line.trim_end();
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut close_idx = None;

    for (idx, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match quote {
            Quote::None => match c {
                '\\' => escaped = true,
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                ')' => close_idx = Some(idx),
                _ => {}
            },
            Quote::Single => {
                if c == '\'' {
                    quote = Quote::None;
                }
            }
            Quote::Double => match c {
                '\\' => escaped = true,
                '"' => quote = Quote::None,
                _ => {}
            },
        }
    }

    if quote != Quote::None {
        return None;
    }

    let close_idx = close_idx?;
    let trailing = line[close_idx + 1..].trim();
    if trailing.is_empty() || trailing.starts_with('#') {
        Some(line[..close_idx].trim_end())
    } else {
        None
    }
}

fn parse_shared_folders_legacy_line(line: &str) -> Vec<SharedFolder> {
    let mut folders = Vec::new();

    // Legacy scalar format:
    // SHARED_FOLDERS_ARGS="-fsdev local,id=...,path=...,security_model=... -device ...,mount_tag=..."
    for part in line.split("-fsdev ") {
        if !part.contains("path=") {
            continue;
        }
        let host_path = extract_path_value(part);
        let mount_tag = extract_simple_value(part, "mount_tag=");

        if let (Some(path), Some(tag)) = (host_path, mount_tag) {
            folders.push(SharedFolder::nine_p(path, tag));
        }
    }

    folders
}

fn parse_shared_folders_array_lines(lines: &[String]) -> Vec<SharedFolder> {
    let mut tokens = Vec::new();
    for line in lines {
        tokens.extend(parse_shell_words(line));
    }
    parse_shared_folder_tokens(&tokens)
}

fn parse_shared_folder_tokens(tokens: &[String]) -> Vec<SharedFolder> {
    let mut folders = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        // SMB-mode folder: `-set netdev.<id>.smb=<path>`
        if tokens[i] == "-set" {
            if let Some(path) = tokens.get(i + 1).and_then(|arg| parse_smb_set_arg(arg)) {
                folders.push(SharedFolder::smb(path));
            }
            i += 2;
            continue;
        }

        if tokens[i] != "-fsdev" {
            i += 1;
            continue;
        }

        let Some(fsdev_arg) = tokens.get(i + 1) else {
            break;
        };

        let mut mount_tag = None;
        let mut j = i + 2;
        while j + 1 < tokens.len() {
            if tokens[j] == "-fsdev" {
                break;
            }
            if tokens[j] == "-device" {
                mount_tag = extract_simple_value(&tokens[j + 1], "mount_tag=");
                break;
            }
            j += 1;
        }

        if let (Some(path), Some(tag)) = (extract_path_value(fsdev_arg), mount_tag) {
            folders.push(SharedFolder::nine_p(path, tag));
        }

        i += 2;
    }

    folders
}

/// Extract the shared path from a `netdev.<id>.smb=<path>` `-set` argument.
fn parse_smb_set_arg(arg: &str) -> Option<String> {
    let rest = arg.strip_prefix("netdev.")?;
    let (_, value) = rest.split_once(".smb=")?;
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_shell_words(line: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote = Quote::None;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Quote::None => match c {
                c if c.is_whitespace() => {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                '\'' => {
                    quote = Quote::Single;
                    in_word = true;
                }
                '"' => {
                    quote = Quote::Double;
                    in_word = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                        in_word = true;
                    }
                }
                _ => {
                    current.push(c);
                    in_word = true;
                }
            },
            Quote::Single => {
                if c == '\'' {
                    quote = Quote::None;
                } else {
                    current.push(c);
                }
            }
            Quote::Double => {
                if c == '"' {
                    quote = Quote::None;
                } else if c == '\\' {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                } else {
                    current.push(c);
                }
            }
        }
    }

    if in_word {
        words.push(current);
    }

    words
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
        // Unquoted path in the array format can contain spaces because the
        // whole fsdev option is one shell array element. It ends at the next
        // QEMU fsdev option key.
        let end = rest
            .find(",security_model=")
            .or_else(|| rest.find(",readonly="))
            .or_else(|| rest.find([',', '"']))
            .unwrap_or(rest.len());
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

// ── Window size launch override ─────────────────────────────────────────────
const WINDOW_SIZE_MARKER_START: &str = "# >>> Window size override (managed by vm-curator) >>>";
const WINDOW_SIZE_MARKER_END: &str = "# <<< Window size override (managed by vm-curator) <<<";
const VIDEO_ARGS_ARRAY_REF: &str = r#""${VM_CURATOR_VIDEO_ARGS[@]}""#;

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ',' | '='))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn strip_matching_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn parse_supported_video_arg_line(line: &str) -> Option<VideoOverrideSpec> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let command = trimmed.strip_suffix('\\').unwrap_or(trimmed).trim_end();

    if let Some(rest) = command.strip_prefix("-vga ") {
        let mut parts = rest.split_whitespace();
        let vga = strip_matching_quotes(parts.next()?);
        if parts.next().is_some() {
            return None;
        }

        let (default_value, override_device) = match vga {
            "std" => ("std", "VGA"),
            "virtio" => ("virtio", "virtio-vga"),
            "qxl" => ("qxl", "qxl-vga"),
            _ => return None,
        };

        return Some(VideoOverrideSpec {
            default_flag: "-vga",
            default_value,
            override_device,
        });
    }

    if let Some(rest) = command.strip_prefix("-device ") {
        let mut parts = rest.split_whitespace();
        let device = strip_matching_quotes(parts.next()?);
        if parts.next().is_some() {
            return None;
        }

        let (default_value, override_device) = match device {
            "VGA" => ("VGA", "VGA"),
            "virtio-vga" => ("virtio-vga", "virtio-vga"),
            "virtio-vga-gl" => ("virtio-vga-gl", "virtio-vga-gl"),
            "qxl-vga" => ("qxl-vga", "qxl-vga"),
            _ => return None,
        };

        return Some(VideoOverrideSpec {
            default_flag: "-device",
            default_value,
            override_device,
        });
    }

    None
}

fn window_size_setup_block(spec: &VideoOverrideSpec) -> String {
    format!(
        r#"{WINDOW_SIZE_MARKER_START}
VM_CURATOR_VIDEO_DEVICE={}
VM_CURATOR_VIDEO_ARGS={}
if [[ -n "${{VM_CURATOR_WINDOW_SIZE:-}}" && -n "$VM_CURATOR_VIDEO_DEVICE" ]]; then
    if [[ "$VM_CURATOR_WINDOW_SIZE" =~ ^([0-9]+)[xX]([0-9]+)$ ]]; then
        VM_CURATOR_WIDTH="${{BASH_REMATCH[1]}}"
        VM_CURATOR_HEIGHT="${{BASH_REMATCH[2]}}"
        if (( VM_CURATOR_WIDTH >= 320 && VM_CURATOR_WIDTH <= 16384 && VM_CURATOR_HEIGHT >= 200 && VM_CURATOR_HEIGHT <= 16384 )); then
            VM_CURATOR_VIDEO_ARGS=(-device "${{VM_CURATOR_VIDEO_DEVICE}},xres=${{VM_CURATOR_WIDTH}},yres=${{VM_CURATOR_HEIGHT}}")
        fi
    fi
fi
{WINDOW_SIZE_MARKER_END}"#,
        shell_quote(spec.override_device),
        shell_array_literal(spec)
    )
}

fn shell_array_literal(spec: &VideoOverrideSpec) -> String {
    format!(
        "({} {})",
        shell_quote(spec.default_flag),
        shell_quote(spec.default_value)
    )
}

fn push_video_args_reference_line(output: &mut String, line: &str) {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let has_continuation = line.trim_end().ends_with('\\');
    output.push_str(indent);
    output.push_str(VIDEO_ARGS_ARRAY_REF);
    if has_continuation {
        output.push_str(" \\");
    }
}

fn patch_window_size_override(content: &str) -> Option<String> {
    if content.contains("VM_CURATOR_VIDEO_ARGS") {
        return None;
    }

    let mut first_case_line = None;
    let mut first_qemu_line = None;
    let mut first_spec = None;
    let mut matching_line_indices = Vec::new();

    for (index, line) in content.lines().enumerate() {
        if first_case_line.is_none() && line.trim_start().starts_with("case ") {
            first_case_line = Some(index);
        }
        if first_qemu_line.is_none() && line.contains("qemu-system") {
            first_qemu_line = Some(index);
        }

        if let Some(spec) = parse_supported_video_arg_line(line) {
            if let Some(existing) = first_spec {
                if spec != existing {
                    return None;
                }
            } else {
                first_spec = Some(spec);
            }
            matching_line_indices.push(index);
        }
    }

    let first_spec = first_spec?;
    let insert_before = first_case_line.or(first_qemu_line)?;

    let setup_block = window_size_setup_block(&first_spec);
    let mut matching_line_indices = matching_line_indices.into_iter().peekable();
    let mut patched = String::with_capacity(content.len() + setup_block.len() + 1);
    let mut wrote_line = false;

    for (index, line) in content.lines().enumerate() {
        if index == insert_before {
            if wrote_line {
                patched.push('\n');
            }
            patched.push_str(&setup_block);
            wrote_line = true;
        }

        if wrote_line {
            patched.push('\n');
        }

        if matches!(matching_line_indices.peek(), Some(line_index) if *line_index == index) {
            matching_line_indices.next();
            push_video_args_reference_line(&mut patched, line);
        } else {
            patched.push_str(line);
        }
        wrote_line = true;
    }

    if content.ends_with('\n') {
        patched.push('\n');
    }
    Some(patched)
}

fn ensure_window_size_override_in_script(script_path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(script_path)
        .with_context(|| format!("Failed to read launch script: {}", script_path.display()))?;

    let Some(patched) = patch_window_size_override(&content) else {
        return Ok(());
    };

    std::fs::write(script_path, patched)
        .with_context(|| format!("Failed to write launch script: {}", script_path.display()))?;
    Ok(())
}

// ── QMP (QEMU Machine Protocol) ─────────────────────────────────────────────
//
// The items below are `#[allow(dead_code)]` infrastructure for planned
// runtime VM control (pause/resume, GUI embedding via D-Bus). They are wired up
// but not yet surfaced in the UI — intentional, not dead.

#[allow(dead_code)]
const QMP_ARG: &str = "        -qmp unix:\"$VM_DIR/qemu.sock\",server=on,wait=off";

/// Patch an existing launch.sh to include a QMP socket if not already present.
/// Idempotent — safe to call before every launch.
#[allow(dead_code)]
/// Make the VM's per-VM `OVMF_VARS` copy writable if it is not.
///
/// `launch.sh` creates that copy with `cp`, which preserves the template's
/// mode. When the template lives on a read-only store — NixOS's `/nix/store`
/// is mode 0444 — the copy is read-only too and QEMU aborts with
/// `Could not open '.../OVMF_VARS.fd': Permission denied`. Scripts generated
/// after this fix repair the copy themselves; this repairs VMs created before
/// it. A missing file (BIOS VM, or first launch) is not an error.
pub fn ensure_ovmf_vars_writable(vm_path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for name in ["OVMF_VARS.fd", "OVMF_VARS.qcow2"] {
        let path = vm_path.join(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mut perms = meta.permissions();
        if perms.mode() & 0o200 == 0 {
            perms.set_mode(perms.mode() | 0o200);
            std::fs::set_permissions(&path, perms)
                .with_context(|| format!("Failed to make {} writable", path.display()))?;
        }
    }
    Ok(())
}

pub fn ensure_qmp_in_script(vm_path: &Path) -> Result<()> {
    let script_path = vm_path.join("launch.sh");
    let content =
        std::fs::read_to_string(&script_path).context("Failed to read launch.sh for QMP patch")?;

    // Repair the unquoted socket path written by v1.0.0–v1.2.1 (issue #65):
    // without quotes, a library path containing spaces word-splits the -qmp arg.
    if content.contains("unix:$VM_DIR/qemu.sock") {
        let fixed = content.replace("unix:$VM_DIR/qemu.sock", "unix:\"$VM_DIR/qemu.sock\"");
        std::fs::write(&script_path, fixed).context("Failed to write repaired launch.sh")?;
        return Ok(());
    }

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

/// Give every QEMU invocation in an existing launch.sh a `-name "$VM_NAME"`
/// so the window is titled `QEMU (<vm-dir>)` (#96). Scripts generated after
/// this change already have it; this repairs older ones at launch.
///
/// Idempotent and conservative: a script that already passes `-name`
/// (ours or the user's own) is left alone, as is a hand-written script
/// without the `VM_DIR=` line we hang the `VM_NAME` definition on.
pub fn ensure_name_in_script(vm_path: &Path) -> Result<()> {
    let script_path = vm_path.join("launch.sh");
    let content = std::fs::read_to_string(&script_path)
        .context("Failed to read launch.sh for -name patch")?;
    if let Some(patched) = add_vm_name_to_script(&content) {
        std::fs::write(&script_path, patched).context("Failed to write patched launch.sh")?;
    }
    Ok(())
}

/// Pure half of [`ensure_name_in_script`]: `None` when nothing needs changing.
pub fn add_vm_name_to_script(content: &str) -> Option<String> {
    let has_name = content.lines().any(|l| {
        let t = l.trim_start();
        !t.starts_with('#') && (t.starts_with("-name ") || t.contains(" -name "))
    });
    if has_name {
        return None;
    }
    let vm_dir_line = content
        .lines()
        .position(|l| l.trim_start().starts_with("VM_DIR="))?;

    let lines: Vec<&str> = content.lines().collect();
    let mut out = String::with_capacity(content.len() + 128);
    let mut changed = false;

    for (i, line) in lines.iter().enumerate() {
        out.push_str(line);
        out.push('\n');

        if i == vm_dir_line {
            out.push_str("VM_NAME=\"$(basename \"$VM_DIR\")\"\n");
            continue;
        }

        // First line of a multi-line QEMU invocation: `qemu-system-* \`
        let t = line.trim();
        let is_qemu_start = !t.starts_with('#')
            && (t.starts_with("qemu-system-") || t.starts_with("exec qemu-system-"))
            && t.ends_with('\\');
        if is_qemu_start {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            out.push_str(&indent);
            out.push_str("-name \"$VM_NAME\" \\\n");
            changed = true;
        }
    }

    if changed {
        Some(out)
    } else {
        None
    }
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
