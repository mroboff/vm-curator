//! VM Import Wizard screens
//!
//! A multi-step wizard for importing VMs from libvirt XML and quickemu .conf files.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::{
    App, FileBrowserMode, ImportDiskAction, ImportSource, ImportStep, ImportWizardState,
};
use crate::vm::import;

// =========================================================================
// Rendering
// =========================================================================

/// Render the import wizard based on current step
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    let dialog_width = 80.min(area.width.saturating_sub(4));
    let dialog_height = 36.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let Some(ref state) = app.import_state else {
        return;
    };

    match state.step {
        ImportStep::SelectSource => render_step_select_source(state, frame, dialog_area),
        ImportStep::SelectVm => render_step_select_vm(state, frame, dialog_area),
        ImportStep::CompatibilityWarnings => render_step_warnings(state, frame, dialog_area),
        ImportStep::ConfigureDisk => render_step_configure_disk(state, frame, dialog_area),
        ImportStep::ReviewAndImport => render_step_review(state, frame, dialog_area),
    }
}

/// Step 1: Select import source
fn render_step_select_source(state: &ImportWizardState, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Import VM - Select Source ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // Description
            Constraint::Length(1), // Spacer
            Constraint::Min(8),   // Options
            Constraint::Length(2), // Help
        ])
        .split(inner);

    let desc = Paragraph::new("Select the source format of the VM you want to import:")
        .style(Style::default().fg(Color::White));
    frame.render_widget(desc, chunks[0]);

    let options: &[(&str, &str)] = &[
        ("libvirt (XML)", "Import from libvirt/virt-manager domain XML"),
        ("quickemu (.conf)", "Import from quickemu configuration file"),
        ("Browse for config file...", "Browse filesystem for .xml or .conf file"),
    ];

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, (label, desc))| {
            let style = if i == state.field_focus {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let marker = if i == state.field_focus { "> " } else { "  " };
            ListItem::new(vec![
                Line::from(Span::styled(format!("{}{}", marker, label), style)),
                Line::from(Span::styled(
                    format!("    {}", desc),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, chunks[2]);

    if let Some(ref err) = state.error_message {
        let help = Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
    } else {
        let help = Paragraph::new("[Enter] Select  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
    }
}

/// Step 2: Select VM from discovered list
fn render_step_select_vm(state: &ImportWizardState, frame: &mut Frame, area: Rect) {
    let source_label = match state.source {
        Some(ImportSource::Libvirt) => "libvirt",
        Some(ImportSource::Quickemu) => "quickemu",
        None => "unknown",
    };

    let block = Block::default()
        .title(format!(" Import VM - Select {} VM ", source_label))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Description
            Constraint::Length(1), // Spacer
            Constraint::Min(8),   // VM list
            Constraint::Length(2), // Help
        ])
        .split(inner);

    if state.discovered_vms.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "No VMs found.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Press [b] to browse for a config file manually."),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(msg, chunks[2]);

        let help = Paragraph::new("[b] Browse  [Esc] Back")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
        return;
    }

    let desc = Paragraph::new(format!(
        "Found {} VM(s). Select one to import:",
        state.discovered_vms.len()
    ))
    .style(Style::default().fg(Color::White));
    frame.render_widget(desc, chunks[0]);

    let items: Vec<ListItem> = state
        .discovered_vms
        .iter()
        .enumerate()
        .map(|(i, vm)| {
            let style = if i == state.selected_vm_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let uefi_str = if vm.qemu_config.uefi { "UEFI" } else { "BIOS" };
            let tpm_str = if vm.qemu_config.tpm { "+TPM" } else { "" };
            let summary = format!(
                "{}MB RAM, {} CPUs, {}{} ",
                vm.qemu_config.memory_mb, vm.qemu_config.cpu_cores, uefi_str, tpm_str
            );

            ListItem::new(vec![
                Line::from(Span::styled(format!("  {}", vm.name), style)),
                Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(summary, Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        vm.config_path.display().to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ])
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_vm_index));

    let list = List::new(items).highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[2], &mut list_state);

    if let Some(ref err) = state.error_message {
        let help = Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
    } else {
        let help = Paragraph::new("[Enter] Select  [b] Browse  [Esc] Back")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
    }
}

/// Step 3: Compatibility warnings (only shown when there are import notes)
fn render_step_warnings(state: &ImportWizardState, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Import VM - Compatibility ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(1), // Spacer
            Constraint::Min(8),   // Warnings list
            Constraint::Length(2), // Help
        ])
        .split(inner);

    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            "  Configuration Changes Required",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  The source VM uses features that require adjustment:"),
    ]);
    frame.render_widget(header, chunks[0]);

    if let Some(ref vm) = state.selected_vm {
        let mut lines = Vec::new();
        for note in &vm.import_notes {
            lines.push(Line::from(vec![
                Span::styled("  * ", Style::default().fg(Color::Yellow)),
                Span::styled(note.as_str(), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(""));
        }

        let warnings = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(warnings, chunks[2]);
    }

    let help = Paragraph::new("[Enter] Accept changes and continue  [Esc] Cancel import")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
}

/// Step 4: Configure disk handling
fn render_step_configure_disk(state: &ImportWizardState, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Import VM - Disk Handling ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // Description
            Constraint::Length(1), // Spacer
            Constraint::Min(6),   // Disk info + options
            Constraint::Length(4), // Warning text
            Constraint::Length(2), // Help
        ])
        .split(inner);

    let desc = Paragraph::new("Choose how to handle the source VM's disk image(s):")
        .style(Style::default().fg(Color::White));
    frame.render_widget(desc, chunks[0]);

    let mut content_lines: Vec<Line> = Vec::new();

    // Show disk info
    if let Some(ref vm) = state.selected_vm {
        for (i, disk) in vm.disk_paths.iter().enumerate() {
            let size_str = std::fs::metadata(disk)
                .map(|m| {
                    let gb = m.len() as f64 / (1024.0 * 1024.0 * 1024.0);
                    format!("{:.1} GB", gb)
                })
                .unwrap_or_else(|_| "unknown size".to_string());

            let readable = vm.disks_readable.get(i).copied().unwrap_or(false);
            let status = if readable { "" } else { " (not readable!)" };

            content_lines.push(Line::from(vec![
                Span::styled("  Disk: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} ({}){}", disk.display(), size_str, status),
                    Style::default().fg(if readable {
                        Color::White
                    } else {
                        Color::Red
                    }),
                ),
            ]));
        }
        content_lines.push(Line::from(""));
    }

    // Disk action options
    let actions = [
        (ImportDiskAction::Symlink, "Symlink", "Instant, saves space. Original must stay in place."),
        (ImportDiskAction::Copy, "Copy", "Independent copy. Slow for large disks."),
        (ImportDiskAction::Move, "Move", "Relocates disk to VM library."),
    ];

    for (i, (action, label, desc)) in actions.iter().enumerate() {
        let selected = state.disk_action == *action;
        let focused = i == state.field_focus;
        let radio = if selected { "(*)" } else { "( )" };
        let style = if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        content_lines.push(Line::from(vec![
            Span::styled(format!("  {} {} ", radio, label), style),
            Span::styled(
                format!("- {}", desc),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let content = Paragraph::new(content_lines);
    frame.render_widget(content, chunks[2]);

    let warning_text = match state.disk_action {
        ImportDiskAction::Symlink => {
            "Note: Symlinked disks depend on the original file remaining in place.\nIf the original is deleted or moved, the VM will fail to start."
        }
        ImportDiskAction::Copy => {
            "Note: Copying may take a long time for large disk images.\nThe original file is not modified."
        }
        ImportDiskAction::Move => {
            "Note: The original disk file will be moved to the VM library.\nThe source VM will no longer have access to it."
        }
    };
    let warning = Paragraph::new(warning_text)
        .style(Style::default().fg(Color::DarkGray))
        .wrap(Wrap { trim: false });
    frame.render_widget(warning, chunks[3]);

    let help = Paragraph::new("[Enter] Continue  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[4]);
}

/// Step 5: Review and import
fn render_step_review(state: &ImportWizardState, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Import VM - Review & Import ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(16),  // Config summary
            Constraint::Length(2), // Help
        ])
        .split(inner);

    let mut lines: Vec<Line> = Vec::new();

    // VM Name (editable)
    if state.editing_name {
        lines.push(Line::from(vec![
            Span::styled("  VM Name:    ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}_", state.vm_name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  VM Name:    ", Style::default().fg(Color::Gray)),
            Span::styled(&state.vm_name, Style::default().fg(Color::White)),
            Span::styled(
                "  [Tab to edit]",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  Folder:     ", Style::default().fg(Color::Gray)),
        Span::styled(&state.folder_name, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    if let Some(ref vm) = state.selected_vm {
        let cfg = &vm.qemu_config;

        lines.push(Line::from(vec![
            Span::styled("  Emulator:   ", Style::default().fg(Color::Gray)),
            Span::styled(&cfg.emulator, Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Memory:     ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} MB", cfg.memory_mb),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  CPU Cores:  ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", cfg.cpu_cores),
                Style::default().fg(Color::White),
            ),
        ]));
        if let Some(ref machine) = cfg.machine {
            lines.push(Line::from(vec![
                Span::styled("  Machine:    ", Style::default().fg(Color::Gray)),
                Span::styled(machine, Style::default().fg(Color::White)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("  VGA:        ", Style::default().fg(Color::Gray)),
            Span::styled(&cfg.vga, Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Display:    ", Style::default().fg(Color::Gray)),
            Span::styled(&cfg.display, Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Network:    ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} ({})", cfg.network_model, cfg.network_backend),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  KVM:        ", Style::default().fg(Color::Gray)),
            Span::styled(
                if cfg.enable_kvm { "Yes" } else { "No" },
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  UEFI:       ", Style::default().fg(Color::Gray)),
            Span::styled(
                if cfg.uefi { "Yes" } else { "No" },
                Style::default().fg(Color::White),
            ),
        ]));
        if cfg.tpm {
            lines.push(Line::from(vec![
                Span::styled("  TPM:        ", Style::default().fg(Color::Gray)),
                Span::styled("Yes", Style::default().fg(Color::White)),
            ]));
        }

        let disk_action_str = match state.disk_action {
            ImportDiskAction::Symlink => "Symlink",
            ImportDiskAction::Copy => "Copy",
            ImportDiskAction::Move => "Move",
        };
        lines.push(Line::from(vec![
            Span::styled("  Disk:       ", Style::default().fg(Color::Gray)),
            Span::styled(disk_action_str, Style::default().fg(Color::White)),
        ]));

        // Show compatibility note count
        if !vm.import_notes.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} compatibility change(s) applied (acknowledged in step 3)",
                    vm.import_notes.len()
                ),
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    if let Some(ref err) = state.error_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  Error: {}", err),
            Style::default().fg(Color::Red),
        )));
    }

    let summary = Paragraph::new(lines);
    frame.render_widget(summary, chunks[0]);

    let help = Paragraph::new("[Enter] Import  [Tab] Edit name  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[1]);
}

// =========================================================================
// Key Handling
// =========================================================================

/// Handle key input for the import wizard
pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let step = app
        .import_state
        .as_ref()
        .map(|s| s.step.clone())
        .unwrap_or_default();

    match step {
        ImportStep::SelectSource => handle_select_source(app, key),
        ImportStep::SelectVm => handle_select_vm(app, key),
        ImportStep::CompatibilityWarnings => handle_warnings(app, key),
        ImportStep::ConfigureDisk => handle_configure_disk(app, key),
        ImportStep::ReviewAndImport => handle_review(app, key),
    }
}

fn handle_select_source(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.cancel_import_wizard();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.import_state {
                if state.field_focus < 2 {
                    state.field_focus += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.import_state {
                if state.field_focus > 0 {
                    state.field_focus -= 1;
                }
            }
        }
        KeyCode::Enter => {
            let focus = app.import_state.as_ref().map(|s| s.field_focus).unwrap_or(0);
            match focus {
                0 => {
                    // libvirt
                    if let Some(ref mut state) = app.import_state {
                        state.source = Some(ImportSource::Libvirt);
                        state.discovered_vms = import::discover_libvirt_vms();
                        state.selected_vm_index = 0;
                        state.step = ImportStep::SelectVm;
                        state.field_focus = 0;
                        state.error_message = None;
                    }
                }
                1 => {
                    // quickemu
                    if let Some(ref mut state) = app.import_state {
                        state.source = Some(ImportSource::Quickemu);
                        state.discovered_vms = import::discover_quickemu_vms();
                        state.selected_vm_index = 0;
                        state.step = ImportStep::SelectVm;
                        state.field_focus = 0;
                        state.error_message = None;
                    }
                }
                2 => {
                    // Browse for config file
                    app.load_file_browser(FileBrowserMode::ImportConfig);
                    app.push_screen(crate::app::Screen::FileBrowser);
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_select_vm(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            if let Some(ref mut state) = app.import_state {
                state.step = ImportStep::SelectSource;
                state.field_focus = 0;
                state.error_message = None;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.import_state {
                let max = state.discovered_vms.len().saturating_sub(1);
                if state.selected_vm_index < max {
                    state.selected_vm_index += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.import_state {
                if state.selected_vm_index > 0 {
                    state.selected_vm_index -= 1;
                }
            }
        }
        KeyCode::Char('b') | KeyCode::Char('B') => {
            // Browse for config file
            app.load_file_browser(FileBrowserMode::ImportConfig);
            app.push_screen(crate::app::Screen::FileBrowser);
        }
        KeyCode::Enter => {
            if let Some(ref mut state) = app.import_state {
                if let Some(vm) = state.discovered_vms.get(state.selected_vm_index).cloned() {
                    let library_path = app.config.vm_library_path.clone();
                    state.vm_name = vm.name.clone();
                    state.folder_name =
                        crate::app::CreateWizardState::find_available_folder_name(
                            &library_path,
                            &crate::app::CreateWizardState::generate_folder_name(&vm.name),
                        );
                    state.selected_vm = Some(vm.clone());
                    state.error_message = None;
                    state.field_focus = 0;

                    // Skip to warnings if there are import notes, otherwise go to disk config
                    if vm.import_notes.is_empty() {
                        state.warnings_acknowledged = true;
                        state.step = ImportStep::ConfigureDisk;
                    } else {
                        state.warnings_acknowledged = false;
                        state.step = ImportStep::CompatibilityWarnings;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_warnings(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            if let Some(ref mut state) = app.import_state {
                state.step = ImportStep::SelectVm;
                state.field_focus = 0;
            }
        }
        KeyCode::Enter => {
            if let Some(ref mut state) = app.import_state {
                state.warnings_acknowledged = true;
                state.step = ImportStep::ConfigureDisk;
                state.field_focus = 0;
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_configure_disk(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            if let Some(ref mut state) = app.import_state {
                // Go back to warnings if they existed, otherwise to VM selection
                if state.selected_vm.as_ref().map(|vm| !vm.import_notes.is_empty()).unwrap_or(false)
                {
                    state.step = ImportStep::CompatibilityWarnings;
                } else {
                    state.step = ImportStep::SelectVm;
                }
                state.field_focus = 0;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.import_state {
                if state.field_focus < 2 {
                    state.field_focus += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.import_state {
                if state.field_focus > 0 {
                    state.field_focus -= 1;
                }
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(ref mut state) = app.import_state {
                // Select disk action based on focus
                match state.field_focus {
                    0 => state.disk_action = ImportDiskAction::Symlink,
                    1 => state.disk_action = ImportDiskAction::Copy,
                    2 => state.disk_action = ImportDiskAction::Move,
                    _ => {}
                }

                // If Enter (not space), advance to review
                if key.code == KeyCode::Enter {
                    state.step = ImportStep::ReviewAndImport;
                    state.field_focus = 0;
                    state.error_message = None;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_review(app: &mut App, key: KeyEvent) -> Result<()> {
    if let Some(ref state) = app.import_state {
        if state.editing_name {
            return handle_review_editing(app, key);
        }
    }

    match key.code {
        KeyCode::Esc => {
            if let Some(ref mut state) = app.import_state {
                state.step = ImportStep::ConfigureDisk;
                state.field_focus = 0;
                state.error_message = None;
            }
        }
        KeyCode::Tab => {
            if let Some(ref mut state) = app.import_state {
                state.editing_name = true;
            }
        }
        KeyCode::Enter => {
            // Execute import
            let result = execute_import_from_state(app);
            match result {
                Ok(()) => {
                    let vm_name = app
                        .import_state
                        .as_ref()
                        .map(|s| s.vm_name.clone())
                        .unwrap_or_default();
                    app.import_state = None;

                    // Pop the ImportWizard screen
                    while app.screen == crate::app::Screen::ImportWizard {
                        app.pop_screen();
                    }

                    // Refresh VM list
                    let _ = app.refresh_vms();
                    app.set_status(format!("Imported: {}", vm_name));
                }
                Err(e) => {
                    if let Some(ref mut state) = app.import_state {
                        state.error_message = Some(e.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_review_editing(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Tab => {
            if let Some(ref mut state) = app.import_state {
                state.editing_name = false;
                // Regenerate folder name from new VM name
                let library_path = app.config.vm_library_path.clone();
                state.folder_name = crate::app::CreateWizardState::find_available_folder_name(
                    &library_path,
                    &crate::app::CreateWizardState::generate_folder_name(&state.vm_name),
                );
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut state) = app.import_state {
                state.vm_name.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut state) = app.import_state {
                state.vm_name.push(c);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Execute the import using current wizard state
fn execute_import_from_state(app: &App) -> Result<()> {
    let state = app
        .import_state
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Import wizard not active"))?;

    let vm = state
        .selected_vm
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No VM selected"))?;

    if state.vm_name.trim().is_empty() {
        anyhow::bail!("VM name cannot be empty");
    }
    if state.folder_name.is_empty() {
        anyhow::bail!("Folder name cannot be empty");
    }

    import::execute_import(
        &app.config.vm_library_path,
        vm,
        &state.vm_name,
        &state.folder_name,
        state.disk_action,
    )?;

    Ok(())
}

// =========================================================================
// Helpers
// =========================================================================

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
