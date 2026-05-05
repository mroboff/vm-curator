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

#[test]
fn test_parse_hostfwd_segment() {
    let pf = parse_hostfwd_segment("tcp::2222-:22").unwrap();
    assert_eq!(pf.protocol, PortProtocol::Tcp);
    assert_eq!(pf.host_port, 2222);
    assert_eq!(pf.guest_port, 22);

    let pf = parse_hostfwd_segment("udp::5353-:5353").unwrap();
    assert_eq!(pf.protocol, PortProtocol::Udp);
    assert_eq!(pf.host_port, 5353);
    assert_eq!(pf.guest_port, 5353);

    // With address
    let pf = parse_hostfwd_segment("tcp:127.0.0.1:8080-:80").unwrap();
    assert_eq!(pf.protocol, PortProtocol::Tcp);
    assert_eq!(pf.host_port, 8080);
    assert_eq!(pf.guest_port, 80);
}

#[test]
fn test_extract_port_forwards() {
    let line = "-netdev user,id=net0,hostfwd=tcp::2222-:22,hostfwd=tcp::8080-:80";
    let forwards = extract_port_forwards(line);
    assert_eq!(forwards.len(), 2);
    assert_eq!(forwards[0].host_port, 2222);
    assert_eq!(forwards[0].guest_port, 22);
    assert_eq!(forwards[1].host_port, 8080);
    assert_eq!(forwards[1].guest_port, 80);
}

#[test]
fn test_extract_network_passt() {
    let content = "qemu-system-x86_64 \\\n  -netdev passt,id=net0 \\\n  -device virtio-net-pci,netdev=net0";
    let config = extract_network(content).unwrap();
    assert_eq!(config.backend, NetworkBackend::Passt);
}

#[test]
fn test_extract_network_bridge() {
    let content = "qemu-system-x86_64 \\\n  -netdev bridge,id=net0,br=virbr0 \\\n  -device e1000,netdev=net0";
    let config = extract_network(content).unwrap();
    assert_eq!(config.backend, NetworkBackend::Bridge("virbr0".to_string()));
    assert_eq!(config.bridge, Some("virbr0".to_string()));
}

#[test]
fn test_extract_network_user_with_portfwd() {
    let content = "qemu-system-x86_64 \\\n  -netdev user,id=net0,hostfwd=tcp::2222-:22 \\\n  -device e1000,netdev=net0";
    let config = extract_network(content).unwrap();
    assert_eq!(config.backend, NetworkBackend::User);
    assert_eq!(config.port_forwards.len(), 1);
    assert_eq!(config.port_forwards[0].host_port, 2222);
    assert_eq!(config.port_forwards[0].guest_port, 22);
}

#[test]
fn test_extract_network_mac_on_device() {
    let content = "qemu-system-x86_64 \\\n  -netdev bridge,id=net0,br=virbr0 \\\n  -device virtio-net-pci,netdev=net0,mac=52:54:00:de:ad:be";
    let config = extract_network(content).unwrap();
    assert_eq!(config.mac_address, Some("52:54:00:de:ad:be".to_string()));
}

#[test]
fn test_extract_network_mac_uppercase_normalized() {
    let content = "qemu-system-x86_64 \\\n  -netdev user,id=net0 \\\n  -device e1000,netdev=net0,mac=AA:BB:CC:DD:EE:FF";
    let config = extract_network(content).unwrap();
    assert_eq!(config.mac_address, Some("aa:bb:cc:dd:ee:ff".to_string()));
}

#[test]
fn test_extract_network_no_mac() {
    let content = "qemu-system-x86_64 \\\n  -netdev user,id=net0 \\\n  -device e1000,netdev=net0";
    let config = extract_network(content).unwrap();
    assert_eq!(config.mac_address, None);
}

#[test]
fn test_extract_bios_path_with_variable() {
    let vm_dir = Path::new("/home/user/vms/mac-system7");
    let content = r#"VM_DIR="$(dirname "$(readlink -f "$0")")"
ROM="$VM_DIR/MacROM.bin"
qemu-system-m68k \
    -bios "$ROM" \
    -m 32M
"#;
    let result = extract_bios_path(content, vm_dir);
    assert!(result.is_some(), "Should extract bios path");
    let path = result.unwrap();
    assert!(
        path.to_string_lossy().contains("MacROM.bin"),
        "Path should contain MacROM.bin, got: {}",
        path.display()
    );
}

#[test]
fn test_extract_bios_path_direct() {
    let vm_dir = Path::new("/home/user/vms/test");
    let content = "qemu-system-m68k -bios /path/to/rom.bin -m 8M";
    let result = extract_bios_path(content, vm_dir);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), PathBuf::from("/path/to/rom.bin"));
}

#[test]
fn test_extract_bios_path_ovmf_filtered() {
    let vm_dir = Path::new("/home/user/vms/test");
    let content = "qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd -m 2048M";
    let result = extract_bios_path(content, vm_dir);
    assert!(result.is_none(), "OVMF paths should be filtered out");
}

#[test]
fn test_extract_bios_path_efi_filtered() {
    let vm_dir = Path::new("/home/user/vms/test");
    let content = "qemu-system-x86_64 -bios /usr/share/efi/firmware.fd -m 2048M";
    let result = extract_bios_path(content, vm_dir);
    assert!(result.is_none(), "EFI paths should be filtered out");
}

#[test]
fn test_extract_bios_path_not_present() {
    let vm_dir = Path::new("/home/user/vms/test");
    let content = "qemu-system-x86_64 -m 2048M -enable-kvm";
    let result = extract_bios_path(content, vm_dir);
    assert!(result.is_none(), "Should return None when no -bios arg");
}

#[test]
fn test_parse_launch_script_with_bios() {
    let vm_dir = Path::new("/home/user/vms/mac-system7");
    let script_path = vm_dir.join("launch.sh");
    let content = r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/mac-system7.qcow2"
ROM="$VM_DIR/MacROM.bin"

qemu-system-m68k \
    -M q800 \
    -bios "$ROM" \
    -m 32M \
    -drive file="$DISK",format=qcow2,if=scsi
"#;
    let config = parse_launch_script(&script_path, content).unwrap();
    assert!(config.bios_path.is_some(), "Should have bios_path");
    assert!(
        config.bios_path.as_ref().unwrap().to_string_lossy().contains("MacROM.bin"),
        "bios_path should contain MacROM.bin"
    );
    // Should NOT trigger UEFI detection
    assert!(!config.uefi, "Bios ROM should not trigger UEFI");
}
