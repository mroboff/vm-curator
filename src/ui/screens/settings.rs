//! Settings Screen
//!
//! Allows users to configure application settings with a tree-view structure
//! that shows dependent settings only when their parent is enabled.
//! Features a two-column layout with contextual help text and GPU validation.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::app::App;
use crate::config::Config;
use crate::fs;
use crate::hardware::{check_multi_gpu_passthrough_status, check_single_gpu_support, MultiGpuPassthroughStatus, LookingGlassConfig, SingleGpuSupport};
use crate::vm::single_gpu_scripts::{run_system_setup, SystemSetupResult};

/// GPU passthrough validation result
#[derive(Debug)]
pub enum GpuValidationResult {
    MultiGpu(MultiGpuPassthroughStatus),
    SingleGpu(SingleGpuSupport),
}

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
    SingleGpuRunSetup, // Action button - runs system setup
    SingleGpuAutoTty,
    SingleGpuShowWarnings,
}

/// A visible settings row with its item and indentation level
#[derive(Debug, Clone)]
struct VisibleItem {
    item: SettingsItem,
    indent: usize,
    is_header: bool,
    is_radio: bool,
    is_action: bool,
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
            SettingsItem::SingleGpuRunSetup => "[Run System Setup]",
            SettingsItem::SingleGpuAutoTty => "Auto TTY Switch (Experimental)",
            SettingsItem::SingleGpuShowWarnings => "Show GPU Warnings",
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
            SettingsItem::SingleGpuRunSetup => String::new(), // Action button, no value display
            SettingsItem::SingleGpuAutoTty => bool_to_yes_no(config.single_gpu_auto_tty),
            SettingsItem::SingleGpuShowWarnings => bool_to_yes_no(config.show_gpu_warnings),
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

    /// Check if this is an action button (executes something when pressed)
    pub fn is_action(&self) -> bool {
        matches!(self, SettingsItem::SingleGpuRunSetup)
    }

    /// Get cycle options for this setting
    pub fn cycle_options(&self) -> Option<&'static [&'static str]> {
        match self {
            SettingsItem::DefaultDisplay => Some(&["gtk", "sdl", "spice"]),
            _ => None,
        }
    }

    /// Get the help key for looking up help text in the settings_help store
    pub fn help_key(&self) -> &'static str {
        match self {
            SettingsItem::VmLibraryPath => "vm_library_path",
            SettingsItem::DefaultMemory => "default_memory",
            SettingsItem::DefaultCpuCores => "default_cpu_cores",
            SettingsItem::DefaultDiskSize => "default_disk_size",
            SettingsItem::DefaultDisplay => "default_display",
            SettingsItem::DefaultEnableKvm => "default_enable_kvm",
            SettingsItem::ConfirmBeforeLaunch => "confirm_before_launch",
            SettingsItem::GpuPassthroughHeader => "gpu_passthrough_header",
            SettingsItem::GpuPassthroughDisabled => "gpu_passthrough_disabled",
            SettingsItem::EnableMultiGpuPassthrough => "enable_multi_gpu_passthrough",
            SettingsItem::MultiGpuIvshmemSize => "multi_gpu_ivshmem_size",
            SettingsItem::MultiGpuShowWarnings | SettingsItem::SingleGpuShowWarnings => "show_gpu_warnings",
            SettingsItem::MultiGpuAutoLaunchLookingGlass => "auto_launch_looking_glass",
            SettingsItem::EnableSingleGpuPassthrough => "enable_single_gpu_passthrough",
            SettingsItem::SingleGpuRunSetup => "single_gpu_run_setup",
            SettingsItem::SingleGpuAutoTty => "single_gpu_auto_tty",
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
        is_action: item.is_action(),
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
    if config.enable_multi_gpu_passthrough {
        items.push(make_visible(SettingsItem::MultiGpuIvshmemSize, 2));
        items.push(make_visible(SettingsItem::MultiGpuShowWarnings, 2));
        items.push(make_visible(SettingsItem::MultiGpuAutoLaunchLookingGlass, 2));
    }

    // Single-GPU option (radio button)
    items.push(make_visible(SettingsItem::EnableSingleGpuPassthrough, 1));

    // Single-GPU sub-settings (only visible when single-GPU is enabled)
    // Note: Looking Glass is NOT available for single-GPU passthrough because
    // the display goes directly to physical monitors connected to the GPU.
    if config.single_gpu_enabled {
        items.push(make_visible(SettingsItem::SingleGpuRunSetup, 2)); // Action button
        items.push(make_visible(SettingsItem::SingleGpuAutoTty, 2));
        items.push(make_visible(SettingsItem::SingleGpuShowWarnings, 2));
    }

    items
}

/// Render the settings screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Clear the area first to prevent artifacts from underlying screen
    frame.render_widget(Clear, area);

    // Main block
    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Add margin
    let content_area = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    );

    // Split into main content and bottom status bar
    let main_and_status = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(content_area);

    // Build visible items based on current config
    let visible_items = build_visible_items(&app.config);

    // Check if validation panel should be shown
    let show_validation = app.settings_gpu_validation.is_some();

    // Two-column layout: settings list (45%) | right panel (55%)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(main_and_status[0]);

    // Right panel: help text + optional validation
    let right_constraints = if show_validation {
        vec![
            Constraint::Min(6),      // Help text
            Constraint::Length(10),  // Validation panel
        ]
    } else {
        vec![Constraint::Min(6)]
    };

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(right_constraints)
        .split(main_chunks[1]);

    // Render settings list
    render_settings_list(app, frame, main_chunks[0], &visible_items);

    // Render help panel
    let current_item = visible_items.get(app.settings_selected).map(|vi| &vi.item);
    render_help_panel(frame, right_chunks[0], current_item, &app.settings_help);

    // Render validation panel if needed
    if show_validation && right_chunks.len() > 1 {
        render_validation_panel(frame, right_chunks[1], &app.settings_gpu_validation);
    }

    // Render bottom status bar with version and config path
    render_status_bar(app, frame, main_and_status[1], &visible_items);
}

/// Render the settings list
fn render_settings_list(app: &App, frame: &mut Frame, area: Rect, visible_items: &[VisibleItem]) {
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
            } else if vi.is_action {
                // Action button - displayed as clickable action
                format!("{}{}", indent_str, name)
            } else if vi.is_radio {
                // Radio button style - only one can be selected
                let is_enabled = match vi.item {
                    SettingsItem::GpuPassthroughDisabled => {
                        !app.config.enable_multi_gpu_passthrough && !app.config.single_gpu_enabled
                    }
                    SettingsItem::EnableMultiGpuPassthrough => app.config.enable_multi_gpu_passthrough,
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
            } else if vi.is_action {
                // Action buttons are styled like links
                Style::default().fg(Color::Cyan)
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
    frame.render_widget(list, area);
}

/// Render the contextual help panel
fn render_help_panel(frame: &mut Frame, area: Rect, current_item: Option<&SettingsItem>, help_store: &crate::metadata::SettingsHelpStore) {
    let help_key = current_item.map(|item| item.help_key()).unwrap_or("default");
    let (title, description) = help_store.get_or_default(help_key);

    let help_block = Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let help_text = Paragraph::new(description)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: true })
        .block(help_block);

    frame.render_widget(help_text, area);
}

/// Render the GPU validation panel
fn render_validation_panel(frame: &mut Frame, area: Rect, validation: &Option<GpuValidationResult>) {
    let Some(result) = validation else {
        return;
    };

    match result {
        GpuValidationResult::MultiGpu(status) => {
            render_multi_gpu_validation(frame, area, status);
        }
        GpuValidationResult::SingleGpu(support) => {
            render_single_gpu_validation(frame, area, support);
        }
    }
}

/// Render multi-GPU validation status
fn render_multi_gpu_validation(frame: &mut Frame, area: Rect, status: &MultiGpuPassthroughStatus) {
    let is_ready = status.is_ready();
    let border_color = if is_ready { Color::Green } else { Color::Yellow };

    let block = Block::default()
        .title(" Multi-GPU Status ")
        .title_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // IOMMU check
    let iommu_icon = if status.iommu_enabled { "[+]" } else { "[-]" };
    let iommu_style = if status.iommu_enabled { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled(iommu_icon, Style::default().fg(iommu_style)),
        Span::raw(" IOMMU enabled"),
    ]));

    // VFIO check
    let vfio_icon = if status.vfio_loaded { "[+]" } else { "[-]" };
    let vfio_style = if status.vfio_loaded { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled(vfio_icon, Style::default().fg(vfio_style)),
        Span::raw(" VFIO modules loaded"),
    ]));

    // GPU count
    let gpu_ok = status.available_gpus > 0;
    let gpu_icon = if gpu_ok { "[+]" } else { "[-]" };
    let gpu_style = if gpu_ok { Color::Green } else { Color::Red };
    let gpu_text = if status.available_gpus == 1 {
        " 1 GPU available".to_string()
    } else {
        format!(" {} GPUs available", status.available_gpus)
    };
    lines.push(Line::from(vec![
        Span::styled(gpu_icon, Style::default().fg(gpu_style)),
        Span::raw(gpu_text),
    ]));

    // Looking Glass check
    let lg_client = LookingGlassConfig::find_client();
    let lg_ok = lg_client.is_some();
    let lg_icon = if lg_ok { "[+]" } else { "[-]" };
    let lg_style = if lg_ok { Color::Green } else { Color::Yellow };
    lines.push(Line::from(vec![
        Span::styled(lg_icon, Style::default().fg(lg_style)),
        Span::raw(" Looking Glass client"),
    ]));

    // Status summary
    lines.push(Line::from(""));
    if is_ready {
        lines.push(Line::from(Span::styled(
            "Ready for passthrough",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Not ready",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        // Show first error as hint
        if let Some(error) = status.errors.first() {
            lines.push(Line::from(""));
            // Truncate long errors to fit panel
            let hint = if error.len() > 35 {
                format!("{}...", &error[..32])
            } else {
                error.clone()
            };
            lines.push(Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, inner);
}

/// Render single-GPU validation status
fn render_single_gpu_validation(frame: &mut Frame, area: Rect, support: &SingleGpuSupport) {
    let is_ready = support.is_supported();
    let border_color = if is_ready { Color::Green } else { Color::Yellow };

    let block = Block::default()
        .title(" Single GPU Status ")
        .title_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // IOMMU check
    let iommu_icon = if support.iommu_enabled { "[+]" } else { "[-]" };
    let iommu_style = if support.iommu_enabled { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled(iommu_icon, Style::default().fg(iommu_style)),
        Span::raw(" IOMMU enabled"),
    ]));

    // VFIO check
    let vfio_icon = if support.vfio_available { "[+]" } else { "[-]" };
    let vfio_style = if support.vfio_available { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled(vfio_icon, Style::default().fg(vfio_style)),
        Span::raw(" VFIO available"),
    ]));

    // Boot VGA check
    let vga_ok = support.boot_vga.is_some();
    let vga_icon = if vga_ok { "[+]" } else { "[-]" };
    let vga_style = if vga_ok { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled(vga_icon, Style::default().fg(vga_style)),
        Span::raw(" Boot VGA detected"),
    ]));

    // Single GPU confirmation (informational - yellow if multiple GPUs detected)
    let single_icon = if support.has_single_gpu { "[+]" } else { "[!]" };
    let single_style = if support.has_single_gpu { Color::Green } else { Color::Yellow };
    let single_text = if support.has_single_gpu {
        " Single GPU confirmed"
    } else {
        " Multiple GPUs detected"
    };
    lines.push(Line::from(vec![
        Span::styled(single_icon, Style::default().fg(single_style)),
        Span::raw(single_text),
    ]));

    // Display manager check
    if let Some(ref dm) = support.display_manager {
        lines.push(Line::from(vec![
            Span::styled("[+]", Style::default().fg(Color::Green)),
            Span::raw(format!(" Display: {}", dm.display_name())),
        ]));
    }

    // Note: Looking Glass is NOT used for single-GPU passthrough
    // because the display goes directly to physical monitors

    // Status summary
    lines.push(Line::from(""));
    if is_ready {
        lines.push(Line::from(Span::styled(
            "Ready for passthrough",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Not ready",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
    }

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, inner);
}

/// Render the bottom status bar
fn render_status_bar(app: &App, frame: &mut Frame, area: Rect, visible_items: &[VisibleItem]) {
    // Get version
    let version = env!("CARGO_PKG_VERSION");

    // Get config path
    let config_path = Config::config_file_path();
    let config_display = config_path.display().to_string();

    // Get help text for current item
    let current_item = visible_items.get(app.settings_selected).map(|vi| &vi.item);
    let is_header = visible_items.get(app.settings_selected).map(|vi| vi.is_header).unwrap_or(false);
    let is_radio = visible_items.get(app.settings_selected).map(|vi| vi.is_radio).unwrap_or(false);

    let is_action = visible_items.get(app.settings_selected).map(|vi| vi.is_action).unwrap_or(false);

    let key_hints = if app.settings_editing {
        "[Enter] Save  [Esc] Cancel"
    } else if is_header {
        "[j/k] Navigate  [Esc] Back"
    } else if is_action {
        "[Enter] Run  [j/k] Navigate  [Esc] Back"
    } else if is_radio {
        "[Enter/Space] Select  [j/k] Navigate  [Esc] Back"
    } else if current_item.map(|i| i.is_toggle()).unwrap_or(false) {
        "[Enter/Space] Toggle  [j/k] Navigate  [Esc] Back"
    } else if current_item.map(|i| i.is_cycle()).unwrap_or(false) {
        "[Enter/Space] Cycle  [j/k] Navigate  [Esc] Back"
    } else {
        "[Enter] Edit  [j/k] Navigate  [Esc] Back"
    };

    // Build status line: version | key hints | config path
    let status_text = format!("v{}  {}  Config: {}", version, key_hints, config_display);

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Left);

    frame.render_widget(status, area);
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
                    } else if vi.is_action {
                        // Action button - execute the action
                        execute_action(app, vi.item)?;
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
            app.config.enable_multi_gpu_passthrough = false;
            app.config.single_gpu_enabled = false;
            app.settings_gpu_validation = None;
        }
        SettingsItem::EnableMultiGpuPassthrough => {
            // Enable multi-GPU, disable single-GPU
            app.config.enable_multi_gpu_passthrough = true;
            app.config.single_gpu_enabled = false;
            // Run validation
            app.settings_gpu_validation = Some(
                GpuValidationResult::MultiGpu(check_multi_gpu_passthrough_status())
            );
        }
        SettingsItem::EnableSingleGpuPassthrough => {
            // Enable single-GPU, disable multi-GPU
            app.config.single_gpu_enabled = true;
            app.config.enable_multi_gpu_passthrough = false;
            // Run validation
            app.settings_gpu_validation = Some(
                GpuValidationResult::SingleGpu(check_single_gpu_support())
            );
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
        SettingsItem::MultiGpuAutoLaunchLookingGlass => {
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

/// Execute an action button
fn execute_action(app: &mut App, item: SettingsItem) -> anyhow::Result<()> {
    match item {
        SettingsItem::SingleGpuRunSetup => {
            // Get the GPU driver - use nvidia by default, or detect from system
            let gpu_driver = detect_gpu_driver();

            match run_system_setup(&gpu_driver) {
                SystemSetupResult::Launched => {
                    app.set_status("Setup launched in terminal window. Follow the prompts there.");
                }
                SystemSetupResult::NoTerminal => {
                    app.set_status("No terminal found. Install alacritty, kitty, ghostty, konsole, or gnome-terminal.");
                }
                SystemSetupResult::Error(e) => {
                    app.set_status(format!("Setup failed: {}", e));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Detect the current GPU driver (nvidia, amdgpu, or i915)
fn detect_gpu_driver() -> String {
    // Check for NVIDIA first
    if std::path::Path::new("/sys/module/nvidia").exists() {
        return "nvidia".to_string();
    }
    // Check for AMD
    if std::path::Path::new("/sys/module/amdgpu").exists() {
        return "amdgpu".to_string();
    }
    // Check for Intel
    if std::path::Path::new("/sys/module/i915").exists() {
        return "i915".to_string();
    }
    // Default to nvidia (most common for passthrough)
    "nvidia".to_string()
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
