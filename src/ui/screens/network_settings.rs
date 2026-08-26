//! Network Settings Screen
//!
//! Allows editing network backend, adapter model, and port forwarding
//! on existing VMs from the management menu.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{
    AddPfStep, AddingPortForward, App, ConfirmAction, NetworkSettingsState, NicConfig, Screen,
    UnsavedKind,
};
use crate::vm::qemu_config::{PortForward, PortProtocol};

/// Network adapter model options (same as create wizard)
const NETWORK_OPTIONS: &[&str] = &["virtio", "e1000", "rtl8139", "ne2k_pci", "pcnet", "none"];


/// Render the network settings screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 72.min(area.width.saturating_sub(4));
    let dialog_height = 32.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let Some(ref ns) = app.network_settings_state else {
        return;
    };

    let block = Block::default()
        .title(" Network Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    if ns.editing_port_forwards {
        render_port_forward_editor(app, ns, frame, inner);
        return;
    }

    if !ns.editing_nic {
        render_nic_list(ns, frame, inner);
        return;
    }

    render_nic_editor(app, ns, frame, inner);
}

/// Render the list of NICs.
fn render_nic_list(ns: &NetworkSettingsState, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Min(6),    // NIC list
            Constraint::Length(2), // Help
        ])
        .split(area);

    let header = Paragraph::new("Network Adapters").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    let mut lines = Vec::new();
    for (i, nic) in ns.nics.iter().enumerate() {
        let selected = i == ns.list_cursor;
        let prefix = if selected { "> " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::styled(
            format!("{}NIC {}: {}", prefix, i + 1, nic.describe()),
            style,
        ));
    }

    frame.render_widget(Paragraph::new(lines), chunks[2]);

    let help = Paragraph::new("[Enter] Edit  [a] Add  [d] Delete  [s] Save  [j/k] Navigate  [Esc] Cancel")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
}

/// Render the field editor for the NIC at `ns.active_nic`.
fn render_nic_editor(app: &App, ns: &NetworkSettingsState, frame: &mut Frame, area: Rect) {
    let nic = &ns.nics[ns.active_nic];
    let is_bridge = nic.is_bridge();
    let show_mac = nic.show_mac();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Adapter field
            Constraint::Length(1), // Backend field
            Constraint::Length(1), // MAC field
            Constraint::Length(1), // Bridge name / Port forwards field
            Constraint::Length(1), // Spacer
            Constraint::Min(6),    // Info area
            Constraint::Length(2), // Help
        ])
        .split(area);

    // Header
    let header = Paragraph::new(format!(
        "Configure NIC {} of {}",
        ns.active_nic + 1,
        ns.nics.len()
    ))
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    // Adapter model
    let adapter_selected = ns.selected_field == 0;
    let adapter_line = render_field_line(
        "Adapter:",
        &nic.model,
        adapter_selected,
        "[←/→/Tab] cycle",
    );
    frame.render_widget(Paragraph::new(adapter_line), chunks[2]);

    // Backend
    let backend_selected = ns.selected_field == 1;
    let backend_display = nic.backend_display();
    let backend_line = render_field_line(
        "Backend:",
        &backend_display,
        backend_selected,
        "[←/→/Tab] cycle",
    );
    frame.render_widget(Paragraph::new(backend_line), chunks[3]);

    // MAC address (hidden when backend == "none")
    if show_mac {
        let mac_selected = ns.selected_field == 2;
        let mac_display = if ns.editing_mac {
            format!("{}_", ns.mac_edit_buffer)
        } else if let Some(mac) = nic.mac_address.as_deref() {
            mac.to_string()
        } else {
            "(auto)".to_string()
        };
        let mac_hint = if ns.editing_mac {
            "[Enter] save  [Esc] cancel"
        } else if mac_selected {
            "[Enter] edit  [r] randomize  [c] clear"
        } else {
            ""
        };
        let mac_line = render_field_line("MAC:", &mac_display, mac_selected, mac_hint);
        frame.render_widget(Paragraph::new(mac_line), chunks[4]);
    }

    // Bridge name (when bridge backend) or Port forwards (when user/passt)
    let show_pf = nic.show_port_forwards();
    let bridge_pf_selected = ns.selected_field == 3;
    if is_bridge {
        let bridge_display = nic.bridge_name.as_deref().unwrap_or("qemubr0");
        let bridge_line = render_field_line(
            "Bridge:",
            bridge_display,
            bridge_pf_selected,
            "[←/→/Tab] cycle",
        );
        frame.render_widget(Paragraph::new(bridge_line), chunks[5]);
    } else if show_pf {
        let pf_count = nic.port_forwards.len();
        let pf_display = if pf_count == 0 {
            "none".to_string()
        } else {
            format!("{} rule(s)", pf_count)
        };
        let pf_hint = if bridge_pf_selected {
            "[Enter] edit"
        } else {
            ""
        };
        let pf_line = render_field_line("Forwards:", &pf_display, bridge_pf_selected, pf_hint);
        frame.render_widget(Paragraph::new(pf_line), chunks[5]);
    }

    // Info area: bridge status (when bridge) or port forward list (when user/passt)
    if is_bridge {
        let caps = &app.network_caps;
        let mut lines = Vec::new();

        // Bridge helper status
        let helper_str = match &caps.bridge_helper_path {
            Some(p) => format!("found ({})", p.display()),
            None => "not found".to_string(),
        };
        let helper_color = if caps.bridge_helper_path.is_some() {
            Color::Green
        } else {
            Color::Red
        };
        lines.push(Line::from(vec![
            Span::styled("  bridge-helper: ", Style::default().fg(Color::Yellow)),
            Span::styled(helper_str, Style::default().fg(helper_color)),
        ]));

        // Permissions status
        let perm_str = if caps.bridge_helper_configured {
            "configured (setuid/cap_net_admin)"
        } else {
            "not configured"
        };
        let perm_color = if caps.bridge_helper_configured {
            Color::Green
        } else {
            Color::Red
        };
        lines.push(Line::from(vec![
            Span::styled("  Permissions:   ", Style::default().fg(Color::Yellow)),
            Span::styled(perm_str, Style::default().fg(perm_color)),
        ]));

        // System bridges
        let bridges_str = if caps.system_bridges.is_empty() {
            "none found".to_string()
        } else {
            caps.system_bridges.join(", ")
        };
        let bridges_color = if caps.system_bridges.is_empty() {
            Color::Red
        } else {
            Color::Green
        };
        lines.push(Line::from(vec![
            Span::styled("  Bridges:       ", Style::default().fg(Color::Yellow)),
            Span::styled(bridges_str, Style::default().fg(bridges_color)),
        ]));

        // Managed networks (issue #53)
        if !app.vnet_networks.is_empty() {
            let managed = app
                .vnet_networks
                .iter()
                .map(|n| n.describe())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(Line::from(vec![
                Span::styled("  Managed nets:  ", Style::default().fg(Color::Yellow)),
                Span::styled(managed, Style::default().fg(Color::Green)),
            ]));
        }

        // Setup guidance if incomplete
        if caps.bridge_helper_path.is_none()
            || !caps.bridge_helper_configured
            || caps.system_bridges.is_empty()
        {
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "  Setup needed:",
                Style::default().fg(Color::Yellow),
            ));
            if caps.bridge_helper_path.is_none() {
                lines.push(Line::styled(
                    "    Install: qemu-bridge-helper (part of QEMU)",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            if !caps.bridge_helper_configured {
                lines.push(Line::styled(
                    "    Run: sudo setcap cap_net_admin+ep /usr/lib/qemu/qemu-bridge-helper",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            if caps.system_bridges.is_empty() {
                lines.push(Line::styled(
                    "    Create bridge: sudo ip link add qemubr0 type bridge",
                    Style::default().fg(Color::DarkGray),
                ));
                lines.push(Line::styled(
                    "    Enable:        sudo ip link set qemubr0 up",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        let info = Paragraph::new(lines);
        frame.render_widget(info, chunks[7]);
    } else if show_pf && !nic.port_forwards.is_empty() {
        let mut lines = Vec::new();
        lines.push(Line::styled(
            "  Current port forwarding rules:",
            Style::default().fg(Color::DarkGray),
        ));
        for pf in &nic.port_forwards {
            lines.push(Line::from(format!(
                "    {} {} -> {}",
                pf.protocol, pf.host_port, pf.guest_port
            )));
        }
        let list = Paragraph::new(lines);
        frame.render_widget(list, chunks[7]);
    }

    // Help
    let help = Paragraph::new("[s] Save  [Esc] Back  [j/k] Navigate  [←/→] Change")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[8]);
}

/// Render the port forward editor overlay
fn render_port_forward_editor(
    _app: &App,
    ns: &NetworkSettingsState,
    frame: &mut Frame,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Min(8),    // Rules list
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Presets
            Constraint::Length(2), // Help
        ])
        .split(area);

    // Check if we're adding a port forward
    if let Some(ref adding) = ns.adding_pf {
        render_adding_pf(adding, frame, area);
        return;
    }

    let header = Paragraph::new("Port Forwarding Rules").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    let port_forwards = &ns.nics[ns.active_nic].port_forwards;

    // Rules list
    if port_forwards.is_empty() {
        let msg = Paragraph::new("  No port forwarding rules configured.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, chunks[2]);
    } else {
        let mut lines = Vec::new();
        for (i, pf) in port_forwards.iter().enumerate() {
            let is_selected = i == ns.pf_selected;
            let prefix = if is_selected { "> " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::styled(
                format!(
                    "{}{}  {} -> {}",
                    prefix, pf.protocol, pf.host_port, pf.guest_port
                ),
                style,
            ));
        }
        let list = Paragraph::new(lines);
        frame.render_widget(list, chunks[2]);
    }

    // Presets
    let presets = Paragraph::new("  Presets: [1] SSH  [2] RDP  [3] HTTP  [4] HTTPS  [5] VNC")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(presets, chunks[4]);

    // Help
    let help = Paragraph::new("[a] Add  [d] Delete  [1-5] Preset  [Esc] Done")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[5]);
}

/// Render the "adding a port forward" input dialog
fn render_adding_pf(adding: &AddingPortForward, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Protocol
            Constraint::Length(1), // Host port
            Constraint::Length(1), // Guest port
            Constraint::Min(3),    // Spacer
            Constraint::Length(2), // Help
        ])
        .split(area);

    let header = Paragraph::new("Add Port Forward Rule").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, chunks[0]);

    // Protocol
    let proto_active = adding.step == AddPfStep::Protocol;
    let proto_style = if proto_active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let proto_hint = if proto_active {
        " [←/→] toggle"
    } else {
        ""
    };
    let proto_line = Line::from(vec![
        Span::styled("  Protocol: ", Style::default().fg(Color::Yellow)),
        Span::styled(format!("{}", adding.protocol), proto_style),
        Span::styled(proto_hint, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(proto_line), chunks[2]);

    // Host port
    let host_active = adding.step == AddPfStep::HostPort;
    let host_style = if host_active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let host_line = Line::from(vec![
        Span::styled("  Host Port: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            if adding.host_port_input.is_empty() {
                "_"
            } else {
                &adding.host_port_input
            },
            host_style,
        ),
    ]);
    frame.render_widget(Paragraph::new(host_line), chunks[3]);

    // Guest port
    let guest_active = adding.step == AddPfStep::GuestPort;
    let guest_style = if guest_active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let guest_line = Line::from(vec![
        Span::styled("  Guest Port: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            if adding.guest_port_input.is_empty() {
                "_"
            } else {
                &adding.guest_port_input
            },
            guest_style,
        ),
    ]);
    frame.render_widget(Paragraph::new(guest_line), chunks[4]);

    let help = Paragraph::new("[Enter] Next/Confirm  [Esc] Cancel")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[6]);
}

fn render_field_line<'a>(label: &str, value: &str, selected: bool, hint: &str) -> Line<'a> {
    let prefix = if selected { "> " } else { "  " };
    let value_style = if selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    Line::from(vec![
        Span::styled(
            prefix.to_string(),
            if selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
        Span::styled(format!("{:12}", label), Style::default().fg(Color::Yellow)),
        Span::styled(format!("{:20}", value), value_style),
        Span::styled(
            if selected {
                hint.to_string()
            } else {
                String::new()
            },
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// Handle key events for network settings screen
pub fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    use crossterm::event::KeyCode;

    let Some(ref mut ns) = app.network_settings_state else {
        return Ok(());
    };

    if !ns.editing_nic {
        return handle_nic_list_key(app, key);
    }

    // Port forward editor mode
    if ns.editing_port_forwards {
        // Adding a port forward
        if ns.adding_pf.is_some() {
            handle_adding_pf(app, key)?;
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                if let Some(ref mut ns) = app.network_settings_state {
                    ns.editing_port_forwards = false;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut ns) = app.network_settings_state {
                    let count = ns.nics[ns.active_nic].port_forwards.len();
                    if ns.pf_selected < count.saturating_sub(1) {
                        ns.pf_selected += 1;
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut ns) = app.network_settings_state {
                    if ns.pf_selected > 0 {
                        ns.pf_selected -= 1;
                    }
                }
            }
            KeyCode::Char('a') | KeyCode::Enter => {
                if let Some(ref mut ns) = app.network_settings_state {
                    ns.adding_pf = Some(AddingPortForward {
                        step: AddPfStep::Protocol,
                        protocol: PortProtocol::Tcp,
                        host_port_input: String::new(),
                        guest_port_input: String::new(),
                    });
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(ref mut ns) = app.network_settings_state {
                    let active_nic = ns.active_nic;
                    let port_forwards = &mut ns.nics[active_nic].port_forwards;
                    if !port_forwards.is_empty() && ns.pf_selected < port_forwards.len() {
                        port_forwards.remove(ns.pf_selected);
                        if ns.pf_selected >= port_forwards.len() && ns.pf_selected > 0 {
                            ns.pf_selected -= 1;
                        }
                    }
                }
            }
            // Preset shortcuts
            KeyCode::Char('1') => add_preset(app, PortProtocol::Tcp, 2222, 22),
            KeyCode::Char('2') => add_preset(app, PortProtocol::Tcp, 13389, 3389),
            KeyCode::Char('3') => add_preset(app, PortProtocol::Tcp, 8080, 80),
            KeyCode::Char('4') => add_preset(app, PortProtocol::Tcp, 8443, 443),
            KeyCode::Char('5') => add_preset(app, PortProtocol::Tcp, 15900, 5900),
            _ => {}
        }
        return Ok(());
    }

    // MAC edit mode: capture text input first.
    let editing_mac = app
        .network_settings_state
        .as_ref()
        .map(|ns| ns.editing_mac)
        .unwrap_or(false);
    if editing_mac {
        let mut bad_mac: Option<String> = None;
        if let Some(ref mut ns) = app.network_settings_state {
            let active_nic = ns.active_nic;
            match key.code {
                KeyCode::Esc => {
                    ns.mac_edit_buffer = ns.nics[active_nic].mac_address.clone().unwrap_or_default();
                    ns.editing_mac = false;
                }
                KeyCode::Enter => {
                    let trimmed = ns.mac_edit_buffer.trim().to_string();
                    if trimmed.is_empty() {
                        ns.nics[active_nic].mac_address = None;
                        ns.mac_edit_buffer.clear();
                        ns.editing_mac = false;
                    } else if crate::vm::mac::is_valid_mac(&trimmed) {
                        ns.nics[active_nic].mac_address = Some(trimmed.to_lowercase());
                        ns.mac_edit_buffer =
                            ns.nics[active_nic].mac_address.clone().unwrap_or_default();
                        ns.editing_mac = false;
                    } else {
                        bad_mac = Some(trimmed);
                    }
                }
                KeyCode::Backspace => {
                    ns.mac_edit_buffer.pop();
                }
                KeyCode::Char(c) if c.is_ascii_hexdigit() || c == ':' => {
                    if ns.mac_edit_buffer.len() < 17 {
                        ns.mac_edit_buffer.push(c);
                    }
                }
                _ => {}
            }
        }
        if let Some(bad) = bad_mac {
            app.set_status(format!("Invalid MAC address: {}", bad));
        }
        return Ok(());
    }

    // Normal settings mode
    let backend_stops = app.get_network_backend_stops();
    let system_bridges = app.bridge_picker_list();
    let (show_pf, max_field) = {
        let ns = app.network_settings_state.as_ref().unwrap();
        let nic = &ns.nics[ns.active_nic];
        (nic.show_port_forwards(), nic.max_editor_field())
    };

    match key.code {
        KeyCode::Esc => {
            // Prompt before discarding if this session actually changed
            // anything; otherwise there's nothing to lose, so just leave.
            let dirty = app
                .network_settings_state
                .as_ref()
                .map(|ns| ns.nic_snapshot.as_ref() != Some(&ns.nics[ns.active_nic]))
                .unwrap_or(false);
            if dirty {
                app.push_screen(Screen::Confirm(ConfirmAction::UnsavedChanges(
                    UnsavedKind::NicEdit,
                )));
            } else {
                discard_nic_edit(app);
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Save, then return to the NIC list (not close the screen).
            save_nic_edit(app)?;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut ns) = app.network_settings_state {
                if ns.selected_field < max_field {
                    ns.selected_field += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut ns) = app.network_settings_state {
                if ns.selected_field > 0 {
                    ns.selected_field -= 1;
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(ref mut ns) = app.network_settings_state {
                let active_nic = ns.active_nic;
                if ns.selected_field == 2 && ns.nics[active_nic].backend != "none" {
                    let mac = crate::vm::mac::generate_random_mac();
                    ns.nics[active_nic].mac_address = Some(mac.clone());
                    ns.mac_edit_buffer = mac;
                }
            }
        }
        KeyCode::Char('c') => {
            if let Some(ref mut ns) = app.network_settings_state {
                let active_nic = ns.active_nic;
                if ns.selected_field == 2 && ns.nics[active_nic].backend != "none" {
                    ns.nics[active_nic].mac_address = None;
                    ns.mac_edit_buffer.clear();
                }
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            // Cycle the focused field (Right/Tab forward, Left/Shift+Tab back).
            let delta = if matches!(key.code, KeyCode::Right | KeyCode::Tab) {
                1i32
            } else {
                -1i32
            };
            if let Some(ref mut ns) = app.network_settings_state {
                let active_nic = ns.active_nic;
                match ns.selected_field {
                    0 => {
                        // Cycle adapter model
                        cycle_option(&mut ns.nics[active_nic].model, NETWORK_OPTIONS, delta);
                    }
                    1 => {
                        // Cycle backend (expands "bridge" into one stop per
                        // managed vmc-* network, auto-syncing bridge_name)
                        let default_bridge = system_bridges
                            .first()
                            .cloned()
                            .or_else(|| Some("qemubr0".to_string()));
                        ns.nics[active_nic].cycle_backend(&backend_stops, &default_bridge, delta);
                    }
                    3 if ns.nics[active_nic].backend == "bridge" => {
                        // Cycle bridge name
                        if !system_bridges.is_empty() {
                            let current_bridge =
                                ns.nics[active_nic].bridge_name.as_deref().unwrap_or("");
                            let current_idx = system_bridges
                                .iter()
                                .position(|b| b == current_bridge)
                                .unwrap_or(0);
                            let new_idx = (current_idx as i32 + delta)
                                .rem_euclid(system_bridges.len() as i32)
                                as usize;
                            ns.nics[active_nic].bridge_name = Some(system_bridges[new_idx].clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Enter => {
            let (sel, backend) = {
                let ns = app.network_settings_state.as_ref().unwrap();
                (ns.selected_field, ns.nics[ns.active_nic].backend.clone())
            };
            if sel == 2 && backend != "none" {
                // Enter MAC edit mode
                if let Some(ref mut ns) = app.network_settings_state {
                    let active_nic = ns.active_nic;
                    ns.mac_edit_buffer = ns.nics[active_nic].mac_address.clone().unwrap_or_default();
                    ns.editing_mac = true;
                }
            } else if sel == 3 && show_pf {
                // Enter port forward editor
                if let Some(ref mut ns) = app.network_settings_state {
                    ns.editing_port_forwards = true;
                    ns.pf_selected = 0;
                }
            } else {
                // No action on this field — Enter never leaves the editor;
                // only Esc (discard) or [s] Save do.
            }
        }
        _ => {}
    }

    Ok(())
}

/// Handle key events for the top-level NIC list view.
fn handle_nic_list_key(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    use crossterm::event::KeyCode;

    match key.code {
        KeyCode::Esc => {
            let dirty = app
                .network_settings_state
                .as_ref()
                .map(|ns| ns.nics != ns.nics_baseline)
                .unwrap_or(false);
            if dirty {
                app.push_screen(Screen::Confirm(ConfirmAction::UnsavedChanges(
                    UnsavedKind::NicList,
                )));
            } else {
                app.network_settings_state = None;
                app.pop_screen();
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut ns) = app.network_settings_state {
                if ns.list_cursor < ns.nics.len().saturating_sub(1) {
                    ns.list_cursor += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut ns) = app.network_settings_state {
                if ns.list_cursor > 0 {
                    ns.list_cursor -= 1;
                }
            }
        }
        KeyCode::Enter => {
            if let Some(ref mut ns) = app.network_settings_state {
                ns.active_nic = ns.list_cursor;
                // Snapshot so Esc can discard edits made in this session.
                ns.nic_snapshot = Some(ns.nics[ns.active_nic].clone());
                ns.editing_nic = true;
                ns.selected_field = 0;
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            // Add a new NIC and jump straight into editing it.
            if let Some(ref mut ns) = app.network_settings_state {
                ns.nics.push(NicConfig::default());
                ns.active_nic = ns.nics.len() - 1;
                ns.list_cursor = ns.active_nic;
                ns.nic_snapshot = Some(ns.nics[ns.active_nic].clone());
                ns.editing_nic = true;
                ns.selected_field = 0;
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(ref mut ns) = app.network_settings_state {
                if ns.list_cursor < ns.nics.len() && ns.nics.len() > 1 {
                    ns.nics.remove(ns.list_cursor);
                    // Keep active_nic pointing at the same adapter it did
                    // before the removal (or the nearest one left).
                    if ns.active_nic == ns.list_cursor {
                        if ns.active_nic >= ns.nics.len() {
                            ns.active_nic = ns.nics.len() - 1;
                        }
                    } else if ns.active_nic > ns.list_cursor {
                        ns.active_nic -= 1;
                    }
                    if ns.list_cursor >= ns.nics.len() {
                        ns.list_cursor = ns.nics.len() - 1;
                    }
                }
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            apply_network_settings(app, true)?;
        }
        _ => {}
    }

    Ok(())
}

fn handle_adding_pf(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    use crossterm::event::KeyCode;

    let Some(ref mut ns) = app.network_settings_state else {
        return Ok(());
    };
    let Some(ref mut adding) = ns.adding_pf else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => {
            ns.adding_pf = None;
        }
        KeyCode::Enter => match adding.step {
            AddPfStep::Protocol => {
                adding.step = AddPfStep::HostPort;
            }
            AddPfStep::HostPort => {
                if adding.host_port_input.parse::<u16>().is_ok() {
                    adding.step = AddPfStep::GuestPort;
                }
            }
            AddPfStep::GuestPort => {
                if let (Ok(host), Ok(guest)) = (
                    adding.host_port_input.parse::<u16>(),
                    adding.guest_port_input.parse::<u16>(),
                ) {
                    let pf = PortForward {
                        protocol: adding.protocol,
                        host_port: host,
                        guest_port: guest,
                    };
                    ns.nics[ns.active_nic].port_forwards.push(pf);
                    ns.adding_pf = None;
                }
            }
        },
        KeyCode::Left | KeyCode::Right => {
            if adding.step == AddPfStep::Protocol {
                adding.protocol = match adding.protocol {
                    PortProtocol::Tcp => PortProtocol::Udp,
                    PortProtocol::Udp => PortProtocol::Tcp,
                };
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => match adding.step {
            AddPfStep::HostPort => adding.host_port_input.push(c),
            AddPfStep::GuestPort => adding.guest_port_input.push(c),
            _ => {}
        },
        KeyCode::Backspace => match adding.step {
            AddPfStep::HostPort => {
                adding.host_port_input.pop();
            }
            AddPfStep::GuestPort => {
                adding.guest_port_input.pop();
            }
            _ => {}
        },
        _ => {}
    }

    Ok(())
}

fn add_preset(app: &mut App, protocol: PortProtocol, host_port: u16, guest_port: u16) {
    if let Some(ref mut ns) = app.network_settings_state {
        let active_nic = ns.active_nic;
        let port_forwards = &mut ns.nics[active_nic].port_forwards;
        // Don't add duplicate
        if !port_forwards
            .iter()
            .any(|pf| pf.host_port == host_port && pf.guest_port == guest_port)
        {
            port_forwards.push(PortForward {
                protocol,
                host_port,
                guest_port,
            });
        }
    }
}

fn cycle_option(current: &mut String, options: &[&str], delta: i32) {
    let current_idx = options
        .iter()
        .position(|&o| o == current.as_str())
        .unwrap_or(0);
    let new_idx = (current_idx as i32 + delta).rem_euclid(options.len() as i32) as usize;
    *current = options[new_idx].to_string();
}

/// Save `ns.nics` to the VM's launch.sh. When `close_screen` is true, the
/// whole Network Settings screen closes afterwards (used from the NIC
/// list's [s] Save); when false, only the caller's overlay is expected to
/// back out on its own (used from the per-NIC editor's [s] Save, which
/// returns to the NIC list instead of closing entirely).
fn apply_network_settings(app: &mut App, close_screen: bool) -> anyhow::Result<()> {
    let ns = app.network_settings_state.as_ref().unwrap().clone();

    if let Some(vm) = app.selected_vm() {
        let vm_path = vm.path.clone();
        crate::vm::create::update_network_in_script(&vm_path, &ns.nics)?;

        app.reload_selected_vm_script();

        // Re-parse VMs to update config
        if let Ok(vms) = crate::vm::discover_vms(&app.config.vm_library_path) {
            app.vms = vms;
            app.update_filter();
        }

        app.set_status("Network settings updated");
    }

    if close_screen {
        app.network_settings_state = None;
        app.pop_screen();
    } else if let Some(ref mut ns) = app.network_settings_state {
        ns.editing_nic = false;
        ns.nic_snapshot = None;
        // Saved successfully — the list's own unsaved-changes check should
        // no longer see these NICs as dirty.
        ns.nics_baseline = ns.nics.clone();
    }
    Ok(())
}

/// Save the active NIC's edits and return to the NIC list. Used by the
/// per-NIC editor's `[s] Save` key and by the "Save" choice on the
/// discard-confirmation prompt (`ConfirmAction::UnsavedChanges(NicEdit)`).
pub(crate) fn save_nic_edit(app: &mut App) -> anyhow::Result<()> {
    apply_network_settings(app, false)
}

/// Discard the active NIC's in-progress edits (restoring the snapshot
/// taken when its editor was opened) and return to the NIC list. Used by
/// a no-op Esc (nothing changed) and by the "Discard" choice on the
/// confirmation prompt.
pub(crate) fn discard_nic_edit(app: &mut App) {
    if let Some(ref mut ns) = app.network_settings_state {
        let active_nic = ns.active_nic;
        if let Some(snapshot) = ns.nic_snapshot.take() {
            ns.nics[active_nic] = snapshot;
        }
        ns.editing_nic = false;
    }
}

/// Save all pending NIC changes and close the Network Settings screen.
/// Used by the "Save" choice on the NIC list's discard-confirmation
/// prompt (`ConfirmAction::UnsavedChanges(NicList)`).
pub(crate) fn save_nic_list(app: &mut App) -> anyhow::Result<()> {
    apply_network_settings(app, true)
}

/// Throw away all pending NIC changes and close the Network Settings
/// screen without saving. Used by the "Discard" choice on the same prompt.
pub(crate) fn discard_nic_list(app: &mut App) {
    app.network_settings_state = None;
    app.pop_screen();
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
