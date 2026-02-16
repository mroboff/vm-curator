use super::*;

#[test]
fn test_pci_device_is_gpu() {
    let device = PciDevice {
        address: "0000:01:00.0".to_string(),
        vendor_id: 0x10de,
        device_id: 0x2684,
        class_code: 0x030000,
        vendor_name: "NVIDIA".to_string(),
        device_name: "GeForce RTX 4090".to_string(),
        driver: Some("nvidia".to_string()),
        iommu_group: Some(1),
        is_boot_vga: false,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    };

    assert!(device.is_gpu());
    assert!(device.is_vga());
    assert!(device.is_nvidia());
    assert!(device.can_passthrough());
}

#[test]
fn test_pci_device_boot_vga_cannot_passthrough() {
    let device = PciDevice {
        address: "0000:00:02.0".to_string(),
        vendor_id: 0x8086,
        device_id: 0x1234,
        class_code: 0x030000,
        vendor_name: "Intel".to_string(),
        device_name: "UHD Graphics".to_string(),
        driver: Some("i915".to_string()),
        iommu_group: Some(0),
        is_boot_vga: true,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    };

    assert!(device.is_gpu());
    assert!(!device.can_passthrough());
}

#[test]
fn test_generate_passthrough_args() {
    let devices = vec![
        PciDevice {
            address: "0000:01:00.0".to_string(),
            vendor_id: 0x10de,
            device_id: 0x2684,
            class_code: 0x030000,
            vendor_name: "NVIDIA".to_string(),
            device_name: "GeForce RTX 4090".to_string(),
            driver: Some("vfio-pci".to_string()),
            iommu_group: Some(1),
            is_boot_vga: false,
            subsystem_vendor_id: 0,
            subsystem_device_id: 0,
        },
        PciDevice {
            address: "0000:01:00.1".to_string(),
            vendor_id: 0x10de,
            device_id: 0x228b,
            class_code: 0x040300,
            vendor_name: "NVIDIA".to_string(),
            device_name: "HD Audio Controller".to_string(),
            driver: Some("vfio-pci".to_string()),
            iommu_group: Some(1),
            is_boot_vga: false,
            subsystem_vendor_id: 0,
            subsystem_device_id: 0,
        },
    ];

    let args = generate_passthrough_args(&devices);

    assert_eq!(args.len(), 4); // 2 devices, each with 2 args
    assert!(args[1].contains("multifunction=on")); // Primary GPU
    assert!(!args[1].contains("x-vga")); // No x-vga (incompatible with modern NVIDIA)
    assert!(!args[3].contains("x-vga")); // Audio device also no x-vga
}
