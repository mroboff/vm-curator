# Changelog

**Unreleased**
- **Data-safety fixes**: deleting a running VM is now blocked (like reset already was), snapshot creation checks the running state, and a successful launch records its PID immediately so the 3-second status poll cannot leave a window where reset/delete/resize treats a running VM as stopped. Reset now builds the replacement overlay in a temporary file and renames it into place only after `qemu-img create` succeeds; a failed reset no longer deletes the old disk.
- **Fix memory parsing for explicit units**: `-m 16M`/`-m 32M` from the DOS/retro profiles was read as 16/32 GiB because any value below 64 was assumed to be gigabytes. Bare values now use QEMU's MiB default and `M`/`G`/`K` suffixes are honoured.
- **Fix libvirt import fidelity**: CD-ROM/floppy devices with an ISO source are no longer imported as writable system disks; all imported disks (not just the first) are attached to the generated `launch.sh`; imports with no existing disk files are refused instead of creating a VM that points at a missing image.
- **Fix 3D toggle**: it no longer corrupts `VM_CURATOR_VIDEO_DEVICE` by matching across newlines, and it updates that variable so a configured default window size cannot replace `virtio-vga-gl` with a non-GL device. `vm-curator info` now also reports `Virtio` for `-device virtio-vga-gl` instead of falling back to `Std`.
- **Narrow USB udev rules**: rules now match vendor *and* product instead of granting `MODE="0666"` to every device from a vendor; USB descriptor strings are sanitized before being written into the rules file.
- **Validate managed-network names on load**, so a tampered `network.toml` cannot point Start/Stop at a script outside the networks directory.
- **Smaller fixes**: mouse clicks after scrolling the VM list select the row under the cursor; running VMs in symlinked directories are detected; `KVM: available` now checks that `/dev/kvm` can actually be opened; embedded metadata/hierarchy parsing has regression tests; unused `nom`, `include_dir` and `unicode-width` dependencies were removed.

**v1.5.0**
- Code contributions in this release from [@HenriqueCrj](https://github.com/HenriqueCrj), [@adonespitogo](https://github.com/adonespitogo) and [@pimkoter](https://github.com/pimkoter), plus the multi-GPU, single-GPU and NixOS work carried over from earlier branches — thank you all, and thanks to everyone who filed the issues fixed below.
- **Virtual Disk Resize** (thanks @HenriqueCrj, #94): New **Resize Storage** action on the VM management screen grows a VM's system disk — raw or qcow2 — to a larger absolute size in GiB with `qemu-img resize`. It shows the disk path and current virtual capacity, presents a disk picker when a VM has several distinct system disks (repeated references to the same image are deduplicated), and refuses to shrink, to touch physical disks (`/dev/…` paths or block devices), firmware or media images, or a running VM; if the image's metadata cannot be inspected it fails closed. The guest's partition and filesystem still have to be expanded inside the VM afterwards — the status message reminds you. CI now installs `qemu-utils` and runs a real raw-image resize end to end.
- **Fix Duplicated and Garbled Disks in `info` / Configuration** (thanks @adonespitogo, #86): Disk parsing scanned the whole `launch.sh`, so on vm-curator's own generated scripts — which wrap the `--install`, `--cdrom`, `--recovery`, `--floppy` and default boot modes in a `case` statement — drives from every branch were merged into one list, producing duplicates and literal unresolved `$2` paths (18 entries instead of 3 on a UEFI + bridge VM). Disk extraction is now scoped to the last `qemu-system-*` invocation in the script, which is always the default boot branch vm-curator generates; scripts with no recognisable invocation fall back to the old whole-file scan.
- **Fix VMs on a Managed Network Losing Connectivity to Each Other** (thanks @adonespitogo, #87): NetworkManager doesn't know about vm-curator's bridges or the per-VM tap devices QEMU's bridge helper attaches at launch, and can deactivate a tap it considers unconfigured — silently detaching it from the bridge with no error anywhere. `net-up.sh` now writes `/etc/NetworkManager/conf.d/vm-curator-unmanaged.conf` (an `unmanaged-devices` entry for `vmc-*` and `tap*`) on first run, reloads NetworkManager, and marks the bridge unmanaged immediately; it is a no-op on hosts without `nmcli`. Note that the `tap*` match is system-wide, so other tap interfaces on the host also become unmanaged, and the file is left in place when a network is deleted.
- **Find `qemu-bridge-helper` on NixOS** (thanks @pimkoter, #89): The bridge-helper lookup now also checks `/run/wrappers/bin/qemu-bridge-helper`, where NixOS installs the setuid wrapper, so the Networks preflight no longer reports the helper as missing there.
- **UEFI Firmware Detection Without libvirt (NixOS)**: The only NixOS firmware path vm-curator knew was `/run/libvirt/nix-ovmf/`, which exists solely when `virtualisation.libvirtd` is enabled — so on a plain NixOS host (QEMU installed, no libvirt) UEFI and Windows 11 VMs were generated pointing at a nonexistent `/usr/share/OVMF/` and failed to boot. Firmware lookup now falls back to the edk2 images QEMU bundles in its own data directory (found via `qemu-system-x86_64 -L help`: `edk2-x86_64-code.fd` + `edk2-i386-vars.fd`, or the `-secure-code` variant for Secure Boot/TPM profiles), which works anywhere QEMU is installed. Existing distro paths are still preferred, so Arch/Debian/Fedora behaviour is unchanged.
  - New `VM_CURATOR_OVMF_DIR` environment variable: point it at any directory holding a CODE+VARS pair (OVMF or edk2 names) to override auto-detection — e.g. `${pkgs.OVMF.fd}/FV` on NixOS to use the standalone OVMF build.
  - Added a `.envrc` (`use flake`) so direnv/nix-direnv users get the flake's Rust toolchain and libudev automatically inside the repo.
- **Fix UEFI VMs Failing with `OVMF_VARS.fd: Permission denied` on NixOS**: `launch.sh` creates the per-VM UEFI variables file with `cp`, which preserves the template's mode — and a template in the read-only Nix store (0444) produced a read-only copy that QEMU could not open as writable pflash. Generated scripts now `chmod u+w` the copy (creating or pre-existing), and the TUI/CLI launch path repairs the file on VMs created before this fix, so affected VMs are fixed by upgrading and launching again.
- **Single-GPU Passthrough Setup for NixOS**: The one-time system setup wrote `/etc/modules-load.d/vfio.conf` and `/etc/modprobe.d/vfio.conf` and hunted for `mkinitcpio`/`dracut`/`update-initramfs` — an imperative FHS workflow that on NixOS leaves undeclared files behind (honoured at boot, invisible to `nixos-rebuild`) and finds no initramfs tool. On NixOS (`/etc/NIXOS` present) the setup now writes a self-contained Nix module to `~/.config/vm-curator/single-gpu-vfio.nix` — `boot.kernelParams` (IOMMU params chosen from the host CPU vendor), `boot.kernelModules`, and `boot.extraModprobeConfig` with the same `softdep`/`vfio_pci` options — and opens an unprivileged terminal showing it with copy/import/rebuild instructions. The script also recognises the stray files from an earlier run by their vm-curator headers and offers to remove them.
- **Fix Single-GPU Start Script Not Killing niri/Hyprland Sessions**: The compositor kill list lacked `niri`, and `hyprland` never matched because the process is named `Hyprland` (pkill is case-sensitive). Both are now handled, so the GPU driver can actually unload on those desktops.
- **Per-VM Launch Log**: Every launch from the TUI or CLI now writes `<vm>/qemu.log` — a header with the timestamp and exact command, then everything `launch.sh` and QEMU print to stdout and stderr for as long as the VM runs (the streams are redirected at the file, so the log is complete even though `vm-curator launch` returns immediately); the previous run is kept as `qemu.log.old`. Previously QEMU's stderr was held only in memory and shown only if the process died within the first second, so warnings from a VM that *ran* (virgl/GL, audio, USB) were unrecoverable. Launch failures now end with `Full log: <path>`.
- **Fix Generated Scripts Not Executable on NixOS**: All generated scripts (`launch.sh`, managed-network `net-up.sh`/`net-down.sh`, single-GPU `single-gpu-start.sh`/`-restore.sh`/setup) used `#!/bin/bash`, which does not exist on NixOS (only `/bin/sh` is provided). Launching from the TUI was unaffected (it runs `bash launch.sh`), but running `./launch.sh` by hand, and the Networks and single-GPU features — which execute their scripts directly via `sudo` — failed with `No such file or directory`. Scripts now use `#!/usr/bin/env bash`, which works on every supported distro.
- **SMB Shared Folders for Windows Guests** (thanks @ahmutkadim, #101): virtio-9p shares need a guest driver Windows doesn't have, and some hosts (Fedora with SELinux) fight 9p. A shared folder can now be switched to **SMB mode** (`m` on the Shared Folders screen), which uses QEMU's built-in SMB server on the user-mode netdev: the guest opens `\\10.0.2.4\qemu` in Explorer with no driver install, and Linux/macOS/BSD guests mount it with `cifs`/`smbfs`. The host only needs the `samba` package (QEMU runs `smbd` on demand). On Windows-profile VMs new folders default to SMB mode; the mount-instructions panel gains SMB tiers, including the registry line newer Windows editions need for guest access.
  - Implemented as `-set netdev.<id>.smb=<path>` inside the managed `SHARED_FOLDERS_ARGS` block, so the network settings and `launch.sh` generator are untouched and paths with commas work. SMB needs the `user` network backend and QEMU exports one directory per netdev, so saving refuses a second SMB folder or a bridge/passt VM with a clear message; a missing host `smbd` is flagged at save time. Existing 9p folders are unaffected.
- **Fix Create/Edit Network Form Hiding How to Save** (thanks @sae-lco, #100): The form's single help line was wider than the dialog, so on terminals narrower than ~75 columns it was clipped on the right — and the clipped part was `[s] Save  [Esc] Cancel`, leaving no visible way to finish (also reported in #53). The form now has a real **`[ Save ]  [ Cancel ]`** button row reachable with `j`/`k`/`Tab` and activated with `Enter`, the help is split onto two lines that wrap instead of clipping and are anchored to the bottom of the dialog so a short terminal shrinks the info text rather than the hints, and the dialog is a little wider. `s` and `Esc` still work from any field. The Networks list screen's help bar wraps the same way.
- **Document How to Swap Floppies/CDs During an Install** (thanks @Revali25, #99): Multi-disk installs such as MS-DOS need the next image inserted while the VM runs. That already works through QEMU's own monitor (`View → compatmonitor0` in the GTK window, then `change floppy0 <img>` or `change ide1-cd0 <iso>`), but nothing in the app or docs said so. The Boot Options screen now shows the tip, and the README has a short section on it.
- **Fix RPM Installing a Headless QEMU** (thanks @Revali25, #98): The RPM required `qemu-system-x86-core`, which on Fedora is the bare emulator with no display, audio or device modules — so a fresh `dnf install` left VMs unable to open a window. It now requires `qemu-system-x86`, the package that pulls in the GTK/SDL/SPICE and audio modules, and both the RPM and DEB gain weak dependencies for the optional features (OVMF, swtpm, passt, virt-viewer, dnsmasq, samba, the non-x86 emulators; the DEB recommends `qemu-system-gui`). The Change Display screen now says which package to install when QEMU reports no windowed backend, and the README lists the per-distro packages.
- **QEMU Windows Titled After the VM** (thanks @jeffsauer, #96): Generated `launch.sh` scripts now pass `-name "$VM_NAME"` (the VM's directory name, derived by the script from its own location) on every invocation, so each VM's window is titled `QEMU (<vm-dir>)` instead of a bare `QEMU` — which lets window managers place or fullscreen specific VMs, e.g. a Hyprland rule on `title = "^QEMU \(tiny11pro\)$"`. Existing VMs are patched on their next launch from vm-curator; scripts that already carry their own `-name` are left alone.
- **Fix Haiku Installer Seeing No Disk** (thanks @jeffsauer, #93): The Haiku profile attached the system disk as virtio-blk, and Haiku beta 6's installer and DriveSetup don't enumerate it — the "Onto:" list was empty. The profile now uses AHCI (`if=ide` on q35, the same as the Windows profiles), which Haiku sees fine. VMs created from the old profile aren't rewritten: change `if=virtio` to `if=ide` on the `-drive file="$DISK"` lines in that VM's `launch.sh` (or re-create it).
- **Fix Single-GPU Start Script Giving Up While the Display Server Is Still Exiting** (thanks @ProgrammerManSudjo, #92; also #64, #75): `single-gpu-start.sh` stopped the display manager, slept two seconds, killed a fixed list of compositors, and tried the driver unload exactly once — so when SDDM's Xorg outlived `systemctl stop sddm` and still held `/dev/nvidia0`, the unload failed and the script aborted with "GPU is still bound to 'nvidia'". The script now waits for the display server processes to exit on their own (up to 10 s) before killing stragglers, sweeps every process holding a GPU device node — `/dev/dri/*`, `/dev/kfd`, and now `/dev/nvidia*` — with fuser, escalating from a wait to SIGTERM to SIGKILL, releases the GPU's HDMI-audio function, and retries the module unload up to five times before giving up. When it still fails it prints real diagnostics (module refcounts, a namespace-proof `/proc` scan of GPU file descriptors, framebuffer and virtual-console state) and offers an opt-in `VMC_APU_FORCE_UNBIND=1` device-unbind last resort for AMD APUs that keep an internal reference. Regenerate your scripts from the Single GPU Setup screen (`g`) to pick this up.
- **Fix GPU Shown Under the Wrong Name** (thanks @ProgrammerManSudjo, #91): An RTX 3060 (`10de:2504`) was listed as "RTX 3060 Ti" — a built-in name table had several wrong rows (that one, the RTX 4060/4060 Ti pair, the RTX 5080/5090 IDs, and the AMD RX 7000/9000 entries). Device names now come from the host's `pci.ids` database when present (`/usr/share/hwdata/pci.ids` or `/usr/share/misc/pci.ids`, the same source `lspci` uses, showing the bracketed marketing name), falling back to a corrected table verified against upstream `pci.ids`, then to the PCI class. Also fixes NVMe controllers being described as generic "Storage Controller".
- **Overhaul Multi-GPU Passthrough VFIO Binding** (thanks @travbp, #25): Launching a VM with PCI passthrough could hang silently forever after the auth prompt — reported on a hybrid NVIDIA + Intel MUX laptop. Unbinding a GPU that the compositor still holds blocks in the kernel indefinitely, and TUI launches discarded the script's output, so there was nothing to see and no way to recover short of killing the terminal.
  - Every unbind/remove sysfs write in the generated bind script is now wrapped in `timeout`, so a driver that won't let go produces a clear per-device error instead of an unkillable hang.
  - GPU drivers (nvidia/amdgpu/nouveau/radeon/i915/xe) get a compositor-release sequence before unbind: a fake udev "remove" event (works with Mutter/KWin/wlroots), `nvidia-persistenced` stop, and a PCI remove+rescan fallback with `driver_override` pre-set to vfio-pci if the unbind still times out. Restore reverses it: remove+rescan back to the original driver, udev "add" event so the compositor re-detects the card, and `nvidia-persistenced` restart. All still within a single auth prompt.
  - The bind/restore steps now mirror everything to `passthrough.log` in the VM directory, so diagnostics survive TUI launches; `VM_CURATOR_DEBUG=1 ./launch.sh` traces every sysfs write.
  - The script refuses to unbind the boot VGA device (the GPU rendering the current desktop) with a message pointing to Single GPU Passthrough instead of freezing the session.
  - Selecting a PCI device now auto-includes its IOMMU-group siblings (QEMU rejects partial groups as "not viable"); `a` fills in missing siblings, saving is blocked while a group is incomplete, and selections saved by older builds are completed on next open.
  - A `bash -n` regression test validates the generated bind script's syntax.

**v1.4.0**
- **Physical Disk Passthrough as Boot Device** (#80): Whole physical disks (NVMe, SATA/HDD, USB) can now be passed straight through to guests — both at creation time and on existing VMs — enabling near-native disk performance and booting existing bare-metal installs inside a VM.
  - The creation wizard's disk step gains a third mode, **Physical Disk**, with a dedicated picker that lists host disks (model, size, bus, stable `/dev/disk/by-id` path) and refuses — with the reason shown — the host system disk, anything mounted, swap members, and LVM/LUKS/RAID members. Selecting a disk requires an explicit typed confirmation naming the exact device; Enter and mouse clicks deliberately cancel.
  - New **Passthrough Disks** management screen attaches/detaches whole disks on existing VMs via a marker-managed `launch.sh` section (like USB/shared folders), with a per-disk firmware **boot index** and `-boot menu=on` while any passthrough disk is attached. The USB Passthrough screen gains the same boot-index option (`b`), finally making passed-through USB drives bootable.
  - Disks are attached as raw virtio-blk with `cache=none`, referenced by stable by-id paths. Generated scripts refuse to start if the disk is missing, not readable/writable (with `usermod -aG disk`/udev hints), or has mounted partitions.
  - Safety fixes underneath: parsed drives are now role-tagged (system / firmware / media), so qcow2 OVMF firmware (as shipped by Fedora) no longer makes a raw-disk VM look snapshot-capable — previously **Reset VM could delete the firmware image**. Reset and the existing-disk copy/move path now refuse block devices outright.
- **Virtual Network Manager** (#53): Create, edit, start/stop, and delete managed virtual networks from a new **Networks** screen (`n` on the main menu) — no more dropping to the CLI to build lab topologies.
  - Two types: **NAT** (outbound internet via a dedicated, cleanly-removable nftables table; iptables fallback) and **Isolated** (host-only, with explicit forward-drop rules so it stays isolated even if something else enabled IP forwarding). Subnet is configurable; gateway and DHCP range are derived. Optional DHCP runs dnsmasq in DHCP-only mode (no DNS — it cannot fight systemd-resolved or libvirt on port 53). `/etc/qemu/bridge.conf` ACL entries are added on start and removed on stop.
  - The TUI never modifies host networking itself: each network's `net-up.sh`/`net-down.sh` live under `~/.config/vm-curator/networks/<name>/`, are regenerated on every edit, and run with explicit sudo in a terminal — inspectable, and equally usable by hand or from boot scripts.
  - Managed networks appear in the per-VM Network Settings bridge picker (even while stopped), and launching a VM whose bridge doesn't exist now fails fast with a pointer to the Networks screen instead of a cryptic bridge-helper error. Coexists cleanly with libvirt (`vmc-` prefix, own dnsmasq and firewall table). Deferred to a future release: physical-NIC bridging, multi-NIC VMs, autostart.
- **Homebrew Tap for Atomic Distros** (#77): vm-curator is now installable with `brew install mroboff/tap/vm-curator` — aimed at immutable distros like Bluefin/Silverblue where Homebrew is the preferred CLI package manager. The formula (at [mroboff/homebrew-tap](https://github.com/mroboff/homebrew-tap)) installs the prebuilt release binary (needs only glibc ≥ 2.34 and libudev), is bumped automatically by the release workflow, and will grow a macOS block when the in-progress macOS port ships. QEMU itself is not bundled; `brew info vm-curator` covers the runtime notes.
- **Fix VMs Failing to Launch with a Relative Library Path** (#79): Entering a relative path like `VMs` at the first-run prompt stored it verbatim, so discovery depended on the directory vm-curator was started from — and launching always failed with `bash: VMs/<vm>/launch.sh: No such file or directory` (the launcher changes into the VM directory, double-resolving the relative path). Paths entered in the setup prompt and Settings now expand `~` and anchor bare relative paths at your home directory, a relative `--library` CLI flag resolves against the current directory at startup, and existing configs with relative or `~`-prefixed paths self-heal on load — affected users are fixed by upgrading.

**v1.3.0**
- Both features in this release come from [@HenriqueCrj](https://github.com/HenriqueCrj) — thank you!
- **Configurable VM Window Size** (thanks @HenriqueCrj, #67): New **Default Window Size** entry in Settings sets the initial display resolution for launched VMs (`WIDTHxHEIGHT`, e.g. `1440x900`; leave empty or enter `none` to keep each VM's own default). Sizes are validated to 320–16384 × 200–16384.
  - The size is passed to `launch.sh` via a `VM_CURATOR_WINDOW_SIZE` environment variable rather than baked into the script: the script's new video block applies it as `xres=`/`yres=` properties on the video device at runtime, so changing the setting affects the next launch of every VM without rewriting scripts, and launching a script by hand (no env var) behaves exactly as before.
  - Applies to vm-curator-generated scripts using standard VGA, `virtio-vga`, `virtio-vga-gl`, or QXL video. New VMs emit the override block at creation; existing supported scripts are patched idempotently on launch. Scripts with unsupported or hand-mixed video setups are deliberately left untouched.
- **Fix Shared Folder Argument Handling** (#66): Shared folders whose host path contains spaces or quotes produced a broken launch script — the 9p arguments were stored in a flat `SHARED_FOLDERS_ARGS="…"` string that the shell word-split at expansion. The managed section is now a properly quoted bash array expanded as `"${SHARED_FOLDERS_ARGS[@]}"`. Old-format scripts still parse, removal cleans up both forms, and single-GPU passthrough script extraction understands the array syntax too.
- **Fix USB Passthrough Being Passed as Script Options** (#66): Launching from the TUI with USB devices selected appended raw QEMU flags (`-usb -device usb-host,…`) as *shell arguments* to `launch.sh` — which the script's option parser does not understand — instead of relying on the USB section persisted inside the script. Launch now uses only the persisted section (runtime devices are ignored with a logged warning), so what you save with `s` on the USB screen is exactly what boots.
- **Fix UEFI Boot Delays from Stale NVRAM Boot Entries** (#66): OVMF preserves guest-created NVRAM boot entries, so a guest that once registered PXE/HTTP network boot could delay every subsequent boot while the firmware tried the network first. Normal UEFI disk boots now pin the disk first — `-global virtio-blk-pci.bootindex=0` with `-boot strict=on` for virtio disks, `-boot order=c,strict=on` otherwise. Install, CD-ROM, recovery, and floppy boots are unaffected (and the floppy branch now explicitly requests `-boot a`).
- **Keep Management Screens Open When Saving Fails** (#66): Choosing **Save** in the unsaved-changes prompt on the USB Passthrough, PCI Passthrough, or Shared Folders screens now keeps the screen open when the save fails (e.g. an unwritable `launch.sh`) instead of closing anyway and silently losing the selections.

**v1.2.3**
- **Hide-KVM Toggle for Single-GPU Passthrough — AMD Included** (#71, continuing #60): Windows guests on modern AMD cards (reported on an RX 9070 XT) black-screened or hit driver Code 43 even with a vBIOS ROM set, because AMD's RDNA3/4 Windows drivers — like NVIDIA's — misbehave when they detect a hypervisor. The generated single-GPU scripts already hid KVM from the guest (`-cpu host,kvm=off,hv_vendor_id=…` plus Hyper-V enlightenments), but only for NVIDIA GPUs.
  - The hypervisor-hiding CPU flags are now controlled by a per-VM **Hide KVM** setting: shown in the Single GPU Setup screen, toggled with `[h]`, persisted in `single-gpu-config.toml` alongside the vBIOS ROM, and **on by default for both NVIDIA and AMD** GPUs.
  - Configs written by earlier versions lack the setting and pick up the vendor default on load; regenerate the scripts with `[g]` to apply it. For AMD *Linux* guests the only cost of the new default is losing KVM paravirt niceties such as kvmclock (the guest still runs KVM-accelerated) — toggle it off if you prefer those.
- **Fix App Quitting When Typing 'q' in Settings** (#72): Setting a value containing the letter `q` (e.g. a VM Library Path like `~/qemu-vms`) in the Settings editor quit the program mid-keystroke. The global `q`-to-quit handler exempted every screen that accepts free text except the Settings screen's inline edit mode, so it consumed the key before the edit buffer saw it. `q`/`Q` now type normally while editing a value; `q` still quits when just browsing the settings list.

**v1.2.2**
- **Fix VM Launch Failure in Library Paths Containing Spaces** (#65): VMs created with v1.0.0–v1.2.1 failed to launch when the VM library path contained a space (e.g. `/mnt/Virtual SSD/vm-space`) — QEMU aborted with `-qmp unix:/mnt/Virtual: Failed to connect to '/mnt/Virtual': No such file or directory`. The generated `launch.sh` wrote the QMP monitor socket path unquoted — the only unquoted path interpolation in the script — so the shell word-split the argument at the space. VMs created with v0.4.10 or earlier were unaffected because they predate the QMP socket line.
  - Newly generated scripts quote the socket path (`unix:"$VM_DIR/qemu.sock"`), matching how `$DISK`, `$ISO`, and the other paths are already quoted.
  - Existing broken scripts self-heal: on launch, vm-curator rewrites the unquoted form in place (idempotently), so affected VMs work again without re-creating them or hand-editing `launch.sh`. This repair runs on the TUI launch path as well as CLI launches.

**v1.2.1**
- **Raw Disk Image Support** (thanks @HenriqueCrj, #55): The VM creation wizard's disk step now offers a qcow2/raw format toggle for new disks, with honest trade-off labels (raw disks don't support snapshots).
  - Existing and imported disks keep their real format: it is detected via `qemu-img info` (falling back to the file extension), the disk file is named to match (`vm.raw` vs `vm.qcow2`), and generated launch scripts emit the matching `format=` argument instead of hardcoding qcow2.
  - The disk file browser now also lists `.raw` and `.img` images.
- **Prevent Passed-Through GPUs Getting Stuck in D3cold** (#60): On modern AMD boot GPUs (reported on an RX 9070 XT), vfio-pci's runtime power management could idle the passed-through GPU into D3cold and fail to wake it — QEMU aborts with `vfio: Unable to power on device, stuck in D3`, and the host display cannot be restored without a reboot. The `/etc/modprobe.d/vfio.conf` written by System Setup now sets `options vfio_pci disable_vga=1 disable_idle_d3=1` (the standard fix; the only cost is slightly higher idle power on passed-through devices). Existing single-GPU users should re-run System Setup from Settings → Single GPU Passthrough to pick up the new option.
- **Fix Host Hang / Power-Off in Single-GPU Passthrough on APUs** (#61): Launching the generated `single-gpu-start.sh` could hard-hang or power the machine off entirely — reported on a Ryzen 7735U (Radeon 680M) mini PC. The script stopped the display manager and unloaded the GPU driver while fbcon was still rendering the active TTY through the GPU; the silent `modprobe -r` failure then led to force-unbinding an in-use driver, which AMD APUs in particular do not survive.
  - The start script now detaches the framebuffer virtual consoles (`/sys/class/vtconsole/*/bind`) and the generic EFI/simple framebuffer before unloading the GPU driver — the standard single-GPU passthrough sequence that was missing.
  - If the GPU driver still refuses to unload, the script aborts gracefully (cleanup restores the display) instead of force-unbinding an in-use driver.
  - Cleanup skips the PCI remove/rescan teardown when the GPU never left its original driver (the abort path), and both cleanup and `single-gpu-restore.sh` reattach the EFI framebuffer and virtual consoles so the TTY comes back.
  - Integrated GPUs (APUs) are now detected (best-effort, via a known AMD APU device-ID list plus Intel's fixed `00:02.0` iGPU address) and flagged with a prominent warning in the Single GPU Setup screen and the generated script header: APU passthrough is best-effort, and guest video requires a vBIOS extracted from that machine's own BIOS image.

**v1.2.0**
- **Security: bump `quick-xml` to 0.41** (RUSTSEC-2026-0194, RUSTSEC-2026-0195): resolves two high-severity advisories (quadratic-time duplicate-attribute check and unbounded namespace-declaration allocation) in the libvirt-XML import parser. Also bumps `anyhow` to 1.0.103.
- **Fix Windows 11 TPM 2.0 Detection on Fedora** (#42): Windows 11 VMs created with TPM enabled failed the installer's "PC must support TPM 2.0" check on Fedora 44, because vm-curator selected the 2M `OVMF_CODE.secboot.fd` firmware, which does not expose TPM 2.0 correctly.
  - OVMF firmware is now chosen as a matched CODE+VARS **pair** (`find_ovmf_firmware`), preferring 4M builds — including Fedora's qcow2-format firmware (`OVMF_CODE_4M.secboot.qcow2`) — over 2M, with the 2M variants kept only as a last-resort fallback. Picking CODE and VARS from one pair guarantees they always agree in size and on-disk format.
  - The pflash `-drive` lines now emit `format=qcow2` or `format=raw` to match the selected firmware instead of a hardcoded `format=raw`, so qcow2 firmware actually works. Applies to both the standard and single-GPU-passthrough launch scripts.
  - Generated launch scripts now create a per-user swtpm CA config (`swtpm_setup --create-config-files skip-if-exist`) so EK/platform certificate creation no longer needs write access to the system-wide `/var/lib/swtpm-localca`, and fall back to a certificate-less swtpm setup if cert creation still fails. The single-GPU-passthrough TPM setup was brought to parity (previously missing `--overwrite` and the certificate handling).
- **Warn Before Discarding Passthrough / Shared-Folder Changes** (#52): On the USB Passthrough, PCI Passthrough, and Shared Folders management screens, selections are saved with `s` while `Esc` exits — so users who toggled a device and pressed `Esc` (expecting it to persist) silently lost their changes, and the device was never passed through.
  - Leaving any of these three screens with unsaved changes now shows a three-way prompt — **Save (s) / Discard (d) / Cancel (Esc)** — instead of silently discarding. Choosing Save persists to `launch.sh` (so the change applies on next boot) and returns; Discard leaves without saving; Cancel stays on the screen.
  - Unsaved-change detection compares against the selection captured on entry (and refreshed after each save), so a previously saved but now-disconnected USB/PCI device does not register as a spurious change.
  - On the USB and PCI screens, `Space` is now the sole toggle and `Enter` saves (alongside `s`). Previously both `Space` and `Enter` toggled, so selecting a device with `Space` and pressing `Enter` to "confirm" toggled it back off — exactly the reporter's flow.
- **Fix Single-GPU Passthrough Script Bugs on AMD + NVIDIA** (#58): The generated single-GPU passthrough script contained two crash-inducing bugs.
  - It could include the primary host bridge (`0000:00:00.0`) — or another infrastructure device sharing the GPU's IOMMU group — in `EXTRA_PCI_ADDRS` and try to bind it to `vfio-pci`, which the kernel rejects with "Invalid argument", aborting the script. Infrastructure devices (host/PCI bridges, etc.) are now excluded from the PCI passthrough selection list and filtered out of `EXTRA_PCI_ADDRS` at generation time (the host bridge is dropped unconditionally as a safety net).
  - Even though single-GPU mode emits `-display none -vga none`, the extracted QEMU command still carried an emulated graphics device (e.g. `-device virtio-vga-gl` from the 3D-acceleration feature), so QEMU aborted on launch. The extractor now strips emulated graphics devices (`virtio-vga-gl`, `virtio-vga`, `qxl`, `VGA`, etc.) alongside the existing `-display`/`-vga` removal, since the passed-through physical GPU drives the display.
- **GPU vBIOS ROM support for Single-GPU Passthrough** (#44): AMD single-GPU passthrough commonly produces no video in the guest because the passed-through card has no clean vBIOS — the host already POSTed it. You can now point the Single GPU Setup screen at a vBIOS ROM file (`[r]` to set, `[R]` to clear); when set, the generated script passes the GPU with `romfile="…"`. The setting is persisted in the single-GPU config and shown in the setup screen.
  - The setup screen and generated script now warn that AMD GPUs usually need a vBIOS ROM to output video, and that integrated (APU) GPUs are often unsupported for passthrough.
  - Note: this enables the standard AMD fix but cannot guarantee success on all hardware — APU passthrough in particular may remain unsupported regardless of a supplied ROM.

**v1.1.0**
- **SPICE Clipboard Sharing** (#41): VMs using the `spice-app` display now support bidirectional host ⇄ guest copy/paste out of the box. vm-curator emitted `-display spice-app` but never the SPICE guest-agent channel that `spice-vdagent` talks over, so clipboard sharing silently did nothing even with the agent installed in the guest.
  - New VMs automatically emit the guest-agent channel (`-device virtio-serial-pci`, `-chardev spicevmc,id=spicechannel0,name=vdagent`, `-device virtserialport,...com.redhat.spice.0`) — the same args virt-manager adds by default
  - `SPICE_AGENT_ARGS` is the single source of truth; a new `set_spice_agent_args()` adds/removes the channel when toggling display backends on existing VMs
  - Requires `spice-vdagent` installed in the guest
- **Fix `-qmp` Crash in Single-GPU Passthrough** (#48): Single-GPU passthrough VMs created on v1.0.0 failed to launch with `-qmp -device: '-device' is not a valid char driver` after binding the GPU to `vfio-pci`, then dropped back to the desktop.
  - `extract_qemu_command_for_passthrough()` appended passthrough args by splitting at the command's last backslash and discarding the remainder. Since v1.0.0 appends the QMP socket as the final argument with its value on its own continuation line, this dropped the socket value and left `-qmp` dangling in front of `-device vfio-pci`
  - The same flawed logic also silently dropped the final argument (e.g. the network device) of any passthrough command
  - Replaced with `append_passthrough_args()`, which strips only a trailing continuation backslash and appends after the complete command, preserving the QMP value; added regression tests
- **Fix Stray AppImage Release Asset** (#46): The `build-appimage` CI job globbed `*.AppImage` when uploading, publishing the downloaded `appimagetool-x86_64.AppImage` alongside the real artifact. Scoped the upload glob to `vm-curator-*.AppImage` so only our AppImage ships.

**v1.0.0**
- **First stable release.** Many thanks to [@indyfive11](https://github.com/indyfive11) for the library-API contributions below! Beyond those, this release focuses on release-engineering and code-hardening rather than new end-user features; existing TUI behavior is unchanged.
- **Library target for GUI/external consumers** (thanks @indyfive11, #39): adds a `[lib]` target exposing the business-logic modules (`commands`, `config`, `fs`, `hardware`, `metadata`, `vm`, `wizard_types`) via `lib.rs`, with the `ui` (ratatui/crossterm) module intentionally excluded. Wizard/import state types were extracted into a front-end-agnostic `wizard_types` module. Also adds QMP-based VM control (pause/resume) and a D-Bus display launch path for GUI embedding.
- **Detect immediate QEMU startup failures in D-Bus launch** (thanks @indyfive11, #40): `launch_vm_dbus` now polls `try_wait()` after 300ms (the same pattern as `launch_vm_with_error_check`) so a QEMU process that exits instantly — no session bus, an unrecognized flag, a missing shared library — surfaces an error with QEMU's stderr instead of returning a PID a GUI consumer would wait on forever.
- **CI quality gates** (`.github/workflows/ci.yml`): every push to `main` and every pull request now runs `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test --locked`, and `cargo audit`. Previously nothing ran tests or lints before a release tag was cut.
- **Toolchain pin**: added `rust-toolchain.toml` (stable + clippy/rustfmt) so local, contributor, and CI builds agree; dropped the unenforced "Rust 1.70+" MSRV claim from the README.
- **Lint clean-up**: added `#![warn(clippy::all)]` and fixed all clippy findings under `-D warnings`; the whole codebase is now `cargo fmt`-clean.
- **Expanded test coverage**: new tests for `qemu-img` disk-format JSON parsing and for `Config` load/save (round-trip, partial/malformed TOML), via new path-parameterized `Config::load_from`/`save_to` helpers.
- **Import parser refactor**: the ~360-line libvirt XML parser was split into a small state struct with focused per-event/element helpers; behavior-preserving, with added regression tests.
- **Documentation**: crate-level public-API docs on the library root, module docs for the app state machine and UI dispatcher, and clarifying comments on intentional future-API `dead_code`; `cargo doc` is warning-free.

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
