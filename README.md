# vm-curator

A fast and friendly Rust TUI for managing desktop QEMU/KVM virtual machines — with 3D acceleration, GPU passthrough, VM import, and 120+ pre-configured OS profiles!

### Changelog

**v0.4.10**
- **First release with external contributions** — many thanks to [@Ibn-Hesham](https://github.com/Ibn-Hesham) and [@nextzard](https://github.com/nextzard) for the patches below!
- **Nix Flake** (thanks @Ibn-Hesham, #32): Reproducible builds and dev shell via `nix build` / `nix develop` — flake exposes `packages.default`, `devShells.default`, and `apps.default`
- **Fix Snapshots on UEFI VMs** (thanks @nextzard, #33 / #37): Snapshot operations now skip OVMF pflash entries when picking the primary disk; UEFI VMs can be snapshotted again instead of failing with `Permission denied` on `OVMF_CODE.fd`
- **Fix Network-Settings Rewrite** (#36, #38): Editing a VM's network settings (model/backend/MAC) now preserves the network device in every boot branch of `launch.sh` and stops sweeping up adjacent args like `-usb` and `-rtc base=localtime`
- **Fix Wizard Hidden-Row Navigation** (#31): VM creation wizard's QEMU step no longer lets keyboard arrows focus invisible network rows (e.g., when Network = `none` hides Backend/Bridge/Forwards/MAC)

**v0.4.9**
- **Fix Port-Forward Editor Rendering**: Create wizard's port-forward editor now actually draws a popup when activated (previously the handler was wired up but no UI was rendered)
- **Fix Display Backend Parser**: Strip QEMU's trailing usage paragraph from `-display help` output so bogus tokens like "Some", "-display", and "For" no longer appear as selectable display backends in the wizard

**v0.4.8**
- **MAC Address Editing**: Set an explicit NIC MAC address or generate a random one (uses QEMU's `52:54:00` OUI prefix) from the create wizard and existing-VM network settings — also parsed from `launch.sh` on import
- **Default ISO Path Setting**: ISO file browser seeds to a configurable directory instead of `$HOME`; set it from Settings or press `[d]` in the browser to use the current directory
- **3D Acceleration Toggle**: New management menu item to toggle para-virtualized 3D (`virtio-vga-gl` + `gl=on`) on existing VMs, with automatic `gtk` → `sdl` display swap for better performance

**v0.4.7**
- **Windows Server Profiles**: Add 9 Windows Server OS profiles (2003, 2008, 2008 R2, 2012, 2012 R2, 2016, 2019, 2022, 2025) with QEMU configurations, metadata, and a new "Windows Server" subcategory under the Microsoft family

**v0.4.6**
- **Fix Multi-GPU Passthrough VFIO Binding**: Launch scripts now automatically bind PCI devices to `vfio-pci` before QEMU and restore original drivers on exit. Fixes `Could not open '/dev/vfio/N'` errors. Uses `pkexec`/`sudo` for authentication — only prompts when devices need rebinding.

**v0.4.5**
- **Fix Multi-GPU Passthrough State**: Multi-GPU Passthrough screen now correctly shows previously selected GPUs. Pressing 'p' from Multi-GPU to enter PCI Passthrough also loads saved selections.

**v0.4.3**
- **Floppy Disk Support**: Boot floppy image support for OSes that require a boot floppy for installation (e.g., OS/2). Browse for floppy images (.img, .ima, .flp, .vfd) in the create wizard and boot from floppy in the management screen.

**v0.4.2**
- **macOS Intel VM Support**: Comprehensive overhaul of macOS Intel profiles with Apple SMC emulation, AHCI disk, OpenCore bootloader integration, version-specific CPU models (Penryn/Skylake-Client), passt networking with vmxnet3, and spice-app display with vmware-svga
- **QEMU Profile Audit**: Review and update of 40+ QEMU profiles against current OS compatibility research — fixes critical boot failures (Bazzite, Pop!_OS, OpenWrt), corrects VGA/network/audio defaults for BSD, Windows 9x, BeOS, Plan 9, and retro OSes, and bumps resource allocations for Proxmox, Tails, and Classic Mac profiles

[Full changelog](CHANGELOG.md)

### Features

**VM Discovery & Organization**
- Automatically scans your VM library for directories containing `launch.sh` scripts
- Hierarchical organization by 16 OS families with emoji icons and 49 subcategories
- Parses QEMU launch scripts to extract configuration (emulator, memory, CPU, VGA, audio, network, disks)
- Smart categorization with configurable hierarchy patterns
- Live process monitoring — shows running VMs with status indicators
- Search and filter VMs by name

**VM Creation Wizard**
- 5-step guided wizard for creating new VMs
- 120+ pre-configured OS profiles with optimal QEMU settings (Windows, macOS, Linux, BSD, Unix, retro, and more)
- Automatic UEFI firmware detection across Linux distributions (Arch, Debian, Fedora, NixOS, etc.)
- ISO file browser for selecting installation media
- Configurable disk size, memory, CPU cores, and QEMU options with direct text editing and size suffixes (e.g., "8GB")
- Use existing disk images (copy or move) instead of creating new ones
- Support for custom OS entries with user metadata

**VM Import Wizard**
- Import existing VMs from libvirt (virsh) XML configurations and Quickemu `.conf` files
- 5-step guided import: select source, choose VM, review compatibility warnings, configure disk handling, review and import
- Automatic OS profile detection from imported configurations
- Disk handling options: symlink, copy, or move existing disk images

**GPU Passthrough**
- **Single-GPU passthrough**: Pass your only GPU to a VM (requires TTY, stops display manager)
- **Multi-GPU passthrough**: Pass a secondary GPU while keeping the primary for the host
- **Looking Glass integration**: Near-zero latency display for multi-GPU setups with auto-launch support
- **PCI passthrough screen**: Select PCI devices (GPUs, USB controllers, NVMe) for VM passthrough
- **System setup wizard**: One-click VFIO/IOMMU configuration with initramfs regeneration

**3D Graphics Acceleration**
- Para-virtualized 3D acceleration with `virtio-vga-gl` and SDL `gl=on`
- Tested on NVIDIA RTX-4090 with driver 590.48.01+
- Automatic SDL display selection for 3D-enabled VMs

**Snapshot Management**
- Create, restore, and delete snapshots for qcow2 disk images
- Visual snapshot list with timestamps and sizes
- Background operations with progress feedback

**Network Configuration**
- Network backend selection: user/SLIRP (NAT), passt, bridge, or none
- Port forwarding with presets for common services (SSH, RDP, HTTP, HTTPS, VNC)
- Bridge networking with automatic bridge detection, status checklist, and setup guidance
- Configurable network adapter models per VM

**Shared Folders**
- Share host directories with VMs using virtio-9p
- Add, remove, and edit shared folders from the management menu
- Automatic mount tag generation

**USB Passthrough**
- USB device enumeration via libudev with sysfs fallback
- xHCI USB 3.0 controller with 8 ports (supports up to 8 USB 2.0 + 8 USB 3.0 devices)
- Persistent passthrough configuration
- Hub filtering and keyboard/mouse detection for passthrough validation

**VM Notes**
- Free-form personal notes for any VM from the management menu
- Multi-line text editor with full keyboard navigation
- Notes displayed in the main info panel and preserved across VM renames

**Launch Script Editor**
- Edit `launch.sh` scripts directly in the TUI
- Syntax-aware display with line numbers and horizontal scrolling
- Automatic QEMU configuration re-parsing after saves
- Automatic single-GPU passthrough script regeneration when applicable

**Additional Features**
- Vim-style navigation (j/k, arrows, mouse) with full clickable interface
- Multiple boot modes (normal, install, custom ISO)
- Dynamic display backend detection per emulator (GTK, SDL, SPICE-app, VNC)
- Headless VM support (display=none) with process monitoring
- Stop/force-stop VMs (ACPI poweroff or SIGKILL)
- VM rename with persistent custom display names
- OS metadata with historical blurbs, fun facts, and multi-step installation guides
- 42+ ASCII art logos for classic and modern operating systems
- BTRFS copy-on-write auto-disable for VM directories
- First-time setup wizard for configuring the VM library directory
- Configurable settings with persistence

### Screenshots

```
 VM Curator (QEMU VM Library in ~/vm-space)
┌─────────────────────────────────────────────────────────────────────┐
│ ┌─────────────────────────┐  ┌────────────────────────────────────┐ │
│ │ VMs (35)                │  │       _    _ _           _        │ │
│ │ ──────────────────────  │  │      | |  | (_)         | |       │ │
│ │ 🪟 Microsoft            │  │      | |/\| |_ _ __   __| | ___   │ │
│ │   ▼ DOS                 │  │      \  /\  / | '_ \ / _` |/ _ \  │ │
│ │     > MS-DOS 6.22   [*] │  │       \/  \/|_|_| |_|\__,_|\___/  │ │
│ │     > Windows 3.11      │  │                                   │ │
│ │   ▼ Windows 9x          │  │   Windows 95 OSR2.5               │ │
│ │     > Windows 95        │  │   Microsoft | August 1995 | i386  │ │
│ │     > Windows 98        │  │                                   │ │
│ │ 🐧 Linux                │  │   The OS that changed everything  │ │
│ │   ▼ Debian-based        │  │   with the Start Menu, taskbar,   │ │
│ │     > Debian 12         │  │   and 32-bit computing for all.   │ │
│ │     > Ubuntu 24.04      │  │                                   │ │
│ └─────────────────────────┘  └────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────────┤
│ [Enter] Launch  [m] Manage  [c] Create  [s] Settings  [?] Help     │
└─────────────────────────────────────────────────────────────────────┘
```

### Installation

**AUR (Arch / Arch-derived)**

```bash
# Using your preferred AUR helper
paru -S vm-curator
yay -S vm-curator
```

**crates.io**

```bash
cargo install vm-curator
```

**Binary Packages**

Pre-built packages (DEB, RPM, AppImage, tarball) are available from [GitHub Releases](https://github.com/mroboff/vm-curator/releases).

**Nix / NixOS**

```bash
# Run directly without installing
nix run github:mroboff/vm-curator

# Build the package
nix build .#default
```

For NixOS, add to `/etc/nixos/configuration.nix`:
```nix
{ pkgs, ... }:
{
  environment.systemPackages = [ pkgs.vm-curator ];
}
```

**From Source**

```bash
git clone https://github.com/mroboff/vm-curator.git
cd vm-curator
cargo build --release
```

The binary will be at `target/release/vm-curator`.

**Prerequisites**
- **Required**: QEMU (`qemu-system-*` binaries), qemu-img (for disk creation and snapshots), libudev
- **Build**: Rust 1.70+, libudev-dev (Debian/Ubuntu) or systemd-libs (Arch/Fedora)
- **Optional**:
  - OVMF/edk2 — UEFI boot support (`edk2-ovmf` on Arch, `ovmf` on Debian/Ubuntu)
  - virt-viewer — SPICE-app display backend
  - passt — passt network backend
  - Looking Glass client — multi-GPU passthrough display
  - polkit — bridge networking permissions

### Usage

#### TUI Mode (default)

```bash
vm-curator
```

#### CLI Commands

```bash
# List all VMs
vm-curator list

# Launch a VM
vm-curator launch windows-95
vm-curator launch windows-95 --install    # Boot in install mode
vm-curator launch windows-95 --cdrom /path/to/image.iso

# View VM configuration
vm-curator info windows-95

# Import a VM
vm-curator  # then press 'i' for the import wizard

# Manage snapshots
vm-curator snapshot windows-95 list
vm-curator snapshot windows-95 create my-snapshot
vm-curator snapshot windows-95 restore my-snapshot
vm-curator snapshot windows-95 delete my-snapshot

# List available QEMU emulators
vm-curator emulators
```

### Key Bindings

#### Main Menu

| Key | Action |
|-----|--------|
| `j/k` or `Down/Up` | Navigate VM list |
| `Enter` | Launch selected VM |
| `m` | Open management menu |
| `x` | Stop VM (if running) |
| `c` | Open VM creation wizard |
| `i` | Open VM import wizard |
| `s` | Open settings |
| `/` | Search/filter VMs |
| `?` | Show help |
| `PgUp/PgDn` | Scroll info panel |
| `Esc` | Back / Cancel |
| `q` | Quit |

#### VM Management

| Key | Action |
|-----|--------|
| `j/k` or `Down/Up` | Navigate menu |
| `Enter` | Select menu option |
| `e` | Edit launch script |
| `u` | Configure USB passthrough |

Management menu options:
- Boot Options (normal, install, custom ISO)
- Snapshots
- USB Passthrough
- PCI Passthrough
- Shared Folders
- Network Settings
- Multi-GPU Passthrough (if enabled)
- Single GPU Passthrough (if enabled)
- Change Display
- Edit Notes
- Rename VM
- Stop VM / Force Stop
- Reset VM (recreate disk)
- Delete VM
- Edit Raw Configuration

#### Create Wizard

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next/previous field |
| `Enter` | Select / Continue |
| `n` | Next step |
| `p` | Previous step |
| `Esc` | Cancel wizard |

### Configuration

Settings are stored in `~/.config/vm-curator/config.toml` and can be edited via the Settings screen (`s` key).

```toml
# VM library location
vm_library_path = "~/vm-space"

# Default values for new VMs
default_memory_mb = 4096
default_cpu_cores = 2
default_disk_size_gb = 64
default_display = "gtk"      # gtk, sdl, spice-app, vnc
default_enable_kvm = true

# Behavior
confirm_before_launch = true

# Multi-GPU passthrough (Looking Glass)
enable_multi_gpu_passthrough = false
default_ivshmem_size_mb = 64
show_gpu_warnings = true
looking_glass_client_path = ""       # Path to Looking Glass client
looking_glass_auto_launch = true     # Auto-launch client when VM starts

# Single GPU passthrough
single_gpu_enabled = false
single_gpu_auto_tty = false          # Experimental: auto switch TTY
single_gpu_dm_override = ""          # Override display manager detection
```

### VM Library Structure

VMs are expected in your library directory (default `~/vm-space/`) with this structure:

```
~/vm-space/
├── windows-95/
│   ├── launch.sh      # QEMU launch script (required)
│   └── disk.qcow2     # Disk image (qcow2 recommended for snapshots)
├── linux-debian/
│   ├── launch.sh
│   ├── disk.qcow2
│   └── install.iso    # Optional: installation media
└── macos-tiger/
    ├── launch.sh
    └── disk.qcow2
```

The `launch.sh` script should invoke QEMU. VM Curator parses this script to extract configuration and can generate new scripts via the creation wizard.

### OS Profiles

The creation wizard includes 120+ pre-configured profiles organized into 16 OS families:

**Microsoft**: DOS, Windows 1.x–3.x, Windows 95/98/ME, Windows NT/2000/XP/Vista, Windows 7/8/10/11, Server editions

**Apple**: Classic Mac OS (System 6–9), Mac OS X PowerPC (Cheetah–Tiger), Mac OS X Intel (Leopard–El Capitan), macOS (Sierra–Tahoe)

**Linux**: Arch, Manjaro, EndeavourOS, Garuda, CachyOS, Debian, Ubuntu, Mint, Pop!_OS, Fedora, RHEL, Rocky, Alma, Bazzite, openSUSE, Slackware, Gentoo, Void, NixOS, Alpine, and more

**BSD**: FreeBSD, GhostBSD, OpenBSD, NetBSD, DragonFly BSD

**Unix**: Solaris, OpenIndiana, illumos, HP-UX, IRIX, MINIX, QNX

**IBM**: OS/2, eComStation, ArcaOS, AIX

**Commodore**: AmigaOS, AROS, MorphOS

**Be / Haiku**: BeOS, Haiku

**NeXT**: NeXTSTEP, OpenStep

**Research**: Plan 9, 9front, Inferno

**Alternative**: SerenityOS, Redox, TempleOS, KolibriOS, MenuetOS, ReactOS

**Retro**: Atari TOS, CP/M, FreeDOS, DR-DOS, GEOS, RISC OS

**Mobile**: Android-x86, LineageOS, Bliss OS

**Infrastructure**: pfSense, OPNsense, OpenWrt, TrueNAS, Proxmox, ESXi

**Utilities**: GParted, Clonezilla, Memtest86+

**Other**: Catch-all for uncategorized VMs

Each profile includes optimal QEMU settings for that OS (emulator, machine type, CPU model, VGA, audio, network, disk interface, and more).

### Metadata Customization

**OS Information**: Override or add OS metadata in `~/.config/vm-curator/metadata/`:

```toml
# ~/.config/vm-curator/metadata/my-os.toml
[my-custom-os]
name = "My Custom OS"
publisher = "My Company"
release_date = "2024-01-01"
architecture = "x86_64"

[my-custom-os.blurb]
short = "A brief description"
long = "A longer description with history and details."

[my-custom-os.fun_facts]
facts = ["Fact 1", "Fact 2"]
```

**ASCII Art**: Add custom ASCII art in `~/.config/vm-curator/ascii/`.

**QEMU Profiles**: Override profiles in `~/.config/vm-curator/qemu_profiles.toml`.

### Dependencies

- **Runtime**: QEMU, qemu-img, libudev
- **Build**: Rust 1.70+, libudev-dev (Debian/Ubuntu) or systemd-libs (Arch)
- **Optional**: OVMF/edk2 (UEFI), virt-viewer (SPICE-app), passt (networking), Looking Glass client (multi-GPU), polkit (bridge networking)

### Cross-Distribution Compatibility

VM Curator automatically detects OVMF/UEFI firmware paths across Linux distributions:
- Arch Linux: `/usr/share/edk2/x64/OVMF_CODE.4m.fd`
- Debian/Ubuntu: `/usr/share/OVMF/OVMF_CODE.fd`
- Fedora/RHEL: `/usr/share/edk2/ovmf/OVMF_CODE.fd`
- NixOS: Multiple search paths supported
- And more...

---

### Contributing

Contributions are welcome! If you find a bug or have an idea for an improvement, feel free to open an issue or submit a Pull Request.

**Help Wanted: ASCII Art**
As a TUI application, `vm-curator` relies on visual flair to stand out. I am specifically looking for help with:
* **Logo/Banner Art:** A cool ASCII banner for the startup screen.
* **Iconography:** Small, recognizable ASCII/block character icons for the TUI menus (e.g., stylized hard drives, network cards, or GPU icons).

If you have a knack for terminal aesthetics, your PRs are highly appreciated!

### Support & Maintenance Status

**`vm-curator`** was built to solve a specific, painful problem: getting high-performance, 3D-accelerated Linux VMs (via QEMU) without the overhead and complexity of `libvirt` or `virt-manager`.

This is a **personal passion project** that I am sharing with the community. While I use this tool daily and will fix critical bugs as I encounter them, please note:

* **Development Pace:** This project is maintained in my spare time. Feature requests will be considered but are not guaranteed.
* **The "As-Is" Philosophy:** The goal is a lean, transparent TUI. I prioritize stability and performance over comprehensive enterprise feature parity.

**If this tool saved you time or helped you get 3D Acceleration working without having to resort to passthrough:**

If you'd like to say thanks, you can support the project below. **Donations are a "thank you" for existing work, not a payment for future support.**

* **[GitHub Sponsors](https://github.com/sponsors/mroboff):** Best for one-time contributions (Goes to the RTX-Pro 6000 fund!)
* **[Ko-fi](https://ko-fi.com/mroboff):** Buy me a coffee (or a generic energy drink).

---

### License

MIT
