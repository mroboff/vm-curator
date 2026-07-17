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

/// Helper for is_integrated_gpu tests
fn gpu_device(address: &str, vendor_id: u16, device_id: u16) -> PciDevice {
    PciDevice {
        address: address.to_string(),
        vendor_id,
        device_id,
        class_code: 0x030000,
        vendor_name: String::new(),
        device_name: String::new(),
        driver: None,
        iommu_group: Some(1),
        is_boot_vga: true,
        subsystem_vendor_id: 0,
        subsystem_device_id: 0,
    }
}

#[test]
fn integrated_gpu_detection() {
    // Known AMD APUs (issue #61 reporter's Rembrandt 680M among them)
    assert!(gpu_device("0000:e4:00.0", 0x1002, 0x1681).is_integrated_gpu());
    assert!(gpu_device("0000:04:00.0", 0x1002, 0x15bf).is_integrated_gpu());

    // AMD discrete cards are not flagged
    assert!(!gpu_device("0000:03:00.0", 0x1002, 0x744c).is_integrated_gpu());

    // Intel iGPU always sits at 00:02.0; Arc dGPUs live elsewhere
    assert!(gpu_device("0000:00:02.0", 0x8086, 0x9a49).is_integrated_gpu());
    assert!(!gpu_device("0000:03:00.0", 0x8086, 0x56a0).is_integrated_gpu());

    // NVIDIA is never integrated
    assert!(!gpu_device("0000:01:00.0", 0x10de, 0x2684).is_integrated_gpu());

    // A non-GPU AMD device with a coincidental APU device ID is not flagged
    let mut audio = gpu_device("0000:e4:00.1", 0x1002, 0x1681);
    audio.class_code = 0x040300;
    assert!(!audio.is_integrated_gpu());
}
