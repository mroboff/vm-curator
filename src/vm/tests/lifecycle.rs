use super::*;

#[test]
fn test_generate_shared_folders_section_empty() {
    let section = generate_shared_folders_section(&[], "virtio-9p-pci");
    assert!(section.is_empty());
}

#[test]
fn test_generate_shared_folders_section_single() {
    let folders = vec![SharedFolder {
        host_path: "/home/user/Documents".to_string(),
        mount_tag: "host_documents".to_string(),
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
    assert!(section.contains(SHARED_FOLDERS_MARKER_START));
    assert!(section.contains(SHARED_FOLDERS_MARKER_END));
    assert!(section.contains("path=/home/user/Documents"));
    assert!(section.contains("mount_tag=host_documents"));
    assert!(section.contains("virtio-9p-pci"));
    assert!(section.contains("fsdev0"));
}

#[test]
fn test_generate_shared_folders_section_multiple() {
    let folders = vec![
        SharedFolder {
            host_path: "/home/user/Documents".to_string(),
            mount_tag: "host_documents".to_string(),
        },
        SharedFolder {
            host_path: "/home/user/Downloads".to_string(),
            mount_tag: "host_downloads".to_string(),
        },
    ];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
    assert!(section.contains("fsdev0"));
    assert!(section.contains("fsdev1"));
    assert!(section.contains("mount_tag=host_documents"));
    assert!(section.contains("mount_tag=host_downloads"));
}

#[test]
fn test_generate_shared_folders_section_arm() {
    let folders = vec![SharedFolder {
        host_path: "/tmp/share".to_string(),
        mount_tag: "host_share".to_string(),
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-device");
    assert!(section.contains("virtio-9p-device"));
    assert!(!section.contains("virtio-9p-pci"));
}

#[test]
fn test_generate_shared_folders_section_path_with_spaces() {
    let folders = vec![SharedFolder {
        host_path: "/home/user/My Documents".to_string(),
        mount_tag: "host_my_documents".to_string(),
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
    assert!(section.contains("'/home/user/My Documents'"));
}

#[test]
fn test_parse_shared_folders_section() {
    let content = format!(
        "{}\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path=/home/user/docs,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=host_docs\"\n{}\n",
        SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
    );
    let folders = parse_shared_folders_section(&content);
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].host_path, "/home/user/docs");
    assert_eq!(folders[0].mount_tag, "host_docs");
}

#[test]
fn test_parse_shared_folders_section_quoted_path() {
    let content = format!(
        "{}\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path='/home/user/My Documents',security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=host_my_documents\"\n{}\n",
        SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
    );
    let folders = parse_shared_folders_section(&content);
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].host_path, "/home/user/My Documents");
    assert_eq!(folders[0].mount_tag, "host_my_documents");
}

#[test]
fn test_parse_shared_folders_section_multiple() {
    let content = format!(
        "{}\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path=/home/a,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=tag_a -fsdev local,id=fsdev1,path=/home/b,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev1,mount_tag=tag_b\"\n{}\n",
        SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
    );
    let folders = parse_shared_folders_section(&content);
    assert_eq!(folders.len(), 2);
    assert_eq!(folders[0].host_path, "/home/a");
    assert_eq!(folders[0].mount_tag, "tag_a");
    assert_eq!(folders[1].host_path, "/home/b");
    assert_eq!(folders[1].mount_tag, "tag_b");
}

#[test]
fn test_remove_shared_folders_section() {
    let content = "#!/bin/bash\n# >>> Shared Folders (managed by vm-curator) >>>\nSHARED_FOLDERS_ARGS=\"...\"\n# <<< Shared Folders <<<\nqemu-system-x86_64 $SHARED_FOLDERS_ARGS\n";
    let result = remove_shared_folders_section(content);
    assert!(!result.contains("SHARED_FOLDERS"));
    assert!(!result.contains(">>> Shared Folders"));
    assert!(result.contains("qemu-system-x86_64"));
}

#[test]
fn test_insert_shared_folders_section_simple() {
    let content = "#!/bin/bash\nqemu-system-x86_64 -m 2048\n";
    let section = "# >>> Shared Folders (managed by vm-curator) >>>\nSHARED_FOLDERS_ARGS=\"-fsdev local,id=fsdev0,path=/tmp,security_model=mapped-xattr -device virtio-9p-pci,fsdev=fsdev0,mount_tag=host_tmp\"\n# <<< Shared Folders <<<\n";
    let result = insert_shared_folders_section(content, section);
    assert!(result.contains(SHARED_FOLDERS_MARKER_START));
    assert!(result.contains("$SHARED_FOLDERS_ARGS"));
    // Section should appear before QEMU command
    let marker_pos = result.find(SHARED_FOLDERS_MARKER_START).unwrap();
    let qemu_pos = result.find("qemu-system-x86_64").unwrap();
    assert!(marker_pos < qemu_pos);
}

#[test]
fn test_insert_section_before_case_statement() {
    // Scripts with case statements need the variable defined BEFORE the case,
    // not inside a branch (otherwise other branches can't see it).
    let content = "#!/bin/bash\nVM_DIR=\".\"\ncase \"$1\" in\n    --install)\n        qemu-system-x86_64 -m 2048\n        ;;\n    \"\")\n        qemu-system-x86_64 -m 2048\n        ;;\nesac\n";
    let section = "# >>> Shared Folders (managed by vm-curator) >>>\nSHARED_FOLDERS_ARGS=\"test\"\n# <<< Shared Folders <<<\n";
    let result = insert_shared_folders_section(content, section);

    // Section must appear before the case statement
    let marker_pos = result.find(SHARED_FOLDERS_MARKER_START).unwrap();
    let case_pos = result.find("case \"$1\"").unwrap();
    assert!(marker_pos < case_pos, "Section must be before case statement, got marker at {} and case at {}", marker_pos, case_pos);

    // Both QEMU commands should have $SHARED_FOLDERS_ARGS appended
    let count = result.matches("$SHARED_FOLDERS_ARGS").count();
    assert_eq!(count, 2, "Expected 2 appended refs (one per QEMU command), got {}", count);
}

#[test]
fn test_roundtrip_shared_folders() {
    let folders = vec![
        SharedFolder {
            host_path: "/home/user/Documents".to_string(),
            mount_tag: "host_documents".to_string(),
        },
        SharedFolder {
            host_path: "/home/user/My Pictures".to_string(),
            mount_tag: "host_my_pictures".to_string(),
        },
    ];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
    let parsed = parse_shared_folders_section(&section);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].host_path, "/home/user/Documents");
    assert_eq!(parsed[0].mount_tag, "host_documents");
    assert_eq!(parsed[1].host_path, "/home/user/My Pictures");
    assert_eq!(parsed[1].mount_tag, "host_my_pictures");
}

#[test]
fn test_shell_escape_safe() {
    assert_eq!(shell_escape("/home/user/docs"), "/home/user/docs");
    assert_eq!(shell_escape("my-file_name.txt"), "my-file_name.txt");
}

#[test]
fn test_shell_escape_special() {
    assert_eq!(shell_escape("/home/user/My Documents"), "'/home/user/My Documents'");
    assert_eq!(shell_escape("path with spaces"), "'path with spaces'");
}

#[test]
fn test_detect_qemu_processes_parsing() {
    // Simulate pgrep -a output parsing logic (same as detect_qemu_processes but without /proc)
    let output = "12345 qemu-system-x86_64 -m 4096 -drive file=disk.qcow2\n\
                   67890 qemu-system-aarch64 -m 2048 -hda test.img\n";
    let mut pids = Vec::new();
    let mut cmdlines = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Some(space_pos) = line.find(' ') {
            if let Ok(pid) = line[..space_pos].parse::<u32>() {
                pids.push(pid);
                cmdlines.push(line[space_pos + 1..].to_string());
            }
        }
    }
    assert_eq!(pids.len(), 2);
    assert_eq!(pids[0], 12345);
    assert!(cmdlines[0].contains("disk.qcow2"));
    assert_eq!(pids[1], 67890);
    assert!(cmdlines[1].contains("test.img"));
}
