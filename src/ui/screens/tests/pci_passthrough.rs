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
    // Per-device progress messages are what tells the user which device hung
    // when binding fails. Lock this contract in for both the GPU-release path
    // and the generic device path.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains(r#"echo 'Releasing GPU $dev from $current...'"#),
        "per-device GPU release echo missing: {}",
        section
    );
    assert!(
        section.contains(r#"echo 'Unbinding $dev from $current...'"#),
        "per-device unbind echo missing: {}",
        section
    );
}

#[test]
fn generate_pci_section_wraps_every_unbind_in_timeout() {
    // The #25 hang: an unbind of a GPU the compositor holds blocks in the
    // kernel forever. Every unbind/remove write must go through `timeout` so
    // the script can report and fall back instead of hanging silently.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        !section.contains(r#"bind_cmds+="echo '$dev' > '$driver_link/unbind'"#),
        "bare (untimed) unbind write still present in bind path: {}",
        section
    );
    assert!(
        section.contains(r#"timeout -k 2 10 sh -c \"echo '$dev' > '$driver_link/unbind'\""#),
        "timeout-wrapped unbind missing: {}",
        section
    );
}

#[test]
fn generate_pci_section_guards_boot_vga() {
    // Unbinding the boot VGA device (the GPU rendering the current desktop)
    // freezes the session — the script must refuse and point to Single GPU
    // Passthrough instead (#25).
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains(r#""$(cat "$dev_path/boot_vga" 2>/dev/null)" == "1""#),
        "boot VGA guard missing: {}",
        section
    );
    assert!(section.contains("Single GPU Passthrough"));
}

#[test]
fn generate_pci_section_releases_gpu_from_compositor() {
    // GPU drivers get the compositor-release treatment: fake udev remove
    // event, nvidia-persistenced stop, and a PCI remove+rescan fallback when
    // the unbind times out.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains(r#"echo remove > \"\$card/uevent\""#),
        "udev remove nudge missing: {}",
        section
    );
    assert!(section.contains("systemctl stop nvidia-persistenced"));
    assert!(
        section.contains(r#"echo 1 > /sys/bus/pci/rescan"#),
        "PCI rescan fallback missing: {}",
        section
    );
}

#[test]
fn generate_pci_section_mirrors_output_to_log() {
    // TUI launches discard stdout; the passthrough.log is how users retrieve
    // diagnostics after a failed bind (#25).
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section
            .contains(r#"PCI_LOG="${VM_DIR:-$(dirname "$(readlink -f "$0")")}/passthrough.log""#),
        "PCI_LOG definition missing: {}",
        section
    );
    assert!(
        section.contains(r#"_pci_elevated "$bind_cmds" 2>&1 | tee -a "$PCI_LOG""#),
        "elevated bind output not mirrored to log: {}",
        section
    );
}

#[test]
fn generate_pci_section_restore_notifies_compositor_and_persistenced() {
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let section = generate_pci_section(&[&gpu]);

    assert!(
        section.contains(r#"echo add > \"\$card/uevent\""#),
        "udev add on restore missing: {}",
        section
    );
    assert!(section.contains("systemctl start nvidia-persistenced"));
}

#[test]
fn generate_pci_section_is_valid_bash() {
    // The section is a self-contained bash fragment full of nested quoting —
    // run it through `bash -n` so any quoting regression fails loudly here
    // rather than on a user's machine.
    let gpu = nvidia_gpu("0000:01:00.0", 14);
    let audio = nvidia_audio("0000:01:00.1", 14);
    let section = generate_pci_section(&[&gpu, &audio]);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pci_section.sh");
    std::fs::write(&path, &section).unwrap();
    let out = std::process::Command::new("bash")
        .arg("-n")
        .arg(&path)
        .output()
        .expect("bash should be runnable");
    assert!(
        out.status.success(),
        "generated PCI section is not valid bash:\n{}\n--- section ---\n{}",
        String::from_utf8_lossy(&out.stderr),
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
