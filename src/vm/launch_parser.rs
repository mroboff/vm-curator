use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::qemu_config::*;
use crate::commands::qemu_img;

/// Parse a launch.sh script and extract QEMU configuration
pub fn parse_launch_script(script_path: &Path, content: &str) -> Result<QemuConfig> {
    let mut config = QemuConfig {
        raw_script: content.to_string(),
        ..Default::default()
    };

    let vm_dir = script_path.parent().unwrap_or(Path::new("."));

    // Extract emulator
    if let Some(emulator) = extract_emulator(content) {
        config.emulator = emulator;
    }

    // Extract memory
    if let Some(mem) = extract_memory(content) {
        config.memory_mb = mem;
    }

    // Extract CPU cores
    if let Some(cores) = extract_cpu_cores(content) {
        config.cpu_cores = cores;
    }

    // Extract CPU model
    config.cpu_model = extract_cpu_model(content);

    // Extract machine type
    config.machine = extract_machine(content);

    // Extract VGA
    if let Some(vga) = extract_vga(content) {
        config.vga = vga;
    }

    // Extract audio devices
    config.audio_devices = extract_audio_devices(content);

    // Check for KVM
    config.enable_kvm = content.contains("-enable-kvm") || content.contains("-accel kvm");

    // Check for UEFI
    config.uefi = content.contains("OVMF") || content.contains("-bios") && content.contains("efi");

    // Check for TPM
    config.tpm = content.contains("-tpmdev") || content.contains("swtpm");

    // Extract BIOS/ROM path (for classic Mac and other custom firmware)
    config.bios_path = extract_bios_path(content, vm_dir);

    // Extract disks
    config.disks = extract_disks(content, vm_dir);

    // Extract network config
    config.networks = extract_networks(content);

    // Extract extra arguments we don't specifically parse
    config.extra_args = extract_extra_args(content);

    Ok(config)
}

/// Extract the QEMU emulator command
fn extract_emulator(content: &str) -> Option<QemuEmulator> {
    let emulators = [
        "qemu-system-x86_64",
        "qemu-system-i386",
        "qemu-system-ppc",
        "qemu-system-m68k",
        "qemu-system-arm",
        "qemu-system-aarch64",
    ];

    for emulator in emulators {
        if content.contains(emulator) {
            return Some(QemuEmulator::from_command(emulator));
        }
    }
    None
}

/// Extract memory configuration
fn extract_memory(content: &str) -> Option<u32> {
    for line in content.lines() {
        // Skip comments
        if line.trim_start().starts_with('#') {
            continue;
        }

        // Look for -m flag
        if let Some(idx) = line.find("-m ") {
            let rest = &line[idx + 3..];
            let value: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(mem) = value.parse::<u32>() {
                // Check for G suffix
                if rest.contains('G') {
                    return Some(mem * 1024);
                }
                // If less than 64, probably gigabytes
                if mem < 64 {
                    return Some(mem * 1024);
                }
                return Some(mem);
            }
        }
    }
    None
}

/// Extract CPU cores
fn extract_cpu_cores(content: &str) -> Option<u32> {
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        // Look for -smp
        if let Some(idx) = line.find("-smp ") {
            let rest = &line[idx + 5..];
            let value: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(cores) = value.parse::<u32>() {
                return Some(cores);
            }
        }
    }
    None
}

/// Extract CPU model
fn extract_cpu_model(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        if let Some(idx) = line.find("-cpu ") {
            let rest = &line[idx + 5..];
            let model: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '\\')
                .collect();
            if !model.is_empty() {
                return Some(model);
            }
        }
    }
    None
}

/// Extract machine type
fn extract_machine(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        if let Some(idx) = line.find("-M ") {
            let rest = &line[idx + 3..];
            let machine: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '\\')
                .collect();
            if !machine.is_empty() {
                return Some(machine);
            }
        }

        if let Some(idx) = line.find("-machine ") {
            let rest = &line[idx + 9..];
            let machine: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ',' && *c != '\\')
                .collect();
            if !machine.is_empty() {
                return Some(machine);
            }
        }
    }
    None
}

/// Extract VGA type
fn extract_vga(content: &str) -> Option<VgaType> {
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        if let Some(idx) = line.find("-vga ") {
            let rest = &line[idx + 5..];
            let vga: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '\\' && *c != ')')
                .collect();
            if !vga.is_empty() {
                return Some(VgaType::from_str(&vga));
            }
        }
    }
    None
}

/// Extract audio devices
fn extract_audio_devices(content: &str) -> Vec<AudioDevice> {
    let mut devices = Vec::new();

    // Check for SoundBlaster 16
    if content.contains("sb16") || content.contains("SB16") {
        devices.push(AudioDevice::Sb16);
    }

    // Check for AC97
    if content.contains("ac97") || content.contains("AC97") {
        devices.push(AudioDevice::Ac97);
    }

    // Check for Intel HDA
    if content.contains("intel-hda") || content.contains("hda-duplex") {
        devices.push(AudioDevice::Hda);
    }

    // Check for ES1370
    if content.contains("es1370") {
        devices.push(AudioDevice::Es1370);
    }

    devices
}

/// Extract shell variable assignments from the script
fn extract_shell_variables(content: &str, vm_dir: &Path) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    // Pre-populate with common directory variables
    let vm_dir_str = vm_dir.to_string_lossy().to_string();
    vars.insert("VM_DIR".to_string(), vm_dir_str.clone());
    vars.insert("DIR".to_string(), vm_dir_str.clone());

    // Parse variable assignments like: VAR="value" or VAR='value' or VAR=value
    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // Look for variable assignments (NAME=value pattern)
        if let Some(eq_pos) = trimmed.find('=') {
            let name = trimmed[..eq_pos].trim();

            // Variable names must be valid shell identifiers
            if !name.is_empty()
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !name.chars().next().unwrap_or('0').is_ascii_digit()
            {
                let value_part = trimmed[eq_pos + 1..].trim();

                // Extract the value, handling quotes with proper nesting
                let value = extract_quoted_value(value_part);

                // Expand any variables in the value
                let expanded = expand_variables(&value, &vars, vm_dir);
                vars.insert(name.to_string(), expanded);
            }
        }
    }

    vars
}

/// Extract a quoted value, handling nested quotes and command substitutions
fn extract_quoted_value(s: &str) -> String {
    if s.starts_with('"') {
        // Find the matching closing quote, accounting for nested quotes in $()
        let chars: Vec<char> = s.chars().collect();
        let mut depth = 0;
        let mut end_idx = s.len() - 1;

        for (i, &c) in chars.iter().enumerate().skip(1) {
            match c {
                '(' if i > 0 && chars[i - 1] == '$' => depth += 1,
                ')' if depth > 0 => depth -= 1,
                '"' if depth == 0 => {
                    end_idx = i;
                    break;
                }
                _ => {}
            }
        }

        s[1..end_idx].to_string()
    } else if let Some(stripped) = s.strip_prefix('\'') {
        // Single quotes don't nest - find first closing quote
        if let Some(end) = stripped.find('\'') {
            stripped[..end].to_string()
        } else {
            stripped.to_string()
        }
    } else {
        // Unquoted value - take until whitespace or comment
        s.chars()
            .take_while(|c| !c.is_whitespace() && *c != '#')
            .collect()
    }
}

/// Expand shell variables in a string
fn expand_variables(s: &str, vars: &HashMap<String, String>, vm_dir: &Path) -> String {
    let mut result = s.to_string();
    let vm_dir_str = vm_dir.to_string_lossy();

    // Handle $(dirname ...) patterns - replace with vm_dir
    while result.contains("$(dirname") {
        if let Some(start) = result.find("$(dirname") {
            // Find matching closing paren
            let mut depth = 0;
            let mut end = start;
            for (i, c) in result[start..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end > start {
                result = format!("{}{}{}", &result[..start], vm_dir_str, &result[end + 1..]);
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Expand ${VAR} format
    for (name, value) in vars {
        result = result.replace(&format!("${{{}}}", name), value);
    }

    // Expand $VAR format (must be done after ${VAR} to avoid partial matches)
    for (name, value) in vars {
        result = result.replace(&format!("${}", name), value);
    }

    // Handle $HOME
    if result.contains("$HOME") || result.contains("${HOME}") {
        if let Some(home) = dirs::home_dir() {
            let home_str = home.to_string_lossy();
            result = result.replace("${HOME}", &home_str);
            result = result.replace("$HOME", &home_str);
        }
    }

    result
}

/// Extract the primary `qemu-system-*` invocation from the script, including
/// its backslash-continuation lines.
///
/// vm-curator's own generated launch.sh files wrap several invocations (one
/// per boot mode: `--install`, `--cdrom`, `--recovery`, `--floppy`, and the
/// default normal boot) in a `case "$1" in ... esac`. The default/no-args
/// branch - the configuration vm-curator treats as canonical - is always the
/// last invocation emitted, right before the catch-all `*)` branch. Scoping
/// per-invocation parsing (like disks) to that last block, rather than the
/// whole file, avoids mixing in install/cdrom/recovery drives (e.g. `-drive
/// file="$2"`) that belong to other branches.
fn extract_primary_command_block(content: &str) -> &str {
    let emulators = [
        "qemu-system-x86_64",
        "qemu-system-i386",
        "qemu-system-ppc",
        "qemu-system-m68k",
        "qemu-system-arm",
        "qemu-system-aarch64",
    ];

    // Byte ranges of each line (newline included) so the block can be sliced
    // back out of `content` without reconstructing the text.
    let mut lines = Vec::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        lines.push((offset, offset + line.len()));
        offset += line.len();
    }

    let mut start_idx = None;
    for (i, &(s, e)) in lines.iter().enumerate() {
        let trimmed = content[s..e].trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if emulators.iter().any(|em| trimmed.starts_with(em)) {
            start_idx = Some(i);
        }
    }

    let Some(start_idx) = start_idx else {
        return content;
    };

    let mut end_idx = start_idx;
    while content[lines[end_idx].0..lines[end_idx].1]
        .trim_end()
        .ends_with('\\')
        && end_idx + 1 < lines.len()
    {
        end_idx += 1;
    }

    &content[lines[start_idx].0..lines[end_idx].1]
}

/// Extract disk configurations
fn extract_disks(content: &str, vm_dir: &Path) -> Vec<DiskConfig> {
    let mut disks = Vec::new();

    // Parse variable assignments from the whole script - DISK/ISO/etc. are
    // typically assigned once near the top, outside any invocation block.
    let vars = extract_shell_variables(content, vm_dir);

    let block = extract_primary_command_block(content);

    for line in block.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        // Look for -hda, -hdb, etc.
        for hd in ["hda", "hdb", "hdc", "hdd"] {
            let pattern = format!("-{} ", hd);
            if let Some(idx) = line.find(&pattern) {
                let rest = &line[idx + pattern.len()..];
                if let Some(path) = extract_path_from_arg(rest) {
                    let expanded = expand_variables(&path, &vars, vm_dir);
                    let full_path = resolve_path(&expanded, vm_dir);
                    let format = guess_disk_format(&full_path);
                    disks.push(DiskConfig {
                        path: full_path,
                        format,
                        interface: "ide".to_string(),
                        role: DiskRole::System,
                    });
                }
            }
        }

        // Look for -drive file=
        if line.contains("-drive") && line.contains("file=") {
            if let Some(path) = extract_drive_file(line) {
                let expanded = expand_variables(&path, &vars, vm_dir);
                let full_path = resolve_path(&expanded, vm_dir);
                let role = if line.contains("if=pflash") {
                    DiskRole::Firmware
                } else if line.contains("media=cdrom") {
                    DiskRole::Media
                } else {
                    DiskRole::System
                };
                let format = guess_disk_format(&full_path);
                let interface = if line.contains("if=virtio") {
                    "virtio"
                } else if line.contains("if=scsi") {
                    "scsi"
                } else {
                    "ide"
                };
                disks.push(DiskConfig {
                    path: full_path,
                    format,
                    interface: interface.to_string(),
                    role,
                });
            }
        }
    }

    disks
}

/// Extract file path from -drive file= argument
fn extract_drive_file(line: &str) -> Option<String> {
    if let Some(idx) = line.find("file=") {
        let rest = &line[idx + 5..];
        // Handle quoted paths
        if let Some(inner) = rest.strip_prefix('"') {
            let end = inner.find('"')?;
            return Some(inner[..end].to_string());
        }
        // Handle unquoted paths
        let path: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ',' && *c != '\\')
            .collect();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

/// Extract a path from an argument
fn extract_path_from_arg(arg: &str) -> Option<String> {
    let trimmed = arg.trim();
    if let Some(inner) = trimmed.strip_prefix('"') {
        let end = inner.find('"')?;
        return Some(inner[..end].to_string());
    }
    if let Some(inner) = trimmed.strip_prefix('\'') {
        let end = inner.find('\'')?;
        return Some(inner[..end].to_string());
    }
    let path: String = trimmed
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '\\')
        .collect();
    if !path.is_empty() && !path.starts_with('-') {
        Some(path)
    } else {
        None
    }
}

/// Resolve a path relative to VM directory
fn resolve_path(path: &str, vm_dir: &Path) -> PathBuf {
    let path = path.replace("$DIR", &vm_dir.to_string_lossy());
    let path = path.replace("${DIR}", &vm_dir.to_string_lossy());
    let path = path.replace("$(dirname $0)", &vm_dir.to_string_lossy());

    let p = PathBuf::from(&path);
    if p.is_absolute() {
        p
    } else {
        vm_dir.join(p)
    }
}

/// Detect disk format using qemu-img info, falling back to extension-based guessing
fn guess_disk_format(path: &Path) -> DiskFormat {
    // Physical block devices are always raw; don't probe them with qemu-img
    // (the device node may not be readable by the current user).
    if path.starts_with("/dev") {
        return DiskFormat::Raw;
    }

    // First, try to detect the actual format using qemu-img info
    if path.exists() {
        if let Some(format_str) = qemu_img::detect_disk_format(path) {
            return match format_str.to_lowercase().as_str() {
                "qcow2" => DiskFormat::Qcow2,
                "raw" => DiskFormat::Raw,
                "vmdk" => DiskFormat::Vmdk,
                "vdi" => DiskFormat::Vdi,
                other => DiskFormat::Other(other.to_string()),
            };
        }
    }

    // Fall back to extension-based detection if qemu-img fails or file doesn't exist
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(DiskFormat::from_extension)
        .unwrap_or(DiskFormat::Raw)
}

/// Extract the numeric suffix following `marker` in `line` (e.g. `marker =
/// "id=net"` on `-netdev user,id=net1,...` yields `Some(1)`).
fn parse_net_index(line: &str, marker: &str) -> Option<usize> {
    let idx = line.find(marker)?;
    let rest = &line[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Extract a `br=<name>` bridge name from a `-netdev`/`-net bridge` line.
fn extract_bridge_name(line: &str) -> Option<String> {
    let idx = line.find("br=")?;
    let rest = &line[idx + 3..];
    Some(
        rest.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect(),
    )
}

/// Extract a `mac=<addr>` MAC address from a `-device`/`-netdev`/`-nic` line.
fn extract_mac(line: &str) -> Option<String> {
    let idx = line.find("mac=")?;
    let rest = &line[idx + 4..];
    let mac: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == ':')
        .collect();
    if crate::vm::mac::is_valid_mac(&mac) {
        Some(mac.to_lowercase())
    } else {
        None
    }
}

/// Apply a `-netdev` line's backend/bridge/port-forwards onto `net`.
fn apply_netdev_line(net: &mut NetworkConfig, line: &str) {
    if line.contains("passt") {
        net.backend = NetworkBackend::Passt;
        net.user_net = false;
    } else if line.contains("bridge") {
        net.user_net = false;
        let bridge = extract_bridge_name(line).unwrap_or_else(|| "qemubr0".to_string());
        net.backend = NetworkBackend::Bridge(bridge.clone());
        net.bridge = Some(bridge);
    } else if line.contains("user") {
        net.user_net = true;
        net.backend = NetworkBackend::User;
        net.port_forwards = extract_port_forwards(line);
    }
}

/// Apply a `-device` line's adapter model onto `net`.
fn apply_device_line(net: &mut NetworkConfig, line: &str) {
    if line.contains("virtio-net") {
        net.model = "virtio-net".to_string();
    } else if line.contains("e1000") {
        net.model = "e1000".to_string();
    } else if line.contains("rtl8139") {
        net.model = "rtl8139".to_string();
    } else if line.contains("ne2k_pci") {
        net.model = "ne2k_pci".to_string();
    } else if line.contains("pcnet") {
        net.model = "pcnet".to_string();
    }
}

/// Extract network configuration for every NIC in the script.
///
/// The generator emits one `-netdev ...,id=netN,...` + `-device
/// ...,netdev=netN[,mac=...]` pair per adapter, so those are grouped by
/// their `N` into one `NetworkConfig` each, in order. Older/hand-written
/// scripts using `-net user`/`-net bridge`/`-nic ...` (no `id=netN` tag)
/// fall back to the pre-multi-NIC behavior of a single adapter at index 0.
fn extract_networks(content: &str) -> Vec<NetworkConfig> {
    use std::collections::BTreeMap;

    let mut by_id: BTreeMap<usize, NetworkConfig> = BTreeMap::new();
    let mut legacy = NetworkConfig::default();
    let mut has_legacy = false;

    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        if line.contains("-netdev") {
            if let Some(id) = parse_net_index(line, "id=net") {
                let net = by_id.entry(id).or_default();
                apply_netdev_line(net, line);
                if let Some(mac) = extract_mac(line) {
                    net.mac_address = Some(mac);
                }
                continue;
            }

            // -netdev line whose id doesn't follow this app's own
            // `netN` numbering (e.g. a hand-edited script) — still
            // extract its backend/bridge/port-forwards, just as a
            // single legacy adapter rather than an indexed one.
            apply_netdev_line(&mut legacy, line);
            if let Some(mac) = extract_mac(line) {
                legacy.mac_address = Some(mac);
            }
            has_legacy = true;
            continue;
        }

        if line.contains("-device") {
            if let Some(id) = parse_net_index(line, "netdev=net") {
                let net = by_id.entry(id).or_default();
                apply_device_line(net, line);
                if let Some(mac) = extract_mac(line) {
                    net.mac_address = Some(mac);
                }
                continue;
            }

            // Legacy -device line for a NIC without an id=netN tag.
            let before = legacy.model.clone();
            apply_device_line(&mut legacy, line);
            if legacy.model != before {
                has_legacy = true;
            }
            if let Some(mac) = extract_mac(line) {
                legacy.mac_address = Some(mac);
                has_legacy = true;
            }
        }

        // Check for network model via -net nic
        if line.contains("-net nic") || line.contains("-nic") {
            has_legacy = true;

            if line.contains("model=virtio") {
                legacy.model = "virtio-net".to_string();
            } else if line.contains("model=e1000") {
                legacy.model = "e1000".to_string();
            } else if line.contains("model=rtl8139") {
                legacy.model = "rtl8139".to_string();
            }
        }

        // Check for -net user/bridge (legacy format)
        if line.contains("-net user") {
            has_legacy = true;
            legacy.user_net = true;
            legacy.backend = NetworkBackend::User;
            legacy.port_forwards.extend(extract_port_forwards(line));
        }

        if line.contains("-net bridge") {
            has_legacy = true;
            legacy.user_net = false;
            if let Some(bridge) = extract_bridge_name(line) {
                legacy.backend = NetworkBackend::Bridge(bridge.clone());
                legacy.bridge = Some(bridge);
            }
        }

        // Extract MAC address for legacy lines. QEMU accepts `mac=...` on
        // either the -device line (most common) or the -netdev/-nic line.
        if (line.contains("-device") || line.contains("-netdev") || line.contains("-nic"))
            && line.contains("mac=")
        {
            if let Some(mac) = extract_mac(line) {
                legacy.mac_address = Some(mac);
                has_legacy = true;
            }
        }
    }

    let results: Vec<NetworkConfig> = by_id.into_values().collect();
    if !results.is_empty() {
        return results;
    }

    if has_legacy || content.contains("-net") || content.contains("-nic") {
        vec![legacy]
    } else {
        Vec::new()
    }
}

/// Extract port forwarding rules from a hostfwd string
fn extract_port_forwards(line: &str) -> Vec<PortForward> {
    let mut forwards = Vec::new();

    // Find each hostfwd= segment
    let mut search_from = 0;
    while let Some(idx) = line[search_from..].find("hostfwd=") {
        let start = search_from + idx + 8; // skip "hostfwd="
        let rest = &line[start..];

        // Format: protocol::hostport-:guestport
        // Or: protocol:addr:hostport-:guestport
        let segment: String = rest
            .chars()
            .take_while(|c| *c != ',' && !c.is_whitespace() && *c != '\\')
            .collect();

        if let Some(pf) = parse_hostfwd_segment(&segment) {
            forwards.push(pf);
        }

        search_from = start + segment.len();
    }

    forwards
}

/// Parse a single hostfwd segment like "tcp::2222-:22"
fn parse_hostfwd_segment(segment: &str) -> Option<PortForward> {
    // Split on the dash separator between host and guest
    let parts: Vec<&str> = segment.splitn(2, '-').collect();
    if parts.len() != 2 {
        return None;
    }

    let host_part = parts[0]; // "tcp::2222" or "tcp:addr:2222"
    let guest_part = parts[1]; // ":22" or ":addr:22"

    // Parse protocol from the beginning
    let protocol = if host_part.starts_with("udp") {
        PortProtocol::Udp
    } else {
        PortProtocol::Tcp
    };

    // Extract host port (last number in host_part after protocol)
    let host_port: u16 = host_part.rsplit(':').next()?.parse().ok()?;

    // Extract guest port (last number in guest_part)
    let guest_port: u16 = guest_part.rsplit(':').next()?.parse().ok()?;

    Some(PortForward {
        protocol,
        host_port,
        guest_port,
    })
}

/// Extract BIOS/ROM path from -bios argument
///
/// Parses lines like `-bios "$ROM"` and resolves shell variables.
/// Filters out OVMF/EFI paths (those are handled by UEFI detection).
fn extract_bios_path(content: &str, vm_dir: &Path) -> Option<PathBuf> {
    let vars = extract_shell_variables(content, vm_dir);

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        if let Some(idx) = trimmed.find("-bios ") {
            let rest = &trimmed[idx + 6..];
            let raw_path = if let Some(inner) = rest.strip_prefix('"') {
                // Quoted path like -bios "$ROM"
                if let Some(end) = inner.find('"') {
                    inner[..end].to_string()
                } else {
                    inner.to_string()
                }
            } else {
                // Unquoted path
                rest.chars()
                    .take_while(|c| !c.is_whitespace() && *c != '\\')
                    .collect()
            };

            // Expand shell variables
            let expanded = expand_variables(&raw_path, &vars, vm_dir);
            let path = resolve_path(&expanded, vm_dir);

            // Filter out OVMF/EFI paths - those are UEFI firmware, not BIOS ROMs
            let path_str = path.to_string_lossy().to_lowercase();
            if path_str.contains("ovmf") || path_str.contains("efi") || path_str.contains("uefi") {
                continue;
            }

            return Some(path);
        }
    }
    None
}

/// Extract extra arguments we don't specifically handle
fn extract_extra_args(content: &str) -> Vec<String> {
    let mut args = Vec::new();

    // Look for display settings generically (handles gtk, sdl, vnc, spice-app, etc.)
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(idx) = line.find("-display ") {
            let rest = &line[idx + 9..];
            // Extract the display backend (supports hyphenated names like spice-app)
            let backend: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-')
                .collect();
            if !backend.is_empty() {
                args.push(format!("-display {}", backend));
                break;
            }
        }
    }

    // Look for USB
    if content.contains("-usb") {
        args.push("-usb".to_string());
    }

    // Look for RTC settings
    if content.contains("-rtc base=localtime") {
        args.push("-rtc base=localtime".to_string());
    }

    args
}

#[cfg(test)]
#[path = "tests/launch_parser.rs"]
mod tests;
