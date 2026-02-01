//! Multi-GPU Passthrough Setup Screen
//!
//! Provides UI for configuring multi-GPU passthrough with Looking Glass:
//! - System requirements status (IOMMU, VFIO, Looking Glass client)
//! - GPU selection status
//! - Script generation and configuration

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{App, Screen};
use crate::hardware::{check_multi_gpu_passthrough_status, LookingGlassConfig};

/// Render the multi-GPU setup screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Calculate dialog size
    let dialog_width = 72.min(area.width.saturating_sub(4));
    let dialog_height = 24.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Multi-GPU Passthrough Setup ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Layout: System status, GPU info, separator, config, help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(6),  // System status
            Constraint::Length(1),  // Separator
            Constraint::Length(5),  // GPU info
            Constraint::Length(1),  // Separator
            Constraint::Length(4),  // Looking Glass config
            Constraint::Min(1),     // Spacer
            Constraint::Length(2),  // Help
        ])
        .split(inner);

    // Render system status
    render_system_status(frame, chunks[0]);

    // Separator
    let sep1 = Paragraph::new("─".repeat(chunks[1].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(sep1, chunks[1]);

    // Render GPU info
    render_gpu_info(app, frame, chunks[2]);

    // Separator
    let sep2 = Paragraph::new("─".repeat(chunks[3].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(sep2, chunks[3]);

    // Render Looking Glass config
    render_looking_glass_config(app, frame, chunks[4]);

    // Help
    render_help(frame, chunks[6]);
}

/// Render system status panel
fn render_system_status(frame: &mut Frame, area: Rect) {
    let status = check_multi_gpu_passthrough_status();

    let mut lines = Vec::new();

    // Title
    lines.push(Line::styled(
        "System Requirements:",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ));

    // IOMMU check
    let iommu_icon = if status.iommu_enabled { "[+]" } else { "[-]" };
    let iommu_style = if status.iommu_enabled { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(iommu_icon, Style::default().fg(iommu_style)),
        Span::raw(" IOMMU enabled (required for GPU passthrough)"),
    ]));

    // VFIO check
    let vfio_icon = if status.vfio_loaded { "[+]" } else { "[-]" };
    let vfio_style = if status.vfio_loaded { Color::Green } else { Color::Red };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(vfio_icon, Style::default().fg(vfio_style)),
        Span::raw(" VFIO modules loaded"),
    ]));

    // GPU count
    let gpu_ok = status.available_gpus > 0;
    let gpu_icon = if gpu_ok { "[+]" } else { "[-]" };
    let gpu_style = if gpu_ok { Color::Green } else { Color::Red };
    let gpu_text = format!(
        " {} GPU{} available for passthrough",
        status.available_gpus,
        if status.available_gpus == 1 { "" } else { "s" }
    );
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(gpu_icon, Style::default().fg(gpu_style)),
        Span::raw(gpu_text),
    ]));

    // Looking Glass check
    let lg_client = LookingGlassConfig::find_client();
    let lg_ok = lg_client.is_some();
    let lg_icon = if lg_ok { "[+]" } else { "[-]" };
    let lg_style = if lg_ok { Color::Green } else { Color::Yellow };
    let lg_text = if let Some(ref path) = lg_client {
        format!(" Looking Glass client: {}", path.display())
    } else {
        " Looking Glass client not found (optional)".to_string()
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(lg_icon, Style::default().fg(lg_style)),
        Span::raw(lg_text),
    ]));

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Render GPU info panel
fn render_gpu_info(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        "Selected GPUs for Passthrough:",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ));

    // Get selected GPUs from PCI devices (selected_pci_devices contains indices)
    let selected_gpus: Vec<_> = app.selected_pci_devices
        .iter()
        .filter_map(|&idx| app.pci_devices.get(idx))
        .filter(|d| d.is_gpu())
        .collect();

    if selected_gpus.is_empty() {
        lines.push(Line::styled(
            "  No GPUs selected for passthrough",
            Style::default().fg(Color::Yellow),
        ));
        lines.push(Line::styled(
            "  Use PCI Passthrough to select a GPU",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        for gpu in selected_gpus {
            let vendor_color = if gpu.is_nvidia() {
                Color::Green
            } else if gpu.is_amd() {
                Color::Red
            } else {
                Color::White
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{} - ", gpu.address),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(&gpu.device_name, Style::default().fg(vendor_color)),
            ]));
        }
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Render Looking Glass config panel
fn render_looking_glass_config(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        "Looking Glass Configuration:",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ));

    // IVSHMEM size
    lines.push(Line::from(vec![
        Span::raw("  IVSHMEM Size: "),
        Span::styled(
            format!("{}MB", app.config.default_ivshmem_size_mb),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            " (configure in Settings)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Auto-launch
    let auto_launch = if app.config.looking_glass_auto_launch { "Yes" } else { "No" };
    lines.push(Line::from(vec![
        Span::raw("  Auto-launch client: "),
        Span::styled(auto_launch, Style::default().fg(Color::White)),
    ]));

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Render help text
fn render_help(frame: &mut Frame, area: Rect) {
    let help = Paragraph::new("[p] PCI Passthrough  [s] Settings  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, area);
}

/// Handle input for the multi-GPU setup screen
pub fn handle_input(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.selected_menu_item = 0; // Reset for management menu
            app.pop_screen();
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            // Go to PCI Passthrough screen
            app.push_screen(Screen::PciPassthrough);
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Go to Settings screen
            app.push_screen(Screen::Settings);
        }
        _ => {}
    }

    Ok(())
}

/// Helper function to create a centered rect
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
