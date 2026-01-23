pub mod screens;
pub mod widgets;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::prelude::*;
use ratatui::backend::CrosstermBackend;
use std::io::Stdout;
use std::time::Duration;

use crate::app::{App, BackgroundResult, ConfirmAction, InputMode, Screen, TextInputContext};
use crate::vm::{launch_vm_sync, lifecycle::is_vm_running, BootMode};
use std::thread;

/// Run the TUI application
pub fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| render(app, frame))?;

        // Check for status message expiry
        app.check_status_expiry();

        // Check for background operation results
        app.check_background_results();

        // Poll with timeout to allow periodic checks
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    // Ignore input while loading
                    if !app.loading {
                        handle_key(app, key)?;
                    }
                }
                Event::Mouse(mouse) => {
                    if !app.loading {
                        handle_mouse(app, mouse)?;
                    }
                }
                _ => {}
            }
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
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            if app.screen == Screen::MainMenu {
                // Calculate VM list area using same layout as render
                // This matches the layout in screens/main_menu.rs
                if let Ok((term_width, term_height)) = crossterm::terminal::size() {
                    // Main layout: title (3), content (rest), help (3)
                    let title_height = 3u16;
                    let help_height = 3u16;
                    let content_y = title_height;
                    let content_height = term_height.saturating_sub(title_height + help_height);

                    // VM list is 40% of content width on the left
                    let list_width = (term_width * 40) / 100;

                    // List area with borders: inner area starts at +1 from each edge
                    let list_inner_x = 1u16;
                    let list_inner_y = content_y + 1; // +1 for block border
                    let list_inner_width = list_width.saturating_sub(2);
                    let list_inner_height = content_height.saturating_sub(2);

                    // Check if click is within the list inner area
                    let click_x = mouse.column;
                    let click_y = mouse.row;

                    if click_x >= list_inner_x
                        && click_x < list_inner_x + list_inner_width
                        && click_y >= list_inner_y
                        && click_y < list_inner_y + list_inner_height
                    {
                        // Calculate which row was clicked
                        let clicked_row = (click_y - list_inner_y) as usize;

                        // Map clicked row to visual_order index (accounting for header rows)
                        if let Some(visual_idx) = widgets::click_row_to_visual_index(
                            &app.vms,
                            &app.filtered_indices,
                            &app.hierarchy,
                            &app.metadata,
                            &app.visual_order,
                            clicked_row,
                        ) {
                            app.selected_vm = visual_idx;
                            app.info_scroll = 0; // Reset scroll when VM changes
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Render a dimming overlay over the entire screen
/// Uses a dark background that the popup's Clear widget will cut through
fn render_dim_overlay(_frame: &mut Frame) {
    // Dimming disabled - causing rendering issues
    // The popup's Clear widget and borders provide sufficient contrast
}

/// Render the current screen
fn render(app: &App, frame: &mut Frame) {
    match &app.screen {
        Screen::MainMenu => screens::main_menu::render(app, frame),
        Screen::Management => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::management::render(app, frame);
        }
        Screen::Configuration => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::configuration::render(app, frame);
        }
        Screen::RawScript => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::configuration::render_raw_script(app, frame);
        }
        Screen::DetailedInfo => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_detailed_info(app, frame);
        }
        Screen::Snapshots => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::management::render_snapshots(app, frame);
        }
        Screen::BootOptions => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::management::render_boot_options(app, frame);
        }
        Screen::UsbDevices => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_usb_devices(app, frame);
        }
        Screen::Confirm(action) => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_confirm(app, action, frame);
        }
        Screen::Help => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::help::render(frame);
        }
        Screen::Search => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_search(app, frame);
        }
        Screen::FileBrowser => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_file_browser(app, frame);
        }
        Screen::TextInput(context) => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_text_input(app, context, frame);
        }
        Screen::ErrorDialog => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_error_dialog(app, frame);
        }
    }
}

/// Handle key input
fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Global quit with Ctrl+C
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return Ok(());
    }

    // Global quit with q/Q (except in text input modes where q might be typed)
    if (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('Q'))
        && !matches!(app.screen, Screen::Search | Screen::TextInput(_))
    {
        app.should_quit = true;
        return Ok(());
    }

    match &app.screen {
        Screen::MainMenu => handle_main_menu(app, key)?,
        Screen::Management => handle_management(app, key)?,
        Screen::Configuration => handle_configuration(app, key)?,
        Screen::RawScript => handle_raw_script(app, key)?,
        Screen::DetailedInfo => handle_detailed_info(app, key)?,
        Screen::Snapshots => handle_snapshots(app, key)?,
        Screen::BootOptions => handle_boot_options(app, key)?,
        Screen::UsbDevices => handle_usb_devices(app, key)?,
        Screen::Confirm(action) => handle_confirm(app, action.clone(), key)?,
        Screen::Help => handle_help(app, key)?,
        Screen::Search => handle_search(app, key)?,
        Screen::FileBrowser => handle_file_browser(app, key)?,
        Screen::TextInput(context) => handle_text_input(app, context.clone(), key)?,
        Screen::ErrorDialog => handle_error_dialog(app, key)?,
    }

    Ok(())
}

fn handle_main_menu(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
        KeyCode::PageDown => {
            app.info_scroll = app.info_scroll.saturating_add(5);
        }
        KeyCode::PageUp => {
            app.info_scroll = app.info_scroll.saturating_sub(5);
        }
        KeyCode::Enter => {
            if app.selected_vm().is_some() {
                app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
            }
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            if app.selected_vm().is_some() {
                app.push_screen(Screen::Management);
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
        KeyCode::Enter | KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') | KeyCode::Char('5') => {
            let item = match key.code {
                KeyCode::Char('1') => 0,
                KeyCode::Char('2') => 1,
                KeyCode::Char('3') => 2,
                KeyCode::Char('4') => 3,
                KeyCode::Char('5') => 4,
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
                4 => app.push_screen(Screen::Configuration),
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
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.push_screen(Screen::RawScript);
        }
        _ => {}
    }
    Ok(())
}

fn handle_raw_script(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
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
            // Create snapshot - open text input for name
            if let Some(vm) = app.selected_vm() {
                // Warn if VM is running - snapshot operations on running VMs can cause corruption
                if is_vm_running(vm) {
                    app.set_status("Warning: VM is running. Snapshot may be inconsistent.");
                }
                // Pre-fill with timestamp-based suggestion
                app.text_input_buffer = format!("snapshot-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                app.push_screen(Screen::TextInput(TextInputContext::SnapshotName));
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

            match item {
                0 => {
                    app.boot_mode = BootMode::Normal;
                    app.pop_screen();
                    app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
                }
                1 => {
                    app.boot_mode = BootMode::Install;
                    app.pop_screen();
                    app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
                }
                2 => {
                    // Open file browser for ISO selection
                    app.load_file_browser();
                    app.push_screen(Screen::FileBrowser);
                }
                _ => {}
            }
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
                        // Check if VM is already running to prevent duplicate launches
                        if is_vm_running(&vm) {
                            app.set_status(format!("{} is already running", vm.display_name()));
                        } else {
                            let options = app.get_launch_options();
                            if let Err(e) = launch_vm_sync(&vm, &options) {
                                app.set_status(format!("Error: {}", e));
                            } else {
                                app.set_status(format!("Launched: {}", vm.display_name()));
                            }
                        }
                    }
                    app.pop_screen();
                }
                ConfirmAction::ResetVm => {
                    if let Some(vm) = app.selected_vm() {
                        // Check if VM is running - resetting while running would be dangerous
                        if is_vm_running(vm) {
                            app.set_status("Error: Cannot reset VM while it is running. Please shut down the VM first.");
                        } else if let Err(e) = crate::vm::lifecycle::reset_vm(vm) {
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
                        // Check if VM is running - restoring snapshots on running VMs is dangerous
                        if is_vm_running(vm) {
                            app.set_status("Error: Cannot restore snapshot while VM is running. Please shut down the VM first.");
                        } else if let Some(disk) = vm.config.primary_disk() {
                            // Spawn background thread for snapshot restore
                            let disk_path = disk.path.clone();
                            let snap_name = name.clone();
                            let tx = app.background_tx.clone();
                            app.loading = true;
                            app.set_status(format!("Restoring snapshot: {}...", name));

                            thread::spawn(move || {
                                let result = crate::vm::restore_snapshot(&disk_path, &snap_name);
                                let _ = tx.send(BackgroundResult::SnapshotRestored {
                                    name: snap_name,
                                    success: result.is_ok(),
                                    error: result.err().map(|e| e.to_string()),
                                });
                            });
                        }
                    }
                    app.pop_screen();
                }
                ConfirmAction::DeleteSnapshot(name) => {
                    if let Some(vm) = app.selected_vm() {
                        // Check if VM is running - deleting snapshots on running VMs can cause issues
                        if is_vm_running(vm) {
                            app.set_status("Error: Cannot delete snapshot while VM is running. Please shut down the VM first.");
                        } else if let Some(disk) = vm.config.primary_disk() {
                            // Spawn background thread for snapshot delete
                            let disk_path = disk.path.clone();
                            let snap_name = name.clone();
                            let tx = app.background_tx.clone();
                            app.loading = true;
                            app.set_status(format!("Deleting snapshot: {}...", name));

                            thread::spawn(move || {
                                let result = crate::vm::delete_snapshot(&disk_path, &snap_name);
                                let _ = tx.send(BackgroundResult::SnapshotDeleted {
                                    name: snap_name,
                                    success: result.is_ok(),
                                    error: result.err().map(|e| e.to_string()),
                                });
                            });
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

fn render_file_browser(app: &App, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
    use ratatui::layout::{Layout, Direction, Constraint};

    let area = frame.area();
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let title = format!(" Select ISO - {} ", app.file_browser_dir.display());
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

    if app.file_browser_entries.is_empty() {
        let msg = ratatui::widgets::Paragraph::new("No ISO files found in this directory.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, content_area);
        return;
    }

    let items: Vec<ListItem> = app.file_browser_entries
        .iter()
        .map(|entry| {
            let prefix = if entry.is_dir { "📁 " } else { "💿 " };
            ListItem::new(format!("{}{}", prefix, entry.name))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.file_browser_selected));

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, content_area, &mut state);
}

fn handle_file_browser(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => app.file_browser_next(),
        KeyCode::Char('k') | KeyCode::Up => app.file_browser_prev(),
        KeyCode::Enter => {
            if let Some(iso_path) = app.file_browser_enter() {
                // Selected an ISO file
                app.boot_mode = BootMode::Cdrom(iso_path);
                app.pop_screen(); // Close file browser
                app.pop_screen(); // Close boot options
                app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
            }
            // If it was a directory, file_browser_enter already navigated
        }
        _ => {}
    }
    Ok(())
}

fn render_text_input(app: &App, context: &TextInputContext, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let title = match context {
        TextInputContext::SnapshotName => " Enter Snapshot Name ",
    };

    let area = frame.area();
    let dialog_width = 50.min(area.width.saturating_sub(4));
    let dialog_height = 5;

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let input = Paragraph::new(format!("{}_", app.text_input_buffer))
        .style(Style::default().fg(Color::White));
    frame.render_widget(input, inner);
}

fn handle_text_input(app: &mut App, context: TextInputContext, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.text_input_buffer.clear();
            app.pop_screen();
        }
        KeyCode::Enter => {
            let input = app.text_input_buffer.clone();
            app.text_input_buffer.clear();
            app.pop_screen();

            match context {
                TextInputContext::SnapshotName => {
                    if !input.is_empty() {
                        if let Some(vm) = app.selected_vm() {
                            if let Some(disk) = vm.config.primary_disk() {
                                // Spawn background thread for snapshot creation
                                let disk_path = disk.path.clone();
                                let name = input.clone();
                                let tx = app.background_tx.clone();
                                app.loading = true;
                                app.set_status(format!("Creating snapshot: {}...", name));

                                thread::spawn(move || {
                                    let result = crate::vm::create_snapshot(&disk_path, &name);
                                    let _ = tx.send(BackgroundResult::SnapshotCreated {
                                        name,
                                        success: result.is_ok(),
                                        error: result.err().map(|e| e.to_string()),
                                    });
                                });
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Backspace => {
            app.text_input_buffer.pop();
        }
        KeyCode::Char(c) => {
            // Only allow safe characters for snapshot names
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                app.text_input_buffer.push(c);
            }
        }
        _ => {}
    }
    Ok(())
}

fn render_error_dialog(app: &App, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

    let area = frame.area();
    let dialog_width = 70.min(area.width.saturating_sub(4));
    let dialog_height = 15.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Error Details (j/k to scroll, Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let error_text = app.error_detail.as_deref().unwrap_or("No error details");
    let paragraph = Paragraph::new(error_text)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false })
        .scroll((app.error_scroll, 0));
    frame.render_widget(paragraph, inner);
}

fn handle_error_dialog(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.error_detail = None;
            app.error_scroll = 0;
            app.pop_screen();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.error_scroll = app.error_scroll.saturating_add(1);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.error_scroll = app.error_scroll.saturating_sub(1);
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
