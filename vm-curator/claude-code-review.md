# VM Curator - Unused Code Review

This report identifies all unused functions, methods, structs, variants, and fields in the vm-curator application. These represent planned features that have not yet been integrated into the UI, or utility code that was written in anticipation of future needs.

---

## Summary

| Category | Count |
|----------|-------|
| Unused Functions | 17 |
| Unused Methods | 12 |
| Unused Structs | 4 |
| Unused Enum Variants | 4 |
| Unused Fields | 12 |
| Unused Imports | 8 |

---

## Unused Functions

### `src/commands/qemu_img.rs`

| Function | Line | Purpose |
|----------|------|---------|
| `path_to_str` | 6 | Helper to convert a Path to a string with proper error handling for invalid UTF-8 |
| `create_disk` | 12 | Create a new qcow2 disk image using `qemu-img create`. Intended for creating new VM disks from scratch |
| `create_disk_with_backing` | 33 | Create a disk with a backing file (copy-on-write). Useful for creating disk overlays for testing or branching VM states |
| `convert_disk` | 56 | Convert a disk image between formats (e.g., raw to qcow2). Useful for optimizing disk storage |
| `resize_disk` | 79 | Resize a disk image. Intended for expanding VM storage capacity |
| `check_disk` | 99 | Check disk integrity using `qemu-img check`. Useful for detecting corruption in qcow2 images |
| `compact_disk` | 128 | Compact a qcow2 disk by removing unused space. Useful for reclaiming disk space after deleting files in the VM |
| `rebase_disk` | 162 | Rebase a disk to a new backing file. Advanced operation for changing the base image of a copy-on-write disk |
| `commit_disk` | 184 | Commit changes from an overlay to its backing file. Merges changes down to the base image |

### `src/commands/qemu_system.rs`

| Function | Line | Purpose |
|----------|------|---------|
| `build_qemu_args` | 8 | Build QEMU command-line arguments from a QemuConfig struct. Alternative to using launch.sh scripts |
| `launch_qemu_direct` | 108 | Launch QEMU directly without using a launch.sh script. Intended for VMs that don't have custom launch scripts |

### `src/hardware/usb.rs`

| Function | Line | Purpose |
|----------|------|---------|
| `enumerate_usb_devices` | 48 | Main entry point for USB device enumeration. Lists all USB devices for passthrough selection |
| `enumerate_via_udev` | 60 | Enumerate USB devices using libudev. Primary method for Linux USB discovery |
| `enumerate_via_sysfs` | 138 | Fallback USB enumeration via /sys/bus/usb/devices. Used when libudev is unavailable |
| `read_sysfs_hex` | 191 | Helper to read hexadecimal values from sysfs attributes |
| `read_sysfs_decimal` | 196 | Helper to read decimal values from sysfs attributes |
| `read_sysfs_string` | 201 | Helper to read string values from sysfs attributes |

### `src/hardware/passthrough.rs`

| Function | Line | Purpose |
|----------|------|---------|
| `generate_usb_passthrough_args` | 4 | Generate QEMU arguments for USB passthrough by vendor/product ID |
| `generate_usb_passthrough_by_bus` | 24 | Generate QEMU arguments for USB passthrough by bus/device number. More specific targeting |

---

## Unused Methods

### `src/app.rs` - App struct

| Method | Line | Purpose |
|--------|------|---------|
| `load_usb_devices` | 335 | Load and cache USB devices for passthrough selection in the UI |
| `show_error` | 376 | Display a detailed error in a scrollable dialog. For showing long error messages |
| `grouped_vms` | 440 | Get VMs grouped by category for alternative display modes |

### `src/config/mod.rs` - Config struct

| Method | Line | Purpose |
|--------|------|---------|
| `save` | 50 | Save the current configuration to the config file. For persisting user preferences |
| `ensure_directories` | 75 | Create required directories (metadata, ASCII art) if they don't exist |

### `src/hardware/usb.rs` - UsbDevice struct

| Method | Line | Purpose |
|--------|------|---------|
| `is_hub` | 17 | Check if a USB device is a hub (to filter out non-passthrough devices) |
| `to_qemu_args` | 36 | Generate QEMU passthrough arguments for this specific device |

### `src/hardware/passthrough.rs` - PassthroughConfig struct

| Method | Line | Purpose |
|--------|------|---------|
| `to_qemu_args` | 52 | Convert passthrough configuration to QEMU command-line arguments |

### `src/metadata/hierarchy.rs` - HierarchyConfig struct

| Method | Line | Purpose |
|--------|------|---------|
| `load_from_file` | 96 | Load hierarchy configuration from an external file (for user overrides) |
| `get_family` | 172 | Get a family definition by ID. Used for looking up family metadata |

### `src/vm/lifecycle.rs` - UsbPassthrough struct

| Method | Line | Purpose |
|--------|------|---------|
| `to_qemu_args` | 30 | Generate QEMU arguments for USB passthrough |

### `src/vm/qemu_config.rs` - AudioDevice enum

| Method | Line | Purpose |
|--------|------|---------|
| `from_str` | 93 | Parse an audio device type from a string |

### `src/ui/widgets/dialog.rs` - InputDialog struct

| Method | Line | Purpose |
|--------|------|---------|
| `render` | 81 | Render a text input dialog for user input |

### `src/ui/widgets/dialog.rs` - MenuDialog struct

| Method | Line | Purpose |
|--------|------|---------|
| `render` | 134 | Render a menu selection dialog |

---

## Unused Structs

| Struct | Location | Purpose |
|--------|----------|---------|
| `DiskCheckResult` | `src/commands/qemu_img.rs:121` | Return type for disk integrity check results |
| `PassthroughConfig` | `src/hardware/passthrough.rs:46` | Configuration for USB and PCI passthrough devices |
| `InputDialog` | `src/ui/widgets/dialog.rs:73` | Reusable text input dialog widget |
| `MenuDialog` | `src/ui/widgets/dialog.rs:127` | Reusable menu selection dialog widget |
| `DiskInfo` | `src/vm/snapshot.rs:251` | Information about a disk image (format, size, backing file) |

---

## Unused Enum Variants

### `src/app.rs` - Screen enum

| Variant | Line | Purpose |
|---------|------|---------|
| `DetailedInfo` | 26 | Screen for showing detailed OS history and blurbs (planned feature) |
| `UsbDevices` | 32 | Screen for selecting USB devices for passthrough |
| `ErrorDialog` | 44 | Screen for displaying scrollable error details |

### `src/app.rs` - BackgroundResult enum

| Variant | Line | Purpose |
|---------|------|---------|
| `SnapshotsLoaded` | 149 | Result type for asynchronous snapshot list loading |

---

## Unused Fields

### `src/hardware/usb.rs` - UsbDevice struct

| Field | Line | Purpose |
|-------|------|---------|
| `bus_num` | 10 | USB bus number for bus-specific passthrough |
| `dev_num` | 11 | USB device number for device-specific passthrough |
| `device_class` | 12 | USB device class for filtering (e.g., identifying hubs) |

### `src/vm/discovery.rs` - DiscoveredVm struct

| Field | Line | Purpose |
|-------|------|---------|
| `parse_success` | 19 | Whether the launch.sh script was successfully parsed |
| `parse_error` | 21 | Error message if parsing failed |

### `src/vm/lifecycle.rs` - LaunchOptions struct

| Field | Line | Purpose |
|-------|------|---------|
| `usb_devices` | 19 | List of USB devices to passthrough when launching |

### `src/vm/lifecycle.rs` - UsbPassthrough struct

| Field | Line | Purpose |
|-------|------|---------|
| `vendor_id` | 25 | USB vendor ID for passthrough targeting |
| `product_id` | 26 | USB product ID for passthrough targeting |

### `src/vm/snapshot.rs` - Snapshot struct

| Field | Line | Purpose |
|-------|------|---------|
| `id` | 9 | Internal snapshot ID from qemu-img |
| `vm_clock` | 13 | VM clock timestamp when snapshot was taken |

### `src/vm/snapshot.rs` - DiskInfo struct

| Field | Line | Purpose |
|-------|------|---------|
| `format` | 252 | Disk image format (qcow2, raw, etc.) |
| `virtual_size` | 253 | Virtual disk size |
| `disk_size` | 254 | Actual disk size on filesystem |
| `cluster_size` | 255 | qcow2 cluster size |

---

## Unused Imports

| Import | Location | Reason |
|--------|----------|--------|
| `DiscoveredVm` | `src/commands/qemu_system.rs:5` | Not used after build_qemu_args became unused |
| `passthrough::PassthroughConfig` | `src/hardware/mod.rs:4` | Re-export of unused struct |
| `Context` | `src/metadata/os_info.rs:1` | Error context helper not needed |
| `OsBlurb` | `src/metadata/mod.rs:7` | Re-export not used externally |
| `InputDialog, MenuDialog` | `src/ui/widgets/mod.rs:6` | Re-exports of unused widgets |
| `Context` | `src/vm/launch_parser.rs:1` | Error context helper not needed |
| `DateTime` | `src/vm/snapshot.rs:155` | Date parsing import not used |
| `validate_snapshot_name` | `src/vm/mod.rs:10` | Validation function not integrated |

---

## Recommendations

### High Priority (Should be integrated or removed)

1. **USB Passthrough** - The entire USB subsystem is implemented but not connected to the UI. The `UsbDevices` screen variant exists but is never used. This would allow users to pass USB devices to VMs.

2. **Error Dialog** - The `ErrorDialog` screen and `show_error` method are implemented but not used. Currently errors are shown in the status bar which truncates long messages.

3. **Disk Management Functions** - Functions like `compact_disk`, `check_disk`, and `resize_disk` could be valuable additions to the Management menu.

### Medium Priority (Nice to have)

4. **InputDialog/MenuDialog widgets** - Generic reusable dialog widgets that could simplify existing dialog code.

5. **DetailedInfo screen** - Would show extended OS history and fun facts in a dedicated view.

6. **Config persistence** - The `Config::save` method would allow users to persist their preferences.

### Low Priority (Future features)

7. **Direct QEMU launch** - `build_qemu_args` and `launch_qemu_direct` provide an alternative to launch.sh scripts, useful for VMs without custom scripts.

8. **Disk format conversion** - `convert_disk` could help users optimize storage by converting between formats.

---

*Report generated: 2025-01-22*
