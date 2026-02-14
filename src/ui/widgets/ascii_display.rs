use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::metadata::OsInfo;

/// ASCII art and info display widget with scrolling support
pub struct AsciiInfoWidget<'a> {
    pub ascii_art: &'a str,
    pub os_info: Option<&'a OsInfo>,
    pub vm_name: &'a str,
    pub scroll: u16,
    pub notes: Option<&'a str>,
}

impl<'a> AsciiInfoWidget<'a> {
    pub fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the area first to prevent stale characters when content changes
        Clear.render(area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        // Add horizontal padding for elegant margins
        let padded = Rect {
            x: inner.x.saturating_add(2),
            y: inner.y.saturating_add(1),
            width: inner.width.saturating_sub(4),
            height: inner.height.saturating_sub(1),
        };

        // Build the full content as a single scrollable text
        let mut lines: Vec<Line> = Vec::new();

        // ASCII art - preserve exact spacing (no trimming)
        for line in self.ascii_art.trim_start_matches('\n').lines() {
            lines.push(Line::styled(line, Style::default().fg(Color::Green)));
        }
        lines.push(Line::from(""));

        // Name and details
        if let Some(info) = self.os_info {
            lines.push(Line::from(vec![
                Span::styled(&info.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(&info.publisher, Style::default().fg(Color::Gray)),
                Span::raw(" | "),
                Span::styled(&info.release_date, Style::default().fg(Color::Gray)),
                Span::raw(" | "),
                Span::styled(&info.architecture, Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(""));

            // Short blurb
            if !info.blurb.short.is_empty() {
                for line in info.blurb.short.lines() {
                    lines.push(Line::styled(line, Style::default().fg(Color::White)));
                }
                lines.push(Line::from(""));
            }

            // Long description
            if !info.blurb.long.is_empty() {
                lines.push(Line::from(Span::styled(
                    "About",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                for line in info.blurb.long.lines() {
                    lines.push(Line::from(line.to_string()));
                }
                lines.push(Line::from(""));
            }

            // Fun facts
            if !info.fun_facts.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Fun Facts",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                for fact in &info.fun_facts {
                    lines.push(Line::from(format!("• {}", fact)));
                }
            }

            // User notes
            if let Some(notes) = self.notes {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Notes",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                for line in notes.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        } else {
            // Just show the VM name
            lines.push(Line::from(Span::styled(
                self.vm_name,
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));

            // User notes (even without OS info)
            if let Some(notes) = self.notes {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Notes",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                for line in notes.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        // Don't use trim: true as it breaks ASCII art spacing
        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        para.render(padded, buf);
    }
}

/// Detailed info display (for the info screen)
pub struct DetailedInfoWidget<'a> {
    pub os_info: Option<&'a OsInfo>,
    pub vm_name: &'a str,
}

impl<'a> DetailedInfoWidget<'a> {
    pub fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(format!(" {} - Details ", self.vm_name))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if let Some(info) = self.os_info {
            let mut text = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&info.name),
                ]),
                Line::from(vec![
                    Span::styled("Publisher: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&info.publisher),
                ]),
                Line::from(vec![
                    Span::styled("Released: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&info.release_date),
                ]),
                Line::from(vec![
                    Span::styled("Architecture: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&info.architecture),
                ]),
                Line::from(""),
            ];

            // Add long description
            if !info.blurb.long.is_empty() {
                text.push(Line::from(Span::styled(
                    "About",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                for line in info.blurb.long.lines() {
                    text.push(Line::from(line.to_string()));
                }
                text.push(Line::from(""));
            }

            // Add fun facts
            if !info.fun_facts.is_empty() {
                text.push(Line::from(Span::styled(
                    "Fun Facts",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                for fact in &info.fun_facts {
                    text.push(Line::from(format!("• {}", fact)));
                }
            }

            let para = Paragraph::new(text)
                .wrap(Wrap { trim: true });
            para.render(inner, buf);
        } else {
            let text = Paragraph::new("No detailed information available for this VM.")
                .style(Style::default().fg(Color::Gray));
            text.render(inner, buf);
        }
    }
}
