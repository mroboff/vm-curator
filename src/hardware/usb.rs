use anyhow::{Context, Result};

/// USB version/speed classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsbVersion {
    /// USB 1.0/1.1 (Low/Full speed - 1.5/12 Mbps)
    Usb1,
    /// USB 2.0 (High speed - 480 Mbps)
    #[default]
    Usb2,
    /// USB 3.0+ (SuperSpeed - 5/10/20 Gbps)
    Usb3,
}

impl UsbVersion {
    /// Parse USB version from sysfs speed attribute value (in Mbps)
    pub fn from_speed(speed: &str) -> Self {
        match speed.trim() {
            "1.5" | "12" => UsbVersion::Usb1,
            "480" => UsbVersion::Usb2,
            "5000" | "10000" | "20000" => UsbVersion::Usb3,
            _ => UsbVersion::Usb2, // Default to USB 2.0 for unknown speeds
        }
    }

    /// Parse USB version from bcdUSB attribute (e.g., "0300" for USB 3.0)
    pub fn from_bcd_usb(bcd: u16) -> Self {
        if bcd >= 0x0300 {
            UsbVersion::Usb3
        } else if bcd >= 0x0200 {
            UsbVersion::Usb2
        } else {
            UsbVersion::Usb1
        }
    }

    /// Check if this is USB 3.0 or higher
    pub fn is_usb3(&self) -> bool {
        matches!(self, UsbVersion::Usb3)
    }
}

/// Represents a USB device
#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub vendor_name: String,
    pub product_name: String,
    /// Bus number - reserved for future bus-specific passthrough
    #[allow(dead_code)]
    pub bus_num: u8,
    /// Device number - reserved for future bus-specific passthrough
    #[allow(dead_code)]
    pub dev_num: u8,
    pub device_class: u8,
    /// USB version/speed classification
    pub usb_version: UsbVersion,
}

impl UsbDevice {
    /// Check if this is a hub device
    pub fn is_hub(&self) -> bool {
        // USB Hub class is 0x09
        self.device_class == 0x09
    }

    /// Get a display string for this device
    pub fn display_name(&self) -> String {
        if !self.product_name.is_empty() {
            if !self.vendor_name.is_empty() {
                format!("{} {}", self.vendor_name, self.product_name)
            } else {
                self.product_name.clone()
            }
        } else {
            format!("USB Device {:04x}:{:04x}", self.vendor_id, self.product_id)
        }
    }

    /// Generate QEMU passthrough arguments
    #[allow(dead_code)]
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

/// Enumerate USB devices using libudev
pub fn enumerate_usb_devices() -> Result<Vec<UsbDevice>> {
    // Try using libudev, fall back to sysfs
    let mut devices = match enumerate_via_udev() {
        Ok(devs) => devs,
        Err(e) => {
            // Log the fallback for debugging purposes
            eprintln!("vm-curator: libudev enumeration failed ({}), falling back to sysfs", e);
            enumerate_via_sysfs().unwrap_or_default()
        }
    };

    // Filter out hubs and root hubs
    devices.retain(|d| !d.is_hub());

    Ok(devices)
}

/// Enumerate using libudev
fn enumerate_via_udev() -> Result<Vec<UsbDevice>> {
    use libudev::Context;

    let context = Context::new().context("Failed to create udev context")?;
    let mut enumerator = libudev::Enumerator::new(&context)
        .context("Failed to create udev enumerator")?;

    enumerator.match_subsystem("usb")
        .context("Failed to match USB subsystem")?;

    let mut devices = Vec::new();

    for device in enumerator.scan_devices()? {
        // Only process USB devices (not interfaces)
        if device.devtype().map(|t| t == "usb_device").unwrap_or(false) {
            let vendor_id = device
                .attribute_value("idVendor")
                .and_then(|v| v.to_str())
                .and_then(|s| u16::from_str_radix(s, 16).ok())
                .unwrap_or(0);

            let product_id = device
                .attribute_value("idProduct")
                .and_then(|v| v.to_str())
                .and_then(|s| u16::from_str_radix(s, 16).ok())
                .unwrap_or(0);

            // Skip root hubs (usually vendor 0x1d6b)
            if vendor_id == 0x1d6b {
                continue;
            }

            let vendor_name = device
                .attribute_value("manufacturer")
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_string();

            let product_name = device
                .attribute_value("product")
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_string();

            let bus_num = device
                .attribute_value("busnum")
                .and_then(|v| v.to_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            let dev_num = device
                .attribute_value("devnum")
                .and_then(|v| v.to_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            let device_class = device
                .attribute_value("bDeviceClass")
                .and_then(|v| v.to_str())
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .unwrap_or(0);

            // Detect USB version from speed attribute first, fall back to bcdUSB
            let usb_version = device
                .attribute_value("speed")
                .and_then(|v| v.to_str())
                .map(UsbVersion::from_speed)
                .unwrap_or_else(|| {
                    // Fall back to bcdUSB attribute (USB protocol version, not bcdDevice which is firmware version)
                    device
                        .attribute_value("bcdUSB")
                        .and_then(|v| v.to_str())
                        .and_then(|s| u16::from_str_radix(s, 16).ok())
                        .map(UsbVersion::from_bcd_usb)
                        .unwrap_or_default()
                });

            devices.push(UsbDevice {
                vendor_id,
                product_id,
                vendor_name,
                product_name,
                bus_num,
                dev_num,
                device_class,
                usb_version,
            });
        }
    }

    Ok(devices)
}

/// Fallback enumeration via /sys/bus/usb/devices
fn enumerate_via_sysfs() -> Result<Vec<UsbDevice>> {
    let mut devices = Vec::new();
    let sysfs_path = std::path::Path::new("/sys/bus/usb/devices");

    if !sysfs_path.exists() {
        return Ok(devices);
    }

    for entry in std::fs::read_dir(sysfs_path)? {
        let entry = entry?;
        let path = entry.path();

        // Skip entries that look like interfaces (contain ':')
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.contains(':') {
            continue;
        }

        // Try to read device attributes
        let vendor_id = read_sysfs_hex(&path, "idVendor").unwrap_or(0);
        let product_id = read_sysfs_hex(&path, "idProduct").unwrap_or(0);

        // Skip if no valid IDs
        if vendor_id == 0 && product_id == 0 {
            continue;
        }

        // Skip root hubs
        if vendor_id == 0x1d6b {
            continue;
        }

        let vendor_name = read_sysfs_string(&path, "manufacturer").unwrap_or_default();
        let product_name = read_sysfs_string(&path, "product").unwrap_or_default();
        let bus_num = read_sysfs_decimal(&path, "busnum").unwrap_or(0) as u8;
        let dev_num = read_sysfs_decimal(&path, "devnum").unwrap_or(0) as u8;
        let device_class = read_sysfs_hex(&path, "bDeviceClass").unwrap_or(0) as u8;

        // Detect USB version from speed attribute first, fall back to bcdUSB
        let usb_version = read_sysfs_string(&path, "speed")
            .map(|s| UsbVersion::from_speed(&s))
            .unwrap_or_else(|| {
                // Fall back to bcdUSB (USB protocol version, not bcdDevice which is firmware version)
                read_sysfs_hex(&path, "bcdUSB")
                    .map(UsbVersion::from_bcd_usb)
                    .unwrap_or_default()
            });

        devices.push(UsbDevice {
            vendor_id,
            product_id,
            vendor_name,
            product_name,
            bus_num,
            dev_num,
            device_class,
            usb_version,
        });
    }

    Ok(devices)
}

fn read_sysfs_hex(path: &std::path::Path, attr: &str) -> Option<u16> {
    let value = std::fs::read_to_string(path.join(attr)).ok()?;
    u16::from_str_radix(value.trim(), 16).ok()
}

fn read_sysfs_decimal(path: &std::path::Path, attr: &str) -> Option<u32> {
    let value = std::fs::read_to_string(path.join(attr)).ok()?;
    value.trim().parse().ok()
}

fn read_sysfs_string(path: &std::path::Path, attr: &str) -> Option<String> {
    std::fs::read_to_string(path.join(attr))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Result of udev rule installation
#[derive(Debug)]
pub enum UdevInstallResult {
    Success,
    NeedsReboot,
    PermissionDenied,
    Error(String),
}

/// Generate udev rules content for USB passthrough
pub fn generate_udev_rules(devices: &[UsbDevice]) -> String {
    let mut rules = String::new();
    rules.push_str("# USB Passthrough rules for QEMU (managed by vm-curator)\n");
    rules.push_str("# These rules allow non-root users to access USB devices for VM passthrough\n\n");

    // Collect unique vendor IDs to avoid duplicate rules
    let mut seen_vendors = std::collections::HashSet::new();

    for device in devices {
        if seen_vendors.insert(device.vendor_id) {
            rules.push_str(&format!(
                "# {} devices\n",
                if device.vendor_name.is_empty() {
                    format!("Vendor {:04x}", device.vendor_id)
                } else {
                    device.vendor_name.clone()
                }
            ));
            rules.push_str(&format!(
                "SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{:04x}\", MODE=\"0666\"\n\n",
                device.vendor_id
            ));
        }
    }

    rules
}

/// Install udev rules for USB passthrough
/// Uses pkexec (graphical sudo) if available, falls back to sudo
pub fn install_udev_rules(devices: &[UsbDevice]) -> UdevInstallResult {
    use std::os::unix::fs::PermissionsExt;

    if devices.is_empty() {
        return UdevInstallResult::Error("No devices selected".to_string());
    }

    let rules_content = generate_udev_rules(devices);
    let rules_path = "/etc/udev/rules.d/99-vm-curator-usb.rules";

    // Write rules to a temporary file with unique name (pid + timestamp)
    let temp_path = format!(
        "/tmp/vm-curator-usb-rules-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    // Create file with restrictive permissions (0600) before writing content
    let file_result = std::fs::File::create(&temp_path)
        .and_then(|file| {
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&temp_path, perms)?;
            Ok(())
        })
        .and_then(|_| std::fs::write(&temp_path, &rules_content));

    if let Err(e) = file_result {
        let _ = std::fs::remove_file(&temp_path);
        return UdevInstallResult::Error(format!("Failed to write temp file: {}", e));
    }

    // Try pkexec first (graphical sudo prompt), then fall back to sudo
    let install_result = try_pkexec_install(&temp_path, rules_path)
        .or_else(|| try_sudo_install(&temp_path, rules_path));

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

    match install_result {
        Some(true) => {
            // Reload udev rules
            let reload_result = reload_udev_rules();
            if reload_result {
                UdevInstallResult::Success
            } else {
                UdevInstallResult::NeedsReboot
            }
        }
        Some(false) => UdevInstallResult::PermissionDenied,
        None => UdevInstallResult::Error("No suitable privilege escalation method found".to_string()),
    }
}

fn try_pkexec_install(temp_path: &str, rules_path: &str) -> Option<bool> {
    use std::process::Command;

    // Check if pkexec is available
    if Command::new("which").arg("pkexec").output().ok()?.status.success() {
        // Use pkexec to copy the file
        let status = Command::new("pkexec")
            .args(["cp", temp_path, rules_path])
            .status()
            .ok()?;

        Some(status.success())
    } else {
        None
    }
}

fn try_sudo_install(temp_path: &str, rules_path: &str) -> Option<bool> {
    use std::process::{Command, Stdio};

    // Check if sudo is available
    if !Command::new("which").arg("sudo").output().ok()?.status.success() {
        return None;
    }

    // Use sudo with -A flag to use SSH_ASKPASS or similar for password prompt
    // If that fails, try regular sudo which will use the terminal
    let status = Command::new("sudo")
        .args(["cp", temp_path, rules_path])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .ok()?;

    Some(status.success())
}

fn reload_udev_rules() -> bool {
    use std::process::Command;

    // Try to reload udev rules using pkexec or sudo
    let reload_cmd = "udevadm control --reload-rules && udevadm trigger";

    // Try pkexec first
    if let Ok(status) = Command::new("pkexec")
        .args(["sh", "-c", reload_cmd])
        .status()
    {
        if status.success() {
            return true;
        }
    }

    // Fall back to sudo
    if let Ok(status) = Command::new("sudo")
        .args(["sh", "-c", reload_cmd])
        .status()
    {
        return status.success();
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_device_display() {
        let device = UsbDevice {
            vendor_id: 0x046d,
            product_id: 0xc077,
            vendor_name: "Logitech".to_string(),
            product_name: "M105 Mouse".to_string(),
            bus_num: 1,
            dev_num: 3,
            device_class: 0,
            usb_version: UsbVersion::Usb2,
        };

        assert_eq!(device.display_name(), "Logitech M105 Mouse");
        assert!(!device.is_hub());
    }

    #[test]
    fn test_qemu_args() {
        let device = UsbDevice {
            vendor_id: 0x046d,
            product_id: 0xc077,
            vendor_name: "Logitech".to_string(),
            product_name: "M105 Mouse".to_string(),
            bus_num: 1,
            dev_num: 3,
            device_class: 0,
            usb_version: UsbVersion::Usb2,
        };

        let args = device.to_qemu_args();
        assert_eq!(args[0], "-device");
        assert!(args[1].contains("vendorid=0x046d"));
        assert!(args[1].contains("productid=0xc077"));
    }

    #[test]
    fn test_usb_version_from_speed() {
        assert_eq!(UsbVersion::from_speed("1.5"), UsbVersion::Usb1);
        assert_eq!(UsbVersion::from_speed("12"), UsbVersion::Usb1);
        assert_eq!(UsbVersion::from_speed("480"), UsbVersion::Usb2);
        assert_eq!(UsbVersion::from_speed("5000"), UsbVersion::Usb3);
        assert_eq!(UsbVersion::from_speed("10000"), UsbVersion::Usb3);
        assert_eq!(UsbVersion::from_speed("20000"), UsbVersion::Usb3);
        // Unknown speed defaults to USB 2.0
        assert_eq!(UsbVersion::from_speed("unknown"), UsbVersion::Usb2);
    }

    #[test]
    fn test_usb_version_from_bcd() {
        assert_eq!(UsbVersion::from_bcd_usb(0x0100), UsbVersion::Usb1);
        assert_eq!(UsbVersion::from_bcd_usb(0x0110), UsbVersion::Usb1);
        assert_eq!(UsbVersion::from_bcd_usb(0x0200), UsbVersion::Usb2);
        assert_eq!(UsbVersion::from_bcd_usb(0x0210), UsbVersion::Usb2);
        assert_eq!(UsbVersion::from_bcd_usb(0x0300), UsbVersion::Usb3);
        assert_eq!(UsbVersion::from_bcd_usb(0x0310), UsbVersion::Usb3);
        assert_eq!(UsbVersion::from_bcd_usb(0x0320), UsbVersion::Usb3);
    }

    #[test]
    fn test_usb_version_is_usb3() {
        assert!(!UsbVersion::Usb1.is_usb3());
        assert!(!UsbVersion::Usb2.is_usb3());
        assert!(UsbVersion::Usb3.is_usb3());
    }
}
