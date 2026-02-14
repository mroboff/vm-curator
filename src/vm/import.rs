//! VM Import Logic
//!
//! Parses libvirt XML and quickemu .conf files, discovers importable VMs,
//! and executes the import (directory creation, disk handling, launch script generation).

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

use crate::app::{ImportDiskAction, ImportSource, ImportableVm, WizardQemuConfig};

// =========================================================================
// libvirt XML Parsing
// =========================================================================

/// Parse a libvirt domain XML file into an ImportableVm
pub fn parse_libvirt_xml(path: &Path) -> Result<ImportableVm> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read libvirt XML: {}", path.display()))?;

    parse_libvirt_xml_str(&content, path)
}

/// Parse libvirt XML from a string (for testing)
fn parse_libvirt_xml_str(xml: &str, config_path: &Path) -> Result<ImportableVm> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);

    // Parsed values
    let mut domain_type = String::new();
    let mut vm_name = String::new();
    let mut memory_kb: u64 = 0;
    let mut memory_unit = String::new();
    let mut vcpu: u32 = 0;
    let mut emulator_path = String::new();
    let mut arch = String::new();
    let mut machine_type = String::new();
    let mut has_uefi = false;
    let mut has_tpm = false;
    let mut disk_paths: Vec<PathBuf> = Vec::new();
    let mut disk_buses: Vec<String> = Vec::new();
    let mut graphics_type = String::new();
    let mut vga_model = String::new();
    let mut import_notes: Vec<String> = Vec::new();

    // Network (first interface found)
    let mut net_type = String::new();
    let mut net_model = String::new();
    let mut net_bridge = String::new();

    // Element tracking
    let mut element_stack: Vec<String> = Vec::new();
    // Text capture for the next Text event
    let mut capture_text_for: Option<String> = None;

    // Current disk/interface state
    let mut in_disk = false;
    let mut current_disk_bus = String::new();
    let mut current_disk_source = PathBuf::new();
    let mut in_interface = false;
    let mut current_net_type = String::new();
    let mut current_net_model = String::new();
    let mut current_net_bridge = String::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let parent = element_stack.last().map(|s| s.as_str()).unwrap_or("");

                match tag.as_str() {
                    "domain" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                domain_type = attr_value(&attr);
                            }
                        }
                    }
                    "name" if parent == "domain" => {
                        capture_text_for = Some("name".to_string());
                    }
                    "memory" | "currentMemory" => {
                        if memory_kb == 0 {
                            memory_unit = "KiB".to_string();
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"unit" {
                                    memory_unit = attr_value(&attr);
                                }
                            }
                            capture_text_for = Some("memory".to_string());
                        }
                    }
                    "vcpu" => {
                        capture_text_for = Some("vcpu".to_string());
                    }
                    "type" if parent == "os" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"arch" {
                                arch = attr_value(&attr);
                            }
                            if attr.key.as_ref() == b"machine" {
                                machine_type = attr_value(&attr);
                            }
                        }
                    }
                    "loader" => {
                        has_uefi = true;
                    }
                    "emulator" => {
                        capture_text_for = Some("emulator".to_string());
                    }
                    "disk" => {
                        in_disk = true;
                        current_disk_bus.clear();
                        current_disk_source = PathBuf::new();
                    }
                    "interface" => {
                        in_interface = true;
                        current_net_type.clear();
                        current_net_model.clear();
                        current_net_bridge.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                current_net_type = attr_value(&attr);
                            }
                        }
                    }
                    "video" => {} // Just track in stack
                    "model" if parent == "video" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                vga_model = attr_value(&attr);
                            }
                        }
                    }
                    "model" if in_interface => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                current_net_model = attr_value(&attr);
                            }
                        }
                    }
                    "tpm" => {
                        has_tpm = true;
                    }
                    _ => {}
                }

                element_stack.push(tag);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let parent = element_stack.last().map(|s| s.as_str()).unwrap_or("");

                match tag.as_str() {
                    "loader" => {
                        has_uefi = true;
                    }
                    "source" if in_disk => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"file" {
                                current_disk_source = PathBuf::from(attr_value(&attr));
                            }
                        }
                    }
                    "target" if in_disk => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"bus" {
                                current_disk_bus = attr_value(&attr);
                            }
                        }
                    }
                    "source" if in_interface => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"bridge" || attr.key.as_ref() == b"network" {
                                current_net_bridge = attr_value(&attr);
                            }
                        }
                    }
                    "model" if parent == "video" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                vga_model = attr_value(&attr);
                            }
                        }
                    }
                    "model" if in_interface => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                current_net_model = attr_value(&attr);
                            }
                        }
                    }
                    "graphics" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                graphics_type = attr_value(&attr);
                            }
                        }
                    }
                    "tpm" => {
                        has_tpm = true;
                    }
                    "type" if parent == "os" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"arch" {
                                arch = attr_value(&attr);
                            }
                            if attr.key.as_ref() == b"machine" {
                                machine_type = attr_value(&attr);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref t)) => {
                if let Some(ref target) = capture_text_for {
                    let text = String::from_utf8_lossy(t.as_ref()).trim().to_string();
                    match target.as_str() {
                        "name" => vm_name = text,
                        "memory" => {
                            if let Ok(val) = text.parse::<u64>() {
                                memory_kb = convert_memory_to_kib(val, &memory_unit);
                            }
                        }
                        "vcpu" => {
                            if let Ok(val) = text.parse::<u32>() {
                                vcpu = val;
                            }
                        }
                        "emulator" => emulator_path = text,
                        _ => {}
                    }
                    capture_text_for = None;
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if tag == "disk" && in_disk {
                    if !current_disk_source.as_os_str().is_empty() {
                        disk_paths.push(current_disk_source.clone());
                        disk_buses.push(current_disk_bus.clone());
                    }
                    in_disk = false;
                }
                if tag == "interface" && in_interface {
                    // Take the first network interface found
                    if net_type.is_empty() {
                        net_type = current_net_type.clone();
                        net_model = current_net_model.clone();
                        net_bridge = current_net_bridge.clone();
                    }
                    in_interface = false;
                }

                // Pop from stack
                if element_stack.last().map(|s| s.as_str()) == Some(&tag) {
                    element_stack.pop();
                }
                capture_text_for = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("Error parsing libvirt XML: {}", e),
            _ => {}
        }
        buf.clear();
    }

    // Validate domain type
    match domain_type.as_str() {
        "kvm" | "qemu" => {}
        "" => {
            bail!("No domain type found in XML. Only QEMU/KVM domains can be imported.");
        }
        other => {
            bail!(
                "This VM uses the {} hypervisor, which is not supported. Only QEMU/KVM domains can be imported.",
                other
            );
        }
    }

    if vm_name.is_empty() {
        vm_name = config_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported-vm")
            .to_string();
    }

    // Map emulator path to emulator string
    let emulator = map_emulator_path(&emulator_path, &arch);

    // Map machine type
    let machine = normalize_machine_type(&machine_type);

    // Map VGA model
    let vga = map_vga_model(&vga_model);

    // Map display
    let display = map_graphics_type(&graphics_type);

    // Map network
    let (network_backend, bridge_name, network_model) =
        map_network(&net_type, &net_model, &net_bridge, &mut import_notes);

    // Map disk interface from first disk
    let disk_interface = disk_buses
        .first()
        .map(|bus| map_disk_bus(bus))
        .unwrap_or_else(|| "ide".to_string());

    // Check disk readability
    let disks_readable: Vec<bool> = disk_paths
        .iter()
        .map(|p| p.exists() && fs::File::open(p).is_ok())
        .collect();

    // Add notes for unreadable/missing disks
    for (i, (path, readable)) in disk_paths.iter().zip(disks_readable.iter()).enumerate() {
        if !readable && path.exists() {
            import_notes.push(format!(
                "Disk {}: {} is not readable by current user. You may need: sudo chmod +r {}",
                i + 1,
                path.display(),
                path.display()
            ));
        } else if !path.exists() {
            import_notes.push(format!("Disk {}: {} does not exist", i + 1, path.display()));
        }
    }

    let enable_kvm = domain_type == "kvm";

    let qemu_config = WizardQemuConfig {
        emulator,
        memory_mb: (memory_kb / 1024) as u32,
        cpu_cores: if vcpu == 0 { 1 } else { vcpu },
        cpu_model: if enable_kvm {
            Some("host".to_string())
        } else {
            None
        },
        machine: if machine.is_empty() {
            None
        } else {
            Some(machine)
        },
        vga,
        audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
        network_model,
        disk_interface,
        enable_kvm,
        gl_acceleration: false,
        uefi: has_uefi,
        tpm: has_tpm,
        rtc_localtime: false,
        usb_tablet: true,
        display,
        network_backend,
        port_forwards: Vec::new(),
        bridge_name,
        extra_args: Vec::new(),
    };

    let detected_os_profile = detect_os_profile(&vm_name);

    Ok(ImportableVm {
        name: vm_name,
        config_path: config_path.to_path_buf(),
        source: ImportSource::Libvirt,
        qemu_config,
        disk_paths,
        detected_os_profile,
        import_notes,
        disks_readable,
    })
}

/// Helper: extract attribute value as String
fn attr_value(attr: &quick_xml::events::attributes::Attribute) -> String {
    String::from_utf8_lossy(&attr.value).to_string()
}

/// Convert memory value from a given unit to KiB
fn convert_memory_to_kib(val: u64, unit: &str) -> u64 {
    match unit {
        "b" | "bytes" => val / 1024,
        "KB" => val,
        "KiB" | "k" => val,
        "MB" => val * 1000 / 1024,
        "MiB" | "M" => val * 1024,
        "GB" => val * 1000 * 1000 / 1024,
        "GiB" | "G" => val * 1024 * 1024,
        _ => val, // default KiB
    }
}

// =========================================================================
// quickemu .conf Parsing
// =========================================================================

/// Parse a quickemu .conf file into an ImportableVm
pub fn parse_quickemu_conf(path: &Path) -> Result<ImportableVm> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read quickemu conf: {}", path.display()))?;

    parse_quickemu_conf_str(&content, path)
}

/// Parse quickemu conf from a string (for testing)
fn parse_quickemu_conf_str(content: &str, config_path: &Path) -> Result<ImportableVm> {
    let conf_dir = config_path.parent().unwrap_or(Path::new("."));

    let mut guest_os = String::new();
    let mut ram = String::new();
    let mut cpu_cores: u32 = 0;
    let mut disk_img = String::new();
    let mut boot = String::new();
    let mut display = String::new();
    let mut tpm = false;
    let mut import_notes: Vec<String> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "guest_os" => guest_os = value.to_string(),
                "ram" => ram = value.to_string(),
                "cpu_cores" => {
                    if let Ok(v) = value.parse::<u32>() {
                        cpu_cores = v;
                    }
                }
                "disk_img" => disk_img = value.to_string(),
                "boot" => boot = value.to_string(),
                "display" => display = value.to_string(),
                "tpm" => tpm = value == "on" || value == "true" || value == "yes",
                _ => {}
            }
        }
    }

    // Parse RAM
    let memory_mb = parse_quickemu_ram(&ram);

    // Resolve disk path relative to conf directory
    let disk_path = if disk_img.is_empty() {
        let stem = config_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("disk");
        conf_dir.join(stem).join(format!("{}.qcow2", stem))
    } else {
        let p = PathBuf::from(&disk_img);
        if p.is_absolute() {
            p
        } else {
            conf_dir.join(p)
        }
    };

    let disk_paths = if disk_path.exists() || !disk_img.is_empty() {
        vec![disk_path.clone()]
    } else {
        Vec::new()
    };

    // Map display
    let display = if display.is_empty() {
        "gtk".to_string()
    } else {
        match display.as_str() {
            "spice" => "spice-app".to_string(),
            "sdl" => "sdl".to_string(),
            "gtk" => "gtk".to_string(),
            other => other.to_string(),
        }
    };

    // Map boot mode
    let uefi = boot == "efi" || boot == "uefi";

    // macOS note
    if guest_os == "macos" {
        import_notes.push(
            "macOS: quickemu's OpenCore bootloader setup is not replicated. \
             The QEMU config is imported as-is."
                .to_string(),
        );
    }

    // Check disk readability
    let disks_readable: Vec<bool> = disk_paths
        .iter()
        .map(|p| p.exists() && fs::File::open(p).is_ok())
        .collect();

    // Derive VM name
    let vm_name = config_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported-vm")
        .to_string();

    if cpu_cores == 0 {
        cpu_cores = 2;
    }

    let qemu_config = WizardQemuConfig {
        emulator: "qemu-system-x86_64".to_string(),
        memory_mb: if memory_mb == 0 { 2048 } else { memory_mb },
        cpu_cores,
        cpu_model: Some("host".to_string()),
        machine: Some("q35".to_string()),
        vga: "virtio".to_string(),
        audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
        network_model: "virtio-net-pci".to_string(),
        disk_interface: "virtio".to_string(),
        enable_kvm: true,
        gl_acceleration: false,
        uefi,
        tpm,
        rtc_localtime: guest_os == "windows",
        usb_tablet: true,
        display,
        network_backend: "user".to_string(),
        port_forwards: Vec::new(),
        bridge_name: None,
        extra_args: Vec::new(),
    };

    let detected_os_profile = if !guest_os.is_empty() {
        detect_os_profile(&guest_os)
    } else {
        detect_os_profile(&vm_name)
    };

    Ok(ImportableVm {
        name: vm_name,
        config_path: config_path.to_path_buf(),
        source: ImportSource::Quickemu,
        qemu_config,
        disk_paths,
        detected_os_profile,
        import_notes,
        disks_readable,
    })
}

/// Parse quickemu RAM string (e.g., "4G" -> 4096, "2048M" -> 2048, "2048" -> 2048)
fn parse_quickemu_ram(ram: &str) -> u32 {
    let ram = ram.trim();
    if ram.is_empty() {
        return 0;
    }

    if let Some(gb) = ram.strip_suffix('G') {
        gb.trim().parse::<u32>().unwrap_or(0) * 1024
    } else if let Some(mb) = ram.strip_suffix('M') {
        mb.trim().parse::<u32>().unwrap_or(0)
    } else {
        ram.parse::<u32>().unwrap_or(0)
    }
}

// =========================================================================
// Auto-Discovery
// =========================================================================

/// Discover libvirt VMs from known system and user directories
pub fn discover_libvirt_vms() -> Vec<ImportableVm> {
    let mut vms = Vec::new();

    let search_dirs = get_libvirt_search_dirs();

    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("xml") {
                    if let Ok(vm) = parse_libvirt_xml(&path) {
                        vms.push(vm);
                    }
                }
            }
        }
    }

    vms
}

/// Discover quickemu VMs from known directories
pub fn discover_quickemu_vms() -> Vec<ImportableVm> {
    let mut vms = Vec::new();

    let search_dirs = get_quickemu_search_dirs();

    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("conf") {
                    if let Ok(vm) = parse_quickemu_conf(&path) {
                        vms.push(vm);
                    }
                }
            }
        }
    }

    vms
}

/// Get libvirt XML search directories
fn get_libvirt_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.push(PathBuf::from("/etc/libvirt/qemu"));
    if let Some(config_dir) = dirs::config_dir() {
        dirs.push(config_dir.join("libvirt").join("qemu"));
    }
    dirs
}

/// Get quickemu search directories
fn get_quickemu_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("quickemu"));
        dirs.push(home.join(".quickemu"));
        dirs.push(home.join("VMs"));
    }
    dirs
}

/// Parse a config file (auto-detect format from extension)
pub fn parse_config_file(path: &Path) -> Result<ImportableVm> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("xml") => parse_libvirt_xml(path),
        Some("conf") => parse_quickemu_conf(path),
        Some(ext) => bail!("Unsupported config file format: .{}", ext),
        None => bail!("Config file has no extension"),
    }
}

// =========================================================================
// Import Execution
// =========================================================================

/// Execute the import: create VM directory, handle disks, generate launch script
pub fn execute_import(
    library_path: &Path,
    vm: &ImportableVm,
    vm_name: &str,
    folder_name: &str,
    disk_action: ImportDiskAction,
) -> Result<PathBuf> {
    use crate::vm::create::{
        create_vm_directory, generate_launch_script_with_os, write_launch_script,
        write_vm_metadata,
    };

    let vm_dir = create_vm_directory(library_path, folder_name)?;

    // Handle each disk
    let mut disk_filenames: Vec<String> = Vec::new();
    for (i, disk_path) in vm.disk_paths.iter().enumerate() {
        if !disk_path.exists() {
            continue;
        }

        let disk_filename = if i == 0 {
            format!("{}.qcow2", folder_name)
        } else {
            format!("{}-disk{}.qcow2", folder_name, i + 1)
        };

        let dest = vm_dir.join(&disk_filename);

        match disk_action {
            ImportDiskAction::Symlink => {
                let abs_source = fs::canonicalize(disk_path).with_context(|| {
                    format!("Failed to resolve path: {}", disk_path.display())
                })?;
                unix_fs::symlink(&abs_source, &dest).with_context(|| {
                    format!(
                        "Failed to create symlink from {} to {}",
                        abs_source.display(),
                        dest.display()
                    )
                })?;
            }
            ImportDiskAction::Copy => {
                fs::copy(disk_path, &dest).with_context(|| {
                    format!(
                        "Failed to copy disk from {} to {}",
                        disk_path.display(),
                        dest.display()
                    )
                })?;
            }
            ImportDiskAction::Move => {
                if fs::rename(disk_path, &dest).is_err() {
                    fs::copy(disk_path, &dest).with_context(|| {
                        format!(
                            "Failed to copy disk from {} to {}",
                            disk_path.display(),
                            dest.display()
                        )
                    })?;
                    fs::remove_file(disk_path).with_context(|| {
                        format!("Failed to remove original disk: {}", disk_path.display())
                    })?;
                }
            }
        }

        disk_filenames.push(disk_filename);
    }

    // Generate launch script using the first disk
    let default_disk = format!("{}.qcow2", folder_name);
    let primary_disk = disk_filenames.first().unwrap_or(&default_disk);

    let script_content = generate_launch_script_with_os(
        vm_name,
        primary_disk,
        None,
        &vm.qemu_config,
        vm.detected_os_profile.as_deref(),
    );

    write_launch_script(&vm_dir, &script_content)?;
    write_vm_metadata(&vm_dir, vm_name, vm.detected_os_profile.as_deref(), None)?;

    Ok(vm_dir)
}

// =========================================================================
// Mapping Helpers
// =========================================================================

/// Map libvirt emulator path to QEMU command string
fn map_emulator_path(emulator_path: &str, arch: &str) -> String {
    if let Some(filename) = Path::new(emulator_path)
        .file_name()
        .and_then(|f| f.to_str())
    {
        if filename.starts_with("qemu-system-") {
            return filename.to_string();
        }
    }

    match arch {
        "x86_64" | "amd64" => "qemu-system-x86_64".to_string(),
        "i686" | "i386" => "qemu-system-i386".to_string(),
        "aarch64" | "arm64" => "qemu-system-aarch64".to_string(),
        "armv7l" | "arm" => "qemu-system-arm".to_string(),
        "ppc" | "ppc64" => "qemu-system-ppc".to_string(),
        _ => "qemu-system-x86_64".to_string(),
    }
}

/// Normalize libvirt machine type (e.g., pc-q35-8.2 -> q35)
fn normalize_machine_type(machine: &str) -> String {
    if machine.starts_with("pc-q35") {
        "q35".to_string()
    } else if machine.starts_with("pc-i440fx") || machine == "pc" {
        "pc".to_string()
    } else if machine.is_empty() {
        String::new()
    } else {
        machine.to_string()
    }
}

/// Map libvirt VGA model to QEMU VGA string
fn map_vga_model(model: &str) -> String {
    match model {
        "vga" | "" => "std".to_string(),
        "cirrus" => "cirrus".to_string(),
        "vmvga" => "vmware".to_string(),
        "qxl" => "qxl".to_string(),
        "virtio" => "virtio".to_string(),
        "bochs" => "std".to_string(),
        "none" => "none".to_string(),
        other => other.to_string(),
    }
}

/// Map libvirt graphics type to QEMU display string
fn map_graphics_type(graphics: &str) -> String {
    match graphics {
        "vnc" => "vnc".to_string(),
        "spice" => "spice-app".to_string(),
        "sdl" => "sdl".to_string(),
        "gtk" => "gtk".to_string(),
        "" => "gtk".to_string(),
        other => other.to_string(),
    }
}

/// Map libvirt network type to QEMU backend/model
fn map_network(
    net_type: &str,
    net_model: &str,
    net_bridge: &str,
    import_notes: &mut Vec<String>,
) -> (String, Option<String>, String) {
    let model = match net_model {
        "virtio" | "virtio-net-pci" => "virtio-net-pci".to_string(),
        "e1000" | "e1000e" => "e1000".to_string(),
        "rtl8139" => "rtl8139".to_string(),
        "" => "e1000".to_string(),
        other => other.to_string(),
    };

    match net_type {
        "bridge" => {
            let bridge = if net_bridge.is_empty() {
                None
            } else {
                Some(net_bridge.to_string())
            };
            ("bridge".to_string(), bridge, model)
        }
        "network" => {
            import_notes.push(format!(
                "Network: libvirt virtual network '{}' changed to user networking \
                 (libvirt-managed bridges don't translate to direct QEMU)",
                net_bridge
            ));
            ("user".to_string(), None, model)
        }
        "direct" => {
            import_notes.push(
                "Network: macvtap (direct attach) changed to user networking \
                 (macvtap not supported in vm-curator)"
                    .to_string(),
            );
            ("user".to_string(), None, model)
        }
        "user" | "" => ("user".to_string(), None, model),
        other => {
            import_notes.push(format!(
                "Network: unknown type '{}' changed to user networking",
                other
            ));
            ("user".to_string(), None, model)
        }
    }
}

/// Map libvirt disk bus to QEMU disk interface
fn map_disk_bus(bus: &str) -> String {
    match bus {
        "virtio" => "virtio".to_string(),
        "ide" => "ide".to_string(),
        "sata" | "ahci" => "ide".to_string(),
        "scsi" => "scsi".to_string(),
        "usb" => "usb".to_string(),
        "" => "ide".to_string(),
        other => other.to_string(),
    }
}

// =========================================================================
// OS Profile Detection
// =========================================================================

/// Fuzzy-match a VM name/guest_os against known profile IDs
pub fn detect_os_profile(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();

    let patterns: &[(&[&str], &str)] = &[
        (&["windows 11", "win11", "windows-11"], "windows-11"),
        (&["windows 10", "win10", "windows-10"], "windows-10"),
        (&["windows 7", "win7", "windows-7"], "windows-7"),
        (&["windows xp", "winxp"], "windows-xp"),
        (&["windows 98", "win98"], "windows-98"),
        (&["windows 95", "win95"], "windows-95"),
        (&["windows 2000", "win2k"], "windows-2000"),
        (&["macos", "mac-os", "osx"], "macos-sonoma"),
        (&["ubuntu"], "linux-ubuntu"),
        (&["fedora"], "linux-fedora"),
        (&["debian"], "linux-debian"),
        (&["arch", "archlinux"], "linux-arch"),
        (&["manjaro"], "linux-manjaro"),
        (&["mint", "linuxmint"], "linux-mint"),
        (&["opensuse", "suse"], "linux-opensuse"),
        (&["cachyos", "cachy"], "linux-cachyos"),
        (&["endeavouros", "endeavour"], "linux-endeavouros"),
        (&["nixos"], "linux-nixos"),
        (&["gentoo"], "linux-gentoo"),
        (&["void"], "linux-void"),
        (&["alpine"], "linux-alpine"),
        (&["centos"], "linux-centos"),
        (&["rocky"], "linux-rocky"),
        (&["alma"], "linux-alma"),
        (&["freebsd"], "bsd-freebsd"),
        (&["openbsd"], "bsd-openbsd"),
        (&["netbsd"], "bsd-netbsd"),
        (&["dos", "msdos", "ms-dos"], "retro-msdos"),
        (&["haiku"], "retro-haiku"),
        (&["kolibri"], "retro-kolibrios"),
    ];

    for (keywords, profile_id) in patterns {
        for keyword in *keywords {
            if name_lower.contains(keyword) {
                return Some(profile_id.to_string());
            }
        }
    }

    None
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_libvirt_xml_kvm_domain() {
        let xml = r#"
<domain type='kvm'>
  <name>test-vm</name>
  <memory unit='KiB'>2097152</memory>
  <vcpu>4</vcpu>
  <os>
    <type arch='x86_64' machine='pc-q35-8.2'>hvm</type>
    <loader readonly='yes' type='pflash'>/usr/share/OVMF/OVMF_CODE.fd</loader>
  </os>
  <devices>
    <emulator>/usr/bin/qemu-system-x86_64</emulator>
    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2'/>
      <source file='/var/lib/libvirt/images/test-vm.qcow2'/>
      <target dev='vda' bus='virtio'/>
    </disk>
    <interface type='bridge'>
      <source bridge='br0'/>
      <model type='virtio'/>
    </interface>
    <graphics type='spice'/>
    <video>
      <model type='qxl'/>
    </video>
  </devices>
</domain>
"#;

        let vm = parse_libvirt_xml_str(xml, Path::new("/etc/libvirt/qemu/test-vm.xml")).unwrap();

        assert_eq!(vm.name, "test-vm");
        assert_eq!(vm.qemu_config.memory_mb, 2048);
        assert_eq!(vm.qemu_config.cpu_cores, 4);
        assert_eq!(vm.qemu_config.emulator, "qemu-system-x86_64");
        assert_eq!(vm.qemu_config.machine, Some("q35".to_string()));
        assert!(vm.qemu_config.uefi);
        assert!(vm.qemu_config.enable_kvm);
        assert_eq!(vm.qemu_config.vga, "qxl");
        assert_eq!(vm.qemu_config.display, "spice-app");
        assert_eq!(vm.qemu_config.network_backend, "bridge");
        assert_eq!(vm.qemu_config.bridge_name, Some("br0".to_string()));
        assert_eq!(vm.qemu_config.network_model, "virtio-net-pci");
        assert_eq!(vm.qemu_config.disk_interface, "virtio");
        assert_eq!(
            vm.disk_paths,
            vec![PathBuf::from("/var/lib/libvirt/images/test-vm.qcow2")]
        );
    }

    #[test]
    fn test_parse_libvirt_xml_rejects_xen() {
        let xml = r#"
<domain type='xen'>
  <name>xen-vm</name>
  <memory unit='KiB'>1048576</memory>
  <vcpu>2</vcpu>
</domain>
"#;

        let result = parse_libvirt_xml_str(xml, Path::new("/etc/libvirt/qemu/xen-vm.xml"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("xen"));
    }

    #[test]
    fn test_parse_libvirt_xml_network_downgrade() {
        let xml = r#"
<domain type='kvm'>
  <name>net-test</name>
  <memory unit='KiB'>1048576</memory>
  <vcpu>2</vcpu>
  <os>
    <type arch='x86_64' machine='pc-q35-8.2'>hvm</type>
  </os>
  <devices>
    <emulator>/usr/bin/qemu-system-x86_64</emulator>
    <interface type='network'>
      <source network='default'/>
      <model type='e1000'/>
    </interface>
  </devices>
</domain>
"#;

        let vm = parse_libvirt_xml_str(xml, Path::new("/test.xml")).unwrap();
        assert_eq!(vm.qemu_config.network_backend, "user");
        assert!(vm
            .import_notes
            .iter()
            .any(|n| n.contains("libvirt virtual network")));
    }

    #[test]
    fn test_parse_libvirt_xml_macvtap_downgrade() {
        let xml = r#"
<domain type='kvm'>
  <name>macvtap-test</name>
  <memory unit='KiB'>1048576</memory>
  <vcpu>2</vcpu>
  <os>
    <type arch='x86_64'>hvm</type>
  </os>
  <devices>
    <emulator>/usr/bin/qemu-system-x86_64</emulator>
    <interface type='direct'>
      <source dev='eth0' mode='bridge'/>
      <model type='virtio'/>
    </interface>
  </devices>
</domain>
"#;

        let vm = parse_libvirt_xml_str(xml, Path::new("/test.xml")).unwrap();
        assert_eq!(vm.qemu_config.network_backend, "user");
        assert!(vm.import_notes.iter().any(|n| n.contains("macvtap")));
    }

    #[test]
    fn test_parse_quickemu_conf() {
        let conf = r#"
guest_os="linux"
ram="4G"
cpu_cores=4
disk_img="ubuntu-22.04/ubuntu-22.04.qcow2"
boot="efi"
tpm="on"
"#;

        let vm = parse_quickemu_conf_str(
            conf,
            Path::new("/home/user/quickemu/ubuntu-22.04.conf"),
        )
        .unwrap();

        assert_eq!(vm.name, "ubuntu-22.04");
        assert_eq!(vm.qemu_config.memory_mb, 4096);
        assert_eq!(vm.qemu_config.cpu_cores, 4);
        assert!(vm.qemu_config.uefi);
        assert!(vm.qemu_config.tpm);
        assert!(vm.qemu_config.enable_kvm);
    }

    #[test]
    fn test_parse_quickemu_ram() {
        assert_eq!(parse_quickemu_ram("4G"), 4096);
        assert_eq!(parse_quickemu_ram("2048M"), 2048);
        assert_eq!(parse_quickemu_ram("2048"), 2048);
        assert_eq!(parse_quickemu_ram(""), 0);
        assert_eq!(parse_quickemu_ram("8G"), 8192);
    }

    #[test]
    fn test_detect_os_profile() {
        assert_eq!(
            detect_os_profile("Windows 11"),
            Some("windows-11".to_string())
        );
        assert_eq!(
            detect_os_profile("ubuntu-22.04"),
            Some("linux-ubuntu".to_string())
        );
        assert_eq!(
            detect_os_profile("FreeBSD-14"),
            Some("bsd-freebsd".to_string())
        );
        assert_eq!(detect_os_profile("my-custom-vm"), None);
        assert_eq!(
            detect_os_profile("fedora-39"),
            Some("linux-fedora".to_string())
        );
    }

    #[test]
    fn test_normalize_machine_type() {
        assert_eq!(normalize_machine_type("pc-q35-8.2"), "q35");
        assert_eq!(normalize_machine_type("pc-i440fx-8.2"), "pc");
        assert_eq!(normalize_machine_type("pc"), "pc");
        assert_eq!(normalize_machine_type("virt"), "virt");
        assert_eq!(normalize_machine_type(""), "");
    }

    #[test]
    fn test_map_network_bridge() {
        let mut notes = Vec::new();
        let (backend, bridge, model) = map_network("bridge", "virtio", "br0", &mut notes);
        assert_eq!(backend, "bridge");
        assert_eq!(bridge, Some("br0".to_string()));
        assert_eq!(model, "virtio-net-pci");
        assert!(notes.is_empty());
    }

    #[test]
    fn test_map_disk_bus() {
        assert_eq!(map_disk_bus("virtio"), "virtio");
        assert_eq!(map_disk_bus("sata"), "ide");
        assert_eq!(map_disk_bus("ide"), "ide");
        assert_eq!(map_disk_bus("scsi"), "scsi");
    }

    #[test]
    fn test_map_emulator_path() {
        assert_eq!(
            map_emulator_path("/usr/bin/qemu-system-x86_64", "x86_64"),
            "qemu-system-x86_64"
        );
        assert_eq!(
            map_emulator_path("/usr/bin/qemu-system-aarch64", "aarch64"),
            "qemu-system-aarch64"
        );
        assert_eq!(
            map_emulator_path("/some/weird/path", "x86_64"),
            "qemu-system-x86_64"
        );
    }

    #[test]
    fn test_convert_memory_to_kib() {
        assert_eq!(convert_memory_to_kib(2097152, "KiB"), 2097152);
        assert_eq!(convert_memory_to_kib(2048, "MiB"), 2048 * 1024);
        assert_eq!(convert_memory_to_kib(2, "GiB"), 2 * 1024 * 1024);
    }

    #[test]
    fn test_parse_libvirt_xml_with_tpm() {
        let xml = r#"
<domain type='kvm'>
  <name>tpm-test</name>
  <memory unit='MiB'>4096</memory>
  <vcpu>2</vcpu>
  <os>
    <type arch='x86_64' machine='pc-q35-8.2'>hvm</type>
    <loader readonly='yes' type='pflash'>/usr/share/OVMF/OVMF_CODE.fd</loader>
  </os>
  <devices>
    <emulator>/usr/bin/qemu-system-x86_64</emulator>
    <tpm model='tpm-tis'>
      <backend type='emulator' version='2.0'/>
    </tpm>
  </devices>
</domain>
"#;

        let vm = parse_libvirt_xml_str(xml, Path::new("/test.xml")).unwrap();
        assert!(vm.qemu_config.tpm);
        assert!(vm.qemu_config.uefi);
        assert_eq!(vm.qemu_config.memory_mb, 4096);
    }
}
