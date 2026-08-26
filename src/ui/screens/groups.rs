//! VM Groups screen.
//!
//! Lets the user create, rename, delete, and reorder freeform VM groups, and
//! open the Group Members screen to manage which VMs belong to each one.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::{App, ConfirmAction, Screen, TextInputContext};
use crate::ui::widgets::wrapped_line_count;

const INTRO_TEXT: &str = "Freeform groups of VMs, seeded from each VM's OS category. \
    Once any exist, they replace the automatic OS-family list on the main menu, in the order shown below.";
// Non-breaking spaces (\u{a0}) glue each [key] to its label so a wrap can
// only land between hints, never split a key from its own label.
const HELP_TEXT: &str = "[c]\u{a0}Create  [r]\u{a0}Rename  [d]\u{a0}Delete  \
    [Enter]\u{a0}Manage\u{a0}VMs  [Shift+J/K]\u{a0}Reorder  [Esc]\u{a0}Back";

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 66.min(area.width.saturating_sub(4));
    let dialog_height = 22.min(area.height.saturating_sub(4));
    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(format!(" Groups ({} defined) ", app.groups.len()))
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

    // Intro and help text wrap to however many lines the dialog's width
    // needs, instead of being silently clipped on narrower terminals.
    let text_width = h_chunks[1].width;
    let intro_lines = wrapped_line_count(INTRO_TEXT, text_width);
    let help_lines = wrapped_line_count(HELP_TEXT, text_width);

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(intro_lines), // Header/intro
            Constraint::Length(1),           // Spacer
            Constraint::Min(4),              // Group list
            Constraint::Length(help_lines),  // Help
        ])
        .split(h_chunks[1]);

    let intro = Paragraph::new(INTRO_TEXT)
        .style(Style::default().fg(Color::Yellow))
        .wrap(Wrap { trim: true });
    frame.render_widget(intro, v_chunks[0]);

    if app.groups.is_empty() {
        let empty = Paragraph::new("No groups defined yet. Press [c] to create one.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty, v_chunks[2]);
    } else {
        let items: Vec<ListItem> = app
            .groups
            .iter()
            .enumerate()
            .map(|(i, group)| {
                let selected = i == app.groups_selected;
                let name_style = if selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let count = group.vm_ids.len();
                let count_label = if count == 1 {
                    "1 VM".to_string()
                } else {
                    format!("{} VMs", count)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:<30}", group.name), name_style),
                    Span::styled(count_label, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(app.groups_selected));
        let list = List::new(items).highlight_symbol("> ");
        frame.render_stateful_widget(list, v_chunks[2], &mut state);
    }

    let help = Paragraph::new(HELP_TEXT)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(help, v_chunks[3]);
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.pop_screen();
        }
        // Shift+J / Shift+K reorder the selected group; plain j/k just navigate.
        KeyCode::Char('J') => {
            app.move_selected_group_down();
        }
        KeyCode::Char('K') => {
            app.move_selected_group_up();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if !app.groups.is_empty() && app.groups_selected + 1 < app.groups.len() {
                app.groups_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.groups_selected > 0 {
                app.groups_selected -= 1;
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            app.text_input_buffer.clear();
            app.push_screen(Screen::TextInput(TextInputContext::CreateGroup));
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            if let Some(group) = app.selected_group() {
                app.text_input_buffer = group.name.clone();
                app.push_screen(Screen::TextInput(TextInputContext::RenameGroup));
            }
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if app.selected_group().is_some() {
                app.push_screen(Screen::Confirm(ConfirmAction::DeleteGroup));
            }
        }
        KeyCode::Enter => {
            if app.selected_group().is_some() {
                app.open_group_members();
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
