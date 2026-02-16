use super::*;

#[test]
fn test_usb_device_display() {
    let device = UsbDevice {
        vendor_id: 0x046d,
        product_id: 0xc077,
        vendor_name: "Logitech".to_string(),
        product_name: "M105 Mouse".to_string(),
        bus_num: 1,
        dev_num: 3,
        device_class: 0,
        usb_version: UsbVersion::Usb2,
    };

    assert_eq!(device.display_name(), "Logitech M105 Mouse");
    assert!(!device.is_hub());
}

#[test]
fn test_usb_version_from_speed() {
    assert_eq!(UsbVersion::from_speed("1.5"), UsbVersion::Usb1);
    assert_eq!(UsbVersion::from_speed("12"), UsbVersion::Usb1);
    assert_eq!(UsbVersion::from_speed("480"), UsbVersion::Usb2);
    assert_eq!(UsbVersion::from_speed("5000"), UsbVersion::Usb3);
    assert_eq!(UsbVersion::from_speed("10000"), UsbVersion::Usb3);
    assert_eq!(UsbVersion::from_speed("20000"), UsbVersion::Usb3);
    // Unknown speed defaults to USB 2.0
    assert_eq!(UsbVersion::from_speed("unknown"), UsbVersion::Usb2);
}

#[test]
fn test_usb_version_from_bcd() {
    assert_eq!(UsbVersion::from_bcd_usb(0x0100), UsbVersion::Usb1);
    assert_eq!(UsbVersion::from_bcd_usb(0x0110), UsbVersion::Usb1);
    assert_eq!(UsbVersion::from_bcd_usb(0x0200), UsbVersion::Usb2);
    assert_eq!(UsbVersion::from_bcd_usb(0x0210), UsbVersion::Usb2);
    assert_eq!(UsbVersion::from_bcd_usb(0x0300), UsbVersion::Usb3);
    assert_eq!(UsbVersion::from_bcd_usb(0x0310), UsbVersion::Usb3);
    assert_eq!(UsbVersion::from_bcd_usb(0x0320), UsbVersion::Usb3);
}

#[test]
fn test_usb_version_is_usb3() {
    assert!(!UsbVersion::Usb1.is_usb3());
    assert!(!UsbVersion::Usb2.is_usb3());
    assert!(UsbVersion::Usb3.is_usb3());
}
