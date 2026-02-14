# Changelog

**v0.4.1**
- Fix Cargo.lock version mismatch that prevented AUR package from building (`cargo fetch --locked` failed due to stale lockfile in v0.4.0 release tarball)

**v0.4.0**
- **VM Import Wizard**: Import existing virtual machines from libvirt (virsh) XML configurations and Quickemu .conf files
  - 5-step guided import: select source, choose VM, review compatibility warnings, configure disk handling, review and import
  - Automatic OS profile detection from imported configurations
  - Disk handling options: symlink, copy, or move existing disk images
  - Compatibility warnings for unsupported features (macvtap, virtio-net bridges, SPICE displays)
- **VM Notes**: Add free-form personal notes to any VM from the management menu
  - Multi-line text editor with full keyboard navigation
  - Notes stored in per-VM `vm-curator.toml` and displayed in the main info panel below Fun Facts
  - Notes preserved across VM renames

**v0.3.4**
- Fix "unsupported bus type 'sata'" error when launching Windows and macOS VMs with default profiles

**v0.3.3**
- Increase xHCI USB controller ports from 4 to 8 for USB passthrough (supports up to 8 USB 2.0 + 8 USB 3.0 devices)

**v0.3.2**
- Fix Secure Boot OVMF firmware selection for Windows 11 VMs

**v0.3.1**
- Fix GitHub Actions security issues found by zizmor

**v0.3.0**
- **Shared Folders**: Share host directories with VMs using virtio-9p, with add/remove/edit from the management menu
- **Headless VM Support**: Run VMs without a graphical display (display=none), with process monitoring and status indicators
- **VM Process Monitoring**: Detect running QEMU processes and show live status in the VM list
- **Stop/Force-Stop VM**: Gracefully shut down (ACPI poweroff) or force-stop running VMs from the management menu
- **Network Settings Screen**: New management menu screen to configure network backend (user/passt/bridge/none), adapter model, and port forwarding on existing VMs
- **Bridge Networking UI**: Bridge name selection with cycling through detected system bridges, status checklist (helper binary, permissions, available bridges), and setup guidance
- **Port Forwarding**: Add/remove port forwarding rules with presets (SSH, RDP, HTTP, HTTPS, VNC) for user and passt backends
- **Network Backend Support**: Full support for user/SLIRP, passt, bridge, and none backends in both the create wizard and existing VM management
- **Dynamic Display Detection**: Auto-detect available display backends per emulator (GTK, SDL, SPICE, VNC), replacing hardcoded list
- **SPICE App Support**: Replace legacy SPICE with spice-app display backend (requires virt-viewer)

**v0.2.7**
- **Direct Text Editing in Create VM Wizard**: Memory, CPU cores, and disk size fields now support direct keyboard input
  - Press Tab to enter edit mode, type values directly, press Enter to apply
  - Supports size suffixes: "8GB", "8192MB", "512000MB" automatically convert to the appropriate unit
  - Arrow keys still work for quick ±256MB (memory), ±1 (CPU), ±8GB (disk) adjustments
- Raised resource limits: RAM from 64GB to 1TB, CPU cores from 64 to 256
- Fix Bazzite categorization as Red Hat-based OS
- Fix navigation bug in Create VM wizard when moving between steps
- Refactor multi-GPU passthrough naming for consistency

**v0.2.6**
- Fix PCI Passthrough screen to pre-select previously saved devices

**v0.2.5**
- Fix single-GPU passthrough scripts to bind extra PCI devices (NICs, USB controllers, NVMe) to vfio-pci

**v0.2.4**
- Remove CD-ROM/ISO from single-GPU passthrough scripts (installation should use standard launch.sh)
- Fix sync issues between PCI/USB device selection and single-GPU passthrough script regeneration

**v0.2.3**
- Add USB 3.0 controller support for USB passthrough (xHCI controller for SuperSpeed devices)

**v0.2.2**
- Add existing disk selection to VM creation wizard (copy or move existing qcow2 files)
- Fix Settings screen overlapping pane artifacts

**v0.2.1**
- Fix UI rendering artifacts after closing Settings screen and during search filtering

**v0.2.0**
- **GPU Passthrough Support**: Full VFIO-based GPU passthrough for gaming VMs
  - Single-GPU passthrough: Pass your only GPU to a VM (requires TTY, stops display manager)
  - Multi-GPU passthrough: Pass a secondary GPU while keeping primary for host
  - Looking Glass integration for multi-GPU setups with near-zero latency display
- **PCI Passthrough Screen**: Select PCI devices (GPUs, USB controllers, NVMe) for VM passthrough
- **System Setup Wizard**: One-click VFIO/IOMMU configuration with initramfs regeneration
- **Settings Help System**: Contextual help tooltips for all settings
- **USB Device Classification**: Improved keyboard/mouse detection for passthrough validation

**v0.1.5**
- **BTRFS Performance Fix**: Automatically disables copy-on-write on BTRFS filesystems when creating VM directories, preventing performance degradation from double CoW (BTRFS + qcow2)

**v0.1.4**
- **First-Time Setup**: New users are now prompted to configure the VM library directory on first run

**v0.1.2**
- **Binary Packages**: Pre-built packages now available for Linux (DEB, RPM, AppImage, tarball)
- **crates.io**: Install via `cargo install vm-curator`
- **AUR**: Available for Arch & Arch-derived Linux users (incl. CachyOS, EndeavourOS, Garuda, and Omarchy)

**v0.1.1**
- **Custom VM Names**: VMs can now have custom display names that persist across sessions
- **Rename VMs**: New management menu option to rename VMs on the fly
- **Change Display**: New management menu option to switch display types (GTK, SDL, SPICE, VNC)
- **SDL Default for 3D**: VMs with 3D acceleration now default to SDL display for better performance
- **Duplicate VM Support**: Creating multiple VMs of the same OS now auto-increments folder names (-2, -3, etc.)
- **Improved Trash Handling**: Fixed conflicts when deleting VMs with duplicate names
- **UI Polish**: Management screen now displays all options without scrolling
