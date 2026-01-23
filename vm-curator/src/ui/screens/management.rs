use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::app::App;

/// Management menu items
pub const MENU_ITEMS: &[&str] = &[
    "Boot Options",
    "Snapshots",
    "Reset VM (recreate disk)",
    "Delete VM",
    "View Configuration",
];

/// Render the management menu
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Calculate dialog size
    let dialog_width = 50.min(area.width.saturating_sub(4));
    let dialog_height = 16.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);

    // Clear the background
    frame.render_widget(Clear, dialog_area);

    let vm_name = app.selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "Unknown".to_string());

    let block = Block::default()
        .title(format!(" {} - Management ", vm_name))
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

    // Split content into padding, menu, and help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Min(4),     // Menu items
            Constraint::Length(2),  // Help text
        ])
        .split(h_chunks[1]);

    // Create menu items with descriptions
    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let description = match i {
                0 => "Normal, install, or custom ISO boot",
                1 => "Create, restore, or delete snapshots",
                2 => "Restore VM to fresh state",
                3 => "Permanently remove this VM",
                4 => "View QEMU settings and launch script",
                _ => "",
            };

            let style = if i == app.selected_menu_item {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::styled(format!("[{}] {}", i + 1, item), style),
                Line::styled(
                    format!("    {}", description),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            ListItem::new(content)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected_menu_item));

    let list = List::new(items)
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, chunks[1], &mut state);

    // Help text
    let help = Paragraph::new("[Enter] Select  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[2]);
}

/// Render boot options submenu
pub fn render_boot_options(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 45.min(area.width.saturating_sub(4));
    let dialog_height = 14.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Boot Options ")
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

    // Add top padding
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Min(1),     // Content
        ])
        .split(h_chunks[1]);

    let boot_items = [
        ("Normal boot", "Start the VM normally"),
        ("Install mode", "Boot from installation media"),
        ("Boot with custom ISO", "Select an ISO file to boot"),
    ];

    let items: Vec<ListItem> = boot_items
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let style = if i == app.selected_menu_item {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(vec![
                Line::styled(format!("[{}] {}", i + 1, name), style),
                Line::styled(format!("    {}", desc), Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected_menu_item));

    let list = List::new(items);
    frame.render_stateful_widget(list, v_chunks[1], &mut state);
}

/// Render snapshot management submenu
pub fn render_snapshots(app: &App, frame: &mut Frame) {
    use ratatui::widgets::Wrap;

    let area = frame.area();
    let dialog_width = 55.min(area.width.saturating_sub(4));
    let dialog_height = 18.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let supports_snapshots = app.selected_vm()
        .map(|vm| vm.config.supports_snapshots())
        .unwrap_or(false);

    let title = if supports_snapshots {
        format!(" Snapshots ({}) ", app.snapshots.len())
    } else {
        " Snapshots (not supported) ".to_string()
    };

    let block = Block::default()
        .title(title)
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

    // Add top padding
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Min(1),     // Content
        ])
        .split(h_chunks[1]);

    let content_area = v_chunks[1];

    if !supports_snapshots {
        let msg = Paragraph::new("This VM uses a raw disk image which doesn't support snapshots.\n\nOnly qcow2 format disks support snapshots.")
            .style(Style::default().fg(Color::Yellow))
            .wrap(Wrap { trim: false });
        frame.render_widget(msg, content_area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(4), Constraint::Length(2)])
        .split(content_area);

    // Action buttons
    let actions = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[c]", Style::default().fg(Color::Yellow)),
            Span::raw(" Create new snapshot"),
        ]),
    ]);
    frame.render_widget(actions, chunks[0]);

    // Snapshot list
    if app.snapshots.is_empty() {
        let msg = Paragraph::new("No snapshots yet.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, chunks[1]);
    } else {
        let items: Vec<ListItem> = app.snapshots
            .iter()
            .enumerate()
            .map(|(i, snap)| {
                let style = if i == app.selected_snapshot {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(vec![
                    Line::styled(format!("  {}", snap.name), style),
                    Line::styled(
                        format!("    {} - {}", snap.date, snap.size),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(app.selected_snapshot));

        let list = List::new(items)
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, chunks[1], &mut state);
    }

    // Help
    let help = Paragraph::new("[r] Restore  [d] Delete  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[2]);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
