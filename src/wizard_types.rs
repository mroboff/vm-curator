//! Wizard and import state types, extracted from app.rs so they can be
//! exposed via the library target without pulling in the TUI (ratatui/crossterm).

use crate::vm::qemu_config::{PortForward, PortProtocol};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Which group a wizard-created/imported VM should join.
///
/// `Default` defers to the same OS-category default `App::refresh_vms`
/// already assigns any newly-discovered, ungrouped VM to — no extra action
/// needed. `Existing`/`New` are an explicit override, applied once the VM
/// exists via `App::set_vm_group`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GroupChoice {
    #[default]
    Default,
    Existing(String),
    New(String),
}

impl GroupChoice {
    /// Human-readable label for display.
    pub fn label(&self) -> String {
        match self {
            GroupChoice::Default => "Default (OS category)".to_string(),
            GroupChoice::Existing(name) | GroupChoice::New(name) => name.clone(),
        }
    }

    /// The explicit group name to join, or `None` for the default.
    pub fn target_name(&self) -> Option<&str> {
        match self {
            GroupChoice::Default => None,
            GroupChoice::Existing(name) | GroupChoice::New(name) => Some(name.as_str()),
        }
    }

    /// Cycle forward: Default -> each of `existing_groups` in order -> Default.
    pub fn next(&self, existing_groups: &[String]) -> Self {
        match self {
            GroupChoice::Default | GroupChoice::New(_) => existing_groups
                .first()
                .cloned()
                .map(GroupChoice::Existing)
                .unwrap_or(GroupChoice::Default),
            GroupChoice::Existing(name) => {
                let next_idx = existing_groups
                    .iter()
                    .position(|g| g == name)
                    .map(|i| i + 1);
                match next_idx.and_then(|i| existing_groups.get(i)) {
                    Some(next) => GroupChoice::Existing(next.clone()),
                    None => GroupChoice::Default,
                }
            }
        }
    }

    /// Cycle backward: the mirror of [`Self::next`].
    pub fn prev(&self, existing_groups: &[String]) -> Self {
        match self {
            GroupChoice::Default | GroupChoice::New(_) => existing_groups
                .last()
                .cloned()
                .map(GroupChoice::Existing)
                .unwrap_or(GroupChoice::Default),
            GroupChoice::Existing(name) => match existing_groups.iter().position(|g| g == name) {
                Some(0) | None => GroupChoice::Default,
                Some(i) => GroupChoice::Existing(existing_groups[i - 1].clone()),
            },
        }
    }
}

/// Disk image format to create for new VMs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskImageFormat {
    #[default]
    Qcow2,
    Raw,
}

impl DiskImageFormat {
    /// Format name accepted by `qemu-img`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qcow2 => "qcow2",
            Self::Raw => "raw",
        }
    }

    /// File extension used for newly-created or imported disks.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Qcow2 => "qcow2",
            Self::Raw => "raw",
        }
    }

    /// Short label for compact UI summaries.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Qcow2 => "qcow2",
            Self::Raw => "raw",
        }
    }

    /// Human-readable description for format selection UI.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Qcow2 => "qcow2 (copy-on-write, snapshots supported)",
            Self::Raw => "raw (plain disk image, no snapshots)",
        }
    }

    /// Human-readable storage behavior summary.
    #[must_use]
    pub fn storage_description(self) -> &'static str {
        match self {
            Self::Qcow2 => "Expandable (only uses space as needed)",
            Self::Raw => "Plain sparse file (guest sees full size)",
        }
    }

    /// Compact summary for confirmation screens.
    #[must_use]
    pub fn summary(self) -> &'static str {
        match self {
            Self::Qcow2 => "qcow2 (expandable)",
            Self::Raw => "raw (no snapshots)",
        }
    }

    /// Alternate between the supported creation formats.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Qcow2 => Self::Raw,
            Self::Raw => Self::Qcow2,
        }
    }

    /// Map a `qemu-img info` format string into a supported wizard format.
    #[must_use]
    pub fn from_qemu_format(format: &str) -> Option<Self> {
        match format.to_lowercase().as_str() {
            "qcow2" | "qcow" => Some(Self::Qcow2),
            "raw" => Some(Self::Raw),
            _ => None,
        }
    }

    /// Infer the disk format from a path extension.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    #[must_use]
    fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "qcow2" | "qcow" => Some(Self::Qcow2),
            "raw" | "img" => Some(Self::Raw),
            _ => None,
        }
    }
}

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

/// A single network adapter's settings, as edited in the wizard or the
/// Network Settings screen. A VM can have any number of these.
#[derive(Debug, Clone, PartialEq)]
pub struct NicConfig {
    /// Network adapter model ("virtio", "e1000", ... or "none")
    pub model: String,
    /// Network backend ("user", "passt", "bridge", or "none")
    pub backend: String,
    /// Port forwarding rules (user & passt backends)
    pub port_forwards: Vec<PortForward>,
    /// Bridge name when backend is "bridge"
    pub bridge_name: Option<String>,
    /// Custom MAC address for the NIC (canonical aa:bb:cc:dd:ee:ff form)
    pub mac_address: Option<String>,
}

impl Default for NicConfig {
    fn default() -> Self {
        Self {
            model: "e1000".to_string(),
            backend: "user".to_string(),
            port_forwards: Vec::new(),
            bridge_name: None,
            mac_address: None,
        }
    }
}

impl NicConfig {
    /// Cycle this NIC's backend through `stops` (see
    /// `App::get_network_backend_stops`), keeping `bridge_name` in sync:
    /// landing on a stop tied to a specific managed network sets
    /// `bridge_name` to that network's bridge; landing on the generic
    /// "bridge" stop (no specific network) defaults it when unset.
    pub fn cycle_backend(
        &mut self,
        stops: &[(String, Option<String>)],
        default_bridge: &Option<String>,
        delta: i32,
    ) {
        // Resolve every stop's *effective* bridge name up front (the
        // generic "bridge" stop falls back to `default_bridge`, same as
        // landing on it does below), then drop any stop that's
        // indistinguishable from an earlier one. This matters whenever
        // `default_bridge` happens to equal a specific managed network's
        // bridge (common once any managed network is running, since it
        // becomes a real system bridge) — without deduping, the generic
        // stop and that specific stop resolve to the same value, so
        // position-based lookups below always land on the specific one
        // and cycling backward gets stuck bouncing between the two,
        // never reaching "user"/"passt"/"none".
        let mut seen = std::collections::HashSet::new();
        let resolved: Vec<(String, Option<String>)> = stops
            .iter()
            .filter_map(|(id, bridge)| {
                let effective = if id == "bridge" {
                    bridge.clone().or_else(|| default_bridge.clone())
                } else {
                    bridge.clone()
                };
                seen.insert((id.clone(), effective.clone())).then_some((id.clone(), effective))
            })
            .collect();
        if resolved.is_empty() {
            return;
        }
        let current_idx = resolved
            .iter()
            .position(|(id, bridge)| {
                if id == "bridge" {
                    self.backend == "bridge" && bridge.as_deref() == self.bridge_name.as_deref()
                } else {
                    id == &self.backend
                }
            })
            .or_else(|| {
                // Backend is "bridge" but bridge_name doesn't match any
                // resolved stop (e.g. a manually-picked host bridge not
                // among the managed networks) — fall back to wherever the
                // generic bridge stop resolved to, so cycling still moves
                // on instead of silently staying put.
                (self.backend == "bridge")
                    .then(|| {
                        resolved
                            .iter()
                            .position(|(id, bridge)| id == "bridge" && bridge == default_bridge)
                    })
                    .flatten()
            })
            .unwrap_or(0);
        let new_idx =
            (current_idx as i32 + delta).rem_euclid(resolved.len() as i32) as usize;
        let (new_backend, new_bridge) = &resolved[new_idx];
        self.backend = new_backend.clone();
        if new_backend == "bridge" {
            self.bridge_name = new_bridge.clone();
        }
    }

    /// `true` unless this NIC's backend is "none" — governs whether the
    /// MAC field is shown in the wizard's and Network Settings' per-NIC
    /// editors.
    pub fn show_mac(&self) -> bool {
        self.backend != "none"
    }

    /// `true` when the backend is "user" or "passt" — governs whether the
    /// Forwards field is shown in the per-NIC editors.
    pub fn show_port_forwards(&self) -> bool {
        self.backend == "user" || self.backend == "passt"
    }

    /// `true` when the backend is "bridge" — governs whether the Bridge
    /// field is shown in the per-NIC editors.
    pub fn is_bridge(&self) -> bool {
        self.backend == "bridge"
    }

    /// Highest focusable field index in a per-NIC editor for this NIC's
    /// current backend: 0=adapter, 1=backend, 2=mac (when `show_mac`),
    /// 3=bridge/forwards (when `is_bridge` or `show_port_forwards`).
    pub fn max_editor_field(&self) -> usize {
        if !self.show_mac() {
            1
        } else if self.show_port_forwards() || self.is_bridge() {
            3
        } else {
            2
        }
    }

    /// Human-readable backend description, e.g. `"bridge (vmc-lan)"`.
    pub fn backend_display(&self) -> String {
        match self.backend.as_str() {
            "user" => "user/SLIRP (NAT)".to_string(),
            "passt" => "passt".to_string(),
            "bridge" => format!(
                "bridge ({})",
                self.bridge_name.as_deref().unwrap_or("qemubr0")
            ),
            "none" => "none".to_string(),
            other => other.to_string(),
        }
    }

    /// Short one-line summary, e.g. `"e1000 (bridge (vmc-lan))"` — used in
    /// NIC list rows.
    pub fn describe(&self) -> String {
        format!("{} ({})", self.model, self.backend_display())
    }
}

impl From<&crate::vm::qemu_config::NetworkConfig> for NicConfig {
    fn from(net: &crate::vm::qemu_config::NetworkConfig) -> Self {
        use crate::vm::qemu_config::NetworkBackend;
        let (backend, bridge_name) = match &net.backend {
            NetworkBackend::User => ("user".to_string(), None),
            NetworkBackend::Passt => ("passt".to_string(), None),
            NetworkBackend::Bridge(name) => ("bridge".to_string(), Some(name.clone())),
            NetworkBackend::None => ("none".to_string(), None),
        };
        Self {
            model: net.model.clone(),
            backend,
            port_forwards: net.port_forwards.clone(),
            bridge_name,
            mac_address: net.mac_address.clone(),
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
    /// Network adapters attached to this VM (at least one)
    pub network_adapters: Vec<NicConfig>,
    /// Index into `network_adapters` of the adapter currently being edited
    pub active_nic: usize,
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
            network_adapters: vec![NicConfig::default()],
            active_nic: 0,
            disk_interface: "ide".to_string(),
            enable_kvm: true,
            gl_acceleration: false,
            uefi: false,
            tpm: false,
            rtc_localtime: false,
            usb_tablet: true,
            display: "gtk".to_string(),
            extra_args: Vec::new(),
            bios_path: None,
        }
    }
}

impl WizardQemuConfig {
    /// Create from a QEMU profile
    pub fn from_profile(profile: &crate::metadata::QemuProfile) -> Self {
        let gl_acceleration = profile
            .extra_args
            .iter()
            .any(|arg| arg.contains("virtio-vga-gl") || arg.contains("gl=on"));

        Self {
            emulator: profile.emulator.clone(),
            memory_mb: profile.memory_mb,
            cpu_cores: profile.cpu_cores,
            cpu_model: profile.cpu_model.clone(),
            machine: profile.machine.clone(),
            vga: profile.vga.clone(),
            audio: profile.audio.clone(),
            network_adapters: vec![NicConfig {
                model: profile.network_model.clone(),
                backend: profile.network_backend.clone(),
                ..Default::default()
            }],
            active_nic: 0,
            disk_interface: profile.disk_interface.clone(),
            enable_kvm: profile.enable_kvm,
            gl_acceleration,
            uefi: profile.uefi,
            tpm: profile.tpm,
            rtc_localtime: profile.rtc_localtime,
            usb_tablet: profile.usb_tablet,
            display: profile.display.clone(),
            extra_args: profile.extra_args.clone(),
            bios_path: None,
        }
    }
}

/// Custom OS entry for when user selects "Other"
///
/// Some fields below are `#[allow(dead_code)]`: they are captured by the entry
/// form but not yet consumed, reserved for the planned "save custom OS to user
/// metadata" feature. They are intentional, not dead.
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
    GroupName,
}

/// Where the new VM's system disk comes from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WizardDiskSource {
    /// Create a fresh disk image in the VM directory
    #[default]
    NewImage,
    /// Copy/move an existing disk image file into the VM directory
    ExistingImage,
    /// Pass through a whole physical disk (destructive for its contents)
    PhysicalDevice,
}

impl WizardDiskSource {
    pub fn next(self) -> Self {
        match self {
            Self::NewImage => Self::ExistingImage,
            Self::ExistingImage => Self::PhysicalDevice,
            Self::PhysicalDevice => Self::NewImage,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::NewImage => Self::PhysicalDevice,
            Self::ExistingImage => Self::NewImage,
            Self::PhysicalDevice => Self::ExistingImage,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewImage => "Create New",
            Self::ExistingImage => "Use Existing",
            Self::PhysicalDevice => "Physical Disk",
        }
    }
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
    pub disk_source: WizardDiskSource,
    pub existing_disk_path: Option<PathBuf>,
    pub existing_disk_action: DiskAction,
    pub physical_disk: Option<crate::hardware::BlockDevice>,
    pub bios_rom_path: Option<PathBuf>,
    pub floppy_path: Option<PathBuf>,
    pub qemu_config: WizardQemuConfig,
    pub auto_launch: bool,
    /// Which group the new VM should join; `Default` uses its OS category.
    pub group_choice: GroupChoice,
    pub field_focus: usize,
    // Reserved for planned scroll/category-aware OS picker UI; not read yet.
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
            disk_source: WizardDiskSource::default(),
            existing_disk_path: None,
            existing_disk_action: DiskAction::Copy,
            physical_disk: None,
            bios_rom_path: None,
            floppy_path: None,
            qemu_config: WizardQemuConfig::default(),
            auto_launch: true,
            group_choice: GroupChoice::default(),
            field_focus: 0,
            os_list_scroll: 0,
            os_filter: String::new(),
            selected_category: 0,
            expanded_categories: vec!["windows".to_string(), "linux".to_string()],
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
                match self.disk_source {
                    WizardDiskSource::ExistingImage => match &self.existing_disk_path {
                        None => return Err("Please select an existing disk".to_string()),
                        Some(path) => {
                            if !path.exists() {
                                return Err(format!("Disk file not found: {}", path.display()));
                            }
                        }
                    },
                    WizardDiskSource::NewImage => {
                        if self.disk_size_gb == 0 {
                            return Err("Disk size must be greater than 0".to_string());
                        }
                        if self.disk_size_gb > 10000 {
                            return Err("Disk size cannot exceed 10TB".to_string());
                        }
                    }
                    WizardDiskSource::PhysicalDevice => {
                        let Some(disk) = &self.physical_disk else {
                            return Err("Please select a physical disk".to_string());
                        };
                        if !disk.dev_path.exists() {
                            return Err(format!(
                                "Physical disk not found: {}",
                                disk.dev_path.display()
                            ));
                        }
                        // Physical passthrough targets x86 machine types only
                        if let Some(machine) = &self.qemu_config.machine {
                            if machine.starts_with("q800") || machine.starts_with("mac99") {
                                return Err("Physical disk passthrough is not supported for this \
                                     machine type"
                                    .to_string());
                            }
                        }
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

/// Create/edit form state for the Virtual Network Manager screen
#[derive(Debug, Clone)]
pub struct VNetEditorState {
    /// Some(name) when editing an existing network (name is then read-only);
    /// None when creating a new one.
    pub original_name: Option<String>,
    pub name: String,
    pub kind: crate::vnet::VNetKind,
    pub subnet: String,
    pub dhcp: bool,
    /// 0 = name, 1 = type, 2 = subnet, 3 = DHCP toggle
    pub field_focus: usize,
    /// True while a text field is being edited
    pub editing: bool,
    pub edit_buffer: String,
    pub error: Option<String>,
    /// (name, kind, subnet, dhcp) as they were when the editor was opened,
    /// to detect unsaved edits on Esc (see `dirty`).
    baseline: (String, crate::vnet::VNetKind, String, bool),
}

impl VNetEditorState {
    pub fn new_network() -> Self {
        let name = String::new();
        let kind = crate::vnet::VNetKind::Nat;
        let subnet = "192.168.150.0/24".to_string();
        let dhcp = true;
        Self {
            original_name: None,
            baseline: (name.clone(), kind, subnet.clone(), dhcp),
            name,
            kind,
            subnet,
            dhcp,
            field_focus: 0,
            editing: false,
            edit_buffer: String::new(),
            error: None,
        }
    }

    pub fn edit(net: &crate::vnet::VirtualNetwork) -> Self {
        Self {
            original_name: Some(net.name.clone()),
            baseline: (net.name.clone(), net.kind, net.subnet.clone(), net.dhcp),
            name: net.name.clone(),
            kind: net.kind,
            subnet: net.subnet.clone(),
            dhcp: net.dhcp,
            field_focus: 1,
            editing: false,
            edit_buffer: String::new(),
            error: None,
        }
    }

    /// Whether the form differs from its state when opened.
    pub fn dirty(&self) -> bool {
        (self.name.as_str(), self.kind, self.subnet.as_str(), self.dhcp)
            != (
                self.baseline.0.as_str(),
                self.baseline.1,
                self.baseline.2.as_str(),
                self.baseline.3,
            )
    }
}

/// State for network settings editing screen
#[derive(Debug, Clone)]
pub struct NetworkSettingsState {
    /// Network adapters attached to this VM (at least one)
    pub nics: Vec<NicConfig>,
    /// `nics` as last loaded from (or saved to) launch.sh, so the NIC
    /// list's Esc can detect and confirm unsaved changes before discarding.
    pub nics_baseline: Vec<NicConfig>,
    /// Index into `nics` of the adapter currently open in the field editor
    pub active_nic: usize,
    /// Row highlighted in the NIC list, in `0..nics.len()`.
    pub list_cursor: usize,
    /// `true` while the per-NIC field editor is open for `active_nic`;
    /// `false` while showing the NIC list.
    pub editing_nic: bool,
    /// `nics[active_nic]` as it was when the field editor was opened, so
    /// Esc can discard in-progress edits instead of keeping them.
    pub nic_snapshot: Option<NicConfig>,
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
    /// Which group the imported VM should join; `Default` uses its OS category.
    pub group_choice: GroupChoice,
    pub field_focus: usize,
    pub error_message: Option<String>,
    pub editing_name: bool,
    pub editing_group_name: bool,
    pub group_name_buffer: String,
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
            group_choice: GroupChoice::default(),
            field_focus: 0,
            error_message: None,
            editing_name: false,
            editing_group_name: false,
            group_name_buffer: String::new(),
            warnings_acknowledged: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> Vec<String> {
        vec!["Linux".to_string(), "Windows".to_string()]
    }

    #[test]
    fn group_choice_label_and_target_name() {
        assert_eq!(GroupChoice::Default.label(), "Default (OS category)");
        assert_eq!(GroupChoice::Default.target_name(), None);
        assert_eq!(GroupChoice::Existing("Linux".to_string()).label(), "Linux");
        assert_eq!(
            GroupChoice::Existing("Linux".to_string()).target_name(),
            Some("Linux")
        );
        assert_eq!(GroupChoice::New("Staging".to_string()).label(), "Staging");
        assert_eq!(
            GroupChoice::New("Staging".to_string()).target_name(),
            Some("Staging")
        );
    }

    #[test]
    fn group_choice_next_cycles_through_existing_groups_then_wraps() {
        let g = groups();
        assert_eq!(
            GroupChoice::Default.next(&g),
            GroupChoice::Existing("Linux".to_string())
        );
        assert_eq!(
            GroupChoice::Existing("Linux".to_string()).next(&g),
            GroupChoice::Existing("Windows".to_string())
        );
        assert_eq!(
            GroupChoice::Existing("Windows".to_string()).next(&g),
            GroupChoice::Default
        );
    }

    #[test]
    fn group_choice_prev_cycles_backward_then_wraps() {
        let g = groups();
        assert_eq!(
            GroupChoice::Default.prev(&g),
            GroupChoice::Existing("Windows".to_string())
        );
        assert_eq!(
            GroupChoice::Existing("Windows".to_string()).prev(&g),
            GroupChoice::Existing("Linux".to_string())
        );
        assert_eq!(
            GroupChoice::Existing("Linux".to_string()).prev(&g),
            GroupChoice::Default
        );
    }

    #[test]
    fn group_choice_next_and_prev_with_no_groups_stay_default() {
        assert_eq!(GroupChoice::Default.next(&[]), GroupChoice::Default);
        assert_eq!(GroupChoice::Default.prev(&[]), GroupChoice::Default);
    }

    #[test]
    fn group_choice_new_cycles_from_the_start_like_default() {
        let g = groups();
        assert_eq!(
            GroupChoice::New("Custom".to_string()).next(&g),
            GroupChoice::Existing("Linux".to_string())
        );
    }

    #[test]
    fn max_editor_field_matches_visible_rows() {
        let none = NicConfig {
            backend: "none".to_string(),
            ..Default::default()
        };
        assert!(!none.show_mac());
        assert_eq!(none.max_editor_field(), 1); // adapter, backend only

        let user = NicConfig {
            backend: "user".to_string(),
            ..Default::default()
        };
        assert!(user.show_mac());
        assert!(user.show_port_forwards());
        assert!(!user.is_bridge());
        assert_eq!(user.max_editor_field(), 3); // + mac + forwards

        let bridge = NicConfig {
            backend: "bridge".to_string(),
            bridge_name: Some("vmc-lan".to_string()),
            ..Default::default()
        };
        assert!(bridge.show_mac());
        assert!(!bridge.show_port_forwards());
        assert!(bridge.is_bridge());
        assert_eq!(bridge.max_editor_field(), 3); // + mac + bridge

        let passt = NicConfig {
            backend: "passt".to_string(),
            ..Default::default()
        };
        assert_eq!(passt.max_editor_field(), 3); // + mac + forwards
    }

    #[test]
    fn backend_display_and_describe_cover_all_backends() {
        let bridge = NicConfig {
            model: "e1000".to_string(),
            backend: "bridge".to_string(),
            bridge_name: Some("vmc-lan".to_string()),
            ..Default::default()
        };
        assert_eq!(bridge.backend_display(), "bridge (vmc-lan)");
        assert_eq!(bridge.describe(), "e1000 (bridge (vmc-lan))");

        let no_bridge_name = NicConfig {
            backend: "bridge".to_string(),
            bridge_name: None,
            ..Default::default()
        };
        assert_eq!(no_bridge_name.backend_display(), "bridge (qemubr0)");

        let user = NicConfig {
            backend: "user".to_string(),
            ..Default::default()
        };
        assert_eq!(user.backend_display(), "user/SLIRP (NAT)");

        let none = NicConfig {
            backend: "none".to_string(),
            ..Default::default()
        };
        assert_eq!(none.backend_display(), "none");
    }

    fn stops_with_two_managed_networks() -> Vec<(String, Option<String>)> {
        vec![
            ("user".to_string(), None),
            ("passt".to_string(), None),
            ("bridge".to_string(), None),
            ("bridge".to_string(), Some("vmc-lan".to_string())),
            ("bridge".to_string(), Some("vmc-dmz".to_string())),
            ("none".to_string(), None),
        ]
    }

    #[test]
    fn cycle_backend_steps_through_managed_networks_in_order() {
        let stops = stops_with_two_managed_networks();
        let default_bridge = Some("qemubr0".to_string());
        let mut nic = NicConfig::default(); // starts on "user"

        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.backend, "passt");

        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.backend, "bridge");
        assert_eq!(nic.bridge_name, Some("qemubr0".to_string()));

        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.backend, "bridge");
        assert_eq!(nic.bridge_name, Some("vmc-lan".to_string()));

        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.bridge_name, Some("vmc-dmz".to_string()));

        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.backend, "none");

        // Wraps back to "user".
        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.backend, "user");
    }

    #[test]
    fn cycle_backend_reverse_from_managed_network_returns_to_generic_bridge() {
        let stops = stops_with_two_managed_networks();
        let default_bridge = Some("qemubr0".to_string());
        let mut nic = NicConfig {
            backend: "bridge".to_string(),
            bridge_name: Some("vmc-lan".to_string()),
            ..Default::default()
        };

        nic.cycle_backend(&stops, &default_bridge, -1);
        assert_eq!(nic.backend, "bridge");
        assert_eq!(nic.bridge_name, Some("qemubr0".to_string()));
    }

    #[test]
    fn cycle_backend_on_unmatched_bridge_falls_back_to_generic_stop() {
        // bridge_name doesn't match any managed network (e.g. a manually
        // picked host bridge) — cycling forward should move on from the
        // generic "bridge" stop, not silently stay put.
        let stops = stops_with_two_managed_networks();
        let default_bridge = Some("qemubr0".to_string());
        let mut nic = NicConfig {
            backend: "bridge".to_string(),
            bridge_name: Some("virbr0".to_string()),
            ..Default::default()
        };

        nic.cycle_backend(&stops, &default_bridge, 1);
        assert_eq!(nic.backend, "bridge");
        assert_eq!(nic.bridge_name, Some("vmc-lan".to_string()));
    }

    #[test]
    fn cycle_backend_reverse_reaches_user_when_default_bridge_matches_managed_network() {
        // Regression: when the "default" bridge (e.g. the first detected
        // system bridge) happens to be a managed network's bridge — very
        // common once any managed network is running — the generic
        // "bridge" stop and that network's stop used to resolve to the
        // same value, so cycling backward got stuck bouncing between them
        // and could never reach "user"/"passt"/"none" again.
        let stops = stops_with_two_managed_networks();
        let default_bridge = Some("vmc-lan".to_string()); // collides with a managed stop
        let mut nic = NicConfig::default(); // starts on "user"

        nic.cycle_backend(&stops, &default_bridge, 1); // -> passt
        nic.cycle_backend(&stops, &default_bridge, 1); // -> bridge (generic, resolves to vmc-lan)
        assert_eq!(nic.backend, "bridge");
        assert_eq!(nic.bridge_name, Some("vmc-lan".to_string()));

        // Cycling further forward must still reach vmc-dmz, none, and
        // wrap back to user — none of these stops are skipped or looped.
        nic.cycle_backend(&stops, &default_bridge, 1); // -> vmc-dmz
        assert_eq!(nic.bridge_name, Some("vmc-dmz".to_string()));
        nic.cycle_backend(&stops, &default_bridge, 1); // -> none
        assert_eq!(nic.backend, "none");
        nic.cycle_backend(&stops, &default_bridge, 1); // wraps -> user
        assert_eq!(nic.backend, "user");

        // And reverse from the collision point must reach "user" in one
        // step, not bounce back and forth between the two colliding stops.
        nic.cycle_backend(&stops, &default_bridge, 1); // -> passt
        nic.cycle_backend(&stops, &default_bridge, 1); // -> bridge (vmc-lan)
        nic.cycle_backend(&stops, &default_bridge, -1); // -> passt
        assert_eq!(nic.backend, "passt");
        nic.cycle_backend(&stops, &default_bridge, -1); // -> user
        assert_eq!(nic.backend, "user");
    }

    #[test]
    fn disk_image_format_strings_match_qemu_and_filenames() {
        assert_eq!(DiskImageFormat::Qcow2.as_str(), "qcow2");
        assert_eq!(DiskImageFormat::Qcow2.extension(), "qcow2");
        assert_eq!(DiskImageFormat::Raw.as_str(), "raw");
        assert_eq!(DiskImageFormat::Raw.extension(), "raw");
    }

    #[test]
    fn disk_image_format_toggles_between_supported_formats() {
        assert_eq!(DiskImageFormat::Qcow2.toggle(), DiskImageFormat::Raw);
        assert_eq!(DiskImageFormat::Raw.toggle(), DiskImageFormat::Qcow2);
    }

    #[test]
    fn disk_image_format_parses_qemu_formats_case_insensitively() {
        assert_eq!(
            DiskImageFormat::from_qemu_format("QCOW2"),
            Some(DiskImageFormat::Qcow2)
        );
        assert_eq!(
            DiskImageFormat::from_qemu_format("qcow"),
            Some(DiskImageFormat::Qcow2)
        );
        assert_eq!(
            DiskImageFormat::from_qemu_format("RAW"),
            Some(DiskImageFormat::Raw)
        );
        assert_eq!(DiskImageFormat::from_qemu_format("vmdk"), None);
    }

    #[test]
    fn disk_image_format_parses_supported_extensions() {
        assert_eq!(
            DiskImageFormat::from_path(Path::new("/vms/debian.QCOW2")),
            Some(DiskImageFormat::Qcow2)
        );
        assert_eq!(
            DiskImageFormat::from_path(Path::new("/vms/debian.img")),
            Some(DiskImageFormat::Raw)
        );
        assert_eq!(
            DiskImageFormat::from_path(Path::new("/vms/debian.vmdk")),
            None
        );
    }
}
