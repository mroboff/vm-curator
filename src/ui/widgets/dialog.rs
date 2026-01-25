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

/// Input dialog for text entry
pub struct InputDialog<'a> {
    pub title: &'a str,
    pub prompt: &'a str,
    pub value: &'a str,
    pub cursor_position: usize,
}

impl<'a> InputDialog<'a> {
    pub fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_width = 50.min(area.width.saturating_sub(4));
        let dialog_height = 7.min(area.height.saturating_sub(4));

        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        Clear.render(dialog_area, buf);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(2), Constraint::Min(0)])
            .split(inner);

        // Prompt
        let prompt = Paragraph::new(self.prompt)
            .style(Style::default().fg(Color::Gray));
        prompt.render(chunks[0], buf);

        // Input field
        let input = Paragraph::new(self.value)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            );
        input.render(chunks[1], buf);

        // Instructions
        let instructions = Paragraph::new("[Enter] Confirm  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        instructions.render(chunks[2], buf);
    }
}

/// Menu dialog for selecting options
pub struct MenuDialog<'a> {
    pub title: &'a str,
    pub items: &'a [&'a str],
    pub selected: usize,
}

impl<'a> MenuDialog<'a> {
    pub fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_width = 40.min(area.width.saturating_sub(4));
        let dialog_height = (self.items.len() as u16 + 4).min(area.height.saturating_sub(4));

        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        Clear.render(dialog_area, buf);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Render menu items
        let items: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let prefix = if i == self.selected { "> " } else { "  " };
                let style = if i == self.selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::styled(format!("{}{}", prefix, item), style)
            })
            .collect();

        let menu = Paragraph::new(items);
        menu.render(inner, buf);
    }
}

/// Helper to create a centered rectangle
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
