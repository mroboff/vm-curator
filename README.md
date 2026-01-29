# vm-curator

A fast and friendly Rust TUI for managing desktop QEMU/KVM virtual machines with 3D acceleration!

### Important Note ##

Para-virtualized and full GPU pass-through (single and multi) are now operational. Testing and feedback wanted!

Please see [discussion](https://github.com/mroboff/vm-curator/discussions/11) for more information, and to post your results.

### Changelog

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

### Features

**VM Discovery & Organization**
- Automatically scans your VM library for directories containing `launch.sh` scripts
- Hierarchical organization by OS family (Windows, Linux, macOS, BSD, etc.)
- Parses QEMU launch scripts to extract configuration (emulator, memory, CPU, VGA, audio, disks)
- Smart categorization based on configurable hierarchy patterns

**VM Creation Wizard**
- 5-step guided wizard for creating new VMs
- 50+ pre-configured OS profiles with optimal QEMU settings
- Automatic UEFI firmware detection across Linux distributions (Arch, Debian, Fedora, NixOS, etc.)
- ISO file browser for selecting installation media
- Configurable disk size, memory, CPU cores, and QEMU options
- Support for custom OS entries with user metadata

**Snapshot Management**
- Create, restore, and delete snapshots for qcow2 disk images
- Visual snapshot list with timestamps and sizes
- Background operations with progress feedback

**Launch Script Editor**
- Edit `launch.sh` scripts directly in the TUI
- Syntax-aware display with line numbers
- Automatic QEMU configuration re-parsing after saves

**USB Passthrough**
- USB device enumeration via libudev
- Select devices for passthrough to VMs
- Persistent passthrough configuration

**Additional Features**
- Vim-style navigation (j/k, arrows, mouse)
- Search and filter VMs
- Multiple boot modes (normal, install, custom ISO)
- OS metadata with historical blurbs and fun facts
- ASCII art logos for classic operating systems
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

**Prerequisites**
- Rust 1.70+
- QEMU (`qemu-system-*` binaries)
- libudev-dev (Debian/Ubuntu) or libudev (Arch/Fedora)

```bash
cd vm-curator
cargo build --release
```

The binary will be at `target/release/vm-curator`.

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
| `c` | Open VM creation wizard |
| `s` | Open settings |
| `/` | Search/filter VMs |
| `?` | Show help |
| `PgUp/PgDn` | Scroll info panel |
| `Esc` | Back / Cancel |
| `q` | Quit |

#### VM Management

| Key | Action |
|-----|--------|
| `Enter` | Select menu option |
| `e` | Edit launch script |
| `u` | Configure USB passthrough |

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
default_display = "gtk"      # gtk, sdl, spice
default_enable_kvm = true

# Behavior
confirm_before_launch = true
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

The creation wizard includes pre-configured profiles for 50+ operating systems:

**Microsoft**: DOS, Windows 3.x, 95, 98, ME, 2000, XP, Vista, 7, 8, 10, 11, Server editions

**Apple**: Classic Mac OS (System 6-9), Mac OS X (10.4-10.15), macOS (11+)

**Linux**: Arch, Debian, Ubuntu, Fedora, openSUSE, Mint, CentOS, RHEL, Gentoo, Slackware, Alpine, NixOS, Void, EndeavourOS, Manjaro, and more

**BSD**: FreeBSD, OpenBSD, NetBSD, DragonFly BSD

**Unix**: Solaris, OpenIndiana, illumos

**Other**: Haiku, ReactOS, FreeDOS, Plan 9, Minix, TempleOS

Each profile includes optimal QEMU settings for that OS (emulator, machine type, VGA, audio, network, etc.).

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

- **Runtime**: QEMU, qemu-img (for snapshots), libudev
- **Build**: Rust 1.70+, libudev-dev

### Cross-Distribution Compatibility

VM Curator automatically detects OVMF/UEFI firmware paths across Linux distributions:
- Arch Linux: `/usr/share/edk2/x64/OVMF_CODE.4m.fd`
- Debian/Ubuntu: `/usr/share/OVMF/OVMF_CODE.fd`
- Fedora/RHEL: `/usr/share/edk2/ovmf/OVMF_CODE.fd`
- NixOS: Multiple search paths supported
- And more...

---

### 🤝 Contributing

Contributions are welcome! If you find a bug or have an idea for an improvement, feel free to open an issue or submit a Pull Request.

**Help Wanted: ASCII Art**
As a TUI application, `vm-curator` relies on visual flair to stand out. I am specifically looking for help with:
* **Logo/Banner Art:** A cool ASCII banner for the startup screen.
* **Iconography:** Small, recognizable ASCII/block character icons for the TUI menus (e.g., stylized hard drives, network cards, or GPU icons).

If you have a knack for terminal aesthetics, your PRs are highly appreciated!

### ☕ Support & Maintenance Status

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
