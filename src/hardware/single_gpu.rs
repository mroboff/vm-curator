//! Single GPU Passthrough Support
//!
//! Handles configuration and detection for single GPU passthrough scenarios where
//! the user's only (or primary) GPU is passed to a VM. This requires stopping the
//! display manager and running from a TTY.

use std::fs;
use std::path::Path;
use std::process::Command;

use super::pci::PciDevice;

/// Supported display managers (systemd-based only)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayManager {
    Gdm,
    Sddm,
    Lightdm,
    Unknown(String),
}

impl DisplayManager {
    /// Get the systemd service name for this display manager
    pub fn service_name(&self) -> &str {
        match self {
            DisplayManager::Gdm => "gdm",
            DisplayManager::Sddm => "sddm",
            DisplayManager::Lightdm => "lightdm",
            DisplayManager::Unknown(name) => name,
        }
    }

    /// Get display name for UI
    pub fn display_name(&self) -> &str {
        match self {
            DisplayManager::Gdm => "GDM (GNOME)",
            DisplayManager::Sddm => "SDDM (KDE)",
            DisplayManager::Lightdm => "LightDM",
            DisplayManager::Unknown(name) => name,
        }
    }
}

impl std::fmt::Display for DisplayManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.service_name())
    }
}

/// GPU driver types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuDriver {
    Nvidia,
    Amdgpu,
    I915,
    Nouveau,
    Radeon,
    Other(String),
}

impl GpuDriver {
    /// Get the kernel module name
    pub fn module_name(&self) -> &str {
        match self {
            GpuDriver::Nvidia => "nvidia",
            GpuDriver::Amdgpu => "amdgpu",
            GpuDriver::I915 => "i915",
            GpuDriver::Nouveau => "nouveau",
            GpuDriver::Radeon => "radeon",
            GpuDriver::Other(name) => name,
        }
    }

    /// Get additional modules that need to be unloaded (e.g., nvidia dependencies)
    pub fn dependent_modules(&self) -> Vec<&'static str> {
        match self {
            GpuDriver::Nvidia => vec!["nvidia_drm", "nvidia_modeset", "nvidia_uvm", "nvidia"],
            GpuDriver::Amdgpu => vec!["amdgpu"],
            GpuDriver::I915 => vec!["i915"],
            GpuDriver::Nouveau => vec!["nouveau"],
            GpuDriver::Radeon => vec!["radeon"],
            GpuDriver::Other(_) => vec![],
        }
    }
}

impl std::fmt::Display for GpuDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.module_name())
    }
}

/// Single GPU passthrough configuration
///
/// Note: Looking Glass is NOT used for single-GPU passthrough because the display
/// goes directly to physical monitors connected to the passed-through GPU.
#[derive(Debug, Clone)]
pub struct SingleGpuConfig {
    /// The GPU device to pass through
    pub gpu: PciDevice,
    /// Associated audio device (if any)
    pub audio: Option<PciDevice>,
    /// All devices in the IOMMU group
    pub iommu_group_devices: Vec<PciDevice>,
    /// Original driver the GPU was using
    pub original_driver: GpuDriver,
    /// Detected display manager
    pub display_manager: DisplayManager,
}

impl SingleGpuConfig {
    /// Create a new configuration for the given GPU
    pub fn new(gpu: PciDevice, all_devices: &[PciDevice]) -> Self {
        let audio = super::pci::find_gpu_audio_pair(&gpu, all_devices);
        let iommu_group_devices = super::pci::find_iommu_group_devices(&gpu);
        let original_driver = detect_gpu_driver(&gpu);
        let display_manager = detect_display_manager();

        Self {
            gpu,
            audio,
            iommu_group_devices,
            original_driver,
            display_manager,
        }
    }

    /// Get all PCI addresses that need to be unbound/rebound
    pub fn all_passthrough_addresses(&self) -> Vec<&str> {
        let mut addrs = vec![self.gpu.address.as_str()];
        if let Some(ref audio) = self.audio {
            addrs.push(audio.address.as_str());
        }
        addrs
    }
}

/// Detect the currently running display manager
pub fn detect_display_manager() -> DisplayManager {
    // Check running services
    let dm_services = [
        ("gdm", DisplayManager::Gdm),
        ("sddm", DisplayManager::Sddm),
        ("lightdm", DisplayManager::Lightdm),
    ];

    for (service, dm) in dm_services {
        if is_service_active(service) {
            return dm;
        }
    }

    // Check systemctl get-default for graphical target
    if let Ok(output) = Command::new("systemctl")
        .args(["get-default"])
        .output()
    {
        let target = String::from_utf8_lossy(&output.stdout);
        if target.contains("graphical") {
            // Check which DM is set as display-manager
            if let Ok(link) = fs::read_link("/etc/systemd/system/display-manager.service") {
                let link_str = link.to_string_lossy();
                if link_str.contains("gdm") {
                    return DisplayManager::Gdm;
                } else if link_str.contains("sddm") {
                    return DisplayManager::Sddm;
                } else if link_str.contains("lightdm") {
                    return DisplayManager::Lightdm;
                } else {
                    // Extract service name from path
                    if let Some(name) = link.file_name() {
                        let name = name.to_string_lossy();
                        if let Some(name) = name.strip_suffix(".service") {
                            return DisplayManager::Unknown(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Try checking /etc/X11/default-display-manager (Debian-based)
    if let Ok(content) = fs::read_to_string("/etc/X11/default-display-manager") {
        let content = content.trim();
        if content.contains("gdm") {
            return DisplayManager::Gdm;
        } else if content.contains("sddm") {
            return DisplayManager::Sddm;
        } else if content.contains("lightdm") {
            return DisplayManager::Lightdm;
        }
    }

    DisplayManager::Unknown("display-manager".to_string())
}

/// Check if a systemd service is active
fn is_service_active(service: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Detect the GPU driver from a PCI device
pub fn detect_gpu_driver(device: &PciDevice) -> GpuDriver {
    if let Some(ref driver) = device.driver {
        match driver.as_str() {
            "nvidia" => GpuDriver::Nvidia,
            "amdgpu" => GpuDriver::Amdgpu,
            "i915" => GpuDriver::I915,
            "nouveau" => GpuDriver::Nouveau,
            "radeon" => GpuDriver::Radeon,
            other => GpuDriver::Other(other.to_string()),
        }
    } else {
        // Try to infer from vendor
        if device.is_nvidia() {
            GpuDriver::Nvidia
        } else if device.is_amd() {
            GpuDriver::Amdgpu
        } else if device.is_intel() {
            GpuDriver::I915
        } else {
            GpuDriver::Other("unknown".to_string())
        }
    }
}

/// Check if the system supports single GPU passthrough
pub fn check_single_gpu_support() -> SingleGpuSupport {
    let (boot_vga, has_single_gpu) = if let Ok(devices) = super::pci::enumerate_pci_devices() {
        (
            devices.iter().find(|d| d.is_boot_vga).cloned(),
            devices.iter().filter(|d| d.is_gpu()).count() == 1,
        )
    } else {
        (None, false)
    };

    let support = SingleGpuSupport {
        iommu_enabled: Path::new("/sys/kernel/iommu_groups").exists()
            && fs::read_dir("/sys/kernel/iommu_groups")
                .map(|d| d.count() > 0)
                .unwrap_or(false),
        vfio_available: Path::new("/sys/bus/pci/drivers/vfio-pci").exists()
            || check_module_available("vfio_pci"),
        boot_vga,
        has_single_gpu,
        display_manager: Some(detect_display_manager()),
    };

    support
}

/// Check if a kernel module is available (not necessarily loaded)
fn check_module_available(module: &str) -> bool {
    // Check /proc/modules for loaded modules
    if let Ok(modules) = fs::read_to_string("/proc/modules") {
        if modules.contains(module) {
            return true;
        }
    }

    // Check if module exists in kernel modules directory
    if let Ok(output) = Command::new("modinfo").arg(module).output() {
        return output.status.success();
    }

    false
}

/// Single GPU passthrough support status
#[derive(Debug, Default)]
pub struct SingleGpuSupport {
    /// IOMMU is enabled
    pub iommu_enabled: bool,
    /// VFIO modules are available
    pub vfio_available: bool,
    /// The boot VGA device (if found)
    pub boot_vga: Option<PciDevice>,
    /// System has only one GPU
    pub has_single_gpu: bool,
    /// Detected display manager
    pub display_manager: Option<DisplayManager>,
}

impl SingleGpuSupport {
    /// Check if basic single GPU passthrough is possible
    pub fn is_supported(&self) -> bool {
        self.iommu_enabled && self.vfio_available && self.boot_vga.is_some()
    }

    /// Get a summary of the support status
    pub fn summary(&self) -> String {
        if self.is_supported() {
            "Single GPU passthrough is available".to_string()
        } else {
            let mut issues = Vec::new();
            if !self.iommu_enabled {
                issues.push("IOMMU not enabled");
            }
            if !self.vfio_available {
                issues.push("VFIO modules not available");
            }
            if self.boot_vga.is_none() {
                issues.push("No boot VGA device found");
            }
            format!("Not available: {}", issues.join(", "))
        }
    }
}

/// Check if we're running from a TTY (not a graphical terminal)
pub fn is_running_from_tty() -> bool {
    // Check for DISPLAY or WAYLAND_DISPLAY environment variables
    std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err()
}

/// Check if single GPU scripts exist for a VM
pub fn scripts_exist(vm_path: &Path) -> bool {
    vm_path.join("single-gpu-start.sh").exists()
}

/// Config file name for persisted single-GPU configuration
const SINGLE_GPU_CONFIG_FILE: &str = "single-gpu-config.toml";

/// Save SingleGpuConfig to a file in the VM directory
pub fn save_config(vm_path: &Path, config: &SingleGpuConfig) -> anyhow::Result<()> {
    let config_path = vm_path.join(SINGLE_GPU_CONFIG_FILE);

    let mut content = String::new();
    content.push_str("# Single GPU Passthrough Configuration\n");
    content.push_str("# Generated by vm-curator - do not edit manually\n\n");

    content.push_str("[gpu]\n");
    content.push_str(&format!("address = \"{}\"\n", config.gpu.address));
    content.push_str(&format!("vendor_id = \"0x{:04x}\"\n", config.gpu.vendor_id));
    content.push_str(&format!("device_id = \"0x{:04x}\"\n", config.gpu.device_id));
    content.push_str(&format!("vendor_name = \"{}\"\n", config.gpu.vendor_name.replace('"', "\\\"")));
    content.push_str(&format!("device_name = \"{}\"\n", config.gpu.device_name.replace('"', "\\\"")));
    content.push_str(&format!("class_code = \"0x{:06x}\"\n", config.gpu.class_code));
    if let Some(ref driver) = config.gpu.driver {
        content.push_str(&format!("driver = \"{}\"\n", driver));
    }
    if let Some(group) = config.gpu.iommu_group {
        content.push_str(&format!("iommu_group = {}\n", group));
    }
    content.push_str(&format!("is_boot_vga = {}\n", config.gpu.is_boot_vga));

    if let Some(ref audio) = config.audio {
        content.push_str("\n[audio]\n");
        content.push_str(&format!("address = \"{}\"\n", audio.address));
        content.push_str(&format!("vendor_id = \"0x{:04x}\"\n", audio.vendor_id));
        content.push_str(&format!("device_id = \"0x{:04x}\"\n", audio.device_id));
        content.push_str(&format!("vendor_name = \"{}\"\n", audio.vendor_name.replace('"', "\\\"")));
        content.push_str(&format!("device_name = \"{}\"\n", audio.device_name.replace('"', "\\\"")));
        content.push_str(&format!("class_code = \"0x{:06x}\"\n", audio.class_code));
        if let Some(ref driver) = audio.driver {
            content.push_str(&format!("driver = \"{}\"\n", driver));
        }
        if let Some(group) = audio.iommu_group {
            content.push_str(&format!("iommu_group = {}\n", group));
        }
    }

    content.push_str("\n[settings]\n");
    content.push_str(&format!("original_driver = \"{}\"\n", config.original_driver.module_name()));
    content.push_str(&format!("display_manager = \"{}\"\n", config.display_manager.service_name()));

    fs::write(&config_path, content)?;
    Ok(())
}

/// Load SingleGpuConfig from a file in the VM directory
pub fn load_config(vm_path: &Path) -> Option<SingleGpuConfig> {
    let config_path = vm_path.join(SINGLE_GPU_CONFIG_FILE);
    if !config_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&config_path).ok()?;

    // Parse the TOML manually (simple parser for our known format)
    let mut gpu_address = String::new();
    let mut gpu_vendor_id: u16 = 0;
    let mut gpu_device_id: u16 = 0;
    let mut gpu_vendor_name = String::new();
    let mut gpu_device_name = String::new();
    let mut gpu_class_id: u32 = 0;
    let mut gpu_driver: Option<String> = None;
    let mut gpu_iommu_group: Option<u32> = None;
    let mut gpu_is_boot_vga = false;

    let mut audio_address: Option<String> = None;
    let mut audio_vendor_id: u16 = 0;
    let mut audio_device_id: u16 = 0;
    let mut audio_vendor_name = String::new();
    let mut audio_device_name = String::new();
    let mut audio_class_id: u32 = 0;
    let mut audio_driver: Option<String> = None;
    let mut audio_iommu_group: Option<u32> = None;

    let mut original_driver = String::new();
    let mut display_manager = String::new();

    let mut current_section = "";

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            current_section = &line[1..line.len()-1];
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match current_section {
                "gpu" => match key {
                    "address" => gpu_address = value.to_string(),
                    "vendor_id" => gpu_vendor_id = parse_hex_u16(value),
                    "device_id" => gpu_device_id = parse_hex_u16(value),
                    "vendor_name" => gpu_vendor_name = value.to_string(),
                    "device_name" => gpu_device_name = value.to_string(),
                    "class_code" => gpu_class_id = parse_hex_u32(value),
                    "driver" => gpu_driver = Some(value.to_string()),
                    "iommu_group" => gpu_iommu_group = value.parse().ok(),
                    "is_boot_vga" => gpu_is_boot_vga = value == "true",
                    _ => {}
                },
                "audio" => match key {
                    "address" => audio_address = Some(value.to_string()),
                    "vendor_id" => audio_vendor_id = parse_hex_u16(value),
                    "device_id" => audio_device_id = parse_hex_u16(value),
                    "vendor_name" => audio_vendor_name = value.to_string(),
                    "device_name" => audio_device_name = value.to_string(),
                    "class_code" => audio_class_id = parse_hex_u32(value),
                    "driver" => audio_driver = Some(value.to_string()),
                    "iommu_group" => audio_iommu_group = value.parse().ok(),
                    _ => {}
                },
                "settings" => match key {
                    "original_driver" => original_driver = value.to_string(),
                    "display_manager" => display_manager = value.to_string(),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    if gpu_address.is_empty() {
        return None;
    }

    let gpu = PciDevice {
        address: gpu_address,
        vendor_id: gpu_vendor_id,
        device_id: gpu_device_id,
        vendor_name: gpu_vendor_name,
        device_name: gpu_device_name,
        class_code: gpu_class_id,
        driver: gpu_driver,
        iommu_group: gpu_iommu_group,
        is_boot_vga: gpu_is_boot_vga,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    };

    let audio = audio_address.map(|addr| PciDevice {
        address: addr,
        vendor_id: audio_vendor_id,
        device_id: audio_device_id,
        vendor_name: audio_vendor_name,
        device_name: audio_device_name,
        class_code: audio_class_id,
        driver: audio_driver,
        iommu_group: audio_iommu_group,
        is_boot_vga: false,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    });

    let original_driver = match original_driver.as_str() {
        "nvidia" => GpuDriver::Nvidia,
        "amdgpu" => GpuDriver::Amdgpu,
        "i915" => GpuDriver::I915,
        "nouveau" => GpuDriver::Nouveau,
        "radeon" => GpuDriver::Radeon,
        other => GpuDriver::Other(other.to_string()),
    };

    let display_manager = match display_manager.as_str() {
        "gdm" => DisplayManager::Gdm,
        "sddm" => DisplayManager::Sddm,
        "lightdm" => DisplayManager::Lightdm,
        other => DisplayManager::Unknown(other.to_string()),
    };

    Some(SingleGpuConfig {
        gpu,
        audio,
        iommu_group_devices: Vec::new(), // Not persisted, not needed for regeneration
        original_driver,
        display_manager,
    })
}

fn parse_hex_u16(s: &str) -> u16 {
    let s = s.trim_start_matches("0x");
    u16::from_str_radix(s, 16).unwrap_or(0)
}

fn parse_hex_u32(s: &str) -> u32 {
    let s = s.trim_start_matches("0x");
    u32::from_str_radix(s, 16).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_manager_service_names() {
        assert_eq!(DisplayManager::Gdm.service_name(), "gdm");
        assert_eq!(DisplayManager::Sddm.service_name(), "sddm");
        assert_eq!(DisplayManager::Lightdm.service_name(), "lightdm");
    }

    #[test]
    fn test_gpu_driver_modules() {
        assert_eq!(GpuDriver::Nvidia.module_name(), "nvidia");
        assert!(!GpuDriver::Nvidia.dependent_modules().is_empty());
    }
}
