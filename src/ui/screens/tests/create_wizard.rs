use super::*;

#[test]
fn test_parse_size_with_suffix_memory() {
    // Plain number assumes target unit (MB)
    assert_eq!(parse_size_with_suffix("8192", "MB"), Some(8192));
    assert_eq!(parse_size_with_suffix("2048", "MB"), Some(2048));

    // GB to MB conversion
    assert_eq!(parse_size_with_suffix("8GB", "MB"), Some(8192));
    assert_eq!(parse_size_with_suffix("8gb", "MB"), Some(8192)); // case insensitive
    assert_eq!(parse_size_with_suffix("32GB", "MB"), Some(32768));
    assert_eq!(parse_size_with_suffix("96GB", "MB"), Some(98304)); // exceeds old 64GB limit
    assert_eq!(parse_size_with_suffix("1024GB", "MB"), Some(1048576)); // 1TB

    // MB to MB (no conversion)
    assert_eq!(parse_size_with_suffix("8192MB", "MB"), Some(8192));

    // KB to MB conversion
    assert_eq!(parse_size_with_suffix("8388608KB", "MB"), Some(8192));

    // Whitespace handling
    assert_eq!(parse_size_with_suffix("  8192  ", "MB"), Some(8192));
    assert_eq!(parse_size_with_suffix("8 GB", "MB"), Some(8192));
}

#[test]
fn test_parse_size_with_suffix_disk() {
    // Plain number assumes target unit (GB)
    assert_eq!(parse_size_with_suffix("500", "GB"), Some(500));
    assert_eq!(parse_size_with_suffix("100", "GB"), Some(100));

    // GB to GB (no conversion)
    assert_eq!(parse_size_with_suffix("500GB", "GB"), Some(500));
    assert_eq!(parse_size_with_suffix("500gb", "GB"), Some(500));

    // MB to GB conversion
    assert_eq!(parse_size_with_suffix("512000MB", "GB"), Some(500));
    assert_eq!(parse_size_with_suffix("1024MB", "GB"), Some(1));
}

#[test]
fn test_parse_size_with_suffix_invalid() {
    // Empty string
    assert_eq!(parse_size_with_suffix("", "MB"), None);

    // Non-numeric
    assert_eq!(parse_size_with_suffix("abc", "MB"), None);
    assert_eq!(parse_size_with_suffix("GB", "MB"), None);

    // Negative values
    assert_eq!(parse_size_with_suffix("-100", "MB"), None);
}

// ---------------------------------------------------------------------------
// Step 4 field navigation: network adapters live in a separate list+editor
// overlay (opened from the single NetworkAdapters row), so step 4's own
// field list has no hidden/conditional rows to navigate around.
// ---------------------------------------------------------------------------

#[test]
fn qemu_field_from_index_round_trips_through_count() {
    // Every index in 0..count() must map to a distinct field and back
    // consistently — a stale index in a render/handler wouldn't panic
    // (from_index clamps to the last variant) but would silently land on
    // the wrong row, so pin the exact mapping.
    let expected = [
        QemuField::Memory,
        QemuField::CpuCores,
        QemuField::Vga,
        QemuField::Audio,
        QemuField::NetworkAdapters,
        QemuField::DiskInterface,
        QemuField::Display,
        QemuField::Kvm,
        QemuField::GlAccel,
        QemuField::Uefi,
        QemuField::Tpm,
        QemuField::UsbTablet,
        QemuField::RtcLocal,
    ];
    assert_eq!(QemuField::count(), expected.len());
    for (idx, field) in expected.iter().enumerate() {
        assert_eq!(QemuField::from_index(idx), *field, "index {idx}");
    }
}

#[test]
fn wizard_config_defaults_to_one_nic() {
    let cfg = WizardQemuConfig::default();
    assert_eq!(cfg.network_adapters.len(), 1);
    assert_eq!(cfg.active_nic, 0);
}
