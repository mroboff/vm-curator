use super::*;

#[test]
fn test_validate_launch_script_accepts_valid_bash() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("launch.sh");
    std::fs::write(&script, "#!/usr/bin/env bash\nqemu-system-x86_64 -m 512\n").unwrap();
    assert!(validate_launch_script(&script).is_ok());
}

#[test]
fn test_validate_launch_script_reports_syntax_error() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("launch.sh");
    std::fs::write(
        &script,
        "#!/usr/bin/env bash\nVM_CURATOR_VIDEO_DEVICE=virtio-device virtio-vga-gl=(-device virtio-vga-gl)\n",
    )
    .unwrap();
    let error = validate_launch_script(&script).unwrap_err().to_string();
    assert!(error.contains("syntax error"));
}

#[test]
fn test_resolve_backing_path_relative_to_overlay() {
    let disk = Path::new("/vms/test/disk.qcow2");
    assert_eq!(
        resolve_backing_path(disk, "base.qcow2"),
        PathBuf::from("/vms/test/base.qcow2")
    );
    assert_eq!(
        resolve_backing_path(disk, "/images/base.qcow2"),
        PathBuf::from("/images/base.qcow2")
    );
}

#[test]
fn test_reset_temp_path_stays_in_disk_directory() {
    let disk = Path::new("/vms/test/disk.qcow2");
    let temp = reset_temp_path(disk).unwrap();
    assert_eq!(temp.parent(), disk.parent());
    assert_ne!(temp, disk);
    assert!(temp
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with(".qcow2"));
}

#[test]
fn test_replace_display_for_dbus_strips_spice_agent_channel() {
    use crate::vm::create::SPICE_AGENT_ARGS;

    // A spice-app launch command with the clipboard channel present.
    let script = "#!/bin/bash\nqemu-system-x86_64 \\\n        -display spice-app \\\n        -device virtio-serial-pci \\\n        -chardev spicevmc,id=spicechannel0,name=vdagent \\\n        -device virtserialport,chardev=spicechannel0,name=com.redhat.spice.0 \\\n        -qmp unix:sock,server=on,wait=off\n";

    let dbus = replace_display_for_dbus(script, "-display dbus");

    for arg in SPICE_AGENT_ARGS {
        assert!(
            !dbus.contains(arg),
            "dbus script should not contain agent arg `{}`",
            arg
        );
    }
    assert!(dbus.contains("-display dbus"), "display swapped to dbus");
}

fn test_vm(vm_dir: &std::path::Path) -> DiscoveredVm {
    DiscoveredVm {
        id: "test-vm".to_string(),
        path: vm_dir.to_path_buf(),
        launch_script: vm_dir.join("launch.sh"),
        config: crate::vm::QemuConfig::default(),
        custom_name: None,
        os_profile: None,
        notes: None,
    }
}

#[test]
fn test_save_usb_passthrough_persists_in_launch_script() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 -m 2048\n",
    )
    .unwrap();
    let devices = vec![UsbPassthrough {
        vendor_id: 0x413c,
        product_id: 0x2113,
        usb_version: crate::hardware::UsbVersion::Usb2,
        bootindex: None,
    }];

    save_usb_passthrough(&vm, &devices).unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(script.contains(USB_MARKER_START));
    assert!(script.contains("USB_PASSTHROUGH_ARGS=\"-usb"));
    assert!(script.contains("vendorid=0x413c,productid=0x2113"));
    assert!(script.contains("qemu-system-x86_64 -m 2048 $USB_PASSTHROUGH_ARGS"));

    let loaded = load_usb_passthrough(&vm);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].vendor_id, 0x413c);
    assert_eq!(loaded[0].product_id, 0x2113);
}

#[test]
fn test_save_usb_passthrough_persists_usb3_controller() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 -m 2048\n",
    )
    .unwrap();
    let devices = vec![UsbPassthrough {
        vendor_id: 0x413c,
        product_id: 0x2113,
        usb_version: crate::hardware::UsbVersion::Usb3,
        bootindex: None,
    }];

    save_usb_passthrough(&vm, &devices).unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(script.contains("-device qemu-xhci,id=xhci,p2=8,p3=8"));
    assert!(script.contains("usb-host,bus=xhci.0,vendorid=0x413c,productid=0x2113"));
    assert_eq!(
        load_usb_passthrough(&vm)[0].usb_version,
        crate::hardware::UsbVersion::Usb3
    );
}

#[test]
fn test_window_size_parse_accepts_common_format() {
    assert_eq!(
        WindowSize::parse(" 1440x900 "),
        Some(WindowSize {
            width: 1440,
            height: 900,
        })
    );
    assert_eq!(
        WindowSize::parse("1920X1080"),
        Some(WindowSize {
            width: 1920,
            height: 1080,
        })
    );
}

#[test]
fn test_window_size_parse_rejects_invalid_values() {
    assert_eq!(WindowSize::parse("1440"), None);
    assert_eq!(WindowSize::parse("1440x"), None);
    assert_eq!(WindowSize::parse("100x100"), None);
}

#[test]
fn test_build_launch_invocation_sets_window_size_env() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    let options = LaunchOptions {
        window_size: WindowSize::parse("1440x900"),
        extra_args: vec!["--dry-run".to_string()],
        ..LaunchOptions::default()
    };

    let invocation = build_launch_invocation(&vm, &options).unwrap();

    assert_eq!(invocation.program, "bash");
    assert_eq!(invocation.current_dir, vm.path);
    assert_eq!(
        invocation.args[0],
        vm.launch_script.to_string_lossy().to_string()
    );
    assert!(invocation.args.contains(&"--dry-run".to_string()));
    assert_eq!(
        invocation.env,
        vec![(
            "VM_CURATOR_WINDOW_SIZE".to_string(),
            Some("1440x900".to_string())
        )]
    );
}

#[test]
fn test_build_launch_invocation_removes_window_size_env_when_unset() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());

    let invocation = build_launch_invocation(&vm, &LaunchOptions::default()).unwrap();

    assert_eq!(
        invocation.env,
        vec![("VM_CURATOR_WINDOW_SIZE".to_string(), None)]
    );
}

#[test]
fn test_build_launch_invocation_validates_boot_media_before_spawning() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    let options = LaunchOptions {
        boot_mode: BootMode::Cdrom(dir.path().join("missing.iso")),
        ..LaunchOptions::default()
    };

    let err = build_launch_invocation(&vm, &options).unwrap_err();

    assert!(err.contains("ISO file not found"));
}

#[test]
fn test_patch_window_size_override_migrates_existing_vga_script() {
    let script = r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/test.qcow2"

case "$1" in
    "")
        qemu-system-x86_64 \
        -m 2048M \
        -vga std \
        -display gtk
        ;;
esac
"#;
    let expected = r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/test.qcow2"

# >>> Window size override (managed by vm-curator) >>>
VM_CURATOR_VIDEO_DEVICE=VGA
VM_CURATOR_VIDEO_ARGS=(-vga std)
if [[ -n "${VM_CURATOR_WINDOW_SIZE:-}" && -n "$VM_CURATOR_VIDEO_DEVICE" ]]; then
    if [[ "$VM_CURATOR_WINDOW_SIZE" =~ ^([0-9]+)[xX]([0-9]+)$ ]]; then
        VM_CURATOR_WIDTH="${BASH_REMATCH[1]}"
        VM_CURATOR_HEIGHT="${BASH_REMATCH[2]}"
        if (( VM_CURATOR_WIDTH >= 320 && VM_CURATOR_WIDTH <= 16384 && VM_CURATOR_HEIGHT >= 200 && VM_CURATOR_HEIGHT <= 16384 )); then
            VM_CURATOR_VIDEO_ARGS=(-device "${VM_CURATOR_VIDEO_DEVICE},xres=${VM_CURATOR_WIDTH},yres=${VM_CURATOR_HEIGHT}")
        fi
    fi
fi
# <<< Window size override (managed by vm-curator) <<<
case "$1" in
    "")
        qemu-system-x86_64 \
        -m 2048M \
        "${VM_CURATOR_VIDEO_ARGS[@]}" \
        -display gtk
        ;;
esac
"#;

    let patched = patch_window_size_override(script).expect("vga script should be patchable");

    assert_eq!(patched, expected);
    assert!(patch_window_size_override(&patched).is_none());
}

#[test]
fn test_patch_window_size_override_migrates_existing_gl_device_script() {
    let script = r#"#!/bin/bash
qemu-system-x86_64 \
    -device virtio-vga-gl \
    -display gtk,gl=on
"#;
    let expected = r#"#!/bin/bash
# >>> Window size override (managed by vm-curator) >>>
VM_CURATOR_VIDEO_DEVICE=virtio-vga-gl
VM_CURATOR_VIDEO_ARGS=(-device virtio-vga-gl)
if [[ -n "${VM_CURATOR_WINDOW_SIZE:-}" && -n "$VM_CURATOR_VIDEO_DEVICE" ]]; then
    if [[ "$VM_CURATOR_WINDOW_SIZE" =~ ^([0-9]+)[xX]([0-9]+)$ ]]; then
        VM_CURATOR_WIDTH="${BASH_REMATCH[1]}"
        VM_CURATOR_HEIGHT="${BASH_REMATCH[2]}"
        if (( VM_CURATOR_WIDTH >= 320 && VM_CURATOR_WIDTH <= 16384 && VM_CURATOR_HEIGHT >= 200 && VM_CURATOR_HEIGHT <= 16384 )); then
            VM_CURATOR_VIDEO_ARGS=(-device "${VM_CURATOR_VIDEO_DEVICE},xres=${VM_CURATOR_WIDTH},yres=${VM_CURATOR_HEIGHT}")
        fi
    fi
fi
# <<< Window size override (managed by vm-curator) <<<
qemu-system-x86_64 \
    "${VM_CURATOR_VIDEO_ARGS[@]}" \
    -display gtk,gl=on
"#;

    let patched =
        patch_window_size_override(script).expect("virtio-vga-gl script should be patchable");

    assert_eq!(patched, expected);
    assert!(patch_window_size_override(&patched).is_none());
}

#[test]
fn test_patch_window_size_override_leaves_unsupported_or_mixed_video_unchanged() {
    let unsupported = r#"#!/bin/bash
qemu-system-x86_64 \
    -vga cirrus \
    -display gtk
"#;
    assert!(patch_window_size_override(unsupported).is_none());

    let mixed = r#"#!/bin/bash
case "$1" in
    --install)
        qemu-system-x86_64 \
        -vga std
        ;;
    "")
        qemu-system-x86_64 \
        -vga virtio
        ;;
esac
"#;
    assert!(patch_window_size_override(mixed).is_none());
}

#[test]
fn test_parse_supported_video_arg_line_parses_supported_video_forms() {
    let cases = [
        ("-vga std", "-vga", "std", "VGA"),
        ("        -vga virtio \\", "-vga", "virtio", "virtio-vga"),
        ("-vga 'qxl'", "-vga", "qxl", "qxl-vga"),
        ("-device VGA", "-device", "VGA", "VGA"),
        (
            "    -device virtio-vga \\",
            "-device",
            "virtio-vga",
            "virtio-vga",
        ),
        (
            "-device \"virtio-vga-gl\"",
            "-device",
            "virtio-vga-gl",
            "virtio-vga-gl",
        ),
        ("-device 'qxl-vga'", "-device", "qxl-vga", "qxl-vga"),
    ];

    for (line, default_flag, default_value, override_device) in cases {
        let spec = parse_supported_video_arg_line(line)
            .unwrap_or_else(|| panic!("expected supported video arg line: {line}"));

        assert_eq!(spec.default_flag, default_flag);
        assert_eq!(spec.default_value, default_value);
        assert_eq!(spec.override_device, override_device);
    }
}

#[test]
fn test_parse_supported_video_arg_line_rejects_unsupported_forms() {
    for line in [
        "",
        "   ",
        "# -vga std",
        "-vga cirrus",
        "-vga std extra",
        "-device cirrus-vga",
        "-device virtio-vga,hostmem=256M",
        "-device virtio-vga extra",
    ] {
        assert!(
            parse_supported_video_arg_line(line).is_none(),
            "expected unsupported video arg line: {line}"
        );
    }
}

#[test]
fn test_ensure_window_size_override_in_script_writes_patched_script() {
    let script = r#"#!/bin/bash
qemu-system-x86_64 \
    -vga std \
    -display gtk
"#;
    let expected = patch_window_size_override(script).expect("vga script should be patchable");
    let dir = tempfile::tempdir().expect("create temp dir for launch script");
    let script_path = dir.path().join("launch.sh");
    std::fs::write(&script_path, script).expect("write launch script fixture");

    ensure_window_size_override_in_script(&script_path).expect("patch launch script on disk");

    let patched = std::fs::read_to_string(&script_path).expect("read patched launch script");
    assert_eq!(patched, expected);
}

#[test]
fn test_ensure_window_size_override_in_script_leaves_unsupported_script_unchanged() {
    let script = r#"#!/bin/bash
qemu-system-x86_64 \
    -vga cirrus \
    -display gtk
"#;
    let dir = tempfile::tempdir().expect("create temp dir for launch script");
    let script_path = dir.path().join("launch.sh");
    std::fs::write(&script_path, script).expect("write launch script fixture");

    ensure_window_size_override_in_script(&script_path)
        .expect("leave unsupported script unchanged");

    let actual = std::fs::read_to_string(&script_path).expect("read launch script");
    assert_eq!(actual, script);
}

#[test]
fn test_ensure_window_size_override_in_script_reports_missing_script() {
    let dir = tempfile::tempdir().expect("create temp dir for launch script");
    let script_path = dir.path().join("missing-launch.sh");

    let err = ensure_window_size_override_in_script(&script_path)
        .expect_err("missing launch script should return an error");

    assert!(err.to_string().contains("Failed to read launch script"));
}

#[test]
fn test_generate_shared_folders_section_empty() {
    let section = generate_shared_folders_section(&[], "virtio-9p-pci", None);
    assert!(section.is_empty());
}

#[test]
fn test_generate_shared_folders_section_single() {
    let folders = vec![SharedFolder {
        host_path: "/home/user/Documents".to_string(),
        mount_tag: "host_documents".to_string(),
        mode: ShareMode::NineP,
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", None);
    assert!(section.contains(SHARED_FOLDERS_MARKER_START));
    assert!(section.contains(SHARED_FOLDERS_MARKER_END));
    assert!(section.contains("SHARED_FOLDERS_ARGS=("));
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
            mode: ShareMode::NineP,
        },
        SharedFolder {
            host_path: "/home/user/Downloads".to_string(),
            mount_tag: "host_downloads".to_string(),
            mode: ShareMode::NineP,
        },
    ];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", None);
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
        mode: ShareMode::NineP,
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-device", None);
    assert!(section.contains("virtio-9p-device"));
    assert!(!section.contains("virtio-9p-pci"));
}

#[test]
fn test_generate_shared_folders_section_path_with_spaces() {
    let folders = vec![SharedFolder {
        host_path: "/home/user/My Documents".to_string(),
        mount_tag: "host_my_documents".to_string(),
        mode: ShareMode::NineP,
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", None);
    assert!(section.contains("path=/home/user/My Documents"));
    assert!(section.contains("SHARED_FOLDERS_ARGS=("));
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
fn test_parse_shared_folders_section_array() {
    let content = format!(
        "{}\nSHARED_FOLDERS_ARGS=(\n    -fsdev\n    'local,id=fsdev0,path=/home/user/My Documents,security_model=mapped-xattr'\n    -device\n    'virtio-9p-pci,fsdev=fsdev0,mount_tag=host_my_documents'\n)\n{}\n",
        SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
    );
    let folders = parse_shared_folders_section(&content);
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].host_path, "/home/user/My Documents");
    assert_eq!(folders[0].mount_tag, "host_my_documents");
}

#[test]
fn test_parse_shared_folders_section_array_closes_on_final_arg_line() {
    let content = format!(
        "{}\nSHARED_FOLDERS_ARGS=(\n    -fsdev\n    'local,id=fsdev0,path=/home/user/My Documents,security_model=mapped-xattr'\n    -device\n    'virtio-9p-pci,fsdev=fsdev0,mount_tag=host_my_documents' )\n{}\nqemu-system-x86_64 \"${{SHARED_FOLDERS_ARGS[@]}}\"\n",
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
    let folders = vec![SharedFolder {
        host_path: "/tmp".to_string(),
        mount_tag: "host_tmp".to_string(),
        mode: ShareMode::NineP,
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", None);
    let result = insert_shared_folders_section(content, &section);
    assert!(result.contains(SHARED_FOLDERS_MARKER_START));
    assert!(result.contains(SHARED_FOLDERS_ARGS_REF));
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
    assert!(
        marker_pos < case_pos,
        "Section must be before case statement, got marker at {} and case at {}",
        marker_pos,
        case_pos
    );

    // Both QEMU commands should have the shared-folder array expansion appended.
    let count = result.matches(SHARED_FOLDERS_ARGS_REF).count();
    assert_eq!(
        count, 2,
        "Expected 2 appended refs (one per QEMU command), got {}",
        count
    );
}

#[test]
fn test_save_shared_folders_preserves_uefi_disk_boot_order() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        r#"#!/bin/bash
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/linux.raw"
OVMF_VARS="$VM_DIR/OVMF_VARS.fd"
case "$1" in
    "")
        qemu-system-x86_64 \
        -drive if=pflash,format=raw,file="$OVMF_VARS" \
        -global virtio-blk-pci.bootindex=0 \
        -drive file="$DISK",format=raw,if=virtio,index=0,media=disk \
        -boot strict=on \
        -qmp \
        unix:$VM_DIR/qemu.sock,server=on,wait=off
        ;;
esac
"#,
    )
    .unwrap();
    let folders = vec![SharedFolder {
        host_path: "/home/user/shared".to_string(),
        mount_tag: "host_shared".to_string(),
        mode: ShareMode::NineP,
    }];

    save_shared_folders(&vm, &folders).unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert_eq!(
        script.matches("-global virtio-blk-pci.bootindex=0").count(),
        1
    );
    assert_eq!(script.matches("-boot strict=on").count(), 1);
    assert!(script.contains("-drive file=\"$DISK\",format=raw,if=virtio,index=0,media=disk"));
    assert!(script.contains("\"${SHARED_FOLDERS_ARGS[@]}\""));
}

#[test]
fn test_roundtrip_shared_folders() {
    let folders = vec![
        SharedFolder {
            host_path: "/home/user/Documents".to_string(),
            mount_tag: "host_documents".to_string(),
            mode: ShareMode::NineP,
        },
        SharedFolder {
            host_path: "/home/user/My Pictures".to_string(),
            mount_tag: "host_my_pictures".to_string(),
            mode: ShareMode::NineP,
        },
    ];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", None);
    let parsed = parse_shared_folders_section(&section);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].host_path, "/home/user/Documents");
    assert_eq!(parsed[0].mount_tag, "host_documents");
    assert_eq!(parsed[1].host_path, "/home/user/My Pictures");
    assert_eq!(parsed[1].mount_tag, "host_my_pictures");
}

#[test]
fn test_shared_folders_array_survives_shell_expansion() {
    let folders = vec![SharedFolder {
        host_path: "/home/user/O'Brien Documents".to_string(),
        mount_tag: "host_obrien_documents".to_string(),
        mode: ShareMode::NineP,
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", None);
    let script = format!(
        "{}\nset -- qemu-system-x86_64 {}\nprintf '%s\\0' \"$@\"\n",
        section, SHARED_FOLDERS_ARGS_REF
    );

    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("bash should run shared-folder expansion test");

    assert!(
        output.status.success(),
        "bash failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args: Vec<String> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8(part.to_vec()).unwrap())
        .collect();

    assert_eq!(
        args,
        vec![
            "qemu-system-x86_64".to_string(),
            "-fsdev".to_string(),
            "local,id=fsdev0,path=/home/user/O'Brien Documents,security_model=mapped-xattr"
                .to_string(),
            "-device".to_string(),
            "virtio-9p-pci,fsdev=fsdev0,mount_tag=host_obrien_documents".to_string(),
        ]
    );
}

#[test]
fn test_shell_escape_safe() {
    assert_eq!(shell_escape("/home/user/docs"), "/home/user/docs");
    assert_eq!(shell_escape("my-file_name.txt"), "my-file_name.txt");
}

#[test]
fn test_shell_escape_special() {
    assert_eq!(
        shell_escape("/home/user/My Documents"),
        "'/home/user/My Documents'"
    );
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
        if line.is_empty() {
            continue;
        }
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

#[test]
fn test_parse_pci_section_empty() {
    let content = "#!/bin/bash\nqemu-system-x86_64 -m 2048\n";
    let args = parse_pci_section(content);
    assert!(args.is_empty());
}

#[test]
fn test_parse_pci_section_single_device() {
    let content = format!(
        "#!/bin/bash\n{}\nPCI_PASSTHROUGH_ARGS=\"-device vfio-pci,host=0000:01:00.0\"\n{}\nqemu-system-x86_64 $PCI_PASSTHROUGH_ARGS\n",
        PCI_MARKER_START, PCI_MARKER_END
    );
    let args = parse_pci_section(&content);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "-device vfio-pci,host=0000:01:00.0");
}

#[test]
fn test_parse_pci_section_multiple_devices() {
    let content = format!(
        "#!/bin/bash\n{}\nPCI_PASSTHROUGH_ARGS=\"-device vfio-pci,host=0000:01:00.0 -device vfio-pci,host=0000:01:00.1\"\n{}\nqemu-system-x86_64 $PCI_PASSTHROUGH_ARGS\n",
        PCI_MARKER_START, PCI_MARKER_END
    );
    let args = parse_pci_section(&content);
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "-device vfio-pci,host=0000:01:00.0");
    assert_eq!(args[1], "-device vfio-pci,host=0000:01:00.1");
}

#[test]
fn test_parse_pci_section_with_multifunction() {
    let content = format!(
        "#!/bin/bash\n{}\nPCI_PASSTHROUGH_ARGS=\"-device vfio-pci,host=0000:01:00.0,multifunction=on\"\n{}\n",
        PCI_MARKER_START, PCI_MARKER_END
    );
    let args = parse_pci_section(&content);
    assert_eq!(args.len(), 1);
    assert_eq!(
        args[0],
        "-device vfio-pci,host=0000:01:00.0,multifunction=on"
    );
}

#[test]
fn test_parse_pci_section_with_vfio_bind_functions() {
    // The new PCI section format includes VFIO bind/unbind functions alongside
    // PCI_PASSTHROUGH_ARGS. The parser should still extract only the QEMU args.
    let content = format!(
        "#!/bin/bash\n\
        {}\n\
        PCI_PASSTHROUGH_ARGS=\"-device vfio-pci,host=0000:10:00.0,multifunction=on -device vfio-pci,host=0000:10:00.1\"\n\
        PCI_DEVICES=(\"0000:10:00.0\" \"0000:10:00.1\")\n\
        declare -A PCI_ORIG_DRIVERS\n\
        \n\
        bind_vfio() {{\n\
            echo \"binding\"\n\
        }}\n\
        restore_pci() {{\n\
            echo \"restoring\"\n\
        }}\n\
        bind_vfio || exit 1\n\
        {}\n",
        PCI_MARKER_START, PCI_MARKER_END
    );
    let args = parse_pci_section(&content);
    assert_eq!(args.len(), 2);
    assert_eq!(
        args[0],
        "-device vfio-pci,host=0000:10:00.0,multifunction=on"
    );
    assert_eq!(args[1], "-device vfio-pci,host=0000:10:00.1");
}

#[test]
fn test_ensure_qmp_repairs_unquoted_socket_path() {
    // Scripts generated by v1.0.0–v1.2.1 carry an unquoted socket path that
    // word-splits when the VM library path contains spaces (issue #65).
    let tmp = tempfile::tempdir().unwrap();
    let script = "#!/bin/bash\n\
        qemu-system-x86_64 \\\n\
        -m 2048 \\\n\
        -qmp \\\n\
        unix:$VM_DIR/qemu.sock,server=on,wait=off\n\
        ;;\n";
    std::fs::write(tmp.path().join("launch.sh"), script).unwrap();

    ensure_qmp_in_script(tmp.path()).unwrap();

    let repaired = std::fs::read_to_string(tmp.path().join("launch.sh")).unwrap();
    assert!(
        repaired.contains("unix:\"$VM_DIR/qemu.sock\",server=on,wait=off"),
        "socket path should be quoted:\n{repaired}"
    );
    assert!(!repaired.contains("unix:$VM_DIR/qemu.sock,"));
}

#[test]
fn test_ensure_qmp_repair_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let script = "#!/bin/bash\n\
        qemu-system-x86_64 \\\n\
        -qmp unix:$VM_DIR/qemu.sock,server=on,wait=off\n";
    std::fs::write(tmp.path().join("launch.sh"), script).unwrap();

    ensure_qmp_in_script(tmp.path()).unwrap();
    let first = std::fs::read_to_string(tmp.path().join("launch.sh")).unwrap();
    ensure_qmp_in_script(tmp.path()).unwrap();
    let second = std::fs::read_to_string(tmp.path().join("launch.sh")).unwrap();

    assert_eq!(first, second, "second repair pass must be a no-op");
    assert!(first.contains("unix:\"$VM_DIR/qemu.sock\""));
    assert!(!first.contains("unix:\"\"$VM_DIR"), "must not double-quote");
}

#[test]
fn test_ensure_qmp_retrofit_inserts_quoted_arg() {
    // A pre-1.0 script with no QMP line at all gets the arg patched in,
    // and the patched-in form must be the quoted one.
    let tmp = tempfile::tempdir().unwrap();
    let script = "#!/bin/bash\n\
case \"$1\" in\n\
    *)\n\
        qemu-system-x86_64 \\\n\
        -m 2048 \\\n\
        -display gtk\n\
        ;;\n\
esac\n";
    std::fs::write(tmp.path().join("launch.sh"), script).unwrap();

    ensure_qmp_in_script(tmp.path()).unwrap();

    let patched = std::fs::read_to_string(tmp.path().join("launch.sh")).unwrap();
    assert!(
        patched.contains("-qmp unix:\"$VM_DIR/qemu.sock\",server=on,wait=off"),
        "retrofitted QMP arg should be quoted:\n{patched}"
    );
}

#[test]
fn test_usb_bootindex_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 -m 2048\n",
    )
    .unwrap();
    let devices = vec![
        UsbPassthrough {
            vendor_id: 0x0781,
            product_id: 0x5567,
            usb_version: crate::hardware::UsbVersion::Usb3,
            bootindex: Some(1),
        },
        UsbPassthrough {
            vendor_id: 0x413c,
            product_id: 0x2113,
            usb_version: crate::hardware::UsbVersion::Usb2,
            bootindex: None,
        },
    ];

    save_usb_passthrough(&vm, &devices).unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(script.contains("vendorid=0x0781,productid=0x5567,bootindex=1"));
    assert!(script.contains("vendorid=0x413c,productid=0x2113"));
    assert!(!script.contains("productid=0x2113,bootindex"));

    let loaded = load_usb_passthrough(&vm);
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].bootindex, Some(1));
    assert_eq!(loaded[1].bootindex, None);
}

#[test]
fn test_disk_passthrough_save_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 -m 2048\n",
    )
    .unwrap();
    let disks = vec![
        DiskPassthrough {
            path: "/dev/disk/by-id/nvme-Samsung_SSD_990_PRO_1TB_S6B0NS0W123456".to_string(),
            bootindex: Some(0),
        },
        DiskPassthrough {
            path: "/dev/disk/by-id/usb-SanDisk_Ultra_0101-0:0".to_string(),
            bootindex: None,
        },
    ];

    save_disk_passthrough(&vm, &disks).unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(script.contains("# >>> Disk Passthrough (managed by vm-curator) >>>"));
    assert!(script.contains(
        "-drive file=/dev/disk/by-id/nvme-Samsung_SSD_990_PRO_1TB_S6B0NS0W123456,format=raw,if=none,id=pdisk0,cache=none"
    ));
    assert!(script.contains("-device virtio-blk-pci,drive=pdisk0,bootindex=0"));
    assert!(script.contains("-device virtio-blk-pci,drive=pdisk1 "));
    assert!(script.contains("-boot menu=on"));
    // Preflight guards present
    assert!(script.contains("if [[ ! -b \"$_pdisk\" ]]"));
    assert!(script.contains("no read/write access"));
    assert!(script.contains("mounted partitions"));
    // Args appended to the qemu command
    assert!(script.contains("qemu-system-x86_64 -m 2048 $DISK_PASSTHROUGH_ARGS"));

    let loaded = load_disk_passthrough(&vm);
    assert_eq!(loaded, disks);
}

#[test]
fn test_disk_passthrough_remove_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    let original = "#!/bin/bash\nqemu-system-x86_64 -m 2048\n";
    std::fs::write(&vm.launch_script, original).unwrap();

    let disks = vec![DiskPassthrough {
        path: "/dev/sdb".to_string(),
        bootindex: None,
    }];
    save_disk_passthrough(&vm, &disks).unwrap();
    assert!(!load_disk_passthrough(&vm).is_empty());

    // Saving an empty list removes the section and the args reference
    save_disk_passthrough(&vm, &[]).unwrap();
    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(!script.contains("Disk Passthrough"));
    assert!(!script.contains("$DISK_PASSTHROUGH_ARGS"));
    assert!(load_disk_passthrough(&vm).is_empty());
}

#[test]
fn test_disk_passthrough_coexists_with_usb_section() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 -m 2048\n",
    )
    .unwrap();

    save_usb_passthrough(
        &vm,
        &[UsbPassthrough {
            vendor_id: 0x413c,
            product_id: 0x2113,
            usb_version: crate::hardware::UsbVersion::Usb2,
            bootindex: None,
        }],
    )
    .unwrap();
    save_disk_passthrough(
        &vm,
        &[DiskPassthrough {
            path: "/dev/sdc".to_string(),
            bootindex: Some(2),
        }],
    )
    .unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    let qemu_line = script
        .lines()
        .find(|l| l.trim_start().starts_with("qemu-system-x86_64"))
        .unwrap();
    assert!(qemu_line.contains("$USB_PASSTHROUGH_ARGS"));
    assert!(qemu_line.contains("$DISK_PASSTHROUGH_ARGS"));

    // Both sections round-trip independently
    assert_eq!(load_usb_passthrough(&vm).len(), 1);
    let disks = load_disk_passthrough(&vm);
    assert_eq!(disks.len(), 1);
    assert_eq!(disks[0].bootindex, Some(2));
}

// ---------------------------------------------------------------------------
// OVMF_VARS writability repair (NixOS read-only firmware templates)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn write_with_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, b"vars").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
#[cfg(unix)]
fn test_ensure_ovmf_vars_writable_adds_owner_write_bit() {
    let tmp = tempfile::tempdir().unwrap();
    let vars = tmp.path().join("OVMF_VARS.fd");
    // Exactly what `cp` from /nix/store produces.
    write_with_mode(&vars, 0o444);

    ensure_ovmf_vars_writable(tmp.path()).unwrap();

    assert_eq!(mode_of(&vars), 0o644);
}

#[test]
#[cfg(unix)]
fn test_ensure_ovmf_vars_writable_handles_qcow2_variant() {
    let tmp = tempfile::tempdir().unwrap();
    let vars = tmp.path().join("OVMF_VARS.qcow2");
    write_with_mode(&vars, 0o444);

    ensure_ovmf_vars_writable(tmp.path()).unwrap();

    assert_eq!(mode_of(&vars), 0o644);
}

#[test]
#[cfg(unix)]
fn test_ensure_ovmf_vars_writable_leaves_writable_file_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let vars = tmp.path().join("OVMF_VARS.fd");
    write_with_mode(&vars, 0o600);

    ensure_ovmf_vars_writable(tmp.path()).unwrap();

    // Only the owner-write bit is ever added; other bits are never touched.
    assert_eq!(mode_of(&vars), 0o600);
}

#[test]
fn test_ensure_ovmf_vars_writable_is_noop_without_vars_file() {
    // BIOS VMs and first launches have no OVMF_VARS yet: not an error.
    let tmp = tempfile::tempdir().unwrap();
    ensure_ovmf_vars_writable(tmp.path()).unwrap();
    assert!(ensure_ovmf_vars_writable(Path::new("/nonexistent/vm-curator-vm")).is_ok());
}

// ---------------------------------------------------------------------------
// Per-VM launch log (qemu.log)
// ---------------------------------------------------------------------------

fn write_stub_launch_script(dir: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("launch.sh");
    std::fs::write(&path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn launch_options(boot_mode: BootMode) -> LaunchOptions {
    LaunchOptions {
        boot_mode,
        extra_args: Vec::new(),
        usb_devices: Vec::new(),
        window_size: None,
    }
}

/// Poll the log until it contains `needle` (the stderr tee is asynchronous).
fn wait_for_log(path: &Path, needle: &str) -> String {
    for _ in 0..60 {
        if let Ok(content) = std::fs::read_to_string(path) {
            if content.contains(needle) {
                return content;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "{needle:?} never appeared in {}:\n{}",
        path.display(),
        std::fs::read_to_string(path).unwrap_or_default()
    );
}

#[test]
fn launch_writes_per_vm_log_with_header_and_both_streams() {
    let dir = tempfile::tempdir().unwrap();
    // Keep running past the 800ms startup check so this counts as a success.
    write_stub_launch_script(
        dir.path(),
        "echo 'Booting stub...'\necho 'qemu-system-x86_64: warning: virgl fake warning' >&2\nsleep 2",
    );
    let vm = test_vm(dir.path());

    let result = launch_vm_with_error_check(&vm, &launch_options(BootMode::Install));
    assert!(result.success, "{:?}", result.error);

    let log = qemu_log_path(dir.path());
    let content = wait_for_log(&log, "virgl fake warning");
    assert!(content.starts_with("# vm-curator launch "), "{content}");
    assert!(
        content.contains(&format!(
            "# command: bash {} --install",
            dir.path().join("launch.sh").display()
        )),
        "{content}"
    );
    assert!(
        content.contains("Booting stub..."),
        "stdout missing: {content}"
    );
    assert!(
        content.contains("qemu-system-x86_64: warning: virgl fake warning"),
        "stderr missing: {content}"
    );
    // A VM that ran fine still has its warnings on disk — the whole point.
    assert!(!content.contains("Full log:"));
}

#[test]
fn launch_failure_reports_error_and_points_at_log() {
    let dir = tempfile::tempdir().unwrap();
    write_stub_launch_script(
        dir.path(),
        "echo \"qemu-system-x86_64: Could not open '/x/OVMF_VARS.fd': Permission denied\" >&2\nexit 1",
    );
    let vm = test_vm(dir.path());

    let result = launch_vm_with_error_check(&vm, &launch_options(BootMode::Normal));
    assert!(!result.success);
    let error = result.error.unwrap();
    assert!(error.contains("Permission denied"), "{error}");
    assert!(
        error.contains(&format!(
            "Full log: {}",
            qemu_log_path(dir.path()).display()
        )),
        "{error}"
    );

    let content = wait_for_log(&qemu_log_path(dir.path()), "Permission denied");
    assert!(content.contains("Permission denied"));
}

#[test]
fn launch_log_keeps_capturing_after_launcher_returns() {
    // `vm-curator launch` (CLI) returns within a second while QEMU runs on.
    // Output written after that must still land in the log — this is the
    // difference between redirecting the child's fds and piping through us.
    let dir = tempfile::tempdir().unwrap();
    write_stub_launch_script(
        dir.path(),
        "echo EARLY >&2\nsleep 1.5\necho 'LATE: after launcher returned' >&2",
    );
    let vm = test_vm(dir.path());

    let result = launch_vm_with_error_check(&vm, &launch_options(BootMode::Normal));
    assert!(result.success, "{:?}", result.error);
    let early = std::fs::read_to_string(qemu_log_path(dir.path())).unwrap();
    assert!(
        early.contains("EARLY") && !early.contains("LATE"),
        "{early}"
    );

    let content = wait_for_log(&qemu_log_path(dir.path()), "LATE: after launcher returned");
    assert!(content.contains("EARLY"));
}

#[test]
fn launch_rotates_previous_log() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());

    write_stub_launch_script(dir.path(), "echo FIRST-RUN >&2\nexit 1");
    launch_vm_with_error_check(&vm, &launch_options(BootMode::Normal));
    wait_for_log(&qemu_log_path(dir.path()), "FIRST-RUN");

    write_stub_launch_script(dir.path(), "echo SECOND-RUN >&2\nexit 1");
    launch_vm_with_error_check(&vm, &launch_options(BootMode::Normal));
    let current = wait_for_log(&qemu_log_path(dir.path()), "SECOND-RUN");
    let previous = std::fs::read_to_string(dir.path().join(QEMU_LOG_PREVIOUS)).unwrap();

    assert!(current.contains("SECOND-RUN") && !current.contains("FIRST-RUN"));
    assert!(previous.contains("FIRST-RUN") && !previous.contains("SECOND-RUN"));
}

// --- SMB-mode shared folders (#101) ---------------------------------------

#[test]
fn test_generate_shared_folders_section_smb_uses_set_on_user_netdev() {
    let folders = vec![SharedFolder::smb("/home/user/Public")];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", Some("net0"));
    assert!(section.contains("    -set\n    'netdev.net0.smb=/home/user/Public'\n"));
    assert!(!section.contains("-fsdev"));
    assert!(!section.contains("-device"));
}

#[test]
fn test_generate_shared_folders_section_smb_path_with_spaces_and_commas() {
    // `-set` values are not comma-parsed by QEMU, so the path only needs
    // shell quoting — commas must survive verbatim.
    let folders = vec![SharedFolder::smb("/home/user/Docs, Pics & More")];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", Some("net1"));
    assert!(section.contains("'netdev.net1.smb=/home/user/Docs, Pics & More'"));
}

#[test]
fn test_generate_shared_folders_section_mixed_modes_keep_fsdev_indices() {
    let folders = vec![
        SharedFolder::nine_p("/srv/a", "host_a"),
        SharedFolder::smb("/srv/smb"),
        SharedFolder::nine_p("/srv/b", "host_b"),
    ];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", Some("net0"));
    assert!(section.contains("id=fsdev0,path=/srv/a"));
    assert!(section.contains("netdev.net0.smb=/srv/smb"));
    // Index follows the folder's position, not the count of 9p folders.
    assert!(section.contains("id=fsdev2,path=/srv/b"));
}

#[test]
fn test_roundtrip_shared_folders_with_smb() {
    let folders = vec![
        SharedFolder::nine_p("/home/user/Documents", "host_documents"),
        SharedFolder::smb("/home/user/My Share"),
    ];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", Some("net0"));
    let parsed = parse_shared_folders_section(&section);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].mode, ShareMode::NineP);
    assert_eq!(parsed[0].mount_tag, "host_documents");
    assert_eq!(parsed[1].mode, ShareMode::Smb);
    assert_eq!(parsed[1].host_path, "/home/user/My Share");
    // A derived tag so switching the folder back to 9p works without input.
    assert_eq!(parsed[1].mount_tag, "host_my_share");
}

#[test]
fn test_parse_shared_folders_section_ignores_malformed_set() {
    let content = format!(
        "{}\nSHARED_FOLDERS_ARGS=(\n    -set\n    netdev.net0.restrict=on\n)\n{}\n",
        SHARED_FOLDERS_MARKER_START, SHARED_FOLDERS_MARKER_END
    );
    assert!(parse_shared_folders_section(&content).is_empty());
}

#[test]
fn test_find_user_netdev_id_prefers_default_boot_branch() {
    let script = r#"#!/bin/bash
case "$1" in
    --install)
        qemu-system-x86_64 \
        -netdev user,id=inst0 \
        -device virtio-net-pci,netdev=inst0
        ;;
    "")
        qemu-system-x86_64 \
        -netdev user,id=net0,hostfwd=tcp::2222-:22 \
        -device virtio-net-pci,netdev=net0
        ;;
esac
"#;
    assert_eq!(find_user_netdev_id(script).as_deref(), Some("net0"));
}

#[test]
fn test_find_user_netdev_id_none_for_bridge_or_passt() {
    let bridge = "qemu-system-x86_64 \\\n    -netdev bridge,id=net0,br=vmc-lab \\\n    -device virtio-net-pci,netdev=net0\n";
    assert_eq!(find_user_netdev_id(bridge), None);
    let passt = "qemu-system-x86_64 \\\n    -netdev stream,id=net0,server=off,addr.type=unix,addr.path=/tmp/p.sock\n";
    assert_eq!(find_user_netdev_id(passt), None);
    let commented = "# -netdev user,id=net0\nqemu-system-x86_64 -m 1024\n";
    assert_eq!(find_user_netdev_id(commented), None);
}

#[test]
fn test_find_user_netdev_id_quoted_spec() {
    let script = "qemu-system-x86_64 -netdev \"user,id=mynet,hostfwd=tcp::8080-:80\" -device e1000,netdev=mynet\n";
    assert_eq!(find_user_netdev_id(script).as_deref(), Some("mynet"));
}

#[test]
fn test_save_shared_folders_smb_attaches_to_user_netdev() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 \\\n    -m 2048 \\\n    -netdev user,id=net0 \\\n    -device virtio-net-pci,netdev=net0\n",
    )
    .unwrap();

    save_shared_folders(&vm, &[SharedFolder::smb("/home/user/Public")]).unwrap();

    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(script.contains("netdev.net0.smb=/home/user/Public"));
    // The managed block is expanded at the end of the command, after -netdev,
    // which is what lets `-set` find the netdev.
    let netdev_pos = script.find("-netdev user,id=net0").unwrap();
    let ref_pos = script.find(SHARED_FOLDERS_ARGS_REF).unwrap();
    assert!(ref_pos > netdev_pos);
    assert_eq!(
        load_shared_folders(&vm),
        vec![SharedFolder::smb("/home/user/Public")]
    );
}

#[test]
fn test_save_shared_folders_smb_rejected_without_user_netdev() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 \\\n    -netdev bridge,id=net0,br=virbr0 \\\n    -device virtio-net-pci,netdev=net0\n",
    )
    .unwrap();

    let err = save_shared_folders(&vm, &[SharedFolder::smb("/srv/share")]).unwrap_err();
    assert!(err.to_string().contains("user-mode networking"), "{err}");
    // Nothing written.
    let script = std::fs::read_to_string(&vm.launch_script).unwrap();
    assert!(!script.contains(SHARED_FOLDERS_MARKER_START));
}

#[test]
fn test_save_shared_folders_rejects_two_smb_folders() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    std::fs::write(
        &vm.launch_script,
        "#!/bin/bash\nqemu-system-x86_64 -netdev user,id=net0 -device e1000,netdev=net0\n",
    )
    .unwrap();

    let err = save_shared_folders(
        &vm,
        &[SharedFolder::smb("/srv/a"), SharedFolder::smb("/srv/b")],
    )
    .unwrap_err();
    assert!(err.to_string().contains("only one folder"), "{err}");
}

#[test]
fn test_smb_shared_folder_survives_shell_expansion() {
    let folders = vec![SharedFolder::smb("/home/user/O'Brien, Docs")];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci", Some("net0"));
    let script = format!(
        "{}\nset -- qemu-system-x86_64 {}\nprintf '%s\\0' \"$@\"\n",
        section, SHARED_FOLDERS_ARGS_REF
    );
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("bash should run");
    assert!(output.status.success());
    let args: Vec<String> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect();
    assert_eq!(
        args,
        vec![
            "qemu-system-x86_64".to_string(),
            "-set".to_string(),
            "netdev.net0.smb=/home/user/O'Brien, Docs".to_string(),
        ]
    );
}

#[test]
fn test_generate_mount_tag() {
    assert_eq!(
        generate_mount_tag("/home/user/My Documents"),
        "host_my_documents"
    );
    assert_eq!(
        generate_mount_tag("/srv/Share-2024.v1"),
        "host_share_2024_v1"
    );
    assert_eq!(generate_mount_tag("/"), "host_shared");
}

// --- `-name "$VM_NAME"` self-heal for older scripts (#96) ---------------

const OLD_SCRIPT_NO_NAME: &str = r#"#!/bin/bash
# Old VM Launch Script
VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/disk.qcow2"

case "$1" in
    --install)
        qemu-system-x86_64 \
        -enable-kvm \
        -m 2048M \
        -cdrom "$ISO"
        ;;
    "")
        echo "Booting..."
        qemu-system-x86_64 \
        -enable-kvm \
        -m 2048M
        ;;
esac
"#;

#[test]
fn test_add_vm_name_patches_every_qemu_block() {
    let patched = add_vm_name_to_script(OLD_SCRIPT_NO_NAME).expect("should patch");
    // VM_NAME defined right after VM_DIR
    assert!(patched.contains(
        "VM_DIR=\"$(dirname \"$(readlink -f \"$0\")\")\"\nVM_NAME=\"$(basename \"$VM_DIR\")\"\n"
    ));
    // -name inserted as the first arg of both invocations, indentation kept
    assert_eq!(
        patched.matches("        -name \"$VM_NAME\" \\\n").count(),
        2
    );
    assert!(patched.contains(
        "        qemu-system-x86_64 \\\n        -name \"$VM_NAME\" \\\n        -enable-kvm \\\n"
    ));
    // Nothing else changed
    assert!(patched.contains("        -cdrom \"$ISO\"\n        ;;"));
}

#[test]
fn test_add_vm_name_is_idempotent_and_respects_existing_name() {
    let patched = add_vm_name_to_script(OLD_SCRIPT_NO_NAME).unwrap();
    assert!(
        add_vm_name_to_script(&patched).is_none(),
        "second pass is a no-op"
    );

    let user_named =
        OLD_SCRIPT_NO_NAME.replace("-enable-kvm \\\n", "-name \"My Box\" \\\n-enable-kvm \\\n");
    assert!(
        add_vm_name_to_script(&user_named).is_none(),
        "user's own -name is kept"
    );
}

#[test]
fn test_add_vm_name_skips_scripts_it_cannot_anchor() {
    // No VM_DIR line to hang VM_NAME on: leave it alone rather than emit an
    // unbound variable.
    let hand_written = "#!/bin/bash\nqemu-system-x86_64 \\\n    -m 1024M\n";
    assert!(add_vm_name_to_script(hand_written).is_none());
    // Single-line invocation (no continuation): nothing to splice into.
    let one_liner = "#!/bin/bash\nVM_DIR=\"$(dirname \"$0\")\"\nqemu-system-x86_64 -m 1024M\n";
    assert!(add_vm_name_to_script(one_liner).is_none());
    // A commented-out -name doesn't count as present.
    let commented = OLD_SCRIPT_NO_NAME.replace("# Old VM Launch Script", "# -name \"x\" was here");
    assert!(add_vm_name_to_script(&commented).is_some());
}

#[test]
fn test_add_vm_name_result_is_valid_bash_and_expands_to_dir_name() {
    let dir = tempfile::tempdir().unwrap();
    let vm_dir = dir.path().join("tiny11pro");
    std::fs::create_dir(&vm_dir).unwrap();
    let script_path = vm_dir.join("launch.sh");
    // Patch first, then swap the qemu invocations for a printf so we can
    // observe the args the script would pass.
    let patched = add_vm_name_to_script(OLD_SCRIPT_NO_NAME).unwrap();
    let observable = patched.replace("qemu-system-x86_64 \\", "printf '%s\\n' \\");
    std::fs::write(&script_path, observable).unwrap();

    let syntax = std::process::Command::new("bash")
        .args(["-n", script_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        syntax.status.success(),
        "{}",
        String::from_utf8_lossy(&syntax.stderr)
    );

    let run = std::process::Command::new("bash")
        .arg(&script_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("-name\ntiny11pro\n"), "stdout: {stdout}");
}

#[test]
fn test_ensure_name_in_script_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("launch.sh"), OLD_SCRIPT_NO_NAME).unwrap();
    ensure_name_in_script(dir.path()).unwrap();
    let after = std::fs::read_to_string(dir.path().join("launch.sh")).unwrap();
    assert!(after.contains("-name \"$VM_NAME\""));
    ensure_name_in_script(dir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("launch.sh")).unwrap(),
        after
    );
}
