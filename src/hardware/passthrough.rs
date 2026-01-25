use super::usb::UsbDevice;

/// Generate QEMU arguments for USB passthrough
pub fn generate_usb_passthrough_args(devices: &[UsbDevice]) -> Vec<String> {
    let mut args = Vec::new();

    if devices.is_empty() {
        return args;
    }

    // Add USB controller if not already present
    args.push("-usb".to_string());

    // Add each device
    for device in devices {
        args.extend(device.to_qemu_args());
    }

    args
}

/// Generate QEMU arguments for USB passthrough by bus/device number
/// This is more specific but requires knowing the exact bus/dev
pub fn generate_usb_passthrough_by_bus(devices: &[UsbDevice]) -> Vec<String> {
    let mut args = Vec::new();

    if devices.is_empty() {
        return args;
    }

    args.push("-usb".to_string());

    for device in devices {
        args.push("-device".to_string());
        args.push(format!(
            "usb-host,hostbus={},hostaddr={}",
            device.bus_num, device.dev_num
        ));
    }

    args
}

/// Passthrough configuration for a VM
#[derive(Debug, Clone, Default)]
pub struct PassthroughConfig {
    pub usb_devices: Vec<UsbDevice>,
    pub pci_devices: Vec<String>, // For future PCI passthrough support
}

impl PassthroughConfig {
    pub fn to_qemu_args(&self) -> Vec<String> {
        let mut args = generate_usb_passthrough_args(&self.usb_devices);

        // PCI passthrough would be added here in the future
        for pci in &self.pci_devices {
            args.push("-device".to_string());
            args.push(format!("vfio-pci,host={}", pci));
        }

        args
    }
}
