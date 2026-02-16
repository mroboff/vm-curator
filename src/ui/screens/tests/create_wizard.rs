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
