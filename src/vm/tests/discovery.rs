use super::*;

#[test]
fn test_display_name() {
    let vm = DiscoveredVm {
        id: "windows-95".to_string(),
        path: PathBuf::from("/test"),
        launch_script: PathBuf::from("/test/launch.sh"),
        config: QemuConfig::default(),
        custom_name: None,
        os_profile: None,
        notes: None,
    };
    assert_eq!(vm.display_name(), "Microsoft® Windows 95");
}

#[test]
fn test_custom_display_name() {
    let vm = DiscoveredVm {
        id: "linux-cachyos-2".to_string(),
        path: PathBuf::from("/test"),
        launch_script: PathBuf::from("/test/launch.sh"),
        config: QemuConfig::default(),
        custom_name: Some("CachyOS Gaming Rig".to_string()),
        os_profile: Some("linux-cachyos".to_string()),
        notes: None,
    };
    // Custom name takes priority
    assert_eq!(vm.display_name(), "CachyOS Gaming Rig");
}

#[test]
fn test_linux_display_names() {
    assert_eq!(format_os_display_name("linux-cachyos"), "CachyOS (rolling)");
    assert_eq!(format_os_display_name("linux-fedora-40"), "Fedora Linux 40");
    assert_eq!(format_os_display_name("linux-arch"), "Arch Linux (rolling)");
    assert_eq!(format_os_display_name("linux-ubuntu-2404"), "Ubuntu 2404");
    // SuSE naming
    assert_eq!(format_os_display_name("linux-suse"), "openSUSE Tumbleweed (rolling)");
    assert_eq!(format_os_display_name("linux-suse-7"), "SuSE Linux 7");
    assert_eq!(format_os_display_name("linux-opensuse-leap-15"), "openSUSE Leap 15");
}

#[test]
fn test_linux_display_names_with_suffix() {
    // Rolling distros with numeric suffixes should display the same as originals
    assert_eq!(format_os_display_name("linux-cachyos-2"), "CachyOS (rolling)");
    assert_eq!(format_os_display_name("linux-cachyos-3"), "CachyOS (rolling)");
    assert_eq!(format_os_display_name("linux-arch-2"), "Arch Linux (rolling)");
    assert_eq!(format_os_display_name("linux-gentoo-2"), "Gentoo Linux (rolling)");
    assert_eq!(format_os_display_name("linux-manjaro-3"), "Manjaro Linux (rolling)");
    // Versioned distros keep version numbers (which may look like suffixes)
    assert_eq!(format_os_display_name("linux-fedora-2"), "Fedora Linux 2");
    assert_eq!(format_os_display_name("linux-ubuntu-2"), "Ubuntu 2");
}

#[test]
fn test_os2_display_names() {
    assert_eq!(format_os_display_name("os2-warp-3"), "IBM® OS/2 Warp 3");
    assert_eq!(format_os_display_name("os2-warp-4"), "IBM® OS/2 Warp 4");
}

#[test]
fn test_custom_names() {
    assert_eq!(
        format_os_display_name("my-first-pc"),
        "Microsoft® MS-DOS / Windows 3.1 (My First PC)"
    );
}
