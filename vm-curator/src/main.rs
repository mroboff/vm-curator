mod app;
mod commands;
mod config;
mod hardware;
mod metadata;
mod ui;
mod vm;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::path::PathBuf;

use app::App;
use config::Config;

#[derive(Parser)]
#[command(name = "vm-curator")]
#[command(author = "Mark Roboff")]
#[command(version = "0.1.0")]
#[command(about = "A TUI application to manage your QEMU VM library")]
struct Cli {
    /// Path to VM library directory
    #[arg(short, long)]
    library: Option<PathBuf>,

    /// Subcommand to run
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all VMs in the library
    List,

    /// Launch a VM by name
    Launch {
        /// VM name or ID
        name: String,
        /// Boot in install mode
        #[arg(short, long)]
        install: bool,
        /// Boot with custom ISO
        #[arg(short, long)]
        cdrom: Option<PathBuf>,
    },

    /// Show VM configuration
    Info {
        /// VM name or ID
        name: String,
    },

    /// Manage snapshots
    Snapshot {
        /// VM name or ID
        name: String,
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// List available QEMU emulators
    Emulators,
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// List snapshots
    List,
    /// Create a snapshot
    Create {
        /// Snapshot name
        snapshot_name: String,
    },
    /// Restore a snapshot
    Restore {
        /// Snapshot name
        snapshot_name: String,
    },
    /// Delete a snapshot
    Delete {
        /// Snapshot name
        snapshot_name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration
    let mut config = Config::load()?;

    // Override library path if provided
    if let Some(ref library) = cli.library {
        config.vm_library_path = library.clone();
    }

    // Handle subcommands
    match cli.command {
        Some(Commands::List) => cmd_list(&config),
        Some(Commands::Launch { name, install, cdrom }) => cmd_launch(&config, &name, install, cdrom),
        Some(Commands::Info { name }) => cmd_info(&config, &name),
        Some(Commands::Snapshot { name, action }) => cmd_snapshot(&config, &name, action),
        Some(Commands::Emulators) => cmd_emulators(),
        None => run_tui(config),
    }
}

/// Guard that ensures terminal is restored on drop (even on panic)
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best effort restoration - ignore errors since we may be panicking
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = crossterm::cursor::Show;
    }
}

fn run_tui(config: Config) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Create guard AFTER setup so it only cleans up if setup succeeded
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(config)?;

    // Run the app - guard will restore terminal even if this panics
    ui::run(&mut terminal, &mut app)
}

fn cmd_list(config: &Config) -> Result<()> {
    let vms = vm::discover_vms(&config.vm_library_path)?;

    if vms.is_empty() {
        println!("No VMs found in {:?}", config.vm_library_path);
        return Ok(());
    }

    println!("VMs in {:?}:", config.vm_library_path);
    println!();

    let groups = vm::group_vms_by_category(&vms);
    for (category, group_vms) in groups {
        println!("{}:", category);
        for vm in group_vms {
            let arch = vm.config.emulator.architecture();
            let mem = vm.config.memory_mb;
            let snapshot_support = if vm.config.supports_snapshots() {
                "[snapshots]"
            } else {
                ""
            };
            println!(
                "  {:24} {:8} {:4}MB {}",
                vm.display_name(),
                arch,
                mem,
                snapshot_support
            );
        }
        println!();
    }

    println!("Total: {} VMs", vms.len());
    Ok(())
}

fn cmd_launch(config: &Config, name: &str, install: bool, cdrom: Option<PathBuf>) -> Result<()> {
    let vms = vm::discover_vms(&config.vm_library_path)?;

    let vm = vms
        .iter()
        .find(|v| v.id == name || v.display_name().to_lowercase() == name.to_lowercase())
        .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

    let boot_mode = if let Some(iso) = cdrom {
        vm::BootMode::Cdrom(iso)
    } else if install {
        vm::BootMode::Install
    } else {
        vm::BootMode::Normal
    };

    let options = vm::LaunchOptions {
        boot_mode,
        extra_args: Vec::new(),
        usb_devices: Vec::new(),
    };

    println!("Launching {}...", vm.display_name());
    vm::launch_vm_sync(vm, &options)?;
    println!("VM started.");

    Ok(())
}

fn cmd_info(config: &Config, name: &str) -> Result<()> {
    let vms = vm::discover_vms(&config.vm_library_path)?;

    let vm = vms
        .iter()
        .find(|v| v.id == name || v.display_name().to_lowercase() == name.to_lowercase())
        .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

    println!("VM: {}", vm.display_name());
    println!("ID: {}", vm.id);
    println!("Path: {:?}", vm.path);
    println!();
    println!("Configuration:");
    println!("  Emulator: {}", vm.config.emulator.command());
    println!("  Architecture: {}", vm.config.emulator.architecture());
    println!("  Memory: {} MB", vm.config.memory_mb);
    println!("  CPU Cores: {}", vm.config.cpu_cores);

    if let Some(ref model) = vm.config.cpu_model {
        println!("  CPU Model: {}", model);
    }
    if let Some(ref machine) = vm.config.machine {
        println!("  Machine: {}", machine);
    }

    println!("  VGA: {:?}", vm.config.vga);
    println!("  KVM: {}", vm.config.enable_kvm);
    println!("  UEFI: {}", vm.config.uefi);
    println!("  TPM: {}", vm.config.tpm);

    println!();
    println!("Disks:");
    for disk in &vm.config.disks {
        println!(
            "  {:?} ({:?}, {})",
            disk.path, disk.format, disk.interface
        );
    }

    println!();
    println!("Snapshots supported: {}", vm.config.supports_snapshots());

    if vm.config.supports_snapshots() {
        if let Some(disk) = vm.config.primary_disk() {
            let snapshots = vm::list_snapshots(&disk.path)?;
            if !snapshots.is_empty() {
                println!();
                println!("Snapshots:");
                for snap in snapshots {
                    println!("  {} ({}, {})", snap.name, snap.date, snap.size);
                }
            }
        }
    }

    Ok(())
}

fn cmd_snapshot(config: &Config, name: &str, action: SnapshotAction) -> Result<()> {
    let vms = vm::discover_vms(&config.vm_library_path)?;

    let vm = vms
        .iter()
        .find(|v| v.id == name || v.display_name().to_lowercase() == name.to_lowercase())
        .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?;

    if !vm.config.supports_snapshots() {
        anyhow::bail!("VM '{}' does not support snapshots (raw disk format)", name);
    }

    let disk = vm
        .config
        .primary_disk()
        .ok_or_else(|| anyhow::anyhow!("VM has no disk configured"))?;

    match action {
        SnapshotAction::List => {
            let snapshots = vm::list_snapshots(&disk.path)?;
            if snapshots.is_empty() {
                println!("No snapshots for {}", vm.display_name());
            } else {
                println!("Snapshots for {}:", vm.display_name());
                for snap in snapshots {
                    println!("  {} ({}, {})", snap.name, snap.date, snap.size);
                }
            }
        }
        SnapshotAction::Create { snapshot_name } => {
            println!("Creating snapshot '{}'...", snapshot_name);
            vm::create_snapshot(&disk.path, &snapshot_name)?;
            println!("Snapshot created.");
        }
        SnapshotAction::Restore { snapshot_name } => {
            println!("Restoring snapshot '{}'...", snapshot_name);
            vm::restore_snapshot(&disk.path, &snapshot_name)?;
            println!("Snapshot restored.");
        }
        SnapshotAction::Delete { snapshot_name } => {
            println!("Deleting snapshot '{}'...", snapshot_name);
            vm::delete_snapshot(&disk.path, &snapshot_name)?;
            println!("Snapshot deleted.");
        }
    }

    Ok(())
}

fn cmd_emulators() -> Result<()> {
    println!("Available QEMU emulators:");
    println!();

    let emulators = commands::qemu_system::list_available_emulators();

    if emulators.is_empty() {
        println!("  No QEMU emulators found. Please install QEMU.");
        return Ok(());
    }

    for emulator in emulators {
        if let Ok(version) = commands::qemu_system::get_qemu_version(&emulator) {
            println!("  {} - {}", emulator, version);
        } else {
            println!("  {}", emulator);
        }
    }

    println!();

    if commands::qemu_system::is_kvm_available() {
        if let Some(module) = commands::qemu_system::get_kvm_info() {
            println!("KVM: available ({})", module);
        } else {
            println!("KVM: available");
        }
    } else {
        println!("KVM: not available");
    }

    Ok(())
}
