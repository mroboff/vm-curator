//! Dialog widgets for the TUI

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Confirmation dialog widget
pub struct ConfirmDialog<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub confirm_label: &'a str,
    pub cancel_label: &'a str,
}

impl<'a> ConfirmDialog<'a> {
    pub fn new(title: &'a str, message: &'a str) -> Self {
        Self {
            title,
            message,
            confirm_label: "Yes (y)",
            cancel_label: "No (n)",
        }
    }

    pub fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate dialog size
        let dialog_width = 50.min(area.width.saturating_sub(4));
        let dialog_height = 8.min(area.height.saturating_sub(4));

        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        // Clear the area
        Clear.render(dialog_area, buf);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Split into message and buttons
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(2)])
            .split(inner);

        // Render message
        let message = Paragraph::new(self.message)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true });
        message.render(chunks[0], buf);

        // Render buttons
        let buttons = Line::from(vec![
            Span::styled(
                format!(" {} ", self.confirm_label),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!(" {} ", self.cancel_label),
                Style::default().fg(Color::Red),
            ),
        ]);
        let buttons_para = Paragraph::new(buttons).alignment(Alignment::Center);
        buttons_para.render(chunks[1], buf);
    }
}

/// Helper to create a centered rectangle
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
