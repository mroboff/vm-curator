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
    assert_eq!(extract_vga("-vga cirrus -m 512"), Some(VgaType::Cirrus));
    assert_eq!(extract_vga("-vga virtio"), Some(VgaType::Virtio));
    assert_eq!(
        extract_vga("VM_CURATOR_VIDEO_ARGS=(-vga std)"),
        Some(VgaType::Std)
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
    let content =
        "qemu-system-x86_64 \\\n  -netdev passt,id=net0 \\\n  -device virtio-net-pci,netdev=net0";
    let config = extract_networks(content).into_iter().next().unwrap();
    assert_eq!(config.backend, NetworkBackend::Passt);
}

#[test]
fn test_extract_network_bridge() {
    let content =
        "qemu-system-x86_64 \\\n  -netdev bridge,id=net0,br=virbr0 \\\n  -device e1000,netdev=net0";
    let config = extract_networks(content).into_iter().next().unwrap();
    assert_eq!(config.backend, NetworkBackend::Bridge("virbr0".to_string()));
    assert_eq!(config.bridge, Some("virbr0".to_string()));
}

#[test]
fn test_extract_network_user_with_portfwd() {
    let content = "qemu-system-x86_64 \\\n  -netdev user,id=net0,hostfwd=tcp::2222-:22 \\\n  -device e1000,netdev=net0";
    let config = extract_networks(content).into_iter().next().unwrap();
    assert_eq!(config.backend, NetworkBackend::User);
    assert_eq!(config.port_forwards.len(), 1);
    assert_eq!(config.port_forwards[0].host_port, 2222);
    assert_eq!(config.port_forwards[0].guest_port, 22);
}

#[test]
fn test_extract_network_mac_on_device() {
    let content = "qemu-system-x86_64 \\\n  -netdev bridge,id=net0,br=virbr0 \\\n  -device virtio-net-pci,netdev=net0,mac=52:54:00:de:ad:be";
    let config = extract_networks(content).into_iter().next().unwrap();
    assert_eq!(config.mac_address, Some("52:54:00:de:ad:be".to_string()));
}

#[test]
fn test_extract_network_mac_uppercase_normalized() {
    let content = "qemu-system-x86_64 \\\n  -netdev user,id=net0 \\\n  -device e1000,netdev=net0,mac=AA:BB:CC:DD:EE:FF";
    let config = extract_networks(content).into_iter().next().unwrap();
    assert_eq!(config.mac_address, Some("aa:bb:cc:dd:ee:ff".to_string()));
}

#[test]
fn test_extract_network_no_mac() {
    let content = "qemu-system-x86_64 \\\n  -netdev user,id=net0 \\\n  -device e1000,netdev=net0";
    let config = extract_networks(content).into_iter().next().unwrap();
    assert_eq!(config.mac_address, None);
}

#[test]
fn test_extract_networks_non_numeric_netdev_id_still_parses_backend() {
    // A hand-edited script may use a descriptive -netdev id instead of this
    // app's own netN convention. The real backend/bridge must still be
    // recovered, not silently dropped to the User default (regression: a
    // save-back via Network Settings would otherwise clobber a working
    // bridge config with plain user/SLIRP).
    let content = "qemu-system-x86_64 \\\n  \
        -netdev bridge,id=mybr0,br=virbr0 \\\n  \
        -device rtl8139,netdev=mybr0";
    let configs = extract_networks(content);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].backend, NetworkBackend::Bridge("virbr0".to_string()));
    assert_eq!(configs[0].bridge, Some("virbr0".to_string()));
    assert_eq!(configs[0].model, "rtl8139");
}

#[test]
fn test_extract_networks_multiple_nics() {
    let content = "qemu-system-x86_64 \\\n  \
        -netdev user,id=net0,hostfwd=tcp::2222-:22 \\\n  \
        -device e1000,netdev=net0 \\\n  \
        -netdev bridge,id=net1,br=vmc-lan \\\n  \
        -device virtio-net-pci,netdev=net1,mac=52:54:00:de:ad:be";
    let configs = extract_networks(content);
    assert_eq!(configs.len(), 2);

    assert_eq!(configs[0].backend, NetworkBackend::User);
    assert_eq!(configs[0].model, "e1000");
    assert_eq!(configs[0].port_forwards.len(), 1);
    assert_eq!(configs[0].port_forwards[0].host_port, 2222);

    assert_eq!(
        configs[1].backend,
        NetworkBackend::Bridge("vmc-lan".to_string())
    );
    assert_eq!(configs[1].bridge, Some("vmc-lan".to_string()));
    assert_eq!(configs[1].model, "virtio-net");
    assert_eq!(
        configs[1].mac_address,
        Some("52:54:00:de:ad:be".to_string())
    );
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
        config
            .bios_path
            .as_ref()
            .unwrap()
            .to_string_lossy()
            .contains("MacROM.bin"),
        "bios_path should contain MacROM.bin"
    );
    // Should NOT trigger UEFI detection
    assert!(!config.uefi, "Bios ROM should not trigger UEFI");
}

#[test]
fn test_disk_roles_pflash_and_cdrom_excluded() {
    // Fedora-style regression: qcow2 OVMF firmware must not count as the
    // VM's snapshot-capable primary disk.
    let vm_dir = Path::new("/home/user/vms/win11-physical");
    let script_path = vm_dir.join("launch.sh");
    let content = r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="/dev/disk/by-id/nvme-Samsung_SSD_990_PRO_1TB_S6B0NS0W123456"
ISO="$VM_DIR/win11.iso"

qemu-system-x86_64 \
    -m 8192 \
    -drive if=pflash,format=qcow2,readonly=on,file=/usr/share/edk2/ovmf/OVMF_CODE_4M.qcow2 \
    -drive if=pflash,format=qcow2,file="$VM_DIR/OVMF_VARS.qcow2" \
    -drive file="$DISK",format=raw,if=virtio,index=0,media=disk \
    -drive file="$ISO",media=cdrom
"#;
    let config = parse_launch_script(&script_path, content).unwrap();

    let roles: Vec<DiskRole> = config.disks.iter().map(|d| d.role).collect();
    assert_eq!(
        roles,
        vec![
            DiskRole::Firmware,
            DiskRole::Firmware,
            DiskRole::System,
            DiskRole::Media
        ]
    );

    // qcow2 firmware must not make a raw-disk VM look snapshotable
    assert!(!config.supports_snapshots());

    // primary disk is the physical system disk, never the pflash firmware
    let primary = config.primary_disk().unwrap();
    assert_eq!(
        primary.path,
        PathBuf::from("/dev/disk/by-id/nvme-Samsung_SSD_990_PRO_1TB_S6B0NS0W123456")
    );
    assert!(primary.is_physical_device());
    // /dev paths are always raw and must not be probed with qemu-img
    assert_eq!(primary.format, DiskFormat::Raw);
}

#[test]
fn test_disk_roles_qcow2_system_disk_still_snapshotable() {
    let vm_dir = Path::new("/home/user/vms/fedora");
    let script_path = vm_dir.join("launch.sh");
    let content = r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/fedora.qcow2"

qemu-system-x86_64 \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
    -drive file="$DISK",format=qcow2,if=virtio \
    -drive file="$VM_DIR/install.iso",media=cdrom
"#;
    let config = parse_launch_script(&script_path, content).unwrap();

    assert!(config.supports_snapshots());
    let primary = config.primary_disk().unwrap();
    assert!(primary.path.to_string_lossy().ends_with("fedora.qcow2"));
    assert_eq!(primary.role, DiskRole::System);
    assert!(!primary.is_physical_device());
}

#[test]
fn test_physical_device_path_not_resolved_relative() {
    // Absolute /dev paths must pass through resolve_path untouched
    let vm_dir = Path::new("/home/user/vms/x");
    assert_eq!(
        resolve_path("/dev/nvme0n1", vm_dir),
        PathBuf::from("/dev/nvme0n1")
    );
}

#[test]
fn test_disks_scoped_to_default_boot_branch() {
    // vm-curator-generated scripts wrap several qemu-system invocations (one
    // per boot mode) in a `case "$1" in ... esac`, with the default no-args
    // boot branch emitted last. Disk extraction must only look at that final
    // invocation, not merge drives from --cdrom/--floppy branches that
    // reference positional parameters like "$2".
    let vm_dir = Path::new("/home/user/vms/openwrt");
    let script_path = vm_dir.join("launch.sh");
    let content = r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/openwrt.raw"
OVMF_VARS="$VM_DIR/OVMF_VARS.fd"

case "$1" in
    --cdrom)
        qemu-system-x86_64 \
        -drive if=pflash,format=raw,file="$OVMF_VARS" \
        -drive file="$DISK",format=raw,if=virtio,index=0,media=disk \
        -drive file="$2",media=cdrom,index=1
        ;;
    "")
        qemu-system-x86_64 \
        -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \
        -drive if=pflash,format=raw,file="$OVMF_VARS" \
        -drive file="$DISK",format=raw,if=virtio,index=0,media=disk
        ;;
esac
"#;
    let config = parse_launch_script(&script_path, content).unwrap();

    assert_eq!(config.disks.len(), 3);
    let paths: Vec<String> = config
        .disks
        .iter()
        .map(|d| d.path.to_string_lossy().to_string())
        .collect();
    assert!(!paths.iter().any(|p| p.contains('$')));
    assert!(paths.iter().any(|p| p.ends_with("openwrt.raw")));
}
