//! Settings Screen
//!
//! Allows users to configure application settings with a tree-view structure
//! that shows dependent settings only when their parent is enabled.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::App;
use crate::config::Config;
use crate::fs;

/// Settings items that can be configured
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsItem {
    // General settings
    VmLibraryPath,
    DefaultMemory,
    DefaultCpuCores,
    DefaultDiskSize,
    DefaultDisplay,
    DefaultEnableKvm,
    ConfirmBeforeLaunch,
    // GPU Passthrough section header (not selectable, just a label)
    GpuPassthroughHeader,
    // GPU Passthrough disabled - radio button
    GpuPassthroughDisabled,
    // GPU Passthrough (Multiple GPUs) - radio button
    EnableMultiGpuPassthrough,
    // Multi-GPU sub-settings (only visible when multi-GPU is enabled)
    MultiGpuIvshmemSize,
    MultiGpuShowWarnings,
    MultiGpuAutoLaunchLookingGlass,
    // GPU Passthrough (Single GPU) - radio button
    EnableSingleGpuPassthrough,
    // Single GPU sub-settings (only visible when single-GPU is enabled)
    SingleGpuAutoTty,
    SingleGpuShowWarnings,
    SingleGpuAutoLaunchLookingGlass,
}

/// A visible settings row with its item and indentation level
#[derive(Debug, Clone)]
struct VisibleItem {
    item: SettingsItem,
    indent: usize,
    is_header: bool,
    is_radio: bool,
}

impl SettingsItem {
    /// Get the display name for this setting
    pub fn display_name(&self) -> &'static str {
        match self {
            SettingsItem::VmLibraryPath => "VM Library Path",
            SettingsItem::DefaultMemory => "Default Memory (MB)",
            SettingsItem::DefaultCpuCores => "Default CPU Cores",
            SettingsItem::DefaultDiskSize => "Default Disk Size (GB)",
            SettingsItem::DefaultDisplay => "Default Display",
            SettingsItem::DefaultEnableKvm => "Enable KVM by Default",
            SettingsItem::ConfirmBeforeLaunch => "Confirm Before Launch",
            // GPU Passthrough
            SettingsItem::GpuPassthroughHeader => "GPU Passthrough",
            SettingsItem::GpuPassthroughDisabled => "Disabled",
            SettingsItem::EnableMultiGpuPassthrough => "Multiple GPUs",
            SettingsItem::MultiGpuIvshmemSize => "IVSHMEM Size (MB)",
            SettingsItem::MultiGpuShowWarnings => "Show GPU Warnings",
            SettingsItem::MultiGpuAutoLaunchLookingGlass => "Auto-launch Looking Glass",
            SettingsItem::EnableSingleGpuPassthrough => "Single GPU",
            SettingsItem::SingleGpuAutoTty => "Auto TTY Switch (Experimental)",
            SettingsItem::SingleGpuShowWarnings => "Show GPU Warnings",
            SettingsItem::SingleGpuAutoLaunchLookingGlass => "Auto-launch Looking Glass",
        }
    }

    /// Get the current value as a string
    pub fn get_value(&self, config: &Config) -> String {
        match self {
            SettingsItem::VmLibraryPath => config.vm_library_path.display().to_string(),
            SettingsItem::DefaultMemory => config.default_memory_mb.to_string(),
            SettingsItem::DefaultCpuCores => config.default_cpu_cores.to_string(),
            SettingsItem::DefaultDiskSize => config.default_disk_size_gb.to_string(),
            SettingsItem::DefaultDisplay => config.default_display.clone(),
            SettingsItem::DefaultEnableKvm => bool_to_yes_no(config.default_enable_kvm),
            SettingsItem::ConfirmBeforeLaunch => bool_to_yes_no(config.confirm_before_launch),
            // GPU Passthrough
            SettingsItem::GpuPassthroughHeader => String::new(),
            SettingsItem::GpuPassthroughDisabled => String::new(), // Radio button, no value display
            SettingsItem::EnableMultiGpuPassthrough => String::new(), // Radio button, no value display
            SettingsItem::MultiGpuIvshmemSize => config.default_ivshmem_size_mb.to_string(),
            SettingsItem::MultiGpuShowWarnings => bool_to_yes_no(config.show_gpu_warnings),
            SettingsItem::MultiGpuAutoLaunchLookingGlass => bool_to_yes_no(config.looking_glass_auto_launch),
            SettingsItem::EnableSingleGpuPassthrough => String::new(), // Radio button, no value display
            SettingsItem::SingleGpuAutoTty => bool_to_yes_no(config.single_gpu_auto_tty),
            SettingsItem::SingleGpuShowWarnings => bool_to_yes_no(config.show_gpu_warnings),
            SettingsItem::SingleGpuAutoLaunchLookingGlass => bool_to_yes_no(config.looking_glass_auto_launch),
        }
    }

    /// Check if this is a boolean toggle setting
    pub fn is_toggle(&self) -> bool {
        matches!(
            self,
            SettingsItem::DefaultEnableKvm
                | SettingsItem::ConfirmBeforeLaunch
                | SettingsItem::MultiGpuShowWarnings
                | SettingsItem::MultiGpuAutoLaunchLookingGlass
                | SettingsItem::SingleGpuAutoTty
                | SettingsItem::SingleGpuShowWarnings
                | SettingsItem::SingleGpuAutoLaunchLookingGlass
        )
    }

    /// Check if this is a radio button (mutually exclusive with others)
    pub fn is_radio(&self) -> bool {
        matches!(
            self,
            SettingsItem::GpuPassthroughDisabled
                | SettingsItem::EnableMultiGpuPassthrough
                | SettingsItem::EnableSingleGpuPassthrough
        )
    }

    /// Check if this is a cycle setting (display backend)
    pub fn is_cycle(&self) -> bool {
        matches!(self, SettingsItem::DefaultDisplay)
    }

    /// Check if this is a section header (not editable)
    pub fn is_header(&self) -> bool {
        matches!(self, SettingsItem::GpuPassthroughHeader)
    }

    /// Get cycle options for this setting
    pub fn cycle_options(&self) -> Option<&'static [&'static str]> {
        match self {
            SettingsItem::DefaultDisplay => Some(&["gtk", "sdl", "spice"]),
            _ => None,
        }
    }
}

fn bool_to_yes_no(b: bool) -> String {
    if b { "Yes" } else { "No" }.to_string()
}

/// Helper to create a visible item from a settings item
fn make_visible(item: SettingsItem, indent: usize) -> VisibleItem {
    VisibleItem {
        is_header: item.is_header(),
        is_radio: item.is_radio(),
        item,
        indent,
    }
}

/// Build the list of visible items based on current config
fn build_visible_items(config: &Config) -> Vec<VisibleItem> {
    let mut items = Vec::new();

    // General settings (always visible)
    items.push(make_visible(SettingsItem::VmLibraryPath, 0));
    items.push(make_visible(SettingsItem::DefaultMemory, 0));
    items.push(make_visible(SettingsItem::DefaultCpuCores, 0));
    items.push(make_visible(SettingsItem::DefaultDiskSize, 0));
    items.push(make_visible(SettingsItem::DefaultDisplay, 0));
    items.push(make_visible(SettingsItem::DefaultEnableKvm, 0));
    items.push(make_visible(SettingsItem::ConfirmBeforeLaunch, 0));

    // GPU Passthrough section
    items.push(make_visible(SettingsItem::GpuPassthroughHeader, 0));

    // Disabled option (radio button)
    items.push(make_visible(SettingsItem::GpuPassthroughDisabled, 1));

    // Multi-GPU option (radio button)
    items.push(make_visible(SettingsItem::EnableMultiGpuPassthrough, 1));

    // Multi-GPU sub-settings (only visible when multi-GPU is enabled)
    if config.enable_gpu_passthrough {
        items.push(make_visible(SettingsItem::MultiGpuIvshmemSize, 2));
        items.push(make_visible(SettingsItem::MultiGpuShowWarnings, 2));
        items.push(make_visible(SettingsItem::MultiGpuAutoLaunchLookingGlass, 2));
    }

    // Single-GPU option (radio button)
    items.push(make_visible(SettingsItem::EnableSingleGpuPassthrough, 1));

    // Single-GPU sub-settings (only visible when single-GPU is enabled)
    if config.single_gpu_enabled {
        items.push(make_visible(SettingsItem::SingleGpuAutoTty, 2));
        items.push(make_visible(SettingsItem::SingleGpuShowWarnings, 2));
        items.push(make_visible(SettingsItem::SingleGpuAutoLaunchLookingGlass, 2));
    }

    items
}

/// Render the settings screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Main block
    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into content and help areas
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(10),   // Settings list
            Constraint::Length(3), // Help text
        ])
        .split(inner);

    // Build visible items based on current config
    let visible_items = build_visible_items(&app.config);

    // Build settings list
    let items: Vec<ListItem> = visible_items
        .iter()
        .enumerate()
        .map(|(i, vi)| {
            let is_selected = i == app.settings_selected;
            let is_editing = is_selected && app.settings_editing;

            let name = vi.item.display_name();

            // Build the line with proper indentation
            let indent_str = "  ".repeat(vi.indent);

            let line = if vi.is_header {
                // Section header - no value, just the name with special styling
                format!("{}--- {} ---", indent_str, name)
            } else if vi.is_radio {
                // Radio button style - only one can be selected
                let is_enabled = match vi.item {
                    SettingsItem::GpuPassthroughDisabled => {
                        !app.config.enable_gpu_passthrough && !app.config.single_gpu_enabled
                    }
                    SettingsItem::EnableMultiGpuPassthrough => app.config.enable_gpu_passthrough,
                    SettingsItem::EnableSingleGpuPassthrough => app.config.single_gpu_enabled,
                    _ => false,
                };
                let radio = if is_enabled { "(*)" } else { "( )" };
                format!("{}{} {}", indent_str, radio, name)
            } else {
                // Normal setting
                let value = if is_editing {
                    app.settings_edit_buffer.clone()
                } else {
                    vi.item.get_value(&app.config)
                };

                if is_editing {
                    format!("{}{} : {}|", indent_str, name, value)
                } else if value.is_empty() {
                    format!("{}{}", indent_str, name)
                } else {
                    format!("{}{} : {}", indent_str, name, value)
                }
            };

            let style = if vi.is_header {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if vi.indent == 2 {
                // Sub-settings are slightly dimmed
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(list, chunks[0]);

    // Help text
    let current_item = visible_items.get(app.settings_selected).map(|vi| &vi.item);
    let is_header = visible_items.get(app.settings_selected).map(|vi| vi.is_header).unwrap_or(false);
    let is_radio = visible_items.get(app.settings_selected).map(|vi| vi.is_radio).unwrap_or(false);

    let help_text = if app.settings_editing {
        "[Enter] Save  [Esc] Cancel"
    } else if is_header {
        "[j/k] Navigate  [Esc] Back"
    } else if is_radio {
        "[Enter/Space] Select  [j/k] Navigate  [Esc] Back"
    } else if current_item.map(|i| i.is_toggle()).unwrap_or(false) {
        "[Enter/Space] Toggle  [j/k] Navigate  [Esc] Back"
    } else if current_item.map(|i| i.is_cycle()).unwrap_or(false) {
        "[Enter/Space] Cycle  [j/k] Navigate  [Esc] Back"
    } else {
        "[Enter] Edit  [j/k] Navigate  [Esc] Back"
    };

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[1]);

    // Show config file path at bottom
    let config_path = Config::config_file_path();
    let path_text = Paragraph::new(format!("Config: {}", config_path.display()))
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Right);

    let path_area = Rect::new(
        inner.x + 1,
        inner.y + inner.height.saturating_sub(2),
        inner.width.saturating_sub(2),
        1,
    );
    frame.render_widget(path_text, path_area);
}

/// Handle input for the settings screen
pub fn handle_input(app: &mut App, key: KeyEvent) -> anyhow::Result<bool> {
    let visible_items = build_visible_items(&app.config);

    if app.settings_editing {
        // Editing mode
        match key.code {
            KeyCode::Enter => {
                // Save the edit
                if let Some(vi) = visible_items.get(app.settings_selected) {
                    apply_edit(app, vi.item)?;
                }
                app.settings_editing = false;
            }
            KeyCode::Esc => {
                // Cancel edit
                app.settings_editing = false;
                app.settings_edit_buffer.clear();
            }
            KeyCode::Backspace => {
                app.settings_edit_buffer.pop();
            }
            KeyCode::Char(c) => {
                app.settings_edit_buffer.push(c);
            }
            _ => {}
        }
    } else {
        // Navigation mode
        match key.code {
            KeyCode::Esc => {
                app.pop_screen();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.settings_selected > 0 {
                    app.settings_selected -= 1;
                    // Skip headers when navigating
                    while app.settings_selected > 0 {
                        if let Some(vi) = visible_items.get(app.settings_selected) {
                            if !vi.is_header {
                                break;
                            }
                        }
                        app.settings_selected -= 1;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.settings_selected < visible_items.len().saturating_sub(1) {
                    app.settings_selected += 1;
                    // Skip headers when navigating
                    while app.settings_selected < visible_items.len().saturating_sub(1) {
                        if let Some(vi) = visible_items.get(app.settings_selected) {
                            if !vi.is_header {
                                break;
                            }
                        }
                        app.settings_selected += 1;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(vi) = visible_items.get(app.settings_selected) {
                    if vi.is_header {
                        // Headers are not interactive
                    } else if vi.is_radio {
                        // Radio button - enable this option and disable the other
                        toggle_radio(app, vi.item)?;
                    } else if vi.item.is_toggle() {
                        toggle_setting(app, vi.item)?;
                    } else if vi.item.is_cycle() {
                        cycle_setting(app, vi.item)?;
                    } else {
                        // Start editing
                        app.settings_edit_buffer = vi.item.get_value(&app.config);
                        app.settings_editing = true;
                    }
                }
            }
            _ => {}
        }
    }

    // Clamp selection to visible items after any change
    let visible_items = build_visible_items(&app.config);
    if app.settings_selected >= visible_items.len() {
        app.settings_selected = visible_items.len().saturating_sub(1);
    }

    Ok(false)
}

/// Toggle a radio button (mutually exclusive options)
fn toggle_radio(app: &mut App, item: SettingsItem) -> anyhow::Result<()> {
    match item {
        SettingsItem::GpuPassthroughDisabled => {
            // Disable all GPU passthrough
            app.config.enable_gpu_passthrough = false;
            app.config.single_gpu_enabled = false;
        }
        SettingsItem::EnableMultiGpuPassthrough => {
            // Enable multi-GPU, disable single-GPU
            app.config.enable_gpu_passthrough = true;
            app.config.single_gpu_enabled = false;
        }
        SettingsItem::EnableSingleGpuPassthrough => {
            // Enable single-GPU, disable multi-GPU
            app.config.single_gpu_enabled = true;
            app.config.enable_gpu_passthrough = false;
        }
        _ => {}
    }
    save_config(app)?;
    Ok(())
}

/// Toggle a boolean setting
fn toggle_setting(app: &mut App, item: SettingsItem) -> anyhow::Result<()> {
    match item {
        SettingsItem::DefaultEnableKvm => {
            app.config.default_enable_kvm = !app.config.default_enable_kvm;
        }
        SettingsItem::ConfirmBeforeLaunch => {
            app.config.confirm_before_launch = !app.config.confirm_before_launch;
        }
        SettingsItem::MultiGpuShowWarnings | SettingsItem::SingleGpuShowWarnings => {
            app.config.show_gpu_warnings = !app.config.show_gpu_warnings;
        }
        SettingsItem::MultiGpuAutoLaunchLookingGlass | SettingsItem::SingleGpuAutoLaunchLookingGlass => {
            app.config.looking_glass_auto_launch = !app.config.looking_glass_auto_launch;
        }
        SettingsItem::SingleGpuAutoTty => {
            app.config.single_gpu_auto_tty = !app.config.single_gpu_auto_tty;
        }
        _ => {}
    }
    save_config(app)?;
    Ok(())
}

/// Cycle through options for a setting
fn cycle_setting(app: &mut App, item: SettingsItem) -> anyhow::Result<()> {
    if let Some(options) = item.cycle_options() {
        let current = item.get_value(&app.config);
        let current_idx = options.iter().position(|&o| o == current).unwrap_or(0);
        let next_idx = (current_idx + 1) % options.len();

        match item {
            SettingsItem::DefaultDisplay => {
                app.config.default_display = options[next_idx].to_string();
            }
            _ => {}
        }
        save_config(app)?;
    }
    Ok(())
}

/// Apply an edit to a setting
fn apply_edit(app: &mut App, item: SettingsItem) -> anyhow::Result<()> {
    let value = app.settings_edit_buffer.trim();

    match item {
        SettingsItem::VmLibraryPath => {
            let path = std::path::PathBuf::from(value);
            // Expand ~ to home directory
            let path = if value.starts_with("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(&value[2..])
                } else {
                    path
                }
            } else {
                path
            };

            // Create directory if it doesn't exist, with BTRFS CoW optimization
            if !path.exists() {
                match fs::setup_vm_directory(&path) {
                    Ok(cow_disabled) => {
                        if cow_disabled {
                            app.set_status("Created directory with BTRFS CoW disabled");
                        } else {
                            app.set_status("Created directory");
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to create directory: {}", e));
                        return Ok(());
                    }
                }
            }

            app.config.vm_library_path = path;
        }
        SettingsItem::DefaultMemory => {
            if let Ok(mb) = value.parse::<u32>() {
                app.config.default_memory_mb = mb;
            }
        }
        SettingsItem::DefaultCpuCores => {
            if let Ok(cores) = value.parse::<u32>() {
                app.config.default_cpu_cores = cores.max(1);
            }
        }
        SettingsItem::DefaultDiskSize => {
            if let Ok(gb) = value.parse::<u32>() {
                app.config.default_disk_size_gb = gb.max(1);
            }
        }
        SettingsItem::DefaultDisplay => {
            app.config.default_display = value.to_string();
        }
        SettingsItem::MultiGpuIvshmemSize => {
            if let Ok(mb) = value.parse::<u32>() {
                // Clamp to reasonable range (16-512 MB)
                app.config.default_ivshmem_size_mb = mb.clamp(16, 512);
            }
        }
        _ => {}
    }

    save_config(app)?;
    app.settings_edit_buffer.clear();
    Ok(())
}

/// Save config and show status
fn save_config(app: &mut App) -> anyhow::Result<()> {
    match app.config.save() {
        Ok(()) => {
            app.set_status("Settings saved");
        }
        Err(e) => {
            app.set_status(format!("Failed to save settings: {}", e));
        }
    }
    Ok(())
}
