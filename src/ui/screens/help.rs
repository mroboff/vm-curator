use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Render the help screen
pub fn render(frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 55.min(area.width.saturating_sub(4));
    let dialog_height = 28.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Help - Key Bindings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let help_text = vec![
        Line::from(Span::styled(
            "Navigation",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        key_line("j / Down", "Move selection down"),
        key_line("k / Up", "Move selection up"),
        key_line("Enter", "Launch selected VM / Confirm"),
        key_line("Esc", "Go back / Cancel"),
        Line::from(""),
        Line::from(Span::styled(
            "Actions",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        key_line("m", "Open Management menu"),
        key_line("x", "Stop selected VM (graceful shutdown)"),
        key_line("c", "Create new VM"),
        key_line("/", "Search/filter VMs"),
        Line::from(""),
        Line::from(Span::styled(
            "Management Menu",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        key_line("Network", "Backend, port forwarding"),
        Line::from(""),
        Line::from(Span::styled(
            "General",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        key_line("?", "Show this help"),
        key_line("q", "Quit application"),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let para = Paragraph::new(help_text);
    frame.render_widget(para, inner);
}

fn key_line<'a>(key: &'a str, description: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:12}", key),
            Style::default().fg(Color::Green),
        ),
        Span::raw(description),
    ])
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
