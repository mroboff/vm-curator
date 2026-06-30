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
    /// Parse USB version from a sysfs speed attribute value (in Mbps).
    ///
    /// Retained as tested public API; the `rusb` enumeration path now reads the
    /// link speed directly via [`rusb::Speed`], so this is no longer called by
    /// the binary itself.
    #[allow(dead_code)]
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
}

/// Enumerate USB devices using libusb (via the cross-platform `rusb` crate).
///
/// Works on Linux, macOS, and Windows. The QEMU passthrough arguments derived
/// from the returned devices use only vendor/product IDs, which are independent
/// of the host OS.
pub fn enumerate_usb_devices() -> Result<Vec<UsbDevice>> {
    let device_list = rusb::devices().context("Failed to enumerate USB devices via libusb")?;

    let mut devices = Vec::new();

    for device in device_list.iter() {
        let descriptor = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        let vendor_id = descriptor.vendor_id();
        let product_id = descriptor.product_id();

        // Skip Linux virtual root hubs (vendor 0x1d6b); harmless on other OSes.
        if vendor_id == 0x1d6b {
            continue;
        }

        let device_class = descriptor.class_code();
        let usb_version = usb_version_from_device(&device, &descriptor);
        let (vendor_name, product_name) = read_device_strings(&device, &descriptor);

        devices.push(UsbDevice {
            vendor_id,
            product_id,
            vendor_name,
            product_name,
            bus_num: device.bus_number(),
            dev_num: device.address(),
            device_class,
            usb_version,
        });
    }

    // Filter out hubs and root hubs.
    devices.retain(|d| !d.is_hub());

    Ok(devices)
}

/// Determine the USB version from the negotiated link speed, falling back to the
/// `bcdUSB` field of the device descriptor when the speed is unknown.
fn usb_version_from_device(
    device: &rusb::Device<rusb::GlobalContext>,
    descriptor: &rusb::DeviceDescriptor,
) -> UsbVersion {
    use rusb::Speed;

    match device.speed() {
        Speed::Low | Speed::Full => UsbVersion::Usb1,
        Speed::High => UsbVersion::Usb2,
        Speed::Super | Speed::SuperPlus => UsbVersion::Usb3,
        _ => {
            // bcdUSB is the USB protocol version (e.g. 0x0300 for USB 3.0).
            let v = descriptor.usb_version();
            let bcd = ((v.major() as u16) << 8)
                | (((v.minor() & 0x0f) as u16) << 4)
                | (v.sub_minor() & 0x0f) as u16;
            UsbVersion::from_bcd_usb(bcd)
        }
    }
}

/// Read the manufacturer and product string descriptors.
///
/// Reading string descriptors requires opening the device, which can fail
/// without sufficient privileges (common on macOS). On failure we return empty
/// strings; [`UsbDevice::display_name`] then degrades to the `VID:PID` form.
fn read_device_strings(
    device: &rusb::Device<rusb::GlobalContext>,
    descriptor: &rusb::DeviceDescriptor,
) -> (String, String) {
    let handle = match device.open() {
        Ok(h) => h,
        Err(_) => return (String::new(), String::new()),
    };

    let timeout = std::time::Duration::from_millis(100);
    let lang = match handle.read_languages(timeout) {
        Ok(langs) => match langs.first().copied() {
            Some(lang) => lang,
            None => return (String::new(), String::new()),
        },
        Err(_) => return (String::new(), String::new()),
    };

    let vendor_name = handle
        .read_manufacturer_string(lang, descriptor, timeout)
        .unwrap_or_default()
        .trim()
        .to_string();
    let product_name = handle
        .read_product_string(lang, descriptor, timeout)
        .unwrap_or_default()
        .trim()
        .to_string();

    (vendor_name, product_name)
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
    rules.push_str(
        "# These rules allow non-root users to access USB devices for VM passthrough\n\n",
    );

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
        None => {
            UdevInstallResult::Error("No suitable privilege escalation method found".to_string())
        }
    }
}

fn try_pkexec_install(temp_path: &str, rules_path: &str) -> Option<bool> {
    use std::process::Command;

    // Check if pkexec is available
    if Command::new("which")
        .arg("pkexec")
        .output()
        .ok()?
        .status
        .success()
    {
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
    if !Command::new("which")
        .arg("sudo")
        .output()
        .ok()?
        .status
        .success()
    {
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
    if let Ok(status) = Command::new("sudo").args(["sh", "-c", reload_cmd]).status() {
        return status.success();
    }

    false
}

#[cfg(test)]
#[path = "tests/usb.rs"]
mod tests;
