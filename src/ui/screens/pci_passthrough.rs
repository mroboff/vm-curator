//! PCI Passthrough Screen
//!
//! Displays PCI devices for passthrough selection with special handling for GPUs.
//! Shows IOMMU groups, driver bindings, and prerequisite status.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::App;
use crate::hardware::PciDevice;

/// Render the PCI passthrough screen
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Calculate dialog size - larger to accommodate device list
    let dialog_width = 85.min(area.width.saturating_sub(4));
    let dialog_height = 28.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);

    // Clear the background
    frame.render_widget(Clear, dialog_area);

    let selected_count = app.selected_pci_devices.len();
    let title = if app.config.single_gpu_enabled {
        // Single GPU mode - don't show GPU count (boot VGA is passed through differently)
        format!(" PCI Passthrough ({} selected) ", selected_count)
    } else if app.config.enable_gpu_passthrough {
        let gpu_count = app.pci_devices.iter().filter(|d| d.is_gpu() && !d.is_boot_vga).count();
        format!(
            " PCI Passthrough ({} selected, {} GPU{} available) ",
            selected_count,
            gpu_count,
            if gpu_count == 1 { "" } else { "s" }
        )
    } else {
        format!(" PCI Passthrough ({} selected) ", selected_count)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Layout: status bar, device list, help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Status bar
            Constraint::Min(8),    // Device list
            Constraint::Length(2), // Help text
        ])
        .split(inner);

    // Render status bar
    render_status_bar(app, frame, chunks[0]);

    // Add horizontal margins for device list
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(1),  // Left margin
            Constraint::Min(1),     // Content
            Constraint::Length(1),  // Right margin
        ])
        .split(chunks[1]);

    // Render device list
    render_device_list(app, frame, h_chunks[1]);

    // Help text - show GPU options only when multi-GPU passthrough is enabled (not single GPU)
    let help_text = if app.config.enable_gpu_passthrough && !app.config.single_gpu_enabled {
        "[Space/Enter] Toggle  [g] Auto-select GPU  [s] Save  [p] Prerequisites  [Esc] Back"
    } else {
        "[Space/Enter] Toggle  [s] Save  [Esc] Back"
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[2]);
}

/// Render the status bar showing prerequisite status
fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    // Handle different GPU passthrough modes
    let spans = if app.config.single_gpu_enabled {
        // Single GPU passthrough mode
        vec![
            Span::styled(" Mode: ", Style::default().fg(Color::White)),
            Span::styled("Single GPU Passthrough", Style::default().fg(Color::Cyan)),
            Span::styled("  (GPU selection managed via Single GPU Setup)", Style::default().fg(Color::DarkGray)),
        ]
    } else if app.config.enable_gpu_passthrough {
        // Multi-GPU passthrough mode - show full status
        let status = app.gpu_status.as_ref();
        let (status_text, status_color) = if let Some(status) = status {
            if status.is_ready() {
                (status.summary(), Color::Green)
            } else {
                (status.summary(), Color::Yellow)
            }
        } else {
            ("Status unknown".to_string(), Color::DarkGray)
        };

        let mut spans = vec![
            Span::styled(" Status: ", Style::default().fg(Color::White)),
            Span::styled(status_text, Style::default().fg(status_color)),
        ];

        // Add quick indicators
        if let Some(status) = status {
            spans.push(Span::raw("  |  "));
            spans.push(Span::styled(
                "IOMMU",
                Style::default().fg(if status.iommu_enabled { Color::Green } else { Color::Red }),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                "VFIO",
                Style::default().fg(if status.vfio_loaded { Color::Green } else { Color::Red }),
            ));
        }
        spans
    } else {
        // GPU passthrough disabled - show simple message
        vec![
            Span::styled(" Select PCI devices to pass through to the VM", Style::default().fg(Color::DarkGray)),
        ]
    };

    let status_para = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::DarkGray)));

    frame.render_widget(status_para, area);
}

/// Render the device list
fn render_device_list(app: &App, frame: &mut Frame, area: Rect) {
    if app.pci_devices.is_empty() {
        let msg = Paragraph::new("No PCI devices found.\n\nEnsure you have permission to read /sys/bus/pci/devices.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    // Filter devices to show useful passthrough candidates
    let relevant_devices: Vec<(usize, &PciDevice)> = app
        .pci_devices
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            // Always show useful passthrough candidates (USB, network, storage, audio)
            if d.is_passthrough_candidate() {
                return true;
            }
            // When GPU passthrough is enabled, also show GPUs and GPU-related devices
            if app.config.enable_gpu_passthrough {
                d.is_gpu()
                    || d.is_audio()
                    || d.iommu_group.is_some() && is_device_in_gpu_group(d, &app.pci_devices)
            } else {
                false
            }
        })
        .collect();

    let items: Vec<ListItem> = relevant_devices
        .iter()
        .map(|(original_idx, device)| {
            let selected = app.selected_pci_devices.contains(original_idx);
            let is_current = *original_idx == app.selected_menu_item;

            // Build the display line
            let checkbox = if selected { "[X]" } else { "[ ]" };

            // Determine device color based on type
            let device_color = if device.is_boot_vga {
                Color::Red // Boot VGA - cannot select
            } else if device.is_gpu() {
                Color::Cyan // GPU - highlight
            } else if device.is_audio() {
                Color::Magenta // Audio device
            } else {
                Color::White
            };

            // Build device info string
            let mut info_parts = vec![device.short_vendor().to_string()];

            if !device.device_name.is_empty() {
                info_parts.push(device.device_name.clone());
            } else {
                info_parts.push(device.class_description().to_string());
            }

            let device_info = info_parts.join(" ");

            // Driver binding
            let driver_info = device
                .driver
                .as_ref()
                .map(|d| format!("[{}]", d))
                .unwrap_or_else(|| "[no driver]".to_string());

            let driver_color = if device.is_vfio_bound() {
                Color::Green
            } else {
                Color::DarkGray
            };

            // IOMMU group
            let iommu_info = device
                .iommu_group
                .map(|g| format!("IOMMU:{}", g))
                .unwrap_or_else(|| "no IOMMU".to_string());

            // Build the line with proper formatting
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", checkbox),
                    if is_current {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else if selected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::styled(
                    format!("{:<12} ", device.address),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<40} ", truncate_str(&device_info, 40)),
                    Style::default().fg(device_color),
                ),
                Span::styled(
                    format!("{:<12} ", driver_info),
                    Style::default().fg(driver_color),
                ),
                Span::styled(
                    iommu_info,
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            // Add boot VGA warning
            let mut lines = vec![line];
            if device.is_boot_vga {
                lines.push(Line::styled(
                    "     (Boot VGA - cannot be passed through)",
                    Style::default().fg(Color::Red).add_modifier(Modifier::ITALIC),
                ));
            }

            ListItem::new(lines)
        })
        .collect();

    // Map selected_menu_item to the filtered list index
    let list_selected = relevant_devices
        .iter()
        .position(|(idx, _)| *idx == app.selected_menu_item);

    let mut state = ListState::default();
    state.select(list_selected);

    let list = List::new(items)
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(list, area, &mut state);
}

/// Render the prerequisites screen
#[allow(dead_code)]
pub fn render_prerequisites(app: &App, frame: &mut Frame) {
    let area = frame.area();

    let dialog_width = 70.min(area.width.saturating_sub(4));
    let dialog_height = 22.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" GPU Passthrough Prerequisites ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let status = app.gpu_status.as_ref();

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::styled(
        "GPU Passthrough Prerequisites",
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));

    if let Some(status) = status {
        // IOMMU check
        let iommu_icon = if status.iommu_enabled { " OK " } else { "FAIL" };
        let iommu_color = if status.iommu_enabled { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", iommu_icon), Style::default().fg(iommu_color)),
            Span::raw("IOMMU enabled in kernel"),
        ]));
        if !status.iommu_enabled {
            lines.push(Line::styled(
                "    Add intel_iommu=on or amd_iommu=on to kernel parameters",
                Style::default().fg(Color::Yellow),
            ));
        }

        // VFIO check
        let vfio_icon = if status.vfio_loaded { " OK " } else { "FAIL" };
        let vfio_color = if status.vfio_loaded { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", vfio_icon), Style::default().fg(vfio_color)),
            Span::raw("VFIO modules loaded"),
        ]));
        if !status.vfio_loaded {
            lines.push(Line::styled(
                "    Run: sudo modprobe vfio-pci",
                Style::default().fg(Color::Yellow),
            ));
        }

        // GPU availability
        let gpu_available = !status.passthrough_gpus.is_empty();
        let gpu_icon = if gpu_available { " OK " } else { "FAIL" };
        let gpu_color = if gpu_available { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", gpu_icon), Style::default().fg(gpu_color)),
            Span::raw(format!(
                "Secondary GPU available ({} found)",
                status.passthrough_gpus.len()
            )),
        ]));

        if let Some(ref boot_vga) = status.boot_vga {
            lines.push(Line::styled(
                format!("    Boot VGA: {} (cannot be passed through)", boot_vga.display_name()),
                Style::default().fg(Color::DarkGray),
            ));
        }

        for gpu in &status.passthrough_gpus {
            let driver_status = if gpu.is_vfio_bound() {
                "vfio-pci"
            } else {
                gpu.driver.as_deref().unwrap_or("no driver")
            };
            lines.push(Line::styled(
                format!("    {} [{}]", gpu.display_name(), driver_status),
                Style::default().fg(Color::Cyan),
            ));
        }

        // Warnings
        if !status.warnings.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("Warnings:", Style::default().fg(Color::Yellow)));
            for warning in &status.warnings {
                lines.push(Line::styled(format!("  - {}", warning), Style::default().fg(Color::Yellow)));
            }
        }

        // Looking Glass info
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "For Looking Glass:",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw("  - Install looking-glass-client on host"));
        lines.push(Line::raw("  - Install Looking Glass Host in guest VM"));
        lines.push(Line::raw("  - Connect HDMI/DP dummy plug to guest GPU"));
    } else {
        lines.push(Line::styled(
            "Unable to check prerequisites",
            Style::default().fg(Color::Red),
        ));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "[Press any key to close]",
        Style::default().fg(Color::DarkGray),
    ));

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

/// Check if a device is in the same IOMMU group as any GPU
fn is_device_in_gpu_group(device: &PciDevice, all_devices: &[PciDevice]) -> bool {
    if let Some(group) = device.iommu_group {
        for d in all_devices {
            if d.is_gpu() && d.iommu_group == Some(group) && d.address != device.address {
                return true;
            }
        }
    }
    false
}

/// Truncate a string to max_len characters, adding "..." if truncated
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

/// Handle key input for PCI passthrough screen
pub fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    use crossterm::event::KeyCode;

    let gpu_enabled = app.config.enable_gpu_passthrough;

    // Get relevant devices for navigation (must match render filter)
    let relevant_indices: Vec<usize> = app
        .pci_devices
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            // Always include useful passthrough candidates
            if d.is_passthrough_candidate() {
                return true;
            }
            // When GPU passthrough is enabled, also include GPUs and GPU-related devices
            if gpu_enabled {
                d.is_gpu()
                    || d.is_audio()
                    || d.iommu_group.is_some() && is_device_in_gpu_group(d, &app.pci_devices)
            } else {
                false
            }
        })
        .map(|(i, _)| i)
        .collect();

    match key.code {
        KeyCode::Esc => {
            app.pop_screen();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            // Find current position in relevant devices
            if let Some(current_pos) = relevant_indices.iter().position(|&i| i == app.selected_menu_item) {
                if current_pos < relevant_indices.len().saturating_sub(1) {
                    app.selected_menu_item = relevant_indices[current_pos + 1];
                }
            } else if !relevant_indices.is_empty() {
                app.selected_menu_item = relevant_indices[0];
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(current_pos) = relevant_indices.iter().position(|&i| i == app.selected_menu_item) {
                if current_pos > 0 {
                    app.selected_menu_item = relevant_indices[current_pos - 1];
                }
            } else if !relevant_indices.is_empty() {
                app.selected_menu_item = relevant_indices[0];
            }
        }
        KeyCode::Char(' ') | KeyCode::Enter => {
            app.toggle_pci_device(app.selected_menu_item);
        }
        KeyCode::Char('g') | KeyCode::Char('G') if gpu_enabled => {
            // Auto-select current GPU and its audio pair
            if let Some(device) = app.pci_devices.get(app.selected_menu_item) {
                if device.is_gpu() && !device.is_boot_vga {
                    app.auto_select_gpu(app.selected_menu_item);
                }
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Save PCI passthrough configuration
            let save_result = save_pci_passthrough(app);
            match save_result {
                Ok(count) => {
                    let mut status_msg = if count > 0 {
                        format!("Saved {} PCI device(s) to launch.sh", count)
                    } else {
                        "Cleared PCI passthrough from launch.sh".to_string()
                    };
                    // Reload script
                    app.reload_selected_vm_script();

                    // Regenerate single-GPU scripts if they exist
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
                    app.set_status(format!("Error saving PCI config: {}", e));
                }
            }
        }
        KeyCode::Char('p') | KeyCode::Char('P') if gpu_enabled => {
            // Refresh and show GPU prerequisites
            app.gpu_status = Some(crate::hardware::check_gpu_passthrough_status());
            // We'll need a separate screen state for prerequisites
            // For now, just refresh the status
            app.set_status("GPU prerequisites refreshed");
        }
        _ => {}
    }

    Ok(())
}

/// Save PCI passthrough configuration to the VM's launch.sh
fn save_pci_passthrough(app: &App) -> anyhow::Result<usize> {
    let vm = app
        .selected_vm()
        .ok_or_else(|| anyhow::anyhow!("No VM selected"))?;

    let devices: Vec<&PciDevice> = app
        .selected_pci_devices
        .iter()
        .filter_map(|&i| app.pci_devices.get(i))
        .collect();

    let count = devices.len();

    // Read current launch.sh
    let script_path = &vm.launch_script;
    let content = std::fs::read_to_string(script_path)?;

    // Remove existing PCI passthrough section
    let content = remove_pci_section(&content);

    // Generate new PCI passthrough section
    let pci_section = generate_pci_section(&devices);

    // Insert the new section
    let new_content = insert_pci_section(&content, &pci_section);

    // Write back
    std::fs::write(script_path, new_content)?;

    Ok(count)
}

// Markers for PCI passthrough section in launch.sh
const PCI_MARKER_START: &str = "# >>> PCI Passthrough (managed by vm-curator) >>>";
const PCI_MARKER_END: &str = "# <<< PCI Passthrough <<<";

fn remove_pci_section(content: &str) -> String {
    let mut result = String::new();
    let mut in_pci_section = false;

    for line in content.lines() {
        if line.trim() == PCI_MARKER_START {
            in_pci_section = true;
            continue;
        }
        if line.trim() == PCI_MARKER_END {
            in_pci_section = false;
            continue;
        }
        if !in_pci_section {
            // Also remove any $PCI_PASSTHROUGH_ARGS references
            let cleaned_line = line
                .replace(" $PCI_PASSTHROUGH_ARGS", "")
                .replace("$PCI_PASSTHROUGH_ARGS ", "")
                .replace("$PCI_PASSTHROUGH_ARGS", "");
            result.push_str(&cleaned_line);
            result.push('\n');
        }
    }

    while result.ends_with("\n\n") {
        result.pop();
    }

    result
}

fn generate_pci_section(devices: &[&PciDevice]) -> String {
    if devices.is_empty() {
        return String::new();
    }

    let mut section = String::new();
    section.push_str(PCI_MARKER_START);
    section.push('\n');

    // Generate the passthrough args
    let args = crate::hardware::generate_passthrough_args(
        &devices.iter().map(|d| (*d).clone()).collect::<Vec<_>>()
    );

    section.push_str("PCI_PASSTHROUGH_ARGS=\"");
    section.push_str(&args.join(" "));
    section.push_str("\"\n");
    section.push_str(PCI_MARKER_END);
    section.push('\n');

    section
}

fn insert_pci_section(content: &str, pci_section: &str) -> String {
    if pci_section.is_empty() {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();
    let mut inserted = false;

    // Find ALL QEMU commands in the script (there may be multiple in a case statement)
    // Each entry is (start_idx, end_idx) for a QEMU command
    let mut qemu_commands: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_qemu_line = (trimmed.starts_with("qemu-system-")
            || trimmed.starts_with("exec qemu-system-")
            || trimmed.starts_with("\"$QEMU\"")
            || trimmed.starts_with("$QEMU "))
            && !trimmed.starts_with('#');

        if is_qemu_line {
            let start_idx = i;
            // Find the end of this QEMU command (the line without trailing \)
            while i < lines.len() {
                let line_trimmed = lines[i].trim();
                if !line_trimmed.ends_with('\\') {
                    break;
                }
                i += 1;
            }
            let end_idx = i;
            qemu_commands.push((start_idx, end_idx));
        }
        i += 1;
    }

    // Track which end lines we need to modify
    let qemu_end_indices: std::collections::HashSet<usize> = qemu_commands.iter().map(|(_, end)| *end).collect();

    // Get the first QEMU command start for inserting the section
    let first_qemu_start = qemu_commands.first().map(|(start, _)| *start);

    for (i, line) in lines.iter().enumerate() {
        // Insert PCI section before the FIRST qemu command
        if Some(i) == first_qemu_start && !inserted {
            result.push_str(pci_section);
            result.push('\n');
            inserted = true;
        }

        // Modify ALL QEMU command endings to include $PCI_PASSTHROUGH_ARGS
        if qemu_end_indices.contains(&i) {
            let trimmed = line.trim_end();
            if let Some(comment_pos) = trimmed.find(" #") {
                let (cmd, comment) = trimmed.split_at(comment_pos);
                result.push_str(cmd);
                result.push_str(" $PCI_PASSTHROUGH_ARGS");
                result.push_str(comment);
            } else {
                result.push_str(trimmed);
                result.push_str(" $PCI_PASSTHROUGH_ARGS");
            }
            result.push('\n');
            continue;
        }

        result.push_str(line);
        if i < lines.len() - 1 {
            result.push('\n');
        }
    }

    // If we didn't find a qemu line, append at the end
    if !inserted {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(pci_section);
    }

    // Ensure file ends with newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    result
}
