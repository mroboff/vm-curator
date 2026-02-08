//! Shared Folders Screen
//!
//! Manages virtio-9p shared folders between host and guest VM.
//! Shows configured folders, mount instructions based on OS tier,
//! and allows adding/removing shared directories.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::{App, FileBrowserMode, Screen};

/// Render the shared folders screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    let dialog_width = 70.min(area.width.saturating_sub(4));
    let dialog_height = 24.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let folder_count = app.shared_folders.len();
    let title = format!(" Shared Folders ({} configured) ", folder_count);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Add horizontal margins
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2), // Left margin
            Constraint::Min(1),   // Content
            Constraint::Length(2), // Right margin
        ])
        .split(inner);

    // Split into padding, folder list, separator, instructions, help
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top padding
            Constraint::Length(folder_list_height(app)), // Folder list
            Constraint::Length(1), // Separator
            Constraint::Min(4),   // Mount instructions
            Constraint::Length(2), // Help text
        ])
        .split(h_chunks[1]);

    let list_area = v_chunks[1];
    let separator_area = v_chunks[2];
    let instructions_area = v_chunks[3];
    let help_area = v_chunks[4];

    // Render folder list
    if app.shared_folders.is_empty() {
        let empty_msg = Paragraph::new("No shared folders configured.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty_msg, list_area);
    } else {
        let items: Vec<ListItem> = app
            .shared_folders
            .iter()
            .enumerate()
            .map(|(i, folder)| {
                let style = if i == app.shared_folder_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {}. ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(&folder.host_path, style),
                    Span::styled(
                        format!("  (tag: {})", folder.mount_tag),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(app.shared_folder_selected));

        let list = List::new(items).highlight_symbol("> ");
        frame.render_stateful_widget(list, list_area, &mut state);
    }

    // Separator
    let sep = Paragraph::new("─".repeat(separator_area.width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(sep, separator_area);

    // Mount instructions
    render_mount_instructions(app, frame, instructions_area);

    // Help text
    let help = Paragraph::new("[a] Add  [d] Remove  [s] Save  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, help_area);
}

/// Calculate the height needed for the folder list
fn folder_list_height(app: &App) -> u16 {
    let count = app.shared_folders.len();
    if count == 0 {
        2
    } else {
        (count as u16).min(6)
    }
}

/// Render mount instructions based on the OS tier
fn render_mount_instructions(app: &App, frame: &mut Frame, area: Rect) {
    let tier = get_mount_tier(app);

    // Use empty_state tier if no folders configured
    let effective_tier = if app.shared_folders.is_empty() {
        "empty_state"
    } else {
        tier
    };

    let (title, description) = app.shared_folders_help.get_or_default(effective_tier);

    // Expand {TAG} placeholders: lines containing {TAG} are repeated once per
    // folder with that folder's mount tag. Lines without {TAG} render once.
    let tags: Vec<&str> = app
        .shared_folders
        .iter()
        .map(|f| f.mount_tag.as_str())
        .collect();
    let fallback_tag = "host_shared";

    let mut expanded = String::new();
    for line in description.lines() {
        if line.contains("{TAG}") {
            if tags.is_empty() {
                expanded.push_str(&line.replace("{TAG}", fallback_tag));
                expanded.push('\n');
            } else {
                for tag in &tags {
                    expanded.push_str(&line.replace("{TAG}", tag));
                    expanded.push('\n');
                }
            }
        } else {
            expanded.push_str(line);
            expanded.push('\n');
        }
    }

    // Add a note when multiple folders are configured
    let header = if app.shared_folders.len() > 1 {
        format!("{} (repeat for each folder):", title)
    } else {
        format!("{}:", title)
    };

    let content = format!("{}\n{}", header, expanded.trim_end());

    let paragraph = Paragraph::new(content)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Determine the mount instruction tier based on the selected VM's OS profile
pub fn get_mount_tier(app: &App) -> &'static str {
    let vm = match app.selected_vm() {
        Some(vm) => vm,
        None => return "unknown",
    };

    // Get the OS profile ID from vm-curator.toml metadata, fall back to directory ID
    let profile_id = vm.os_profile.clone().or_else(|| Some(vm.id.clone()));

    // Check specific unsupported profile IDs first
    let unsupported_profiles = [
        "windows-95",
        "windows-98",
        "windows-me",
        "windows-2000",
        "windows-xp",
        "windows-vista",
        "windows-nt",
        "ms-dos",
        "freedos",
        "cpm",
    ];
    if let Some(ref id) = profile_id {
        if unsupported_profiles.iter().any(|p| id == p) {
            return "not_supported";
        }
    }

    // Check emulator for unsupported architectures
    let emulator = vm.config.emulator.command();
    if emulator.contains("m68k") || emulator.contains("ppc") {
        return "not_supported";
    }

    // Check specific supported Windows profiles
    if let Some(ref id) = profile_id {
        let windows_ids = [
            "windows-7",
            "windows-8",
            "windows-10",
            "windows-11",
            "generic-windows",
        ];
        if windows_ids.iter().any(|w| id == w) || id.starts_with("windows-server-") {
            return "windows";
        }
    }

    // Look up profile category
    if let Some(ref id) = profile_id {
        if let Some(profile) = app.qemu_profiles.get(id) {
            return match profile.category.as_str() {
                "linux" | "infrastructure" | "utilities" | "mobile" => "linux",
                "bsd" => "bsd",
                "macos" => "macos",
                "unix" => "unix",
                "alternative" => "alternative",
                "retro" | "classic-mac" => "not_supported",
                "windows" => "windows",
                _ => "unknown",
            };
        }
    }

    "unknown"
}

/// Handle key input for the shared folders screen
pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => {
            if !app.shared_folders.is_empty()
                && app.shared_folder_selected < app.shared_folders.len() - 1
            {
                app.shared_folder_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.shared_folder_selected > 0 {
                app.shared_folder_selected -= 1;
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            app.load_file_browser(FileBrowserMode::Directory);
            app.push_screen(Screen::FileBrowser);
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            app.remove_shared_folder();
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            save_shared_folders(app);
        }
        _ => {}
    }
    Ok(())
}

/// Save shared folders to launch.sh
fn save_shared_folders(app: &mut App) {
    let save_result = if let Some(vm) = app.selected_vm() {
        let result = crate::vm::save_shared_folders(vm, &app.shared_folders);
        Some((result, app.shared_folders.len()))
    } else {
        None
    };

    if let Some((result, count)) = save_result {
        match result {
            Ok(()) => {
                app.reload_selected_vm_script();
                if count > 0 {
                    app.set_status(format!("Saved {} shared folder(s) to launch.sh", count));
                } else {
                    app.set_status("Cleared shared folders from launch.sh");
                }
            }
            Err(e) => {
                app.set_status(format!("Error saving shared folders: {}", e));
            }
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
