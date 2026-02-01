//! PCI Device Enumeration and GPU Passthrough Support
//!
//! This module handles PCI device discovery, GPU detection, and VFIO passthrough
//! configuration for GPU passthrough scenarios with Looking Glass support.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// PCI device class codes
pub mod class_codes {
    /// VGA-compatible controller
    pub const VGA_COMPATIBLE: u32 = 0x030000;
    /// 3D controller (non-VGA)
    pub const CONTROLLER_3D: u32 = 0x030200;
    /// Display controller
    pub const DISPLAY_CONTROLLER: u32 = 0x038000;
    /// Audio device
    pub const AUDIO_DEVICE: u32 = 0x040300;
    /// USB controller
    pub const USB_CONTROLLER: u32 = 0x0c0300;
    /// Network controller (Ethernet)
    pub const NETWORK_CONTROLLER: u32 = 0x020000;
    /// NVMe controller
    pub const NVME_CONTROLLER: u32 = 0x010802;
    /// SATA controller
    pub const SATA_CONTROLLER: u32 = 0x010600;
    /// Serial controller
    pub const SERIAL_CONTROLLER: u32 = 0x070000;

    // Infrastructure devices (typically not useful for passthrough)
    /// Host bridge
    pub const HOST_BRIDGE: u32 = 0x060000;
    /// ISA bridge
    pub const ISA_BRIDGE: u32 = 0x060100;
    /// PCI bridge
    pub const PCI_BRIDGE: u32 = 0x060400;
    /// PCI-to-PCI bridge (subtractive decode)
    pub const PCI_BRIDGE_SUB: u32 = 0x060401;
    /// Other bridge
    pub const OTHER_BRIDGE: u32 = 0x068000;
    /// SMBus controller
    pub const SMBUS: u32 = 0x0c0500;
    /// Signal processing controller
    pub const SIGNAL_PROC: u32 = 0x118000;
    /// Encryption controller
    pub const ENCRYPTION: u32 = 0x108000;
    /// Non-essential instrumentation
    pub const INSTRUMENTATION: u32 = 0x130000;
}

/// Represents a PCI device discovered on the system
#[derive(Debug, Clone)]
pub struct PciDevice {
    /// PCI address in format "0000:01:00.0"
    pub address: String,
    /// Vendor ID (e.g., 0x10de for NVIDIA)
    pub vendor_id: u16,
    /// Device ID (reserved for future use with PCI ID database lookup)
    #[allow(dead_code)]
    pub device_id: u16,
    /// PCI class code (e.g., 0x030000 for VGA)
    pub class_code: u32,
    /// Human-readable vendor name
    pub vendor_name: String,
    /// Human-readable device name
    pub device_name: String,
    /// Current kernel driver (e.g., "nvidia", "vfio-pci")
    pub driver: Option<String>,
    /// IOMMU group number
    pub iommu_group: Option<u32>,
    /// Whether this is the boot VGA device
    pub is_boot_vga: bool,
    /// Subsystem vendor ID (reserved for future use)
    #[allow(dead_code)]
    pub subsystem_vendor_id: u16,
    /// Subsystem device ID (reserved for future use)
    #[allow(dead_code)]
    pub subsystem_device_id: u16,
}

impl PciDevice {
    /// Check if this is a GPU device (VGA, 3D controller, or display controller)
    pub fn is_gpu(&self) -> bool {
        let class_base = self.class_code & 0xFFFF00;
        class_base == class_codes::VGA_COMPATIBLE
            || class_base == class_codes::CONTROLLER_3D
            || class_base == class_codes::DISPLAY_CONTROLLER
    }

    /// Check if this is a VGA-compatible controller
    pub fn is_vga(&self) -> bool {
        (self.class_code & 0xFFFF00) == class_codes::VGA_COMPATIBLE
    }

    /// Check if this is an audio device (likely GPU audio companion)
    pub fn is_audio(&self) -> bool {
        (self.class_code & 0xFFFF00) == class_codes::AUDIO_DEVICE
    }

    /// Check if this is a USB controller
    pub fn is_usb_controller(&self) -> bool {
        (self.class_code & 0xFFFF00) == class_codes::USB_CONTROLLER
    }

    /// Check if this is a network controller
    pub fn is_network_controller(&self) -> bool {
        (self.class_code & 0xFFFF00) == class_codes::NETWORK_CONTROLLER
    }

    /// Check if this is a storage controller (NVMe, SATA)
    pub fn is_storage_controller(&self) -> bool {
        let class_base = self.class_code & 0xFFFF00;
        class_base == class_codes::NVME_CONTROLLER
            || class_base == class_codes::SATA_CONTROLLER
            || (self.class_code & 0xFF0000) == 0x010000 // Mass storage controller class
    }

    /// Check if this is an infrastructure device (bridge, SMBus, etc.)
    pub fn is_infrastructure(&self) -> bool {
        let class_base = self.class_code & 0xFFFF00;
        let class_main = self.class_code & 0xFF0000;
        // Bridge devices (class 06)
        class_main == 0x060000
            // SMBus
            || class_base == class_codes::SMBUS
            // Signal processing
            || class_base == class_codes::SIGNAL_PROC
            // Encryption controller (typically platform security)
            || class_base == class_codes::ENCRYPTION
            // Non-essential instrumentation
            || class_base == class_codes::INSTRUMENTATION
    }

    /// Check if this device is a good candidate for non-GPU passthrough
    /// (USB controllers, network cards, storage controllers, audio devices)
    pub fn is_passthrough_candidate(&self) -> bool {
        !self.is_infrastructure()
            && !self.is_gpu()
            && self.iommu_group.is_some()
            && (self.is_usb_controller()
                || self.is_network_controller()
                || self.is_storage_controller()
                || self.is_audio())
    }

    /// Check if this is an NVIDIA device
    pub fn is_nvidia(&self) -> bool {
        self.vendor_id == 0x10de
    }

    /// Check if this is an AMD/ATI device
    pub fn is_amd(&self) -> bool {
        self.vendor_id == 0x1002
    }

    /// Check if this is an Intel device
    pub fn is_intel(&self) -> bool {
        self.vendor_id == 0x8086
    }

    /// Get a display string for this device
    #[allow(dead_code)]
    pub fn display_name(&self) -> String {
        if !self.device_name.is_empty() {
            if !self.vendor_name.is_empty() {
                format!("{} {}", self.vendor_name, self.device_name)
            } else {
                self.device_name.clone()
            }
        } else {
            format!(
                "PCI Device {:04x}:{:04x}",
                self.vendor_id, self.device_id
            )
        }
    }

    /// Get short vendor name
    pub fn short_vendor(&self) -> &str {
        if self.is_nvidia() {
            "NVIDIA"
        } else if self.is_amd() {
            "AMD"
        } else if self.is_intel() {
            "Intel"
        } else if !self.vendor_name.is_empty() {
            &self.vendor_name
        } else {
            "Unknown"
        }
    }

    /// Get the device class description
    pub fn class_description(&self) -> &str {
        let class_base = self.class_code & 0xFFFF00;
        match class_base {
            class_codes::VGA_COMPATIBLE => "VGA Controller",
            class_codes::CONTROLLER_3D => "3D Controller",
            class_codes::DISPLAY_CONTROLLER => "Display Controller",
            class_codes::AUDIO_DEVICE => "Audio Device",
            class_codes::USB_CONTROLLER => "USB Controller",
            class_codes::NETWORK_CONTROLLER => "Network Controller",
            class_codes::NVME_CONTROLLER => "NVMe Controller",
            class_codes::SATA_CONTROLLER => "SATA Controller",
            class_codes::SERIAL_CONTROLLER => "Serial Controller",
            class_codes::HOST_BRIDGE => "Host Bridge",
            class_codes::ISA_BRIDGE => "ISA Bridge",
            class_codes::PCI_BRIDGE | class_codes::PCI_BRIDGE_SUB => "PCI Bridge",
            class_codes::OTHER_BRIDGE => "Bridge Device",
            class_codes::SMBUS => "SMBus Controller",
            class_codes::SIGNAL_PROC => "Signal Processor",
            class_codes::ENCRYPTION => "Encryption Controller",
            _ => "PCI Device",
        }
    }

    /// Check if the device is bound to vfio-pci driver
    pub fn is_vfio_bound(&self) -> bool {
        self.driver
            .as_ref()
            .map(|d| d == "vfio-pci")
            .unwrap_or(false)
    }

    /// Check if passthrough is possible (not boot VGA, has IOMMU group)
    #[allow(dead_code)]
    pub fn can_passthrough(&self) -> bool {
        !self.is_boot_vga && self.iommu_group.is_some()
    }

    /// Check if this device can be used for single GPU passthrough
    /// Returns true for boot VGA devices with IOMMU group (requires special handling)
    pub fn can_single_gpu_passthrough(&self) -> bool {
        self.is_boot_vga && self.is_gpu() && self.iommu_group.is_some()
    }

    /// Generate QEMU vfio-pci passthrough arguments
    ///
    /// Note: x-vga=on is NOT used because it's incompatible with modern NVIDIA GPUs
    /// (RTX 4xxx series and newer). The GPU will output to its physical display ports
    /// without needing this legacy VGA compatibility flag.
    pub fn to_qemu_args(&self, is_primary_gpu: bool) -> Vec<String> {
        let mut args = vec!["-device".to_string()];

        let mut device_str = format!("vfio-pci,host={}", self.address);

        // For GPUs with companion audio devices (same IOMMU group), use multifunction
        // to present them as a single multi-function device
        if is_primary_gpu && self.is_gpu() {
            device_str.push_str(",multifunction=on");
        }

        args.push(device_str);
        args
    }
}

/// Status of multi-GPU passthrough prerequisites
#[derive(Debug, Clone)]
pub struct MultiGpuPassthroughStatus {
    /// IOMMU is enabled in kernel
    pub iommu_enabled: bool,
    /// VFIO modules are loaded
    pub vfio_loaded: bool,
    /// Number of available GPUs (excluding boot VGA)
    pub available_gpus: usize,
    /// List of GPUs that can be passed through
    pub passthrough_gpus: Vec<PciDevice>,
    /// Boot VGA device (cannot be passed through)
    pub boot_vga: Option<PciDevice>,
    /// Error messages for any failed checks
    pub errors: Vec<String>,
    /// Warning messages
    pub warnings: Vec<String>,
}

impl MultiGpuPassthroughStatus {
    /// Check if all prerequisites are met for GPU passthrough
    pub fn is_ready(&self) -> bool {
        self.iommu_enabled && self.vfio_loaded && !self.passthrough_gpus.is_empty()
    }

    /// Get a summary status message
    pub fn summary(&self) -> String {
        if self.is_ready() {
            format!(
                "Ready ({} GPU{} available)",
                self.passthrough_gpus.len(),
                if self.passthrough_gpus.len() == 1 { "" } else { "s" }
            )
        } else {
            let mut issues = Vec::new();
            if !self.iommu_enabled {
                issues.push("IOMMU disabled");
            }
            if !self.vfio_loaded {
                issues.push("VFIO not loaded");
            }
            if self.passthrough_gpus.is_empty() {
                issues.push("No passthrough GPUs");
            }
            format!("Not ready: {}", issues.join(", "))
        }
    }
}

/// Enumerate all PCI devices on the system
pub fn enumerate_pci_devices() -> Result<Vec<PciDevice>> {
    let pci_path = Path::new("/sys/bus/pci/devices");

    if !pci_path.exists() {
        return Ok(Vec::new());
    }

    let mut devices = Vec::new();

    for entry in fs::read_dir(pci_path)? {
        let entry = entry?;
        let path = entry.path();
        let address = entry.file_name().to_string_lossy().to_string();

        if let Ok(device) = read_pci_device(&path, &address) {
            devices.push(device);
        }
    }

    // Sort by address for consistent display
    devices.sort_by(|a, b| a.address.cmp(&b.address));

    Ok(devices)
}

/// Read a single PCI device from sysfs
fn read_pci_device(path: &Path, address: &str) -> Result<PciDevice> {
    let vendor_id = read_sysfs_hex_u16(path, "vendor")?;
    let device_id = read_sysfs_hex_u16(path, "device")?;
    let class_code = read_sysfs_hex_u32(path, "class")?;
    let subsystem_vendor_id = read_sysfs_hex_u16(path, "subsystem_vendor").unwrap_or(0);
    let subsystem_device_id = read_sysfs_hex_u16(path, "subsystem_device").unwrap_or(0);

    // Get driver binding
    let driver = read_driver_binding(path);

    // Get IOMMU group
    let iommu_group = read_iommu_group(path);

    // Check if boot VGA
    let is_boot_vga = read_sysfs_bool(path, "boot_vga");

    // Look up device names from PCI IDs database or use basic descriptions
    let (vendor_name, device_name) = get_pci_names(vendor_id, device_id, class_code);

    Ok(PciDevice {
        address: address.to_string(),
        vendor_id,
        device_id,
        class_code,
        vendor_name,
        device_name,
        driver,
        iommu_group,
        is_boot_vga,
        subsystem_vendor_id,
        subsystem_device_id,
    })
}

/// Read hex value from sysfs as u16
fn read_sysfs_hex_u16(path: &Path, attr: &str) -> Result<u16> {
    let value = fs::read_to_string(path.join(attr))
        .with_context(|| format!("Failed to read {}", attr))?;
    let value = value.trim().trim_start_matches("0x");
    u16::from_str_radix(value, 16).context("Failed to parse hex value")
}

/// Read hex value from sysfs as u32
fn read_sysfs_hex_u32(path: &Path, attr: &str) -> Result<u32> {
    let value = fs::read_to_string(path.join(attr))
        .with_context(|| format!("Failed to read {}", attr))?;
    let value = value.trim().trim_start_matches("0x");
    u32::from_str_radix(value, 16).context("Failed to parse hex value")
}

/// Read boolean from sysfs (1 = true, 0 = false)
fn read_sysfs_bool(path: &Path, attr: &str) -> bool {
    fs::read_to_string(path.join(attr))
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// Read driver binding from sysfs
fn read_driver_binding(path: &Path) -> Option<String> {
    let driver_link = path.join("driver");
    if driver_link.exists() {
        fs::read_link(&driver_link)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
    } else {
        None
    }
}

/// Read IOMMU group from sysfs
fn read_iommu_group(path: &Path) -> Option<u32> {
    let iommu_link = path.join("iommu_group");
    if iommu_link.exists() {
        fs::read_link(&iommu_link)
            .ok()
            .and_then(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.parse().ok())
            })
    } else {
        None
    }
}

/// Get PCI vendor and device names
fn get_pci_names(vendor_id: u16, device_id: u16, class_code: u32) -> (String, String) {
    // Known vendors
    let vendor_name = match vendor_id {
        0x10de => "NVIDIA".to_string(),
        0x1002 => "AMD/ATI".to_string(),
        0x8086 => "Intel".to_string(),
        0x1022 => "AMD".to_string(),
        0x14e4 => "Broadcom".to_string(),
        0x10ec => "Realtek".to_string(),
        0x1b4b => "Marvell".to_string(),
        0x144d => "Samsung".to_string(),
        0x15b7 => "SanDisk".to_string(),
        0x1987 => "Phison".to_string(),
        0x1c5c => "SK hynix".to_string(),
        0x126f => "Silicon Motion".to_string(),
        0x1e0f => "KIOXIA".to_string(),
        _ => String::new(),
    };

    // Try to get device name from specific known devices or use class description
    let device_name = get_known_device_name(vendor_id, device_id)
        .unwrap_or_else(|| get_class_description(class_code).to_string());

    (vendor_name, device_name)
}

/// Get known device names for common devices
fn get_known_device_name(vendor_id: u16, device_id: u16) -> Option<String> {
    // Common NVIDIA GPUs
    if vendor_id == 0x10de {
        let name = match device_id {
            // RTX 50 series (Blackwell)
            0x2B84 => "GeForce RTX 5090",
            0x2B85 => "GeForce RTX 5080",
            0x2B86 => "GeForce RTX 5070 Ti",
            0x2B87 => "GeForce RTX 5070",
            0x2B88 => "GeForce RTX 5060 Ti",
            0x2B89 => "GeForce RTX 5060",
            // RTX 40 series
            0x2684 => "GeForce RTX 4090",
            0x2702 => "GeForce RTX 4080 SUPER",
            0x2704 => "GeForce RTX 4080",
            0x2705 => "GeForce RTX 4070 Ti SUPER",
            0x2782 => "GeForce RTX 4070 Ti",
            0x2783 => "GeForce RTX 4070 SUPER",
            0x2786 => "GeForce RTX 4070",
            0x2860 => "GeForce RTX 4060",
            0x2882 => "GeForce RTX 4060 Ti",
            // RTX 30 series
            0x2204 => "GeForce RTX 3090",
            0x2206 => "GeForce RTX 3080",
            0x2208 => "GeForce RTX 3080 Ti",
            0x2484 => "GeForce RTX 3070",
            0x2488 => "GeForce RTX 3070 Ti",
            0x2504 => "GeForce RTX 3060 Ti",
            0x2544 => "GeForce RTX 3060",
            // RTX 20 series (Turing)
            0x1E04 => "GeForce RTX 2080 Ti",
            0x1E82 => "GeForce RTX 2080",
            0x1E84 => "GeForce RTX 2070 SUPER",
            0x1F02 => "GeForce RTX 2070",
            0x1F07 => "GeForce RTX 2060 SUPER",
            0x1F08 => "GeForce RTX 2060",
            // Audio devices
            0x10f9 | 0x10f8 | 0x10f7 | 0x228b | 0x22bd => "HD Audio Controller",
            _ => return None,
        };
        return Some(name.to_string());
    }

    // Common AMD GPUs
    if vendor_id == 0x1002 {
        let name = match device_id {
            // RX 7000 series
            0x744c => "Radeon RX 7900 XTX",
            0x7448 => "Radeon RX 7900 XT",
            0x747e => "Radeon RX 7900 GRE",
            0x7470 => "Radeon RX 7800 XT",
            0x7480 => "Radeon RX 7700 XT",
            // RX 6000 series
            0x73bf => "Radeon RX 6900 XT",
            0x73a5 => "Radeon RX 6800 XT",
            0x73a3 => "Radeon RX 6800",
            0x73df => "Radeon RX 6700 XT",
            // RX 9000 series (RDNA 4)
            0x7500 => "Radeon RX 9070 XT",
            0x7501 => "Radeon RX 9070",
            0x7502 => "Radeon RX 9600 XT",
            0x7503 => "Radeon RX 9600",
            // Audio devices
            0xab38 | 0xab28 => "HD Audio Controller",
            _ => return None,
        };
        return Some(name.to_string());
    }

    None
}

/// Get class description from class code
fn get_class_description(class_code: u32) -> &'static str {
    let class_base = class_code & 0xFFFF00;
    match class_base {
        class_codes::VGA_COMPATIBLE => "VGA Controller",
        class_codes::CONTROLLER_3D => "3D Controller",
        class_codes::DISPLAY_CONTROLLER => "Display Controller",
        class_codes::AUDIO_DEVICE => "Audio Device",
        class_codes::USB_CONTROLLER => "USB Controller",
        class_codes::NETWORK_CONTROLLER => "Network Controller",
        class_codes::NVME_CONTROLLER => "NVMe Controller",
        _ => {
            let class_high = (class_code >> 16) & 0xFF;
            match class_high {
                0x01 => "Storage Controller",
                0x02 => "Network Controller",
                0x03 => "Display Controller",
                0x04 => "Multimedia Controller",
                0x05 => "Memory Controller",
                0x06 => "Bridge",
                0x07 => "Communication Controller",
                0x08 => "System Peripheral",
                0x09 => "Input Device",
                0x0a => "Docking Station",
                0x0b => "Processor",
                0x0c => "Serial Bus Controller",
                0x0d => "Wireless Controller",
                0x0e => "Intelligent I/O",
                0x0f => "Satellite Controller",
                0x10 => "Encryption Controller",
                0x11 => "Signal Processing",
                _ => "Unknown Device",
            }
        }
    }
}

/// Check multi-GPU passthrough prerequisites
pub fn check_multi_gpu_passthrough_status() -> MultiGpuPassthroughStatus {
    let mut status = MultiGpuPassthroughStatus {
        iommu_enabled: false,
        vfio_loaded: false,
        available_gpus: 0,
        passthrough_gpus: Vec::new(),
        boot_vga: None,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    // Check IOMMU
    status.iommu_enabled = check_iommu_enabled();
    if !status.iommu_enabled {
        status.errors.push("IOMMU is not enabled. Add intel_iommu=on or amd_iommu=on to kernel parameters.".to_string());
    }

    // Check VFIO modules
    status.vfio_loaded = check_vfio_modules();
    if !status.vfio_loaded {
        status.errors.push("VFIO modules not loaded. Run: sudo modprobe vfio-pci".to_string());
    }

    // Enumerate GPUs
    if let Ok(devices) = enumerate_pci_devices() {
        for device in devices {
            if device.is_gpu() {
                if device.is_boot_vga {
                    status.boot_vga = Some(device);
                } else if device.iommu_group.is_some() {
                    status.passthrough_gpus.push(device);
                } else {
                    status.warnings.push(format!(
                        "GPU {} has no IOMMU group - passthrough not possible",
                        device.address
                    ));
                }
            }
        }
    }

    status.available_gpus = status.passthrough_gpus.len();

    if status.available_gpus == 0 && status.boot_vga.is_some() {
        status.errors.push("Only one GPU found (boot VGA). Need a secondary GPU for passthrough.".to_string());
    }

    status
}

/// Check if IOMMU is enabled
fn check_iommu_enabled() -> bool {
    // Check for IOMMU groups directory
    let iommu_groups = Path::new("/sys/kernel/iommu_groups");
    if !iommu_groups.exists() {
        return false;
    }

    // Check if there are any groups
    if let Ok(entries) = fs::read_dir(iommu_groups) {
        return entries.count() > 0;
    }

    false
}

/// Check if VFIO modules are loaded
fn check_vfio_modules() -> bool {
    // Check /proc/modules for vfio-pci
    if let Ok(modules) = fs::read_to_string("/proc/modules") {
        // vfio_pci is the kernel module name (underscore not hyphen)
        return modules.contains("vfio_pci") || modules.contains("vfio-pci");
    }

    // Alternative: check if vfio-pci driver exists in sysfs
    Path::new("/sys/bus/pci/drivers/vfio-pci").exists()
}

/// Find devices in the same IOMMU group as the given device
pub fn find_iommu_group_devices(device: &PciDevice) -> Vec<PciDevice> {
    let Some(group) = device.iommu_group else {
        return Vec::new();
    };

    let group_path = PathBuf::from(format!("/sys/kernel/iommu_groups/{}/devices", group));

    if !group_path.exists() {
        return Vec::new();
    }

    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir(&group_path) {
        for entry in entries.flatten() {
            let address = entry.file_name().to_string_lossy().to_string();
            let device_path = PathBuf::from("/sys/bus/pci/devices").join(&address);

            if let Ok(dev) = read_pci_device(&device_path, &address) {
                devices.push(dev);
            }
        }
    }

    devices
}

/// Find the audio device paired with a GPU (same IOMMU group or adjacent address)
pub fn find_gpu_audio_pair(gpu: &PciDevice, all_devices: &[PciDevice]) -> Option<PciDevice> {
    // First, check IOMMU group for audio device
    if gpu.iommu_group.is_some() {
        let group_devices = find_iommu_group_devices(gpu);
        for dev in group_devices {
            if dev.is_audio() && dev.address != gpu.address {
                return Some(dev);
            }
        }
    }

    // Check for adjacent addresses (e.g., 01:00.0 GPU, 01:00.1 audio)
    // Parse the GPU address
    let parts: Vec<&str> = gpu.address.split('.').collect();
    if parts.len() == 2 {
        let base = parts[0];
        // Look for function 1 (audio is typically function 1)
        let audio_addr = format!("{}.1", base);

        for dev in all_devices {
            if dev.address == audio_addr && dev.is_audio() {
                return Some(dev.clone());
            }
        }
    }

    None
}

/// Generate QEMU arguments for PCI passthrough
pub fn generate_passthrough_args(devices: &[PciDevice]) -> Vec<String> {
    let mut args = Vec::new();

    // Find primary GPU (first VGA device)
    let primary_gpu_idx = devices.iter().position(|d| d.is_vga());

    for (i, device) in devices.iter().enumerate() {
        let is_primary = Some(i) == primary_gpu_idx;
        args.extend(device.to_qemu_args(is_primary));
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_device_is_gpu() {
        let device = PciDevice {
            address: "0000:01:00.0".to_string(),
            vendor_id: 0x10de,
            device_id: 0x2684,
            class_code: 0x030000,
            vendor_name: "NVIDIA".to_string(),
            device_name: "GeForce RTX 4090".to_string(),
            driver: Some("nvidia".to_string()),
            iommu_group: Some(1),
            is_boot_vga: false,
            subsystem_vendor_id: 0,
            subsystem_device_id: 0,
        };

        assert!(device.is_gpu());
        assert!(device.is_vga());
        assert!(device.is_nvidia());
        assert!(device.can_passthrough());
    }

    #[test]
    fn test_pci_device_boot_vga_cannot_passthrough() {
        let device = PciDevice {
            address: "0000:00:02.0".to_string(),
            vendor_id: 0x8086,
            device_id: 0x1234,
            class_code: 0x030000,
            vendor_name: "Intel".to_string(),
            device_name: "UHD Graphics".to_string(),
            driver: Some("i915".to_string()),
            iommu_group: Some(0),
            is_boot_vga: true,
            subsystem_vendor_id: 0,
            subsystem_device_id: 0,
        };

        assert!(device.is_gpu());
        assert!(!device.can_passthrough());
    }

    #[test]
    fn test_generate_passthrough_args() {
        let devices = vec![
            PciDevice {
                address: "0000:01:00.0".to_string(),
                vendor_id: 0x10de,
                device_id: 0x2684,
                class_code: 0x030000,
                vendor_name: "NVIDIA".to_string(),
                device_name: "GeForce RTX 4090".to_string(),
                driver: Some("vfio-pci".to_string()),
                iommu_group: Some(1),
                is_boot_vga: false,
                subsystem_vendor_id: 0,
                subsystem_device_id: 0,
            },
            PciDevice {
                address: "0000:01:00.1".to_string(),
                vendor_id: 0x10de,
                device_id: 0x228b,
                class_code: 0x040300,
                vendor_name: "NVIDIA".to_string(),
                device_name: "HD Audio Controller".to_string(),
                driver: Some("vfio-pci".to_string()),
                iommu_group: Some(1),
                is_boot_vga: false,
                subsystem_vendor_id: 0,
                subsystem_device_id: 0,
            },
        ];

        let args = generate_passthrough_args(&devices);

        assert_eq!(args.len(), 4); // 2 devices, each with 2 args
        assert!(args[1].contains("multifunction=on")); // Primary GPU
        assert!(!args[1].contains("x-vga")); // No x-vga (incompatible with modern NVIDIA)
        assert!(!args[3].contains("x-vga")); // Audio device also no x-vga
    }
}
