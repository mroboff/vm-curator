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
