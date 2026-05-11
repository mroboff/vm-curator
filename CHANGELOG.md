# Changelog

**v0.4.10**
- **First release with external contributions** — many thanks to [@Ibn-Hesham](https://github.com/Ibn-Hesham) and [@nextzard](https://github.com/nextzard) for the patches below!
- **Nix Flake** (thanks @Ibn-Hesham, #32): Reproducible builds and dev shell. Adds `flake.nix` with `packages.default`, `devShells.default`, and `apps.default` outputs, plus a `flake.lock`. README updated with Nix/NixOS installation instructions. Build artifacts excluded via `.gitignore`.
- **Fix Snapshots on UEFI VMs** (thanks @nextzard, #33 / #37): `primary_disk()` returned the first parsed `-drive` line, which on UEFI VMs is the read-only `OVMF_CODE.fd` pflash entry — `qemu-img snapshot` then failed with `Permission denied` against the firmware blob
  - `primary_disk()` now picks the first disk whose format supports snapshots (qcow2), falling back to `disks.first()` for legacy/non-qcow2 cases
  - Fix applies to all snapshot entry points (CLI, TUI, lifecycle)
- **Fix Network-Settings Rewrite** (#36, #38): Editing model/backend/MAC on an existing VM via the Network Settings screen had two compounding bugs in `update_network_in_script`:
  - Replacement args were inserted only at the first match (`--install` branch), leaving `--cdrom`, `--recovery`, `--floppy`, and normal-boot branches with no networking at all
  - The line-strip loop consumed every backslash-continued line from `-netdev` through the next non-`\` line, sweeping up adjacent non-network args (`-usb`, `-device usb-tablet`, `-rtc base=localtime`)
  - Rewrite now consumes only contiguous network-arg lines and inserts the replacement in every branch; regression tests cover the bug, the `model = "none"` strip path, and the originally-no-network fallback
- **Fix Wizard Hidden-Row Navigation** (#31): In the create wizard's "Configure QEMU" step, network sub-rows are conditionally rendered based on Network / Net Backend settings, but keyboard arrows still walked the full static field range — letting users focus invisible rows and open hidden editors (Net Backend / Bridge / Forwards / MAC)
  - Visibility rules consolidated on `QemuField::is_visible`; Up/Down navigation, Tab/Enter/`g`/`c` action handlers, Left/Right cycling, and the `r` profile-reset path all route through it
  - 10 unit tests pin the visibility truth table, skip navigation, bound conditions, and focus snap behavior

**v0.4.9**
- **Fix Port-Forward Editor Rendering**: Pressing Enter on the create wizard's "Forwards:" field activated the editor handler, but no popup was drawn — input went to an invisible target. Adds an overlay over step 4 with a rules list (plus presets) and an add-rule prompt, mirroring the existing network settings editor.
- **Fix Display Backend Parser**: `qemu-system-* -display help` output ends with a usage paragraph after the backend list, which slipped past the old filter and contributed bogus "backends" like "Some", "-display", and "For" to the wizard's display option cycler
  - Parse only the block between the "Available …:" header and the first blank line
  - Validate each token looks like a backend name (lowercase letters, digits, hyphens)
  - Unit tests added against real QEMU 10.x output

**v0.4.8**
- **MAC Address Editing**: Set an explicit MAC address on a VM's NIC, or generate a random one using QEMU's `52:54:00` OUI prefix
  - New `vm::mac` module for generation and validation
  - Editable in the create wizard and existing-VM network settings
  - Parsed from `launch.sh` on import so existing VMs round-trip correctly
- **Default ISO Path Setting**: ISO file browser now seeds to a configurable directory instead of always starting from `$HOME`
  - Settable from the Settings screen, or via `[d]` from inside the file browser to make the current directory the default
- **3D Acceleration Toggle**: New management menu item "3D Acceleration (non-pass-through)" on existing VMs toggles para-virtualized 3D (`virtio-vga-gl` + `gl=on`)
  - Automatically swaps `gtk` → `sdl` display when enabling, since SDL gives better performance for `gl=on`
  - Distinct from the GPU passthrough options to avoid confusion

**v0.4.7**
- **Windows Server Profiles**: Add 9 Windows Server OS profiles spanning two decades of Microsoft's server platform
  - Versions: 2003, 2008, 2008 R2, 2012, 2012 R2, 2016, 2019, 2022, 2025
  - QEMU configurations mirror each version's desktop kernel counterpart (XP through Windows 11) with server-appropriate resources
  - New "Windows Server" subcategory under the Microsoft family in the hierarchy
  - Full metadata with descriptions, release dates, and fun facts for each version
  - ASCII art automatically uses the Windows fallback logo

**v0.4.6**
- **Fix Multi-GPU Passthrough VFIO Binding**: Launch scripts now automatically bind PCI devices to the `vfio-pci` driver before starting QEMU, and restore original drivers on VM exit
  - Fixes `Could not open '/dev/vfio/N': No such file or directory` error when launching VMs with GPU passthrough
  - Uses `pkexec` (polkit) for graphical authentication, with `sudo` fallback — only prompts when devices need rebinding
  - Skips binding entirely if devices are already on `vfio-pci` (e.g., persistent kernel parameter setup)
  - Original drivers are restored on VM exit via cached sudo credentials or pkexec
  - Prerequisites dialog updated with VFIO binding info and permission requirements

**v0.4.5**
- **Fix Multi-GPU Passthrough State**: Multi-GPU Passthrough screen now correctly shows previously selected GPUs instead of always displaying "No GPU Selected"
  - Saved PCI device selections from launch.sh are restored when entering the Multi-GPU Passthrough screen
  - Pressing 'p' from Multi-GPU to enter PCI Passthrough now loads saved selections
  - Extracted reusable `restore_pci_selections()` method to eliminate duplicated selection restoration logic

**v0.4.4**
- Fix Cargo.lock mismatch for source builds

**v0.4.3**
- **Floppy Disk Support**: Boot floppy image support for older operating systems that require a boot floppy for installation (e.g., OS/2)
  - New "Browse for boot floppy image" option in the create wizard's install media step (Step 2)
  - Floppy file browser filters for common floppy formats (.img, .ima, .flp, .vfd)
  - Generated launch scripts include `FLOPPY=` variable, `-fda` QEMU argument, and `--floppy` CLI option
  - When a floppy is paired with an ISO, boot priority automatically changes to floppy-first (`-boot a`) so the floppy bootloader can access the CD-ROM
  - New "Boot with floppy image" option in the management screen's boot options menu
  - Floppy path displayed in the create wizard's review/confirm step

**v0.4.2**
- **macOS Intel VM Support**: Comprehensive overhaul of macOS Intel profiles (Leopard through Tahoe) for reliable out-of-the-box virtualization
  - Apple SMC device emulation with correct OSK for macOS guest detection
  - AHCI (ich9-ahci) disk interface replacing plain IDE for proper macOS disk support
  - OpenCore bootloader integration with bios_rom configuration (optional for Catalina, required for Big Sur+)
  - Version-specific CPU models: Penryn with extended features (invtsc, vmware-cpuid-freq, AVX, AES, etc.) for Leopard–Ventura; Skylake-Client for Sonoma+
  - passt user-mode networking with vmxnet3 adapter for reliable macOS-compatible networking
  - spice-app display with vmware-svga device (256MB VRAM) for high-resolution output via virt-viewer
  - USB keyboard device for macOS compatibility
- **QEMU Profile Audit**: Comprehensive review and update of 40+ QEMU configuration profiles against current best practices and OS compatibility research
  - Fix critical boot failures: Bazzite and Pop!_OS now correctly default to UEFI (mandatory for both), OpenWrt switched to Legacy BIOS (UEFI has known issues)
  - Fix VGA compatibility: FreeBSD/GhostBSD switched from virtio to std (virtio-gpu is WIP), NetBSD switched to vmware (built-in X11 driver), KolibriOS switched to vmware (wiki recommendation), Haiku switched to virtio (modesetting driver added in 2024), historic Linux distros switched to cirrus (XFree86 compatibility)
  - Fix network adapters: Windows 9x switched to pcnet (built-in drivers), BeOS switched to ne2k_pci, ReactOS switched to e1000 (documented recommendation), OS/2 switched to pcnet, Inferno switched to e1000
  - Fix resource allocations: Proxmox bumped to 8GB RAM / 4 cores (hypervisor minimum), Tails bumped to 4GB (v7.0+ minimum), Puppy Linux bumped to 1GB / 2 cores (64-bit version), Mac OS 9 bumped to 512MB with G4 CPU, System 7 bumped to 128MB
  - Fix RTC clock: Android-x86, LineageOS, and Bliss OS switched to UTC (Linux-based)
  - Improve Windows defaults: Windows 10 now defaults to UEFI, Vista upgraded to q35 machine
  - Update Plan 9 to use virtio and host CPU (9front support)
  - Disable BeOS audio (media_addon_server freeze workaround)
  - Add MorphOS networking (sungem) and video (std VGA)
  - Add detailed notes for 15+ profiles with compatibility tips, workarounds, and alternative emulator recommendations

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
