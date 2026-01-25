# VM Creation Wizard Implementation Plan

## Vision

VM Curator aims to be a full-featured alternative to Virt-Manager that works directly with QEMU/KVM without libvirt dependency. This enables:
- Direct QEMU control without libvirt bugs (e.g., NVIDIA 3D acceleration issues)
- Fast, responsive Rust TUI experience
- Curated, managed VM experience with sensible defaults
- Support for everything QEMU can do

## Feature: VM Creation Wizard

A 5-step wizard accessible via `c` key from the main menu to create new VMs with OS-specific QEMU defaults, proper directory structure, and generated launch scripts.

---

## Phase 1: QEMU Profiles Metadata

### New file: `assets/metadata/qemu_profiles.toml`

Contains OS-specific QEMU defaults for ALL supported OSes (not just locally created ones).

```toml
[windows-11]
display_name = "Windows 11"
category = "windows"
emulator = "qemu-system-x86_64"
memory_mb = 8192
cpu_cores = 4
cpu_model = "host"
machine = "q35"
vga = "qxl"
audio = ["intel-hda", "hda-duplex"]
network_model = "e1000"
disk_interface = "ide"
disk_size_gb = 128
enable_kvm = true
uefi = true
tpm = false
rtc_localtime = true
usb_tablet = true
display = "gtk"

[windows-10]
# ... similar

[debian]
display_name = "Debian GNU/Linux"
category = "linux"
emulator = "qemu-system-x86_64"
memory_mb = 2048
cpu_cores = 2
cpu_model = "host"
machine = "q35"
vga = "virtio"
audio = ["intel-hda", "hda-duplex"]
network_model = "virtio"
disk_interface = "virtio"
disk_size_gb = 32
enable_kvm = true
uefi = false
rtc_localtime = false
usb_tablet = true
display = "gtk"
iso_url = "https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/"

# Profiles for ALL OSes in metadata:
# - Windows: 11, 10, 8.1, 8, 7, Vista, XP, 2000, ME, 98, 95, 3.1, DOS
# - Linux: Debian, Ubuntu, Fedora, Arch, Mint, Pop!_OS, openSUSE, etc.
# - BSD: FreeBSD, OpenBSD, NetBSD
# - Other: Haiku, ReactOS, Plan 9, Solaris, etc.
# - Classic Mac: System 6/7, OS 9 (note: PPC support deferred to post-V1.0)
# - Classic: Amiga, Atari ST, etc. (architecture support deferred)
```

### New file: `src/metadata/qemu_profiles.rs`

```rust
pub struct QemuProfile {
    pub id: String,
    pub display_name: String,
    pub category: String,
    pub emulator: String,
    pub memory_mb: u32,
    pub cpu_cores: u32,
    pub cpu_model: Option<String>,
    pub machine: Option<String>,
    pub vga: String,
    pub audio: Vec<String>,
    pub network_model: String,
    pub disk_interface: String,
    pub disk_size_gb: u32,
    pub enable_kvm: bool,
    pub uefi: bool,
    pub tpm: bool,
    pub rtc_localtime: bool,
    pub usb_tablet: bool,
    pub display: String,
    pub extra_args: Vec<String>,
    pub iso_url: Option<String>,
    pub notes: Option<String>,  // Tips for this OS
}

pub struct QemuProfileStore { /* ... */ }

impl QemuProfileStore {
    pub fn load_embedded() -> Self;
    pub fn load_user_overrides(path: &Path) -> Self;
    pub fn merge(&mut self, other: Self);
    pub fn get(&self, os_id: &str) -> Option<&QemuProfile>;
    pub fn list_all(&self) -> Vec<&QemuProfile>;
    pub fn list_by_category(&self, category: &str) -> Vec<&QemuProfile>;
    pub fn default_profile() -> QemuProfile;  // Fallback for unknown OS
}
```

---

## Phase 2: App State Extensions

### Add to `src/app.rs`:

```rust
/// Wizard state for VM creation
pub struct CreateWizardState {
    pub step: WizardStep,
    pub vm_name: String,
    pub folder_name: String,              // Auto-generated from vm_name
    pub selected_os: Option<String>,      // OS ID from profiles
    pub custom_os: Option<CustomOsEntry>, // If "Other" selected
    pub iso_path: Option<PathBuf>,
    pub iso_downloading: bool,
    pub iso_download_progress: f32,
    pub disk_size_gb: u32,
    pub qemu_config: WizardQemuConfig,
    pub auto_launch: bool,
    pub field_focus: usize,               // Which field is focused
    pub os_list_scroll: usize,            // Scroll position in OS list
    pub os_filter: String,                // Search/filter for OS list
}

pub struct WizardQemuConfig {
    pub emulator: String,
    pub memory_mb: u32,
    pub cpu_cores: u32,
    pub cpu_model: Option<String>,
    pub machine: Option<String>,
    pub vga: String,
    pub audio: Vec<String>,
    pub network_model: String,
    pub disk_interface: String,
    pub enable_kvm: bool,
    pub uefi: bool,
    pub tpm: bool,
    pub rtc_localtime: bool,
    pub usb_tablet: bool,
    pub display: String,
    pub extra_args: Vec<String>,
}

pub struct CustomOsEntry {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub release_date: Option<String>,
    pub architecture: String,
    pub short_blurb: String,
    pub long_blurb: String,
    pub fun_facts: Vec<String>,
    // QEMU profile fields also included
}

#[derive(Clone, PartialEq, Eq)]
pub enum WizardStep {
    SelectOs,       // Step 1: Name + OS selection
    SelectIso,      // Step 2: ISO selection/download
    ConfigureDisk,  // Step 3: Disk size/settings
    ConfigureQemu,  // Step 4: QEMU settings
    Confirm,        // Step 5: Summary + create
}
```

### Add Screen variants:

```rust
pub enum Screen {
    // ... existing variants
    CreateWizard,           // Main wizard screens (step tracked in wizard state)
    CreateWizardCustomOs,   // Secondary form for "Other" OS metadata entry
    CreateWizardDownload,   // ISO download progress screen
}
```

### Add to App struct:

```rust
pub struct App {
    // ... existing fields
    pub wizard_state: Option<CreateWizardState>,
    pub qemu_profiles: QemuProfileStore,
}
```

---

## Phase 3: VM Creation Logic

### New file: `src/vm/create.rs`

```rust
/// Convert display name to folder name: "Windows 7" → "windows-7"
pub fn generate_folder_name(display_name: &str) -> String;

/// Check if folder name already exists in vm_library_path
pub fn folder_exists(library_path: &Path, folder_name: &str) -> bool;

/// Create VM directory structure: ~/vm-space/[name]/
pub fn create_vm_directory(library_path: &Path, folder_name: &str) -> Result<PathBuf>;

/// Create disk image with qemu-img
pub fn create_disk_image(vm_dir: &Path, name: &str, size_gb: u32) -> Result<PathBuf>;

/// Generate launch.sh script content
pub fn generate_launch_script(
    vm_name: &str,
    disk_filename: &str,
    iso_path: Option<&Path>,
    config: &WizardQemuConfig,
) -> String;

/// Write launch.sh to VM directory
pub fn write_launch_script(vm_dir: &Path, content: &str) -> Result<()>;

/// Download ISO with progress reporting
pub fn download_iso(
    url: &str,
    dest_dir: &Path,
    progress_tx: Sender<DownloadProgress>
) -> Result<PathBuf>;

/// Full VM creation orchestration
pub fn create_vm(
    library_path: &Path,
    state: &CreateWizardState,
) -> Result<CreatedVm>;

pub struct CreatedVm {
    pub path: PathBuf,
    pub launch_script: PathBuf,
    pub disk_image: PathBuf,
}

pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub finished: bool,
    pub error: Option<String>,
}
```

---

## Phase 4: Wizard UI Screens

### New file: `src/ui/screens/create_wizard.rs`

#### Step 1: Select OS (`render_step_select_os`)

Layout:
```
┌─────────────────── Create New VM (1/5) ───────────────────┐
│                                                            │
│  VM Name: [_______________Windows 7 Pro________________]   │
│                                                            │
│  Select Operating System:  [Filter: ________]              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ > Windows                                            │  │
│  │     Windows 11          8GB RAM, 128GB, UEFI        │  │
│  │     Windows 10          4GB RAM, 64GB               │  │
│  │   * Windows 7           4GB RAM, 64GB               │  │
│  │     Windows XP          512MB RAM, 16GB             │  │
│  │   Linux                                              │  │
│  │     Debian              2GB RAM, 32GB, virtio       │  │
│  │     Ubuntu              2GB RAM, 32GB, virtio       │  │
│  │   BSD                                                │  │
│  │     FreeBSD             1GB RAM, 16GB               │  │
│  │   Other                                              │  │
│  │     Custom OS...        Define your own             │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  [Tab] Switch field  [Enter] Next  [Esc] Cancel           │
└────────────────────────────────────────────────────────────┘
```

Features:
- Text input for VM name (auto-generates folder name)
- Hierarchical OS list grouped by category
- Filter/search box for OS list
- Shows profile summary (RAM, disk, notes) for selected OS
- "Custom OS..." option at bottom triggers CustomOs screen

#### Step 2: Select ISO (`render_step_select_iso`)

Layout:
```
┌─────────────────── Create New VM (2/5) ───────────────────┐
│                                                            │
│  Operating System: Windows 7                               │
│                                                            │
│  Installation ISO:                                         │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  ( ) Browse for local ISO file...                    │  │
│  │  ( ) No ISO (configure later)                        │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  Selected: /home/user/vm-space/ISOs/Win7.iso              │
│                                                            │
│  [Enter] Browse  [n] No ISO  [Esc] Back                   │
└────────────────────────────────────────────────────────────┘
```

For OSes with iso_url (free/open-source):
```
│  ┌──────────────────────────────────────────────────────┐  │
│  │  ( ) Download Debian 12 netinst ISO                  │  │
│  │  ( ) Browse for local ISO file...                    │  │
│  │  ( ) No ISO (configure later)                        │  │
│  └──────────────────────────────────────────────────────┘  │
```

#### Step 3: Configure Disk (`render_step_configure_disk`)

Layout:
```
┌─────────────────── Create New VM (3/5) ───────────────────┐
│                                                            │
│  Disk Configuration                                        │
│                                                            │
│  Disk Size:  [ 64 ] GB    (Recommended: 64 GB)            │
│                                                            │
│  ┌─ Disk Info ─────────────────────────────────────────┐  │
│  │  Format: qcow2 (copy-on-write, snapshots supported) │  │
│  │  Type: Expandable (only uses space as needed)       │  │
│  │  Location: ~/vm-space/windows-7/windows-7.qcow2     │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                            │
│  [←/→] Adjust size  [Enter] Next  [Esc] Back              │
└────────────────────────────────────────────────────────────┘
```

#### Step 4: Configure QEMU (`render_step_configure_qemu`)

Layout:
```
┌─────────────────── Create New VM (4/5) ───────────────────┐
│                                                            │
│  QEMU Configuration            [r] Reset to defaults      │
│                                                            │
│  ┌─ Hardware ──────────────────────────────────────────┐  │
│  │  Memory:      [ 4096 ] MB     (Recommended: 4096)   │  │
│  │  CPU Cores:   [    4 ]        (Recommended: 4)      │  │
│  │  CPU Model:   [ host        ▼]                      │  │
│  │  Machine:     [ q35         ▼]                      │  │
│  └─────────────────────────────────────────────────────┘  │
│  ┌─ Display & Audio ───────────────────────────────────┐  │
│  │  VGA:         [ qxl         ▼]                      │  │
│  │  Display:     [ gtk         ▼]                      │  │
│  │  Audio:       [✓] Intel HDA  [ ] AC97  [ ] SB16    │  │
│  └─────────────────────────────────────────────────────┘  │
│  ┌─ Storage & Network ─────────────────────────────────┐  │
│  │  Disk I/F:    [ ide         ▼]                      │  │
│  │  Network:     [ e1000       ▼]                      │  │
│  └─────────────────────────────────────────────────────┘  │
│  ┌─ Options ───────────────────────────────────────────┐  │
│  │  [✓] Enable KVM acceleration                        │  │
│  │  [ ] UEFI boot                                      │  │
│  │  [✓] USB tablet (better mouse)                      │  │
│  │  [✓] RTC uses local time (for Windows)              │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                            │
│  [Tab] Next field  [Space] Toggle  [Enter] Next  [Esc] Back│
└────────────────────────────────────────────────────────────┘
```

#### Step 5: Confirm (`render_step_confirm`)

Layout:
```
┌─────────────────── Create New VM (5/5) ───────────────────┐
│                                                            │
│  Summary                                                   │
│  ═══════                                                   │
│                                                            │
│  VM Name:        Windows 7 Pro                             │
│  Folder:         ~/vm-space/windows-7-pro/                 │
│  OS Type:        Windows 7                                 │
│                                                            │
│  Disk:           64 GB qcow2 (expandable)                  │
│  ISO:            ~/vm-space/ISOs/Win7.iso                  │
│                                                            │
│  Hardware:       4 cores, 4096 MB RAM                      │
│  Graphics:       QXL                                       │
│  Audio:          Intel HDA                                 │
│  Network:        e1000                                     │
│  Acceleration:   KVM enabled                               │
│                                                            │
│  ┌─────────────────────────────────────────────────────┐  │
│  │  [✓] Launch VM in install mode after creation       │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                            │
│           [ Create VM ]         [ Cancel ]                 │
│                                                            │
│  [Enter] Create  [Space] Toggle launch  [Esc] Back        │
└────────────────────────────────────────────────────────────┘
```

#### Custom OS Form (`render_custom_os_form`)

For when user selects "Other/Custom OS":
```
┌──────────────────── Custom OS Entry ──────────────────────┐
│                                                            │
│  OS Identifier:    [__my-custom-os__________________]     │
│  Display Name:     [__My Custom OS__________________]     │
│  Publisher:        [__Custom Publisher______________]     │
│  Release Date:     [__2024-01-01____] (YYYY-MM-DD)       │
│  Architecture:     [ x86_64      ▼]                       │
│                                                            │
│  Short Description (one line):                            │
│  [__A custom operating system for testing___________]     │
│                                                            │
│  ┌─ QEMU Defaults for this OS ─────────────────────────┐  │
│  │  Base profile: [ Generic Linux ▼]                   │  │
│  │  (You can adjust settings in step 4)                │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                            │
│  [ ] Save to user metadata for future use                 │
│                                                            │
│  [Enter] Continue  [Esc] Cancel                           │
│                                                            │
│  Tip: Consider contributing new OS profiles to the        │
│  project at github.com/mroboff/computer-history-with-claude│
└────────────────────────────────────────────────────────────┘
```

---

## Phase 5: Navigation & Input Handling

### Key bindings

**Main menu:**
- `c` → Start create wizard

**All wizard steps:**
- `Esc` → Go back (or cancel on step 1)
- `Enter` → Next step / confirm action
- `Tab` / `Shift+Tab` → Move between fields

**Step-specific:**
- Step 1: Arrow keys for OS list, typing for filter/name
- Step 2: Arrow keys for options, Enter to browse
- Step 3: Left/Right or type number for disk size
- Step 4: Arrow keys between fields, Space to toggle, Enter for dropdowns
- Step 5: Space to toggle auto-launch, Enter to create

### Validation

- Step 1: VM name required, folder name must not exist
- Step 2: ISO path must exist if specified (unless "No ISO")
- Step 3: Disk size must be > 0 and reasonable (< 10TB)
- Step 4: Memory must be > 0, cores must be > 0
- Step 5: Ready to create

---

## Phase 6: Launch Script Generation

Template matching existing scripts in the user's collection:

```bash
#!/bin/bash

# ${VM_DISPLAY_NAME} VM Launch Script
# ${CPU_CORES} CPUs, ${MEMORY_MB}MB RAM, ${VGA} graphics, ${DISK_SIZE}GB disk

VM_DIR="$(dirname "$(readlink -f "$0")")"
DISK="$VM_DIR/${DISK_FILENAME}"
ISO="${ISO_PATH}"

show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --install        Boot from installation media"
    echo "  --cdrom <iso>    Boot with specified ISO as CD-ROM"
    echo "  (no options)     Normal boot from hard disk"
}

case "$1" in
    --install)
        if [[ ! -f "$ISO" ]]; then
            echo "Error: Installation ISO not found at $ISO"
            exit 1
        fi
        echo "Booting from installation ISO..."
        exec ${QEMU_COMMAND_INSTALL}
        ;;
    --cdrom)
        if [[ -z "$2" ]] || [[ ! -f "$2" ]]; then
            echo "Error: Please specify a valid ISO file"
            exit 1
        fi
        echo "Booting with CD-ROM: $2"
        exec ${QEMU_COMMAND_CDROM}
        ;;
    --help|-h)
        show_help
        exit 0
        ;;
    "")
        echo "Booting ${VM_DISPLAY_NAME}..."
        exec ${QEMU_COMMAND_NORMAL}
        ;;
    *)
        echo "Unknown option: $1"
        show_help
        exit 1
        ;;
esac
```

QEMU command construction includes:
- Emulator selection
- `-name "VM Name"`
- `-enable-kvm` (if enabled)
- `-machine ${machine},accel=kvm`
- `-cpu ${cpu_model}`
- `-smp ${cores},sockets=1,cores=${cores},threads=1`
- `-m ${memory}M` or `-m ${memory/1024}G`
- `-drive file="$DISK",format=qcow2,if=${interface},index=0,media=disk`
- `-vga ${vga}`
- `-display ${display}`
- Audio device configuration
- Network configuration
- `-usb -device usb-tablet` (if enabled)
- `-rtc base=localtime` (if enabled for Windows)
- UEFI/OVMF configuration (if enabled)

---

## Phase 7: File Structure

```
assets/metadata/
├── defaults.toml           # Existing OS metadata (blurbs, fun facts)
└── qemu_profiles.toml      # NEW: QEMU defaults per OS

src/
├── metadata/
│   ├── mod.rs              # Add qemu_profiles module
│   ├── os_info.rs          # Existing
│   └── qemu_profiles.rs    # NEW: Profile loading
├── vm/
│   ├── mod.rs              # Add create module
│   ├── create.rs           # NEW: VM creation logic
│   └── ...                 # Existing modules
└── ui/
    ├── mod.rs              # Add wizard handling
    └── screens/
        ├── mod.rs          # Add create_wizard module
        ├── create_wizard.rs # NEW: All wizard screens
        └── ...             # Existing screens
```

---

## Phase 8: Implementation Order

1. **Create `qemu_profiles.toml`** with profiles for ALL supported OSes
   - Research optimal defaults for each OS
   - Include ISO URLs for free/open-source OSes
   - Group by category

2. **Create `qemu_profiles.rs`** to load and serve profiles
   - Embedded loading at compile time
   - User override support
   - Fallback default profile

3. **Add wizard state structs to `app.rs`**
   - CreateWizardState
   - WizardQemuConfig
   - WizardStep enum
   - CustomOsEntry

4. **Add Screen variants and initialize wizard state**
   - CreateWizard, CreateWizardCustomOs
   - Add qemu_profiles to App
   - Add wizard_state to App

5. **Create `create.rs`** with VM creation logic
   - Folder name generation
   - Directory creation
   - Disk image creation
   - Launch script generation

6. **Create `create_wizard.rs`** with Step 1
   - OS selection screen
   - Name input
   - Wire up 'c' key in main menu

7. **Implement Steps 2-5** one by one
   - ISO selection with file browser integration
   - Disk configuration
   - QEMU configuration
   - Summary and creation

8. **Implement ISO download** (for free OSes)
   - Background download with progress
   - Error handling

9. **Implement Custom OS form**
   - Metadata entry
   - Save to user config
   - Contribution message

10. **Testing and polish**
    - Test creation for multiple OS types
    - Verify generated scripts work
    - Navigation edge cases
    - Error messages

---

## Supported OS Profiles (V1.0)

### Windows (x86_64/i386)
- Windows 11, 10, 8.1, 8, 7, Vista, XP, 2000, ME, 98, 95
- MS-DOS / Windows 3.1

### Linux (x86_64)
- Debian, Ubuntu, Linux Mint, Pop!_OS
- Fedora, Red Hat, CentOS, Rocky Linux
- openSUSE, SUSE Linux
- Arch Linux, Manjaro, EndeavourOS
- Gentoo, Slackware
- NixOS, Void Linux
- Zorin OS, elementary OS
- Kali Linux, Parrot OS

### BSD (x86_64)
- FreeBSD, OpenBSD, NetBSD, DragonFly BSD

### Other (x86_64)
- Haiku, ReactOS
- Solaris, illumos
- Plan 9
- TempleOS
- Minix, SerenityOS

### Deferred to post-V1.0 (non-x86)
- macOS (requires different approach)
- Mac OS 9, System 7 (PowerPC)
- AmigaOS (m68k)
- Atari ST (m68k)
- RISC OS (ARM)

---

## Notes

- All profiles should be editable via user config overrides
- ISO URLs should only be included for legally distributable OSes
- Consider caching downloaded ISOs to avoid re-downloading
- Launch scripts should be consistent with user's existing scripts
- The wizard should feel snappy - no unnecessary delays
