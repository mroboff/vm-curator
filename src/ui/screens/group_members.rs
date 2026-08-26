//! Group Members screen — toggle which VMs belong to the selected group.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 66.min(area.width.saturating_sub(4));
    let dialog_height = 22.min(area.height.saturating_sub(4));
    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let group_name = app
        .selected_group()
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Group".to_string());

    let block = Block::default()
        .title(format!(" {} — Members ", group_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header/intro
            Constraint::Length(1), // Spacer
            Constraint::Min(4),    // VM list
            Constraint::Length(1), // Help
        ])
        .split(h_chunks[1]);

    let intro =
        Paragraph::new("Space toggles membership.").style(Style::default().fg(Color::Yellow));
    frame.render_widget(intro, v_chunks[0]);

    if app.vms.is_empty() {
        let empty = Paragraph::new("No VMs discovered.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty, v_chunks[2]);
    } else {
        let group = app.selected_group();
        let items: Vec<ListItem> = app
            .vms
            .iter()
            .enumerate()
            .map(|(i, vm)| {
                let selected = i == app.group_members_selected;
                let name_style = if selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let is_member = group.map(|g| g.contains(&vm.id)).unwrap_or(false);
                let checkbox = if is_member { "[x] " } else { "[ ] " };
                ListItem::new(Line::from(vec![
                    Span::styled(checkbox, Style::default().fg(Color::Green)),
                    Span::styled(vm.display_name(), name_style),
                ]))
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(app.group_members_selected));
        let list = List::new(items).highlight_symbol("> ");
        frame.render_stateful_widget(list, v_chunks[2], &mut state);
    }

    let help = Paragraph::new("[j/k] Move  [Space] Toggle  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, v_chunks[3]);
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.persist_groups();
            app.pop_screen();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if !app.vms.is_empty() && app.group_members_selected + 1 < app.vms.len() {
                app.group_members_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.group_members_selected > 0 {
                app.group_members_selected -= 1;
            }
        }
        KeyCode::Char(' ') => {
            if let Some(vm) = app.vms.get(app.group_members_selected) {
                let vm_id = vm.id.clone();
                app.toggle_vm_in_selected_group(&vm_id);
            }
        }
        _ => {}
    }
    Ok(())
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
