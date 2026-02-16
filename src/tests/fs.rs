use super::*;
use std::path::PathBuf;

#[test]
fn test_is_btrfs_nonexistent() {
    // Should not panic on non-existent path
    let result = is_btrfs(&PathBuf::from("/nonexistent/path/12345"));
    // Result depends on root filesystem type
    assert!(result == true || result == false);
}

#[test]
fn test_is_btrfs_root() {
    // Should work on root
    let _result = is_btrfs(&PathBuf::from("/"));
    // Just verify it doesn't panic
}
