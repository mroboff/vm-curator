use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::app::App;
use crate::config::Config;
use crate::vm::DiscoveredVm;

/// Menu item with name and description
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: &'static str,
    pub description: &'static str,
    pub action: MenuAction,
}

/// Actions that can be performed from the management menu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    StopVm,
    BootOptions,
    Snapshots,
    UsbPassthrough,
    PciPassthrough,
    SharedFolders,
    MultiGpuPassthrough,
    SingleGpuPassthrough,
    ChangeDisplay,
    RenameVm,
    ResetVm,
    DeleteVm,
    EditRawConfig,
}

/// Get menu items based on config and VM state
pub fn get_menu_items(vm: &DiscoveredVm, config: &Config) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem {
            name: "Stop VM",
            description: "Shut down the running VM (ACPI poweroff)",
            action: MenuAction::StopVm,
        },
        MenuItem {
            name: "Boot Options",
            description: "Normal, install, or custom ISO boot",
            action: MenuAction::BootOptions,
        },
        MenuItem {
            name: "Snapshots",
            description: "Create, restore, or delete snapshots",
            action: MenuAction::Snapshots,
        },
        MenuItem {
            name: "USB Passthrough",
            description: "Pass USB devices to the VM",
            action: MenuAction::UsbPassthrough,
        },
        MenuItem {
            name: "PCI Passthrough",
            description: "Pass PCI devices to the VM",
            action: MenuAction::PciPassthrough,
        },
        MenuItem {
            name: "Shared Folders",
            description: "Share host directories with the VM (9p)",
            action: MenuAction::SharedFolders,
        },
    ];

    // Add Multi-GPU Passthrough option if enabled in settings
    if config.enable_multi_gpu_passthrough {
        items.push(MenuItem {
            name: "Multi-GPU Passthrough",
            description: "Pass a secondary GPU to the VM with Looking Glass",
            action: MenuAction::MultiGpuPassthrough,
        });
    }

    // Add Single GPU Passthrough option if enabled in settings
    if config.single_gpu_enabled {
        items.push(MenuItem {
            name: "Single GPU Passthrough",
            description: "Configure passthrough for your primary GPU",
            action: MenuAction::SingleGpuPassthrough,
        });
    }

    items.extend([
        MenuItem {
            name: "Change Display",
            description: "GTK, SDL, SPICE-app, or VNC output",
            action: MenuAction::ChangeDisplay,
        },
        MenuItem {
            name: "Rename VM",
            description: "Change the VM's display name",
            action: MenuAction::RenameVm,
        },
    ]);

    // Add dangerous operations at the end
    items.extend([
        MenuItem {
            name: "Reset VM (recreate disk)",
            description: "Restore VM to fresh state",
            action: MenuAction::ResetVm,
        },
        MenuItem {
            name: "Delete VM",
            description: "Permanently remove this VM",
            action: MenuAction::DeleteVm,
        },
        MenuItem {
            name: "Edit Raw Configuration",
            description: "Edit the launch.sh script directly",
            action: MenuAction::EditRawConfig,
        },
    ]);

    // Check for GPU passthrough script
    let _has_gpu_script = vm.path.join("launch-with-gpu-passthrough.sh").exists();
    // Future: Add "Launch with GPU Passthrough" or "Remove GPU Passthrough" based on this

    items
}

/// Get the count of menu items (for navigation bounds)
pub fn menu_item_count(app: &App) -> usize {
    if let Some(vm) = app.selected_vm() {
        get_menu_items(vm, &app.config).len()
    } else {
        6 // Default count
    }
}

/// Default display options for VMs (used as fallback descriptions)
const DISPLAY_OPTIONS: &[(&str, &str)] = &[
    ("gtk", "GTK - Default windowed display"),
    ("sdl", "SDL - Better for 3D acceleration"),
    ("spice-app", "SPICE - Remote desktop (needs virt-viewer)"),
    ("vnc", "VNC - Network accessible display"),
    ("none", "None - Headless, no graphical output"),
];

/// Get dynamic display options based on detected emulator capabilities.
/// Falls back to DISPLAY_OPTIONS if detection is not available.
pub fn get_display_options(app: &App) -> Vec<(String, String)> {
    // Get the emulator for the currently selected VM
    let emulator = app.selected_vm()
        .map(|vm| vm.config.emulator.command())
        .unwrap_or("qemu-system-x86_64");

    let detected = app.get_display_options_for_emulator(emulator);

    // Map detected backends to (name, description) pairs using DISPLAY_OPTIONS for descriptions
    detected.iter().map(|backend| {
        let desc = DISPLAY_OPTIONS.iter()
            .find(|(name, _)| *name == backend.as_str())
            .map(|(_, desc)| desc.to_string())
            .unwrap_or_else(|| format!("{} display", backend));
        (backend.clone(), desc)
    }).collect()
}

/// Render the management menu
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Get dynamic menu items
    let menu_items = if let Some(vm) = app.selected_vm() {
        get_menu_items(vm, &app.config)
    } else {
        Vec::new()
    };

    // Calculate dialog size - adjust height based on item count
    let dialog_width = 50.min(area.width.saturating_sub(4));
    let item_count = menu_items.len();
    let dialog_height = (6 + item_count * 2).min(area.height.saturating_sub(4) as usize) as u16;

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
    let items: Vec<ListItem> = menu_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == app.selected_menu_item {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::styled(format!("[{}] {}", i + 1, item.name), style),
                Line::styled(
                    format!("    {}", item.description),
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

/// Render display options submenu
pub fn render_display_options(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 50.min(area.width.saturating_sub(4));
    let dialog_height = 16.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    // Get current display setting from VM
    let current_display = app.selected_vm()
        .map(|vm| extract_display_from_script(&vm.config.raw_script))
        .unwrap_or_else(|| "gtk".to_string());

    let block = Block::default()
        .title(format!(" Display Options (current: {}) ", current_display))
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

    // Add top padding and help area
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Min(1),     // Content
            Constraint::Length(2),  // Help
        ])
        .split(h_chunks[1]);

    let display_options = get_display_options(app);

    let items: Vec<ListItem> = display_options
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let is_current = *name == current_display;
            let style = if i == app.selected_menu_item {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };

            let marker = if is_current { " *" } else { "" };

            ListItem::new(vec![
                Line::styled(format!("[{}] {}{}", i + 1, name, marker), style),
                Line::styled(format!("    {}", desc), Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected_menu_item));

    let list = List::new(items);
    frame.render_stateful_widget(list, v_chunks[1], &mut state);

    // Help text
    let help = Paragraph::new("[Enter] Select  [Esc] Back")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, v_chunks[2]);
}

/// Extract display setting from launch script
fn extract_display_from_script(script: &str) -> String {
    // Look for -display X pattern
    if let Some(pos) = script.find("-display ") {
        let rest = &script[pos + 9..];
        // Find the display value (ends at space, comma, or backslash)
        let end = rest.find(|c: char| c.is_whitespace() || c == ',' || c == '\\')
            .unwrap_or(rest.len());
        let display = rest[..end].trim();
        // Handle gl=on suffix
        if let Some(comma_pos) = display.find(',') {
            return display[..comma_pos].to_string();
        }
        return display.to_string();
    }
    "gtk".to_string() // Default
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
