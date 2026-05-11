use super::*;

#[test]
fn test_parse_size_with_suffix_memory() {
    // Plain number assumes target unit (MB)
    assert_eq!(parse_size_with_suffix("8192", "MB"), Some(8192));
    assert_eq!(parse_size_with_suffix("2048", "MB"), Some(2048));

    // GB to MB conversion
    assert_eq!(parse_size_with_suffix("8GB", "MB"), Some(8192));
    assert_eq!(parse_size_with_suffix("8gb", "MB"), Some(8192));  // case insensitive
    assert_eq!(parse_size_with_suffix("32GB", "MB"), Some(32768));
    assert_eq!(parse_size_with_suffix("96GB", "MB"), Some(98304));  // exceeds old 64GB limit
    assert_eq!(parse_size_with_suffix("1024GB", "MB"), Some(1048576));  // 1TB

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
// Issue #31: step-4 hidden-row navigation regression tests
// ---------------------------------------------------------------------------

fn cfg_with(network: &str, backend: &str) -> WizardQemuConfig {
    let mut c = WizardQemuConfig::default();
    c.network_model = network.to_string();
    c.network_backend = backend.to_string();
    c
}

#[test]
fn is_visible_network_none_hides_all_network_subfields() {
    let cfg = cfg_with("none", "user");
    assert!(!QemuField::NetBackend.is_visible(&cfg));
    assert!(!QemuField::BridgeName.is_visible(&cfg));
    assert!(!QemuField::PortForwards.is_visible(&cfg));
    assert!(!QemuField::MacAddress.is_visible(&cfg));
    // Sanity: non-network rows stay visible.
    assert!(QemuField::Memory.is_visible(&cfg));
    assert!(QemuField::Network.is_visible(&cfg));
    assert!(QemuField::DiskInterface.is_visible(&cfg));
    assert!(QemuField::RtcLocal.is_visible(&cfg));
}

#[test]
fn is_visible_user_backend_hides_bridge_keeps_forwards_and_mac() {
    let cfg = cfg_with("virtio", "user");
    assert!(QemuField::NetBackend.is_visible(&cfg));
    assert!(!QemuField::BridgeName.is_visible(&cfg));
    assert!(QemuField::PortForwards.is_visible(&cfg));
    assert!(QemuField::MacAddress.is_visible(&cfg));
}

#[test]
fn is_visible_bridge_backend_hides_forwards_keeps_bridge_and_mac() {
    let cfg = cfg_with("virtio", "bridge");
    assert!(QemuField::NetBackend.is_visible(&cfg));
    assert!(QemuField::BridgeName.is_visible(&cfg));
    assert!(!QemuField::PortForwards.is_visible(&cfg));
    assert!(QemuField::MacAddress.is_visible(&cfg));
}

#[test]
fn is_visible_passt_backend_hides_bridge_keeps_forwards() {
    let cfg = cfg_with("virtio", "passt");
    assert!(QemuField::NetBackend.is_visible(&cfg));
    assert!(!QemuField::BridgeName.is_visible(&cfg));
    assert!(QemuField::PortForwards.is_visible(&cfg));
    assert!(QemuField::MacAddress.is_visible(&cfg));
}

#[test]
fn next_visible_field_skips_hidden_when_network_none() {
    // Direct repro of issue #31: Down from Network (idx 4) must skip
    // NetBackend/Bridge/Forwards/MAC and land on DiskInterface (idx 9).
    let cfg = cfg_with("none", "user");
    assert_eq!(next_visible_field(4, &cfg, 1), 9, "Down from Network");
    // And Up from DiskInterface must skip back to Network.
    assert_eq!(next_visible_field(9, &cfg, -1), 4, "Up from DiskInterface");
}

#[test]
fn next_visible_field_skips_bridge_with_user_backend() {
    let cfg = cfg_with("virtio", "user");
    // Down from NetBackend (idx 5) skips BridgeName (idx 6) → PortForwards (idx 7).
    assert_eq!(next_visible_field(5, &cfg, 1), 7);
    // Up from PortForwards (idx 7) returns to NetBackend (idx 5).
    assert_eq!(next_visible_field(7, &cfg, -1), 5);
}

#[test]
fn next_visible_field_skips_forwards_with_bridge_backend() {
    let cfg = cfg_with("virtio", "bridge");
    // Down from BridgeName (idx 6) skips PortForwards (idx 7) → MAC (idx 8).
    assert_eq!(next_visible_field(6, &cfg, 1), 8);
    // Up from MAC returns to BridgeName.
    assert_eq!(next_visible_field(8, &cfg, -1), 6);
}

#[test]
fn next_visible_field_stays_put_at_bounds() {
    let cfg = WizardQemuConfig::default();
    // No visible row beyond RtcLocal (idx 16) → stay put.
    assert_eq!(next_visible_field(16, &cfg, 1), 16);
    // No row before Memory (idx 0) → stay put.
    assert_eq!(next_visible_field(0, &cfg, -1), 0);
}

#[test]
fn snap_focus_to_visible_moves_off_hidden_field() {
    // After a 'r' reset to a profile with network_model = "none", a focus
    // parked on NetBackend (idx 5) must snap forward to DiskInterface (9).
    let cfg = cfg_with("none", "user");
    assert_eq!(snap_focus_to_visible(5, &cfg), 9);
    // MAC (idx 8) is also hidden in this config — snap forward to 9.
    assert_eq!(snap_focus_to_visible(8, &cfg), 9);
}

#[test]
fn snap_focus_to_visible_keeps_visible_focus_put() {
    let cfg = WizardQemuConfig::default();
    // Memory (0), Network (4), and RtcLocal (16) are always visible.
    assert_eq!(snap_focus_to_visible(0, &cfg), 0);
    assert_eq!(snap_focus_to_visible(4, &cfg), 4);
    assert_eq!(snap_focus_to_visible(16, &cfg), 16);
}
