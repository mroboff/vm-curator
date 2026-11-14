use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Render the help screen
pub fn render(frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 64.min(area.width.saturating_sub(4));
    let dialog_height = 34.min(area.height.saturating_sub(4));

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
        heading("Main menu"),
        Line::from(""),
        key_line("j / k", "Move selection"),
        key_line("Enter", "Launch selected VM / confirm"),
        key_line("m", "Open Management menu"),
        key_line("c / i", "Create / import a VM"),
        key_line("s / n", "Settings / Networks"),
        key_line("x", "Stop the running VM"),
        key_line("/", "Search VMs, notes and OS names"),
        key_line("r", "Refresh the VM list"),
        key_line("Home / End", "Jump to first / last VM"),
        key_line("g / G", "Jump to first / last VM"),
        key_line("PgUp / PgDn", "Scroll the info panel"),
        Line::from(""),
        heading("Management menu"),
        Line::from(""),
        key_line("j / k", "Move between actions"),
        key_line("Enter", "Run the selected action"),
        key_line("1 - 9", "Jump directly to an action"),
        key_line("Esc", "Back to the VM list"),
        Line::from(""),
        heading("Lists and dialogs"),
        Line::from(""),
        key_line("s / d / r", "Save / delete / restore or refresh"),
        key_line("Space", "Toggle a checkbox or radio option"),
        key_line("Esc", "Cancel / go back"),
        Line::from(""),
        Line::from(Span::styled(
            "Mouse: click and scroll on the main list; the wheel also works in",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "management, snapshots and networks. Other screens are keyboard-first.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        key_line("q / Ctrl+C", "Quit application"),
        Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let para = Paragraph::new(help_text);
    frame.render_widget(para, inner);
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))
}

fn key_line<'a>(key: &'a str, description: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:14}", key), Style::default().fg(Color::Green)),
        Span::raw(description),
    ])
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
