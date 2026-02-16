use super::*;

#[test]
fn test_load_embedded_profiles() {
    let store = QemuProfileStore::load_embedded();
    assert!(!store.is_empty(), "Should have loaded some profiles");

    // Check that some expected profiles exist
    assert!(store.get("windows-10").is_some(), "Should have Windows 10");
    assert!(store.get("linux-debian").is_some(), "Should have Debian");
    assert!(store.get("freebsd").is_some(), "Should have FreeBSD");
}

#[test]
fn test_profile_summary() {
    let profile = QemuProfile {
        display_name: "Test OS".to_string(),
        memory_mb: 4096,
        disk_size_gb: 64,
        uefi: true,
        disk_interface: "virtio".to_string(),
        ..Default::default()
    };

    let summary = profile.summary();
    assert!(summary.contains("4GB RAM"));
    assert!(summary.contains("64GB"));
    assert!(summary.contains("UEFI"));
    assert!(summary.contains("virtio"));
}

#[test]
fn test_categories() {
    let store = QemuProfileStore::load_embedded();
    let categories = store.categories();

    assert!(categories.contains(&"windows".to_string()));
    assert!(categories.contains(&"linux".to_string()));
    assert!(categories.contains(&"bsd".to_string()));
}

#[test]
fn test_search() {
    let store = QemuProfileStore::load_embedded();

    let results = store.search("windows");
    assert!(!results.is_empty(), "Should find Windows profiles");

    let results = store.search("debian");
    assert!(!results.is_empty(), "Should find Debian profiles");
}

#[test]
fn test_classic_mac_bios_rom_config() {
    let store = QemuProfileStore::load_embedded();

    // m68k Mac profiles should have required bios_rom
    let system6 = store.get("mac-system6").expect("Should have mac-system6");
    assert!(system6.bios_rom.is_some(), "mac-system6 should have bios_rom config");
    assert!(system6.bios_rom.as_ref().unwrap().required, "mac-system6 bios_rom should be required");

    let system7 = store.get("mac-system7").expect("Should have mac-system7");
    assert!(system7.bios_rom.is_some(), "mac-system7 should have bios_rom config");
    assert!(system7.bios_rom.as_ref().unwrap().required, "mac-system7 bios_rom should be required");

    // PPC Mac profiles should have optional bios_rom
    let os8 = store.get("mac-os8").expect("Should have mac-os8");
    assert!(os8.bios_rom.is_some(), "mac-os8 should have bios_rom config");
    assert!(!os8.bios_rom.as_ref().unwrap().required, "mac-os8 bios_rom should be optional");

    let os9 = store.get("mac-os9").expect("Should have mac-os9");
    assert!(os9.bios_rom.is_some(), "mac-os9 should have bios_rom config");
    assert!(!os9.bios_rom.as_ref().unwrap().required, "mac-os9 bios_rom should be optional");
}

#[test]
fn test_non_mac_profiles_no_bios_rom() {
    let store = QemuProfileStore::load_embedded();

    let win10 = store.get("windows-10").expect("Should have windows-10");
    assert!(win10.bios_rom.is_none(), "windows-10 should not have bios_rom");

    let debian = store.get("linux-debian").expect("Should have linux-debian");
    assert!(debian.bios_rom.is_none(), "linux-debian should not have bios_rom");
}

#[test]
fn test_free_iso_profiles() {
    let store = QemuProfileStore::load_embedded();
    let free_profiles = store.list_with_free_iso();

    // Should have at least some free/open-source OSes
    assert!(
        !free_profiles.is_empty(),
        "Should have profiles with free ISOs"
    );

    // Check that a known free OS is in the list
    let has_debian = free_profiles.iter().any(|(id, _)| *id == "linux-debian");
    assert!(has_debian, "Debian should have a free ISO URL");
}
