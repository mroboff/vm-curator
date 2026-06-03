pub mod create;
pub mod discovery;
pub mod import;
pub mod launch_parser;
pub mod lifecycle;
pub mod mac;
pub mod qemu_config;
pub mod single_gpu_scripts;
pub mod snapshot;

pub use create::create_vm;
pub use discovery::{discover_vms, group_vms_by_category, DiscoveredVm};
pub use lifecycle::{detect_qemu_processes, force_stop_vm, launch_vm_sync, launch_vm_with_error_check, load_pci_passthrough, load_shared_folders, load_usb_passthrough, save_shared_folders, save_usb_passthrough, stop_vm_by_pid, LaunchOptions, QemuProcess, SharedFolder, UsbPassthrough};
pub use qemu_config::{BootMode, QemuConfig};
pub use single_gpu_scripts::generate_single_gpu_scripts;
pub use snapshot::{create_snapshot, delete_snapshot, list_snapshots, restore_snapshot, Snapshot};

// Library-only exports — used by external GUI consumers, not the binary.
#[allow(unused_imports)]
pub use create::update_network_in_script;
#[allow(unused_imports)]
pub use import::{discover_libvirt_vms, discover_quickemu_vms, discover_vms_in_dir, execute_import};
#[allow(unused_imports)]
pub use lifecycle::{delete_vm, ensure_qmp_in_script, is_vm_paused, launch_vm_dbus, pause_vm, rename_vm, reset_vm, resume_vm, save_notes, save_pci_passthrough};
#[allow(unused_imports)]
pub use qemu_config::{NetworkBackend, NetworkConfig, PortForward, PortProtocol};
#[allow(unused_imports)]
pub use single_gpu_scripts::{delete_scripts, GeneratedScripts};
