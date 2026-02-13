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

use crate::app::{App, WizardStep, WizardField, WizardQemuConfig};
use crate::metadata::QemuProfileStore;
use crate::vm::create_vm;

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
        (&input[..input.len()-2], "GB")
    } else if input.ends_with("MB") {
        (&input[..input.len()-2], "MB")
    } else if input.ends_with("KB") {
        (&input[..input.len()-2], "KB")
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
            Constraint::Length(1),   // Intro text
            Constraint::Length(1),   // Spacer
            Constraint::Length(3),   // OS Name
            Constraint::Length(3),   // Publisher
            Constraint::Length(3),   // Architecture
            Constraint::Length(1),   // Spacer
            Constraint::Length(5),   // Base profile selection
            Constraint::Length(1),   // Spacer
            Constraint::Min(3),      // Tips
            Constraint::Length(2),   // Help
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
        frame, chunks[2],
        "OS Name",
        if os_name.is_empty() { "e.g., My Custom Linux" } else { os_name },
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
        frame, chunks[3],
        "Publisher",
        if publisher.is_empty() { "e.g., Open Source Community" } else { publisher },
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
    let arch = custom_os.map(|c| c.architecture.as_str()).unwrap_or("x86_64");
    let arch_focus = state.field_focus == 2;
    render_select_field(
        frame, chunks[4],
        "Architecture",
        arch,
        arch_focus,
        "[←/→] to change",
    );

    // Base profile selection
    let base_profile = custom_os.map(|c| c.base_profile.as_str()).unwrap_or("generic-other");
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
        Span::styled(base_display, if base_focus { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
    ]));
    base_lines.push(Line::from(Span::styled(
        if base_focus { "[←/→] Change profile" } else { "" },
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
         Consider contributing new OS profiles to the project!"
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
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
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

    let mut spans = vec![
        Span::styled(value, if is_focused {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        }),
    ];

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

    let progress = app.wizard_state.as_ref()
        .map(|s| s.iso_download_progress)
        .unwrap_or(0.0);

    let text = Paragraph::new(format!("Downloading... {:.0}%\n\n[Esc] Cancel", progress * 100.0))
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
    let editing = app.wizard_state.as_ref()
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
                            Some(WizardField::CustomOsName) => { custom.name.pop(); }
                            Some(WizardField::CustomOsPublisher) => { custom.publisher.pop(); }
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
                    state.field_focus = if state.field_focus == 0 { 3 } else { state.field_focus - 1 };
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let delta = if key.code == KeyCode::Right { 1i32 } else { -1i32 };
                if let Some(ref mut state) = app.wizard_state {
                    if let Some(ref mut custom) = state.custom_os {
                        match state.field_focus {
                            2 => {
                                // Architecture
                                let current_idx = ARCH_OPTIONS.iter()
                                    .position(|&a| a == custom.architecture)
                                    .unwrap_or(0);
                                let new_idx = (current_idx as i32 + delta)
                                    .rem_euclid(ARCH_OPTIONS.len() as i32) as usize;
                                custom.architecture = ARCH_OPTIONS[new_idx].to_string();
                            }
                            3 => {
                                // Base profile
                                let current_idx = BASE_PROFILE_OPTIONS.iter()
                                    .position(|&p| p == custom.base_profile)
                                    .unwrap_or(0);
                                let new_idx = (current_idx as i32 + delta)
                                    .rem_euclid(BASE_PROFILE_OPTIONS.len() as i32) as usize;
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
                let valid = app.wizard_state.as_ref()
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
                        let id = custom_name.to_lowercase()
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
                } else {
                    if let Some(ref mut state) = app.wizard_state {
                        state.error_message = Some("Please enter an OS name".to_string());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Handle key input for download screen
pub fn handle_download_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            // Cancel download
            if let Some(ref mut state) = app.wizard_state {
                state.iso_downloading = false;
                state.iso_download_progress = 0.0;
            }
            app.pop_screen();
        }
        _ => {}
    }
    Ok(())
}

// =============================================================================
// Step 1: Select OS
// =============================================================================

fn render_step_select_os(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(" Create New VM ({}/5) - {} ", state.step.number(), state.step.title()))
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
            Constraint::Length(1),   // OS list header
            Constraint::Min(10),     // OS list
            Constraint::Length(1),   // Spacer
            Constraint::Length(3),   // VM Name field
            Constraint::Length(1),   // Error message
            Constraint::Length(2),   // Help text
        ])
        .split(inner);

    // OS list header
    let header = Paragraph::new("Select Operating System:")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
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
        let error_text = Paragraph::new(error.as_str())
            .style(Style::default().fg(Color::Red));
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
    let category_order = ["windows", "linux", "bsd", "unix", "macos", "mobile", "infrastructure", "utilities", "alternative", "retro", "classic-mac"];

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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
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
                    Span::styled(format!("{}", profile.display_name), os_style),
                    Span::styled(format!("  ({})", summary), Style::default().fg(Color::DarkGray)),
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
    let editing_name = app.wizard_state.as_ref()
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
    let category_order = ["windows", "linux", "bsd", "unix", "macos", "mobile", "infrastructure", "utilities", "alternative", "retro", "classic-mac"];

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

    let category_order = ["windows", "linux", "bsd", "unix", "macos", "mobile", "infrastructure", "utilities", "alternative", "retro", "classic-mac"];

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
        .title(format!(" Create New VM ({}/5) - {} ", state.step.number(), state.step.title()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),   // OS info
            Constraint::Length(1),   // Spacer
            Constraint::Length(1),   // Header
            Constraint::Min(10),     // Options
            Constraint::Length(1),   // Selected path
            Constraint::Length(2),   // Help
        ])
        .split(inner);

    // OS info
    let os_name = state.selected_os.as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .map(|p| p.display_name.as_str())
        .unwrap_or("Custom OS");

    let os_info = Paragraph::new(format!("Operating System: {}", os_name))
        .style(Style::default().fg(Color::White));
    frame.render_widget(os_info, chunks[0]);

    // Header
    let header = Paragraph::new("Installation ISO:")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[2]);

    // Options
    let mut lines = Vec::new();

    // Check if this OS has a free ISO URL
    let has_download = state.selected_os.as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .and_then(|p| p.iso_url.as_ref())
        .is_some();

    let mut option_idx = 0;

    if has_download {
        let is_selected = state.field_focus == option_idx;
        let style = if is_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_selected { "> " } else { "  " };
        lines.push(Line::styled(format!("{}( ) Open download page in browser", prefix), style));
        option_idx += 1;
    }

    let is_browse_selected = state.field_focus == option_idx;
    let browse_style = if is_browse_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let browse_prefix = if is_browse_selected { "> " } else { "  " };
    lines.push(Line::styled(format!("{}( ) Browse for local ISO file...", browse_prefix), browse_style));
    option_idx += 1;

    let is_none_selected = state.field_focus == option_idx;
    let none_style = if is_none_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let none_prefix = if is_none_selected { "> " } else { "  " };
    lines.push(Line::styled(format!("{}( ) No ISO (configure later)", none_prefix), none_style));

    let options = Paragraph::new(lines);
    frame.render_widget(options, chunks[3]);

    // Selected path
    if let Some(ref path) = state.iso_path {
        let path_text = Paragraph::new(format!("Selected: {}", path.display()))
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
    let has_download = app.wizard_state.as_ref()
        .and_then(|s| s.selected_os.as_ref())
        .and_then(|id| app.qemu_profiles.get(id))
        .and_then(|p| p.iso_url.as_ref())
        .is_some();

    let max_options = if has_download { 3 } else { 2 };

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
            let focus = app.wizard_state.as_ref().map(|s| s.field_focus).unwrap_or(0);
            let option_offset = if has_download { 0 } else { 1 };

            match focus + option_offset {
                0 => {
                    // Open download page in browser
                    if let Some(url) = app.wizard_state.as_ref()
                        .and_then(|s| s.selected_os.as_ref())
                        .and_then(|id| app.qemu_profiles.get(id))
                        .and_then(|p| p.iso_url.as_ref())
                    {
                        // Try to open in browser
                        let url = url.clone();
                        if let Err(e) = open_url_in_browser(&url) {
                            app.set_status(format!("Failed to open browser: {}", e));
                        } else {
                            app.set_status("Opened download page in browser. Use 'Browse for ISO' after downloading.");
                        }
                    }
                }
                1 => {
                    // Browse for ISO - open file browser
                    app.load_file_browser(crate::app::FileBrowserMode::Iso);
                    app.push_screen(crate::app::Screen::FileBrowser);
                }
                2 => {
                    // No ISO
                    if let Some(ref mut state) = app.wizard_state {
                        state.iso_path = None;
                    }
                    let _ = app.wizard_next_step();
                }
                _ => {}
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
        .title(format!(" Create New VM ({}/5) - {} ", state.step.number(), state.step.title()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),   // Header
            Constraint::Length(1),   // Spacer
            Constraint::Length(1),   // Disk source toggle
            Constraint::Length(1),   // Spacer
            Constraint::Min(10),     // Mode-specific content
            Constraint::Length(2),   // Help
        ])
        .split(inner);

    // Header
    let header = Paragraph::new("Disk Configuration")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Disk source toggle (field_focus == 0)
    let source_focused = state.field_focus == 0;
    let create_style = if !state.use_existing_disk {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let existing_style = if state.use_existing_disk {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let prefix = if source_focused { "> " } else { "  " };

    let source_line = Line::from(vec![
        Span::styled(prefix, if source_focused { Style::default().fg(Color::Yellow) } else { Style::default() }),
        Span::styled("Disk Source: ", Style::default().fg(Color::Yellow)),
        Span::styled("[ ", Style::default()),
        Span::styled("Create New", create_style),
        Span::styled(" ] [ ", Style::default()),
        Span::styled("Use Existing", existing_style),
        Span::styled(" ]", Style::default()),
        if source_focused {
            Span::styled("  [←/→] toggle", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        },
    ]);
    let source_toggle = Paragraph::new(source_line);
    frame.render_widget(source_toggle, chunks[2]);

    // Mode-specific content area
    let content_area = chunks[4];

    if state.use_existing_disk {
        // "Use Existing" mode
        render_existing_disk_mode(app, frame, content_area);
    } else {
        // "Create New" mode (existing behavior)
        render_new_disk_mode(app, frame, content_area);
    }

    // Help
    let help_text = if state.use_existing_disk {
        if state.field_focus == 0 {
            "[←/→] Toggle mode  [j/k] Navigate  [Enter] Next  [Esc] Back"
        } else if state.field_focus == 1 {
            "[Enter] Browse  [j/k] Navigate  [Esc] Back"
        } else {
            "[←/→] Toggle action  [j/k] Navigate  [Enter] Next  [Esc] Back"
        }
    } else {
        let editing = matches!(state.editing_field, Some(WizardField::DiskSize));
        if editing {
            "[Enter] Done  [Backspace] Delete  [0-9] Enter size"
        } else if state.field_focus == 0 {
            "[←/→] Toggle mode  [j/k] Navigate  [Enter] Next  [Esc] Back"
        } else {
            "[Tab] Edit size  [←/→] Adjust  [Enter] Next  [Esc] Back"
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
            Constraint::Length(3),   // Disk size input
            Constraint::Length(1),   // Spacer
            Constraint::Min(5),      // Disk info
        ])
        .split(area);

    // Disk size input (field_focus == 1 when in new disk mode)
    let size_focused = state.field_focus == 1;
    let editing = matches!(state.editing_field, Some(WizardField::DiskSize));
    let size_style = if editing {
        Style::default().fg(Color::Yellow)
    } else if size_focused {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let border_style = if editing || size_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let recommended = app.wizard_selected_profile()
        .map(|p| p.disk_size_gb)
        .unwrap_or(32);

    let size_block = Block::default()
        .title(format!(" Disk Size (Recommended: {} GB) ", recommended))
        .borders(Borders::ALL)
        .border_style(border_style);

    // Show edit buffer when editing, otherwise show current value
    let size_display = if editing {
        format!("{}|  (e.g., 500, 500GB, 512000MB)", state.wizard_edit_buffer)
    } else {
        format!("{} GB", state.disk_size_gb)
    };

    let size_text = Paragraph::new(size_display)
        .style(size_style)
        .block(size_block);
    frame.render_widget(size_text, sub_chunks[0]);

    // Disk info box
    let info_block = Block::default()
        .title(" Disk Info ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));

    let disk_path = app.wizard_vm_path()
        .map(|p| p.join(format!("{}.qcow2", state.folder_name)))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/vm-space/<vm-name>/<vm-name>.qcow2".to_string());

    let info_text = vec![
        Line::from(vec![
            Span::styled("Format: ", Style::default().fg(Color::Yellow)),
            Span::raw("qcow2 (copy-on-write, snapshots supported)"),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Yellow)),
            Span::raw("Expandable (only uses space as needed)"),
        ]),
        Line::from(vec![
            Span::styled("Location: ", Style::default().fg(Color::Yellow)),
            Span::raw(disk_path),
        ]),
    ];

    let info = Paragraph::new(info_text)
        .block(info_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(info, sub_chunks[2]);
}

/// Render the "Use Existing" disk mode content
fn render_existing_disk_mode(app: &App, frame: &mut Frame, area: Rect) {
    use crate::app::DiskAction;

    let state = app.wizard_state.as_ref().unwrap();

    let sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Browse / selected path
            Constraint::Length(1),   // Spacer
            Constraint::Length(1),   // Action toggle
            Constraint::Length(1),   // Spacer
            Constraint::Min(3),      // Note
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
        Paragraph::new(format!("{}( ) Browse for qcow2 disk file...", prefix))
            .style(if browse_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            })
    };
    frame.render_widget(browse_text, browse_inner);

    // Action toggle (field_focus == 2)
    let action_focused = state.field_focus == 2;
    let copy_style = if matches!(state.existing_disk_action, DiskAction::Copy) {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let move_style = if matches!(state.existing_disk_action, DiskAction::Move) {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let prefix = if action_focused { "> " } else { "  " };

    let action_line = Line::from(vec![
        Span::styled(prefix, if action_focused { Style::default().fg(Color::Yellow) } else { Style::default() }),
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
        "Note: The disk will be renamed to {}.qcow2",
        state.folder_name
    );
    let note = Paragraph::new(note_text)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(note, sub_chunks[4]);
}

fn handle_step_configure_disk(app: &mut App, key: KeyEvent) -> Result<()> {
    use crate::app::{DiskAction, FileBrowserMode};

    let (editing, use_existing, field_focus) = app.wizard_state.as_ref()
        .map(|s| (
            matches!(s.editing_field, Some(WizardField::DiskSize)),
            s.use_existing_disk,
            s.field_focus,
        ))
        .unwrap_or((false, false, 0));

    // Handle disk size editing mode (only in "Create New" mode)
    if editing && !use_existing {
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
                        let clamped = value.max(1).min(10000);
                        state.disk_size_gb = clamped;
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
                let max_focus = if state.use_existing_disk { 2 } else { 1 };
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
                match state.field_focus {
                    0 => {
                        // Toggle disk source mode
                        state.use_existing_disk = !state.use_existing_disk;
                    }
                    1 if !state.use_existing_disk => {
                        // Adjust disk size (Create New mode)
                        if key.code == KeyCode::Left {
                            state.disk_size_gb = state.disk_size_gb.saturating_sub(8).max(1);
                        } else {
                            state.disk_size_gb = (state.disk_size_gb + 8).min(10000);
                        }
                    }
                    2 if state.use_existing_disk => {
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
            if !use_existing && field_focus == 1 {
                if let Some(ref mut state) = app.wizard_state {
                    state.editing_field = Some(WizardField::DiskSize);
                    state.wizard_edit_buffer = state.disk_size_gb.to_string();
                }
            }
        }
        KeyCode::Enter => {
            // If on browse button in existing mode, open file browser
            if use_existing && field_focus == 1 {
                app.load_file_browser(FileBrowserMode::Disk);
                app.push_screen(crate::app::Screen::FileBrowser);
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

/// Fields in the QEMU config screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QemuField {
    Memory,
    CpuCores,
    Vga,
    Audio,
    Network,
    NetBackend,
    BridgeName,
    PortForwards,
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
            4 => Self::Network,
            5 => Self::NetBackend,
            6 => Self::BridgeName,
            7 => Self::PortForwards,
            8 => Self::DiskInterface,
            9 => Self::Display,
            10 => Self::Kvm,
            11 => Self::GlAccel,
            12 => Self::Uefi,
            13 => Self::Tpm,
            14 => Self::UsbTablet,
            _ => Self::RtcLocal,
        }
    }

    fn count() -> usize {
        16
    }
}

fn render_step_configure_qemu(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.wizard_state.as_ref().unwrap();

    let block = Block::default()
        .title(format!(" Create New VM ({}/5) - {} ", state.step.number(), state.step.title()))
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
            Constraint::Length(1),   // Header
            Constraint::Min(18),     // Settings
            Constraint::Length(2),   // Help
        ])
        .split(h_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),   // Header
            Constraint::Min(18),     // Notes
        ])
        .split(h_chunks[1]);

    // Left side: Settings header
    let header = Paragraph::new("QEMU Settings")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
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

    // Network adapter (cycle)
    let net_selected = focus == 4;
    lines.push(render_field_line(
        "Network:",
        &config.network_model,
        net_selected,
        false,
        "[←/→] cycle",
    ));

    // Network backend (cycle) - hidden if network model is "none"
    if config.network_model != "none" {
        let backend_selected = focus == 5;
        let backend_display = match config.network_backend.as_str() {
            "user" => "user/SLIRP (NAT)".to_string(),
            "passt" => "passt".to_string(),
            "bridge" => format!("bridge ({})", config.bridge_name.as_deref().unwrap_or("qemubr0")),
            "none" => "none".to_string(),
            other => other.to_string(),
        };
        lines.push(render_field_line(
            "Net Backend:",
            &backend_display,
            backend_selected,
            false,
            "[←/→] cycle",
        ));

        // Bridge name (only for bridge backend)
        if config.network_backend == "bridge" {
            let bridge_selected = focus == 6;
            let bridge_display = config.bridge_name.as_deref().unwrap_or("qemubr0");
            lines.push(render_field_line(
                "Bridge:",
                bridge_display,
                bridge_selected,
                false,
                "[←/→] cycle",
            ));
        }

        // Port forwards (only for user/passt)
        if config.network_backend == "user" || config.network_backend == "passt" {
            let pf_selected = focus == 7;
            let pf_display = if config.port_forwards.is_empty() {
                "none".to_string()
            } else {
                format!("{} rule(s)", config.port_forwards.len())
            };
            lines.push(render_field_line(
                "Forwards:",
                &pf_display,
                pf_selected,
                false,
                "[Enter] edit",
            ));
        }
    }

    // Disk Interface (cycle)
    let disk_selected = focus == 8;
    lines.push(render_field_line(
        "Disk I/F:",
        &config.disk_interface,
        disk_selected,
        false,
        "[←/→] cycle",
    ));

    // Display (cycle)
    let disp_selected = focus == 9;
    lines.push(render_field_line(
        "Display:",
        &config.display,
        disp_selected,
        false,
        "[←/→] cycle",
    ));

    lines.push(Line::from(""));
    lines.push(Line::styled("  Features (toggle with Space):", Style::default().fg(Color::DarkGray)));

    // KVM toggle
    let kvm_selected = focus == 10;
    lines.push(render_toggle_line("KVM Accel:", config.enable_kvm, kvm_selected));

    // 3D/GL acceleration toggle
    let gl_selected = focus == 11;
    lines.push(render_toggle_line("3D Accel:", config.gl_acceleration, gl_selected));

    // UEFI toggle
    let uefi_selected = focus == 12;
    lines.push(render_toggle_line("UEFI Boot:", config.uefi, uefi_selected));

    // TPM toggle
    let tpm_selected = focus == 13;
    lines.push(render_toggle_line("TPM 2.0:", config.tpm, tpm_selected));

    // USB Tablet toggle
    let usb_selected = focus == 14;
    lines.push(render_toggle_line("USB Tablet:", config.usb_tablet, usb_selected));

    // RTC Local toggle
    let rtc_selected = focus == 15;
    lines.push(render_toggle_line("RTC Local:", config.rtc_localtime, rtc_selected));

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
    let notes_header = Paragraph::new("Why These Defaults?")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
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

fn render_field_line(label: &str, value: &str, selected: bool, editing: bool, hint: &str) -> Line<'static> {
    let prefix = if selected { "> " } else { "  " };
    let label_style = Style::default().fg(Color::Yellow);
    let value_style = if editing {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let hint_style = Style::default().fg(Color::DarkGray);

    Line::from(vec![
        Span::styled(prefix.to_string(), if selected { Style::default().fg(Color::Yellow) } else { Style::default() }),
        Span::styled(format!("{:12}", label), label_style),
        Span::styled(format!("{:15}", value), value_style),
        Span::styled(if selected { hint.to_string() } else { String::new() }, hint_style),
    ])
}

fn render_toggle_line(label: &str, enabled: bool, selected: bool) -> Line<'static> {
    let prefix = if selected { "> " } else { "  " };
    let checkbox = if enabled { "[x]" } else { "[ ]" };
    let label_style = Style::default().fg(Color::Yellow);
    let value_style = if selected {
        Style::default().fg(if enabled { Color::Green } else { Color::Red }).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(if enabled { Color::Green } else { Color::DarkGray })
    };

    Line::from(vec![
        Span::styled(prefix.to_string(), if selected { Style::default().fg(Color::Yellow) } else { Style::default() }),
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
    let profile_notes = profile.and_then(|p| p.notes.as_ref()).cloned().unwrap_or_default();
    let os_name = profile.map(|p| p.display_name.as_str()).unwrap_or("this OS");

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
        QemuField::Network => format!(
            "Network adapter for {}.\n\n\
            virtio: Best perf (needs driver)\n\
            e1000: Wide compat (Intel)\n\
            rtl8139: Win XP built-in\n\
            ne2k_pci: DOS/old Linux\n\
            pcnet: BSD compatible",
            os_name
        ),
        QemuField::NetBackend => format!(
            "Network backend for {}.\n\n\
            user: NAT via SLIRP (default)\n  Works everywhere, no setup needed\n\n\
            passt: Fast NAT, ping works\n  Requires passt package\n\n\
            bridge: Full network access\n  VM gets own IP on LAN\n  One-time setup needed\n\n\
            none: No networking",
            os_name
        ),
        QemuField::BridgeName => {
            let bridges = &app.network_caps.system_bridges;
            let bridges_str = if bridges.is_empty() {
                "No bridges detected on system.".to_string()
            } else {
                format!("Available: {}", bridges.join(", "))
            };
            format!(
                "Network bridge for {}.\n\n\
                {}\n\n\
                The VM will get its own IP on the bridge network, \
                providing full LAN access.\n\n\
                Requires qemu-bridge-helper with proper permissions.",
                os_name, bridges_str
            )
        },
        QemuField::PortForwards => format!(
            "Port forwarding for {}.\n\n\
            Forward host ports to the VM for \
            services like SSH, HTTP, RDP.\n\n\
            Only available with user (NAT) and \
            passt backends.\n\n\
            Press Enter to edit forwarding rules.",
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
            Disable for: Non-x86 guests, nested virt issues.".to_string(),
        QemuField::GlAccel => "3D/OpenGL acceleration.\n\n\
            Hardware-accelerated 3D graphics via virtio-gpu.\n\n\
            Requires: virtio VGA (auto-set when enabled)\n\
            Best for: Linux guests, Android x86\n\
            Not for: Windows (no virtio 3D), retro OSes".to_string(),
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
            Not needed: Most other OSes".to_string(),
        QemuField::UsbTablet => "USB tablet device.\n\n\
            Provides seamless mouse integration (no capture).\n\n\
            Recommended: Most modern systems\n\
            Disable: Old OSes with USB issues".to_string(),
        QemuField::RtcLocal => "RTC in local time.\n\n\
            Sets hardware clock to local timezone.\n\n\
            Enable: Windows (expects local time)\n\
            Disable: Linux/Unix (expects UTC)".to_string(),
    };

    if profile_notes.is_empty() {
        explanation
    } else {
        format!("{}\n\n---\nProfile note:\n{}", explanation, profile_notes)
    }
}

fn handle_step_configure_qemu(app: &mut App, key: KeyEvent) -> Result<()> {
    let field_count = QemuField::count();

    // Handle wizard port forward editing
    if app.wizard_editing_port_forwards {
        return handle_wizard_port_forward_editor(app, key);
    }

    // Check if we're in edit mode for Memory or CPU
    let editing_memory = app.wizard_state.as_ref()
        .map(|s| matches!(s.editing_field, Some(WizardField::MemoryMb)))
        .unwrap_or(false);
    let editing_cpu = app.wizard_state.as_ref()
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
                            let clamped = value.max(128).min(1048576);
                            state.qemu_config.memory_mb = clamped;
                        }
                    } else if editing_cpu {
                        // Parse as plain number
                        if let Ok(value) = buffer.trim().parse::<u32>() {
                            let clamped = value.max(1).min(256);
                            state.qemu_config.cpu_cores = clamped;
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
                let delta = if key.code == KeyCode::Right { 1i32 } else { -1i32 };
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
            // Check if on PortForwards field
            let on_pf = app.wizard_state.as_ref()
                .map(|s| QemuField::from_index(s.field_focus) == QemuField::PortForwards)
                .unwrap_or(false);
            if on_pf {
                app.wizard_editing_port_forwards = true;
                app.wizard_pf_selected = 0;
                app.wizard_adding_pf = None;
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
                if state.field_focus < field_count - 1 {
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
            let delta = if key.code == KeyCode::Right { 1i32 } else { -1i32 };
            handle_qemu_field_change(app, delta);
            // Show warning if spice-app selected without viewer
            if let Some(ref state) = app.wizard_state {
                if state.qemu_config.display.contains("spice")
                    && !crate::commands::qemu_system::is_spice_viewer_available()
                {
                    app.set_status("Warning: spice-app requires virt-viewer/remote-viewer to be installed");
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
                    QemuField::UsbTablet => state.qemu_config.usb_tablet = !state.qemu_config.usb_tablet,
                    QemuField::RtcLocal => state.qemu_config.rtc_localtime = !state.qemu_config.rtc_localtime,
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
            KeyCode::Enter => {
                match adding.step {
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
                                state.qemu_config.port_forwards.push(pf);
                            }
                            app.wizard_adding_pf = None;
                        }
                    }
                }
            }
            KeyCode::Left | KeyCode::Right => {
                if adding.step == AddPfStep::Protocol {
                    adding.protocol = match adding.protocol {
                        PortProtocol::Tcp => PortProtocol::Udp,
                        PortProtocol::Udp => PortProtocol::Tcp,
                    };
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                match adding.step {
                    AddPfStep::HostPort => adding.host_port_input.push(c),
                    AddPfStep::GuestPort => adding.guest_port_input.push(c),
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                match adding.step {
                    AddPfStep::HostPort => { adding.host_port_input.pop(); }
                    AddPfStep::GuestPort => { adding.guest_port_input.pop(); }
                    _ => {}
                }
            }
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
            let pf_len = app.wizard_state.as_ref()
                .map(|s| s.qemu_config.port_forwards.len())
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
                if !state.qemu_config.port_forwards.is_empty()
                    && app.wizard_pf_selected < state.qemu_config.port_forwards.len()
                {
                    state.qemu_config.port_forwards.remove(app.wizard_pf_selected);
                    if app.wizard_pf_selected >= state.qemu_config.port_forwards.len()
                        && app.wizard_pf_selected > 0
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

fn add_wizard_preset(app: &mut App, protocol: crate::vm::qemu_config::PortProtocol, host_port: u16, guest_port: u16) {
    if let Some(ref mut state) = app.wizard_state {
        if !state.qemu_config.port_forwards.iter().any(|pf| pf.host_port == host_port && pf.guest_port == guest_port) {
            state.qemu_config.port_forwards.push(crate::vm::qemu_config::PortForward {
                protocol,
                host_port,
                guest_port,
            });
        }
    }
}

fn handle_qemu_field_change(app: &mut App, delta: i32) {
    // Get dynamic display options based on the current emulator
    let emulator = app.wizard_state.as_ref()
        .map(|s| s.qemu_config.emulator.clone())
        .unwrap_or_else(|| "qemu-system-x86_64".to_string());
    let dynamic_display_options = app.get_display_options_for_emulator(&emulator);

    // Collect network backend options before mutable borrow
    let backend_options: Vec<String> = app.get_network_backend_options()
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    let system_bridges = app.network_caps.system_bridges.clone();
    let default_bridge = system_bridges.first().cloned()
        .or_else(|| Some("qemubr0".to_string()));

    let Some(ref mut state) = app.wizard_state else { return };
    let field = QemuField::from_index(state.field_focus);

    match field {
        QemuField::Memory => {
            let change = 256 * delta;
            let new_val = (state.qemu_config.memory_mb as i32 + change).max(128).min(1048576);
            state.qemu_config.memory_mb = new_val as u32;
        }
        QemuField::CpuCores => {
            let new_val = (state.qemu_config.cpu_cores as i32 + delta).max(1).min(256);
            state.qemu_config.cpu_cores = new_val as u32;
        }
        QemuField::Vga => {
            cycle_option(&mut state.qemu_config.vga, VGA_OPTIONS, delta);
        }
        QemuField::Audio => {
            cycle_audio(&mut state.qemu_config.audio, delta);
        }
        QemuField::Network => {
            cycle_option(&mut state.qemu_config.network_model, NETWORK_OPTIONS, delta);
        }
        QemuField::NetBackend => {
            let backend_strs: Vec<&str> = backend_options.iter().map(|s| s.as_str()).collect();
            cycle_option(&mut state.qemu_config.network_backend, &backend_strs, delta);

            // Set default bridge name when switching to bridge
            if state.qemu_config.network_backend == "bridge" && state.qemu_config.bridge_name.is_none() {
                state.qemu_config.bridge_name = default_bridge.clone();
            }
        }
        QemuField::BridgeName => {
            // Cycle through available system bridges
            if !system_bridges.is_empty() {
                let current_bridge = state.qemu_config.bridge_name.as_deref().unwrap_or("");
                let current_idx = system_bridges.iter()
                    .position(|b| b == current_bridge)
                    .unwrap_or(0);
                let new_idx = (current_idx as i32 + delta)
                    .rem_euclid(system_bridges.len() as i32) as usize;
                state.qemu_config.bridge_name = Some(system_bridges[new_idx].clone());
            }
        }
        QemuField::PortForwards => {
            // Handled via Enter key, not left/right
        }
        QemuField::DiskInterface => {
            cycle_option(&mut state.qemu_config.disk_interface, DISK_INTERFACE_OPTIONS, delta);
        }
        QemuField::Display => {
            // Use dynamic options from detected capabilities
            let display_strs: Vec<&str> = dynamic_display_options.iter().map(|s| s.as_str()).collect();
            if !display_strs.is_empty() {
                cycle_option(&mut state.qemu_config.display, &display_strs, delta);
            } else {
                cycle_option(&mut state.qemu_config.display, DISPLAY_OPTIONS, delta);
            }
        }
        // Toggles use space, not left/right
        _ => {}
    }
}

fn cycle_option(current: &mut String, options: &[&str], delta: i32) {
    let current_idx = options.iter().position(|&o| o == current.as_str()).unwrap_or(0);
    let new_idx = (current_idx as i32 + delta).rem_euclid(options.len() as i32) as usize;
    *current = options[new_idx].to_string();
}

fn cycle_audio(current: &mut Vec<String>, delta: i32) {
    // Find current audio preset
    let current_idx = AUDIO_OPTIONS.iter().position(|(_, devices)| {
        if devices.is_empty() && current.is_empty() {
            true
        } else if !devices.is_empty() && !current.is_empty() {
            current.iter().any(|c| devices.iter().any(|d| c.contains(d)))
        } else {
            false
        }
    }).unwrap_or(0);

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
        .title(format!(" Create New VM ({}/5) - {} ", state.step.number(), state.step.title()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),   // Header
            Constraint::Length(1),   // Spacer
            Constraint::Min(15),     // Summary
            Constraint::Length(3),   // Auto-launch toggle
            Constraint::Length(1),   // Error
            Constraint::Length(2),   // Help
        ])
        .split(inner);

    // Header
    let header = Paragraph::new("Summary")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Summary
    let os_name = state.selected_os.as_ref()
        .and_then(|id| app.qemu_profiles.get(id))
        .map(|p| p.display_name.as_str())
        .unwrap_or("Custom OS");

    let vm_path = app.wizard_vm_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let iso_str = state.iso_path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "None".to_string());

    let config = &state.qemu_config;

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("VM Name:        ", Style::default().fg(Color::Yellow)),
        Span::raw(&state.vm_name),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Folder:         ", Style::default().fg(Color::Yellow)),
        Span::raw(vm_path),
    ]));
    lines.push(Line::from(vec![
        Span::styled("OS Type:        ", Style::default().fg(Color::Yellow)),
        Span::raw(os_name),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Disk:           ", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{} GB qcow2 (expandable)", state.disk_size_gb)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("ISO:            ", Style::default().fg(Color::Yellow)),
        Span::raw(iso_str),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Hardware:       ", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{} cores, {} MB RAM", config.cpu_cores, config.memory_mb)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Graphics:       ", Style::default().fg(Color::Yellow)),
        Span::raw(&config.vga),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Audio:          ", Style::default().fg(Color::Yellow)),
        Span::raw(config.audio.first().cloned().unwrap_or_else(|| "None".to_string())),
    ]));
    let net_display = if config.network_model == "none" {
        "none".to_string()
    } else {
        let backend_str = match config.network_backend.as_str() {
            "passt" => "passt".to_string(),
            "bridge" => format!("bridge ({})", config.bridge_name.as_deref().unwrap_or("qemubr0")),
            "none" => "disabled".to_string(),
            _ => "user/SLIRP (NAT)".to_string(),
        };
        format!("{} ({})", config.network_model, backend_str)
    };
    lines.push(Line::from(vec![
        Span::styled("Network:        ", Style::default().fg(Color::Yellow)),
        Span::raw(net_display),
    ]));
    if !config.port_forwards.is_empty() {
        for pf in &config.port_forwards {
            lines.push(Line::from(format!("                {} {} -> {}", pf.protocol, pf.host_port, pf.guest_port)));
        }
    }

    let accel = if config.enable_kvm { "KVM enabled" } else { "No acceleration" };
    lines.push(Line::from(vec![
        Span::styled("Acceleration:   ", Style::default().fg(Color::Yellow)),
        Span::raw(accel),
    ]));

    let summary = Paragraph::new(lines)
        .wrap(Wrap { trim: false });
    frame.render_widget(summary, chunks[2]);

    // Auto-launch toggle
    let launch_box = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));
    let checkbox = if state.auto_launch { "[x]" } else { "[ ]" };
    let launch_text = Paragraph::new(format!("{} Launch VM in install mode after creation", checkbox))
        .style(Style::default().fg(Color::White))
        .block(launch_box);
    frame.render_widget(launch_text, chunks[3]);

    // Error
    if let Some(ref error) = state.error_message {
        let error_text = Paragraph::new(error.as_str())
            .style(Style::default().fg(Color::Red));
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

            match create_vm(&library_path, &state) {
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
                        if let Some(idx) = app.vms.iter().position(|vm| {
                            vm.launch_script == created.launch_script
                        }) {
                            // Find in visual order
                            if let Some(visual_idx) = app.visual_order.iter().position(|&filtered_idx| {
                                app.filtered_indices.get(filtered_idx) == Some(&idx)
                            }) {
                                app.selected_vm = visual_idx;

                                // Set boot mode to install
                                app.boot_mode = crate::vm::BootMode::Install;

                                // Launch the VM
                                match launch_created_vm(app) {
                                    Ok(()) => {
                                        app.set_status(format!("Launched: {}", vm_name));
                                    }
                                    Err(e) => {
                                        app.set_status(format!("VM created but launch failed: {}", e));
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
    let result = Command::new("xdg-open")
        .arg(url)
        .spawn();

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
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_with_suffix_memory() {
        // Plain number assumes target unit (MB)
        assert_eq!(parse_size_with_suffix("8192", "MB"), Some(8192));
        assert_eq!(parse_size_with_suffix("2048", "MB"), Some(2048));

        // GB to MB conversion
        assert_eq!(parse_size_with_suffix("8GB", "MB"), Some(8192));
        assert_eq!(parse_size_with_suffix("8gb", "MB"), Some(8192));  // case insensitive
        assert_eq!(parse_size_with_suffix("32GB", "MB"), Some(32768));
        assert_eq!(parse_size_with_suffix("96GB", "MB"), Some(98304));  // exceeds old 64GB limit
        assert_eq!(parse_size_with_suffix("1024GB", "MB"), Some(1048576));  // 1TB

        // MB to MB (no conversion)
        assert_eq!(parse_size_with_suffix("8192MB", "MB"), Some(8192));

        // KB to MB conversion
        assert_eq!(parse_size_with_suffix("8388608KB", "MB"), Some(8192));

        // Whitespace handling
        assert_eq!(parse_size_with_suffix("  8192  ", "MB"), Some(8192));
        assert_eq!(parse_size_with_suffix("8 GB", "MB"), Some(8192));
    }

    #[test]
    fn test_parse_size_with_suffix_disk() {
        // Plain number assumes target unit (GB)
        assert_eq!(parse_size_with_suffix("500", "GB"), Some(500));
        assert_eq!(parse_size_with_suffix("100", "GB"), Some(100));

        // GB to GB (no conversion)
        assert_eq!(parse_size_with_suffix("500GB", "GB"), Some(500));
        assert_eq!(parse_size_with_suffix("500gb", "GB"), Some(500));

        // MB to GB conversion
        assert_eq!(parse_size_with_suffix("512000MB", "GB"), Some(500));
        assert_eq!(parse_size_with_suffix("1024MB", "GB"), Some(1));
    }

    #[test]
    fn test_parse_size_with_suffix_invalid() {
        // Empty string
        assert_eq!(parse_size_with_suffix("", "MB"), None);

        // Non-numeric
        assert_eq!(parse_size_with_suffix("abc", "MB"), None);
        assert_eq!(parse_size_with_suffix("GB", "MB"), None);

        // Negative values
        assert_eq!(parse_size_with_suffix("-100", "MB"), None);
    }
}
