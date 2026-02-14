pub mod screens;
pub mod widgets;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::prelude::*;
use ratatui::backend::CrosstermBackend;
use regex::Regex;
use std::io::Stdout;
use std::time::{Duration, Instant};

use crate::app::{App, BackgroundResult, ConfirmAction, InputMode, Screen, TextInputContext};
use crate::vm::{launch_vm_with_error_check, BootMode};
use std::thread;

/// Run the TUI application
pub fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| render(app, frame))?;

        // Check for status message expiry
        app.check_status_expiry();

        // Check for background operation results
        app.check_background_results();

        // Check for VM status updates from background thread
        app.check_vm_status();

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
            match &app.screen {
                Screen::MainMenu => {
                    handle_main_menu_click(app, mouse.column, mouse.row)?;
                }
                Screen::Confirm(action) => {
                    handle_confirm_click(app, action.clone(), mouse.column, mouse.row)?;
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle mouse click in the main menu
fn handle_main_menu_click(app: &mut App, click_x: u16, click_y: u16) -> Result<()> {
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
                // If clicking on already-selected VM, show launch confirmation
                if visual_idx == app.selected_vm && app.selected_vm().is_some() {
                    app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
                } else {
                    // Otherwise, just select the VM
                    app.selected_vm = visual_idx;
                    app.info_scroll = 0; // Reset scroll when VM changes
                }
            }
        }
    }
    Ok(())
}

/// Handle mouse click in the confirmation dialog
fn handle_confirm_click(app: &mut App, action: ConfirmAction, click_x: u16, click_y: u16) -> Result<()> {
    if let Ok((term_width, term_height)) = crossterm::terminal::size() {
        let area = Rect::new(0, 0, term_width, term_height);

        // Calculate dialog dimensions (same as ConfirmDialog::render)
        let dialog_width = 50.min(area.width.saturating_sub(4));
        let dialog_height = 8.min(area.height.saturating_sub(4));

        // Calculate centered position
        let dialog_x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

        // Inner area after borders (1 pixel each side)
        let inner_x = dialog_x + 1;
        let inner_y = dialog_y + 1;
        let inner_width = dialog_width.saturating_sub(2);
        let inner_height = dialog_height.saturating_sub(2);

        // Buttons are in the bottom 2 rows of the inner area
        let buttons_y = inner_y + inner_height.saturating_sub(2);

        // Check if click is in the buttons row
        if click_y >= buttons_y && click_y < buttons_y + 2 {
            // Buttons are centered: " Yes (y) "  "  "  " No (n) "
            // Yes button is roughly in the left half, No in the right half
            let center_x = inner_x + inner_width / 2;

            if click_x >= inner_x && click_x < center_x {
                // Clicked on Yes - execute the action
                execute_confirm_action(app, action)?;
            } else if click_x >= center_x && click_x < inner_x + inner_width {
                // Clicked on No - cancel
                app.pop_screen();
            }
        }

        // Allow clicking outside the dialog to cancel
        if click_x < dialog_x || click_x >= dialog_x + dialog_width
            || click_y < dialog_y || click_y >= dialog_y + dialog_height
        {
            app.pop_screen();
        }
    }
    Ok(())
}

/// Execute a confirmed action (extracted from handle_confirm for reuse)
fn execute_confirm_action(app: &mut App, action: ConfirmAction) -> Result<()> {
    match action {
        ConfirmAction::LaunchVm => {
            // Pop the confirm dialog first
            app.pop_screen();

            if let Some(vm) = app.selected_vm().cloned() {
                if app.running_vms.contains_key(&vm.id) {
                    app.set_status(format!("{} is already running", vm.display_name()));
                } else {
                    let options = app.get_launch_options();
                    let result = launch_vm_with_error_check(&vm, &options);

                    if result.success {
                        app.set_status(format!("Launched: {}", result.vm_name));
                    } else {
                        // Show error in the error dialog for better visibility
                        let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
                        app.show_error(format!(
                            "Failed to launch {}\n\n{}",
                            result.vm_name, error_msg
                        ));
                    }
                }
            }
        }
        ConfirmAction::ResetVm => {
            if let Some(vm) = app.selected_vm() {
                if app.running_vms.contains_key(&vm.id) {
                    app.set_status("Error: Cannot reset VM while it is running. Please shut down the VM first.");
                } else if let Err(e) = crate::vm::lifecycle::reset_vm(vm) {
                    app.set_status(format!("Error: {}", e));
                } else {
                    app.set_status("VM reset to fresh state");
                }
            }
            app.pop_screen();
            app.pop_screen();
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
                if app.running_vms.contains_key(&vm.id) {
                    app.set_status("Error: Cannot restore snapshot while VM is running. Please shut down the VM first.");
                } else if let Some(disk) = vm.config.primary_disk() {
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
                if app.running_vms.contains_key(&vm.id) {
                    app.set_status("Error: Cannot delete snapshot while VM is running. Please shut down the VM first.");
                } else if let Some(disk) = vm.config.primary_disk() {
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
        ConfirmAction::DiscardScriptChanges => {
            // Discard changes and exit editor
            app.raw_script_scroll = 0;
            app.script_editor_lines.clear();
            app.script_editor_modified = false;
            app.pop_screen(); // Close confirm dialog
            app.pop_screen(); // Close editor
        }
        ConfirmAction::DiscardNotesChanges => {
            // Discard changes and exit notes editor
            app.raw_script_scroll = 0;
            app.script_editor_lines.clear();
            app.script_editor_modified = false;
            app.pop_screen(); // Close confirm dialog
            app.pop_screen(); // Close editor
        }
        ConfirmAction::StopVm => {
            app.pop_screen();
            if let Some(vm) = app.selected_vm().cloned() {
                if let Some(pid) = app.running_vms.get(&vm.id).copied() {
                    match crate::vm::stop_vm_by_pid(pid) {
                        Ok(()) => {
                            app.stopping_vms.insert(vm.id.clone(), Instant::now());
                            app.set_status(format!("Stopping {}...", vm.display_name()));
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to stop {}: {}", vm.display_name(), e));
                        }
                    }
                } else {
                    app.set_status(format!("{} is not running", vm.display_name()));
                }
            }
        }
        ConfirmAction::ForceStopVm => {
            app.pop_screen();
            if let Some(vm) = app.selected_vm().cloned() {
                if let Some(pid) = app.running_vms.get(&vm.id).copied() {
                    match crate::vm::force_stop_vm(pid) {
                        Ok(()) => {
                            app.stopping_vms.remove(&vm.id);
                            app.set_status(format!("Force stopped {}", vm.display_name()));
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to force stop {}: {}", vm.display_name(), e));
                        }
                    }
                }
            }
        }
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
        Screen::EditNotes => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::configuration::render_edit_notes(app, frame);
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
        Screen::DisplayOptions => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::management::render_display_options(app, frame);
        }
        Screen::UsbDevices => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            render_usb_devices(app, frame);
        }
        Screen::PciPassthrough => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::pci_passthrough::render(app, frame);
        }
        Screen::SharedFolders => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::shared_folders::render(app, frame);
        }
        Screen::SingleGpuSetup => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::single_gpu_setup::render(app, frame);
        }
        Screen::SingleGpuInstructions => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::single_gpu_setup::render_instructions(app, frame);
        }
        Screen::MultiGpuSetup => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::multi_gpu_setup::render(app, frame);
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
        Screen::CreateWizard => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::create_wizard::render(app, frame);
        }
        Screen::CreateWizardCustomOs => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::create_wizard::render_custom_os(app, frame);
        }
        Screen::CreateWizardDownload => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::create_wizard::render_download(app, frame);
        }
        Screen::NetworkSettings => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::network_settings::render(app, frame);
        }
        Screen::Settings => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::settings::render(app, frame);
        }
        Screen::ImportWizard => {
            screens::main_menu::render(app, frame);
            render_dim_overlay(frame);
            screens::import_wizard::render(app, frame);
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
        && !matches!(app.screen, Screen::Search | Screen::TextInput(_) | Screen::RawScript | Screen::EditNotes | Screen::CreateWizard | Screen::CreateWizardCustomOs | Screen::NetworkSettings | Screen::ImportWizard)
    {
        app.should_quit = true;
        return Ok(());
    }

    match &app.screen {
        Screen::MainMenu => handle_main_menu(app, key)?,
        Screen::Management => handle_management(app, key)?,
        Screen::Configuration => handle_configuration(app, key)?,
        Screen::RawScript => handle_raw_script(app, key)?,
        Screen::EditNotes => handle_edit_notes(app, key)?,
        Screen::DetailedInfo => handle_detailed_info(app, key)?,
        Screen::Snapshots => handle_snapshots(app, key)?,
        Screen::BootOptions => handle_boot_options(app, key)?,
        Screen::DisplayOptions => handle_display_options(app, key)?,
        Screen::UsbDevices => handle_usb_devices(app, key)?,
        Screen::PciPassthrough => screens::pci_passthrough::handle_key(app, key)?,
        Screen::SharedFolders => screens::shared_folders::handle_key(app, key)?,
        Screen::SingleGpuSetup => screens::single_gpu_setup::handle_key(app, key)?,
        Screen::SingleGpuInstructions => handle_single_gpu_instructions(app, key)?,
        Screen::MultiGpuSetup => screens::multi_gpu_setup::handle_input(app, key)?,
        Screen::Confirm(action) => handle_confirm(app, action.clone(), key)?,
        Screen::Help => handle_help(app, key)?,
        Screen::Search => handle_search(app, key)?,
        Screen::FileBrowser => handle_file_browser(app, key)?,
        Screen::TextInput(context) => handle_text_input(app, context.clone(), key)?,
        Screen::ErrorDialog => handle_error_dialog(app, key)?,
        Screen::CreateWizard => screens::create_wizard::handle_key(app, key)?,
        Screen::CreateWizardCustomOs => screens::create_wizard::handle_custom_os_key(app, key)?,
        Screen::CreateWizardDownload => screens::create_wizard::handle_download_key(app, key)?,
        Screen::NetworkSettings => screens::network_settings::handle_key(app, key)?,
        Screen::Settings => { screens::settings::handle_input(app, key)?; }
        Screen::ImportWizard => screens::import_wizard::handle_key(app, key)?,
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
                if app.config.confirm_before_launch {
                    app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
                } else {
                    // Launch directly without confirmation
                    execute_confirm_action(app, ConfirmAction::LaunchVm)?;
                }
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
        KeyCode::Char('c') | KeyCode::Char('C') => {
            app.start_create_wizard();
        }
        KeyCode::Char('i') | KeyCode::Char('I') => {
            app.start_import_wizard();
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            app.push_screen(Screen::Settings);
        }
        KeyCode::Char('x') | KeyCode::Char('X') => {
            if let Some(vm) = app.selected_vm().cloned() {
                if app.selected_vm_pid().is_some() {
                    if let Some(sent_at) = app.stopping_vms.get(&vm.id) {
                        if sent_at.elapsed() > Duration::from_secs(10) {
                            app.push_screen(Screen::Confirm(ConfirmAction::ForceStopVm));
                        } else {
                            app.set_status(format!(
                                "Waiting for {} to shut down... (press x again after 10s to force)",
                                vm.display_name()
                            ));
                        }
                    } else {
                        app.push_screen(Screen::Confirm(ConfirmAction::StopVm));
                    }
                } else {
                    app.set_status("VM is not running");
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_management(app: &mut App, key: KeyEvent) -> Result<()> {
    use screens::management::{get_menu_items, menu_item_count, MenuAction};

    let item_count = menu_item_count(app);

    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => app.menu_next(item_count),
        KeyCode::Char('k') | KeyCode::Up => app.menu_prev(),
        KeyCode::Enter | KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') | KeyCode::Char('5') | KeyCode::Char('6') | KeyCode::Char('7') | KeyCode::Char('8') | KeyCode::Char('9') => {
            // Map number keys to menu index
            let selected_idx = match key.code {
                KeyCode::Char('1') => 0,
                KeyCode::Char('2') => 1,
                KeyCode::Char('3') => 2,
                KeyCode::Char('4') => 3,
                KeyCode::Char('5') => 4,
                KeyCode::Char('6') => 5,
                KeyCode::Char('7') => 6,
                KeyCode::Char('8') => 7,
                KeyCode::Char('9') => 8,
                _ => app.selected_menu_item,
            };

            // Get the menu items and find the action
            if let Some(vm) = app.selected_vm() {
                let menu_items = get_menu_items(vm, &app.config);
                if let Some(item) = menu_items.get(selected_idx) {
                    match item.action {
                        MenuAction::StopVm => {
                            if let Some(vm) = app.selected_vm().cloned() {
                                if app.selected_vm_pid().is_some() {
                                    if let Some(sent_at) = app.stopping_vms.get(&vm.id) {
                                        if sent_at.elapsed() > Duration::from_secs(10) {
                                            app.push_screen(Screen::Confirm(ConfirmAction::ForceStopVm));
                                        } else {
                                            app.set_status(format!(
                                                "Waiting for {} to shut down...",
                                                vm.display_name()
                                            ));
                                        }
                                    } else {
                                        app.push_screen(Screen::Confirm(ConfirmAction::StopVm));
                                    }
                                } else {
                                    app.set_status("VM is not running");
                                }
                            }
                        }
                        MenuAction::BootOptions => {
                            app.selected_menu_item = 0;
                            app.push_screen(Screen::BootOptions);
                        }
                        MenuAction::Snapshots => {
                            app.load_snapshots()?;
                            app.push_screen(Screen::Snapshots);
                        }
                        MenuAction::UsbPassthrough => {
                            app.load_usb_devices()?;
                            // Load saved USB passthrough config and pre-select matching devices
                            if let Some(vm) = app.selected_vm() {
                                let saved = crate::vm::load_usb_passthrough(vm);
                                app.selected_usb_devices.clear();
                                for saved_dev in &saved {
                                    // Find matching device by vendor/product ID
                                    for (i, dev) in app.usb_devices.iter().enumerate() {
                                        if dev.vendor_id == saved_dev.vendor_id
                                            && dev.product_id == saved_dev.product_id
                                        {
                                            app.selected_usb_devices.push(i);
                                            break;
                                        }
                                    }
                                }
                            }
                            app.selected_menu_item = 0;
                            app.push_screen(Screen::UsbDevices);
                        }
                        MenuAction::PciPassthrough => {
                            app.load_pci_devices()?;
                            // Load saved PCI passthrough config and pre-select matching devices
                            if let Some(vm) = app.selected_vm() {
                                let saved_args = crate::vm::load_pci_passthrough(vm);
                                app.selected_pci_devices.clear();
                                for arg in &saved_args {
                                    // Extract address from "-device vfio-pci,host=0000:01:00.0"
                                    if let Some(host_start) = arg.find("host=") {
                                        let addr_start = host_start + 5;
                                        let addr = arg[addr_start..]
                                            .split(|c: char| c == ',' || c.is_whitespace())
                                            .next()
                                            .unwrap_or("");
                                        // Find matching device by address
                                        for (i, dev) in app.pci_devices.iter().enumerate() {
                                            if dev.address == addr {
                                                app.selected_pci_devices.push(i);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            app.selected_menu_item = 0;
                            app.push_screen(Screen::PciPassthrough);
                        }
                        MenuAction::SharedFolders => {
                            app.load_shared_folders();
                            app.selected_menu_item = 0;
                            app.push_screen(Screen::SharedFolders);
                        }
                        MenuAction::NetworkSettings => {
                            // Initialize network settings state from current VM config
                            if let Some(vm) = app.selected_vm() {
                                let net = vm.config.network.as_ref();
                                let model = net.map(|n| n.model.clone()).unwrap_or_else(|| "e1000".to_string());
                                let (backend, bridge_name) = net.map(|n| {
                                    match &n.backend {
                                        crate::vm::qemu_config::NetworkBackend::User => ("user".to_string(), None),
                                        crate::vm::qemu_config::NetworkBackend::Passt => ("passt".to_string(), None),
                                        crate::vm::qemu_config::NetworkBackend::Bridge(name) => ("bridge".to_string(), Some(name.clone())),
                                        crate::vm::qemu_config::NetworkBackend::None => ("none".to_string(), None),
                                    }
                                }).unwrap_or_else(|| ("user".to_string(), None));
                                let port_forwards = net.map(|n| n.port_forwards.clone()).unwrap_or_default();

                                app.network_settings_state = Some(crate::app::NetworkSettingsState {
                                    model,
                                    backend,
                                    bridge_name,
                                    port_forwards,
                                    selected_field: 0,
                                    editing_port_forwards: false,
                                    pf_selected: 0,
                                    adding_pf: None,
                                });
                                app.push_screen(Screen::NetworkSettings);
                            }
                        }
                        MenuAction::MultiGpuPassthrough => {
                            // Load PCI devices for multi-GPU setup
                            app.load_pci_devices()?;
                            app.push_screen(Screen::MultiGpuSetup);
                        }
                        MenuAction::SingleGpuPassthrough => {
                            // Load PCI devices and initialize single GPU config
                            app.load_pci_devices()?;
                            screens::single_gpu_setup::init_single_gpu_config(app);
                            app.single_gpu_selected_field = 0;
                            app.push_screen(Screen::SingleGpuSetup);
                        }
                        MenuAction::ChangeDisplay => {
                            app.selected_menu_item = 0;
                            app.push_screen(Screen::DisplayOptions);
                        }
                        MenuAction::EditNotes => {
                            app.load_notes_into_editor();
                            app.push_screen(Screen::EditNotes);
                        }
                        MenuAction::RenameVm => {
                            if let Some(vm) = app.selected_vm() {
                                app.text_input_buffer = vm.display_name();
                            }
                            app.push_screen(Screen::TextInput(TextInputContext::RenameVm));
                        }
                        MenuAction::ResetVm => {
                            app.push_screen(Screen::Confirm(ConfirmAction::ResetVm));
                        }
                        MenuAction::DeleteVm => {
                            app.push_screen(Screen::Confirm(ConfirmAction::DeleteVm));
                        }
                        MenuAction::EditRawConfig => {
                            app.load_script_into_editor();
                            app.push_screen(Screen::RawScript);
                        }
                    }
                }
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
    let total_lines = app.script_editor_lines.len();

    match (key.code, key.modifiers) {
        // Save with Ctrl+S
        (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => {
            match app.save_script_from_editor() {
                Ok(()) => app.set_status("Script saved successfully"),
                Err(e) => app.set_status(format!("Error saving script: {}", e)),
            }
        }

        // Cancel/Exit with Esc
        (KeyCode::Esc, _) => {
            if app.script_editor_modified {
                // Show confirmation if modified
                app.push_screen(Screen::Confirm(ConfirmAction::DiscardScriptChanges));
            } else {
                app.raw_script_scroll = 0;
                app.script_editor_lines.clear();
                app.pop_screen();
            }
        }

        // Navigation
        (KeyCode::Up, _) => {
            if app.script_editor_cursor.0 > 0 {
                app.script_editor_cursor.0 -= 1;
                // Adjust column if new line is shorter
                let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                    .map(|l| l.len()).unwrap_or(0);
                if app.script_editor_cursor.1 > line_len {
                    app.script_editor_cursor.1 = line_len;
                }
                // Scroll up if cursor goes above visible area
                if app.script_editor_cursor.0 < app.raw_script_scroll as usize {
                    app.raw_script_scroll = app.script_editor_cursor.0 as u16;
                }
            }
        }
        (KeyCode::Down, _) => {
            if app.script_editor_cursor.0 < total_lines.saturating_sub(1) {
                app.script_editor_cursor.0 += 1;
                // Adjust column if new line is shorter
                let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                    .map(|l| l.len()).unwrap_or(0);
                if app.script_editor_cursor.1 > line_len {
                    app.script_editor_cursor.1 = line_len;
                }
                // Scroll down if needed (assuming ~35 visible lines)
                let visible_height = 35usize;
                if app.script_editor_cursor.0 >= app.raw_script_scroll as usize + visible_height {
                    app.raw_script_scroll = (app.script_editor_cursor.0 - visible_height + 1) as u16;
                }
            }
        }
        (KeyCode::Left, _) => {
            if app.script_editor_cursor.1 > 0 {
                app.script_editor_cursor.1 -= 1;
            } else if app.script_editor_cursor.0 > 0 {
                // Move to end of previous line
                app.script_editor_cursor.0 -= 1;
                app.script_editor_cursor.1 = app.script_editor_lines.get(app.script_editor_cursor.0)
                    .map(|l| l.len()).unwrap_or(0);
            }
            // Adjust horizontal scroll
            if app.script_editor_cursor.1 < app.script_editor_h_scroll {
                app.script_editor_h_scroll = app.script_editor_cursor.1;
            }
        }
        (KeyCode::Right, _) => {
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            if app.script_editor_cursor.1 < line_len {
                app.script_editor_cursor.1 += 1;
            } else if app.script_editor_cursor.0 < total_lines.saturating_sub(1) {
                // Move to start of next line
                app.script_editor_cursor.0 += 1;
                app.script_editor_cursor.1 = 0;
            }
            // Adjust horizontal scroll (assuming ~80 visible columns)
            let visible_width = 80usize;
            if app.script_editor_cursor.1 >= app.script_editor_h_scroll + visible_width {
                app.script_editor_h_scroll = app.script_editor_cursor.1 - visible_width + 1;
            }
        }
        (KeyCode::Home, _) => {
            app.script_editor_cursor.1 = 0;
            app.script_editor_h_scroll = 0;
        }
        (KeyCode::End, _) => {
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            app.script_editor_cursor.1 = line_len;
        }
        (KeyCode::PageUp, _) => {
            let jump = 20;
            app.script_editor_cursor.0 = app.script_editor_cursor.0.saturating_sub(jump);
            app.raw_script_scroll = app.raw_script_scroll.saturating_sub(jump as u16);
            // Adjust column
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            if app.script_editor_cursor.1 > line_len {
                app.script_editor_cursor.1 = line_len;
            }
        }
        (KeyCode::PageDown, _) => {
            let jump = 20;
            app.script_editor_cursor.0 = (app.script_editor_cursor.0 + jump).min(total_lines.saturating_sub(1));
            app.raw_script_scroll = (app.raw_script_scroll + jump as u16).min(total_lines.saturating_sub(1) as u16);
            // Adjust column
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            if app.script_editor_cursor.1 > line_len {
                app.script_editor_cursor.1 = line_len;
            }
        }

        // Editing - Enter (new line)
        (KeyCode::Enter, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                let remainder = line[col..].to_string();
                line.truncate(col);
                app.script_editor_lines.insert(line_idx + 1, remainder);
                app.script_editor_cursor = (line_idx + 1, 0);
                app.script_editor_modified = true;
            }
        }

        // Editing - Backspace
        (KeyCode::Backspace, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if col > 0 {
                if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                    line.remove(col - 1);
                    app.script_editor_cursor.1 -= 1;
                    app.script_editor_modified = true;
                }
            } else if line_idx > 0 {
                // Join with previous line
                let current_line = app.script_editor_lines.remove(line_idx);
                if let Some(prev_line) = app.script_editor_lines.get_mut(line_idx - 1) {
                    let prev_len = prev_line.len();
                    prev_line.push_str(&current_line);
                    app.script_editor_cursor = (line_idx - 1, prev_len);
                    app.script_editor_modified = true;
                }
            }
        }

        // Editing - Delete
        (KeyCode::Delete, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                if col < line.len() {
                    line.remove(col);
                    app.script_editor_modified = true;
                } else if line_idx < total_lines - 1 {
                    // Join with next line
                    let next_line = app.script_editor_lines.remove(line_idx + 1);
                    app.script_editor_lines.get_mut(line_idx).unwrap().push_str(&next_line);
                    app.script_editor_modified = true;
                }
            }
        }

        // Editing - Tab (insert 4 spaces)
        (KeyCode::Tab, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                line.insert_str(col, "    ");
                app.script_editor_cursor.1 += 4;
                app.script_editor_modified = true;
            }
        }

        // Typing characters
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                line.insert(col, c);
                app.script_editor_cursor.1 += 1;
                app.script_editor_modified = true;
            }
        }

        _ => {}
    }
    Ok(())
}

fn handle_edit_notes(app: &mut App, key: KeyEvent) -> Result<()> {
    let total_lines = app.script_editor_lines.len();

    match (key.code, key.modifiers) {
        // Save with Ctrl+S
        (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => {
            match app.save_notes_from_editor() {
                Ok(()) => app.set_status("Notes saved"),
                Err(e) => app.set_status(format!("Error saving notes: {}", e)),
            }
        }

        // Cancel/Exit with Esc
        (KeyCode::Esc, _) => {
            if app.script_editor_modified {
                app.push_screen(Screen::Confirm(ConfirmAction::DiscardNotesChanges));
            } else {
                app.raw_script_scroll = 0;
                app.script_editor_lines.clear();
                app.pop_screen();
            }
        }

        // Navigation
        (KeyCode::Up, _) => {
            if app.script_editor_cursor.0 > 0 {
                app.script_editor_cursor.0 -= 1;
                let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                    .map(|l| l.len()).unwrap_or(0);
                if app.script_editor_cursor.1 > line_len {
                    app.script_editor_cursor.1 = line_len;
                }
                if app.script_editor_cursor.0 < app.raw_script_scroll as usize {
                    app.raw_script_scroll = app.script_editor_cursor.0 as u16;
                }
            }
        }
        (KeyCode::Down, _) => {
            if app.script_editor_cursor.0 < total_lines.saturating_sub(1) {
                app.script_editor_cursor.0 += 1;
                let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                    .map(|l| l.len()).unwrap_or(0);
                if app.script_editor_cursor.1 > line_len {
                    app.script_editor_cursor.1 = line_len;
                }
                let visible_height = 35usize;
                if app.script_editor_cursor.0 >= app.raw_script_scroll as usize + visible_height {
                    app.raw_script_scroll = (app.script_editor_cursor.0 - visible_height + 1) as u16;
                }
            }
        }
        (KeyCode::Left, _) => {
            if app.script_editor_cursor.1 > 0 {
                app.script_editor_cursor.1 -= 1;
            } else if app.script_editor_cursor.0 > 0 {
                app.script_editor_cursor.0 -= 1;
                app.script_editor_cursor.1 = app.script_editor_lines.get(app.script_editor_cursor.0)
                    .map(|l| l.len()).unwrap_or(0);
            }
            if app.script_editor_cursor.1 < app.script_editor_h_scroll {
                app.script_editor_h_scroll = app.script_editor_cursor.1;
            }
        }
        (KeyCode::Right, _) => {
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            if app.script_editor_cursor.1 < line_len {
                app.script_editor_cursor.1 += 1;
            } else if app.script_editor_cursor.0 < total_lines.saturating_sub(1) {
                app.script_editor_cursor.0 += 1;
                app.script_editor_cursor.1 = 0;
            }
            let visible_width = 80usize;
            if app.script_editor_cursor.1 >= app.script_editor_h_scroll + visible_width {
                app.script_editor_h_scroll = app.script_editor_cursor.1 - visible_width + 1;
            }
        }
        (KeyCode::Home, _) => {
            app.script_editor_cursor.1 = 0;
            app.script_editor_h_scroll = 0;
        }
        (KeyCode::End, _) => {
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            app.script_editor_cursor.1 = line_len;
        }
        (KeyCode::PageUp, _) => {
            let jump = 20;
            app.script_editor_cursor.0 = app.script_editor_cursor.0.saturating_sub(jump);
            app.raw_script_scroll = app.raw_script_scroll.saturating_sub(jump as u16);
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            if app.script_editor_cursor.1 > line_len {
                app.script_editor_cursor.1 = line_len;
            }
        }
        (KeyCode::PageDown, _) => {
            let jump = 20;
            app.script_editor_cursor.0 = (app.script_editor_cursor.0 + jump).min(total_lines.saturating_sub(1));
            app.raw_script_scroll = (app.raw_script_scroll + jump as u16).min(total_lines.saturating_sub(1) as u16);
            let line_len = app.script_editor_lines.get(app.script_editor_cursor.0)
                .map(|l| l.len()).unwrap_or(0);
            if app.script_editor_cursor.1 > line_len {
                app.script_editor_cursor.1 = line_len;
            }
        }

        // Editing - Enter (new line)
        (KeyCode::Enter, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                let remainder = line[col..].to_string();
                line.truncate(col);
                app.script_editor_lines.insert(line_idx + 1, remainder);
                app.script_editor_cursor = (line_idx + 1, 0);
                app.script_editor_modified = true;
            }
        }

        // Editing - Backspace
        (KeyCode::Backspace, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if col > 0 {
                if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                    line.remove(col - 1);
                    app.script_editor_cursor.1 -= 1;
                    app.script_editor_modified = true;
                }
            } else if line_idx > 0 {
                let current_line = app.script_editor_lines.remove(line_idx);
                if let Some(prev_line) = app.script_editor_lines.get_mut(line_idx - 1) {
                    let prev_len = prev_line.len();
                    prev_line.push_str(&current_line);
                    app.script_editor_cursor = (line_idx - 1, prev_len);
                    app.script_editor_modified = true;
                }
            }
        }

        // Editing - Delete
        (KeyCode::Delete, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                if col < line.len() {
                    line.remove(col);
                    app.script_editor_modified = true;
                } else if line_idx < total_lines - 1 {
                    let next_line = app.script_editor_lines.remove(line_idx + 1);
                    app.script_editor_lines.get_mut(line_idx).unwrap().push_str(&next_line);
                    app.script_editor_modified = true;
                }
            }
        }

        // Editing - Tab (insert 4 spaces)
        (KeyCode::Tab, _) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                line.insert_str(col, "    ");
                app.script_editor_cursor.1 += 4;
                app.script_editor_modified = true;
            }
        }

        // Typing characters
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            let (line_idx, col) = app.script_editor_cursor;
            if let Some(line) = app.script_editor_lines.get_mut(line_idx) {
                line.insert(col, c);
                app.script_editor_cursor.1 += 1;
                app.script_editor_modified = true;
            }
        }

        _ => {}
    }
    Ok(())
}

fn handle_detailed_info(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        app.pop_screen();
    }
    Ok(())
}

fn handle_snapshots(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.selected_menu_item = 0; // Reset for management menu
            app.pop_screen();
        }
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
                if app.running_vms.contains_key(&vm.id) {
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
    use crate::app::FileBrowserMode;

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
                    app.load_file_browser(FileBrowserMode::Iso);
                    app.push_screen(Screen::FileBrowser);
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_display_options(app: &mut App, key: KeyEvent) -> Result<()> {
    let display_options = screens::management::get_display_options(app);
    let option_count = display_options.len();

    match key.code {
        KeyCode::Esc => {
            app.selected_menu_item = 3; // Reset to Change Display position in management menu
            app.pop_screen();
        }
        KeyCode::Char('j') | KeyCode::Down => app.menu_next(option_count),
        KeyCode::Char('k') | KeyCode::Up => app.menu_prev(),
        KeyCode::Enter | KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') => {
            let item = match key.code {
                KeyCode::Char('1') => 0,
                KeyCode::Char('2') => 1,
                KeyCode::Char('3') => 2,
                KeyCode::Char('4') => 3,
                _ => app.selected_menu_item,
            };

            if let Some((display_name, _)) = display_options.get(item) {
                let display_name = display_name.clone();
                // Update the display setting in launch.sh
                if let Some(vm) = app.selected_vm() {
                    match update_vm_display(&vm.launch_script, &display_name) {
                        Ok(()) => {
                            // Show spice-app warning if viewer not installed
                            if display_name.contains("spice") && !crate::commands::qemu_system::is_spice_viewer_available() {
                                app.set_status(format!("Display changed to {}. Warning: virt-viewer/remote-viewer not found!", display_name));
                            } else {
                                app.set_status(format!("Display changed to {}", display_name));
                            }
                            // Reload the script to reflect changes
                            app.reload_selected_vm_script();
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to change display: {}", e));
                        }
                    }
                }
                app.selected_menu_item = 3;
                app.pop_screen();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Update the display setting in a VM's launch script
fn update_vm_display(script_path: &std::path::Path, new_display: &str) -> Result<()> {
    let content = std::fs::read_to_string(script_path)?;

    // Regex to match -display with optional gl=on suffix
    // Uses [\w-]+ to match hyphenated backends like spice-app
    let display_re = Regex::new(r"-display\s+([\w-]+)(,gl=on)?")?;

    let new_content = if display_re.is_match(&content) {
        // Replace existing -display setting, preserving gl=on if present
        display_re.replace_all(&content, |caps: &regex::Captures| {
            if caps.get(2).is_some() {
                format!("-display {},gl=on", new_display)
            } else {
                format!("-display {}", new_display)
            }
        }).to_string()
    } else {
        // No -display found, this shouldn't happen for wizard-generated scripts
        // but handle gracefully
        content
    };

    std::fs::write(script_path, new_content)?;
    Ok(())
}

fn handle_usb_devices(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.selected_menu_item = 2; // Reset to USB Passthrough position in management menu
            app.pop_screen();
        }
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
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Save USB passthrough configuration to launch.sh
            let save_result = if let Some(vm) = app.selected_vm() {
                let devices: Vec<crate::vm::UsbPassthrough> = app
                    .selected_usb_devices
                    .iter()
                    .filter_map(|&i| app.usb_devices.get(i))
                    .map(|d| crate::vm::UsbPassthrough {
                        vendor_id: d.vendor_id,
                        product_id: d.product_id,
                        usb_version: d.usb_version,
                    })
                    .collect();

                let result = crate::vm::save_usb_passthrough(vm, &devices);
                Some((result, devices.len()))
            } else {
                None
            };

            if let Some((result, count)) = save_result {
                match result {
                    Ok(()) => {
                        // Reload the script so changes are visible in raw script viewer
                        app.reload_selected_vm_script();

                        let mut status_msg = if count > 0 {
                            format!("Saved {} USB device(s) to launch.sh", count)
                        } else {
                            "Cleared USB passthrough from launch.sh".to_string()
                        };

                        // Regenerate single-GPU scripts if they exist
                        // USB devices are important for single-GPU since there's no graphical session
                        if let Some(vm) = app.selected_vm() {
                            if crate::hardware::scripts_exist(&vm.path) {
                                // Try with in-memory config first, fall back to saved config
                                let regen_result = if let Some(config) = app.single_gpu_config.as_ref() {
                                    crate::vm::single_gpu_scripts::regenerate_if_exists(vm, config)
                                } else {
                                    crate::vm::single_gpu_scripts::regenerate_from_saved_config(vm)
                                };

                                match regen_result {
                                    Ok(true) => {
                                        status_msg.push_str("; single-GPU scripts regenerated");
                                    }
                                    Ok(false) => {} // Scripts don't exist, nothing to regenerate
                                    Err(e) => {
                                        status_msg.push_str(&format!("; warning: failed to regenerate single-GPU scripts: {}", e));
                                    }
                                }
                            }
                        }

                        app.set_status(status_msg);
                    }
                    Err(e) => {
                        app.set_status(format!("Error saving USB config: {}", e));
                    }
                }
            }
        }
        KeyCode::Char('u') | KeyCode::Char('U') => {
            // Install udev rules for selected USB devices
            if app.selected_usb_devices.is_empty() {
                app.set_status("Select USB devices first, then press 'u' to install permissions");
            } else {
                let selected_devices: Vec<_> = app
                    .selected_usb_devices
                    .iter()
                    .filter_map(|&i| app.usb_devices.get(i).cloned())
                    .collect();

                app.set_status("Installing udev rules (you may be prompted for your password)...");

                // We need to drop the terminal temporarily for the password prompt
                // The install function will handle elevation
                match crate::hardware::install_udev_rules(&selected_devices) {
                    crate::hardware::UdevInstallResult::Success => {
                        app.set_status("USB permissions installed! Devices should now work without sudo.");
                    }
                    crate::hardware::UdevInstallResult::NeedsReboot => {
                        app.set_status("Rules installed. Please unplug/replug devices or reboot.");
                    }
                    crate::hardware::UdevInstallResult::PermissionDenied => {
                        app.set_status("Permission denied. Authentication cancelled or failed.");
                    }
                    crate::hardware::UdevInstallResult::Error(e) => {
                        app.set_status(format!("Error installing rules: {}", e));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_confirm(app: &mut App, action: ConfirmAction, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => app.pop_screen(),
        KeyCode::Char('y') | KeyCode::Enter => {
            execute_confirm_action(app, action)?;
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
        ConfirmAction::DiscardScriptChanges | ConfirmAction::DiscardNotesChanges => {
            ("Discard Changes", "You have unsaved changes. Discard them?".to_string())
        }
        ConfirmAction::StopVm => {
            let name = app.selected_vm()
                .map(|vm| vm.display_name())
                .unwrap_or_else(|| "VM".to_string());
            ("Stop VM", format!("Stop {}?", name))
        }
        ConfirmAction::ForceStopVm => {
            let name = app.selected_vm()
                .map(|vm| vm.display_name())
                .unwrap_or_else(|| "VM".to_string());
            ("Force Stop VM", format!("Force stop {}? This may cause data loss.", name))
        }
    };

    ConfirmDialog::new(title, &message).render(frame.area(), frame.buffer_mut());
}

fn render_usb_devices(app: &App, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
    use ratatui::layout::{Layout, Direction, Constraint};

    let area = frame.area();
    let dialog_width = 80.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let selected_count = app.selected_usb_devices.len();
    let title = if selected_count > 0 {
        format!(" USB Passthrough ({} selected) ", selected_count)
    } else {
        " USB Passthrough ".to_string()
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

    // Split into padding, content, and help
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Min(4),     // Device list
            Constraint::Length(2),  // Help text
        ])
        .split(h_chunks[1]);

    let content_area = v_chunks[1];
    let help_area = v_chunks[2];

    if app.usb_devices.is_empty() {
        let msg = Paragraph::new("No USB devices found.\n\nConnect a USB device and reopen this screen.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, content_area);
    } else {
        let items: Vec<ListItem> = app.usb_devices
            .iter()
            .enumerate()
            .map(|(i, device)| {
                let selected = app.selected_usb_devices.contains(&i);
                let checkbox = if selected { "[✓]" } else { "[ ]" };
                let style = if i == app.selected_menu_item {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if selected {
                    Style::default().fg(Color::Green)
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
        frame.render_stateful_widget(list, content_area, &mut state);
    }

    // Help text
    let help = Paragraph::new("[Space] Toggle  [s] Save  [u] Install USB permissions  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, help_area);
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
    use crate::app::FileBrowserMode;

    let area = frame.area();
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let title_prefix = match app.file_browser_mode {
        FileBrowserMode::Iso => "Select ISO",
        FileBrowserMode::Disk => "Select Disk Image",
        FileBrowserMode::Directory => "Select Directory",
        FileBrowserMode::ImportConfig => "Select Config File",
    };
    let title = format!(" {} - {} ", title_prefix, app.file_browser_dir.display());
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
        let msg_text = match app.file_browser_mode {
            FileBrowserMode::Iso => "No ISO files found in this directory.",
            FileBrowserMode::Disk => "No disk images found in this directory.",
            FileBrowserMode::Directory => "No subdirectories in this directory.",
            FileBrowserMode::ImportConfig => "No config files (.xml, .conf) found in this directory.",
        };
        let msg = ratatui::widgets::Paragraph::new(msg_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, content_area);
        return;
    }

    let items: Vec<ListItem> = app.file_browser_entries
        .iter()
        .map(|entry| {
            let prefix = if entry.name == "[Select This Directory]" {
                ">> "
            } else if entry.is_dir {
                "📁 "
            } else {
                "💿 "
            };
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
    use crate::app::FileBrowserMode;

    match key.code {
        KeyCode::Esc => app.pop_screen(),
        KeyCode::Char('j') | KeyCode::Down => app.file_browser_next(),
        KeyCode::Char('k') | KeyCode::Up => app.file_browser_prev(),
        KeyCode::Enter => {
            if let Some(selected_path) = app.file_browser_enter() {
                match app.file_browser_mode {
                    FileBrowserMode::Iso => {
                        // Check if we're in wizard mode
                        if app.wizard_state.is_some() {
                            // Set the ISO path in wizard state
                            if let Some(ref mut state) = app.wizard_state {
                                state.iso_path = Some(selected_path);
                            }
                            app.pop_screen(); // Close file browser

                            // Proceed to next step
                            let _ = app.wizard_next_step();
                        } else {
                            // Normal boot mode - selected an ISO file
                            app.boot_mode = BootMode::Cdrom(selected_path);
                            app.pop_screen(); // Close file browser
                            app.pop_screen(); // Close boot options
                            app.push_screen(Screen::Confirm(ConfirmAction::LaunchVm));
                        }
                    }
                    FileBrowserMode::Disk => {
                        // Selected a disk file - must be in wizard mode
                        if let Some(ref mut state) = app.wizard_state {
                            state.existing_disk_path = Some(selected_path);
                        }
                        app.pop_screen(); // Close file browser, return to disk config step
                    }
                    FileBrowserMode::Directory => {
                        // Directory selected (from [Select This Directory] entry)
                        app.add_shared_folder(selected_path.to_string_lossy().to_string());
                        app.pop_screen(); // Return to SharedFolders screen
                    }
                    FileBrowserMode::ImportConfig => {
                        // Selected a config file for import
                        match crate::vm::import::parse_config_file(&selected_path) {
                            Ok(vm) => {
                                app.pop_screen(); // Close file browser

                                let library_path = app.config.vm_library_path.clone();
                                if let Some(ref mut state) = app.import_state {
                                    state.vm_name = vm.name.clone();
                                    state.folder_name =
                                        crate::app::CreateWizardState::find_available_folder_name(
                                            &library_path,
                                            &crate::app::CreateWizardState::generate_folder_name(&vm.name),
                                        );
                                    let has_notes = !vm.import_notes.is_empty();
                                    state.selected_vm = Some(vm);
                                    state.error_message = None;
                                    state.field_focus = 0;

                                    if has_notes {
                                        state.warnings_acknowledged = false;
                                        state.step = crate::app::ImportStep::CompatibilityWarnings;
                                    } else {
                                        state.warnings_acknowledged = true;
                                        state.step = crate::app::ImportStep::ConfigureDisk;
                                    }
                                } else {
                                    // No import state - start fresh
                                    let has_notes = !vm.import_notes.is_empty();
                                    let source = vm.source.clone();
                                    let folder_name =
                                        crate::app::CreateWizardState::find_available_folder_name(
                                            &library_path,
                                            &crate::app::CreateWizardState::generate_folder_name(&vm.name),
                                        );
                                    let vm_name = vm.name.clone();
                                    let (step, warnings_acknowledged) = if has_notes {
                                        (crate::app::ImportStep::CompatibilityWarnings, false)
                                    } else {
                                        (crate::app::ImportStep::ConfigureDisk, true)
                                    };

                                    app.import_state = Some(crate::app::ImportWizardState {
                                        source: Some(source),
                                        vm_name,
                                        folder_name,
                                        selected_vm: Some(vm),
                                        step,
                                        warnings_acknowledged,
                                        ..crate::app::ImportWizardState::default()
                                    });
                                    app.push_screen(crate::app::Screen::ImportWizard);
                                }
                            }
                            Err(e) => {
                                if let Some(ref mut state) = app.import_state {
                                    state.error_message = Some(format!("Failed to parse: {}", e));
                                }
                                app.pop_screen(); // Close file browser
                            }
                        }
                    }
                }
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
        TextInputContext::RenameVm => " Enter New VM Name ",
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
                TextInputContext::RenameVm => {
                    if !input.is_empty() {
                        if let Some(vm) = app.selected_vm().cloned() {
                            match crate::vm::lifecycle::rename_vm(&vm, &input) {
                                Ok(()) => {
                                    app.set_status(format!("Renamed to: {}", input));
                                    // Refresh to pick up the new name
                                    let _ = app.refresh_vms();
                                }
                                Err(e) => {
                                    app.set_status(format!("Error renaming: {}", e));
                                }
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
            // Allow different characters based on context
            let allowed = match context {
                TextInputContext::SnapshotName => {
                    // Only safe characters for snapshot names
                    c.is_alphanumeric() || c == '-' || c == '_' || c == '.'
                }
                TextInputContext::RenameVm => {
                    // Allow more characters for VM display names
                    c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' || c == '(' || c == ')'
                }
            };
            if allowed {
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
    // Make error dialog larger and more prominent
    let dialog_width = 80.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" ⚠ Error ")
        .title_bottom(" [↑/↓ or j/k] Scroll  [Enter/Esc] Close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let error_text = app.error_detail.as_deref().unwrap_or("No error details");

    // Add visual separator and formatting
    let formatted_error = format!("{}\n\n─────────────────────────────────────────\nCheck the QEMU configuration or launch.sh script for issues.", error_text);

    let paragraph = Paragraph::new(formatted_error)
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

fn handle_single_gpu_instructions(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.single_gpu_show_instructions = false;
            app.pop_screen();
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
