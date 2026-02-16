use super::*;
use crate::app::CreateWizardState;

#[test]
fn test_shell_escape_safe_strings() {
    // Safe strings should pass through unchanged
    assert_eq!(shell_escape("hello"), "hello");
    assert_eq!(shell_escape("path/to/file.iso"), "path/to/file.iso");
    assert_eq!(shell_escape("my-vm_name.qcow2"), "my-vm_name.qcow2");
}

#[test]
fn test_shell_escape_unsafe_strings() {
    // Strings with spaces
    assert_eq!(shell_escape("hello world"), "'hello world'");
    // Strings with quotes
    assert_eq!(shell_escape("it's a test"), "'it'\\''s a test'");
    // Strings with shell metacharacters
    assert_eq!(shell_escape("test; echo pwned"), "'test; echo pwned'");
    assert_eq!(shell_escape("$(whoami)"), "'$(whoami)'");
    assert_eq!(shell_escape("`whoami`"), "'`whoami`'");
    assert_eq!(shell_escape("test\"; echo pwned; echo \""), "'test\"; echo pwned; echo \"'");
}

#[test]
fn test_generate_folder_name() {
    assert_eq!(CreateWizardState::generate_folder_name("Windows 10"), "windows-10");
    assert_eq!(CreateWizardState::generate_folder_name("Debian GNU/Linux"), "debian-gnu-linux");
    assert_eq!(CreateWizardState::generate_folder_name("MS-DOS 6.22"), "ms-dos-6-22");
    assert_eq!(CreateWizardState::generate_folder_name("  Spaced  Out  "), "spaced-out");
}

#[test]
fn test_generate_launch_script() {
    let config = WizardQemuConfig::default();
    let script = generate_launch_script_with_os(
        "Test VM",
        "test.qcow2",
        Some(Path::new("/tmp/test.iso")),
        false,
        &config,
        None,
    );

    assert!(script.contains("#!/bin/bash"));
    assert!(script.contains("Test VM"));
    assert!(script.contains("test.qcow2"));
    assert!(script.contains("/tmp/test.iso"));
    assert!(script.contains("--install"));
    assert!(script.contains("--cdrom"));
    assert!(script.contains("--recovery"));
}

#[test]
fn test_build_qemu_command_basic() {
    let config = WizardQemuConfig {
        emulator: "qemu-system-x86_64".to_string(),
        memory_mb: 2048,
        cpu_cores: 2,
        cpu_model: Some("host".to_string()),
        machine: Some("q35".to_string()),
        vga: "std".to_string(),
        audio: vec![],
        network_model: "e1000".to_string(),
        disk_interface: "ide".to_string(),
        enable_kvm: true,
        uefi: false,
        tpm: false,
        rtc_localtime: false,
        usb_tablet: true,
        display: "gtk".to_string(),
        gl_acceleration: false,
        network_backend: "user".to_string(),
        port_forwards: vec![],
        bridge_name: None,
        extra_args: vec![],
        bios_path: None,
    };

    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, None);

    assert!(cmd.contains("qemu-system-x86_64"));
    assert!(cmd.contains("-enable-kvm"));
    assert!(cmd.contains("-m 2048M"));
    assert!(cmd.contains("-smp 2"));
    assert!(cmd.contains("-vga std"));
    assert!(cmd.contains("-display gtk"));
    assert!(cmd.contains("-device e1000"));
    assert!(cmd.contains("-usb"));
    assert!(cmd.contains("-device usb-tablet"));
}

#[test]
fn test_build_qemu_command_with_cdrom() {
    let config = WizardQemuConfig::default();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::Iso(None), None);

    assert!(cmd.contains("-drive file=\"$ISO\",media=cdrom"));
    assert!(cmd.contains("-boot d"));
}

#[test]
fn test_generate_network_args_user_with_portfwd() {
    let forwards = vec![
        PortForward { protocol: PortProtocol::Tcp, host_port: 2222, guest_port: 22 },
        PortForward { protocol: PortProtocol::Tcp, host_port: 8080, guest_port: 80 },
    ];
    let args = generate_network_args("e1000", "user", None, &forwards);
    assert_eq!(args.len(), 2);
    assert!(args[0].contains("hostfwd=tcp::2222-:22"));
    assert!(args[0].contains("hostfwd=tcp::8080-:80"));
    assert!(args[1].contains("e1000,netdev=net0"));
}

#[test]
fn test_generate_network_args_passt() {
    let args = generate_network_args("virtio", "passt", None, &[]);
    assert_eq!(args.len(), 2);
    assert!(args[0].contains("-netdev passt,id=net0"));
    assert!(args[1].contains("virtio-net-pci,netdev=net0"));
}

#[test]
fn test_generate_network_args_bridge() {
    let args = generate_network_args("e1000", "bridge", Some("virbr0"), &[]);
    assert_eq!(args.len(), 2);
    assert!(args[0].contains("-netdev bridge,id=net0,br=virbr0"));
}

#[test]
fn test_generate_network_args_none() {
    let args = generate_network_args("none", "user", None, &[]);
    assert!(args.is_empty());
}

#[test]
fn test_build_qemu_command_with_audio() {
    let config = WizardQemuConfig {
        audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
        ..Default::default()
    };

    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, None);

    assert!(cmd.contains("-audiodev pa,id=audio0"));
    assert!(cmd.contains("-device intel-hda"));
    assert!(cmd.contains("-device hda-duplex,audiodev=audio0"));
}

#[test]
fn test_build_qemu_command_with_bios() {
    let config = WizardQemuConfig {
        emulator: "qemu-system-m68k".to_string(),
        memory_mb: 32,
        cpu_cores: 1,
        cpu_model: Some("m68040".to_string()),
        machine: Some("q800".to_string()),
        vga: "none".to_string(),
        audio: vec![],
        network_model: "none".to_string(),
        disk_interface: "scsi".to_string(),
        enable_kvm: false,
        gl_acceleration: false,
        uefi: false,
        tpm: false,
        rtc_localtime: false,
        usb_tablet: false,
        display: "gtk".to_string(),
        network_backend: "user".to_string(),
        port_forwards: vec![],
        bridge_name: None,
        extra_args: vec![],
        bios_path: Some(PathBuf::from("MacROM.bin")),
    };

    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("mac-system7"));
    assert!(cmd.contains("-bios \"$ROM\""), "Should contain -bios \"$ROM\", got:\n{}", cmd);
    assert!(cmd.contains("qemu-system-m68k"), "Should contain m68k emulator");
}

#[test]
fn test_build_qemu_command_without_bios() {
    let config = WizardQemuConfig::default();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, None);
    assert!(!cmd.contains("-bios"), "Should NOT contain -bios when no bios_path");
}

#[test]
fn test_generate_launch_script_with_rom() {
    let config = WizardQemuConfig {
        emulator: "qemu-system-m68k".to_string(),
        bios_path: Some(PathBuf::from("MacROM.bin")),
        ..WizardQemuConfig::default()
    };

    let script = generate_launch_script_with_os(
        "Mac System 7",
        "mac-system7.qcow2",
        None,
        false,
        &config,
        Some("mac-system7"),
    );

    assert!(script.contains("ROM=\"$VM_DIR/MacROM.bin\""), "Script should contain ROM variable");
    assert!(script.contains("-bios \"$ROM\""), "Script should contain -bios \"$ROM\"");
}

#[test]
fn test_build_qemu_command_with_recovery_image() {
    let config = WizardQemuConfig::default();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::RecoveryImage(None), None);

    assert!(cmd.contains("format=dmg"), "Should contain format=dmg for recovery image");
    assert!(cmd.contains("snapshot=on"), "Should use snapshot overlay for writability");
    assert!(cmd.contains("if=ide,index=2"), "Should attach via IDE/AHCI at index 2");
    assert!(!cmd.contains("-boot d"), "Should NOT contain -boot d for recovery images");
    assert!(cmd.contains("\"$RECOVERY_IMG\""), "Should reference $RECOVERY_IMG variable");
}

#[test]
fn test_build_qemu_command_with_recovery_image_custom_path() {
    let config = WizardQemuConfig::default();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::RecoveryImage(Some("\"$2\"")), None);

    assert!(cmd.contains("format=dmg"), "Should contain format=dmg");
    assert!(cmd.contains("\"$2\""), "Should use custom path expression");
    assert!(!cmd.contains("-boot d"), "Should NOT contain -boot d");
}

#[test]
fn test_generate_launch_script_with_recovery_image() {
    let config = WizardQemuConfig::default();
    let script = generate_launch_script_with_os(
        "macOS Tahoe",
        "disk.qcow2",
        Some(Path::new("/tmp/BaseSystem.dmg")),
        true,
        &config,
        Some("macos-tahoe"),
    );

    assert!(script.contains("RECOVERY_IMG="), "Should use RECOVERY_IMG variable");
    assert!(!script.contains("ISO="), "Should NOT contain ISO variable when recovery image");
    assert!(script.contains("/tmp/BaseSystem.dmg"), "Should contain DMG path");
    assert!(script.contains("--recovery"), "Should contain --recovery option");
    assert!(script.contains("format=dmg"), "Install mode should use format=dmg");
}

#[test]
fn test_generate_launch_script_iso_unchanged() {
    let config = WizardQemuConfig::default();
    let script = generate_launch_script_with_os(
        "Linux VM",
        "disk.qcow2",
        Some(Path::new("/tmp/linux.iso")),
        false,
        &config,
        None,
    );

    assert!(script.contains("ISO="), "Should use ISO variable");
    assert!(!script.contains("RECOVERY_IMG="), "Should NOT contain RECOVERY_IMG variable");
    assert!(script.contains("--cdrom"), "Should contain --cdrom option");
    assert!(script.contains("--recovery"), "Should still contain --recovery option for flexibility");
    assert!(script.contains("-boot d"), "Install mode should boot from CD-ROM");
}

// === macOS-specific tests ===

/// Helper to create an Intel macOS UEFI config (like Big Sur+)
fn macos_uefi_config() -> WizardQemuConfig {
    WizardQemuConfig {
        emulator: "qemu-system-x86_64".to_string(),
        memory_mb: 8192,
        cpu_cores: 4,
        cpu_model: Some("Penryn,kvm=on,vendor=GenuineIntel,+invtsc,vmware-cpuid-freq=on,+ssse3,+sse4.2,+popcnt,+avx,+aes,+xsave,+xsaveopt,check".to_string()),
        machine: Some("q35".to_string()),
        vga: "none".to_string(),
        audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
        network_model: "vmxnet3".to_string(),
        disk_interface: "ide".to_string(),
        enable_kvm: true,
        gl_acceleration: false,
        uefi: true,
        tpm: false,
        rtc_localtime: false,
        usb_tablet: true,
        display: "spice-app".to_string(),
        network_backend: "passt".to_string(),
        port_forwards: vec![],
        bridge_name: None,
        extra_args: vec!["-device vmware-svga,vgamem_mb=256".to_string()],
        bios_path: Some(PathBuf::from("OpenCore.qcow2")),
    }
}

/// Helper to create an Intel macOS non-UEFI config (like Leopard)
fn macos_non_uefi_config() -> WizardQemuConfig {
    WizardQemuConfig {
        emulator: "qemu-system-x86_64".to_string(),
        memory_mb: 2048,
        cpu_cores: 2,
        cpu_model: Some("Penryn,kvm=on,vendor=GenuineIntel".to_string()),
        machine: Some("q35".to_string()),
        vga: "none".to_string(),
        audio: vec!["intel-hda".to_string(), "hda-duplex".to_string()],
        network_model: "vmxnet3".to_string(),
        disk_interface: "ide".to_string(),
        enable_kvm: true,
        gl_acceleration: false,
        uefi: false,
        tpm: false,
        rtc_localtime: false,
        usb_tablet: true,
        display: "spice-app".to_string(),
        network_backend: "passt".to_string(),
        port_forwards: vec![],
        bridge_name: None,
        extra_args: vec!["-device vmware-svga,vgamem_mb=256".to_string()],
        bios_path: None,
    }
}

#[test]
fn test_macos_includes_smc_and_smbios() {
    let config = macos_non_uefi_config();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("mac-osx-leopard"));

    assert!(cmd.contains("isa-applesmc,osk="), "Should contain Apple SMC device with quoted value");
    assert!(cmd.contains("-smbios type=2"), "Should contain SMBIOS type=2");
}

#[test]
fn test_macos_uefi_uses_ahci() {
    let config = macos_uefi_config();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("macos-sonoma"));

    assert!(cmd.contains("ich9-ahci,id=sata"), "Should have explicit AHCI controller");
    assert!(cmd.contains("bus=sata."), "Should use sata bus addressing");
    // Should NOT use the old if=ide,index=0 style
    assert!(!cmd.contains("if=ide,index=0"), "Should NOT use legacy if=ide,index=0 for macOS UEFI");
}

#[test]
fn test_macos_uefi_with_opencore() {
    let config = macos_uefi_config();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("macos-sonoma"));

    // OpenCore as sata.0
    assert!(cmd.contains("file=\"$ROM\",format=qcow2,if=none,id=oc"), "Should have OpenCore drive");
    assert!(cmd.contains("drive=oc,bus=sata.0"), "OpenCore should be on sata.0");
    // Main disk as sata.1
    assert!(cmd.contains("drive=maindisk,bus=sata.1"), "Main disk should be on sata.1");
    // Should NOT have -bios "$ROM" (OpenCore is an AHCI drive, not a BIOS)
    assert!(!cmd.contains("-bios \"$ROM\""), "Should NOT use -bios for macOS UEFI with OpenCore");
}

#[test]
fn test_macos_recovery_image_qcow2_on_ahci() {
    let config = macos_uefi_config();
    let cmd = build_qemu_command_with_os(
        &config, "disk.qcow2", &InstallMedia::RecoveryImage(None), Some("macos-sonoma")
    );

    // Recovery image on AHCI bus (no format= so QEMU auto-detects DMG vs qcow2)
    assert!(cmd.contains("if=none,id=recovery"), "Recovery should be on AHCI bus");
    assert!(!cmd.contains("format=qcow2,if=none,id=recovery"), "Recovery should NOT hardcode format (auto-detect)");
    assert!(cmd.contains("bus=sata.2"), "Recovery should be on sata.2 (after OpenCore on sata.0 and disk on sata.1)");
    assert!(!cmd.contains("-boot d"), "Should NOT boot from recovery directly (OpenCore handles it)");
}

#[test]
fn test_macos_spice_audio() {
    let config = macos_uefi_config();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("macos-sonoma"));

    assert!(cmd.contains("-audiodev spice,id=audio0"), "Should use spice audio backend with spice-app display");
    assert!(!cmd.contains("-audiodev pa,id=audio0"), "Should NOT use pa audio with spice-app display");
}

#[test]
fn test_macos_usb_kbd() {
    let config = macos_uefi_config();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("macos-sonoma"));

    assert!(cmd.contains("-device usb-kbd"), "Should include USB keyboard for macOS");
    assert!(cmd.contains("-device usb-tablet"), "Should also include USB tablet");
}

#[test]
fn test_ppc_macos_no_smc() {
    let config = WizardQemuConfig {
        emulator: "qemu-system-ppc".to_string(),
        machine: Some("mac99".to_string()),
        vga: "std".to_string(),
        audio: vec!["screamer".to_string()],
        network_model: "sungem".to_string(),
        disk_interface: "ide".to_string(),
        enable_kvm: false,
        uefi: false,
        usb_tablet: false,
        display: "gtk".to_string(),
        network_backend: "user".to_string(),
        ..Default::default()
    };

    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("mac-osx-tiger"));

    assert!(!cmd.contains("applesmc"), "PPC macOS should NOT have Apple SMC");
    assert!(!cmd.contains("-smbios type=2"), "PPC macOS should NOT have SMBIOS type=2");
    assert!(!cmd.contains("usb-kbd"), "PPC macOS should NOT have USB keyboard");
}

#[test]
fn test_non_macos_unchanged() {
    // Linux VM should not get any macOS-specific args
    let config = WizardQemuConfig::default();
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("ubuntu-24-04"));

    assert!(!cmd.contains("applesmc"), "Linux VM should NOT have Apple SMC");
    assert!(!cmd.contains("-smbios type=2"), "Linux VM should NOT have SMBIOS type=2");
    assert!(!cmd.contains("usb-kbd"), "Linux VM should NOT have USB keyboard");
    assert!(!cmd.contains("ich9-ahci"), "Linux VM should NOT have explicit AHCI controller");
    // Default config includes audio, so pa backend should be used (not spice)
    assert!(cmd.contains("-audiodev pa,id=audio0"), "Linux VM should use pa audio backend");
    assert!(!cmd.contains("-audiodev spice"), "Linux VM should NOT use spice audio backend");
}

#[test]
fn test_macos_uefi_iso_no_boot_d() {
    let config = macos_uefi_config();
    let cmd = build_qemu_command_with_os(
        &config, "disk.qcow2", &InstallMedia::Iso(None), Some("macos-sonoma")
    );

    // macOS UEFI should attach ISO on AHCI bus and NOT add -boot d
    assert!(cmd.contains("bus=sata.3"), "ISO should be on sata.3 (after OpenCore.0, disk.1, skipping .2 for recovery)");
    assert!(!cmd.contains("-boot d"), "macOS UEFI should NOT use -boot d (OpenCore handles boot)");
}

#[test]
fn test_macos_opencore_bootloader_check_in_script() {
    let config = macos_uefi_config();
    let script = generate_launch_script_with_os(
        "macOS Sonoma",
        "disk.qcow2",
        None,
        false,
        &config,
        Some("macos-sonoma"),
    );

    assert!(script.contains("Verify OpenCore bootloader exists"), "Script should verify OpenCore exists");
    assert!(script.contains("kholia/OSX-KVM"), "Script should mention OSX-KVM download source");
}

#[test]
fn test_macos_non_uefi_uses_bios() {
    // Leopard-era macOS: non-UEFI Intel, with a bios_path should use -bios "$ROM"
    let mut config = macos_non_uefi_config();
    config.bios_path = Some(PathBuf::from("some-rom.bin"));
    let cmd = build_qemu_command_with_os(&config, "disk.qcow2", &InstallMedia::None, Some("mac-osx-leopard"));

    assert!(cmd.contains("-bios \"$ROM\""), "Non-UEFI macOS with bios_path should use -bios");
    assert!(!cmd.contains("ich9-ahci"), "Non-UEFI macOS should NOT use explicit AHCI controller");
}
