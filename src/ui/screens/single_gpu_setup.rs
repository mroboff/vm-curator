//! Single GPU Passthrough Setup Screen
//!
//! Provides UI for configuring single GPU passthrough, including:
//! - GPU and IOMMU group information display
//! - Script generation controls
//! - System support status
//!
//! Note: Looking Glass is NOT used for single-GPU passthrough because the display
//! goes directly to physical monitors connected to the passed-through GPU.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, Screen};
use crate::hardware::single_gpu::{check_single_gpu_support, is_running_from_tty};
use crate::hardware::{scripts_exist, SingleGpuConfig};
use crate::vm::single_gpu_scripts;

/// Fields that can be focused in the setup screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupField {
    GenerateScripts,
    DeleteScripts,
}

impl SetupField {
    fn all() -> &'static [SetupField] {
        &[
            SetupField::GenerateScripts,
            SetupField::DeleteScripts,
        ]
    }
}

/// Render the single GPU setup screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Calculate dialog size
    let dialog_width = 72.min(area.width.saturating_sub(4));
    let dialog_height = 26.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Single GPU Passthrough Setup ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Layout: System support, GPU info, separator, scripts, help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // System support status
            Constraint::Length(6),  // GPU info
            Constraint::Length(1),  // Separator
            Constraint::Length(6),  // Scripts info
            Constraint::Min(1),     // Spacer
            Constraint::Length(2),  // Help
        ])
        .split(inner);

    // Render system support status
    render_system_support(app, frame, chunks[0]);

    // Render GPU info
    render_gpu_info(app, frame, chunks[1]);

    // Separator
    let sep1 = Paragraph::new("─".repeat(chunks[2].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(sep1, chunks[2]);

    // Scripts info
    render_scripts_info(app, frame, chunks[3]);

    // Help
    render_help(app, frame, chunks[5]);
}

/// Render system support status
fn render_system_support(_app: &App, frame: &mut Frame, area: Rect) {
    let support = check_single_gpu_support();

    let mut lines = Vec::new();

    // Status line
    let (status_text, status_color) = if support.is_supported() {
        ("System Ready", Color::Green)
    } else {
        ("System Not Ready", Color::Red)
    };

    lines.push(Line::from(vec![
        Span::styled("Status: ", Style::default().fg(Color::White)),
        Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
    ]));

    // Details
    let mut details = Vec::new();
    details.push(format!(
        "IOMMU: {}",
        if support.iommu_enabled { "Enabled" } else { "Disabled" }
    ));
    details.push(format!(
        "VFIO: {}",
        if support.vfio_available { "Available" } else { "Not Available" }
    ));

    lines.push(Line::styled(
        details.join("  |  "),
        Style::default().fg(Color::DarkGray),
    ));

    // Warning if running from graphical terminal
    if !is_running_from_tty() {
        lines.push(Line::styled(
            "Note: Scripts must be run from TTY, not graphical terminal",
            Style::default().fg(Color::Yellow),
        ));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Render GPU information section
fn render_gpu_info(app: &App, frame: &mut Frame, area: Rect) {
    let single_gpu_config = app.single_gpu_config.as_ref();

    let mut lines = Vec::new();

    if let Some(config) = single_gpu_config {
        // GPU name
        let gpu_name = if !config.gpu.device_name.is_empty() {
            format!("{} {}", config.gpu.short_vendor(), config.gpu.device_name)
        } else {
            format!("{} GPU", config.gpu.short_vendor())
        };
        lines.push(Line::from(vec![
            Span::styled("GPU: ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{} [{}]", gpu_name, config.gpu.address),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        // Audio device (if present)
        if let Some(ref audio) = config.audio {
            lines.push(Line::from(vec![
                Span::styled("Audio: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{} [{}]", audio.device_name, audio.address),
                    Style::default().fg(Color::Magenta),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Audio: ", Style::default().fg(Color::White)),
                Span::styled("None detected", Style::default().fg(Color::DarkGray)),
            ]));
        }

        // IOMMU group - use the stored iommu_group_devices
        let iommu_info = config
            .gpu
            .iommu_group
            .map(|g| {
                let device_count = config.iommu_group_devices.len();
                format!(
                    "{} ({} device{})",
                    g,
                    device_count,
                    if device_count == 1 { "" } else { "s" }
                )
            })
            .unwrap_or_else(|| "None".to_string());
        lines.push(Line::from(vec![
            Span::styled("IOMMU Group: ", Style::default().fg(Color::White)),
            Span::styled(iommu_info, Style::default().fg(Color::Yellow)),
        ]));

        // Passthrough addresses
        let addrs = config.all_passthrough_addresses();
        lines.push(Line::from(vec![
            Span::styled("Passthrough: ", Style::default().fg(Color::White)),
            Span::styled(addrs.join(", "), Style::default().fg(Color::White)),
        ]));

        // Display manager
        lines.push(Line::from(vec![
            Span::styled("Display Manager: ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{} (auto-detected)", config.display_manager.display_name()),
                Style::default().fg(Color::White),
            ),
        ]));
    } else {
        lines.push(Line::styled(
            "No GPU selected for passthrough",
            Style::default().fg(Color::Yellow),
        ));
        lines.push(Line::styled(
            "Select a boot VGA device from PCI Passthrough screen",
            Style::default().fg(Color::DarkGray),
        ));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Render scripts information
fn render_scripts_info(app: &App, frame: &mut Frame, area: Rect) {
    let vm = app.selected_vm();
    let selected_field = app.single_gpu_selected_field;

    let scripts_present = vm.map(|v| scripts_exist(&v.path)).unwrap_or(false);

    let vm_path = vm
        .map(|v| v.path.display().to_string())
        .unwrap_or_else(|| "~/vm-space/<vm>/".to_string());

    let mut lines = vec![
        Line::styled(
            "Scripts location:",
            Style::default().fg(Color::White),
        ),
        Line::styled(format!("  {}", vm_path), Style::default().fg(Color::DarkGray)),
    ];

    if scripts_present {
        lines.push(Line::styled(
            "  Scripts exist (will be overwritten on generate)",
            Style::default().fg(Color::Green),
        ));
    } else {
        lines.push(Line::styled(
            "  No scripts generated yet",
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Action buttons
    let generate_style = if selected_field == SetupField::GenerateScripts as usize {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    let delete_style = if selected_field == SetupField::DeleteScripts as usize {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if scripts_present {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    lines.push(Line::from(vec![
        Span::styled("[g] ", generate_style),
        Span::styled("Generate", generate_style),
        Span::raw("  "),
        Span::styled("[d] ", delete_style),
        Span::styled("Delete", delete_style),
    ]));

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Render help text
fn render_help(_app: &App, frame: &mut Frame, area: Rect) {
    let help = Paragraph::new(
        "[g] Generate Scripts  [d] Delete Scripts  [Esc] Back",
    )
    .style(Style::default().fg(Color::DarkGray))
    .alignment(Alignment::Center);
    frame.render_widget(help, area);
}

/// Render instructions dialog (shown after generating scripts)
pub fn render_instructions(app: &App, frame: &mut Frame) {
    let area = frame.area();

    let dialog_width = 64.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Single GPU Passthrough Launch ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let vm_path = app
        .selected_vm()
        .map(|v| v.path.display().to_string())
        .unwrap_or_else(|| "~/vm-space/<vm>".to_string());

    // Check if running from TTY
    let tty_warning = if !is_running_from_tty() {
        Line::styled(
            "WARNING: You are in a graphical session!",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Line::styled(
            "You are running from TTY (good!)",
            Style::default().fg(Color::Green),
        )
    };

    let lines = vec![
        tty_warning,
        Line::raw(""),
        Line::styled(
            "Single GPU passthrough requires running from TTY.",
            Style::default().fg(Color::Yellow),
        ),
        Line::raw(""),
        Line::styled(
            "Instructions:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw("1. Press Ctrl+Alt+F3 to switch to TTY3"),
        Line::raw("2. Log in with your username"),
        Line::raw("3. Run the following command:"),
        Line::raw(""),
        Line::styled(
            format!("   sudo {}/single-gpu-start.sh", vm_path),
            Style::default().fg(Color::Cyan),
        ),
        Line::raw(""),
        Line::raw("After the VM exits, your display will be restored."),
        Line::raw(""),
        Line::styled(
            "If something goes wrong, SSH in and run:",
            Style::default().fg(Color::Yellow),
        ),
        Line::styled(
            format!("   sudo {}/single-gpu-restore.sh", vm_path),
            Style::default().fg(Color::Cyan),
        ),
        Line::raw(""),
        Line::styled("[Enter/Esc] Close", Style::default().fg(Color::DarkGray)),
    ];

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

/// Handle key input for the single GPU setup screen
pub fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let field_count = SetupField::all().len();

    match key.code {
        KeyCode::Esc => {
            app.selected_menu_item = 0; // Reset for management menu
            app.pop_screen();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.single_gpu_selected_field > 0 {
                app.single_gpu_selected_field -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.single_gpu_selected_field < field_count - 1 {
                app.single_gpu_selected_field += 1;
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            handle_field_action(app)?;
        }
        KeyCode::Char('g') | KeyCode::Char('G') => {
            generate_scripts(app)?;
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            delete_scripts(app)?;
        }
        _ => {}
    }

    Ok(())
}

/// Handle action for the currently selected field
fn handle_field_action(app: &mut App) -> anyhow::Result<()> {
    let field = SetupField::all().get(app.single_gpu_selected_field);

    match field {
        Some(SetupField::GenerateScripts) => {
            generate_scripts(app)?;
        }
        Some(SetupField::DeleteScripts) => {
            delete_scripts(app)?;
        }
        None => {}
    }

    Ok(())
}

/// Generate single GPU passthrough scripts
fn generate_scripts(app: &mut App) -> anyhow::Result<()> {
    let vm = app.selected_vm().cloned();
    let config = app.single_gpu_config.clone();

    // Pre-flight check
    let support = check_single_gpu_support();
    if !support.is_supported() {
        app.set_status(format!("Cannot generate: {}", support.summary()));
        return Ok(());
    }

    match (vm, config) {
        (Some(vm), Some(config)) => {
            match crate::vm::generate_single_gpu_scripts(&vm, &config) {
                Ok(scripts) => {
                    // Log generated script paths
                    let dir = scripts.start_script.parent().unwrap();
                    app.set_status(format!(
                        "Generated: {}, {} in {}",
                        scripts.start_script.file_name().unwrap().to_string_lossy(),
                        scripts.restore_script.file_name().unwrap().to_string_lossy(),
                        dir.display()
                    ));
                    // Show instructions dialog
                    app.single_gpu_show_instructions = true;
                    app.push_screen(Screen::SingleGpuInstructions);
                }
                Err(e) => {
                    app.set_status(format!("Error generating scripts: {}", e));
                }
            }
        }
        (None, _) => {
            app.set_status("No VM selected");
        }
        (_, None) => {
            app.set_status("No GPU configured for passthrough");
        }
    }

    Ok(())
}

/// Delete single GPU passthrough scripts
fn delete_scripts(app: &mut App) -> anyhow::Result<()> {
    let vm = app.selected_vm();

    if let Some(vm) = vm {
        if !scripts_exist(&vm.path) {
            app.set_status("No scripts to delete");
            return Ok(());
        }

        match single_gpu_scripts::delete_scripts(&vm.path) {
            Ok(()) => {
                app.set_status("Scripts deleted");
            }
            Err(e) => {
                app.set_status(format!("Error deleting scripts: {}", e));
            }
        }
    } else {
        app.set_status("No VM selected");
    }

    Ok(())
}

/// Initialize single GPU config for the currently selected GPU
pub fn init_single_gpu_config(app: &mut App) {
    // Check if single GPU passthrough is supported
    let support = check_single_gpu_support();

    if !support.is_supported() {
        app.set_status(support.summary());
    }

    // Find the boot VGA device
    let boot_vga = app.pci_devices.iter().find(|d| d.can_single_gpu_passthrough());

    if let Some(gpu) = boot_vga {
        let config = SingleGpuConfig::new(gpu.clone(), &app.pci_devices);
        app.single_gpu_config = Some(config);
    } else if let Some(gpu) = app.pci_devices.iter().find(|d| d.is_boot_vga) {
        // Fallback to boot VGA even if it doesn't have IOMMU
        let config = SingleGpuConfig::new(gpu.clone(), &app.pci_devices);
        app.single_gpu_config = Some(config);
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
