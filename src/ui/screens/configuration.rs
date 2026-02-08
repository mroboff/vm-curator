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
        let backend_str = match &net.backend {
            crate::vm::qemu_config::NetworkBackend::User => "user/SLIRP (NAT)".to_string(),
            crate::vm::qemu_config::NetworkBackend::Passt => "passt".to_string(),
            crate::vm::qemu_config::NetworkBackend::Bridge(name) => format!("bridge: {}", name),
            crate::vm::qemu_config::NetworkBackend::None => "none".to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled("Network: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{} ({})", net.model, backend_str)),
        ]));
        if !net.port_forwards.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Forwarded ports:",
                Style::default().fg(Color::DarkGray),
            )));
            for pf in &net.port_forwards {
                lines.push(Line::from(format!("    {} {} -> {}", pf.protocol, pf.host_port, pf.guest_port)));
            }
        }
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

/// Render raw script editor
pub fn render_raw_script(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 90.min(area.width.saturating_sub(4));
    let dialog_height = 40.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let vm_name = app.selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "Unknown".to_string());

    let modified_indicator = if app.script_editor_modified { " [modified]" } else { "" };

    let block = Block::default()
        .title(format!(" {} - launch.sh{} ", vm_name, modified_indicator))
        .borders(Borders::ALL)
        .border_style(if app.script_editor_modified {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Cyan)
        })
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Split into line numbers, content, and help text
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),     // Editor content
            Constraint::Length(1),  // Help text
        ])
        .split(inner);

    let editor_area = v_chunks[0];
    let help_area = v_chunks[1];

    // Split editor area into line numbers and text
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(5),  // Line numbers
            Constraint::Min(1),     // Text content
        ])
        .split(editor_area);

    let line_num_area = h_chunks[0];
    let text_area = h_chunks[1];

    let visible_height = text_area.height as usize;
    let total_lines = app.script_editor_lines.len();
    let scroll_offset = app.raw_script_scroll as usize;

    // Calculate visible line range
    let start_line = scroll_offset;
    let end_line = (scroll_offset + visible_height).min(total_lines);

    // Render line numbers
    let line_numbers: Vec<Line> = (start_line..end_line)
        .map(|i| {
            let style = if i == app.script_editor_cursor.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::styled(format!("{:4} ", i + 1), style)
        })
        .collect();
    let line_nums_widget = Paragraph::new(line_numbers);
    frame.render_widget(line_nums_widget, line_num_area);

    // Render text content with cursor
    let text_width = text_area.width as usize;
    let h_scroll = app.script_editor_h_scroll;

    let text_lines: Vec<Line> = (start_line..end_line)
        .map(|i| {
            let line = app.script_editor_lines.get(i).map(|s| s.as_str()).unwrap_or("");

            // Apply horizontal scroll
            let visible_line = if h_scroll < line.len() {
                &line[h_scroll..]
            } else {
                ""
            };

            // Truncate to visible width
            let display_line: String = visible_line.chars().take(text_width).collect();

            if i == app.script_editor_cursor.0 {
                // This is the cursor line - highlight it slightly
                Line::styled(display_line, Style::default().fg(Color::White))
            } else {
                Line::styled(display_line, Style::default().fg(Color::Gray))
            }
        })
        .collect();

    let text_widget = Paragraph::new(text_lines);
    frame.render_widget(text_widget, text_area);

    // Draw cursor
    let cursor_line = app.script_editor_cursor.0;
    let cursor_col = app.script_editor_cursor.1;

    if cursor_line >= scroll_offset && cursor_line < scroll_offset + visible_height {
        let screen_y = text_area.y + (cursor_line - scroll_offset) as u16;
        let screen_x = if cursor_col >= h_scroll {
            let col_in_view = cursor_col - h_scroll;
            if col_in_view < text_width {
                text_area.x + col_in_view as u16
            } else {
                text_area.x + text_area.width - 1
            }
        } else {
            text_area.x
        };

        // Set cursor position
        frame.set_cursor_position((screen_x, screen_y));
    }

    // Help text
    let help_text = if app.script_editor_modified {
        "[Ctrl+S] Save  [Esc] Cancel  [↑/↓/←/→] Navigate  [PgUp/PgDn] Scroll"
    } else {
        "[Esc] Back  [↑/↓/←/→] Navigate  [PgUp/PgDn] Scroll"
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, help_area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
