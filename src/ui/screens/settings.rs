//! Settings Screen
//!
//! Allows users to configure application settings.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::App;
use crate::config::Config;

/// Settings items that can be configured
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsItem {
    VmLibraryPath,
    DefaultMemory,
    DefaultCpuCores,
    DefaultDiskSize,
    DefaultDisplay,
    DefaultEnableKvm,
    ConfirmBeforeLaunch,
}

impl SettingsItem {
    /// Get all settings items in order
    pub fn all() -> &'static [SettingsItem] {
        &[
            SettingsItem::VmLibraryPath,
            SettingsItem::DefaultMemory,
            SettingsItem::DefaultCpuCores,
            SettingsItem::DefaultDiskSize,
            SettingsItem::DefaultDisplay,
            SettingsItem::DefaultEnableKvm,
            SettingsItem::ConfirmBeforeLaunch,
        ]
    }

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
            SettingsItem::DefaultEnableKvm => if config.default_enable_kvm { "Yes" } else { "No" }.to_string(),
            SettingsItem::ConfirmBeforeLaunch => if config.confirm_before_launch { "Yes" } else { "No" }.to_string(),
        }
    }

    /// Check if this is a boolean toggle setting
    pub fn is_toggle(&self) -> bool {
        matches!(self, SettingsItem::DefaultEnableKvm | SettingsItem::ConfirmBeforeLaunch)
    }

    /// Check if this is a cycle setting (display backend)
    pub fn is_cycle(&self) -> bool {
        matches!(self, SettingsItem::DefaultDisplay)
    }

    /// Get cycle options for this setting
    pub fn cycle_options(&self) -> Option<&'static [&'static str]> {
        match self {
            SettingsItem::DefaultDisplay => Some(&["gtk", "sdl", "spice"]),
            _ => None,
        }
    }
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

    // Build settings list
    let items: Vec<ListItem> = SettingsItem::all()
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == app.settings_selected;
            let is_editing = is_selected && app.settings_editing;

            let name = item.display_name();
            let value = if is_editing {
                app.settings_edit_buffer.clone()
            } else {
                item.get_value(&app.config)
            };

            let line = if is_editing {
                format!("  {} : {}▌", name, value)
            } else {
                format!("  {} : {}", name, value)
            };

            let style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
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
    let current_item = SettingsItem::all().get(app.settings_selected);
    let help_text = if app.settings_editing {
        "[Enter] Save  [Esc] Cancel"
    } else if current_item.map(|i| i.is_toggle()).unwrap_or(false) {
        "[Enter/Space] Toggle  [Esc] Back"
    } else if current_item.map(|i| i.is_cycle()).unwrap_or(false) {
        "[Enter/Space] Cycle  [Esc] Back"
    } else {
        "[Enter] Edit  [Esc] Back"
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
    let items = SettingsItem::all();

    if app.settings_editing {
        // Editing mode
        match key.code {
            KeyCode::Enter => {
                // Save the edit
                apply_edit(app)?;
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
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.settings_selected < items.len().saturating_sub(1) {
                    app.settings_selected += 1;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(item) = items.get(app.settings_selected) {
                    if item.is_toggle() {
                        toggle_setting(app, *item)?;
                    } else if item.is_cycle() {
                        cycle_setting(app, *item)?;
                    } else {
                        // Start editing
                        app.settings_edit_buffer = item.get_value(&app.config);
                        app.settings_editing = true;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(false)
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

/// Apply an edit to the current setting
fn apply_edit(app: &mut App) -> anyhow::Result<()> {
    let items = SettingsItem::all();
    let item = items.get(app.settings_selected);

    if let Some(item) = item {
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
            _ => {}
        }

        save_config(app)?;
    }

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
