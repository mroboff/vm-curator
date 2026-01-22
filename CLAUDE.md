# CLAUDE.md

This file provides context to Claude Code when working in this repository.

## Project Overview

**computer-history-with-claude** is a project for managing a collection of 30+ QEMU virtual machines showcasing historic operating systems. The main component is `vm-curator`, a Rust TUI application.

## Repository Structure

```
.
├── README.md
├── CLAUDE.md
├── LICENSE
└── vm-curator/           # Rust TUI application
    ├── Cargo.toml
    ├── .gitignore
    ├── assets/
    │   └── metadata/
    │       └── defaults.toml    # Embedded OS metadata
    └── src/
        ├── main.rs              # Entry point, CLI parsing
        ├── app.rs               # Application state machine
        ├── config/              # User settings
        ├── vm/                  # VM discovery, parsing, lifecycle
        ├── metadata/            # OS info, ASCII art
        ├── hardware/            # USB enumeration
        ├── ui/                  # TUI screens and widgets
        │   ├── screens/
        │   └── widgets/
        └── commands/            # qemu-img, qemu-system wrappers
```

## Tech Stack

- **Language**: Rust (edition 2021)
- **TUI Framework**: ratatui 0.30 with crossterm 0.29
- **Async Runtime**: tokio (for VM launching)
- **CLI**: clap 4.5 with derive feature
- **Serialization**: serde + toml for config/metadata
- **USB**: libudev for device enumeration

## Key Concepts

### VM Discovery
VMs are discovered by scanning `~/vm-space/` for directories containing a `launch.sh` script. The script is parsed to extract QEMU configuration.

### VM Library Path
Default: `~/vm-space/`
Each VM is a directory with at minimum:
- `launch.sh` - Bash script that invokes QEMU
- Disk image(s) - qcow2 (supports snapshots) or raw

### Metadata
OS metadata (blurbs, fun facts, release dates) is embedded at compile time from `assets/metadata/defaults.toml`. Users can override with files in `~/.config/vm-curator/metadata/`.

## Build Commands

```bash
cd vm-curator
cargo build              # Debug build
cargo build --release    # Release build
cargo run                # Run TUI
cargo run -- list        # Run CLI command
cargo test               # Run tests
```

## Common Development Tasks

### Adding a new OS to metadata
Edit `vm-curator/assets/metadata/defaults.toml` and add a new section:
```toml
[os-name]
name = "Display Name"
publisher = "Publisher"
release_date = "YYYY-MM-DD"
architecture = "i386|x86_64|ppc|m68k"

[os-name.blurb]
short = "One-line description"
long = "Multi-paragraph description"

[os-name.fun_facts]
facts = ["Fact 1", "Fact 2"]
```

### Adding ASCII art
Add to `vm-curator/src/metadata/ascii_art.rs` in the `load_embedded()` function.

### Adding a new TUI screen
1. Create `vm-curator/src/ui/screens/new_screen.rs`
2. Add to `vm-curator/src/ui/screens/mod.rs`
3. Add screen variant to `Screen` enum in `app.rs`
4. Handle rendering and input in `ui/mod.rs`

## Architecture Notes

- **State Machine**: `App` struct in `app.rs` holds all application state
- **Screen Stack**: Navigation uses a stack for back/forward
- **No Async TUI**: The TUI runs synchronously; async is only for VM launching
- **Graceful Degradation**: If launch.sh parsing fails, raw script is preserved

## Testing

The project has unit tests for parsing and data structures. Run with:
```bash
cargo test
```

## Dependencies to Note

- `libudev` requires libudev-dev system package on Debian/Ubuntu
- QEMU must be installed for actual VM operations
- qemu-img required for snapshot management
