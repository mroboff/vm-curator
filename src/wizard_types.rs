//! Wizard and import state types, extracted from app.rs so they can be
//! exposed via the library target without pulling in the TUI (ratatui/crossterm).

use std::path::PathBuf;
use anyhow::Result;
use crate::vm::qemu_config::{PortForward, PortProtocol};

/// Action to take with an existing disk when using it for a new VM
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskAction {
    #[default]
    Copy,
    Move,
}

/// Steps in the VM creation wizard
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WizardStep {
    /// Step 1: Select name and OS type
    #[default]
    SelectOs,
    /// Step 2: Select ISO file
    SelectIso,
    /// Step 3: Configure disk settings
    ConfigureDisk,
    /// Step 4: Configure QEMU settings
    ConfigureQemu,
    /// Step 5: Review and confirm
    Confirm,
}

impl WizardStep {
    /// Get the step number (1-5)
    pub fn number(&self) -> u8 {
        match self {
            WizardStep::SelectOs => 1,
            WizardStep::SelectIso => 2,
            WizardStep::ConfigureDisk => 3,
            WizardStep::ConfigureQemu => 4,
            WizardStep::Confirm => 5,
        }
    }

    /// Get the step title
    pub fn title(&self) -> &'static str {
        match self {
            WizardStep::SelectOs => "Select Operating System",
            WizardStep::SelectIso => "Select Install Media",
            WizardStep::ConfigureDisk => "Configure Disk",
            WizardStep::ConfigureQemu => "Configure QEMU",
            WizardStep::Confirm => "Review & Create",
        }
    }

    /// Move to the next step
    pub fn next(&self) -> Option<WizardStep> {
        match self {
            WizardStep::SelectOs => Some(WizardStep::SelectIso),
            WizardStep::SelectIso => Some(WizardStep::ConfigureDisk),
            WizardStep::ConfigureDisk => Some(WizardStep::ConfigureQemu),
            WizardStep::ConfigureQemu => Some(WizardStep::Confirm),
            WizardStep::Confirm => None,
        }
    }

    /// Move to the previous step
    pub fn prev(&self) -> Option<WizardStep> {
        match self {
            WizardStep::SelectOs => None,
            WizardStep::SelectIso => Some(WizardStep::SelectOs),
            WizardStep::ConfigureDisk => Some(WizardStep::SelectIso),
            WizardStep::ConfigureQemu => Some(WizardStep::ConfigureDisk),
            WizardStep::Confirm => Some(WizardStep::ConfigureQemu),
        }
    }
}

/// QEMU configuration settings for the wizard
#[derive(Debug, Clone)]
pub struct WizardQemuConfig {
    /// QEMU emulator command
    pub emulator: String,
    /// RAM in megabytes
    pub memory_mb: u32,
    /// CPU cores
    pub cpu_cores: u32,
    /// CPU model (host, qemu64, pentium, etc.)
    pub cpu_model: Option<String>,
    /// Machine type (q35, pc, etc.)
    pub machine: Option<String>,
    /// Graphics adapter
    pub vga: String,
    /// Audio devices
    pub audio: Vec<String>,
    /// Network adapter model
    pub network_model: String,
    /// Disk interface
    pub disk_interface: String,
    /// Enable KVM acceleration
    pub enable_kvm: bool,
    /// Enable 3D/GL acceleration (requires virtio-vga)
    pub gl_acceleration: bool,
    /// UEFI boot mode
    pub uefi: bool,
    /// TPM emulation
    pub tpm: bool,
    /// RTC uses local time (for Windows)
    pub rtc_localtime: bool,
    /// USB tablet for mouse
    pub usb_tablet: bool,
    /// Display output
    pub display: String,
    /// Network backend
    pub network_backend: String,
    /// Port forwarding rules (user & passt backends)
    pub port_forwards: Vec<PortForward>,
    /// Bridge name when backend is "bridge"
    pub bridge_name: Option<String>,
    /// Custom MAC address for the NIC (canonical aa:bb:cc:dd:ee:ff form)
    pub mac_address: Option<String>,
    /// Additional QEMU arguments
    pub extra_args: Vec<String>,
    /// BIOS/ROM file path (for classic Mac and other systems needing custom firmware)
    pub bios_path: Option<PathBuf>,
}

impl Default for WizardQemuConfig {
    fn default() -> Self {
        Self {
            emulator: "qemu-system-x86_64".to_string(),
            memory_mb: 2048,
            cpu_cores: 2,
            cpu_model: Some("host".to_string()),
            machine: Some("q35".to_string()),
            vga: "std".to_string(),
            audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
            network_model: "e1000".to_string(),
            disk_interface: "ide".to_string(),
            enable_kvm: true,
            gl_acceleration: false,
            uefi: false,
            tpm: false,
            rtc_localtime: false,
            usb_tablet: true,
            display: "gtk".to_string(),
            network_backend: "user".to_string(),
            port_forwards: Vec::new(),
            bridge_name: None,
            mac_address: None,
            extra_args: Vec::new(),
            bios_path: None,
        }
    }
}

impl WizardQemuConfig {
    /// Create from a QEMU profile
    pub fn from_profile(profile: &crate::metadata::QemuProfile) -> Self {
        let gl_acceleration = profile.extra_args.iter().any(|arg|
            arg.contains("virtio-vga-gl") || arg.contains("gl=on")
        );

        Self {
            emulator: profile.emulator.clone(),
            memory_mb: profile.memory_mb,
            cpu_cores: profile.cpu_cores,
            cpu_model: profile.cpu_model.clone(),
            machine: profile.machine.clone(),
            vga: profile.vga.clone(),
            audio: profile.audio.clone(),
            network_model: profile.network_model.clone(),
            disk_interface: profile.disk_interface.clone(),
            enable_kvm: profile.enable_kvm,
            gl_acceleration,
            uefi: profile.uefi,
            tpm: profile.tpm,
            rtc_localtime: profile.rtc_localtime,
            usb_tablet: profile.usb_tablet,
            display: profile.display.clone(),
            network_backend: profile.network_backend.clone(),
            port_forwards: Vec::new(),
            bridge_name: None,
            mac_address: None,
            extra_args: profile.extra_args.clone(),
            bios_path: None,
        }
    }
}

/// Custom OS entry for when user selects "Other"
#[derive(Debug, Clone, Default)]
pub struct CustomOsEntry {
    pub id: String,
    pub name: String,
    pub publisher: String,
    #[allow(dead_code)]
    pub release_date: Option<String>,
    pub architecture: String,
    #[allow(dead_code)]
    pub short_blurb: String,
    #[allow(dead_code)]
    pub long_blurb: String,
    #[allow(dead_code)]
    pub fun_facts: Vec<String>,
    pub base_profile: String,
    #[allow(dead_code)]
    pub save_to_user: bool,
}

/// Fields that can be edited in the wizard
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WizardField {
    VmName,
    OsFilter,
    DiskSize,
    MemoryMb,
    CpuCores,
    MacAddress,
    CustomOsId,
    CustomOsName,
    CustomOsPublisher,
    CustomOsReleaseDate,
    CustomOsShortBlurb,
}

/// State for the VM creation wizard
#[derive(Debug, Clone)]
pub struct CreateWizardState {
    pub step: WizardStep,
    pub vm_name: String,
    pub folder_name: String,
    pub selected_os: Option<String>,
    pub custom_os: Option<CustomOsEntry>,
    pub iso_path: Option<PathBuf>,
    pub is_recovery_image: bool,
    pub iso_downloading: bool,
    pub iso_download_progress: f32,
    pub disk_size_gb: u32,
    pub use_existing_disk: bool,
    pub existing_disk_path: Option<PathBuf>,
    pub existing_disk_action: DiskAction,
    pub bios_rom_path: Option<PathBuf>,
    pub floppy_path: Option<PathBuf>,
    pub qemu_config: WizardQemuConfig,
    pub auto_launch: bool,
    pub field_focus: usize,
    #[allow(dead_code)]
    pub os_list_scroll: usize,
    pub os_filter: String,
    #[allow(dead_code)]
    pub selected_category: usize,
    pub expanded_categories: Vec<String>,
    pub os_list_selected: usize,
    pub error_message: Option<String>,
    pub editing_field: Option<WizardField>,
    pub wizard_edit_buffer: String,
}

impl Default for CreateWizardState {
    fn default() -> Self {
        Self {
            step: WizardStep::SelectOs,
            vm_name: String::new(),
            folder_name: String::new(),
            selected_os: None,
            custom_os: None,
            iso_path: None,
            is_recovery_image: false,
            iso_downloading: false,
            iso_download_progress: 0.0,
            disk_size_gb: 32,
            use_existing_disk: false,
            existing_disk_path: None,
            existing_disk_action: DiskAction::Copy,
            bios_rom_path: None,
            floppy_path: None,
            qemu_config: WizardQemuConfig::default(),
            auto_launch: true,
            field_focus: 0,
            os_list_scroll: 0,
            os_filter: String::new(),
            selected_category: 0,
            expanded_categories: vec![
                "windows".to_string(),
                "linux".to_string(),
            ],
            os_list_selected: 0,
            error_message: None,
            editing_field: None,
            wizard_edit_buffer: String::new(),
        }
    }
}

impl CreateWizardState {
    pub fn generate_folder_name(display_name: &str) -> String {
        display_name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn update_folder_name(&mut self, library_path: &std::path::Path) {
        let base_name = if let Some(ref os_id) = self.selected_os {
            os_id.clone()
        } else {
            Self::generate_folder_name(&self.vm_name)
        };
        self.folder_name = Self::find_available_folder_name(library_path, &base_name);
    }

    pub fn find_available_folder_name(library_path: &std::path::Path, base_name: &str) -> String {
        let first_candidate = library_path.join(base_name);
        if !first_candidate.exists() {
            return base_name.to_string();
        }
        for suffix in 2..=1000 {
            let candidate_name = format!("{}-{}", base_name, suffix);
            let candidate_path = library_path.join(&candidate_name);
            if !candidate_path.exists() {
                return candidate_name;
            }
        }
        format!("{}-error-too-many-vms", base_name)
    }

    pub fn apply_profile(&mut self, profile: &crate::metadata::QemuProfile) {
        self.disk_size_gb = profile.disk_size_gb;
        self.qemu_config = WizardQemuConfig::from_profile(profile);
    }

    pub fn can_proceed(&self) -> Result<(), String> {
        match self.step {
            WizardStep::SelectOs => {
                if self.vm_name.trim().is_empty() {
                    return Err("Please enter a VM name".to_string());
                }
                if self.selected_os.is_none() && self.custom_os.is_none() {
                    return Err("Please select an operating system".to_string());
                }
                Ok(())
            }
            WizardStep::SelectIso => Ok(()),
            WizardStep::ConfigureDisk => {
                if self.use_existing_disk {
                    match &self.existing_disk_path {
                        None => return Err("Please select an existing disk".to_string()),
                        Some(path) => {
                            if !path.exists() {
                                return Err(format!("Disk file not found: {}", path.display()));
                            }
                        }
                    }
                } else {
                    if self.disk_size_gb == 0 {
                        return Err("Disk size must be greater than 0".to_string());
                    }
                    if self.disk_size_gb > 10000 {
                        return Err("Disk size cannot exceed 10TB".to_string());
                    }
                }
                Ok(())
            }
            WizardStep::ConfigureQemu => {
                if self.qemu_config.memory_mb == 0 {
                    return Err("Memory must be greater than 0".to_string());
                }
                if self.qemu_config.cpu_cores == 0 {
                    return Err("CPU cores must be greater than 0".to_string());
                }
                Ok(())
            }
            WizardStep::Confirm => Ok(()),
        }
    }

    pub fn toggle_category(&mut self, category: &str) {
        if let Some(pos) = self.expanded_categories.iter().position(|c| c == category) {
            self.expanded_categories.remove(pos);
        } else {
            self.expanded_categories.push(category.to_string());
        }
    }

    pub fn is_category_expanded(&self, category: &str) -> bool {
        self.expanded_categories.iter().any(|c| c == category)
    }
}

/// State for network settings editing screen
#[derive(Debug, Clone)]
pub struct NetworkSettingsState {
    pub model: String,
    pub backend: String,
    pub bridge_name: Option<String>,
    pub port_forwards: Vec<PortForward>,
    pub mac_address: Option<String>,
    pub mac_edit_buffer: String,
    pub editing_mac: bool,
    pub selected_field: usize,
    pub editing_port_forwards: bool,
    pub pf_selected: usize,
    pub adding_pf: Option<AddingPortForward>,
}

/// State when adding a new port forward rule
#[derive(Debug, Clone)]
pub struct AddingPortForward {
    pub step: AddPfStep,
    pub protocol: PortProtocol,
    pub host_port_input: String,
    pub guest_port_input: String,
}

/// Steps when adding a port forward
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddPfStep {
    Protocol,
    HostPort,
    GuestPort,
}

// =========================================================================
// VM Import Wizard Types
// =========================================================================

/// Source type for VM import
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    Libvirt,
    Quickemu,
}

/// Disk handling action during import
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportDiskAction {
    #[default]
    Symlink,
    Copy,
    Move,
}

/// Steps in the import wizard
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ImportStep {
    #[default]
    SelectSource,
    SelectVm,
    CompatibilityWarnings,
    ConfigureDisk,
    ReviewAndImport,
}

/// A VM discovered from an external source that can be imported
#[derive(Debug, Clone)]
pub struct ImportableVm {
    pub name: String,
    pub config_path: PathBuf,
    pub source: ImportSource,
    pub qemu_config: WizardQemuConfig,
    pub disk_paths: Vec<PathBuf>,
    pub detected_os_profile: Option<String>,
    pub import_notes: Vec<String>,
    pub disks_readable: Vec<bool>,
}

/// State for the VM import wizard
#[derive(Debug, Clone)]
pub struct ImportWizardState {
    pub step: ImportStep,
    pub source: Option<ImportSource>,
    pub discovered_vms: Vec<ImportableVm>,
    pub selected_vm_index: usize,
    pub selected_vm: Option<ImportableVm>,
    pub vm_name: String,
    pub folder_name: String,
    pub disk_action: ImportDiskAction,
    pub field_focus: usize,
    pub error_message: Option<String>,
    pub editing_name: bool,
    pub warnings_acknowledged: bool,
}

impl Default for ImportWizardState {
    fn default() -> Self {
        Self {
            step: ImportStep::SelectSource,
            source: None,
            discovered_vms: Vec::new(),
            selected_vm_index: 0,
            selected_vm: None,
            vm_name: String::new(),
            folder_name: String::new(),
            disk_action: ImportDiskAction::Symlink,
            field_focus: 0,
            error_message: None,
            editing_name: false,
            warnings_acknowledged: false,
        }
    }
}
