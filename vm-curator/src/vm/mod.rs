pub mod discovery;
pub mod launch_parser;
pub mod lifecycle;
pub mod qemu_config;
pub mod snapshot;

pub use discovery::{discover_vms, group_vms_by_category, DiscoveredVm};
pub use lifecycle::{launch_vm_sync, load_usb_passthrough, save_usb_passthrough, LaunchOptions, UsbPassthrough};
pub use qemu_config::{BootMode, QemuConfig, VgaType};
pub use snapshot::{create_snapshot, delete_snapshot, list_snapshots, restore_snapshot, validate_snapshot_name, Snapshot};
