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
    } else if app.config.enable_multi_gpu_passthrough {
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
    let help_text = if app.config.enable_multi_gpu_passthrough && !app.config.single_gpu_enabled {
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
    } else if app.config.enable_multi_gpu_passthrough {
        // Multi-GPU passthrough mode - show full status
        let status = app.multi_gpu_status.as_ref();
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
            if app.config.enable_multi_gpu_passthrough {
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
    let dialog_height = 26.min(area.height.saturating_sub(4));

    let dialog_area = centered_rect(dialog_width, dialog_height, area);
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" GPU Passthrough Prerequisites ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let status = app.multi_gpu_status.as_ref();

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

        // VFIO binding info
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "VFIO Driver Binding:",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw("  Devices are automatically bound to vfio-pci at launch"));
        lines.push(Line::raw("  and restored to their original driver on VM exit."));
        lines.push(Line::styled(
            "  Requires authentication via pkexec (polkit) or sudo.",
            Style::default().fg(Color::Yellow),
        ));

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

    let gpu_enabled = app.config.enable_multi_gpu_passthrough;

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
            app.selected_menu_item = 0; // Reset for management menu
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
                        format!("Saved {} PCI device(s) to launch.sh (will use pkexec/sudo for VFIO binding)", count)
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
            app.multi_gpu_status = Some(crate::hardware::check_multi_gpu_passthrough_status());
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

    let has_gpus = devices.iter().any(|d| d.is_gpu());

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

    // Generate PCI device list for VFIO binding
    section.push_str("PCI_DEVICES=(");
    for (i, dev) in devices.iter().enumerate() {
        if i > 0 {
            section.push(' ');
        }
        section.push_str(&format!("\"{}\"", dev.address));
    }
    section.push_str(")\n");
    section.push_str("declare -A PCI_ORIG_DRIVERS\n");

    if has_gpus {
        section.push_str("PCI_GPU_USED_RESCAN=0\n");
    }
    section.push('\n');

    // Helper to run sysfs commands with privilege escalation
    section.push_str(r#"# Run a command with elevated privileges if needed
_pci_elevated() {
    if [[ $EUID -eq 0 ]]; then
        sh -c "$1"
    elif command -v pkexec >/dev/null 2>&1; then
        pkexec sh -c "$1"
    elif command -v sudo >/dev/null 2>&1; then
        sudo sh -c "$1"
    else
        echo "Error: Root privileges required to bind PCI devices to vfio-pci"
        echo "Install polkit (pkexec) or run with sudo"
        return 1
    fi
}
"#);

    // GPU release/restore functions (only when GPUs are selected)
    if has_gpus {
        section.push_str(&generate_gpu_release_function());
        section.push_str(&generate_gpu_restore_function());
    }

    // VFIO bind function
    section.push_str(&generate_bind_vfio_function(has_gpus));

    // VFIO restore function
    section.push_str(&generate_restore_pci_function(has_gpus));

    // Hook restore_pci into exit cleanup, then bind devices
    // The trap/cleanup setup must happen before bind_vfio so partially-bound
    // devices get restored if binding fails partway through
    section.push_str(r#"if declare -f cleanup >/dev/null 2>&1; then
    eval "$(declare -f cleanup | sed '1s/cleanup/_pci_pre_cleanup/')"
    cleanup() { restore_pci; _pci_pre_cleanup; }
else
    trap 'restore_pci' EXIT
fi
bind_vfio || exit 1
"#);

    section.push_str(PCI_MARKER_END);
    section.push('\n');

    section
}

/// Generate the _release_gpu() bash function for compositor GPU release.
///
/// Uses a two-tier approach:
/// - Tier 1: Send fake udev "remove" event to compositor via DRM subsystem
/// - Tier 2: PCI remove/rescan (forceful but clean)
fn generate_gpu_release_function() -> String {
    r#"# Release a GPU from the compositor before VFIO binding.
# Tier 1: Fake udev remove event (least disruptive, works with Mutter/KWin/wlroots).
# Tier 2: PCI remove + rescan with driver_override (forceful but clean).
_release_gpu() {
    local dev="$1"
    local dev_path="/sys/bus/pci/devices/$dev"
    local driver_link="$dev_path/driver"

    # Only needed if bound to a GPU driver
    if [[ ! -L "$driver_link" ]]; then
        return 0
    fi
    local current
    current=$(basename "$(readlink "$driver_link")")
    case "$current" in
        nvidia|amdgpu|nouveau|radeon|i915|xe) ;;
        *) return 0 ;;  # Not a GPU driver, standard unbind is fine
    esac

    echo "Releasing GPU $dev from compositor (driver: $current)..."

    # Stop nvidia-persistenced for NVIDIA GPUs (holds device references)
    if [[ "$current" == "nvidia" ]]; then
        _pci_elevated "systemctl stop nvidia-persistenced 2>/dev/null || true" 2>/dev/null
        sleep 0.5
    fi

    # Tier 1: Send fake udev "remove" event via DRM subsystem
    local drm_card=""
    for card in "$dev_path"/drm/card*; do
        if [[ -d "$card" ]]; then
            drm_card="$card"
            break
        fi
    done

    if [[ -n "$drm_card" ]]; then
        echo "  Tier 1: Sending udev remove event via $(basename "$drm_card")..."
        _pci_elevated "echo 'remove' > '$drm_card/uevent'" 2>/dev/null
        sleep 2

        # Try standard unbind now that compositor should have released
        _pci_elevated "echo '$dev' > '$driver_link/unbind'" 2>/dev/null
        sleep 0.5

        # Check if unbind succeeded
        if [[ ! -L "$driver_link" ]]; then
            echo "  GPU $dev released successfully (Tier 1)"
            return 0
        fi
        echo "  Tier 1 did not release GPU, trying Tier 2..."
    fi

    # Tier 2: PCI remove + rescan with driver_override
    echo "  Tier 2: PCI remove/rescan for $dev..."
    _pci_elevated "echo 'vfio-pci' > '$dev_path/driver_override'; echo 1 > '$dev_path/remove'" 2>/dev/null
    sleep 2

    # Rescan PCI bus - device will bind to vfio-pci due to driver_override
    _pci_elevated "echo 1 > /sys/bus/pci/rescan" 2>/dev/null
    sleep 2

    # Verify the device came back and bound to vfio-pci
    if [[ -L "$dev_path/driver" ]]; then
        local new_driver
        new_driver=$(basename "$(readlink "$dev_path/driver")")
        if [[ "$new_driver" == "vfio-pci" ]]; then
            echo "  GPU $dev bound to vfio-pci (Tier 2)"
            PCI_GPU_USED_RESCAN=1
            return 0
        fi
    fi

    # Both tiers failed
    echo ""
    echo "ERROR: Could not release GPU $dev from the compositor."
    echo ""
    echo "The Wayland compositor is holding the GPU. Options:"
    echo "  1. Add 'vfio-pci.ids=$(cat "$dev_path/vendor" | sed 's/0x//'):$(cat "$dev_path/device" | sed 's/0x//')' to kernel parameters for boot-time binding"
    echo "  2. Use the Single GPU Passthrough workflow (stops the display manager)"
    echo ""
    return 1
}
"#.to_string()
}

/// Generate the _restore_gpu() bash function for GPU restoration after VM exit.
///
/// Reverses the GPU release: unbinds from vfio-pci, rescans to reclaim with
/// original driver, notifies compositor, restarts services.
fn generate_gpu_restore_function() -> String {
    r#"# Restore a GPU after VM exit: rebind to original driver and notify compositor.
_restore_gpu() {
    local dev="$1"
    local orig="$2"
    local dev_path="/sys/bus/pci/devices/$dev"

    echo "Restoring GPU $dev to $orig..."

    # Unbind from vfio-pci and clear driver_override
    local restore_cmds=""
    restore_cmds+="echo '$dev' > '$dev_path/driver/unbind' 2>/dev/null; "
    restore_cmds+="echo '' > '$dev_path/driver_override'; "

    # Use PCI remove/rescan for reliable driver reclaim
    restore_cmds+="echo 1 > '$dev_path/remove' 2>/dev/null || true; "
    restore_cmds+="sleep 2; "
    restore_cmds+="echo 1 > /sys/bus/pci/rescan; "
    restore_cmds+="sleep 2; "

    # Send udev "add" event to notify compositor of GPU return
    restore_cmds+="for card in /sys/bus/pci/devices/$dev/drm/card*; do "
    restore_cmds+="  if [[ -d \"\\\$card\" ]]; then "
    restore_cmds+="    echo 'add' > \"\\\$card/uevent\" 2>/dev/null; "
    restore_cmds+="  fi; "
    restore_cmds+="done; "

    # Restart nvidia-persistenced for NVIDIA GPUs
    if [[ "$orig" == "nvidia" ]]; then
        restore_cmds+="systemctl start nvidia-persistenced 2>/dev/null || true; "
    fi

    if [[ $EUID -eq 0 ]]; then
        sh -c "$restore_cmds"
    elif sudo -n true 2>/dev/null; then
        sudo sh -c "$restore_cmds"
    elif command -v pkexec >/dev/null 2>&1; then
        pkexec sh -c "$restore_cmds" 2>/dev/null
    else
        echo "Warning: Could not restore GPU $dev (no cached credentials)"
    fi
}
"#.to_string()
}

/// Generate the bind_vfio() bash function.
/// When `has_gpus` is true, calls `_release_gpu()` per-device before batched unbind.
fn generate_bind_vfio_function(has_gpus: bool) -> String {
    let mut f = String::new();
    f.push_str("bind_vfio() {\n");

    if has_gpus {
        // First pass: release GPUs from compositor (per-device, before batched unbind)
        f.push_str(r#"    # Release GPUs from compositor first
    for dev in "${PCI_DEVICES[@]}"; do
        local dev_path="/sys/bus/pci/devices/$dev"
        if [[ ! -d "$dev_path" ]]; then
            continue
        fi
        if [[ -L "$dev_path/driver" ]]; then
            local current
            current=$(basename "$(readlink "$dev_path/driver")")
            PCI_ORIG_DRIVERS[$dev]="$current"
            if [[ "$current" == "vfio-pci" ]]; then
                continue
            fi
        fi
        _release_gpu "$dev" || return 1
    done
"#);
    }

    // Standard bind pass: unbind remaining non-GPU devices and bind all to vfio-pci
    f.push_str(r#"    local bind_cmds=""
    for dev in "${PCI_DEVICES[@]}"; do
        local dev_path="/sys/bus/pci/devices/$dev"
        local driver_link="$dev_path/driver"
        if [[ ! -d "$dev_path" ]]; then
            echo "Warning: PCI device $dev not found, skipping"
            continue
        fi
        if [[ -L "$driver_link" ]]; then
            local current
            current=$(basename "$(readlink "$driver_link")")
"#);

    if has_gpus {
        // Record driver if not already recorded in GPU release pass
        f.push_str(r#"            [[ -z "${PCI_ORIG_DRIVERS[$dev]:-}" ]] && PCI_ORIG_DRIVERS[$dev]="$current"
"#);
    } else {
        f.push_str(r#"            PCI_ORIG_DRIVERS[$dev]="$current"
"#);
    }

    f.push_str(r#"            if [[ "$current" == "vfio-pci" ]]; then
                echo "$dev already bound to vfio-pci"
                continue
            fi
            bind_cmds+="echo '$dev' > '$driver_link/unbind' 2>/dev/null; sleep 0.1; "
        fi
        bind_cmds+="echo 'vfio-pci' > '$dev_path/driver_override'; "
        bind_cmds+="echo '$dev' > /sys/bus/pci/drivers_probe; "
    done
    if [[ -n "$bind_cmds" ]]; then
        echo "Binding PCI devices to vfio-pci..."
        _pci_elevated "$bind_cmds" || return 1
    fi
    sleep 0.5
}
"#);

    f
}

/// Generate the restore_pci() bash function.
/// When `has_gpus` is true, uses `_restore_gpu()` for GPU devices.
fn generate_restore_pci_function(has_gpus: bool) -> String {
    let mut f = String::new();

    if has_gpus {
        f.push_str(r#"restore_pci() {
    local restore_cmds=""
    for dev in "${PCI_DEVICES[@]}"; do
        local dev_path="/sys/bus/pci/devices/$dev"
        local orig="${PCI_ORIG_DRIVERS[$dev]:-}"
        if [[ -z "$orig" ]] || [[ "$orig" == "vfio-pci" ]]; then
            continue
        fi
        # Check if original driver was a GPU driver
        case "$orig" in
            nvidia|amdgpu|nouveau|radeon|i915|xe)
                _restore_gpu "$dev" "$orig"
                continue
                ;;
        esac
        echo "Restoring $dev to $orig..."
        restore_cmds+="echo '$dev' > '$dev_path/driver/unbind' 2>/dev/null; "
        restore_cmds+="echo '' > '$dev_path/driver_override'; "
        restore_cmds+="echo '$dev' > /sys/bus/pci/drivers_probe; "
    done
    if [[ -n "$restore_cmds" ]]; then
        if [[ $EUID -eq 0 ]]; then
            sh -c "$restore_cmds"
        elif sudo -n true 2>/dev/null; then
            sudo sh -c "$restore_cmds"
        elif command -v pkexec >/dev/null 2>&1; then
            pkexec sh -c "$restore_cmds" 2>/dev/null
        else
            echo "Warning: Could not restore PCI devices (no cached credentials)"
            echo "Devices will be restored on next reboot, or run: sudo modprobe -r vfio-pci"
        fi
    fi
}
"#);
    } else {
        f.push_str(r#"restore_pci() {
    local restore_cmds=""
    for dev in "${PCI_DEVICES[@]}"; do
        local dev_path="/sys/bus/pci/devices/$dev"
        local orig="${PCI_ORIG_DRIVERS[$dev]:-}"
        if [[ -z "$orig" ]] || [[ "$orig" == "vfio-pci" ]]; then
            continue
        fi
        echo "Restoring $dev to $orig..."
        restore_cmds+="echo '$dev' > '$dev_path/driver/unbind' 2>/dev/null; "
        restore_cmds+="echo '' > '$dev_path/driver_override'; "
        restore_cmds+="echo '$dev' > /sys/bus/pci/drivers_probe; "
    done
    if [[ -n "$restore_cmds" ]]; then
        if [[ $EUID -eq 0 ]]; then
            sh -c "$restore_cmds"
        elif sudo -n true 2>/dev/null; then
            sudo sh -c "$restore_cmds"
        elif command -v pkexec >/dev/null 2>&1; then
            pkexec sh -c "$restore_cmds" 2>/dev/null
        else
            echo "Warning: Could not restore PCI devices (no cached credentials)"
            echo "Devices will be restored on next reboot, or run: sudo modprobe -r vfio-pci"
        fi
    fi
}
"#);
    }

    f
}

fn insert_pci_section(content: &str, pci_section: &str) -> String {
    crate::vm::lifecycle::insert_args_section(content, pci_section, "$PCI_PASSTHROUGH_ARGS")
}
