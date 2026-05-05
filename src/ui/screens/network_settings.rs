//! Network Settings Screen
//!
//! Allows editing network backend, adapter model, and port forwarding
//! on existing VMs from the management menu.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{AddPfStep, AddingPortForward, App, NetworkSettingsState};
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

    // Check if we're in port forward editing mode
    if ns.editing_port_forwards {
        render_port_forward_editor(app, ns, frame, inner);
        return;
    }

    let is_bridge = ns.backend == "bridge";
    let show_mac = ns.backend != "none";

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),   // Header
            Constraint::Length(1),   // Spacer
            Constraint::Length(1),   // Adapter field
            Constraint::Length(1),   // Backend field
            Constraint::Length(1),   // MAC field
            Constraint::Length(1),   // Bridge name / Port forwards field
            Constraint::Length(1),   // Spacer
            Constraint::Min(6),      // Info area
            Constraint::Length(2),   // Help
        ])
        .split(inner);

    // Header
    let header = Paragraph::new("Configure VM Networking")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Adapter model
    let adapter_selected = ns.selected_field == 0;
    let adapter_line = render_field_line("Adapter:", &ns.model, adapter_selected, "[Left/Right] cycle");
    frame.render_widget(Paragraph::new(adapter_line), chunks[2]);

    // Backend
    let backend_selected = ns.selected_field == 1;
    let backend_display = match ns.backend.as_str() {
        "user" => "user/SLIRP (NAT)".to_string(),
        "passt" => "passt".to_string(),
        "bridge" => format!("bridge ({})", ns.bridge_name.as_deref().unwrap_or("qemubr0")),
        "none" => "none".to_string(),
        other => other.to_string(),
    };
    let backend_line = render_field_line("Backend:", &backend_display, backend_selected, "[Left/Right] cycle");
    frame.render_widget(Paragraph::new(backend_line), chunks[3]);

    // MAC address (hidden when backend == "none")
    if show_mac {
        let mac_selected = ns.selected_field == 2;
        let mac_display = if ns.editing_mac {
            format!("{}_", ns.mac_edit_buffer)
        } else if let Some(mac) = ns.mac_address.as_deref() {
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
    let show_pf = ns.backend == "user" || ns.backend == "passt";
    let bridge_pf_selected = ns.selected_field == 3;
    if is_bridge {
        let bridge_display = ns.bridge_name.as_deref().unwrap_or("qemubr0");
        let bridge_line = render_field_line("Bridge:", bridge_display, bridge_pf_selected, "[Left/Right] cycle");
        frame.render_widget(Paragraph::new(bridge_line), chunks[5]);
    } else if show_pf {
        let pf_count = ns.port_forwards.len();
        let pf_display = if pf_count == 0 {
            "none".to_string()
        } else {
            format!("{} rule(s)", pf_count)
        };
        let pf_hint = if bridge_pf_selected { "[Enter] edit" } else { "" };
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
        let helper_color = if caps.bridge_helper_path.is_some() { Color::Green } else { Color::Red };
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
        let perm_color = if caps.bridge_helper_configured { Color::Green } else { Color::Red };
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
        let bridges_color = if caps.system_bridges.is_empty() { Color::Red } else { Color::Green };
        lines.push(Line::from(vec![
            Span::styled("  Bridges:       ", Style::default().fg(Color::Yellow)),
            Span::styled(bridges_str, Style::default().fg(bridges_color)),
        ]));

        // Setup guidance if incomplete
        if caps.bridge_helper_path.is_none() || !caps.bridge_helper_configured || caps.system_bridges.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::styled("  Setup needed:", Style::default().fg(Color::Yellow)));
            if caps.bridge_helper_path.is_none() {
                lines.push(Line::styled("    Install: qemu-bridge-helper (part of QEMU)", Style::default().fg(Color::DarkGray)));
            }
            if !caps.bridge_helper_configured {
                lines.push(Line::styled("    Run: sudo setcap cap_net_admin+ep /usr/lib/qemu/qemu-bridge-helper", Style::default().fg(Color::DarkGray)));
            }
            if caps.system_bridges.is_empty() {
                lines.push(Line::styled("    Create bridge: sudo ip link add qemubr0 type bridge", Style::default().fg(Color::DarkGray)));
                lines.push(Line::styled("    Enable:        sudo ip link set qemubr0 up", Style::default().fg(Color::DarkGray)));
            }
        }

        let info = Paragraph::new(lines);
        frame.render_widget(info, chunks[7]);
    } else if show_pf && !ns.port_forwards.is_empty() {
        let mut lines = Vec::new();
        lines.push(Line::styled("  Current port forwarding rules:", Style::default().fg(Color::DarkGray)));
        for pf in &ns.port_forwards {
            lines.push(Line::from(format!("    {} {} -> {}", pf.protocol, pf.host_port, pf.guest_port)));
        }
        let list = Paragraph::new(lines);
        frame.render_widget(list, chunks[7]);
    }

    // Help
    let help = Paragraph::new("[Enter] Apply  [Esc] Cancel  [j/k] Navigate  [Left/Right] Change")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[8]);
}

/// Render the port forward editor overlay
fn render_port_forward_editor(_app: &App, ns: &NetworkSettingsState, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),   // Header
            Constraint::Length(1),   // Spacer
            Constraint::Min(8),      // Rules list
            Constraint::Length(1),   // Spacer
            Constraint::Length(1),   // Presets
            Constraint::Length(2),   // Help
        ])
        .split(area);

    // Check if we're adding a port forward
    if let Some(ref adding) = ns.adding_pf {
        render_adding_pf(adding, frame, area);
        return;
    }

    let header = Paragraph::new("Port Forwarding Rules")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Rules list
    if ns.port_forwards.is_empty() {
        let msg = Paragraph::new("  No port forwarding rules configured.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, chunks[2]);
    } else {
        let mut lines = Vec::new();
        for (i, pf) in ns.port_forwards.iter().enumerate() {
            let is_selected = i == ns.pf_selected;
            let prefix = if is_selected { "> " } else { "  " };
            let style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::styled(
                format!("{}{}  {} -> {}", prefix, pf.protocol, pf.host_port, pf.guest_port),
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
            Constraint::Length(1),   // Header
            Constraint::Length(1),   // Spacer
            Constraint::Length(1),   // Protocol
            Constraint::Length(1),   // Host port
            Constraint::Length(1),   // Guest port
            Constraint::Min(3),      // Spacer
            Constraint::Length(2),   // Help
        ])
        .split(area);

    let header = Paragraph::new("Add Port Forward Rule")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Protocol
    let proto_active = adding.step == AddPfStep::Protocol;
    let proto_style = if proto_active {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let proto_hint = if proto_active { " [Left/Right] toggle" } else { "" };
    let proto_line = Line::from(vec![
        Span::styled("  Protocol: ", Style::default().fg(Color::Yellow)),
        Span::styled(format!("{}", adding.protocol), proto_style),
        Span::styled(proto_hint, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(proto_line), chunks[2]);

    // Host port
    let host_active = adding.step == AddPfStep::HostPort;
    let host_style = if host_active {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let host_line = Line::from(vec![
        Span::styled("  Host Port: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            if adding.host_port_input.is_empty() { "_" } else { &adding.host_port_input },
            host_style,
        ),
    ]);
    frame.render_widget(Paragraph::new(host_line), chunks[3]);

    // Guest port
    let guest_active = adding.step == AddPfStep::GuestPort;
    let guest_style = if guest_active {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let guest_line = Line::from(vec![
        Span::styled("  Guest Port: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            if adding.guest_port_input.is_empty() { "_" } else { &adding.guest_port_input },
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
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    Line::from(vec![
        Span::styled(prefix.to_string(), if selected { Style::default().fg(Color::Yellow) } else { Style::default() }),
        Span::styled(format!("{:12}", label), Style::default().fg(Color::Yellow)),
        Span::styled(format!("{:20}", value), value_style),
        Span::styled(if selected { hint.to_string() } else { String::new() }, Style::default().fg(Color::DarkGray)),
    ])
}

/// Handle key events for network settings screen
pub fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    use crossterm::event::KeyCode;

    let Some(ref mut ns) = app.network_settings_state else {
        return Ok(());
    };

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
                    if ns.pf_selected < ns.port_forwards.len().saturating_sub(1) {
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
                    if !ns.port_forwards.is_empty() && ns.pf_selected < ns.port_forwards.len() {
                        ns.port_forwards.remove(ns.pf_selected);
                        if ns.pf_selected >= ns.port_forwards.len() && ns.pf_selected > 0 {
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
            match key.code {
                KeyCode::Esc => {
                    ns.mac_edit_buffer = ns.mac_address.clone().unwrap_or_default();
                    ns.editing_mac = false;
                }
                KeyCode::Enter => {
                    let trimmed = ns.mac_edit_buffer.trim().to_string();
                    if trimmed.is_empty() {
                        ns.mac_address = None;
                        ns.mac_edit_buffer.clear();
                        ns.editing_mac = false;
                    } else if crate::vm::mac::is_valid_mac(&trimmed) {
                        ns.mac_address = Some(trimmed.to_lowercase());
                        ns.mac_edit_buffer = ns.mac_address.clone().unwrap_or_default();
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
    let backend_options: Vec<String> = app.get_network_backend_options()
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    let system_bridges = app.network_caps.system_bridges.clone();
    let show_pf = {
        let ns = app.network_settings_state.as_ref().unwrap();
        ns.backend == "user" || ns.backend == "passt"
    };
    let is_bridge = {
        let ns = app.network_settings_state.as_ref().unwrap();
        ns.backend == "bridge"
    };
    let show_mac = {
        let ns = app.network_settings_state.as_ref().unwrap();
        ns.backend != "none"
    };
    // Field indices: 0=adapter, 1=backend, 2=mac (when show_mac), 3=bridge/forwards
    let max_field = if !show_mac {
        1
    } else if show_pf || is_bridge {
        3
    } else {
        2
    };

    match key.code {
        KeyCode::Esc => {
            app.network_settings_state = None;
            app.pop_screen();
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
                if ns.selected_field == 2 && ns.backend != "none" {
                    let mac = crate::vm::mac::generate_random_mac();
                    ns.mac_address = Some(mac.clone());
                    ns.mac_edit_buffer = mac;
                }
            }
        }
        KeyCode::Char('c') => {
            if let Some(ref mut ns) = app.network_settings_state {
                if ns.selected_field == 2 && ns.backend != "none" {
                    ns.mac_address = None;
                    ns.mac_edit_buffer.clear();
                }
            }
        }
        KeyCode::Left | KeyCode::Right => {
            let delta = if key.code == KeyCode::Right { 1i32 } else { -1i32 };
            if let Some(ref mut ns) = app.network_settings_state {
                match ns.selected_field {
                    0 => {
                        // Cycle adapter model
                        cycle_option(&mut ns.model, NETWORK_OPTIONS, delta);
                    }
                    1 => {
                        // Cycle backend
                        let current_idx = backend_options.iter()
                            .position(|b| b == &ns.backend)
                            .unwrap_or(0);
                        let new_idx = (current_idx as i32 + delta)
                            .rem_euclid(backend_options.len() as i32) as usize;
                        ns.backend = backend_options[new_idx].clone();

                        // Set default bridge name
                        if ns.backend == "bridge" && ns.bridge_name.is_none() {
                            ns.bridge_name = system_bridges.first().cloned()
                                .or_else(|| Some("qemubr0".to_string()));
                        }
                    }
                    3 if ns.backend == "bridge" => {
                        // Cycle bridge name
                        if !system_bridges.is_empty() {
                            let current_bridge = ns.bridge_name.as_deref().unwrap_or("");
                            let current_idx = system_bridges.iter()
                                .position(|b| b == current_bridge)
                                .unwrap_or(0);
                            let new_idx = (current_idx as i32 + delta)
                                .rem_euclid(system_bridges.len() as i32) as usize;
                            ns.bridge_name = Some(system_bridges[new_idx].clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Enter => {
            let (sel, backend) = {
                let ns = app.network_settings_state.as_ref().unwrap();
                (ns.selected_field, ns.backend.clone())
            };
            if sel == 2 && backend != "none" {
                // Enter MAC edit mode
                if let Some(ref mut ns) = app.network_settings_state {
                    ns.mac_edit_buffer = ns.mac_address.clone().unwrap_or_default();
                    ns.editing_mac = true;
                }
            } else if sel == 3 && show_pf {
                // Enter port forward editor
                if let Some(ref mut ns) = app.network_settings_state {
                    ns.editing_port_forwards = true;
                    ns.pf_selected = 0;
                }
            } else {
                // Apply changes
                apply_network_settings(app)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_adding_pf(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    use crossterm::event::KeyCode;

    let Some(ref mut ns) = app.network_settings_state else { return Ok(()) };
    let Some(ref mut adding) = ns.adding_pf else { return Ok(()) };

    match key.code {
        KeyCode::Esc => {
            ns.adding_pf = None;
        }
        KeyCode::Enter => {
            match adding.step {
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
                        ns.port_forwards.push(pf);
                        ns.adding_pf = None;
                    }
                }
            }
        }
        KeyCode::Left | KeyCode::Right => {
            if adding.step == AddPfStep::Protocol {
                adding.protocol = match adding.protocol {
                    PortProtocol::Tcp => PortProtocol::Udp,
                    PortProtocol::Udp => PortProtocol::Tcp,
                };
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            match adding.step {
                AddPfStep::HostPort => adding.host_port_input.push(c),
                AddPfStep::GuestPort => adding.guest_port_input.push(c),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match adding.step {
                AddPfStep::HostPort => { adding.host_port_input.pop(); }
                AddPfStep::GuestPort => { adding.guest_port_input.pop(); }
                _ => {}
            }
        }
        _ => {}
    }

    Ok(())
}

fn add_preset(app: &mut App, protocol: PortProtocol, host_port: u16, guest_port: u16) {
    if let Some(ref mut ns) = app.network_settings_state {
        // Don't add duplicate
        if !ns.port_forwards.iter().any(|pf| pf.host_port == host_port && pf.guest_port == guest_port) {
            ns.port_forwards.push(PortForward { protocol, host_port, guest_port });
        }
    }
}

fn cycle_option(current: &mut String, options: &[&str], delta: i32) {
    let current_idx = options.iter().position(|&o| o == current.as_str()).unwrap_or(0);
    let new_idx = (current_idx as i32 + delta).rem_euclid(options.len() as i32) as usize;
    *current = options[new_idx].to_string();
}

/// Apply network settings changes to the VM's launch.sh
fn apply_network_settings(app: &mut App) -> anyhow::Result<()> {
    let ns = app.network_settings_state.as_ref().unwrap().clone();

    if let Some(vm) = app.selected_vm() {
        let vm_path = vm.path.clone();
        crate::vm::create::update_network_in_script(
            &vm_path,
            &ns.model,
            &ns.backend,
            ns.bridge_name.as_deref(),
            &ns.port_forwards,
            ns.mac_address.as_deref(),
        )?;

        app.reload_selected_vm_script();

        // Re-parse VMs to update config
        if let Ok(vms) = crate::vm::discover_vms(&app.config.vm_library_path) {
            app.vms = vms;
            app.update_filter();
        }

        app.set_status("Network settings updated");
    }

    app.network_settings_state = None;
    app.pop_screen();
    Ok(())
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
