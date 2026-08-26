use super::*;

#[test]
fn load_from_missing_path_returns_empty() {
    let groups = load_groups(Path::new("/nonexistent/vm-curator/groups.toml"));
    assert!(groups.is_empty());
}

#[test]
fn save_then_load_roundtrips_order_and_membership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("groups.toml");

    let groups = vec![
        VmGroup {
            name: "Servers".to_string(),
            vm_ids: vec!["debian-12".to_string(), "ubuntu-24-04".to_string()],
        },
        VmGroup::new("Desktops"),
    ];

    save_groups(&path, &groups).unwrap();
    assert!(path.exists());

    let loaded = load_groups(&path);
    assert_eq!(loaded, groups);
}

#[test]
fn load_from_malformed_toml_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("groups.toml");
    std::fs::write(&path, "this is not = valid = toml = [[[").unwrap();

    assert!(load_groups(&path).is_empty());
}

#[test]
fn vm_group_contains_checks_membership() {
    let group = VmGroup {
        name: "Servers".to_string(),
        vm_ids: vec!["debian-12".to_string()],
    };
    assert!(group.contains("debian-12"));
    assert!(!group.contains("ubuntu-24-04"));
}

#[test]
fn prune_stale_removes_missing_vm_ids_only() {
    let mut groups = vec![
        VmGroup {
            name: "Servers".to_string(),
            vm_ids: vec!["debian-12".to_string(), "deleted-vm".to_string()],
        },
        VmGroup {
            name: "Desktops".to_string(),
            vm_ids: vec!["deleted-vm".to_string()],
        },
    ];
    let valid_ids: std::collections::HashSet<&str> = ["debian-12"].into_iter().collect();

    let changed = prune_stale(&mut groups, &valid_ids);

    assert!(changed);
    assert_eq!(groups[0].vm_ids, vec!["debian-12".to_string()]);
    assert!(groups[1].vm_ids.is_empty());
}

#[test]
fn prune_stale_no_op_when_all_ids_valid() {
    let mut groups = vec![VmGroup {
        name: "Servers".to_string(),
        vm_ids: vec!["debian-12".to_string()],
    }];
    let valid_ids: std::collections::HashSet<&str> =
        ["debian-12", "ubuntu-24-04"].into_iter().collect();

    let changed = prune_stale(&mut groups, &valid_ids);

    assert!(!changed);
    assert_eq!(groups[0].vm_ids, vec!["debian-12".to_string()]);
}

#[test]
fn groups_file_path_ends_with_expected_segments() {
    let path = groups_file_path();
    assert!(path.ends_with("vm-curator/groups.toml"));
}

fn category_order() -> HashMap<String, i32> {
    [
        ("Windows".to_string(), 0),
        ("Linux".to_string(), 1),
        ("Other".to_string(), 99),
    ]
    .into_iter()
    .collect()
}

#[test]
fn assign_to_category_groups_creates_groups_in_category_order() {
    let mut groups = Vec::new();
    let candidates = vec![
        ("ubuntu".to_string(), "Linux".to_string()),
        ("windows-11".to_string(), "Windows".to_string()),
    ];

    let changed = assign_to_category_groups(&mut groups, &candidates, &category_order());

    assert!(changed);
    // "Windows" (order 0) sorts before "Linux" (order 1) even though the
    // candidate list processed Linux first.
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "Windows");
    assert_eq!(groups[0].vm_ids, vec!["windows-11".to_string()]);
    assert_eq!(groups[1].name, "Linux");
    assert_eq!(groups[1].vm_ids, vec!["ubuntu".to_string()]);
}

#[test]
fn assign_to_category_groups_reuses_existing_group_with_matching_name() {
    let mut groups = vec![VmGroup {
        name: "Linux".to_string(),
        vm_ids: vec!["debian-12".to_string()],
    }];
    let candidates = vec![("ubuntu".to_string(), "Linux".to_string())];

    let changed = assign_to_category_groups(&mut groups, &candidates, &category_order());

    assert!(changed);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].vm_ids,
        vec!["debian-12".to_string(), "ubuntu".to_string()]
    );
}

#[test]
fn assign_to_category_groups_never_overrides_existing_membership() {
    // "ubuntu" was manually moved into "Staging" — its category default
    // must not fight that choice.
    let mut groups = vec![VmGroup {
        name: "Staging".to_string(),
        vm_ids: vec!["ubuntu".to_string()],
    }];
    let candidates = vec![("ubuntu".to_string(), "Linux".to_string())];

    let changed = assign_to_category_groups(&mut groups, &candidates, &category_order());

    assert!(!changed);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "Staging");
}

#[test]
fn assign_to_category_groups_new_category_slots_after_custom_group() {
    // A custom group (no category order entry) is treated as "after
    // everything" — a new category group still lands before it since every
    // real category has a finite order and the custom group falls back to
    // i32::MAX.
    let mut groups = vec![VmGroup::new("My Custom Group")];
    let candidates = vec![("debian-12".to_string(), "Linux".to_string())];

    assign_to_category_groups(&mut groups, &candidates, &category_order());

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "Linux");
    assert_eq!(groups[1].name, "My Custom Group");
}
