use super::*;

#[test]
fn test_parse_json_snapshots() {
    let json = r#"{
        "filename": "test.qcow2",
        "format": "qcow2",
        "virtual-size": 10737418240,
        "actual-size": 1234567890,
        "snapshots": [
            {
                "id": "1",
                "name": "fresh-install",
                "vm-state-size": 536870912,
                "date-sec": 1705312245,
                "date-nsec": 123456789,
                "vm-clock-sec": 330,
                "vm-clock-nsec": 123000000
            },
            {
                "id": "2",
                "name": "after-drivers",
                "vm-state-size": 805306368,
                "date-sec": 1705412400,
                "date-nsec": 456789012,
                "vm-clock-sec": 945,
                "vm-clock-nsec": 456000000
            }
        ]
    }"#;

    let info: QemuImgInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.snapshots.len(), 2);
    assert_eq!(info.snapshots[0].name, "fresh-install");
    assert_eq!(info.snapshots[1].name, "after-drivers");
    assert_eq!(info.format, "qcow2");
}

#[test]
fn test_format_size() {
    assert_eq!(format_size(512), "512B");
    assert_eq!(format_size(1024), "1.0K");
    assert_eq!(format_size(1536), "1.5K");
    assert_eq!(format_size(1048576), "1.0M");
    assert_eq!(format_size(1073741824), "1.0G");
}

#[test]
fn test_format_vm_clock() {
    assert_eq!(format_vm_clock(0, 0), "00:00:00.000");
    assert_eq!(format_vm_clock(330, 123000000), "00:05:30.123");
    assert_eq!(format_vm_clock(3661, 500000000), "01:01:01.500");
}

#[test]
fn test_validate_snapshot_name() {
    // Valid names
    assert!(validate_snapshot_name("fresh-install").is_ok());
    assert!(validate_snapshot_name("snapshot_2024").is_ok());
    assert!(validate_snapshot_name("test.snapshot").is_ok());

    // Empty name should fail
    assert!(validate_snapshot_name("").is_err());
    assert!(validate_snapshot_name("   ").is_err());

    // Name with unsafe chars gets sanitized
    let result = validate_snapshot_name("test snapshot").unwrap();
    assert_eq!(result, "test_snapshot");

    // Name starting with dash gets prefixed
    let result = validate_snapshot_name("-test").unwrap();
    assert_eq!(result, "_-test");
}
