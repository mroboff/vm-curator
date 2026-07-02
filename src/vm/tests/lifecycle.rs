use super::*;

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
fn test_build_launch_invocation_ignores_transient_usb_devices() {
    let dir = tempfile::tempdir().unwrap();
    let vm = test_vm(dir.path());
    let options = LaunchOptions {
        usb_devices: vec![UsbPassthrough {
            vendor_id: 0x413c,
            product_id: 0x2113,
            usb_version: crate::hardware::UsbVersion::Usb2,
        }],
        ..LaunchOptions::default()
    };

    let invocation = build_launch_invocation(&vm, &options).unwrap();

    assert_eq!(
        invocation.args,
        vec![vm.launch_script.to_string_lossy().to_string()]
    );
}

#[test]
fn test_save_usb_passthrough_persists_in_launch_script_not_launch_args() {
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

    let invocation = build_launch_invocation(&vm, &LaunchOptions::default()).unwrap();
    assert_eq!(
        invocation.args,
        vec![vm.launch_script.to_string_lossy().to_string()]
    );
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
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
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
fn test_shared_folders_array_survives_shell_expansion() {
    let folders = vec![SharedFolder {
        host_path: "/home/user/O'Brien Documents".to_string(),
        mount_tag: "host_obrien_documents".to_string(),
    }];
    let section = generate_shared_folders_section(&folders, "virtio-9p-pci");
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
