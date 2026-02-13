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
    #[allow(dead_code)]
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
        write!(f, "{} {} -> {}", self.protocol, self.host_port, self.guest_port)
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

/// Disk configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    pub path: PathBuf,
    pub format: DiskFormat,
    pub interface: String,
}

/// Boot mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BootMode {
    #[default]
    Normal,
    Install,
    Cdrom(PathBuf),
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
    pub network: Option<NetworkConfig>,
    pub disks: Vec<DiskConfig>,
    pub boot_mode: BootMode,
    pub enable_kvm: bool,
    pub uefi: bool,
    pub tpm: bool,
    pub extra_args: Vec<String>,
    pub raw_script: String,
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
            network: Some(NetworkConfig::default()),
            disks: Vec::new(),
            boot_mode: BootMode::default(),
            enable_kvm: false,
            uefi: false,
            tpm: false,
            extra_args: Vec::new(),
            raw_script: String::new(),
        }
    }
}

impl QemuConfig {
    /// Check if this VM supports snapshots (qcow2 disks)
    pub fn supports_snapshots(&self) -> bool {
        self.disks.iter().any(|d| d.format.supports_snapshots())
    }

    /// Get the primary disk for snapshot operations
    pub fn primary_disk(&self) -> Option<&DiskConfig> {
        self.disks.first()
    }
}
