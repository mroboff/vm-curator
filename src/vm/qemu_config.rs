use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// QEMU emulator type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QemuEmulator {
    X86_64,
    I386,
    Ppc,
    M68k,
    Arm,
    Aarch64,
    Other(String),
}

impl QemuEmulator {
    pub fn from_command(cmd: &str) -> Self {
        match cmd {
            "qemu-system-x86_64" => Self::X86_64,
            "qemu-system-i386" => Self::I386,
            "qemu-system-ppc" => Self::Ppc,
            "qemu-system-m68k" => Self::M68k,
            "qemu-system-arm" => Self::Arm,
            "qemu-system-aarch64" => Self::Aarch64,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn command(&self) -> &str {
        match self {
            Self::X86_64 => "qemu-system-x86_64",
            Self::I386 => "qemu-system-i386",
            Self::Ppc => "qemu-system-ppc",
            Self::M68k => "qemu-system-m68k",
            Self::Arm => "qemu-system-arm",
            Self::Aarch64 => "qemu-system-aarch64",
            Self::Other(cmd) => cmd,
        }
    }

    pub fn architecture(&self) -> &str {
        match self {
            Self::X86_64 => "x86_64",
            Self::I386 => "i386",
            Self::Ppc => "PowerPC",
            Self::M68k => "Motorola 68k",
            Self::Arm => "ARM",
            Self::Aarch64 => "ARM64",
            Self::Other(_) => "Unknown",
        }
    }
}

/// VGA adapter type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VgaType {
    #[default]
    Std,
    Cirrus,
    Vmware,
    Qxl,
    Virtio,
    None,
    Other(String),
}

impl VgaType {
    // Infallible parser (unknown values map to `Other`), so an inherent method is
    // more ergonomic than the fallible `std::str::FromStr` trait.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "std" => Self::Std,
            "cirrus" => Self::Cirrus,
            "vmware" => Self::Vmware,
            "qxl" => Self::Qxl,
            "virtio" => Self::Virtio,
            "none" => Self::None,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Audio device type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioDevice {
    Sb16,
    Ac97,
    Es1370,
    Hda,
    PcSpk,
    Other(String),
}

impl AudioDevice {
    // Infallible parser (unknown values map to `Other`), so an inherent method is
    // more ergonomic than the fallible `std::str::FromStr` trait.
    #[allow(dead_code, clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sb16" => Self::Sb16,
            "ac97" => Self::Ac97,
            "es1370" => Self::Es1370,
            "hda" | "intel-hda" => Self::Hda,
            "pcspk" => Self::PcSpk,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Network backend type
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkBackend {
    /// SLIRP - default, no root needed
    #[default]
    User,
    /// passt - modern, fast, no root needed
    Passt,
    /// Bridge networking via qemu-bridge-helper
    Bridge(String),
    /// No networking
    None,
}

impl fmt::Display for NetworkBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Passt => write!(f, "passt"),
            Self::Bridge(name) => write!(f, "bridge:{}", name),
            Self::None => write!(f, "none"),
        }
    }
}

/// A single port forwarding rule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortForward {
    pub protocol: PortProtocol,
    pub host_port: u16,
    pub guest_port: u16,
}

impl fmt::Display for PortForward {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} -> {}",
            self.protocol, self.host_port, self.guest_port
        )
    }
}

/// Port forwarding protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortProtocol {
    Tcp,
    Udp,
}

impl fmt::Display for PortProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp => write!(f, "TCP"),
            Self::Udp => write!(f, "UDP"),
        }
    }
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub model: String,
    pub backend: NetworkBackend,
    pub port_forwards: Vec<PortForward>,
    /// Legacy field kept for parsing existing launch.sh
    #[serde(default = "default_true")]
    pub user_net: bool,
    pub bridge: Option<String>,
    /// Custom MAC address for the NIC (canonical aa:bb:cc:dd:ee:ff form).
    /// `None` lets QEMU pick its own.
    #[serde(default)]
    pub mac_address: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            model: "e1000".to_string(),
            backend: NetworkBackend::User,
            port_forwards: Vec::new(),
            user_net: true,
            bridge: None,
            mac_address: None,
        }
    }
}

/// Disk image format
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiskFormat {
    Qcow2,
    Raw,
    Vmdk,
    Vdi,
    Other(String),
}

impl DiskFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "qcow2" => Self::Qcow2,
            "raw" | "img" => Self::Raw,
            "vmdk" => Self::Vmdk,
            "vdi" => Self::Vdi,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn supports_snapshots(&self) -> bool {
        matches!(self, Self::Qcow2)
    }
}

/// Role a parsed drive plays in the VM
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DiskRole {
    /// Bootable system disk (image file or physical block device)
    #[default]
    System,
    /// Firmware image (pflash / OVMF)
    Firmware,
    /// Removable media attachment (cdrom, recovery image)
    Media,
}

/// Disk configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    pub path: PathBuf,
    pub format: DiskFormat,
    pub interface: String,
    #[serde(default)]
    pub role: DiskRole,
}

impl DiskConfig {
    /// True for raw block devices passed through from the host (/dev/...)
    pub fn is_physical_device(&self) -> bool {
        self.path.starts_with("/dev")
    }
}

/// Boot mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BootMode {
    #[default]
    Normal,
    Install,
    Cdrom(PathBuf),
    Recovery(PathBuf),
    Floppy(PathBuf),
    Network,
}

/// QEMU configuration extracted from launch.sh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QemuConfig {
    pub emulator: QemuEmulator,
    pub memory_mb: u32,
    pub cpu_cores: u32,
    pub cpu_model: Option<String>,
    pub machine: Option<String>,
    pub vga: VgaType,
    pub audio_devices: Vec<AudioDevice>,
    pub networks: Vec<NetworkConfig>,
    pub disks: Vec<DiskConfig>,
    pub boot_mode: BootMode,
    pub enable_kvm: bool,
    pub uefi: bool,
    pub tpm: bool,
    pub extra_args: Vec<String>,
    pub raw_script: String,
    /// BIOS/ROM file path (for classic Mac and other systems needing custom firmware)
    #[serde(default)]
    pub bios_path: Option<PathBuf>,
}

impl Default for QemuConfig {
    fn default() -> Self {
        Self {
            emulator: QemuEmulator::X86_64,
            memory_mb: 512,
            cpu_cores: 1,
            cpu_model: None,
            machine: None,
            vga: VgaType::default(),
            audio_devices: Vec::new(),
            networks: vec![NetworkConfig::default()],
            disks: Vec::new(),
            boot_mode: BootMode::default(),
            enable_kvm: false,
            uefi: false,
            tpm: false,
            extra_args: Vec::new(),
            raw_script: String::new(),
            bios_path: None,
        }
    }
}

impl QemuConfig {
    /// Check if this VM supports snapshots (qcow2 system disks).
    /// Firmware (pflash/OVMF) and media drives are excluded: on distros that
    /// ship qcow2 OVMF images the firmware must never count as snapshotable.
    pub fn supports_snapshots(&self) -> bool {
        self.disks
            .iter()
            .any(|d| d.role == DiskRole::System && d.format.supports_snapshots())
    }

    /// Get the primary disk for snapshot/reset operations.
    /// Only System-role disks qualify — never pflash firmware or cdrom media.
    pub fn primary_disk(&self) -> Option<&DiskConfig> {
        self.disks
            .iter()
            .find(|d| d.role == DiskRole::System && d.format.supports_snapshots())
            .or_else(|| self.disks.iter().find(|d| d.role == DiskRole::System))
    }

    /// Whether para-virtualized 3D acceleration is currently enabled.
    /// Detected from the raw script (gl=on on display, virtio-vga-gl device,
    /// or any extra_arg containing those tokens).
    pub fn has_gl_acceleration(&self) -> bool {
        if self
            .extra_args
            .iter()
            .any(|arg| arg.contains("virtio-vga-gl") || arg.contains("gl=on"))
        {
            return true;
        }
        let s = &self.raw_script;
        s.contains("virtio-vga-gl") || s.contains("gl=on")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_gl_acceleration_detects_device() {
        let cfg = QemuConfig {
            raw_script: "qemu-system-x86_64 \\\n  -device virtio-vga-gl \\\n  -display sdl,gl=on"
                .to_string(),
            ..Default::default()
        };
        assert!(cfg.has_gl_acceleration());
    }

    #[test]
    fn has_gl_acceleration_detects_extra_arg() {
        let cfg = QemuConfig {
            extra_args: vec!["-display gtk,gl=on".to_string()],
            ..Default::default()
        };
        assert!(cfg.has_gl_acceleration());
    }

    #[test]
    fn has_gl_acceleration_negative() {
        let cfg = QemuConfig {
            raw_script: "qemu-system-x86_64 \\\n  -vga std \\\n  -display gtk".to_string(),
            ..Default::default()
        };
        assert!(!cfg.has_gl_acceleration());
    }
}
