## vm-curator

A Rust TUI application for managing QEMU virtual machine libraries. Discover, organize, launch, and create VMs with an intuitive terminal interface.

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

### License

MIT
