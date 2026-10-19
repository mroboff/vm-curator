//! VM Creation Wizard screens
//!
//! A 5-step wizard for creating new VMs with OS-specific QEMU defaults.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{
    App, DiskAction, DiskImageFormat, FileBrowserMode, NicConfig, WizardDiskSource, WizardField,
    WizardQemuConfig, WizardStep,
};
use crate::metadata::QemuProfileStore;
use crate::vm::create::create_vm_with_disk_format;

/// Parse a size string with optional suffix (KB, MB, GB, case-insensitive)
/// Returns value normalized to target unit.
///
/// For memory (target="MB"): "8GB" -> 8192, "8192" -> 8192
/// For disk (target="GB"): "500GB" -> 500, "512000MB" -> 500
fn parse_size_with_suffix(input: &str, target_unit: &str) -> Option<u32> {
    let input = input.trim().to_uppercase();
    if input.is_empty() {
        return None;
    }

    let (num_str, suffix) = if input.ends_with("GB") {
        (&input[..input.len() - 2], "GB")
    } else if input.ends_with("MB") {
        (&input[..input.len() - 2], "MB")
    } else if input.ends_with("KB") {
        (&input[..input.len() - 2], "KB")
    } else {
        (input.as_str(), target_unit)
    };

    let value: f64 = num_str.trim().parse().ok()?;
    if value < 0.0 {
        return None;
    }

    let result = match (suffix, target_unit) {
        ("GB", "MB") => value * 1024.0,
        ("MB", "MB") => value,
        ("KB", "MB") => value / 1024.0,
        ("GB", "GB") => value,
        ("MB", "GB") => value / 1024.0,
        ("KB", "GB") => value / (1024.0 * 1024.0),
        _ => value,
    };

    if result >= 0.0 && result <= u32::MAX as f64 {
        Some(result.round() as u32)
    } else {
        None
    }
}

/// Render the create wizard based on current step
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Wizard dialog size
    let dialog_width = 80.min(area.width.saturating_sub(4));
    let dialog_height = 40.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let Some(ref state) = app.wizard_state else {
        return;
    };

    // Render the appropriate step
    match state.step {
        WizardStep::SelectOs => render_step_select_os(app, frame, dialog_area),
        WizardStep::SelectIso => render_step_select_iso(app, frame, dialog_area),
        WizardStep::ConfigureDisk => render_step_configure_disk(app, frame, dialog_area),
        WizardStep::ConfigureQemu => render_step_configure_qemu(app, frame, dialog_area),
        WizardStep::Confirm => render_step_confirm(app, frame, dialog_area),
    }

    // Network adapter overlays draw on top of step 4 when active, stacked
    // list -> per-NIC editor -> port-forward editor.
    if app.wizard_editing_nics {
        render_wizard_nic_list(app, frame, dialog_area);
    }
    if app.wizard_editing_nic_fields {
        render_wizard_nic_editor(app, frame, dialog_area);
    }
    if app.wizard_editing_port_forwards {
        render_wizard_port_forward_editor(app, frame, dialog_area);
    }
}

/// Render custom OS entry form
pub fn render_custom_os(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 70.min(area.width.saturating_sub(4));
    let dialog_height = 28.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Custom OS Entry ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let Some(ref state) = app.wizard_state else {
        return;
    };

    let custom_os = state.custom_os.as_ref();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Intro text
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // OS Name
            Constraint::Length(3), // Publisher
            Constraint::Length(3), // Architecture
            Constraint::Length(1), // Spacer
            Constraint::Length(5), // Base profile selection
            Constraint::Length(1), // Spacer
            Constraint::Min(3),    // Tips
            Constraint::Length(2), // Help
        ])
        .split(inner);

    // Intro text
    let intro = Paragraph::new("Define your custom operating system:")
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(intro, chunks[0]);

    // OS Name input
    let os_name = custom_os.map(|c| c.name.as_str()).unwrap_or("");
    let name_focus = state.field_focus == 0;
    let name_editing = matches!(state.editing_field, Some(WizardField::CustomOsName));

    render_input_field(
        frame,
        chunks[2],
        "OS Name",
        if os_name.is_empty() {
            "e.g., My Custom Linux"
        } else {
            os_name
        },
        os_name.is_empty(),
        name_focus,
        name_editing,
    );

    if name_editing {
        let cursor_x = chunks[2].x + 1 + os_name.len() as u16;
        let cursor_y = chunks[2].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    // Publisher input
    let publisher = custom_os.map(|c| c.publisher.as_str()).unwrap_or("");
    let pub_focus = state.field_focus == 1;
    let pub_editing = matches!(state.editing_field, Some(WizardField::CustomOsPublisher));

    render_input_field(
        frame,
        chunks[3],
        "Publisher",
        if publisher.is_empty() {
            "e.g., Open Source Community"
        } else {
            publisher
        },
        publisher.is_empty(),
        pub_focus,
        pub_editing,
    );

    if pub_editing {
        let cursor_x = chunks[3].x + 1 + publisher.len() as u16;
        let cursor_y = chunks[3].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    // Architecture selection (cycle)
    let arch = custom_os
        .map(|c| c.architecture.as_str())
        .unwrap_or("x86_64");
    let arch_focus = state.field_focus == 2;
    render_select_field(
        frame,
        chunks[4],
        "Architecture",
        arch,
        arch_focus,
        "[←/→] to change",
    );

    // Base profile selection
    let base_profile = custom_os
        .map(|c| c.base_profile.as_str())
        .unwrap_or("generic-other");
    let base_focus = state.field_focus == 3;

    let base_block = Block::default()
        .title(" Base QEMU Profile ")
        .borders(Borders::ALL)
        .border_style(if base_focus {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        });

    let base_inner = base_block.inner(chunks[6]);
    frame.render_widget(base_block, chunks[6]);

    let base_display = get_base_profile_display(base_profile);
    let mut base_lines = Vec::new();
    base_lines.push(Line::from(vec![
        Span::styled("Profile: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            base_display,
            if base_focus {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            },
        ),
    ]));
    base_lines.push(Line::from(Span::styled(
        if base_focus {
            "[←/→] Change profile"
        } else {
            ""
        },
        Style::default().fg(Color::DarkGray),
    )));

    let base_text = Paragraph::new(base_lines);
    frame.render_widget(base_text, base_inner);

    // Tips
    let tips_block = Block::default()
        .title(" Tip ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let tips_inner = tips_block.inner(chunks[8]);
    frame.render_widget(tips_block, chunks[8]);

    let tips_text = Paragraph::new(
        "You can adjust QEMU settings in step 4.\n\
         Consider contributing new OS profiles to the project!",
    )
    .style(Style::default().fg(Color::DarkGray))
    .wrap(Wrap { trim: false });
    frame.render_widget(tips_text, tips_inner);

    // Help
    let help = Paragraph::new("[Tab] Next field  [Enter] Continue  [Esc] Cancel")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[9]);
}

fn render_input_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    is_placeholder: bool,
    is_focused: bool,
    is_editing: bool,
) {
    let border_style = if is_editing {
        Style::default().fg(Color::Yellow)
    } else if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };

    let block = Block::default()
        .title(format!(" {} ", label))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text_style = if is_placeholder {
        Style::default().fg(Color::DarkGray)
    } else if is_editing {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let text = Paragraph::new(value).style(text_style);
    frame.render_widget(text, inner);
}

fn render_select_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    is_focused: bool,
    hint: &str,
) {
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let block = Block::default()
        .title(format!(" {} ", label))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = vec![Span::styled(
        value,
        if is_focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        },
    )];

    if is_focused {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    }

    let text = Paragraph::new(Line::from(spans));
    frame.render_widget(text, inner);
}

fn get_base_profile_display(profile_id: &str) -> &'static str {
    match profile_id {
        "generic-linux" => "Generic Linux (modern, virtio)",
        "generic-windows" => "Generic Windows (SATA, e1000)",
        "generic-bsd" => "Generic BSD (IDE, pcnet)",
        "linux-debian" => "Debian-based Linux",
        "linux-fedora" => "Fedora/RHEL-based Linux",
        "linux-arch" => "Arch Linux",
        _ => "Generic (safe defaults)",
    }
}

const ARCH_OPTIONS: &[&str] = &["x86_64", "i386", "arm64", "ppc64", "mips64", "riscv64"];
const BASE_PROFILE_OPTIONS: &[&str] = &[
    "generic-other",
    "generic-linux",
    "generic-windows",
    "generic-bsd",
    "linux-debian",
    "linux-fedora",
    "linux-arch",
];

/// Render ISO download progress
pub fn render_download(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 10.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Downloading ISO ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let progress = app
        .wizard_state
        .as_ref()
        .map(|s| s.iso_download_progress)
        .unwrap_or(0.0);

    let text = Paragraph::new(format!(
        "Downloading... {:.0}%\n\n[Esc] Cancel",
        progress * 100.0
    ))
    .style(Style::default().fg(Color::White))
    .alignment(Alignment::Center);
    frame.render_widget(text, inner);
}

/// Handle key input for wizard
pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(ref state) = app.wizard_state else {
        return Ok(());
    };

    // Handle step-specific keys
    match state.step {
        WizardStep::SelectOs => handle_step_select_os(app, key),
        WizardStep::SelectIso => handle_step_select_iso(app, key),
        WizardStep::ConfigureDisk => handle_step_configure_disk(app, key),
        WizardStep::ConfigureQemu => handle_step_configure_qemu(app, key),
        WizardStep::Confirm => handle_step_confirm(app, key),
    }
}

/// Handle key input for custom OS form
pub fn handle_custom_os_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let editing = app
        .wizard_state
        .as_ref()
        .map(|s| s.editing_field.is_some())
        .unwrap_or(false);

    if editing {
        // Text input mode
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Tab => {
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = None;
                    if key.code == KeyCode::Tab {
                        // Move to next field
                        state.field_focus = (state.field_focus + 1) % 4;
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut state) = app.wizard_state {
                    if let Some(ref mut custom) = state.custom_os {
                        match state.editing_field {
                            Some(WizardField::CustomOsName) => custom.name.push(c),
                            Some(WizardField::CustomOsPublisher) => custom.publisher.push(c),
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut state) = app.wizard_state {
                    if let Some(ref mut custom) = state.custom_os {
                        match state.editing_field {
                            Some(WizardField::CustomOsName) => {
                                custom.name.pop();
                            }
                            Some(WizardField::CustomOsPublisher) => {
                                custom.publisher.pop();
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    } else {
        // Navigation mode
        match key.code {
            KeyCode::Esc => {
                // Cancel custom OS and return to wizard
                if let Some(ref mut state) = app.wizard_state {
                    state.custom_os = None;
                }
                app.pop_screen();
            }
            KeyCode::Tab | KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut state) = app.wizard_state {
                    state.field_focus = (state.field_focus + 1) % 4;
                }
            }
            KeyCode::BackTab | KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut state) = app.wizard_state {
                    state.field_focus = if state.field_focus == 0 {
                        3
                    } else {
                        state.field_focus - 1
                    };
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let delta = if key.code == KeyCode::Right {
                    1i32
                } else {
                    -1i32
                };
                if let Some(ref mut state) = app.wizard_state {
                    if let Some(ref mut custom) = state.custom_os {
                        match state.field_focus {
                            2 => {
                                // Architecture
                                let current_idx = ARCH_OPTIONS
                                    .iter()
                                    .position(|&a| a == custom.architecture)
                                    .unwrap_or(0);
                                let new_idx = (current_idx as i32 + delta)
                                    .rem_euclid(ARCH_OPTIONS.len() as i32)
                                    as usize;
                                custom.architecture = ARCH_OPTIONS[new_idx].to_string();
                            }
                            3 => {
                                // Base profile
                                let current_idx = BASE_PROFILE_OPTIONS
                                    .iter()
                                    .position(|&p| p == custom.base_profile)
                                    .unwrap_or(0);
                                let new_idx = (current_idx as i32 + delta)
                                    .rem_euclid(BASE_PROFILE_OPTIONS.len() as i32)
                                    as usize;
                                custom.base_profile = BASE_PROFILE_OPTIONS[new_idx].to_string();
                            }
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Char(' ') => {
                // Enter edit mode for text fields
                if let Some(ref mut state) = app.wizard_state {
                    match state.field_focus {
                        0 => state.editing_field = Some(WizardField::CustomOsName),
                        1 => state.editing_field = Some(WizardField::CustomOsPublisher),
                        _ => {}
                    }
                }
            }
            KeyCode::Enter => {
                // Validate and continue
                let valid = app
                    .wizard_state
                    .as_ref()
                    .and_then(|s| s.custom_os.as_ref())
                    .map(|c| !c.name.trim().is_empty())
                    .unwrap_or(false);

                if valid {
                    // Extract needed data first
                    let (base_profile_id, custom_name, vm_name_empty) = {
                        let state = app.wizard_state.as_ref().unwrap();
                        let custom = state.custom_os.as_ref().unwrap();
                        (
                            custom.base_profile.clone(),
                            custom.name.clone(),
                            state.vm_name.is_empty(),
                        )
                    };

                    // Get profile settings
                    let profile_settings = app.qemu_profiles.get(&base_profile_id).cloned();

                    // Now apply changes
                    if let Some(ref mut state) = app.wizard_state {
                        // Apply profile settings
                        if let Some(profile) = profile_settings {
                            state.qemu_config = WizardQemuConfig::from_profile(&profile);
                            state.disk_size_gb = profile.disk_size_gb;
                        }

                        // Set VM name if empty
                        if vm_name_empty {
                            state.vm_name = custom_name.clone();
                            state.update_folder_name(&app.config.vm_library_path);
                        }

                        // Generate ID from name
                        let id = custom_name
                            .to_lowercase()
                            .chars()
                            .map(|c| if c.is_alphanumeric() { c } else { '-' })
                            .collect::<String>()
                            .split('-')
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("-");

                        if let Some(ref mut custom) = state.custom_os {
                            custom.id = id;
                        }
                    }

                    app.pop_screen(); // Return to wizard
                } else if let Some(ref mut state) = app.wizard_state {
                    state.error_message = Some("Please enter an OS name".to_string());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Handle key input for download screen
pub fn handle_download_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        // Cancel download
        if let Some(ref mut state) = app.wizard_state {
            state.iso_downloading = false;
            state.iso_download_progress = 0.0;
        }
        app.pop_screen();
    }
    Ok(())
}

// =============================================================================
// Step 1: Select OS
// =============================================================================

fn render_step_select_os(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(
            " Create New VM ({}/5) - {} ",
            state.step.number(),
            state.step.title()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout: OS list first, then VM name field below
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // OS list header
            Constraint::Min(10),   // OS list
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // VM Name field
            Constraint::Length(1), // Error message
            Constraint::Length(2), // Help text
        ])
        .split(inner);

    // OS list header
    let header = Paragraph::new("Select Operating System:").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    // OS list (grouped by category)
    render_os_list(app, frame, chunks[1]);

    // VM Name input (below OS list)
    let name_editing = matches!(state.editing_field, Some(WizardField::VmName));
    let name_style = if name_editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let name_border = if name_editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let name_block = Block::default()
        .title(" VM Name (Tab to edit) ")
        .borders(Borders::ALL)
        .border_style(name_border);

    let name_text = if state.vm_name.is_empty() {
        Paragraph::new("Select an OS above...")
            .style(Style::default().fg(Color::DarkGray))
            .block(name_block)
    } else {
        Paragraph::new(state.vm_name.as_str())
            .style(name_style)
            .block(name_block)
    };
    frame.render_widget(name_text, chunks[3]);

    // Set cursor position when editing
    if name_editing {
        let cursor_x = chunks[3].x + 1 + state.vm_name.len() as u16;
        let cursor_y = chunks[3].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    // Error message
    if let Some(ref error) = state.error_message {
        let error_text = Paragraph::new(error.as_str()).style(Style::default().fg(Color::Red));
        frame.render_widget(error_text, chunks[4]);
    }

    // Help text
    let help_text = if name_editing {
        "[Enter] Done editing  [Esc] Cancel"
    } else {
        "[j/k] Select OS  [Tab] Edit name  [Enter] Next  [Esc] Cancel"
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[5]);
}

fn render_os_list(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build the list of items (categories and OSes)
    let mut lines: Vec<Line> = Vec::new();
    let mut item_index = 0;

    // Get categories in display order
    let category_order = [
        "windows",
        "linux",
        "bsd",
        "unix",
        "macos",
        "mobile",
        "infrastructure",
        "utilities",
        "alternative",
        "retro",
        "classic-mac",
    ];

    for category in &category_order {
        let profiles = app.qemu_profiles.list_by_category(category);
        if profiles.is_empty() {
            continue;
        }

        let is_expanded = state.is_category_expanded(category);
        let is_selected = item_index == state.os_list_selected;

        // Category header
        let expand_icon = if is_expanded { "v" } else { ">" };
        let category_name = QemuProfileStore::category_display_name(category);
        let category_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };

        let prefix = if is_selected { "> " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, category_style),
            Span::styled(expand_icon, category_style),
            Span::styled(format!(" {}", category_name), category_style),
        ]));

        item_index += 1;

        // OS items (if expanded)
        if is_expanded {
            for (os_id, profile) in &profiles {
                // Filter by search query
                if !state.os_filter.is_empty() {
                    let filter_lower = state.os_filter.to_lowercase();
                    if !profile.display_name.to_lowercase().contains(&filter_lower)
                        && !os_id.to_lowercase().contains(&filter_lower)
                    {
                        continue;
                    }
                }

                let is_os_selected = item_index == state.os_list_selected;
                let is_chosen = state.selected_os.as_ref() == Some(*os_id);

                let os_style = if is_os_selected {
                    Style::default().fg(Color::Yellow)
                } else if is_chosen {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                let prefix = if is_os_selected { "> " } else { "  " };
                let chosen_marker = if is_chosen { "*" } else { " " };
                let summary = profile.summary();

                lines.push(Line::from(vec![
                    Span::styled(prefix, os_style),
                    Span::styled(format!("   {}", chosen_marker), os_style),
                    Span::styled(profile.display_name.to_string(), os_style),
                    Span::styled(
                        format!("  ({})", summary),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));

                item_index += 1;
            }
        }
    }

    // Add "Custom OS" option at the end
    let is_custom_selected = item_index == state.os_list_selected;
    let custom_style = if is_custom_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Magenta)
    };
    let prefix = if is_custom_selected { "> " } else { "  " };
    lines.push(Line::from(vec![
        Span::styled(prefix, custom_style),
        Span::styled("   Custom OS...", custom_style),
        Span::styled("  (Define your own)", Style::default().fg(Color::DarkGray)),
    ]));

    // Calculate scroll offset
    let visible_height = inner.height as usize;
    let scroll_offset = if state.os_list_selected >= visible_height {
        state.os_list_selected - visible_height + 1
    } else {
        0
    };

    // Render visible portion
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    let list = Paragraph::new(visible_lines);
    frame.render_widget(list, inner);
}

fn handle_step_select_os(app: &mut App, key: KeyEvent) -> Result<()> {
    let editing_name = app
        .wizard_state
        .as_ref()
        .map(|s| matches!(s.editing_field, Some(WizardField::VmName)))
        .unwrap_or(false);

    if editing_name {
        // Text input mode for VM name
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Tab => {
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = None;
                    state.update_folder_name(&app.config.vm_library_path);
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut state) = app.wizard_state {
                    state.vm_name.push(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut state) = app.wizard_state {
                    state.vm_name.pop();
                }
            }
            _ => {}
        }
    } else {
        // Normal navigation mode
        match key.code {
            KeyCode::Esc => {
                app.cancel_wizard();
            }
            KeyCode::Tab => {
                // Toggle to name editing
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = Some(WizardField::VmName);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                // Count total items first (immutable borrow)
                let total = count_os_list_items(app);
                // Then mutate
                if let Some(ref mut state) = app.wizard_state {
                    if state.os_list_selected < total.saturating_sub(1) {
                        state.os_list_selected += 1;
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut state) = app.wizard_state {
                    if state.os_list_selected > 0 {
                        state.os_list_selected -= 1;
                    }
                }
            }
            KeyCode::Char(' ') => {
                // Toggle category expansion or select OS
                handle_os_list_action(app, false);
            }
            KeyCode::Enter => {
                // Select OS or expand category, then proceed if valid
                handle_os_list_action(app, true);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Count total items in the OS list (categories + visible OSes + custom)
fn count_os_list_items(app: &App) -> usize {
    let state = app.wizard_state.as_ref().unwrap();
    let category_order = [
        "windows",
        "linux",
        "bsd",
        "unix",
        "macos",
        "mobile",
        "infrastructure",
        "utilities",
        "alternative",
        "retro",
        "classic-mac",
    ];

    let mut count = 0;
    for category in &category_order {
        let profiles = app.qemu_profiles.list_by_category(category);
        if profiles.is_empty() {
            continue;
        }
        count += 1; // Category header
        if state.is_category_expanded(category) {
            // Count visible profiles (with filter)
            for (os_id, profile) in &profiles {
                if !state.os_filter.is_empty() {
                    let filter_lower = state.os_filter.to_lowercase();
                    if !profile.display_name.to_lowercase().contains(&filter_lower)
                        && !os_id.to_lowercase().contains(&filter_lower)
                    {
                        continue;
                    }
                }
                count += 1;
            }
        }
    }
    count += 1; // Custom OS option
    count
}

/// Handle action on OS list item (space to toggle, enter to select and proceed)
fn handle_os_list_action(app: &mut App, proceed: bool) {
    // First, collect all the information we need without holding borrows
    let Some(ref state) = app.wizard_state else {
        return;
    };
    let selected = state.os_list_selected;
    let os_filter = state.os_filter.clone();
    let expanded_categories: Vec<String> = state.expanded_categories.clone();

    let category_order = [
        "windows",
        "linux",
        "bsd",
        "unix",
        "macos",
        "mobile",
        "infrastructure",
        "utilities",
        "alternative",
        "retro",
        "classic-mac",
    ];

    let mut item_index = 0;
    let mut action: Option<OsListAction> = None;

    for category in &category_order {
        let profiles = app.qemu_profiles.list_by_category(category);
        if profiles.is_empty() {
            continue;
        }

        // Category header
        if item_index == selected {
            action = Some(OsListAction::ToggleCategory(category.to_string()));
            break;
        }
        item_index += 1;

        // OS items (if expanded)
        let is_expanded = expanded_categories.iter().any(|c| c == *category);
        if is_expanded {
            for (os_id, profile) in &profiles {
                if !os_filter.is_empty() {
                    let filter_lower = os_filter.to_lowercase();
                    if !profile.display_name.to_lowercase().contains(&filter_lower)
                        && !os_id.to_lowercase().contains(&filter_lower)
                    {
                        continue;
                    }
                }

                if item_index == selected {
                    action = Some(OsListAction::SelectOs(os_id.to_string()));
                    break;
                }
                item_index += 1;
            }
        }

        if action.is_some() {
            break;
        }
    }

    // Check if custom OS was selected (at the end)
    if action.is_none() && item_index == selected {
        action = Some(OsListAction::CustomOs);
    }

    // Now execute the action
    match action {
        Some(OsListAction::ToggleCategory(cat)) => {
            if let Some(ref mut state) = app.wizard_state {
                state.toggle_category(&cat);
            }
        }
        Some(OsListAction::SelectOs(os_id)) => {
            app.wizard_select_os(&os_id);
            if proceed {
                if let Err(e) = app.wizard_next_step() {
                    if let Some(ref mut state) = app.wizard_state {
                        state.error_message = Some(e);
                    }
                }
            }
        }
        Some(OsListAction::CustomOs) => {
            app.wizard_use_custom_os();
        }
        None => {}
    }
}

/// Actions that can be taken on the OS list
enum OsListAction {
    ToggleCategory(String),
    SelectOs(String),
    CustomOs,
}

// =============================================================================
// Step 2: Select ISO
// =============================================================================

fn render_step_select_iso(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(
            " Create New VM ({}/5) - {} ",
            state.step.number(),
            state.step.title()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // OS info
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Header
            Constraint::Min(10),   // Options
            Constraint::Length(1), // Selected path
            Constraint::Length(2), // Help
        ])
        .split(inner);

    // OS info
    let os_name = state
        .selected_os
        .as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .map(|p| p.display_name.as_str())
        .unwrap_or("Custom OS");

    let os_info = Paragraph::new(format!("Operating System: {}", os_name))
        .style(Style::default().fg(Color::White));
    frame.render_widget(os_info, chunks[0]);

    // Header
    let header = Paragraph::new("Install Media:").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[2]);

    // Options
    let mut lines = Vec::new();
    let mut option_idx = 0;

    // BIOS/ROM option first (if the selected profile has bios_rom config)
    let bios_rom_config = state
        .selected_os
        .as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .and_then(|p| p.bios_rom.as_ref());

    if let Some(bios_config) = bios_rom_config {
        let req_label = if bios_config.required {
            " (REQUIRED)"
        } else {
            " (optional)"
        };
        let is_rom_selected = state.field_focus == option_idx;
        let rom_style = if is_rom_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let rom_prefix = if is_rom_selected { "> " } else { "  " };
        lines.push(Line::styled(
            format!(
                "{}( ) Browse for {} file...{}",
                rom_prefix, bios_config.label, req_label
            ),
            rom_style,
        ));

        if let Some(ref hint) = bios_config.hint {
            lines.push(Line::styled(
                format!("       {}", hint),
                Style::default().fg(Color::DarkGray),
            ));
        }

        if let Some(ref rom_path) = state.bios_rom_path {
            lines.push(Line::styled(
                format!("       ROM: {}", rom_path.display()),
                Style::default().fg(Color::Green),
            ));
        }

        lines.push(Line::from(""));
        option_idx += 1;
    }

    // Check if this OS has a free ISO URL
    let has_download = state
        .selected_os
        .as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .and_then(|p| p.iso_url.as_ref())
        .is_some();

    if has_download {
        let is_selected = state.field_focus == option_idx;
        let style = if is_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_selected { "> " } else { "  " };
        lines.push(Line::styled(
            format!("{}( ) Open download page in browser", prefix),
            style,
        ));
        option_idx += 1;
    }

    // Floppy image option (for OSes that need a boot floppy, e.g., OS/2)
    let is_floppy_selected = state.field_focus == option_idx;
    let floppy_style = if is_floppy_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let floppy_prefix = if is_floppy_selected { "> " } else { "  " };
    lines.push(Line::styled(
        format!("{}( ) Browse for boot floppy image...", floppy_prefix),
        floppy_style,
    ));
    if let Some(ref floppy_path) = state.floppy_path {
        lines.push(Line::styled(
            format!("       Floppy: {}", floppy_path.display()),
            Style::default().fg(Color::Green),
        ));
    }
    option_idx += 1;

    let is_browse_selected = state.field_focus == option_idx;
    let browse_style = if is_browse_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let browse_prefix = if is_browse_selected { "> " } else { "  " };
    lines.push(Line::styled(
        format!("{}( ) Browse for local ISO file...", browse_prefix),
        browse_style,
    ));
    option_idx += 1;

    let is_recovery_selected = state.field_focus == option_idx;
    let recovery_style = if is_recovery_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let recovery_prefix = if is_recovery_selected { "> " } else { "  " };
    lines.push(Line::styled(
        format!("{}( ) Browse for recovery image (DMG)...", recovery_prefix),
        recovery_style,
    ));
    option_idx += 1;

    let is_none_selected = state.field_focus == option_idx;
    let none_style = if is_none_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let none_prefix = if is_none_selected { "> " } else { "  " };
    lines.push(Line::styled(
        format!("{}( ) Skip (configure later)", none_prefix),
        none_style,
    ));

    let options = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(options, chunks[3]);

    // Selected path
    if let Some(ref path) = state.iso_path {
        let label = if state.is_recovery_image {
            "Selected recovery image"
        } else {
            "Selected ISO"
        };
        let path_text = Paragraph::new(format!("{}: {}", label, path.display()))
            .style(Style::default().fg(Color::Green));
        frame.render_widget(path_text, chunks[4]);
    }

    // Help
    let help = Paragraph::new("[j/k] Select  [Enter] Choose  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[5]);
}

fn handle_step_select_iso(app: &mut App, key: KeyEvent) -> Result<()> {
    let has_download = app
        .wizard_state
        .as_ref()
        .and_then(|s| s.selected_os.as_ref())
        .and_then(|id| app.qemu_profiles.get(id))
        .and_then(|p| p.iso_url.as_ref())
        .is_some();

    let has_bios_rom = app
        .wizard_state
        .as_ref()
        .and_then(|s| s.selected_os.as_ref())
        .and_then(|id| app.qemu_profiles.get(id))
        .and_then(|p| p.bios_rom.as_ref())
        .is_some();

    // Compute option indices matching the render order: ROM, download, floppy, browse, recovery, skip
    let mut idx = 0;
    let rom_idx = if has_bios_rom {
        let i = idx;
        idx += 1;
        Some(i)
    } else {
        None
    };
    let download_idx = if has_download {
        let i = idx;
        idx += 1;
        Some(i)
    } else {
        None
    };
    let floppy_idx = idx;
    idx += 1;
    let browse_idx = idx;
    idx += 1;
    let recovery_browse_idx = idx;
    idx += 1;
    let no_iso_idx = idx;
    idx += 1;
    let max_options = idx;

    match key.code {
        KeyCode::Esc => {
            app.wizard_prev_step();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.wizard_state {
                if state.field_focus < max_options - 1 {
                    state.field_focus += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.wizard_state {
                if state.field_focus > 0 {
                    state.field_focus -= 1;
                }
            }
        }
        KeyCode::Enter => {
            let focus = app
                .wizard_state
                .as_ref()
                .map(|s| s.field_focus)
                .unwrap_or(0);

            if Some(focus) == rom_idx {
                // Browse for ROM/BIOS file
                app.load_file_browser(crate::app::FileBrowserMode::Bios);
                app.push_screen(crate::app::Screen::FileBrowser);
            } else if Some(focus) == download_idx {
                // Open download page in browser
                if let Some(url) = app
                    .wizard_state
                    .as_ref()
                    .and_then(|s| s.selected_os.as_ref())
                    .and_then(|id| app.qemu_profiles.get(id))
                    .and_then(|p| p.iso_url.as_ref())
                {
                    let url = url.clone();
                    if let Err(e) = open_url_in_browser(&url) {
                        app.set_status(format!("Failed to open browser: {}", e));
                    } else {
                        app.set_status("Opened download page in browser. Use 'Browse for ISO' after downloading.");
                    }
                }
            } else if focus == floppy_idx {
                // Browse for floppy image - open file browser
                app.load_file_browser(crate::app::FileBrowserMode::Floppy);
                app.push_screen(crate::app::Screen::FileBrowser);
            } else if focus == browse_idx {
                // Browse for ISO - open file browser
                app.seed_iso_browser_dir();
                app.load_file_browser(crate::app::FileBrowserMode::Iso);
                app.push_screen(crate::app::Screen::FileBrowser);
            } else if focus == recovery_browse_idx {
                // Browse for recovery image (DMG) - open file browser
                app.load_file_browser(crate::app::FileBrowserMode::RecoveryImage);
                app.push_screen(crate::app::Screen::FileBrowser);
            } else if focus == no_iso_idx {
                // Skip - check if ROM is required but missing
                let rom_required_but_missing = app
                    .wizard_state
                    .as_ref()
                    .and_then(|s| s.selected_os.as_ref())
                    .and_then(|id| app.qemu_profiles.get(id))
                    .and_then(|p| p.bios_rom.as_ref())
                    .map(|b| b.required)
                    .unwrap_or(false)
                    && app
                        .wizard_state
                        .as_ref()
                        .map(|s| s.bios_rom_path.is_none())
                        .unwrap_or(false);

                if rom_required_but_missing {
                    if let Some(ref mut state) = app.wizard_state {
                        state.error_message = Some("Warning: ROM file is required for this OS but not selected. Proceeding anyway.".to_string());
                    }
                }

                if let Some(ref mut state) = app.wizard_state {
                    state.iso_path = None;
                    state.is_recovery_image = false;
                }
                let _ = app.wizard_next_step();
            }
        }
        _ => {}
    }
    Ok(())
}

// =============================================================================
// Step 3: Configure Disk
// =============================================================================

fn render_step_configure_disk(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(
            " Create New VM ({}/5) - {} ",
            state.step.number(),
            state.step.title()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Disk source toggle
            Constraint::Length(1), // Spacer
            Constraint::Min(10),   // Mode-specific content
            Constraint::Length(2), // Help
        ])
        .split(inner);

    // Header
    let header = Paragraph::new("Disk Configuration").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    // Disk source toggle (field_focus == 0)
    let source_focused = state.field_focus == 0;
    let prefix = if source_focused { "> " } else { "  " };

    let mut source_spans = vec![
        Span::styled(
            prefix,
            if source_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
        Span::styled("Disk Source: ", Style::default().fg(Color::Yellow)),
    ];
    for (i, source) in [
        WizardDiskSource::NewImage,
        WizardDiskSource::ExistingImage,
        WizardDiskSource::PhysicalDevice,
    ]
    .into_iter()
    .enumerate()
    {
        let style = if state.disk_source == source {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        source_spans.push(Span::styled(
            if i == 0 { "[ " } else { " ] [ " },
            Style::default(),
        ));
        source_spans.push(Span::styled(source.label(), style));
    }
    source_spans.push(Span::styled(" ]", Style::default()));
    if source_focused {
        source_spans.push(Span::styled(
            "  [←/→] toggle",
            Style::default().fg(Color::DarkGray),
        ));
    }
    let source_toggle = Paragraph::new(Line::from(source_spans));
    frame.render_widget(source_toggle, chunks[2]);

    // Mode-specific content area
    let content_area = chunks[4];

    match state.disk_source {
        WizardDiskSource::NewImage => render_new_disk_mode(app, frame, content_area),
        WizardDiskSource::ExistingImage => render_existing_disk_mode(app, frame, content_area),
        WizardDiskSource::PhysicalDevice => render_physical_disk_mode(app, frame, content_area),
    }

    // Help
    let help_text = match state.disk_source {
        WizardDiskSource::ExistingImage => {
            if state.field_focus == 0 {
                "[←/→] Toggle mode  [j/k] Navigate  [Enter] Next  [Esc] Back"
            } else if state.field_focus == 1 {
                "[Enter] Browse  [j/k] Navigate  [Esc] Back"
            } else {
                "[←/→] Toggle action  [j/k] Navigate  [Enter] Next  [Esc] Back"
            }
        }
        WizardDiskSource::PhysicalDevice => {
            if state.field_focus == 0 {
                "[←/→] Toggle mode  [j/k] Navigate  [Enter] Next  [Esc] Back"
            } else {
                "[Enter] Select disk  [j/k] Navigate  [Esc] Back"
            }
        }
        WizardDiskSource::NewImage => {
            let editing = matches!(state.editing_field, Some(WizardField::DiskSize));
            if editing {
                "[Enter] Done  [Backspace] Delete  [0-9] Enter size"
            } else if state.field_focus == 0 {
                "[←/→] Toggle mode  [j/k] Navigate  [Enter] Next  [Esc] Back"
            } else if state.field_focus == 1 {
                "[Tab] Edit size  [←/→] Adjust  [Enter] Next  [Esc] Back"
            } else {
                "[←/→] Toggle format  [j/k] Navigate  [Enter] Next  [Esc] Back"
            }
        }
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[5]);
}

/// Render the "Create New" disk mode content
fn render_new_disk_mode(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Disk size input
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Disk format toggle
            Constraint::Length(1), // Spacer
            Constraint::Min(5),    // Disk info
        ])
        .split(area);

    // Disk size input (field_focus == 1 when in new disk mode)
    let size_focused = state.field_focus == 1;
    let editing = matches!(state.editing_field, Some(WizardField::DiskSize));
    let size_style = if editing {
        Style::default().fg(Color::Yellow)
    } else if size_focused {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let border_style = if editing || size_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let recommended = app
        .wizard_selected_profile()
        .map(|p| p.disk_size_gb)
        .unwrap_or(32);

    let size_block = Block::default()
        .title(format!(" Disk Size (Recommended: {} GB) ", recommended))
        .borders(Borders::ALL)
        .border_style(border_style);

    // Show edit buffer when editing, otherwise show current value
    let size_display = if editing {
        format!(
            "{}|  (e.g., 500, 500GB, 512000MB)",
            state.wizard_edit_buffer
        )
    } else {
        format!("{} GB", state.disk_size_gb)
    };

    let size_text = Paragraph::new(size_display)
        .style(size_style)
        .block(size_block);
    frame.render_widget(size_text, sub_chunks[0]);

    let format_focused = state.field_focus == 2;
    let disk_format = app.create_wizard_disk_format;
    let qcow2_style = if matches!(disk_format, DiskImageFormat::Qcow2) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let raw_style = if matches!(disk_format, DiskImageFormat::Raw) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let prefix = if format_focused { "> " } else { "  " };
    let format_line = Line::from(vec![
        Span::styled(
            prefix,
            if format_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
        Span::styled("Format: ", Style::default().fg(Color::Yellow)),
        Span::styled("[ ", Style::default()),
        Span::styled("qcow2", qcow2_style),
        Span::styled(" ] [ ", Style::default()),
        Span::styled("raw", raw_style),
        Span::styled(" ]", Style::default()),
        if format_focused {
            Span::styled("  [←/→] toggle", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        },
    ]);
    frame.render_widget(Paragraph::new(format_line), sub_chunks[2]);

    // Disk info box
    let info_block = Block::default()
        .title(" Disk Info ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));

    let disk_path = app
        .wizard_vm_path()
        .map(|p| p.join(format!("{}.{}", state.folder_name, disk_format.extension())))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("~/vm-space/<vm-name>/<vm-name>.{}", disk_format.extension()));

    let info_text = vec![
        Line::from(vec![
            Span::styled("Format: ", Style::default().fg(Color::Yellow)),
            Span::raw(disk_format.description()),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Yellow)),
            Span::raw(disk_format.storage_description()),
        ]),
        Line::from(vec![
            Span::styled("Location: ", Style::default().fg(Color::Yellow)),
            Span::raw(disk_path),
        ]),
    ];

    let info = Paragraph::new(info_text)
        .block(info_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(info, sub_chunks[4]);
}

/// Render the "Use Existing" disk mode content
fn render_existing_disk_mode(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Browse / selected path
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Action toggle
            Constraint::Length(1), // Spacer
            Constraint::Min(3),    // Note
        ])
        .split(area);

    // Browse button / selected path (field_focus == 1)
    let browse_focused = state.field_focus == 1;
    let browse_border = if browse_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let browse_block = Block::default()
        .title(" Disk Image ")
        .borders(Borders::ALL)
        .border_style(browse_border);

    let browse_inner = browse_block.inner(sub_chunks[0]);
    frame.render_widget(browse_block, sub_chunks[0]);

    let browse_text = if let Some(ref path) = state.existing_disk_path {
        let path_str = path.display().to_string();
        // Truncate path if too long
        let max_len = browse_inner.width as usize - 2;
        let display = if path_str.len() > max_len {
            format!("...{}", &path_str[path_str.len() - max_len + 3..])
        } else {
            path_str
        };
        Paragraph::new(display).style(Style::default().fg(Color::Green))
    } else {
        let prefix = if browse_focused { "> " } else { "  " };
        Paragraph::new(format!("{}( ) Browse for disk image...", prefix)).style(if browse_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        })
    };
    frame.render_widget(browse_text, browse_inner);

    // Action toggle (field_focus == 2)
    let action_focused = state.field_focus == 2;
    let copy_style = if matches!(state.existing_disk_action, DiskAction::Copy) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let move_style = if matches!(state.existing_disk_action, DiskAction::Move) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let prefix = if action_focused { "> " } else { "  " };

    let action_line = Line::from(vec![
        Span::styled(
            prefix,
            if action_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
        Span::styled("Action: ", Style::default().fg(Color::Yellow)),
        Span::styled("[ ", Style::default()),
        Span::styled("Copy to VM folder", copy_style),
        Span::styled(" ] [ ", Style::default()),
        Span::styled("Move to VM folder", move_style),
        Span::styled(" ]", Style::default()),
    ]);
    let action_toggle = Paragraph::new(action_line);
    frame.render_widget(action_toggle, sub_chunks[2]);

    // Note about renaming
    let note_text = format!(
        "Note: The disk will be renamed to match its detected format under {}",
        state.folder_name
    );
    let note = Paragraph::new(note_text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(note, sub_chunks[4]);
}

/// Render the "Physical Disk" passthrough mode content
fn render_physical_disk_mode(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Selected device / select button
            Constraint::Length(1), // Spacer
            Constraint::Min(5),    // Danger warning
        ])
        .split(area);

    // Selected device box (field_focus == 1)
    let select_focused = state.field_focus == 1;
    let select_border = if select_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let select_block = Block::default()
        .title(" Physical Disk ")
        .borders(Borders::ALL)
        .border_style(select_border);
    let select_inner = select_block.inner(sub_chunks[0]);
    frame.render_widget(select_block, sub_chunks[0]);

    let select_text = if let Some(ref disk) = state.physical_disk {
        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!("{} ", disk.model),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("({}, {})", disk.size_display(), disk.bus.label()),
                Style::default().fg(Color::Green),
            ),
        ])];
        let mut path_line = disk.launch_path().display().to_string();
        if disk.by_id_path.is_none() {
            path_line.push_str("  (no stable by-id path found)");
        }
        lines.push(Line::from(Span::styled(
            path_line,
            Style::default().fg(Color::DarkGray),
        )));
        Paragraph::new(lines)
    } else {
        let prefix = if select_focused { "> " } else { "  " };
        Paragraph::new(format!("{}( ) Select physical disk...", prefix)).style(if select_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        })
    };
    frame.render_widget(select_text, select_inner);

    // Persistent danger warning
    let warning = Paragraph::new(vec![
        Line::from(Span::styled(
            "⚠ DANGER: destructive operation",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "The guest OS gets full read/write control of this disk. ALL existing",
            Style::default().fg(Color::Red),
        )),
        Line::from(Span::styled(
            "data on it can be modified or destroyed. Never select a disk the host",
            Style::default().fg(Color::Red),
        )),
        Line::from(Span::styled(
            "is using. The disk is passed through as-is; nothing is copied.",
            Style::default().fg(Color::Red),
        )),
    ]);
    frame.render_widget(warning, sub_chunks[2]);
}

fn handle_step_configure_disk(app: &mut App, key: KeyEvent) -> Result<()> {
    let (editing, disk_source, field_focus) = app
        .wizard_state
        .as_ref()
        .map(|s| {
            (
                matches!(s.editing_field, Some(WizardField::DiskSize)),
                s.disk_source,
                s.field_focus,
            )
        })
        .unwrap_or((false, WizardDiskSource::NewImage, 0));

    // Handle disk size editing mode (only in "Create New" mode)
    if editing && disk_source == WizardDiskSource::NewImage {
        match key.code {
            KeyCode::Esc => {
                // Cancel edit, restore original value (clear buffer)
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = None;
                    state.wizard_edit_buffer.clear();
                }
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Apply the edit with suffix support
                if let Some(ref mut state) = app.wizard_state {
                    let buffer = state.wizard_edit_buffer.clone();
                    if let Some(value) = parse_size_with_suffix(&buffer, "GB") {
                        state.disk_size_gb = value.clamp(1, 10000);
                    }
                    state.editing_field = None;
                    state.wizard_edit_buffer.clear();
                }
            }
            KeyCode::Char(c) if c.is_ascii_alphanumeric() => {
                if let Some(ref mut state) = app.wizard_state {
                    state.wizard_edit_buffer.push(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut state) = app.wizard_state {
                    state.wizard_edit_buffer.pop();
                }
            }
            KeyCode::Left | KeyCode::Right => {
                // Allow arrow keys to still adjust while editing
                if let Some(ref mut state) = app.wizard_state {
                    if key.code == KeyCode::Left {
                        state.disk_size_gb = state.disk_size_gb.saturating_sub(8).max(1);
                    } else {
                        state.disk_size_gb = (state.disk_size_gb + 8).min(10000);
                    }
                    // Update buffer to reflect new value
                    state.wizard_edit_buffer = state.disk_size_gb.to_string();
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Normal navigation mode
    match key.code {
        KeyCode::Esc => {
            app.wizard_prev_step();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.wizard_state {
                let max_focus = match state.disk_source {
                    WizardDiskSource::PhysicalDevice => 1,
                    _ => 2,
                };
                if state.field_focus < max_focus {
                    state.field_focus += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.wizard_state {
                if state.field_focus > 0 {
                    state.field_focus -= 1;
                }
            }
        }
        KeyCode::Left | KeyCode::Right => {
            if let Some(ref mut state) = app.wizard_state {
                match (state.field_focus, state.disk_source) {
                    (0, _) => {
                        // Cycle disk source mode
                        state.disk_source = if key.code == KeyCode::Left {
                            state.disk_source.prev()
                        } else {
                            state.disk_source.next()
                        };
                        state.field_focus = 0;
                    }
                    (1, WizardDiskSource::NewImage) => {
                        // Adjust disk size
                        if key.code == KeyCode::Left {
                            state.disk_size_gb = state.disk_size_gb.saturating_sub(8).max(1);
                        } else {
                            state.disk_size_gb = (state.disk_size_gb + 8).min(10000);
                        }
                    }
                    (2, WizardDiskSource::NewImage) => {
                        // Toggle new disk image format
                        app.create_wizard_disk_format = app.create_wizard_disk_format.toggle();
                    }
                    (2, WizardDiskSource::ExistingImage) => {
                        // Toggle copy/move action
                        state.existing_disk_action = match state.existing_disk_action {
                            DiskAction::Copy => DiskAction::Move,
                            DiskAction::Move => DiskAction::Copy,
                        };
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Tab => {
            // Enter edit mode for disk size (Create New mode only)
            if disk_source == WizardDiskSource::NewImage && field_focus == 1 {
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = Some(WizardField::DiskSize);
                    state.wizard_edit_buffer = state.disk_size_gb.to_string();
                }
            }
        }
        KeyCode::Enter => {
            if disk_source == WizardDiskSource::ExistingImage && field_focus == 1 {
                // Browse for a disk image file
                app.load_file_browser(FileBrowserMode::Disk);
                app.push_screen(crate::app::Screen::FileBrowser);
            } else if disk_source == WizardDiskSource::PhysicalDevice && field_focus == 1 {
                // Open the physical disk picker
                app.open_physical_disk_picker(crate::app::DiskPickerContext::Wizard);
            } else {
                // Try to proceed to next step
                if let Err(e) = app.wizard_next_step() {
                    if let Some(ref mut state) = app.wizard_state {
                        state.error_message = Some(e);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

// =============================================================================
// Step 4: Configure QEMU
// =============================================================================

/// QEMU field options for cycling through values
const VGA_OPTIONS: &[&str] = &["std", "virtio", "qxl", "cirrus", "vmware", "none"];
const NETWORK_OPTIONS: &[&str] = &["virtio", "e1000", "rtl8139", "ne2k_pci", "pcnet", "none"];
const DISK_INTERFACE_OPTIONS: &[&str] = &["virtio", "ide", "scsi"];
const DISPLAY_OPTIONS: &[&str] = &["gtk", "sdl", "spice-app", "vnc", "none"];
const AUDIO_OPTIONS: &[(&str, &[&str])] = &[
    ("Intel HDA", &["intel-hda", "hda-duplex"]),
    ("AC97", &["ac97"]),
    ("Sound Blaster 16", &["sb16"]),
    ("None", &[]),
];

/// Fields in the QEMU config screen. Network adapters are configured in a
/// separate list+editor overlay (opened from the `NetworkAdapters` row),
/// not as inline rows here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QemuField {
    Memory,
    CpuCores,
    Vga,
    Audio,
    NetworkAdapters,
    DiskInterface,
    Display,
    Kvm,
    GlAccel,
    Uefi,
    Tpm,
    UsbTablet,
    RtcLocal,
}

impl QemuField {
    fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Memory,
            1 => Self::CpuCores,
            2 => Self::Vga,
            3 => Self::Audio,
            4 => Self::NetworkAdapters,
            5 => Self::DiskInterface,
            6 => Self::Display,
            7 => Self::Kvm,
            8 => Self::GlAccel,
            9 => Self::Uefi,
            10 => Self::Tpm,
            11 => Self::UsbTablet,
            _ => Self::RtcLocal,
        }
    }

    fn count() -> usize {
        13
    }
}

fn render_step_configure_qemu(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(
            " Create New VM ({}/5) - {} ",
            state.step.number(),
            state.step.title()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into left (settings) and right (notes) panels
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(18),   // Settings
            Constraint::Length(2), // Help
        ])
        .split(h_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(18),   // Notes
        ])
        .split(h_chunks[1]);

    // Left side: Settings header
    let header = Paragraph::new("QEMU Settings").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, left_chunks[0]);

    // Settings list (editable)
    let config = &state.qemu_config;
    let focus = state.field_focus;
    let editing = state.editing_field.is_some();
    let mut lines = Vec::new();

    // Memory (editable)
    let mem_selected = focus == 0;
    let mem_editing = matches!(state.editing_field, Some(WizardField::MemoryMb));
    let mem_value = if mem_editing {
        format!("{}|", state.wizard_edit_buffer)
    } else {
        format!("{} MB", config.memory_mb)
    };
    let mem_hint = if mem_editing {
        "[Enter] Done  [Esc] Cancel"
    } else if mem_selected {
        "[Tab] Edit  [←/→] ±256MB"
    } else {
        ""
    };
    lines.push(render_field_line(
        "Memory:",
        &mem_value,
        mem_selected,
        mem_editing,
        mem_hint,
    ));

    // CPU Cores (editable)
    let cpu_selected = focus == 1;
    let cpu_editing = matches!(state.editing_field, Some(WizardField::CpuCores));
    let cpu_value = if cpu_editing {
        format!("{}|", state.wizard_edit_buffer)
    } else {
        format!("{}", config.cpu_cores)
    };
    let cpu_hint = if cpu_editing {
        "[Enter] Done  [Esc] Cancel"
    } else if cpu_selected {
        "[Tab] Edit  [←/→] ±1"
    } else {
        ""
    };
    lines.push(render_field_line(
        "CPU Cores:",
        &cpu_value,
        cpu_selected,
        cpu_editing,
        cpu_hint,
    ));

    // VGA (cycle)
    let vga_selected = focus == 2;
    lines.push(render_field_line(
        "Graphics:",
        &config.vga,
        vga_selected,
        false,
        "[←/→] cycle",
    ));

    // Audio (cycle)
    let audio_selected = focus == 3;
    let audio_label = get_audio_label(&config.audio);
    lines.push(render_field_line(
        "Audio:",
        audio_label,
        audio_selected,
        false,
        "[←/→] cycle",
    ));

    // Network adapters (opens the NIC list overlay)
    let net_selected = focus == 4;
    let net_display = format!(
        "{} adapter{}",
        config.network_adapters.len(),
        if config.network_adapters.len() == 1 { "" } else { "s" }
    );
    lines.push(render_field_line(
        "Network:",
        &net_display,
        net_selected,
        false,
        "[Enter] configure",
    ));

    // Disk Interface (cycle)
    let disk_selected = focus == 5;
    lines.push(render_field_line(
        "Disk I/F:",
        &config.disk_interface,
        disk_selected,
        false,
        "[←/→] cycle",
    ));

    // Display (cycle)
    let disp_selected = focus == 6;
    lines.push(render_field_line(
        "Display:",
        &config.display,
        disp_selected,
        false,
        "[←/→] cycle",
    ));

    lines.push(Line::from(""));
    lines.push(Line::styled(
        "  Features (toggle with Space):",
        Style::default().fg(Color::DarkGray),
    ));

    // KVM toggle
    let kvm_selected = focus == 7;
    lines.push(render_toggle_line(
        "KVM Accel:",
        config.enable_kvm,
        kvm_selected,
    ));

    // 3D/GL acceleration toggle
    let gl_selected = focus == 8;
    lines.push(render_toggle_line(
        "3D Accel:",
        config.gl_acceleration,
        gl_selected,
    ));

    // UEFI toggle
    let uefi_selected = focus == 9;
    lines.push(render_toggle_line("UEFI Boot:", config.uefi, uefi_selected));

    // TPM toggle
    let tpm_selected = focus == 10;
    lines.push(render_toggle_line("TPM 2.0:", config.tpm, tpm_selected));

    // USB Tablet toggle
    let usb_selected = focus == 11;
    lines.push(render_toggle_line(
        "USB Tablet:",
        config.usb_tablet,
        usb_selected,
    ));

    // RTC Local toggle
    let rtc_selected = focus == 12;
    lines.push(render_toggle_line(
        "RTC Local:",
        config.rtc_localtime,
        rtc_selected,
    ));

    let settings = Paragraph::new(lines);
    frame.render_widget(settings, left_chunks[1]);

    // Help text
    let help_text = if editing {
        "[Enter] Done  [Esc] Cancel  [←/→] Adjust"
    } else {
        "[j/k] Navigate  [Tab] Edit  [←/→] Change  [Space] Toggle  [Enter] Next"
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, left_chunks[2]);

    // Right side: Notes header
    let notes_header = Paragraph::new("Why These Defaults?").style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(notes_header, right_chunks[0]);

    // Right side: Explanation notes
    let notes_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let notes_inner = notes_block.inner(right_chunks[1]);
    frame.render_widget(notes_block, right_chunks[1]);

    // Build notes based on selected field and profile
    let notes_text = get_field_notes(app, focus);
    let notes = Paragraph::new(notes_text)
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: false });
    frame.render_widget(notes, notes_inner);
}

fn render_field_line(
    label: &str,
    value: &str,
    selected: bool,
    editing: bool,
    hint: &str,
) -> Line<'static> {
    let prefix = if selected { "> " } else { "  " };
    let label_style = Style::default().fg(Color::Yellow);
    let value_style = if editing {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let hint_style = Style::default().fg(Color::DarkGray);

    Line::from(vec![
        Span::styled(
            prefix.to_string(),
            if selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
        Span::styled(format!("{:12}", label), label_style),
        Span::styled(format!("{:15}", value), value_style),
        Span::styled(
            if selected {
                hint.to_string()
            } else {
                String::new()
            },
            hint_style,
        ),
    ])
}

fn render_toggle_line(label: &str, enabled: bool, selected: bool) -> Line<'static> {
    let prefix = if selected { "> " } else { "  " };
    let checkbox = if enabled { "[x]" } else { "[ ]" };
    let label_style = Style::default().fg(Color::Yellow);
    let value_style = if selected {
        Style::default()
            .fg(if enabled { Color::Green } else { Color::Red })
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(if enabled {
            Color::Green
        } else {
            Color::DarkGray
        })
    };

    Line::from(vec![
        Span::styled(
            prefix.to_string(),
            if selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
        Span::styled(format!("{:12}", label), label_style),
        Span::styled(checkbox.to_string(), value_style),
    ])
}

fn get_audio_label(audio: &[String]) -> &'static str {
    if audio.is_empty() {
        "None"
    } else if audio.iter().any(|a| a.contains("intel-hda")) {
        "Intel HDA"
    } else if audio.iter().any(|a| a.contains("ac97")) {
        "AC97"
    } else if audio.iter().any(|a| a.contains("sb16")) {
        "Sound Blaster 16"
    } else {
        "Custom"
    }
}

fn get_field_notes(app: &App, focus: usize) -> String {
    let profile = app.wizard_selected_profile();
    let profile_notes = profile
        .and_then(|p| p.notes.as_ref())
        .cloned()
        .unwrap_or_default();
    let os_name = profile
        .map(|p| p.display_name.as_str())
        .unwrap_or("this OS");

    let field = QemuField::from_index(focus);

    let explanation = match field {
        QemuField::Memory => format!(
            "RAM for {}.\n\n\
            Modern OSes need 4GB+. Older systems may crash with too much RAM.\n\n\
            Windows 95: max 480MB\n\
            Windows 98/ME: max 512MB\n\
            Windows XP: 512MB-1GB\n\
            Linux GUI: 2GB minimum",
            os_name
        ),
        QemuField::CpuCores => format!(
            "CPU cores for {}.\n\n\
            More cores = faster for multi-threaded tasks.\n\n\
            Old OSes (pre-2000) may not support multiple CPUs.\n\
            Don't exceed your host's core count.",
            os_name
        ),
        QemuField::Vga => format!(
            "Graphics adapter for {}.\n\n\
            std: Safe, universal\n\
            virtio: Best Linux perf\n\
            qxl: Best for Windows/Spice\n\
            cirrus: Old OS compat\n\
            vmware: macOS guest\n\
            none: Headless server",
            os_name
        ),
        QemuField::Audio => format!(
            "Audio device for {}.\n\n\
            Intel HDA: Modern (Win Vista+)\n\
            AC97: Win 2000/XP era\n\
            SB16: DOS/Win 9x games\n\
            None: Server/headless",
            os_name
        ),
        QemuField::NetworkAdapters => format!(
            "Network adapters for {}.\n\n\
            A VM can have any number of network adapters, each with \
            its own model, backend, and (for bridge) target network.\n\n\
            Press Enter to open the adapter list, where you can select \
            an adapter to configure, add a new one, or delete one.",
            os_name
        ),
        QemuField::DiskInterface => format!(
            "Disk interface for {}.\n\n\
            virtio: Best perf (needs driver)\n\
            ide: Universal compat\n\
            scsi: Server workloads",
            os_name
        ),
        QemuField::Display => format!(
            "Display output for {}.\n\n\
            gtk: Native Linux window\n\
            sdl: Cross-platform\n\
            spice-app: SPICE protocol (needs virt-viewer)\n\
            vnc: Remote access only\n\
            none: Headless, no graphical output",
            os_name
        ),
        QemuField::Kvm => "KVM hardware acceleration.\n\n\
            Enables near-native speed using CPU virtualization.\n\n\
            Requires: Linux host with Intel VT-x or AMD-V.\n\
            Disable for: Non-x86 guests, nested virt issues."
            .to_string(),
        QemuField::GlAccel => "3D/OpenGL acceleration.\n\n\
            Hardware-accelerated 3D graphics via virtio-gpu.\n\n\
            Requires: virtio VGA (auto-set when enabled)\n\
            Best for: Linux guests, Android x86\n\
            Not for: Windows (no virtio 3D), retro OSes"
            .to_string(),
        QemuField::Uefi => format!(
            "UEFI boot mode for {}.\n\n\
            Modern boot firmware (vs legacy BIOS).\n\n\
            Required: Windows 11, some Linux installs\n\
            Optional: Windows 8+, modern Linux\n\
            Incompatible: DOS, Win 9x, old systems",
            os_name
        ),
        QemuField::Tpm => "TPM 2.0 emulation.\n\n\
            Trusted Platform Module for security features.\n\n\
            Required: Windows 11\n\
            Optional: BitLocker, Secure Boot\n\
            Not needed: Most other OSes"
            .to_string(),
        QemuField::UsbTablet => "USB tablet device.\n\n\
            Provides seamless mouse integration (no capture).\n\n\
            Recommended: Most modern systems\n\
            Disable: Old OSes with USB issues"
            .to_string(),
        QemuField::RtcLocal => "RTC in local time.\n\n\
            Sets hardware clock to local timezone.\n\n\
            Enable: Windows (expects local time)\n\
            Disable: Linux/Unix (expects UTC)"
            .to_string(),
    };

    if profile_notes.is_empty() {
        explanation
    } else {
        format!("{}\n\n---\nProfile note:\n{}", explanation, profile_notes)
    }
}

fn handle_step_configure_qemu(app: &mut App, key: KeyEvent) -> Result<()> {
    // Handle wizard port forward editing (nested inside the NIC field editor)
    if app.wizard_editing_port_forwards {
        return handle_wizard_port_forward_editor(app, key);
    }

    // Handle the per-NIC field editor (nested inside the NIC list)
    if app.wizard_editing_nic_fields {
        return handle_wizard_nic_editor_key(app, key);
    }

    // Handle the NIC list overlay
    if app.wizard_editing_nics {
        return handle_wizard_nic_list_key(app, key);
    }

    // Check if we're in edit mode for Memory or CPU
    let editing_memory = app
        .wizard_state
        .as_ref()
        .map(|s| matches!(s.editing_field, Some(WizardField::MemoryMb)))
        .unwrap_or(false);
    let editing_cpu = app
        .wizard_state
        .as_ref()
        .map(|s| matches!(s.editing_field, Some(WizardField::CpuCores)))
        .unwrap_or(false);

    if editing_memory || editing_cpu {
        // Text input mode for Memory or CPU
        match key.code {
            KeyCode::Esc => {
                // Cancel edit, restore original value
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = None;
                    state.wizard_edit_buffer.clear();
                }
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Apply the edit
                if let Some(ref mut state) = app.wizard_state {
                    let buffer = state.wizard_edit_buffer.clone();
                    if editing_memory {
                        // Parse with suffix support (target: MB)
                        if let Some(value) = parse_size_with_suffix(&buffer, "MB") {
                            state.qemu_config.memory_mb = value.clamp(128, 1048576);
                        }
                    } else if editing_cpu {
                        // Parse as plain number
                        if let Ok(value) = buffer.trim().parse::<u32>() {
                            state.qemu_config.cpu_cores = value.clamp(1, 256);
                        }
                    }
                    state.editing_field = None;
                    state.wizard_edit_buffer.clear();
                }
            }
            KeyCode::Char(c) if c.is_ascii_alphanumeric() => {
                if let Some(ref mut state) = app.wizard_state {
                    state.wizard_edit_buffer.push(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut state) = app.wizard_state {
                    state.wizard_edit_buffer.pop();
                }
            }
            KeyCode::Left | KeyCode::Right => {
                // Allow arrow keys to still adjust while editing
                let delta = if key.code == KeyCode::Right {
                    1i32
                } else {
                    -1i32
                };
                handle_qemu_field_change(app, delta);
                // Update buffer to reflect new value
                if let Some(ref mut state) = app.wizard_state {
                    if editing_memory {
                        state.wizard_edit_buffer = state.qemu_config.memory_mb.to_string();
                    } else if editing_cpu {
                        state.wizard_edit_buffer = state.qemu_config.cpu_cores.to_string();
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            app.wizard_prev_step();
        }
        KeyCode::Enter => {
            // Open the NIC list overlay only if NetworkAdapters is focused.
            let on_network_adapters = app
                .wizard_state
                .as_ref()
                .map(|s| QemuField::from_index(s.field_focus) == QemuField::NetworkAdapters)
                .unwrap_or(false);
            if on_network_adapters {
                let active_nic = app
                    .wizard_state
                    .as_ref()
                    .map(|s| s.qemu_config.active_nic)
                    .unwrap_or(0);
                app.wizard_nic_list_cursor = active_nic;
                app.wizard_editing_nics = true;
            } else {
                let _ = app.wizard_next_step();
            }
        }
        KeyCode::Tab => {
            // Enter edit mode for Memory or CPU fields
            if let Some(ref mut state) = app.wizard_state {
                let field = QemuField::from_index(state.field_focus);
                match field {
                    QemuField::Memory => {
                        state.editing_field = Some(WizardField::MemoryMb);
                        state.wizard_edit_buffer = state.qemu_config.memory_mb.to_string();
                    }
                    QemuField::CpuCores => {
                        state.editing_field = Some(WizardField::CpuCores);
                        state.wizard_edit_buffer = state.qemu_config.cpu_cores.to_string();
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.wizard_state {
                if state.field_focus < QemuField::count() - 1 {
                    state.field_focus += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.wizard_state {
                if state.field_focus > 0 {
                    state.field_focus -= 1;
                }
            }
        }
        KeyCode::Left | KeyCode::Right => {
            let delta = if key.code == KeyCode::Right {
                1i32
            } else {
                -1i32
            };
            handle_qemu_field_change(app, delta);
            // Show warning if spice-app selected without viewer
            if let Some(ref state) = app.wizard_state {
                if state.qemu_config.display.contains("spice")
                    && !crate::commands::qemu_system::is_spice_viewer_available()
                {
                    app.set_status(
                        "Warning: spice-app requires virt-viewer/remote-viewer to be installed",
                    );
                }
            }
        }
        KeyCode::Char(' ') => {
            // Toggle for boolean fields
            if let Some(ref mut state) = app.wizard_state {
                let field = QemuField::from_index(state.field_focus);
                match field {
                    QemuField::Kvm => state.qemu_config.enable_kvm = !state.qemu_config.enable_kvm,
                    QemuField::GlAccel => {
                        state.qemu_config.gl_acceleration = !state.qemu_config.gl_acceleration;
                        // Enabling GL acceleration requires virtio VGA and works best with SDL
                        if state.qemu_config.gl_acceleration {
                            if state.qemu_config.vga != "virtio" {
                                state.qemu_config.vga = "virtio".to_string();
                            }
                            // SDL has better performance for 3D acceleration than GTK
                            if state.qemu_config.display == "gtk" {
                                state.qemu_config.display = "sdl".to_string();
                            }
                        }
                    }
                    QemuField::Uefi => state.qemu_config.uefi = !state.qemu_config.uefi,
                    QemuField::Tpm => state.qemu_config.tpm = !state.qemu_config.tpm,
                    QemuField::UsbTablet => {
                        state.qemu_config.usb_tablet = !state.qemu_config.usb_tablet
                    }
                    QemuField::RtcLocal => {
                        state.qemu_config.rtc_localtime = !state.qemu_config.rtc_localtime
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            // Reset to profile defaults
            if let Some(profile) = app.wizard_selected_profile().cloned() {
                if let Some(ref mut state) = app.wizard_state {
                    state.qemu_config = WizardQemuConfig::from_profile(&profile);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_wizard_port_forward_editor(app: &mut App, key: KeyEvent) -> Result<()> {
    use crate::app::{AddPfStep, AddingPortForward};
    use crate::vm::qemu_config::{PortForward, PortProtocol};

    // Handle adding mode
    if let Some(ref mut adding) = app.wizard_adding_pf {
        match key.code {
            KeyCode::Esc => {
                app.wizard_adding_pf = None;
            }
            KeyCode::Enter => match adding.step {
                AddPfStep::Protocol => {
                    adding.step = AddPfStep::HostPort;
                }
                AddPfStep::HostPort => {
                    if adding.host_port_input.parse::<u16>().is_ok() {
                        adding.step = AddPfStep::GuestPort;
                    }
                }
                AddPfStep::GuestPort => {
                    if let (Ok(host), Ok(guest)) = (
                        adding.host_port_input.parse::<u16>(),
                        adding.guest_port_input.parse::<u16>(),
                    ) {
                        let pf = PortForward {
                            protocol: adding.protocol,
                            host_port: host,
                            guest_port: guest,
                        };
                        if let Some(ref mut state) = app.wizard_state {
                            let active_nic = state.qemu_config.active_nic;
                            state.qemu_config.network_adapters[active_nic]
                                .port_forwards
                                .push(pf);
                        }
                        app.wizard_adding_pf = None;
                    }
                }
            },
            KeyCode::Left | KeyCode::Right => {
                if adding.step == AddPfStep::Protocol {
                    adding.protocol = match adding.protocol {
                        PortProtocol::Tcp => PortProtocol::Udp,
                        PortProtocol::Udp => PortProtocol::Tcp,
                    };
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => match adding.step {
                AddPfStep::HostPort => adding.host_port_input.push(c),
                AddPfStep::GuestPort => adding.guest_port_input.push(c),
                _ => {}
            },
            KeyCode::Backspace => match adding.step {
                AddPfStep::HostPort => {
                    adding.host_port_input.pop();
                }
                AddPfStep::GuestPort => {
                    adding.guest_port_input.pop();
                }
                _ => {}
            },
            _ => {}
        }
        return Ok(());
    }

    // Normal port forward list mode
    match key.code {
        KeyCode::Esc => {
            app.wizard_editing_port_forwards = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let pf_len = app
                .wizard_state
                .as_ref()
                .map(|s| s.qemu_config.network_adapters[s.qemu_config.active_nic]
                    .port_forwards
                    .len())
                .unwrap_or(0);
            if app.wizard_pf_selected < pf_len.saturating_sub(1) {
                app.wizard_pf_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.wizard_pf_selected > 0 {
                app.wizard_pf_selected -= 1;
            }
        }
        KeyCode::Char('a') | KeyCode::Enter => {
            app.wizard_adding_pf = Some(AddingPortForward {
                step: AddPfStep::Protocol,
                protocol: PortProtocol::Tcp,
                host_port_input: String::new(),
                guest_port_input: String::new(),
            });
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(ref mut state) = app.wizard_state {
                let active_nic = state.qemu_config.active_nic;
                let port_forwards = &mut state.qemu_config.network_adapters[active_nic].port_forwards;
                if !port_forwards.is_empty() && app.wizard_pf_selected < port_forwards.len() {
                    port_forwards.remove(app.wizard_pf_selected);
                    if app.wizard_pf_selected >= port_forwards.len() && app.wizard_pf_selected > 0
                    {
                        app.wizard_pf_selected -= 1;
                    }
                }
            }
        }
        // Presets
        KeyCode::Char('1') => add_wizard_preset(app, PortProtocol::Tcp, 2222, 22),
        KeyCode::Char('2') => add_wizard_preset(app, PortProtocol::Tcp, 13389, 3389),
        KeyCode::Char('3') => add_wizard_preset(app, PortProtocol::Tcp, 8080, 80),
        KeyCode::Char('4') => add_wizard_preset(app, PortProtocol::Tcp, 8443, 443),
        KeyCode::Char('5') => add_wizard_preset(app, PortProtocol::Tcp, 15900, 5900),
        _ => {}
    }
    Ok(())
}

/// Handle key events for the wizard's NIC list overlay.
fn handle_wizard_nic_list_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let nic_count = app
        .wizard_state
        .as_ref()
        .map(|s| s.qemu_config.network_adapters.len())
        .unwrap_or(1);

    match key.code {
        KeyCode::Esc => {
            app.wizard_editing_nics = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if app.wizard_nic_list_cursor < nic_count.saturating_sub(1) {
                app.wizard_nic_list_cursor += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.wizard_nic_list_cursor > 0 {
                app.wizard_nic_list_cursor -= 1;
            }
        }
        KeyCode::Enter => {
            let cursor = app.wizard_nic_list_cursor;
            if let Some(ref mut state) = app.wizard_state {
                state.qemu_config.active_nic = cursor;
            }
            app.wizard_nic_field_focus = 0;
            app.wizard_editing_nic_fields = true;
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            // Add a new NIC and jump straight into editing it.
            if let Some(ref mut state) = app.wizard_state {
                state.qemu_config.network_adapters.push(NicConfig::default());
                state.qemu_config.active_nic = state.qemu_config.network_adapters.len() - 1;
                app.wizard_nic_list_cursor = state.qemu_config.active_nic;
            }
            app.wizard_nic_field_focus = 0;
            app.wizard_editing_nic_fields = true;
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let cursor = app.wizard_nic_list_cursor;
            let mut new_cursor = cursor;
            if let Some(ref mut state) = app.wizard_state {
                if cursor < state.qemu_config.network_adapters.len()
                    && state.qemu_config.network_adapters.len() > 1
                {
                    state.qemu_config.network_adapters.remove(cursor);
                    // Keep active_nic pointing at the same adapter it did
                    // before the removal (or the nearest one left).
                    if state.qemu_config.active_nic == cursor {
                        if state.qemu_config.active_nic
                            >= state.qemu_config.network_adapters.len()
                        {
                            state.qemu_config.active_nic =
                                state.qemu_config.network_adapters.len() - 1;
                        }
                    } else if state.qemu_config.active_nic > cursor {
                        state.qemu_config.active_nic -= 1;
                    }
                    if new_cursor >= state.qemu_config.network_adapters.len() {
                        new_cursor = state.qemu_config.network_adapters.len() - 1;
                    }
                }
            }
            app.wizard_nic_list_cursor = new_cursor;
        }
        _ => {}
    }
    Ok(())
}

/// Handle key events for the wizard's per-NIC field editor.
fn handle_wizard_nic_editor_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // MAC edit mode: capture text input first.
    let editing_mac = app
        .wizard_state
        .as_ref()
        .map(|s| matches!(s.editing_field, Some(WizardField::MacAddress)))
        .unwrap_or(false);
    if editing_mac {
        let mut bad_mac: Option<String> = None;
        if let Some(ref mut state) = app.wizard_state {
            match key.code {
                KeyCode::Esc => {
                    state.editing_field = None;
                    state.wizard_edit_buffer.clear();
                }
                KeyCode::Enter | KeyCode::Tab => {
                    let trimmed = state.wizard_edit_buffer.trim().to_string();
                    let active_nic = state.qemu_config.active_nic;
                    if trimmed.is_empty() {
                        state.qemu_config.network_adapters[active_nic].mac_address = None;
                        state.editing_field = None;
                        state.wizard_edit_buffer.clear();
                    } else if crate::vm::mac::is_valid_mac(&trimmed) {
                        state.qemu_config.network_adapters[active_nic].mac_address =
                            Some(trimmed.to_lowercase());
                        state.editing_field = None;
                        state.wizard_edit_buffer.clear();
                    } else {
                        bad_mac = Some(trimmed);
                    }
                }
                KeyCode::Char(c) if c.is_ascii_hexdigit() || c == ':' => {
                    if state.wizard_edit_buffer.len() < 17 {
                        state.wizard_edit_buffer.push(c);
                    }
                }
                KeyCode::Backspace => {
                    state.wizard_edit_buffer.pop();
                }
                _ => {}
            }
        }
        if let Some(bad) = bad_mac {
            app.set_status(format!("Invalid MAC address: {}", bad));
        }
        return Ok(());
    }

    let backend_stops = app.get_network_backend_stops();
    let system_bridges = app.bridge_picker_list();
    let default_bridge = system_bridges
        .first()
        .cloned()
        .or_else(|| Some("qemubr0".to_string()));

    let Some((show_pf, show_mac, max_field)) = app.wizard_state.as_ref().map(|state| {
        let nic = &state.qemu_config.network_adapters[state.qemu_config.active_nic];
        (
            nic.show_port_forwards(),
            nic.show_mac(),
            nic.max_editor_field(),
        )
    }) else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => {
            app.wizard_editing_nic_fields = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if app.wizard_nic_field_focus < max_field {
                app.wizard_nic_field_focus += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.wizard_nic_field_focus > 0 {
                app.wizard_nic_field_focus -= 1;
            }
        }
        KeyCode::Char('g') => {
            if app.wizard_nic_field_focus == 2 && show_mac {
                if let Some(ref mut state) = app.wizard_state {
                    let active_nic = state.qemu_config.active_nic;
                    state.qemu_config.network_adapters[active_nic].mac_address =
                        Some(crate::vm::mac::generate_random_mac());
                }
            }
        }
        KeyCode::Char('c') => {
            if app.wizard_nic_field_focus == 2 && show_mac {
                if let Some(ref mut state) = app.wizard_state {
                    let active_nic = state.qemu_config.active_nic;
                    state.qemu_config.network_adapters[active_nic].mac_address = None;
                }
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            if key.code == KeyCode::Tab && app.wizard_nic_field_focus == 2 && show_mac {
                // Tab on the MAC field opens text-edit mode (no "reverse"
                // equivalent — Shift+Tab there is a no-op, like Left/Right).
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = Some(WizardField::MacAddress);
                    let active_nic = state.qemu_config.active_nic;
                    state.wizard_edit_buffer = state.qemu_config.network_adapters[active_nic]
                        .mac_address
                        .clone()
                        .unwrap_or_default();
                }
                return Ok(());
            }
            // Cycle the focused field (Right/Tab forward, Left/Shift+Tab back).
            let delta = if matches!(key.code, KeyCode::Right | KeyCode::Tab) {
                1i32
            } else {
                -1i32
            };
            if let Some(ref mut state) = app.wizard_state {
                let active_nic = state.qemu_config.active_nic;
                match app.wizard_nic_field_focus {
                    0 => {
                        cycle_option(
                            &mut state.qemu_config.network_adapters[active_nic].model,
                            NETWORK_OPTIONS,
                            delta,
                        );
                    }
                    1 => {
                        state.qemu_config.network_adapters[active_nic].cycle_backend(
                            &backend_stops,
                            &default_bridge,
                            delta,
                        );
                    }
                    3 if state.qemu_config.network_adapters[active_nic].backend == "bridge" => {
                        if !system_bridges.is_empty() {
                            let current_bridge = state.qemu_config.network_adapters[active_nic]
                                .bridge_name
                                .as_deref()
                                .unwrap_or("");
                            let current_idx = system_bridges
                                .iter()
                                .position(|b| b == current_bridge)
                                .unwrap_or(0);
                            let new_idx = (current_idx as i32 + delta)
                                .rem_euclid(system_bridges.len() as i32)
                                as usize;
                            state.qemu_config.network_adapters[active_nic].bridge_name =
                                Some(system_bridges[new_idx].clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Enter => {
            if app.wizard_nic_field_focus == 2 && show_mac {
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = Some(WizardField::MacAddress);
                    let active_nic = state.qemu_config.active_nic;
                    state.wizard_edit_buffer = state.qemu_config.network_adapters[active_nic]
                        .mac_address
                        .clone()
                        .unwrap_or_default();
                }
            } else if app.wizard_nic_field_focus == 3 && show_pf {
                app.wizard_editing_port_forwards = true;
                app.wizard_pf_selected = 0;
                app.wizard_adding_pf = None;
            }
            // No action on this field — Enter never leaves the editor;
            // only Esc does.
        }
        _ => {}
    }
    Ok(())
}

fn add_wizard_preset(
    app: &mut App,
    protocol: crate::vm::qemu_config::PortProtocol,
    host_port: u16,
    guest_port: u16,
) {
    if let Some(ref mut state) = app.wizard_state {
        let active_nic = state.qemu_config.active_nic;
        let port_forwards = &mut state.qemu_config.network_adapters[active_nic].port_forwards;
        if !port_forwards
            .iter()
            .any(|pf| pf.host_port == host_port && pf.guest_port == guest_port)
        {
            port_forwards.push(crate::vm::qemu_config::PortForward {
                protocol,
                host_port,
                guest_port,
            });
        }
    }
}

/// Render the NIC list overlay as a popup over the wizard dialog.
fn render_wizard_nic_list(app: &App, frame: &mut Frame, parent: Rect) {
    let Some(state) = app.wizard_state.as_ref() else {
        return;
    };

    let width = parent.width.saturating_sub(8).min(64);
    let height = parent.height.saturating_sub(6).min(16);
    let x = parent.x + (parent.width.saturating_sub(width)) / 2;
    let y = parent.y + (parent.height.saturating_sub(height)) / 2;
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Network Adapters ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let mut lines = Vec::new();
    for (i, nic) in state.qemu_config.network_adapters.iter().enumerate() {
        let selected = i == app.wizard_nic_list_cursor;
        let prefix = if selected { "> " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::styled(
            format!("{}NIC {}: {}", prefix, i + 1, nic.describe()),
            style,
        ));
    }
    frame.render_widget(Paragraph::new(lines), chunks[0]);

    let help = Paragraph::new("[Enter] Edit  [a] Add  [d] Delete  [j/k] Navigate  [Esc] Done")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[1]);
}

/// Render the per-NIC field editor overlay, on top of the NIC list.
fn render_wizard_nic_editor(app: &App, frame: &mut Frame, parent: Rect) {
    let Some(state) = app.wizard_state.as_ref() else {
        return;
    };
    let nic = &state.qemu_config.network_adapters[state.qemu_config.active_nic];
    let is_bridge = nic.is_bridge();
    let show_mac = nic.show_mac();
    let show_pf = nic.show_port_forwards();
    let focus = app.wizard_nic_field_focus;

    let width = parent.width.saturating_sub(10).min(60);
    let height = parent.height.saturating_sub(8).min(14);
    let x = parent.x + (parent.width.saturating_sub(width)) / 2;
    let y = parent.y + (parent.height.saturating_sub(height)) / 2;
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(
            " NIC {} of {} ",
            state.qemu_config.active_nic + 1,
            state.qemu_config.network_adapters.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Adapter
            Constraint::Length(1), // Backend
            Constraint::Length(1), // MAC
            Constraint::Length(1), // Bridge / Forwards
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(inner);

    render_nic_field_row(
        frame,
        chunks[0],
        "Adapter:",
        &nic.model,
        focus == 0,
        false,
        "[←/→/Tab] cycle",
    );

    let backend_display = nic.backend_display();
    render_nic_field_row(
        frame,
        chunks[1],
        "Backend:",
        &backend_display,
        focus == 1,
        false,
        "[←/→/Tab] cycle",
    );

    if show_mac {
        let mac_editing = matches!(state.editing_field, Some(WizardField::MacAddress));
        let mac_value = if mac_editing {
            format!("{}|", state.wizard_edit_buffer)
        } else if let Some(mac) = nic.mac_address.as_deref() {
            mac.to_string()
        } else {
            "(auto)".to_string()
        };
        let mac_hint = if mac_editing {
            "[Enter] Done  [Esc] Cancel"
        } else if focus == 2 {
            "[Tab] Edit  [g] Generate  [c] Clear"
        } else {
            ""
        };
        render_nic_field_row(
            frame,
            chunks[2],
            "MAC:",
            &mac_value,
            focus == 2,
            mac_editing,
            mac_hint,
        );
    }

    if is_bridge {
        let bridge_display = nic.bridge_name.as_deref().unwrap_or("qemubr0");
        render_nic_field_row(
            frame,
            chunks[3],
            "Bridge:",
            bridge_display,
            focus == 3,
            false,
            "[←/→/Tab] cycle",
        );
    } else if show_pf {
        let pf_display = if nic.port_forwards.is_empty() {
            "none".to_string()
        } else {
            format!("{} rule(s)", nic.port_forwards.len())
        };
        render_nic_field_row(
            frame,
            chunks[3],
            "Forwards:",
            &pf_display,
            focus == 3,
            false,
            "[Enter] edit",
        );
    }

    let help = Paragraph::new("[j/k] Navigate  [←/→] Change  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[5]);
}

/// Render a single field row directly into `area` (used by the compact
/// per-NIC editor popup, which has one row per chunk rather than a single
/// scrolling list).
fn render_nic_field_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    selected: bool,
    editing: bool,
    hint: &str,
) {
    let line = render_field_line(label, value, selected, editing, hint);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_wizard_port_forward_editor(app: &App, frame: &mut Frame, parent: Rect) {
    let Some(state) = app.wizard_state.as_ref() else {
        return;
    };

    // Centered popup, sized to the parent dialog.
    let width = parent.width.saturating_sub(8).min(64);
    let height = parent.height.saturating_sub(6).min(16);
    let x = parent.x + (parent.width.saturating_sub(width)) / 2;
    let y = parent.y + (parent.height.saturating_sub(height)) / 2;
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Port Forwarding ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Add-rule sub-dialog takes over the whole popup when active.
    if let Some(adding) = app.wizard_adding_pf.as_ref() {
        render_wizard_adding_pf(adding, frame, inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(3),    // Rules list
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Presets
            Constraint::Length(1), // Help
        ])
        .split(inner);

    // Rules list
    let pfs = &state.qemu_config.network_adapters[state.qemu_config.active_nic].port_forwards;
    if pfs.is_empty() {
        let msg = Paragraph::new("  No port forwarding rules configured.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, chunks[0]);
    } else {
        let mut lines = Vec::new();
        for (i, pf) in pfs.iter().enumerate() {
            let is_selected = i == app.wizard_pf_selected;
            let prefix = if is_selected { "> " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::styled(
                format!(
                    "{}{}  {} -> {}",
                    prefix, pf.protocol, pf.host_port, pf.guest_port
                ),
                style,
            ));
        }
        frame.render_widget(Paragraph::new(lines), chunks[0]);
    }

    let presets = Paragraph::new("  Presets: [1] SSH  [2] RDP  [3] HTTP  [4] HTTPS  [5] VNC")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(presets, chunks[2]);

    let help = Paragraph::new("[a] Add  [d] Delete  [1-5] Preset  [Esc] Done")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
}

/// Render the "add a port forward rule" prompt inside the editor popup.
fn render_wizard_adding_pf(adding: &crate::app::AddingPortForward, frame: &mut Frame, area: Rect) {
    use crate::app::AddPfStep;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Protocol
            Constraint::Length(1), // Host port
            Constraint::Length(1), // Guest port
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(area);

    let header = Paragraph::new("Add Port Forward Rule").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    let active_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let idle_style = Style::default().fg(Color::White);
    let hint_style = Style::default().fg(Color::DarkGray);

    // Protocol
    let proto_active = adding.step == AddPfStep::Protocol;
    let proto_line = Line::from(vec![
        Span::styled("  Protocol:   ", Style::default().fg(Color::Yellow)),
        Span::styled(
            format!("{}", adding.protocol),
            if proto_active {
                active_style
            } else {
                idle_style
            },
        ),
        Span::styled(
            if proto_active {
                "  [←/→] toggle"
            } else {
                ""
            },
            hint_style,
        ),
    ]);
    frame.render_widget(Paragraph::new(proto_line), chunks[2]);

    // Host port
    let host_active = adding.step == AddPfStep::HostPort;
    let host_value = if adding.host_port_input.is_empty() {
        "_".to_string()
    } else if host_active {
        format!("{}|", adding.host_port_input)
    } else {
        adding.host_port_input.clone()
    };
    let host_line = Line::from(vec![
        Span::styled("  Host Port:  ", Style::default().fg(Color::Yellow)),
        Span::styled(
            host_value,
            if host_active {
                active_style
            } else {
                idle_style
            },
        ),
    ]);
    frame.render_widget(Paragraph::new(host_line), chunks[3]);

    // Guest port
    let guest_active = adding.step == AddPfStep::GuestPort;
    let guest_value = if adding.guest_port_input.is_empty() {
        "_".to_string()
    } else if guest_active {
        format!("{}|", adding.guest_port_input)
    } else {
        adding.guest_port_input.clone()
    };
    let guest_line = Line::from(vec![
        Span::styled("  Guest Port: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            guest_value,
            if guest_active {
                active_style
            } else {
                idle_style
            },
        ),
    ]);
    frame.render_widget(Paragraph::new(guest_line), chunks[4]);

    let help = Paragraph::new("[Enter] Next/Confirm  [Esc] Cancel")
        .style(hint_style)
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[6]);
}

fn handle_qemu_field_change(app: &mut App, delta: i32) {
    // Get dynamic display options based on the current emulator
    let emulator = app
        .wizard_state
        .as_ref()
        .map(|s| s.qemu_config.emulator.clone())
        .unwrap_or_else(|| "qemu-system-x86_64".to_string());
    let dynamic_display_options = app.get_display_options_for_emulator(&emulator);

    let Some(ref mut state) = app.wizard_state else {
        return;
    };
    let field = QemuField::from_index(state.field_focus);

    match field {
        QemuField::Memory => {
            let change = 256 * delta;
            state.qemu_config.memory_mb =
                (state.qemu_config.memory_mb as i32 + change).clamp(128, 1048576) as u32;
        }
        QemuField::CpuCores => {
            state.qemu_config.cpu_cores =
                (state.qemu_config.cpu_cores as i32 + delta).clamp(1, 256) as u32;
        }
        QemuField::Vga => {
            cycle_option(&mut state.qemu_config.vga, VGA_OPTIONS, delta);
        }
        QemuField::Audio => {
            cycle_audio(&mut state.qemu_config.audio, delta);
        }
        QemuField::DiskInterface => {
            cycle_option(
                &mut state.qemu_config.disk_interface,
                DISK_INTERFACE_OPTIONS,
                delta,
            );
        }
        QemuField::Display => {
            // Use dynamic options from detected capabilities
            let display_strs: Vec<&str> =
                dynamic_display_options.iter().map(|s| s.as_str()).collect();
            if !display_strs.is_empty() {
                cycle_option(&mut state.qemu_config.display, &display_strs, delta);
            } else {
                cycle_option(&mut state.qemu_config.display, DISPLAY_OPTIONS, delta);
            }
        }
        // NetworkAdapters is opened via Enter, not cycled.
        // Toggles use space, not left/right.
        _ => {}
    }
}

fn cycle_option(current: &mut String, options: &[&str], delta: i32) {
    let current_idx = options
        .iter()
        .position(|&o| o == current.as_str())
        .unwrap_or(0);
    let new_idx = (current_idx as i32 + delta).rem_euclid(options.len() as i32) as usize;
    *current = options[new_idx].to_string();
}

fn cycle_audio(current: &mut Vec<String>, delta: i32) {
    // Find current audio preset
    let current_idx = AUDIO_OPTIONS
        .iter()
        .position(|(_, devices)| {
            if devices.is_empty() && current.is_empty() {
                true
            } else if !devices.is_empty() && !current.is_empty() {
                current
                    .iter()
                    .any(|c| devices.iter().any(|d| c.contains(d)))
            } else {
                false
            }
        })
        .unwrap_or(0);

    let new_idx = (current_idx as i32 + delta).rem_euclid(AUDIO_OPTIONS.len() as i32) as usize;
    let (_, devices) = AUDIO_OPTIONS[new_idx];
    *current = devices.iter().map(|&s| s.to_string()).collect();
}

// =============================================================================
// Step 5: Confirm
// =============================================================================

fn render_step_confirm(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(
            " Create New VM ({}/5) - {} ",
            state.step.number(),
            state.step.title()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Min(15),   // Summary
            Constraint::Length(3), // Auto-launch toggle
            Constraint::Length(1), // Error
            Constraint::Length(2), // Help
        ])
        .split(inner);

    // Header
    let header = Paragraph::new("Summary").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    // Summary
    let os_name = state
        .selected_os
        .as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .map(|p| p.display_name.as_str())
        .unwrap_or("Custom OS");

    let vm_path = app
        .wizard_vm_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let iso_str = state
        .iso_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "None".to_string());
    let disk_line = match state.disk_source {
        WizardDiskSource::ExistingImage => {
            let action = match state.existing_disk_action {
                DiskAction::Copy => "copy",
                DiskAction::Move => "move",
            };
            let format = state
                .existing_disk_path
                .as_deref()
                .and_then(DiskImageFormat::from_path)
                .map(|format| format.label())
                .unwrap_or("detected");
            Line::from(vec![
                Span::styled("Disk:           ", Style::default().fg(Color::Yellow)),
                Span::raw(format!("existing disk ({action}, {format})")),
            ])
        }
        WizardDiskSource::NewImage => Line::from(vec![
            Span::styled("Disk:           ", Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "{} GB {}",
                state.disk_size_gb,
                app.create_wizard_disk_format.summary()
            )),
        ]),
        WizardDiskSource::PhysicalDevice => {
            let desc = state
                .physical_disk
                .as_ref()
                .map(|d| format!("{} ({})", d.model, d.size_display()))
                .unwrap_or_else(|| "physical disk".to_string());
            Line::from(vec![
                Span::styled("Disk:           ", Style::default().fg(Color::Yellow)),
                Span::raw(format!("{desc} ")),
                Span::styled(
                    "[DESTRUCTIVE — raw device]",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ])
        }
    };

    let config = &state.qemu_config;

    let mut lines = vec![
        Line::from(vec![
            Span::styled("VM Name:        ", Style::default().fg(Color::Yellow)),
            Span::raw(&state.vm_name),
        ]),
        Line::from(vec![
            Span::styled("Folder:         ", Style::default().fg(Color::Yellow)),
            Span::raw(vm_path),
        ]),
        Line::from(vec![
            Span::styled("OS Type:        ", Style::default().fg(Color::Yellow)),
            Span::raw(os_name),
        ]),
        Line::from(""),
        disk_line,
        Line::from(vec![
            Span::styled("ISO:            ", Style::default().fg(Color::Yellow)),
            Span::raw(iso_str),
        ]),
    ];
    if let Some(ref floppy_path) = state.floppy_path {
        lines.push(Line::from(vec![
            Span::styled("Floppy:         ", Style::default().fg(Color::Yellow)),
            Span::raw(floppy_path.display().to_string()),
        ]));
    }
    if let Some(ref rom_path) = state.bios_rom_path {
        lines.push(Line::from(vec![
            Span::styled("BIOS/ROM:       ", Style::default().fg(Color::Yellow)),
            Span::raw(rom_path.display().to_string()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Hardware:       ", Style::default().fg(Color::Yellow)),
        Span::raw(format!(
            "{} cores, {} MB RAM",
            config.cpu_cores, config.memory_mb
        )),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Graphics:       ", Style::default().fg(Color::Yellow)),
        Span::raw(&config.vga),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Audio:          ", Style::default().fg(Color::Yellow)),
        Span::raw(
            config
                .audio
                .first()
                .cloned()
                .unwrap_or_else(|| "None".to_string()),
        ),
    ]));
    let multi_nic = config.network_adapters.len() > 1;
    for (idx, adapter) in config.network_adapters.iter().enumerate() {
        let net_display = if adapter.model == "none" {
            "none".to_string()
        } else {
            let backend_str = match adapter.backend.as_str() {
                "passt" => "passt".to_string(),
                "bridge" => format!(
                    "bridge ({})",
                    adapter.bridge_name.as_deref().unwrap_or("qemubr0")
                ),
                "none" => "disabled".to_string(),
                _ => "user/SLIRP (NAT)".to_string(),
            };
            format!("{} ({})", adapter.model, backend_str)
        };
        let label = if multi_nic {
            format!("Network {}:      ", idx + 1)
        } else {
            "Network:        ".to_string()
        };
        lines.push(Line::from(vec![
            Span::styled(label, Style::default().fg(Color::Yellow)),
            Span::raw(net_display),
        ]));
        for pf in &adapter.port_forwards {
            lines.push(Line::from(format!(
                "                {} {} -> {}",
                pf.protocol, pf.host_port, pf.guest_port
            )));
        }
    }

    let accel = if config.enable_kvm {
        "KVM enabled"
    } else {
        "No acceleration"
    };
    lines.push(Line::from(vec![
        Span::styled("Acceleration:   ", Style::default().fg(Color::Yellow)),
        Span::raw(accel),
    ]));

    let summary = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(summary, chunks[2]);

    // Auto-launch toggle
    let launch_box = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));
    let checkbox = if state.auto_launch { "[x]" } else { "[ ]" };
    let launch_text = Paragraph::new(format!(
        "{} Launch VM in install mode after creation",
        checkbox
    ))
    .style(Style::default().fg(Color::White))
    .block(launch_box);
    frame.render_widget(launch_text, chunks[3]);

    // Error
    if let Some(ref error) = state.error_message {
        let error_text = Paragraph::new(error.as_str()).style(Style::default().fg(Color::Red));
        frame.render_widget(error_text, chunks[4]);
    }

    // Help
    let help = Paragraph::new("[Enter] Create VM  [Space] Toggle launch  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[5]);
}

fn handle_step_confirm(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.wizard_prev_step();
        }
        KeyCode::Char(' ') => {
            if let Some(ref mut state) = app.wizard_state {
                state.auto_launch = !state.auto_launch;
            }
        }
        KeyCode::Enter => {
            // Create the VM
            let (library_path, auto_launch) = {
                let state = app.wizard_state.as_ref().unwrap();
                let path = app.config.vm_library_path.clone();
                let launch = state.auto_launch;
                (path, launch)
            };

            // Clone the state for creation
            let state = app.wizard_state.as_ref().unwrap().clone();
            let vm_name = state.vm_name.clone();

            match create_vm_with_disk_format(&library_path, &state, app.create_wizard_disk_format) {
                Ok(created) => {
                    // Cancel wizard first (closes screens)
                    app.cancel_wizard();

                    // Refresh VM list to include the new VM
                    match app.refresh_vms() {
                        Ok(()) => {
                            app.set_status(format!("VM created: {}", vm_name));
                        }
                        Err(e) => {
                            app.set_status(format!("VM created but refresh failed: {}", e));
                        }
                    }

                    // If auto_launch is enabled, find and launch the new VM
                    if auto_launch {
                        // Find the newly created VM and select it
                        if let Some(idx) = app
                            .vms
                            .iter()
                            .position(|vm| vm.launch_script == created.launch_script)
                        {
                            // Find in visual order
                            if let Some(visual_idx) =
                                app.visual_order.iter().position(|&filtered_idx| {
                                    app.filtered_indices.get(filtered_idx) == Some(&idx)
                                })
                            {
                                app.selected_vm = visual_idx;

                                // Set boot mode to install
                                app.boot_mode = crate::vm::BootMode::Install;

                                // Launch the VM
                                match launch_created_vm(app) {
                                    Ok(()) => {
                                        app.set_status(format!("Launched: {}", vm_name));
                                    }
                                    Err(e) => {
                                        app.set_status(format!(
                                            "VM created but launch failed: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Some(ref mut state) = app.wizard_state {
                        state.error_message = Some(format!("Failed to create VM: {}", e));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Launch a newly created VM
fn launch_created_vm(app: &mut App) -> Result<()> {
    if let Some(vm) = app.selected_vm() {
        let options = app.get_launch_options();
        crate::vm::launch_vm_sync(vm, &options)?;
    }
    Ok(())
}

/// Open a URL in the default browser
fn open_url_in_browser(url: &str) -> Result<()> {
    use std::process::Command;

    // Try xdg-open first (standard on Linux)
    let result = Command::new("xdg-open").arg(url).spawn();

    match result {
        Ok(_) => Ok(()),
        Err(_) => {
            // Fallback to other openers
            for opener in &["firefox", "chromium", "google-chrome", "open"] {
                if Command::new(opener).arg(url).spawn().is_ok() {
                    return Ok(());
                }
            }
            anyhow::bail!("No browser found. Please visit: {}", url)
        }
    }
}

// =============================================================================
// Utility
// =============================================================================

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
#[path = "tests/create_wizard.rs"]
mod tests;
