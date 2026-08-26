use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::App;
use crate::ui::widgets::{AsciiInfoWidget, VmListWidget, NBSP};

/// Render the main menu screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // The help bar wraps onto as many lines as the terminal width demands,
    // so measure it against this frame's width before laying out the screen.
    let hints = build_help_hints(app);
    let help_height = hints_line_count(&hints, area.width.saturating_sub(2)) + 2;

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),           // Title
            Constraint::Min(10),             // Main content
            Constraint::Length(help_height), // Status/help bar
        ])
        .split(area);

    // Render title
    render_title(app, chunks[0], frame);

    // Split main content: VM list on left, info on right
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    // Render VM list
    VmListWidget::new(app).render(main_chunks[0], frame.buffer_mut());

    // Render ASCII art and info
    let vm_name = app
        .selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "No VM selected".to_string());

    let os_info = app.selected_vm_info();
    let ascii_art = app.selected_vm_ascii();

    let notes = app.selected_vm().and_then(|vm| vm.notes.as_deref());

    AsciiInfoWidget {
        ascii_art,
        os_info: os_info.as_ref(),
        vm_name: &vm_name,
        scroll: app.info_scroll,
        notes,
    }
    .render(main_chunks[1], frame.buffer_mut());

    // Render help bar
    render_help_bar(hints, chunks[2], frame);
}

fn render_title(app: &App, area: Rect, frame: &mut Frame) {
    // Format the library path, shortening home directory to ~
    let library_path = &app.config.vm_library_path;
    let display_path = if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = library_path.strip_prefix(&home) {
            format!("~/{}", stripped.display())
        } else {
            library_path.display().to_string()
        }
    } else {
        library_path.display().to_string()
    };

    let title = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            " VM Curator ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("(QEMU VM Library in {})", display_path),
            Style::default().fg(Color::Gray),
        ),
    ])])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .alignment(Alignment::Center);

    frame.render_widget(title, area);
}

/// Height (including top/bottom borders) the help bar will render at for the
/// given terminal width. Exposed so mouse-click hit testing in `ui::mod` can
/// locate the VM list area without duplicating the hint/wrap layout.
pub fn help_bar_height(app: &App, width: u16) -> u16 {
    hints_line_count(&build_help_hints(app), width.saturating_sub(2)) + 2
}

/// Word-wrapped line count for a list of hint spans (concatenates their text
/// and delegates to the shared wrap estimator).
fn hints_line_count(spans: &[Span], width: u16) -> u16 {
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    crate::ui::widgets::wrapped_line_count(&text, width)
}

/// Build the help bar's hint spans, or a status/stopping-VM message that
/// overrides them. Owned (`'static`) so it can be measured for wrapping and
/// rendered without borrowing from `app` across the layout split.
fn build_help_hints(app: &App) -> Vec<Span<'static>> {
    // A non-breaking space joins each [key] to its label so a wrap can only
    // land in the gap *between* hints, never split a key from its own label
    // (see ui::widgets::text::NBSP).
    let mut hints = vec![
        Span::styled(" [Enter]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Launch ")),
        Span::styled(" [x]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Stop ")),
        Span::styled(" [m]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Manage ")),
        Span::styled(" [c]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Create ")),
        Span::styled(" [i]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Import ")),
        Span::styled(" [s]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Settings ")),
        Span::styled(" [n]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Networks ")),
        Span::styled(" [g]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Groups ")),
        Span::styled(" [/]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Search ")),
        Span::styled(" [?]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Help ")),
        Span::styled(" [q]", Style::default().fg(Color::Yellow)),
        Span::raw(format!("{NBSP}Quit ")),
    ];

    // Show stopping VM status
    if app.status_message.is_none() {
        if let Some((id, sent_at)) = app.stopping_vms.iter().next() {
            let elapsed = sent_at.elapsed().as_secs();
            let vm_name = app
                .vms
                .iter()
                .find(|vm| &vm.id == id)
                .map(|vm| vm.display_name())
                .unwrap_or_else(|| id.clone());
            hints.clear();
            if elapsed >= 10 {
                hints.push(Span::styled(
                    format!("Stopping {}... (press x to force stop)", vm_name),
                    Style::default().fg(Color::Yellow),
                ));
            } else {
                hints.push(Span::styled(
                    format!("Stopping {}...", vm_name),
                    Style::default().fg(Color::Yellow),
                ));
            }
        }
    }

    // Add status message if present (overrides everything)
    if let Some(ref msg) = app.status_message {
        hints.clear();
        hints.push(Span::styled(msg.clone(), Style::default().fg(Color::Green)));
    }

    hints
}

fn render_help_bar(hints: Vec<Span<'static>>, area: Rect, frame: &mut Frame) {
    let help = Paragraph::new(Line::from(hints))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);

    frame.render_widget(help, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_line_count_concatenates_spans_before_wrapping() {
        let spans = vec![
            Span::raw("[Enter] Launch  "),
            Span::raw("[x] Stop  "),
            Span::raw("[m] Manage"),
        ];
        assert_eq!(hints_line_count(&spans, 200), 1);
        let narrow = hints_line_count(&spans, 10);
        assert!(
            narrow > 1,
            "expected wrapping at width 10, got {narrow} line(s)"
        );
    }
}
