use super::*;
use crate::hardware::PciDevice;

fn nvidia_gpu(addr: &str, group: u32) -> PciDevice {
    PciDevice {
        address: addr.to_string(),
        vendor_id: 0x10de,
        device_id: 0x2860,
        class_code: 0x030000,
        vendor_name: "NVIDIA".to_string(),
        device_name: "GeForce RTX 4060".to_string(),
        driver: Some("nvidia".to_string()),
        iommu_group: Some(group),
        is_boot_vga: false,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    }
}

fn nvidia_audio(addr: &str, group: u32) -> PciDevice {
    PciDevice {
        address: addr.to_string(),
        vendor_id: 0x10de,
        device_id: 0x228b,
        class_code: 0x040300,
        vendor_name: "NVIDIA".to_string(),
        device_name: "HD Audio Controller".to_string(),
        driver: Some("snd_hda_intel".to_string()),
        iommu_group: Some(group),
        is_boot_vga: false,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    }
}

#[test]
fn generate_pci_section_empty_devices_returns_empty() {
    assert_eq!(generate_pci_section(&[]), "");
}

#[test]
fn generate_pci_section_lists_all_addresses_in_pci_devices_array() {
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let audio = nvidia_audio("0000:01:00.1", 14);
    let section = generate_pci_section(&[&gpu, &audio]);

    assert!(
        section.contains(r#"PCI_DEVICES=("0000:01:00.0" "0000:01:00.1")"#),
        "PCI_DEVICES array missing both addresses: {}",
        section
    );
}

#[test]
fn generate_pci_section_includes_debug_gate() {
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains(r#"if [[ "${VM_CURATOR_DEBUG:-0}" == "1" ]]; then"#),
        "VM_CURATOR_DEBUG gate missing: {}",
        section
    );
    assert!(section.contains("set -x"));
}

#[test]
fn generate_pci_section_emits_per_device_progress_echos() {
    // Per-device "Unbinding $dev from $current..." messages are what tells the
    // user which device hung when binding fails. Lock this contract in.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains(r#"bind_cmds+="echo 'Unbinding $dev from $current...' >&2; ""#),
        "per-device unbind echo missing: {}",
        section
    );
}

#[test]
fn generate_pci_section_does_not_silence_unbind_stderr() {
    // travbp's hang in issue #25 was invisible because the original bind line
    // had `2>/dev/null` on the unbind. Surface kernel errors instead.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        !section.contains("/unbind' 2>/dev/null"),
        "unbind line still suppresses stderr: {}",
        section
    );
}

#[test]
fn generate_pci_section_polls_for_vfio_binding() {
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains("_wait_driver()"),
        "_wait_driver helper missing: {}",
        section
    );
    assert!(
        section.contains(r#"_wait_driver "$dev" "vfio-pci""#),
        "post-bind viability check missing: {}",
        section
    );
}

#[test]
fn generate_pci_section_restore_does_not_silence_pkexec() {
    // The original `pkexec ... 2>/dev/null` in restore_pci hid cleanup failures.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        !section.contains(r#"pkexec sh -c "$restore_cmds" 2>/dev/null"#),
        "restore_pci still silences pkexec stderr: {}",
        section
    );
}
