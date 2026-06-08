//! VM Import Logic
//!
//! Parses libvirt XML and quickemu .conf files, discovers importable VMs,
//! and executes the import (directory creation, disk handling, launch script generation).

use anyhow::{bail, Context, Result};
use log::warn;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

use crate::wizard_types::{ImportDiskAction, ImportSource, ImportableVm, WizardQemuConfig};

// =========================================================================
// libvirt XML Parsing
// =========================================================================

/// Parse a libvirt domain XML file into an ImportableVm
pub fn parse_libvirt_xml(path: &Path) -> Result<ImportableVm> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read libvirt XML: {}", path.display()))?;

    parse_libvirt_xml_str(&content, path)
}

/// Return the value of the first attribute named `key` on an element, if present.
fn find_attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|attr| attr.key.as_ref() == key)
        .map(|attr| attr_value(&attr))
}

/// Mutable accumulator for streaming a libvirt domain XML document.
///
/// quick-xml is event-based, so parsed values build up across Start/Empty/Text/End
/// events. Each `handle_*` method applies a single event, and
/// [`LibvirtParse::into_importable_vm`] validates and maps the accumulated values
/// onto a [`WizardQemuConfig`]. Splitting the work this way keeps each step small
/// and independently readable instead of one large event loop.
#[derive(Default)]
struct LibvirtParse {
    domain_type: String,
    vm_name: String,
    memory_kb: u64,
    memory_unit: String,
    vcpu: u32,
    emulator_path: String,
    arch: String,
    machine_type: String,
    has_uefi: bool,
    has_tpm: bool,
    disk_paths: Vec<PathBuf>,
    disk_buses: Vec<String>,
    graphics_type: String,
    vga_model: String,
    import_notes: Vec<String>,

    // Network (first interface found)
    net_type: String,
    net_model: String,
    net_bridge: String,

    // Streaming bookkeeping
    element_stack: Vec<String>,
    /// Element name whose text content should be captured on the next Text event.
    capture_text_for: Option<String>,

    // Current <disk> being parsed
    in_disk: bool,
    current_disk_bus: String,
    current_disk_source: PathBuf,

    // Current <interface> being parsed
    in_interface: bool,
    current_net_type: String,
    current_net_model: String,
    current_net_bridge: String,
}

impl LibvirtParse {
    /// Name of the currently-open enclosing element.
    fn parent(&self) -> String {
        self.element_stack
            .last()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Read `arch`/`machine` from an `<os><type ...>` element.
    fn apply_os_type(&mut self, e: &quick_xml::events::BytesStart) {
        if let Some(v) = find_attr(e, b"arch") {
            self.arch = v;
        }
        if let Some(v) = find_attr(e, b"machine") {
            self.machine_type = v;
        }
    }

    /// Read the VGA model from a `<video><model type=...>` element.
    fn apply_video_model(&mut self, e: &quick_xml::events::BytesStart) {
        if let Some(v) = find_attr(e, b"type") {
            self.vga_model = v;
        }
    }

    /// Read the NIC model from an `<interface><model type=...>` element.
    fn apply_interface_model(&mut self, e: &quick_xml::events::BytesStart) {
        if let Some(v) = find_attr(e, b"type") {
            self.current_net_model = v;
        }
    }

    /// Apply a Start element (one with children or text content).
    fn handle_start(&mut self, e: &quick_xml::events::BytesStart) {
        let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
        let parent = self.parent();

        match tag.as_str() {
            "domain" => {
                if let Some(v) = find_attr(e, b"type") {
                    self.domain_type = v;
                }
            }
            "name" if parent == "domain" => {
                self.capture_text_for = Some("name".to_string());
            }
            "memory" | "currentMemory" if self.memory_kb == 0 => {
                self.memory_unit = find_attr(e, b"unit").unwrap_or_else(|| "KiB".to_string());
                self.capture_text_for = Some("memory".to_string());
            }
            "vcpu" => {
                self.capture_text_for = Some("vcpu".to_string());
            }
            "type" if parent == "os" => self.apply_os_type(e),
            "loader" => self.has_uefi = true,
            "emulator" => {
                self.capture_text_for = Some("emulator".to_string());
            }
            "disk" => {
                self.in_disk = true;
                self.current_disk_bus.clear();
                self.current_disk_source = PathBuf::new();
            }
            "interface" => {
                self.in_interface = true;
                self.current_net_type.clear();
                self.current_net_model.clear();
                self.current_net_bridge.clear();
                if let Some(v) = find_attr(e, b"type") {
                    self.current_net_type = v;
                }
            }
            "video" => {} // Just track in stack
            "model" if parent == "video" => self.apply_video_model(e),
            "model" if self.in_interface => self.apply_interface_model(e),
            "tpm" => self.has_tpm = true,
            _ => {}
        }

        self.element_stack.push(tag);
    }

    /// Apply an Empty element (self-closing, e.g. `<source .../>`).
    fn handle_empty(&mut self, e: &quick_xml::events::BytesStart) {
        let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
        let parent = self.parent();

        match tag.as_str() {
            "loader" => self.has_uefi = true,
            "source" if self.in_disk => {
                if let Some(v) = find_attr(e, b"file") {
                    self.current_disk_source = PathBuf::from(v);
                }
            }
            "target" if self.in_disk => {
                if let Some(v) = find_attr(e, b"bus") {
                    self.current_disk_bus = v;
                }
            }
            "source" if self.in_interface => {
                // A libvirt interface source uses `bridge` or `network`.
                if let Some(v) = find_attr(e, b"bridge") {
                    self.current_net_bridge = v;
                }
                if let Some(v) = find_attr(e, b"network") {
                    self.current_net_bridge = v;
                }
            }
            "model" if parent == "video" => self.apply_video_model(e),
            "model" if self.in_interface => self.apply_interface_model(e),
            "graphics" => {
                if let Some(v) = find_attr(e, b"type") {
                    self.graphics_type = v;
                }
            }
            "tpm" => self.has_tpm = true,
            "type" if parent == "os" => self.apply_os_type(e),
            _ => {}
        }
    }

    /// Apply a Text event, storing the content for the pending captured element.
    fn handle_text(&mut self, raw: &str) {
        let Some(target) = self.capture_text_for.take() else {
            return;
        };
        let text = raw.trim();
        match target.as_str() {
            "name" => self.vm_name = text.to_string(),
            "memory" => {
                if let Ok(val) = text.parse::<u64>() {
                    self.memory_kb = convert_memory_to_kib(val, &self.memory_unit);
                }
            }
            "vcpu" => {
                if let Ok(val) = text.parse::<u32>() {
                    self.vcpu = val;
                }
            }
            "emulator" => self.emulator_path = text.to_string(),
            _ => {}
        }
    }

    /// Apply an End element, finalizing the current disk/interface and popping the stack.
    fn handle_end(&mut self, tag: &str) {
        if tag == "disk" && self.in_disk {
            if !self.current_disk_source.as_os_str().is_empty() {
                self.disk_paths.push(self.current_disk_source.clone());
                self.disk_buses.push(self.current_disk_bus.clone());
            }
            self.in_disk = false;
        }
        if tag == "interface" && self.in_interface {
            // Take the first network interface found
            if self.net_type.is_empty() {
                self.net_type = self.current_net_type.clone();
                self.net_model = self.current_net_model.clone();
                self.net_bridge = self.current_net_bridge.clone();
            }
            self.in_interface = false;
        }

        // Pop from stack
        if self.element_stack.last().map(|s| s.as_str()) == Some(tag) {
            self.element_stack.pop();
        }
        self.capture_text_for = None;
    }

    /// Validate the parsed domain and map it onto an [`ImportableVm`].
    fn into_importable_vm(mut self, config_path: &Path) -> Result<ImportableVm> {
        // Validate domain type
        match self.domain_type.as_str() {
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

        if self.vm_name.is_empty() {
            self.vm_name = config_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("imported-vm")
                .to_string();
        }

        let emulator = map_emulator_path(&self.emulator_path, &self.arch);
        let machine = normalize_machine_type(&self.machine_type);
        let vga = map_vga_model(&self.vga_model);
        let display = map_graphics_type(&self.graphics_type);
        let (network_backend, bridge_name, network_model) = map_network(
            &self.net_type,
            &self.net_model,
            &self.net_bridge,
            &mut self.import_notes,
        );

        // Map disk interface from first disk
        let disk_interface = self
            .disk_buses
            .first()
            .map(|bus| map_disk_bus(bus))
            .unwrap_or_else(|| "ide".to_string());

        // Move disks out so we can push readability notes onto import_notes.
        let disk_paths = std::mem::take(&mut self.disk_paths);
        let disks_readable: Vec<bool> = disk_paths
            .iter()
            .map(|p| p.exists() && fs::File::open(p).is_ok())
            .collect();

        // Add notes for unreadable/missing disks
        for (i, (path, readable)) in disk_paths.iter().zip(disks_readable.iter()).enumerate() {
            if !readable && path.exists() {
                self.import_notes.push(format!(
                    "Disk {}: {} is not readable by current user. You may need: sudo chmod +r {}",
                    i + 1,
                    path.display(),
                    path.display()
                ));
            } else if !path.exists() {
                self.import_notes.push(format!(
                    "Disk {}: {} does not exist",
                    i + 1,
                    path.display()
                ));
            }
        }

        let enable_kvm = self.domain_type == "kvm";
        let detected_os_profile = detect_os_profile(&self.vm_name);

        let qemu_config = WizardQemuConfig {
            emulator,
            memory_mb: (self.memory_kb / 1024) as u32,
            cpu_cores: if self.vcpu == 0 { 1 } else { self.vcpu },
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
            uefi: self.has_uefi,
            tpm: self.has_tpm,
            rtc_localtime: false,
            usb_tablet: true,
            display,
            network_backend,
            port_forwards: Vec::new(),
            bridge_name,
            mac_address: None,
            extra_args: Vec::new(),
            bios_path: None,
        };

        Ok(ImportableVm {
            name: self.vm_name,
            config_path: config_path.to_path_buf(),
            source: ImportSource::Libvirt,
            qemu_config,
            disk_paths,
            detected_os_profile,
            import_notes: self.import_notes,
            disks_readable,
        })
    }
}

/// Parse libvirt XML from a string (separated from file IO so it can be tested).
fn parse_libvirt_xml_str(xml: &str, config_path: &Path) -> Result<ImportableVm> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut state = LibvirtParse::default();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => state.handle_start(e),
            Ok(Event::Empty(ref e)) => state.handle_empty(e),
            Ok(Event::Text(ref t)) => {
                let text = String::from_utf8_lossy(t.as_ref());
                state.handle_text(&text);
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                state.handle_end(&tag);
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("Error parsing libvirt XML: {}", e),
            _ => {}
        }
        buf.clear();
    }

    state.into_importable_vm(config_path)
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
        mac_address: None,
        extra_args: Vec::new(),
        bios_path: None,
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
                    match parse_libvirt_xml(&path) {
                        Ok(vm) => vms.push(vm),
                        Err(e) => warn!("Failed to parse libvirt XML {}: {}", path.display(), e),
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
                    match parse_quickemu_conf(&path) {
                        Ok(vm) => vms.push(vm),
                        Err(e) => warn!("Failed to parse quickemu conf {}: {}", path.display(), e),
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

/// Scan any directory for importable VMs (libvirt .xml and quickemu .conf files).
#[allow(dead_code)]
pub fn discover_vms_in_dir(dir: &Path) -> Vec<ImportableVm> {
    let mut vms = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return vms;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            match parse_config_file(&path) {
                Ok(vm) => vms.push(vm),
                Err(e) => warn!("Skipping {}: {}", path.display(), e),
            }
        }
    }
    vms
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
        create_vm_directory, generate_launch_script_with_os, write_launch_script, write_vm_metadata,
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
                let abs_source = fs::canonicalize(disk_path)
                    .with_context(|| format!("Failed to resolve path: {}", disk_path.display()))?;
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
        false,
        &vm.qemu_config,
        vm.detected_os_profile.as_deref(),
        None,
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

#[cfg(test)]
#[path = "tests/import.rs"]
mod tests;
