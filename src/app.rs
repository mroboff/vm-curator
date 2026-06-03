use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::commands::qemu_system::NetworkCapabilities;
use crate::config::Config;
use crate::hardware::{MultiGpuPassthroughStatus, PciDevice, SingleGpuConfig, UsbDevice};
use crate::metadata::{AsciiArtStore, HierarchyConfig, MetadataStore, OsInfo, QemuProfileStore, SettingsHelpStore, SharedFoldersHelpStore};
use crate::ui::widgets::build_visual_order;
use crate::vm::{discover_vms, BootMode, DiscoveredVm, LaunchOptions, QemuProcess, SharedFolder, Snapshot};
pub use crate::wizard_types::*;

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
    /// Shared folder management (virtio-9p)
    SharedFolders,
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
    /// Network settings (backend + port forwarding)
    NetworkSettings,
    /// Application settings
    Settings,
    /// VM Import wizard
    ImportWizard,
    /// Notes editor
    EditNotes,
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
    DiscardNotesChanges,
    StopVm,
    ForceStopVm,
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
    RecoveryImage,
    Disk,
    Directory,
    ImportConfig,
    Bios,
    Floppy,
}

// DiskAction, WizardStep, WizardQemuConfig, CreateWizardState, NetworkSettingsState,
// AddingPortForward, AddPfStep, ImportSource, ImportDiskAction, ImportStep,
// ImportableVm, ImportWizardState and related types are defined in wizard_types.rs
// and imported above via `use crate::wizard_types::*`.

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
    /// Shared folders for the current VM
    pub shared_folders: Vec<SharedFolder>,
    /// Selected shared folder index
    pub shared_folder_selected: usize,
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
    /// Shared folders help text store
    pub shared_folders_help: SharedFoldersHelpStore,
    /// VM creation wizard state
    pub wizard_state: Option<CreateWizardState>,
    /// VM import wizard state
    pub import_state: Option<ImportWizardState>,
    /// Settings screen selected item
    pub settings_selected: usize,
    /// Settings screen editing mode
    pub settings_editing: bool,
    /// Settings screen edit buffer (for text fields)
    pub settings_edit_buffer: String,
    /// GPU passthrough validation result for settings screen
    pub settings_gpu_validation: Option<crate::ui::screens::settings::GpuValidationResult>,
    /// Cached display capabilities per emulator (populated at startup)
    pub display_capabilities: HashMap<String, Vec<String>>,

    // === VM Process Monitoring ===
    /// Receives QEMU process info from background detection thread
    pub vm_status_rx: Receiver<Vec<QemuProcess>>,
    /// Map of vm_id -> PID for currently running VMs
    pub running_vms: HashMap<String, u32>,
    /// Map of vm_id -> when SIGTERM was sent (for force-stop timeout)
    pub stopping_vms: HashMap<String, Instant>,

    // === Single GPU Passthrough ===
    /// Single GPU passthrough configuration
    pub single_gpu_config: Option<SingleGpuConfig>,
    /// Selected field in single GPU setup screen
    pub single_gpu_selected_field: usize,
    /// Whether to show the instructions dialog
    pub single_gpu_show_instructions: bool,

    // === Networking ===
    /// Detected network capabilities (passt, bridge helper, etc.)
    pub network_caps: NetworkCapabilities,
    /// Network settings editing state
    pub network_settings_state: Option<NetworkSettingsState>,
    /// Whether the wizard port forward editor is active
    pub wizard_editing_port_forwards: bool,
    /// Wizard port forward editor selection index
    pub wizard_pf_selected: usize,
    /// Wizard port forward adding state
    pub wizard_adding_pf: Option<AddingPortForward>,
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

        // Load shared folders help text
        let mut shared_folders_help = SharedFoldersHelpStore::load_embedded();
        shared_folders_help.load_user_overrides(&config_dir.join("shared_folders_help.toml"));

        // Step 6: Build visual order and detect display capabilities
        progress(6, TOTAL_STEPS, "Building VM list...");
        let filtered_indices: Vec<usize> = (0..vms.len()).collect();
        let visual_order = build_visual_order(&vms, &filtered_indices, &hierarchy, &metadata);
        let (background_tx, background_rx) = mpsc::channel();

        // Detect network capabilities
        let network_caps = crate::commands::qemu_system::detect_network_capabilities();

        // Detect display capabilities for each available emulator
        let mut display_capabilities = HashMap::new();
        for emulator in crate::commands::qemu_system::list_available_emulators() {
            let displays = crate::commands::qemu_system::get_supported_displays(&emulator);
            if !displays.is_empty() {
                display_capabilities.insert(emulator, displays);
            }
        }

        // Spawn background VM status detection thread
        let (vm_status_tx, vm_status_rx) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let processes = crate::vm::detect_qemu_processes();
                if vm_status_tx.send(processes).is_err() {
                    break; // Receiver dropped (app exited)
                }
            }
        });

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
            shared_folders: Vec::new(),
            shared_folder_selected: 0,
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
            shared_folders_help,
            wizard_state: None,
            import_state: None,
            settings_selected: 0,
            settings_editing: false,
            settings_edit_buffer: String::new(),
            settings_gpu_validation: None,
            display_capabilities,

            // VM Process Monitoring
            vm_status_rx,
            running_vms: HashMap::new(),
            stopping_vms: HashMap::new(),

            // Single GPU Passthrough
            single_gpu_config: None,
            single_gpu_selected_field: 0,
            single_gpu_show_instructions: false,

            // Networking
            network_caps,
            network_settings_state: None,
            wizard_editing_port_forwards: false,
            wizard_pf_selected: 0,
            wizard_adding_pf: None,
        })
    }

    /// Get display options for an emulator, filtered and ordered.
    ///
    /// Returns detected display backends for the emulator, preferring `spice-app`
    /// over `spice`. Falls back to a default list if detection returned nothing.
    pub fn get_display_options_for_emulator(&self, emulator: &str) -> Vec<String> {
        // Preferred order of display backends
        let preferred_order = ["gtk", "sdl", "spice-app", "vnc", "none"];

        if let Some(detected) = self.display_capabilities.get(emulator) {
            let mut result = Vec::new();
            // Add backends in preferred order if they were detected
            for &pref in &preferred_order {
                if detected.iter().any(|d| d == pref) {
                    result.push(pref.to_string());
                }
            }
            // Add any remaining detected backends not in preferred order
            for d in detected {
                if !result.iter().any(|r| r == d) && d != "spice" {
                    result.push(d.clone());
                }
            }
            if !result.is_empty() {
                return result;
            }
        }

        // Fallback: default list
        preferred_order.iter().map(|s| s.to_string()).collect()
    }

    /// Get available network backend options based on detected capabilities
    pub fn get_network_backend_options(&self) -> Vec<(&str, &str)> {
        let mut options = vec![
            ("user", "User/SLIRP (NAT) - Default, works everywhere"),
        ];
        if self.network_caps.passt_available {
            options.push(("passt", "passt - Fast NAT, ping works"));
        }
        if self.network_caps.bridge_helper_path.is_some() {
            if !self.network_caps.system_bridges.is_empty() && self.network_caps.bridge_helper_configured {
                options.push(("bridge", "Bridge - Full network, own IP"));
            } else {
                options.push(("bridge", "Bridge - Requires one-time setup"));
            }
        }
        options.push(("none", "None - No networking"));
        options
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

    /// Restore PCI device selections from the VM's saved launch.sh config
    pub fn restore_pci_selections(&mut self) {
        let saved_args = match self.selected_vm() {
            Some(vm) => crate::vm::load_pci_passthrough(vm),
            None => return,
        };
        self.selected_pci_devices.clear();
        for arg in &saved_args {
            if let Some(host_start) = arg.find("host=") {
                let addr_start = host_start + 5;
                let addr = arg[addr_start..]
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .next()
                    .unwrap_or("");
                for (i, dev) in self.pci_devices.iter().enumerate() {
                    if dev.address == addr {
                        self.selected_pci_devices.push(i);
                        break;
                    }
                }
            }
        }
    }

    /// Load shared folders for the current VM
    pub fn load_shared_folders(&mut self) {
        self.shared_folders.clear();
        self.shared_folder_selected = 0;

        if let Some(vm) = self.selected_vm() {
            self.shared_folders = crate::vm::load_shared_folders(vm);
        }
    }

    /// Add a shared folder, generating a unique mount tag
    pub fn add_shared_folder(&mut self, host_path: String) {
        // Reject duplicate paths
        if self.shared_folders.iter().any(|f| f.host_path == host_path) {
            return;
        }

        let mut mount_tag = generate_mount_tag(&host_path);

        // Ensure unique mount tag
        let base_tag = mount_tag.clone();
        let mut suffix = 2;
        while self.shared_folders.iter().any(|f| f.mount_tag == mount_tag) {
            mount_tag = format!("{}_{}", base_tag, suffix);
            suffix += 1;
        }

        self.shared_folders.push(SharedFolder {
            host_path,
            mount_tag,
        });
    }

    /// Remove the currently selected shared folder
    pub fn remove_shared_folder(&mut self) {
        if !self.shared_folders.is_empty() && self.shared_folder_selected < self.shared_folders.len()
        {
            self.shared_folders.remove(self.shared_folder_selected);
            if self.shared_folder_selected >= self.shared_folders.len()
                && self.shared_folder_selected > 0
            {
                self.shared_folder_selected -= 1;
            }
        }
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

    /// Non-blocking check for VM status updates from background thread.
    /// Consumes all pending messages, keeping only the latest result.
    pub fn check_vm_status(&mut self) {
        let mut latest = None;
        while let Ok(processes) = self.vm_status_rx.try_recv() {
            latest = Some(processes);
        }
        if let Some(processes) = latest {
            self.running_vms = self.match_running_vms(&processes);
            // Clean up stopping_vms for VMs that have actually stopped
            self.stopping_vms.retain(|id, _| self.running_vms.contains_key(id));
        }
    }

    /// Match QEMU processes against known VMs using the process working directory.
    ///
    /// Launch scripts run QEMU from the VM's directory, so /proc/<pid>/cwd
    /// reliably identifies which VM a process belongs to — unlike disk filenames
    /// which are often generic (e.g., "disk.qcow2").
    fn match_running_vms(&self, processes: &[QemuProcess]) -> HashMap<String, u32> {
        let mut result = HashMap::new();
        for vm in &self.vms {
            for proc in processes {
                if let Some(ref cwd) = proc.cwd {
                    // cwd is available — use it as the authoritative match
                    if cwd == &vm.path {
                        result.insert(vm.id.clone(), proc.pid);
                        break;
                    }
                } else {
                    // No cwd available (permissions?) — fall back to full disk path in cmdline
                    if let Some(disk) = vm.config.primary_disk() {
                        if let Some(disk_path_str) = disk.path.to_str() {
                            if !disk_path_str.is_empty() && proc.cmdline.contains(disk_path_str) {
                                result.insert(vm.id.clone(), proc.pid);
                                break;
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Get PID of the currently selected VM if it's running.
    pub fn selected_vm_pid(&self) -> Option<u32> {
        let vm = self.selected_vm()?;
        self.running_vms.get(&vm.id).copied()
    }

    /// Seed `file_browser_dir` for ISO selection: prefer the configured
    /// default ISO path if it still exists, else the user's home directory.
    pub fn seed_iso_browser_dir(&mut self) {
        let target = self
            .config
            .default_iso_path
            .as_ref()
            .filter(|p| p.is_dir())
            .cloned()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        self.file_browser_dir = target;
    }

    /// Load file browser entries for current directory
    pub fn load_file_browser(&mut self, mode: FileBrowserMode) {
        self.file_browser_mode = mode;
        self.file_browser_entries.clear();
        self.file_browser_selected = 0;

        // Determine file extensions to filter by based on mode
        let extensions: &[&str] = match mode {
            FileBrowserMode::Iso => &[".iso", ".ISO"],
            FileBrowserMode::RecoveryImage => &[".dmg", ".DMG", ".qcow2", ".QCOW2"],
            FileBrowserMode::Disk => &[".qcow2", ".QCOW2", ".qcow", ".QCOW"],
            FileBrowserMode::Directory => &[],
            FileBrowserMode::ImportConfig => &[".xml", ".XML", ".conf"],
            FileBrowserMode::Bios => &[".bin", ".BIN", ".rom", ".ROM", ".qcow2", ".QCOW2", ".fd", ".FD"],
            FileBrowserMode::Floppy => &[".img", ".IMG", ".ima", ".IMA", ".flp", ".FLP", ".vfd", ".VFD"],
        };

        // For Directory mode, add a [Select This Directory] sentinel entry first
        if mode == FileBrowserMode::Directory {
            self.file_browser_entries.push(FileBrowserEntry {
                name: "[Select This Directory]".to_string(),
                path: self.file_browser_dir.clone(),
                is_dir: false, // So Enter returns it as a selection
            });
        }

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
                    } else if mode != FileBrowserMode::Directory
                        && extensions.iter().any(|ext| entry.name.ends_with(ext))
                    {
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
    // Notes Editor Methods
    // =========================================================================

    /// Load the selected VM's notes into the editor
    pub fn load_notes_into_editor(&mut self) {
        if let Some(vm) = self.selected_vm() {
            let notes_text = vm.notes.as_deref().unwrap_or("");
            self.script_editor_lines = notes_text.lines().map(String::from).collect();
            if self.script_editor_lines.is_empty() {
                self.script_editor_lines.push(String::new());
            }
            self.script_editor_cursor = (0, 0);
            self.script_editor_modified = false;
            self.script_editor_h_scroll = 0;
            self.raw_script_scroll = 0;
        }
    }

    /// Save the editor content as notes to vm-curator.toml
    pub fn save_notes_from_editor(&mut self) -> Result<()> {
        let vm = self.selected_vm()
            .ok_or_else(|| anyhow::anyhow!("No VM selected"))?;

        let vm_path = vm.path.clone();
        let display_name = vm.display_name();
        let os_profile = vm.os_profile.clone();

        let notes_text = self.script_editor_lines.join("\n");
        // Trim trailing whitespace/newlines
        let notes_text = notes_text.trim_end().to_string();
        let notes = if notes_text.is_empty() { None } else { Some(notes_text.as_str()) };

        crate::vm::create::write_vm_metadata(
            &vm_path,
            &display_name,
            os_profile.as_deref(),
            notes,
        )?;

        // Update the in-memory VM's notes
        if let Some(filtered_idx) = self.visual_order.get(self.selected_vm) {
            if let Some(actual_idx) = self.filtered_indices.get(*filtered_idx) {
                if let Some(vm) = self.vms.get_mut(*actual_idx) {
                    vm.notes = notes.map(String::from);
                }
            }
        }

        self.script_editor_modified = false;
        Ok(())
    }

    // =========================================================================
    // VM Creation Wizard Methods
    // =========================================================================

    /// Start the VM creation wizard
    pub fn start_create_wizard(&mut self) {
        let state = CreateWizardState {
            disk_size_gb: self.config.default_disk_size_gb,
            qemu_config: WizardQemuConfig {
                memory_mb: self.config.default_memory_mb,
                cpu_cores: self.config.default_cpu_cores,
                enable_kvm: self.config.default_enable_kvm,
                display: self.config.default_display.clone(),
                ..WizardQemuConfig::default()
            },
            ..CreateWizardState::default()
        };

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

    // =========================================================================
    // VM Import Wizard Methods
    // =========================================================================

    /// Start the VM import wizard
    pub fn start_import_wizard(&mut self) {
        self.import_state = Some(ImportWizardState::default());
        self.push_screen(Screen::ImportWizard);
    }

    /// Cancel the import wizard and return to main menu
    pub fn cancel_import_wizard(&mut self) {
        self.import_state = None;
        while self.screen == Screen::ImportWizard {
            self.pop_screen();
        }
    }

    /// Get the selected OS profile in the wizard
    pub fn wizard_selected_profile(&self) -> Option<&crate::metadata::QemuProfile> {
        self.wizard_state
            .as_ref()
            .and_then(|s| s.selected_os.as_ref())
            .and_then(|os_id| self.qemu_profiles.get(os_id))
    }
}

/// Generate a mount tag from a host directory path
fn generate_mount_tag(path: &str) -> String {
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
