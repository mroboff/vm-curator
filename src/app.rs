use anyhow::Result;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::config::Config;
use crate::hardware::{MultiGpuPassthroughStatus, PciDevice, SingleGpuConfig, UsbDevice};
use crate::metadata::{AsciiArtStore, HierarchyConfig, MetadataStore, OsInfo, QemuProfileStore, SettingsHelpStore};
use crate::ui::widgets::build_visual_order;
use crate::vm::{discover_vms, BootMode, DiscoveredVm, LaunchOptions, Snapshot};

/// Application screens/views
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// Main VM list
    MainMenu,
    /// VM management options
    Management,
    /// Configuration view (planned feature)
    #[allow(dead_code)]
    Configuration,
    /// Raw launch script view
    RawScript,
    /// Detailed info (history, blurbs) - planned feature
    #[allow(dead_code)]
    DetailedInfo,
    /// Snapshot management
    Snapshots,
    /// Boot options
    BootOptions,
    /// Display options
    DisplayOptions,
    /// USB device selection
    UsbDevices,
    /// PCI device selection for passthrough
    PciPassthrough,
    /// Single GPU passthrough setup
    SingleGpuSetup,
    /// Single GPU passthrough instructions dialog
    SingleGpuInstructions,
    /// Multi-GPU passthrough setup (Looking Glass)
    MultiGpuSetup,
    /// Confirmation dialog
    Confirm(ConfirmAction),
    /// Help screen
    Help,
    /// Search/filter
    Search,
    /// File browser (for ISO selection)
    FileBrowser,
    /// Text input dialog
    TextInput(TextInputContext),
    /// Error dialog (scrollable)
    ErrorDialog,
    /// VM Creation wizard (step tracked in wizard_state)
    CreateWizard,
    /// Custom OS metadata entry (secondary form during wizard)
    CreateWizardCustomOs,
    /// ISO download progress screen (planned feature)
    #[allow(dead_code)]
    CreateWizardDownload,
    /// Application settings
    Settings,
}

/// Context for text input dialogs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputContext {
    SnapshotName,
    RenameVm,
}

/// Actions that need confirmation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    LaunchVm,
    ResetVm,
    DeleteVm,
    DeleteSnapshot(String),
    RestoreSnapshot(String),
    DiscardScriptChanges,
}

/// Input mode for text entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

/// File browser mode (determines file filter and behavior)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileBrowserMode {
    #[default]
    Iso,
    Disk,
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
            WizardStep::SelectIso => "Select Installation ISO",
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
    /// Additional QEMU arguments
    pub extra_args: Vec<String>,
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
            extra_args: Vec::new(),
        }
    }
}

impl WizardQemuConfig {
    /// Create from a QEMU profile
    pub fn from_profile(profile: &crate::metadata::QemuProfile) -> Self {
        // Check if profile has GL acceleration hints in extra_args
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
            extra_args: profile.extra_args.clone(),
        }
    }
}

/// Custom OS entry for when user selects "Other"
#[derive(Debug, Clone, Default)]
pub struct CustomOsEntry {
    /// OS identifier (e.g., "my-custom-os")
    pub id: String,
    /// Display name
    pub name: String,
    /// Publisher/developer
    pub publisher: String,
    /// Release date (YYYY-MM-DD) - planned for future save feature
    #[allow(dead_code)]
    pub release_date: Option<String>,
    /// Architecture (x86_64, i386, etc.)
    pub architecture: String,
    /// Short description (one line) - planned for future save feature
    #[allow(dead_code)]
    pub short_blurb: String,
    /// Long description (multi-paragraph) - planned for future save feature
    #[allow(dead_code)]
    pub long_blurb: String,
    /// Fun facts - planned for future save feature
    #[allow(dead_code)]
    pub fun_facts: Vec<String>,
    /// Base profile to use for QEMU defaults
    pub base_profile: String,
    /// Save to user metadata for future use - planned feature
    #[allow(dead_code)]
    pub save_to_user: bool,
}

/// State for the VM creation wizard
#[derive(Debug, Clone)]
pub struct CreateWizardState {
    /// Current wizard step
    pub step: WizardStep,
    /// VM display name (user-entered)
    pub vm_name: String,
    /// Folder name (auto-generated from vm_name)
    pub folder_name: String,
    /// Selected OS profile ID (from qemu_profiles)
    pub selected_os: Option<String>,
    /// Custom OS entry (if "Other" selected)
    pub custom_os: Option<CustomOsEntry>,
    /// ISO file path
    pub iso_path: Option<PathBuf>,
    /// Whether an ISO download is in progress
    pub iso_downloading: bool,
    /// ISO download progress (0.0 - 1.0)
    pub iso_download_progress: f32,
    /// Disk size in gigabytes (for new disk creation)
    pub disk_size_gb: u32,
    /// Whether to use an existing disk instead of creating a new one
    pub use_existing_disk: bool,
    /// Path to an existing disk to use
    pub existing_disk_path: Option<PathBuf>,
    /// Action to take with existing disk (copy or move)
    pub existing_disk_action: DiskAction,
    /// QEMU configuration
    pub qemu_config: WizardQemuConfig,
    /// Auto-launch VM after creation
    pub auto_launch: bool,
    /// Currently focused field index (for navigation)
    pub field_focus: usize,
    /// OS list scroll position - reserved for virtual scrolling
    #[allow(dead_code)]
    pub os_list_scroll: usize,
    /// OS filter/search string
    pub os_filter: String,
    /// Selected OS category index - reserved for future use
    #[allow(dead_code)]
    pub selected_category: usize,
    /// Expanded categories (by name)
    pub expanded_categories: Vec<String>,
    /// Selected item within OS list (category header or OS)
    pub os_list_selected: usize,
    /// Error message to display
    pub error_message: Option<String>,
    /// Currently editing field (for text input focus)
    pub editing_field: Option<WizardField>,
}

/// Fields that can be edited in the wizard
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Some variants reserved for future inline editing
pub enum WizardField {
    VmName,
    OsFilter,
    DiskSize,
    MemoryMb,
    CpuCores,
    CustomOsId,
    CustomOsName,
    CustomOsPublisher,
    CustomOsReleaseDate,
    CustomOsShortBlurb,
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
            iso_downloading: false,
            iso_download_progress: 0.0,
            disk_size_gb: 32,
            use_existing_disk: false,
            existing_disk_path: None,
            existing_disk_action: DiskAction::Copy,
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
        }
    }
}

impl CreateWizardState {
    /// Generate folder name from VM display name
    pub fn generate_folder_name(display_name: &str) -> String {
        display_name
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c
                } else if c.is_whitespace() || c == '_' {
                    '-'
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Update folder name based on selected OS profile ID
    /// Uses the profile ID as base (e.g., "linux-endeavouros") for proper hierarchy matching
    /// If a folder with that name already exists, appends -2, -3, etc.
    pub fn update_folder_name(&mut self, library_path: &std::path::Path) {
        let base_name = if let Some(ref os_id) = self.selected_os {
            // Use the profile ID as the folder name for proper categorization
            os_id.clone()
        } else {
            // Fallback to generating from display name for custom OSes
            Self::generate_folder_name(&self.vm_name)
        };

        // Check if folder already exists, and if so, find an available suffix
        self.folder_name = Self::find_available_folder_name(library_path, &base_name);
    }

    /// Find an available folder name by appending numeric suffixes if needed
    /// e.g., "windows-10" -> "windows-10-2" -> "windows-10-3"
    fn find_available_folder_name(library_path: &std::path::Path, base_name: &str) -> String {
        let first_candidate = library_path.join(base_name);
        if !first_candidate.exists() {
            return base_name.to_string();
        }

        // Folder exists, try with numeric suffixes
        let mut suffix = 2;
        loop {
            let candidate_name = format!("{}-{}", base_name, suffix);
            let candidate_path = library_path.join(&candidate_name);
            if !candidate_path.exists() {
                return candidate_name;
            }
            suffix += 1;
            // Safety limit to prevent infinite loop
            if suffix > 1000 {
                return candidate_name;
            }
        }
    }

    /// Apply profile settings to the wizard state
    pub fn apply_profile(&mut self, profile: &crate::metadata::QemuProfile) {
        self.disk_size_gb = profile.disk_size_gb;
        self.qemu_config = WizardQemuConfig::from_profile(profile);
    }

    /// Check if the wizard can proceed to the next step
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
            WizardStep::SelectIso => {
                // ISO is optional - user can configure later
                Ok(())
            }
            WizardStep::ConfigureDisk => {
                if self.use_existing_disk {
                    // Validate existing disk path
                    match &self.existing_disk_path {
                        None => return Err("Please select an existing disk".to_string()),
                        Some(path) => {
                            if !path.exists() {
                                return Err(format!("Disk file not found: {}", path.display()));
                            }
                        }
                    }
                } else {
                    // Validate new disk size
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

    /// Toggle a category's expanded state
    pub fn toggle_category(&mut self, category: &str) {
        if let Some(pos) = self.expanded_categories.iter().position(|c| c == category) {
            self.expanded_categories.remove(pos);
        } else {
            self.expanded_categories.push(category.to_string());
        }
    }

    /// Check if a category is expanded
    pub fn is_category_expanded(&self, category: &str) -> bool {
        self.expanded_categories.iter().any(|c| c == category)
    }
}

/// Application state
pub struct App {
    /// Current screen
    pub screen: Screen,
    /// Screen history for back navigation
    pub screen_stack: Vec<Screen>,
    /// Application configuration
    pub config: Config,
    /// Discovered VMs
    pub vms: Vec<DiscoveredVm>,
    /// Currently selected VM index
    pub selected_vm: usize,
    /// OS metadata store
    pub metadata: MetadataStore,
    /// ASCII art store
    pub ascii_art: AsciiArtStore,
    /// Hierarchy configuration for VM categorization
    pub hierarchy: HierarchyConfig,
    /// Snapshots for current VM (cached)
    pub snapshots: Vec<Snapshot>,
    /// Selected snapshot index
    pub selected_snapshot: usize,
    /// USB devices (cached)
    pub usb_devices: Vec<UsbDevice>,
    /// Selected USB devices for passthrough
    pub selected_usb_devices: Vec<usize>,
    /// PCI devices (cached)
    pub pci_devices: Vec<PciDevice>,
    /// Selected PCI devices for passthrough
    pub selected_pci_devices: Vec<usize>,
    /// Multi-GPU passthrough status (prerequisites)
    pub multi_gpu_status: Option<MultiGpuPassthroughStatus>,
    /// Selected management menu item
    pub selected_menu_item: usize,
    /// Current boot mode
    pub boot_mode: BootMode,
    /// Search query
    pub search_query: String,
    /// Input mode
    pub input_mode: InputMode,
    /// Filtered VM indices (for search)
    pub filtered_indices: Vec<usize>,
    /// Visual order of VMs (maps visual position to filtered_idx for hierarchy navigation)
    pub visual_order: Vec<usize>,
    /// Status message
    pub status_message: Option<String>,
    /// When status message was set (for auto-clearing)
    pub status_time: Option<Instant>,
    /// Whether the app should quit
    pub should_quit: bool,
    /// File browser current directory
    pub file_browser_dir: PathBuf,
    /// File browser entries (directories first, then files)
    pub file_browser_entries: Vec<FileBrowserEntry>,
    /// File browser selected index
    pub file_browser_selected: usize,
    /// File browser mode (determines file filter and behavior)
    pub file_browser_mode: FileBrowserMode,
    /// Text input buffer (for dialogs)
    pub text_input_buffer: String,
    /// Channel for background operation results
    pub background_rx: Receiver<BackgroundResult>,
    /// Sender for background operations (clone this for threads)
    pub background_tx: Sender<BackgroundResult>,
    /// Whether a background operation is in progress
    pub loading: bool,
    /// Error dialog content (for detailed errors)
    pub error_detail: Option<String>,
    /// Error dialog scroll position
    pub error_scroll: u16,
    /// Right panel scroll position (for info panel)
    pub info_scroll: u16,
    /// Raw script view scroll position
    pub raw_script_scroll: u16,
    /// Script editor buffer (lines of text)
    pub script_editor_lines: Vec<String>,
    /// Script editor cursor position (line, column)
    pub script_editor_cursor: (usize, usize),
    /// Whether the script has been modified
    pub script_editor_modified: bool,
    /// Horizontal scroll offset for the editor
    pub script_editor_h_scroll: usize,
    /// QEMU profiles for VM creation
    pub qemu_profiles: QemuProfileStore,
    /// Settings help text store
    pub settings_help: SettingsHelpStore,
    /// VM creation wizard state
    pub wizard_state: Option<CreateWizardState>,
    /// Settings screen selected item
    pub settings_selected: usize,
    /// Settings screen editing mode
    pub settings_editing: bool,
    /// Settings screen edit buffer (for text fields)
    pub settings_edit_buffer: String,
    /// GPU passthrough validation result for settings screen
    pub settings_gpu_validation: Option<crate::ui::screens::settings::GpuValidationResult>,

    // === Single GPU Passthrough ===
    /// Single GPU passthrough configuration
    pub single_gpu_config: Option<SingleGpuConfig>,
    /// Selected field in single GPU setup screen
    pub single_gpu_selected_field: usize,
    /// Whether to show the instructions dialog
    pub single_gpu_show_instructions: bool,
}

/// Entry in file browser
#[derive(Debug, Clone)]
pub struct FileBrowserEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Background operation result
pub enum BackgroundResult {
    SnapshotCreated { name: String, success: bool, error: Option<String> },
    SnapshotRestored { name: String, success: bool, error: Option<String> },
    SnapshotDeleted { name: String, success: bool, error: Option<String> },
    /// Reserved for async snapshot loading
    #[allow(dead_code)]
    SnapshotsLoaded { snapshots: Vec<Snapshot>, error: Option<String> },
}

impl App {
    /// Create a new application instance with progress callback
    pub fn new_with_progress<F>(config: Config, progress: F) -> Result<Self>
    where
        F: Fn(usize, usize, &str),
    {
        const TOTAL_STEPS: usize = 6;

        // Step 1: Discover VMs
        progress(1, TOTAL_STEPS, "Discovering VMs...");
        let vms = discover_vms(&config.vm_library_path)?;
        progress(1, TOTAL_STEPS, &format!("Found {} VMs", vms.len()));

        // Step 2: Load metadata
        progress(2, TOTAL_STEPS, "Loading OS metadata...");
        let mut metadata = MetadataStore::load_embedded();
        if let Ok(user_metadata) = MetadataStore::load_from_dir(&config.metadata_path) {
            metadata.merge(user_metadata);
        }

        // Step 3: Load ASCII art
        progress(3, TOTAL_STEPS, "Loading ASCII art...");
        let mut ascii_art = AsciiArtStore::load_embedded();
        let user_art = AsciiArtStore::load_from_dir(&config.ascii_art_path);
        ascii_art.merge(user_art);

        // Step 4: Load hierarchy config
        progress(4, TOTAL_STEPS, "Loading hierarchy...");
        let hierarchy = HierarchyConfig::load_embedded();

        // Step 5: Load QEMU profiles
        progress(5, TOTAL_STEPS, "Loading QEMU profiles...");
        let mut qemu_profiles = QemuProfileStore::load_embedded();
        let config_dir = Config::config_file_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let user_profiles_path = config_dir.join("qemu_profiles.toml");
        qemu_profiles.load_user_overrides(&user_profiles_path);

        // Load settings help text
        let mut settings_help = SettingsHelpStore::load_embedded();
        let user_help_path = config_dir.join("settings_help.toml");
        settings_help.load_user_overrides(&user_help_path);

        // Step 6: Build visual order
        progress(6, TOTAL_STEPS, "Building VM list...");
        let filtered_indices: Vec<usize> = (0..vms.len()).collect();
        let visual_order = build_visual_order(&vms, &filtered_indices, &hierarchy, &metadata);
        let (background_tx, background_rx) = mpsc::channel();

        Ok(Self {
            screen: Screen::MainMenu,
            screen_stack: Vec::new(),
            config,
            vms,
            selected_vm: 0,
            metadata,
            ascii_art,
            hierarchy,
            snapshots: Vec::new(),
            selected_snapshot: 0,
            usb_devices: Vec::new(),
            selected_usb_devices: Vec::new(),
            pci_devices: Vec::new(),
            selected_pci_devices: Vec::new(),
            multi_gpu_status: None,
            selected_menu_item: 0,
            boot_mode: BootMode::Normal,
            search_query: String::new(),
            input_mode: InputMode::Normal,
            filtered_indices,
            visual_order,
            status_message: None,
            status_time: None,
            should_quit: false,
            file_browser_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
            file_browser_entries: Vec::new(),
            file_browser_selected: 0,
            file_browser_mode: FileBrowserMode::Iso,
            text_input_buffer: String::new(),
            background_rx,
            background_tx,
            loading: false,
            error_detail: None,
            error_scroll: 0,
            info_scroll: 0,
            raw_script_scroll: 0,
            script_editor_lines: Vec::new(),
            script_editor_cursor: (0, 0),
            script_editor_modified: false,
            script_editor_h_scroll: 0,
            qemu_profiles,
            settings_help,
            wizard_state: None,
            settings_selected: 0,
            settings_editing: false,
            settings_edit_buffer: String::new(),
            settings_gpu_validation: None,

            // Single GPU Passthrough
            single_gpu_config: None,
            single_gpu_selected_field: 0,
            single_gpu_show_instructions: false,
        })
    }

    /// Get the currently selected VM
    pub fn selected_vm(&self) -> Option<&DiscoveredVm> {
        if self.visual_order.is_empty() {
            return None;
        }
        // selected_vm is an index into visual_order
        // visual_order[selected_vm] gives the filtered_idx
        // filtered_indices[filtered_idx] gives the actual vm index
        let filtered_idx = self.visual_order.get(self.selected_vm)?;
        let actual_idx = self.filtered_indices.get(*filtered_idx)?;
        self.vms.get(*actual_idx)
    }

    /// Get OS info for the selected VM
    pub fn selected_vm_info(&self) -> Option<OsInfo> {
        let vm = self.selected_vm()?;
        self.metadata
            .get(&vm.id)
            .cloned()
            .or_else(|| Some(crate::metadata::default_os_info(&vm.id)))
    }

    /// Get ASCII art for the selected VM
    pub fn selected_vm_ascii(&self) -> &str {
        self.selected_vm()
            .map(|vm| self.ascii_art.get_or_fallback(&vm.id))
            .unwrap_or("")
    }

    /// Navigate to a new screen
    pub fn push_screen(&mut self, screen: Screen) {
        self.screen_stack.push(self.screen.clone());
        self.screen = screen;
        self.selected_menu_item = 0;
    }

    /// Go back to the previous screen
    pub fn pop_screen(&mut self) {
        if let Some(prev) = self.screen_stack.pop() {
            self.screen = prev;
        }
    }

    /// Move selection up in VM list (follows visual/hierarchy order)
    pub fn select_prev(&mut self) {
        if !self.visual_order.is_empty() && self.selected_vm > 0 {
            self.selected_vm -= 1;
            self.info_scroll = 0; // Reset scroll when VM changes
        }
    }

    /// Move selection down in VM list (follows visual/hierarchy order)
    pub fn select_next(&mut self) {
        if !self.visual_order.is_empty() && self.selected_vm < self.visual_order.len() - 1 {
            self.selected_vm += 1;
            self.info_scroll = 0; // Reset scroll when VM changes
        }
    }

    /// Move selection up in menu
    pub fn menu_prev(&mut self) {
        if self.selected_menu_item > 0 {
            self.selected_menu_item -= 1;
        }
    }

    /// Move selection down in menu
    pub fn menu_next(&mut self, max_items: usize) {
        if self.selected_menu_item < max_items.saturating_sub(1) {
            self.selected_menu_item += 1;
        }
    }

    /// Update search filter
    pub fn update_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_indices = (0..self.vms.len()).collect();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_indices = self
                .vms
                .iter()
                .enumerate()
                .filter(|(_, vm)| {
                    vm.display_name().to_lowercase().contains(&query)
                        || vm.id.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Rebuild visual order for hierarchy navigation
        self.visual_order = build_visual_order(&self.vms, &self.filtered_indices, &self.hierarchy, &self.metadata);

        // Reset selection if out of bounds
        if self.selected_vm >= self.visual_order.len() {
            self.selected_vm = self.visual_order.len().saturating_sub(1);
        }
    }

    /// Refresh VM list
    pub fn refresh_vms(&mut self) -> Result<()> {
        self.vms = discover_vms(&self.config.vm_library_path)?;
        self.update_filter();
        Ok(())
    }

    /// Load snapshots for the current VM
    pub fn load_snapshots(&mut self) -> Result<()> {
        self.snapshots.clear();
        self.selected_snapshot = 0;

        if let Some(vm) = self.selected_vm() {
            if let Some(disk) = vm.config.primary_disk() {
                if disk.format.supports_snapshots() {
                    self.snapshots = crate::vm::list_snapshots(&disk.path)?;
                }
            }
        }

        Ok(())
    }

    /// Load USB devices
    pub fn load_usb_devices(&mut self) -> Result<()> {
        self.usb_devices = crate::hardware::enumerate_usb_devices()?;
        self.selected_usb_devices.clear();
        Ok(())
    }

    /// Toggle USB device selection
    pub fn toggle_usb_device(&mut self, index: usize) {
        if let Some(pos) = self.selected_usb_devices.iter().position(|&i| i == index) {
            self.selected_usb_devices.remove(pos);
        } else {
            self.selected_usb_devices.push(index);
        }
    }

    /// Load PCI devices
    pub fn load_pci_devices(&mut self) -> Result<()> {
        self.pci_devices = crate::hardware::enumerate_pci_devices()?;
        self.selected_pci_devices.clear();
        self.multi_gpu_status = Some(crate::hardware::check_multi_gpu_passthrough_status());
        Ok(())
    }

    /// Toggle PCI device selection
    pub fn toggle_pci_device(&mut self, index: usize) {
        // Don't allow selecting boot VGA
        if let Some(device) = self.pci_devices.get(index) {
            if device.is_boot_vga {
                return;
            }
        }

        if let Some(pos) = self.selected_pci_devices.iter().position(|&i| i == index) {
            self.selected_pci_devices.remove(pos);
        } else {
            self.selected_pci_devices.push(index);
        }
    }

    /// Auto-select a GPU and its paired audio device
    pub fn auto_select_gpu(&mut self, gpu_index: usize) {
        // Clear existing selection
        self.selected_pci_devices.clear();

        if let Some(gpu) = self.pci_devices.get(gpu_index) {
            if gpu.is_boot_vga {
                return;
            }

            // Select the GPU
            self.selected_pci_devices.push(gpu_index);

            // Try to find and select the paired audio device
            if let Some(audio) = crate::hardware::find_gpu_audio_pair(gpu, &self.pci_devices) {
                if let Some(audio_idx) = self.pci_devices.iter().position(|d| d.address == audio.address) {
                    self.selected_pci_devices.push(audio_idx);
                }
            }
        }
    }

    /// Reload the selected VM's raw script from disk
    pub fn reload_selected_vm_script(&mut self) {
        if self.visual_order.is_empty() {
            return;
        }
        if let Some(filtered_idx) = self.visual_order.get(self.selected_vm) {
            if let Some(actual_idx) = self.filtered_indices.get(*filtered_idx) {
                if let Some(vm) = self.vms.get_mut(*actual_idx) {
                    if let Ok(content) = std::fs::read_to_string(&vm.launch_script) {
                        vm.config.raw_script = content;
                    }
                }
            }
        }
    }

    /// Get launch options based on current state
    pub fn get_launch_options(&self) -> LaunchOptions {
        let usb_devices = self
            .selected_usb_devices
            .iter()
            .filter_map(|&i| self.usb_devices.get(i))
            .map(|d| crate::vm::UsbPassthrough {
                vendor_id: d.vendor_id,
                product_id: d.product_id,
                usb_version: d.usb_version,
            })
            .collect();

        LaunchOptions {
            boot_mode: self.boot_mode.clone(),
            extra_args: Vec::new(),
            usb_devices,
        }
    }

    /// Set a status message (auto-clears after 5 seconds)
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_time = Some(Instant::now());
    }

    /// Show a detailed error in a scrollable dialog
    pub fn show_error(&mut self, error: impl Into<String>) {
        self.error_detail = Some(error.into());
        self.error_scroll = 0;
        self.push_screen(Screen::ErrorDialog);
    }

    /// Clear status message
    pub fn clear_status(&mut self) {
        self.status_message = None;
        self.status_time = None;
    }

    /// Check and clear status if expired (call in event loop)
    pub fn check_status_expiry(&mut self) {
        if let Some(time) = self.status_time {
            if time.elapsed().as_secs() >= 5 {
                self.clear_status();
            }
        }
    }

    /// Check for background operation results (call in event loop)
    pub fn check_background_results(&mut self) {
        // Non-blocking check for results
        while let Ok(result) = self.background_rx.try_recv() {
            self.loading = false;
            match result {
                BackgroundResult::SnapshotCreated { name, success, error } => {
                    if success {
                        self.set_status(format!("Created snapshot: {}", name));
                        // Reload snapshots
                        let _ = self.load_snapshots();
                    } else if let Some(e) = error {
                        self.set_status(format!("Error creating snapshot: {}", e));
                    }
                }
                BackgroundResult::SnapshotRestored { name, success, error } => {
                    if success {
                        self.set_status(format!("Restored snapshot: {}", name));
                    } else if let Some(e) = error {
                        self.set_status(format!("Error restoring snapshot: {}", e));
                    }
                }
                BackgroundResult::SnapshotDeleted { name, success, error } => {
                    if success {
                        self.set_status(format!("Deleted snapshot: {}", name));
                        let _ = self.load_snapshots();
                    } else if let Some(e) = error {
                        self.set_status(format!("Error deleting snapshot: {}", e));
                    }
                }
                BackgroundResult::SnapshotsLoaded { snapshots, error } => {
                    if let Some(e) = error {
                        self.set_status(format!("Error loading snapshots: {}", e));
                    } else {
                        self.snapshots = snapshots;
                        self.selected_snapshot = 0;
                    }
                }
            }
        }
    }

    /// Load file browser entries for current directory
    pub fn load_file_browser(&mut self, mode: FileBrowserMode) {
        self.file_browser_mode = mode;
        self.file_browser_entries.clear();
        self.file_browser_selected = 0;

        // Determine file extensions to filter by based on mode
        let extensions: &[&str] = match mode {
            FileBrowserMode::Iso => &[".iso", ".ISO"],
            FileBrowserMode::Disk => &[".qcow2", ".QCOW2", ".qcow", ".QCOW"],
        };

        // Add parent directory entry if not at root
        if let Some(parent) = self.file_browser_dir.parent() {
            self.file_browser_entries.push(FileBrowserEntry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                is_dir: true,
            });
        }

        // Read directory entries
        if let Ok(entries) = std::fs::read_dir(&self.file_browser_dir) {
            let mut dirs = Vec::new();
            let mut files = Vec::new();

            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Skip hidden files
                    if name.starts_with('.') {
                        continue;
                    }
                    let entry = FileBrowserEntry {
                        name,
                        path: entry.path(),
                        is_dir: metadata.is_dir(),
                    };
                    if metadata.is_dir() {
                        dirs.push(entry);
                    } else if extensions.iter().any(|ext| entry.name.ends_with(ext)) {
                        files.push(entry);
                    }
                }
            }

            // Sort alphabetically
            dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

            self.file_browser_entries.extend(dirs);
            self.file_browser_entries.extend(files);
        }
    }

    /// Navigate into directory or select file in file browser
    pub fn file_browser_enter(&mut self) -> Option<PathBuf> {
        if let Some(entry) = self.file_browser_entries.get(self.file_browser_selected) {
            if entry.is_dir {
                self.file_browser_dir = entry.path.clone();
                // Preserve the current mode when navigating directories
                let mode = self.file_browser_mode;
                self.load_file_browser(mode);
                None
            } else {
                // Return selected file
                Some(entry.path.clone())
            }
        } else {
            None
        }
    }

    /// Move selection up in file browser
    pub fn file_browser_prev(&mut self) {
        if self.file_browser_selected > 0 {
            self.file_browser_selected -= 1;
        }
    }

    /// Move selection down in file browser
    pub fn file_browser_next(&mut self) {
        if self.file_browser_selected < self.file_browser_entries.len().saturating_sub(1) {
            self.file_browser_selected += 1;
        }
    }

    /// Load the selected VM's script into the editor
    pub fn load_script_into_editor(&mut self) {
        if let Some(vm) = self.selected_vm() {
            self.script_editor_lines = vm.config.raw_script.lines().map(String::from).collect();
            // Ensure at least one line exists
            if self.script_editor_lines.is_empty() {
                self.script_editor_lines.push(String::new());
            }
            self.script_editor_cursor = (0, 0);
            self.script_editor_modified = false;
            self.script_editor_h_scroll = 0;
            self.raw_script_scroll = 0;
        }
    }

    /// Save the editor content back to the launch.sh file
    pub fn save_script_from_editor(&mut self) -> Result<()> {
        // Get the launch script path before we need mutable access
        let launch_script_path = self.selected_vm()
            .map(|vm| vm.launch_script.clone())
            .ok_or_else(|| anyhow::anyhow!("No VM selected"))?;

        let content = self.script_editor_lines.join("\n");
        // Ensure the file ends with a newline
        let content = if content.ends_with('\n') {
            content
        } else {
            format!("{}\n", content)
        };

        std::fs::write(&launch_script_path, &content)?;

        // Update the cached raw_script in the VM
        self.reload_selected_vm_script();
        self.script_editor_modified = false;

        // Re-parse the VM config since the script changed
        if let Ok(vms) = discover_vms(&self.config.vm_library_path) {
            self.vms = vms;
            self.update_filter();
        }

        // Regenerate single-GPU scripts if they exist
        if let Some(vm) = self.selected_vm() {
            if crate::hardware::scripts_exist(&vm.path) {
                // Try with in-memory config first, fall back to saved config
                // Ignore errors - the main save succeeded
                let _ = if let Some(config) = self.single_gpu_config.as_ref() {
                    crate::vm::single_gpu_scripts::regenerate_if_exists(vm, config)
                } else {
                    crate::vm::single_gpu_scripts::regenerate_from_saved_config(vm)
                };
            }
        }

        Ok(())
    }

    // =========================================================================
    // VM Creation Wizard Methods
    // =========================================================================

    /// Start the VM creation wizard
    pub fn start_create_wizard(&mut self) {
        let mut state = CreateWizardState::default();

        // Apply user config defaults
        state.disk_size_gb = self.config.default_disk_size_gb;
        state.qemu_config.memory_mb = self.config.default_memory_mb;
        state.qemu_config.cpu_cores = self.config.default_cpu_cores;
        state.qemu_config.enable_kvm = self.config.default_enable_kvm;
        state.qemu_config.display = self.config.default_display.clone();

        self.wizard_state = Some(state);
        self.push_screen(Screen::CreateWizard);
    }

    /// Cancel the wizard and return to main menu
    pub fn cancel_wizard(&mut self) {
        self.wizard_state = None;
        // Pop all wizard-related screens
        while matches!(
            self.screen,
            Screen::CreateWizard | Screen::CreateWizardCustomOs | Screen::CreateWizardDownload
        ) {
            self.pop_screen();
        }
    }

    /// Move to the next wizard step
    pub fn wizard_next_step(&mut self) -> Result<(), String> {
        if let Some(ref mut state) = self.wizard_state {
            // Validate current step
            state.can_proceed()?;

            // Move to next step
            if let Some(next) = state.step.next() {
                state.step = next;
                state.field_focus = 0;
                state.error_message = None;
                Ok(())
            } else {
                Err("Already at final step".to_string())
            }
        } else {
            Err("Wizard not active".to_string())
        }
    }

    /// Move to the previous wizard step
    pub fn wizard_prev_step(&mut self) {
        if let Some(ref mut state) = self.wizard_state {
            if let Some(prev) = state.step.prev() {
                state.step = prev;
                state.field_focus = 0;
                state.error_message = None;
            }
        }
    }

    /// Select an OS profile in the wizard
    pub fn wizard_select_os(&mut self, os_id: &str) {
        let library_path = self.config.vm_library_path.clone();

        // Get full display name from metadata (e.g., "CachyOS (rolling)")
        // Fall back to profile's display_name if not in metadata
        let new_display_name = self.metadata.get(os_id)
            .and_then(|info| info.display_name.clone())
            .or_else(|| self.qemu_profiles.get(os_id).map(|p| p.display_name.clone()))
            .unwrap_or_else(|| os_id.to_string());

        // Get the previous OS's display name (if any) to check if user customized the name
        let previous_default_name = self.wizard_state.as_ref()
            .and_then(|s| s.selected_os.as_ref())
            .and_then(|prev_id| {
                self.metadata.get(prev_id)
                    .and_then(|info| info.display_name.clone())
                    .or_else(|| self.qemu_profiles.get(prev_id).map(|p| p.display_name.clone()))
            });

        if let Some(ref mut state) = self.wizard_state {
            state.selected_os = Some(os_id.to_string());
            state.custom_os = None;

            // Apply profile settings
            if let Some(profile) = self.qemu_profiles.get(os_id) {
                state.apply_profile(profile);

                // Only update VM name if:
                // 1. Name is empty, OR
                // 2. Name matches the previous OS's default (user hasn't customized it)
                let should_update_name = state.vm_name.is_empty()
                    || previous_default_name.as_ref().map(|n| n == &state.vm_name).unwrap_or(false);

                if should_update_name {
                    state.vm_name = new_display_name;
                }
                state.update_folder_name(&library_path);
            }
        }
    }

    /// Set the wizard to use a custom OS
    pub fn wizard_use_custom_os(&mut self) {
        if let Some(ref mut state) = self.wizard_state {
            state.selected_os = None;
            state.custom_os = Some(CustomOsEntry {
                base_profile: "generic-other".to_string(),
                architecture: "x86_64".to_string(),
                ..Default::default()
            });
            self.push_screen(Screen::CreateWizardCustomOs);
        }
    }

    /// Get the full path where the new VM will be created
    pub fn wizard_vm_path(&self) -> Option<PathBuf> {
        self.wizard_state
            .as_ref()
            .filter(|s| !s.folder_name.is_empty())
            .map(|s| self.config.vm_library_path.join(&s.folder_name))
    }

    /// Get the selected OS profile in the wizard
    pub fn wizard_selected_profile(&self) -> Option<&crate::metadata::QemuProfile> {
        self.wizard_state
            .as_ref()
            .and_then(|s| s.selected_os.as_ref())
            .and_then(|os_id| self.qemu_profiles.get(os_id))
    }
}
