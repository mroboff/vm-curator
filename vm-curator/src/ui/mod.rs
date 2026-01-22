pub mod screens;
pub mod widgets;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::prelude::*;
use ratatui::backend::CrosstermBackend;
use std::io::Stdout;

use crate::app::{App, ConfirmAction, InputMode, Screen};
use crate::vm::{launch_vm_sync, BootMode};

/// Run the TUI application
pub fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| render(app, frame))?;

        // Block until an event is available (responsive input)
        match event::read()? {
            Event::Key(key) => handle_key(app, key)?,
            Event::Mouse(mouse) => handle_mouse(app, mouse)?,
            _ => {}
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Handle mouse input
fn handle_mouse(app: &mut App, mouse: MouseEvent) -> Result<()> {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            if app.screen == Screen::MainMenu {
                app.select_prev();
            }
        }
        MouseEventKind::ScrollDown => {
            if app.screen == Screen::MainMenu {
                app.select_next();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Render the current screen
fn render(app: &App, frame: &mut Frame) {
    match &app.screen {
        Screen::MainMenu => screens::main_menu::render(app, frame),
        Screen::Management => screens::management::render(app, frame),
        Screen::Configuration => screens::configuration::render(app, frame),
        Screen::DetailedInfo => {
            screens::main_menu::render(app, frame);
            render_detailed_info(app, frame);
        }
        Screen::Snapshots => screens::management::render_snapshots(app, frame),
        Screen::BootOptions => screens::management::render_boot_options(app, frame),
        Screen::UsbDevices => render_usb_devices(app, frame),
        Screen::Confirm(action) => {
            screens::main_menu::render(app, frame);
            render_confirm(app, action, frame);
        }
        Screen::Help => {
            screens::main_menu::render(app, frame);
            screens::help::render(frame);
        }
        Screen::Search => {
            screens::main_menu::render(app, frame);
            render_search(app, frame);
        }
    }
}

/// Handle key input
fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Global quit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return Ok(());
    }

    match &app.screen {
        Screen::MainMenu => handle_main_menu(app, key)?,
        Screen::Management => handle_management(app, key)?,
        Screen::Configuration => handle_configuration(app, key)?,
        Screen::DetailedInfo => handle_detailed_info(app, key)?,
        Screen::Snapshots => handle_snapshots(app, key)?,
        Screen::BootOptions => handle_boot_options(app, key)?,
        Screen::UsbDevices => handle_usb_devices(app, key)?,
        Screen::Confirm(action) => handle_confirm(app, action.clone(), key)?,
        Screen::Help => handle_help(app, key)?,
        Screen::Search => handle_search(app, key)?,
    }

    Ok(())
}

fn handle_main_menu(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
        KeyCode::Enter => {
            if app.selected_vm().is_some() {
                app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
            }
        }
        KeyCode::Char('m') => {
            if app.selected_vm().is_some() {
                app.push_screen(Screen::Management);
            }
        }
        KeyCode::Char('c') => {
            if app.selected_vm().is_some() {
                app.push_screen(Screen::Configuration);
            }
        }
        KeyCode::Char('i') => {
            if app.selected_vm().is_some() {
                app.push_screen(Screen::DetailedInfo);
            }
        }
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Editing;
            app.push_screen(Screen::Search);
        }
        KeyCode::Char('?') => app.push_screen(Screen::Help),
        _ => {}
    }
    Ok(())
}

fn handle_management(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => app.menu_next(screens::management::MENU_ITEMS.len()),
        KeyCode::Char('k') | KeyCode::Up => app.menu_prev(),
        KeyCode::Enter | KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            let item = match key.code {
                KeyCode::Char('1') => 0,
                KeyCode::Char('2') => 1,
                KeyCode::Char('3') => 2,
                KeyCode::Char('4') => 3,
                _ => app.selected_menu_item,
            };

            match item {
                0 => {
                    app.selected_menu_item = 0;
                    app.push_screen(Screen::BootOptions);
                }
                1 => {
                    app.load_snapshots()?;
                    app.push_screen(Screen::Snapshots);
                }
                2 => app.push_screen(Screen::Confirm(ConfirmAction::ResetVm)),
                3 => app.push_screen(Screen::Confirm(ConfirmAction::DeleteVm)),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_configuration(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('r') => {
            // Toggle raw script view - could add a state for this
        }
        _ => {}
    }
    Ok(())
}

fn handle_detailed_info(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        _ => {}
    }
    Ok(())
}

fn handle_snapshots(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => {
            if app.selected_snapshot < app.snapshots.len().saturating_sub(1) {
                app.selected_snapshot += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_snapshot > 0 {
                app.selected_snapshot -= 1;
            }
        }
        KeyCode::Char('c') => {
            // Create snapshot - would need input dialog for name
            if let Some(vm) = app.selected_vm() {
                if let Some(disk) = vm.config.primary_disk() {
                    let name = format!("snapshot-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                    if let Err(e) = crate::vm::create_snapshot(&disk.path, &name) {
                        app.set_status(format!("Error: {}", e));
                    } else {
                        app.set_status(format!("Created snapshot: {}", name));
                        app.load_snapshots()?;
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(snap) = app.snapshots.get(app.selected_snapshot) {
                app.push_screen(Screen::Confirm(ConfirmAction::RestoreSnapshot(snap.name.clone())));
            }
        }
        KeyCode::Char('d') => {
            if let Some(snap) = app.snapshots.get(app.selected_snapshot) {
                app.push_screen(Screen::Confirm(ConfirmAction::DeleteSnapshot(snap.name.clone())));
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_boot_options(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => app.menu_next(3),
        KeyCode::Char('k') | KeyCode::Up => app.menu_prev(),
        KeyCode::Enter | KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') => {
            let item = match key.code {
                KeyCode::Char('1') => 0,
                KeyCode::Char('2') => 1,
                KeyCode::Char('3') => 2,
                _ => app.selected_menu_item,
            };

            app.boot_mode = match item {
                0 => BootMode::Normal,
                1 => BootMode::Install,
                2 => {
                    // Would need file picker for custom ISO
                    BootMode::Normal
                }
                _ => BootMode::Normal,
            };

            app.pop_screen();
            app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
        }
        _ => {}
    }
    Ok(())
}

fn handle_usb_devices(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => {
            app.selected_menu_item = (app.selected_menu_item + 1).min(app.usb_devices.len().saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_menu_item > 0 {
                app.selected_menu_item -= 1;
            }
        }
        KeyCode::Char(' ') | KeyCode::Enter => {
            app.toggle_usb_device(app.selected_menu_item);
        }
        _ => {}
    }
    Ok(())
}

fn handle_confirm(app: &mut App, action: ConfirmAction, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => app.pop_screen(),
        KeyCode::Char('y') | KeyCode::Enter => {
            match action {
                ConfirmAction::LaunchVm => {
                    if let Some(vm) = app.selected_vm().cloned() {
                        let options = app.get_launch_options();
                        if let Err(e) = launch_vm_sync(&vm, &options) {
                            app.set_status(format!("Error: {}", e));
                        } else {
                            app.set_status(format!("Launched: {}", vm.display_name()));
                        }
                    }
                    app.pop_screen();
                }
                ConfirmAction::ResetVm => {
                    if let Some(vm) = app.selected_vm() {
                        if let Err(e) = crate::vm::lifecycle::reset_vm(vm) {
                            app.set_status(format!("Error: {}", e));
                        } else {
                            app.set_status("VM reset to fresh state");
                        }
                    }
                    app.pop_screen();
                    app.pop_screen(); // Back to main menu
                }
                ConfirmAction::DeleteVm => {
                    if let Some(vm) = app.selected_vm().cloned() {
                        if let Err(e) = crate::vm::lifecycle::delete_vm(&vm, false) {
                            app.set_status(format!("Error: {}", e));
                        } else {
                            app.set_status(format!("Deleted: {}", vm.display_name()));
                            app.refresh_vms()?;
                        }
                    }
                    app.pop_screen();
                    app.pop_screen();
                }
                ConfirmAction::RestoreSnapshot(name) => {
                    if let Some(vm) = app.selected_vm() {
                        if let Some(disk) = vm.config.primary_disk() {
                            if let Err(e) = crate::vm::restore_snapshot(&disk.path, &name) {
                                app.set_status(format!("Error: {}", e));
                            } else {
                                app.set_status(format!("Restored snapshot: {}", name));
                            }
                        }
                    }
                    app.pop_screen();
                }
                ConfirmAction::DeleteSnapshot(name) => {
                    if let Some(vm) = app.selected_vm() {
                        if let Some(disk) = vm.config.primary_disk() {
                            if let Err(e) = crate::vm::delete_snapshot(&disk.path, &name) {
                                app.set_status(format!("Error: {}", e));
                            } else {
                                app.set_status(format!("Deleted snapshot: {}", name));
                                app.load_snapshots()?;
                            }
                        }
                    }
                    app.pop_screen();
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_help(app: &mut App, _key: KeyEvent) -> Result<()> {
    // Any key closes help
    app.pop_screen();
    Ok(())
}

fn handle_search(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.search_query.clear();
            app.update_filter();
            app.pop_screen();
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            app.pop_screen();
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.update_filter();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.update_filter();
        }
        _ => {}
    }
    Ok(())
}

fn render_detailed_info(app: &App, frame: &mut Frame) {
    use crate::ui::widgets::DetailedInfoWidget;

    let area = frame.area();
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(ratatui::widgets::Clear, dialog_area);

    let vm_name = app.selected_vm()
        .map(|vm| vm.display_name())
        .unwrap_or_else(|| "Unknown".to_string());

    let os_info = app.selected_vm_info();

    DetailedInfoWidget {
        os_info: os_info.as_ref(),
        vm_name: &vm_name,
    }
    .render(dialog_area, frame.buffer_mut());
}

fn render_confirm(app: &App, action: &ConfirmAction, frame: &mut Frame) {
    use crate::ui::widgets::ConfirmDialog;

    let (title, message) = match action {
        ConfirmAction::LaunchVm => {
            let name = app.selected_vm()
                .map(|vm| vm.display_name())
                .unwrap_or_else(|| "VM".to_string());
            ("Launch VM", format!("Launch {}?", name))
        }
        ConfirmAction::ResetVm => {
            ("Reset VM", "This will reset the VM to its initial state. All changes will be lost. Continue?".to_string())
        }
        ConfirmAction::DeleteVm => {
            let name = app.selected_vm()
                .map(|vm| vm.display_name())
                .unwrap_or_else(|| "VM".to_string());
            ("Delete VM", format!("Delete {}? This will move the VM to trash.", name))
        }
        ConfirmAction::RestoreSnapshot(name) => {
            ("Restore Snapshot", format!("Restore snapshot '{}'? Current state will be lost.", name))
        }
        ConfirmAction::DeleteSnapshot(name) => {
            ("Delete Snapshot", format!("Delete snapshot '{}'? This cannot be undone.", name))
        }
    };

    ConfirmDialog::new(title, &message).render(frame.area(), frame.buffer_mut());
}

fn render_usb_devices(app: &App, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

    let area = frame.area();
    let dialog_width = 55.min(area.width.saturating_sub(4));
    let dialog_height = 16.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" USB Devices ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    if app.usb_devices.is_empty() {
        let msg = ratatui::widgets::Paragraph::new("No USB devices found.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    let items: Vec<ListItem> = app.usb_devices
        .iter()
        .enumerate()
        .map(|(i, device)| {
            let selected = app.selected_usb_devices.contains(&i);
            let checkbox = if selected { "[x]" } else { "[ ]" };
            let style = if i == app.selected_menu_item {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(format!(
                "{} {} ({:04x}:{:04x})",
                checkbox,
                device.display_name(),
                device.vendor_id,
                device.product_id
            ))
            .style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected_menu_item));

    let list = List::new(items)
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, inner, &mut state);
}

fn render_search(app: &App, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let area = frame.area();
    let dialog_width = 40.min(area.width.saturating_sub(4));
    let dialog_height = 5;

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let input = Paragraph::new(format!("/{}", app.search_query))
        .style(Style::default().fg(Color::White));
    frame.render_widget(input, inner);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
