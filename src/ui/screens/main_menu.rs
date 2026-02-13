use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::ui::widgets::{AsciiInfoWidget, VmListWidget};

/// Render the main menu screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),    // Main content
            Constraint::Length(3),  // Status/help bar
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
    let vm_name = app.selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "No VM selected".to_string());

    let os_info = app.selected_vm_info();
    let ascii_art = app.selected_vm_ascii();

    AsciiInfoWidget {
        ascii_art,
        os_info: os_info.as_ref(),
        vm_name: &vm_name,
        scroll: app.info_scroll,
    }
    .render(main_chunks[1], frame.buffer_mut());

    // Render help bar
    render_help_bar(app, chunks[2], frame);
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

fn render_help_bar(app: &App, area: Rect, frame: &mut Frame) {
    let mut hints = vec![
        Span::styled(" [Enter]", Style::default().fg(Color::Yellow)),
        Span::raw(" Launch "),
        Span::styled(" [x]", Style::default().fg(Color::Yellow)),
        Span::raw(" Stop "),
        Span::styled(" [m]", Style::default().fg(Color::Yellow)),
        Span::raw(" Manage "),
        Span::styled(" [c]", Style::default().fg(Color::Yellow)),
        Span::raw(" Create "),
        Span::styled(" [i]", Style::default().fg(Color::Yellow)),
        Span::raw(" Import "),
        Span::styled(" [s]", Style::default().fg(Color::Yellow)),
        Span::raw(" Settings "),
        Span::styled(" [/]", Style::default().fg(Color::Yellow)),
        Span::raw(" Search "),
        Span::styled(" [?]", Style::default().fg(Color::Yellow)),
        Span::raw(" Help "),
        Span::styled(" [q]", Style::default().fg(Color::Yellow)),
        Span::raw(" Quit "),
    ];

    // Show stopping VM status
    if app.status_message.is_none() {
        if let Some((id, sent_at)) = app.stopping_vms.iter().next() {
            let elapsed = sent_at.elapsed().as_secs();
            let vm_name = app.vms.iter()
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
        hints.push(Span::styled(
            msg.clone(),
            Style::default().fg(Color::Green),
        ));
    }

    let help = Paragraph::new(Line::from(hints))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .alignment(Alignment::Center);

    frame.render_widget(help, area);
}
