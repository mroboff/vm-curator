use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::App;
use crate::vm::QemuConfig;

/// Render the configuration view
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 70.min(area.width.saturating_sub(4));
    let dialog_height = 30.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let vm_name = app.selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "Unknown".to_string());

    let block = Block::default()
        .title(format!(" {} - Configuration ", vm_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Add horizontal margins
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),  // Left margin
            Constraint::Min(1),     // Content
            Constraint::Length(2),  // Right margin
        ])
        .split(inner);

    // Split into padding, config, bottom padding, and help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),   // Top padding
            Constraint::Min(10),     // Config content
            Constraint::Length(1),   // Bottom padding
            Constraint::Length(2),   // Help text
        ])
        .split(h_chunks[1]);

    if let Some(vm) = app.selected_vm() {
        render_config(&vm.config, chunks[1], frame);
    } else {
        let msg = Paragraph::new("No VM selected")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, chunks[1]);
    }

    // Help text
    let help = Paragraph::new("[r] View raw script  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
}

fn render_config(config: &QemuConfig, area: Rect, frame: &mut Frame) {
    let mut lines = Vec::new();

    // Emulator
    lines.push(Line::from(vec![
        Span::styled("Emulator: ", Style::default().fg(Color::Yellow)),
        Span::raw(config.emulator.command()),
    ]));

    // Architecture
    lines.push(Line::from(vec![
        Span::styled("Architecture: ", Style::default().fg(Color::Yellow)),
        Span::raw(config.emulator.architecture()),
    ]));

    // Memory
    lines.push(Line::from(vec![
        Span::styled("Memory: ", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{} MB", config.memory_mb)),
    ]));

    // CPU
    lines.push(Line::from(vec![
        Span::styled("CPU Cores: ", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{}", config.cpu_cores)),
    ]));

    if let Some(ref model) = config.cpu_model {
        lines.push(Line::from(vec![
            Span::styled("CPU Model: ", Style::default().fg(Color::Yellow)),
            Span::raw(model.clone()),
        ]));
    }

    // Machine type
    if let Some(ref machine) = config.machine {
        lines.push(Line::from(vec![
            Span::styled("Machine: ", Style::default().fg(Color::Yellow)),
            Span::raw(machine.clone()),
        ]));
    }

    lines.push(Line::from(""));

    // Graphics
    lines.push(Line::from(vec![
        Span::styled("VGA: ", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{:?}", config.vga)),
    ]));

    // Audio
    if !config.audio_devices.is_empty() {
        let audio_str = config.audio_devices
            .iter()
            .map(|a| format!("{:?}", a))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(Line::from(vec![
            Span::styled("Audio: ", Style::default().fg(Color::Yellow)),
            Span::raw(audio_str),
        ]));
    }

    // Network
    if let Some(ref net) = config.network {
        lines.push(Line::from(vec![
            Span::styled("Network: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{} ({})", net.model, if net.user_net { "user" } else { "bridge" })),
        ]));
    }

    lines.push(Line::from(""));

    // Disks
    lines.push(Line::from(Span::styled(
        "Disks:",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));

    for disk in &config.disks {
        let path = disk.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        lines.push(Line::from(format!(
            "  {} ({:?}, {})",
            path, disk.format, disk.interface
        )));
    }

    lines.push(Line::from(""));

    // Features
    let mut features = Vec::new();
    if config.enable_kvm {
        features.push("KVM");
    }
    if config.uefi {
        features.push("UEFI");
    }
    if config.tpm {
        features.push("TPM");
    }

    if !features.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Features: ", Style::default().fg(Color::Yellow)),
            Span::raw(features.join(", ")),
        ]));
    }

    // Snapshot support
    let snapshot_support = if config.supports_snapshots() {
        Span::styled("Yes", Style::default().fg(Color::Green))
    } else {
        Span::styled("No (raw disk)", Style::default().fg(Color::Red))
    };
    lines.push(Line::from(vec![
        Span::styled("Snapshots: ", Style::default().fg(Color::Yellow)),
        snapshot_support,
    ]));

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

/// Render raw script view
pub fn render_raw_script(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 80.min(area.width.saturating_sub(4));
    let dialog_height = 35.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let vm_name = app.selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "Unknown".to_string());

    let block = Block::default()
        .title(format!(" {} - launch.sh ", vm_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Add horizontal margins
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),  // Left margin
            Constraint::Min(1),     // Content
            Constraint::Length(2),  // Right margin
        ])
        .split(inner);

    // Split into padding, script content, padding, and help text
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Min(1),     // Script content
            Constraint::Length(1),  // Bottom padding
            Constraint::Length(1),  // Help text
        ])
        .split(h_chunks[1]);

    if let Some(vm) = app.selected_vm() {
        let script = &vm.config.raw_script;
        let para = Paragraph::new(script.as_str())
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false })
            .scroll((app.raw_script_scroll, 0));
        frame.render_widget(para, chunks[1]);
    }

    // Help text
    let help = Paragraph::new("[↑/↓] Scroll  [Esc] Back  [q] Quit")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
