# computer-history-with-claude

A project using Claude Code (CLI) to build, maintain, and manage over 30 QEMU virtual machines showcasing a variety of Linux distributions and operating systems from computing history.

## vm-curator

A Rust TUI application for managing the QEMU VM library at `~/vm-space`.

### Features

- **Interactive TUI**: Full terminal interface with vim-style navigation (j/k, arrow keys)
- **VM Discovery**: Automatically scans `~/vm-space` for VMs with `launch.sh` scripts
- **Launch Script Parsing**: Extracts QEMU configuration (emulator, memory, CPU, VGA, audio, disks)
- **Snapshot Management**: Create, restore, and delete snapshots for qcow2 disk images
- **OS Metadata**: Embedded historical information, release dates, and fun facts for classic OSes
- **ASCII Art**: Nostalgic ASCII logos for Windows, Mac, and Linux variants
- **USB Passthrough**: USB device enumeration for hardware passthrough (via libudev)
- **CLI Commands**: Non-interactive mode for scripting and automation

### Screenshots

```
┌─────────────────────────────────────────────────────────────────┐
│                   vm-curator - QEMU Library                     │
├─────────────────────────────────────────────────────────────────┤
│ ┌─────────────────────┐  ┌────────────────────────────────────┐ │
│ │  VMs (31)           │  │     _    _ _           _           │ │
│ │ ───────────────────-│  │    | |  | (_)         | |          │ │
│ │ > Windows 95    [*] │  │    | |/\| |_ _ __   __| | _____    │ │
│ │   Windows ME        │  │                                    │ │
│ │   Windows 11        │  │   Windows 95 OSR2.5                │ │
│ │ ────────────────────│  │   Microsoft | August 1995 | i386   │ │
│ │   Mac System 7      │  │                                    │ │
│ │   Mac OS 9          │  │   The OS that changed everything - │ │
│ │   Mac OS X Tiger    │  │   Start Menu, taskbar, and 32-bit  │ │
│ │ ────────────────────│  │   computing for the masses.        │ │
│ │   Linux Fedora      │  │                                    │ │
│ └─────────────────────┘  └────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  [Enter] Launch  [m] Manage  [c] Config  [?] Help  [q] Quit    │
└─────────────────────────────────────────────────────────────────┘
```

### Installation

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

| Key | Action |
|-----|--------|
| `j/k` or `Down/Up` | Navigate VM list |
| `Enter` | Launch selected VM |
| `m` | Open Management menu |
| `c` | View Configuration |
| `i` | View detailed Info |
| `/` | Search/filter VMs |
| `?` | Help |
| `Esc` | Back |
| `q` | Quit |

### VM Library Structure

VMs are expected to be in `~/vm-space/` with the following structure:

```
~/vm-space/
├── windows-95/
│   ├── launch.sh      # QEMU launch script
│   └── disk.qcow2     # Disk image
├── mac-osx-tiger/
│   ├── launch.sh
│   └── disk.qcow2
└── linux-fedora/
    ├── launch.sh
    └── disk.qcow2
```

### Supported Operating Systems

The metadata includes information for classic systems including:

- **Windows**: 95, 98, ME, 2000, XP, Vista, 7, 10, 11
- **Mac**: System 7, OS 9, OS X Tiger/Leopard
- **Linux**: Fedora, and other distributions
- **DOS**: MS-DOS, FreeDOS

### Dependencies

- Rust 1.70+
- QEMU (qemu-system-x86_64, qemu-system-i386, qemu-system-ppc, etc.)
- libudev (for USB device enumeration)

### License

MIT
