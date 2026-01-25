use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::qemu_config::*;
use crate::commands::qemu_img;

/// Parse a launch.sh script and extract QEMU configuration
pub fn parse_launch_script(script_path: &Path, content: &str) -> Result<QemuConfig> {
    let mut config = QemuConfig::default();
    config.raw_script = content.to_string();

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

    // Extract disks
    config.disks = extract_disks(content, vm_dir);

    // Extract network config
    config.network = extract_network(content);

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
                .take_while(|c| !c.is_whitespace() && *c != '\\')
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
    } else if s.starts_with('\'') {
        // Single quotes don't nest - find first closing quote
        if let Some(end) = s[1..].find('\'') {
            s[1..=end].to_string()
        } else {
            s[1..].to_string()
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

/// Extract disk configurations
fn extract_disks(content: &str, vm_dir: &Path) -> Vec<DiskConfig> {
    let mut disks = Vec::new();

    // First, parse all variable assignments
    let vars = extract_shell_variables(content, vm_dir);

    for line in content.lines() {
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
                    });
                }
            }
        }

        // Look for -drive file=
        if line.contains("-drive") && line.contains("file=") {
            if let Some(path) = extract_drive_file(line) {
                let expanded = expand_variables(&path, &vars, vm_dir);
                let full_path = resolve_path(&expanded, vm_dir);
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
        if rest.starts_with('"') {
            let end = rest[1..].find('"')?;
            return Some(rest[1..=end].to_string());
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
    if trimmed.starts_with('"') {
        let end = trimmed[1..].find('"')?;
        return Some(trimmed[1..=end].to_string());
    }
    if trimmed.starts_with('\'') {
        let end = trimmed[1..].find('\'')?;
        return Some(trimmed[1..=end].to_string());
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
fn guess_disk_format(path: &PathBuf) -> DiskFormat {
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

/// Extract network configuration
fn extract_network(content: &str) -> Option<NetworkConfig> {
    let mut config = NetworkConfig::default();
    let mut has_network = false;

    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }

        // Check for network model
        if line.contains("-net nic") || line.contains("-netdev") || line.contains("-nic") {
            has_network = true;

            if line.contains("model=virtio") {
                config.model = "virtio-net".to_string();
            } else if line.contains("model=e1000") {
                config.model = "e1000".to_string();
            } else if line.contains("model=rtl8139") {
                config.model = "rtl8139".to_string();
            }
        }

        // Check for user networking
        if line.contains("-net user") || line.contains("user,") {
            config.user_net = true;
        }

        // Check for bridge
        if line.contains("-net bridge") || line.contains("bridge,") {
            config.user_net = false;
            // Extract bridge name if present
            if let Some(idx) = line.find("br=") {
                let rest = &line[idx + 3..];
                let bridge: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                config.bridge = Some(bridge);
            }
        }
    }

    if has_network || content.contains("-net") || content.contains("-nic") {
        Some(config)
    } else {
        None
    }
}

/// Extract extra arguments we don't specifically handle
fn extract_extra_args(content: &str) -> Vec<String> {
    let mut args = Vec::new();

    // Look for display settings
    if content.contains("-display gtk") {
        args.push("-display gtk".to_string());
    } else if content.contains("-display sdl") {
        args.push("-display sdl".to_string());
    } else if content.contains("-display vnc") {
        args.push("-display vnc".to_string());
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
mod tests {
    use super::*;

    #[test]
    fn test_extract_memory() {
        assert_eq!(extract_memory("-m 512"), Some(512));
        assert_eq!(extract_memory("-m 2G"), Some(2048));
        assert_eq!(extract_memory("qemu -m 1024 -cpu host"), Some(1024));
    }

    #[test]
    fn test_extract_emulator() {
        assert_eq!(
            extract_emulator("#!/bin/bash\nqemu-system-i386 -m 512"),
            Some(QemuEmulator::I386)
        );
        assert_eq!(
            extract_emulator("qemu-system-ppc -M mac99"),
            Some(QemuEmulator::Ppc)
        );
    }

    #[test]
    fn test_extract_vga() {
        assert_eq!(
            extract_vga("-vga cirrus -m 512"),
            Some(VgaType::Cirrus)
        );
        assert_eq!(
            extract_vga("-vga virtio"),
            Some(VgaType::Virtio)
        );
    }
}
