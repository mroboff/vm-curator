use anyhow::Result;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::config::Config;
use crate::hardware::UsbDevice;
use crate::metadata::{AsciiArtStore, MetadataStore, OsInfo};
use crate::vm::{
    discover_vms, group_vms_by_category, BootMode, DiscoveredVm, LaunchOptions, Snapshot,
};

/// Application screens/views
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// Main VM list
    MainMenu,
    /// VM management options
    Management,
    /// Configuration view
    Configuration,
    /// Detailed info (history, blurbs)
    DetailedInfo,
    /// Snapshot management
    Snapshots,
    /// Boot options
    BootOptions,
    /// USB device selection
    UsbDevices,
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
}

/// Context for text input dialogs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputContext {
    SnapshotName,
}

/// Actions that need confirmation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    LaunchVm,
    ResetVm,
    DeleteVm,
    DeleteSnapshot(String),
    RestoreSnapshot(String),
}

/// Input mode for text entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
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
    /// Snapshots for current VM (cached)
    pub snapshots: Vec<Snapshot>,
    /// Selected snapshot index
    pub selected_snapshot: usize,
    /// USB devices (cached)
    pub usb_devices: Vec<UsbDevice>,
    /// Selected USB devices for passthrough
    pub selected_usb_devices: Vec<usize>,
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
    SnapshotsLoaded { snapshots: Vec<Snapshot>, error: Option<String> },
}

impl App {
    /// Create a new application instance
    pub fn new(config: Config) -> Result<Self> {
        // Discover VMs
        let vms = discover_vms(&config.vm_library_path)?;

        // Load metadata
        let mut metadata = MetadataStore::load_embedded();
        if let Ok(user_metadata) = MetadataStore::load_from_dir(&config.metadata_path) {
            metadata.merge(user_metadata);
        }

        // Load ASCII art
        let mut ascii_art = AsciiArtStore::load_embedded();
        let user_art = AsciiArtStore::load_from_dir(&config.ascii_art_path);
        ascii_art.merge(user_art);

        let filtered_indices: Vec<usize> = (0..vms.len()).collect();
        let (background_tx, background_rx) = mpsc::channel();

        Ok(Self {
            screen: Screen::MainMenu,
            screen_stack: Vec::new(),
            config,
            vms,
            selected_vm: 0,
            metadata,
            ascii_art,
            snapshots: Vec::new(),
            selected_snapshot: 0,
            usb_devices: Vec::new(),
            selected_usb_devices: Vec::new(),
            selected_menu_item: 0,
            boot_mode: BootMode::Normal,
            search_query: String::new(),
            input_mode: InputMode::Normal,
            filtered_indices,
            status_message: None,
            status_time: None,
            should_quit: false,
            file_browser_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
            file_browser_entries: Vec::new(),
            file_browser_selected: 0,
            text_input_buffer: String::new(),
            background_rx,
            background_tx,
            loading: false,
            error_detail: None,
            error_scroll: 0,
        })
    }

    /// Get the currently selected VM
    pub fn selected_vm(&self) -> Option<&DiscoveredVm> {
        if self.filtered_indices.is_empty() {
            return None;
        }
        let actual_idx = self.filtered_indices.get(self.selected_vm)?;
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

    /// Move selection up in VM list
    pub fn select_prev(&mut self) {
        if !self.filtered_indices.is_empty() && self.selected_vm > 0 {
            self.selected_vm -= 1;
        }
    }

    /// Move selection down in VM list
    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() && self.selected_vm < self.filtered_indices.len() - 1 {
            self.selected_vm += 1;
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

        // Reset selection if out of bounds
        if self.selected_vm >= self.filtered_indices.len() {
            self.selected_vm = self.filtered_indices.len().saturating_sub(1);
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

    /// Get launch options based on current state
    pub fn get_launch_options(&self) -> LaunchOptions {
        let usb_devices = self
            .selected_usb_devices
            .iter()
            .filter_map(|&i| self.usb_devices.get(i))
            .map(|d| crate::vm::UsbPassthrough {
                vendor_id: d.vendor_id,
                product_id: d.product_id,
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

    /// Get grouped VMs for display
    pub fn grouped_vms(&self) -> Vec<(&'static str, Vec<&DiscoveredVm>)> {
        group_vms_by_category(&self.vms)
    }

    /// Load file browser entries for current directory
    pub fn load_file_browser(&mut self) {
        self.file_browser_entries.clear();
        self.file_browser_selected = 0;

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
                    } else if entry.name.ends_with(".iso") || entry.name.ends_with(".ISO") {
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
                self.load_file_browser();
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
}
