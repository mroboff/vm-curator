use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::launch_parser::parse_launch_script;
use super::qemu_config::QemuConfig;

/// A discovered VM in the library
#[derive(Debug, Clone)]
pub struct DiscoveredVm {
    /// Directory name (e.g., "windows-95")
    pub id: String,
    /// Full path to VM directory
    pub path: PathBuf,
    /// Path to launch.sh
    pub launch_script: PathBuf,
    /// Parsed QEMU configuration
    pub config: QemuConfig,
    /// Whether the VM has been parsed successfully
    pub parse_success: bool,
    /// Parse error message if failed
    pub parse_error: Option<String>,
}

impl DiscoveredVm {
    /// Get a display name from the directory name with proper OS naming conventions
    pub fn display_name(&self) -> String {
        format_os_display_name(&self.id)
    }
}

/// Format an OS display name with proper naming conventions, trademarks, and publisher names
fn format_os_display_name(id: &str) -> String {
    let id_lower = id.to_lowercase();

    // Check for custom named VMs that should show OS + (custom name)
    if let Some(custom) = get_custom_name_mapping(&id_lower) {
        return custom;
    }

    // Microsoft Windows
    if id_lower.starts_with("windows-") || id_lower == "windows" {
        return format_windows_name(&id_lower);
    }

    // MS-DOS
    if id_lower == "ms-dos" || id_lower == "msdos" || id_lower == "dos" {
        return "Microsoft® MS-DOS".to_string();
    }

    // IBM OS/2
    if id_lower.starts_with("os2-") || id_lower.starts_with("os-2-") {
        return format_os2_name(&id_lower);
    }

    // Apple Macintosh
    if id_lower.starts_with("mac-") {
        return format_mac_name(&id_lower);
    }

    // Linux distributions
    if id_lower.starts_with("linux-") {
        return format_linux_name(&id_lower);
    }

    // BSD variants
    if id_lower.contains("bsd") {
        return format_bsd_name(&id_lower);
    }

    // Fallback: title case the ID
    fallback_title_case(id)
}

/// Get custom name mappings for special VMs
fn get_custom_name_mapping(id: &str) -> Option<String> {
    match id {
        "my-first-pc" => Some("Microsoft® MS-DOS / Windows 3.1 (My First PC)".to_string()),
        _ => None,
    }
}

/// Format Microsoft Windows names
fn format_windows_name(id: &str) -> String {
    let version = id.strip_prefix("windows-").unwrap_or(id);

    match version {
        "1" | "1-0" => "Microsoft® Windows 1.0".to_string(),
        "2" | "2-0" => "Microsoft® Windows 2.0".to_string(),
        "3" | "3-0" => "Microsoft® Windows 3.0".to_string(),
        "31" | "3-1" => "Microsoft® Windows 3.1".to_string(),
        "95" => "Microsoft® Windows 95".to_string(),
        "98" => "Microsoft® Windows 98".to_string(),
        "98se" | "98-se" => "Microsoft® Windows 98 SE".to_string(),
        "me" => "Microsoft® Windows Me".to_string(),
        "nt" | "nt4" | "nt-4" => "Microsoft® Windows NT 4.0".to_string(),
        "nt-31" | "nt31" => "Microsoft® Windows NT 3.1".to_string(),
        "nt-35" | "nt35" => "Microsoft® Windows NT 3.5".to_string(),
        "nt-351" | "nt351" => "Microsoft® Windows NT 3.51".to_string(),
        "2000" | "2k" => "Microsoft® Windows 2000".to_string(),
        "xp" => "Microsoft® Windows XP".to_string(),
        "vista" => "Microsoft® Windows Vista".to_string(),
        "7" => "Microsoft® Windows 7".to_string(),
        "8" => "Microsoft® Windows 8".to_string(),
        "81" | "8-1" => "Microsoft® Windows 8.1".to_string(),
        "10" => "Microsoft® Windows 10".to_string(),
        "11" => "Microsoft® Windows 11".to_string(),
        "server-2003" | "2003" => "Microsoft® Windows Server 2003".to_string(),
        "server-2008" | "2008" => "Microsoft® Windows Server 2008".to_string(),
        "server-2012" | "2012" => "Microsoft® Windows Server 2012".to_string(),
        "server-2016" | "2016" => "Microsoft® Windows Server 2016".to_string(),
        "server-2019" | "2019" => "Microsoft® Windows Server 2019".to_string(),
        "server-2022" | "2022" => "Microsoft® Windows Server 2022".to_string(),
        _ => format!("Microsoft® Windows {}", fallback_title_case(version)),
    }
}

/// Format IBM OS/2 names
fn format_os2_name(id: &str) -> String {
    let version = id.strip_prefix("os2-")
        .or_else(|| id.strip_prefix("os-2-"))
        .unwrap_or(id);

    match version {
        "warp-3" | "warp3" => "IBM® OS/2 Warp 3".to_string(),
        "warp-4" | "warp4" => "IBM® OS/2 Warp 4".to_string(),
        "warp" => "IBM® OS/2 Warp".to_string(),
        "1" | "10" => "IBM® OS/2 1.0".to_string(),
        "2" | "20" => "IBM® OS/2 2.0".to_string(),
        "21" | "2-1" => "IBM® OS/2 2.1".to_string(),
        "ecomstation" | "ecs" => "eComStation".to_string(),
        "arcaos" => "ArcaOS".to_string(),
        _ => format!("IBM® OS/2 {}", fallback_title_case(version)),
    }
}

/// Format Apple Macintosh names
fn format_mac_name(id: &str) -> String {
    let version = id.strip_prefix("mac-").unwrap_or(id);

    match version {
        "system6" | "system-6" => "Apple® Macintosh System 6".to_string(),
        "system7" | "system-7" => "Apple® Macintosh System 7".to_string(),
        "os8" | "os-8" => "Apple® Mac OS 8".to_string(),
        "os9" | "os-9" => "Apple® Mac OS 9".to_string(),
        "osx-cheetah" | "osx-10-0" => "Apple® Mac OS X 10.0 Cheetah".to_string(),
        "osx-puma" | "osx-10-1" => "Apple® Mac OS X 10.1 Puma".to_string(),
        "osx-jaguar" | "osx-10-2" => "Apple® Mac OS X 10.2 Jaguar".to_string(),
        "osx-panther" | "osx-10-3" => "Apple® Mac OS X 10.3 Panther".to_string(),
        "osx-tiger" | "osx-10-4" => "Apple® Mac OS X 10.4 Tiger".to_string(),
        "osx-leopard" | "osx-10-5" => "Apple® Mac OS X 10.5 Leopard".to_string(),
        "osx-snow-leopard" | "osx-10-6" => "Apple® Mac OS X 10.6 Snow Leopard".to_string(),
        "osx-lion" | "osx-10-7" => "Apple® Mac OS X 10.7 Lion".to_string(),
        "osx-mountain-lion" | "osx-10-8" => "Apple® Mac OS X 10.8 Mountain Lion".to_string(),
        "osx-mavericks" | "osx-10-9" => "Apple® Mac OS X 10.9 Mavericks".to_string(),
        "osx-yosemite" | "osx-10-10" => "Apple® Mac OS X 10.10 Yosemite".to_string(),
        "osx-el-capitan" | "osx-10-11" => "Apple® Mac OS X 10.11 El Capitan".to_string(),
        "macos-sierra" | "macos-10-12" => "Apple® macOS 10.12 Sierra".to_string(),
        "macos-high-sierra" | "macos-10-13" => "Apple® macOS 10.13 High Sierra".to_string(),
        "macos-mojave" | "macos-10-14" => "Apple® macOS 10.14 Mojave".to_string(),
        "macos-catalina" | "macos-10-15" => "Apple® macOS 10.15 Catalina".to_string(),
        _ => format!("Apple® Mac {}", fallback_title_case(version)),
    }
}

/// Format Linux distribution names
fn format_linux_name(id: &str) -> String {
    let distro = id.strip_prefix("linux-").unwrap_or(id);

    // Rolling release distributions
    match distro {
        "arch" => return "Arch Linux (rolling)".to_string(),
        "artix" => return "Artix Linux (rolling)".to_string(),
        "cachyos" => return "CachyOS (rolling)".to_string(),
        "endeavouros" | "endeavour" => return "EndeavourOS (rolling)".to_string(),
        "garuda" => return "Garuda Linux (rolling)".to_string(),
        "gentoo" => return "Gentoo Linux (rolling)".to_string(),
        "manjaro" => return "Manjaro Linux (rolling)".to_string(),
        "nixos" => return "NixOS (rolling)".to_string(),
        "opensuse-tumbleweed" | "suse-tumbleweed" | "tumbleweed" => {
            return "openSUSE Tumbleweed (rolling)".to_string()
        }
        "void" => return "Void Linux (rolling)".to_string(),
        _ => {}
    }

    // Versioned distributions - check for version numbers
    if distro.starts_with("fedora") {
        return format_versioned_distro(distro, "fedora", "Fedora Linux");
    }
    if distro.starts_with("ubuntu") {
        return format_versioned_distro(distro, "ubuntu", "Ubuntu");
    }
    if distro.starts_with("debian") {
        return format_versioned_distro(distro, "debian", "Debian GNU/Linux");
    }
    if distro.starts_with("mint") {
        return format_versioned_distro(distro, "mint", "Linux Mint");
    }
    if distro.starts_with("centos") {
        return format_versioned_distro(distro, "centos", "CentOS Linux");
    }
    if distro.starts_with("rhel") || distro.starts_with("redhat") {
        let prefix = if distro.starts_with("rhel") { "rhel" } else { "redhat" };
        return format_versioned_distro(distro, prefix, "Red Hat® Enterprise Linux");
    }
    if distro.starts_with("suse") || distro.starts_with("opensuse") {
        // Check for leap
        if distro.contains("leap") {
            return format_versioned_distro(distro, "opensuse-leap", "openSUSE Leap");
        }
        // Plain "suse" without version = openSUSE Tumbleweed (modern rolling)
        if distro == "suse" || distro == "opensuse" {
            return "openSUSE Tumbleweed (rolling)".to_string();
        }
        // Versioned SuSE = old SuSE Linux (e.g., SuSE Linux 7)
        let prefix = if distro.starts_with("opensuse") { "opensuse" } else { "suse" };
        return format_versioned_distro(distro, prefix, "SuSE Linux");
    }
    if distro.starts_with("slackware") {
        return format_versioned_distro(distro, "slackware", "Slackware Linux");
    }
    if distro.starts_with("alpine") {
        return format_versioned_distro(distro, "alpine", "Alpine Linux");
    }
    if distro.starts_with("elementary") {
        return format_versioned_distro(distro, "elementary", "elementary OS");
    }
    if distro.starts_with("pop") || distro.starts_with("popos") {
        let prefix = if distro.starts_with("popos") { "popos" } else { "pop" };
        return format_versioned_distro(distro, prefix, "Pop!_OS");
    }
    if distro.starts_with("zorin") {
        return format_versioned_distro(distro, "zorin", "Zorin OS");
    }
    if distro.starts_with("mx") {
        return format_versioned_distro(distro, "mx", "MX Linux");
    }
    if distro.starts_with("kali") {
        return format_versioned_distro(distro, "kali", "Kali Linux");
    }
    if distro.starts_with("rocky") {
        return format_versioned_distro(distro, "rocky", "Rocky Linux");
    }
    if distro.starts_with("alma") || distro.starts_with("almalinux") {
        let prefix = if distro.starts_with("almalinux") { "almalinux" } else { "alma" };
        return format_versioned_distro(distro, prefix, "AlmaLinux");
    }

    // Fallback for unknown Linux distros
    format!("Linux {}", fallback_title_case(distro))
}

/// Format a versioned distribution name
fn format_versioned_distro(full: &str, prefix: &str, display_name: &str) -> String {
    let version = full.strip_prefix(prefix)
        .map(|s| s.trim_start_matches('-').trim_start_matches('_'))
        .filter(|s| !s.is_empty());

    match version {
        Some(v) => format!("{} {}", display_name, v),
        None => display_name.to_string(),
    }
}

/// Format BSD variant names
fn format_bsd_name(id: &str) -> String {
    let id_lower = id.to_lowercase();

    if id_lower.contains("freebsd") {
        let version = id_lower.replace("freebsd", "").replace('-', " ").trim().to_string();
        if version.is_empty() {
            return "FreeBSD".to_string();
        }
        return format!("FreeBSD {}", version);
    }
    if id_lower.contains("openbsd") {
        let version = id_lower.replace("openbsd", "").replace('-', " ").trim().to_string();
        if version.is_empty() {
            return "OpenBSD".to_string();
        }
        return format!("OpenBSD {}", version);
    }
    if id_lower.contains("netbsd") {
        let version = id_lower.replace("netbsd", "").replace('-', " ").trim().to_string();
        if version.is_empty() {
            return "NetBSD".to_string();
        }
        return format!("NetBSD {}", version);
    }
    if id_lower.contains("dragonfly") {
        return "DragonFly BSD".to_string();
    }

    fallback_title_case(id)
}

/// Fallback title case conversion
fn fallback_title_case(s: &str) -> String {
    s.replace('-', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scan the VM library directory for VMs
pub fn discover_vms(library_path: &Path) -> Result<Vec<DiscoveredVm>> {
    let mut vms = Vec::new();

    if !library_path.exists() {
        return Ok(vms);
    }

    let entries = std::fs::read_dir(library_path)
        .with_context(|| format!("Failed to read VM library at {:?}", library_path))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let launch_script = path.join("launch.sh");
        if !launch_script.exists() {
            continue;
        }

        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Try to parse the launch script
        let script_content = std::fs::read_to_string(&launch_script)
            .unwrap_or_default();

        let (config, parse_success, parse_error) = match parse_launch_script(&launch_script, &script_content) {
            Ok(cfg) => (cfg, true, None),
            Err(e) => {
                let mut default_config = QemuConfig::default();
                default_config.raw_script = script_content;
                (default_config, false, Some(e.to_string()))
            }
        };

        vms.push(DiscoveredVm {
            id,
            path,
            launch_script,
            config,
            parse_success,
            parse_error,
        });
    }

    // Sort by display name
    vms.sort_by(|a, b| a.display_name().cmp(&b.display_name()));

    Ok(vms)
}

/// Group VMs by category (extracted from naming conventions)
pub fn group_vms_by_category(vms: &[DiscoveredVm]) -> Vec<(&'static str, Vec<&DiscoveredVm>)> {
    let mut windows: Vec<&DiscoveredVm> = Vec::new();
    let mut mac: Vec<&DiscoveredVm> = Vec::new();
    let mut linux: Vec<&DiscoveredVm> = Vec::new();
    let mut other: Vec<&DiscoveredVm> = Vec::new();

    for vm in vms {
        let id_lower = vm.id.to_lowercase();
        if id_lower.starts_with("windows") || id_lower.contains("dos") || id_lower.starts_with("my-first") {
            windows.push(vm);
        } else if id_lower.starts_with("mac") {
            mac.push(vm);
        } else if id_lower.starts_with("linux")
            || id_lower.contains("fedora")
            || id_lower.contains("ubuntu")
            || id_lower.contains("debian")
            || id_lower.contains("arch")
        {
            linux.push(vm);
        } else {
            other.push(vm);
        }
    }

    let mut groups = Vec::new();
    if !windows.is_empty() {
        groups.push(("Windows / DOS", windows));
    }
    if !mac.is_empty() {
        groups.push(("Macintosh", mac));
    }
    if !linux.is_empty() {
        groups.push(("Linux", linux));
    }
    if !other.is_empty() {
        groups.push(("Other", other));
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name() {
        let vm = DiscoveredVm {
            id: "windows-95".to_string(),
            path: PathBuf::from("/test"),
            launch_script: PathBuf::from("/test/launch.sh"),
            config: QemuConfig::default(),
            parse_success: true,
            parse_error: None,
        };
        assert_eq!(vm.display_name(), "Microsoft® Windows 95");
    }

    #[test]
    fn test_linux_display_names() {
        assert_eq!(format_os_display_name("linux-cachyos"), "CachyOS (rolling)");
        assert_eq!(format_os_display_name("linux-fedora-40"), "Fedora Linux 40");
        assert_eq!(format_os_display_name("linux-arch"), "Arch Linux (rolling)");
        assert_eq!(format_os_display_name("linux-ubuntu-2404"), "Ubuntu 2404");
        // SuSE naming
        assert_eq!(format_os_display_name("linux-suse"), "openSUSE Tumbleweed (rolling)");
        assert_eq!(format_os_display_name("linux-suse-7"), "SuSE Linux 7");
        assert_eq!(format_os_display_name("linux-opensuse-leap-15"), "openSUSE Leap 15");
    }

    #[test]
    fn test_os2_display_names() {
        assert_eq!(format_os_display_name("os2-warp-3"), "IBM® OS/2 Warp 3");
        assert_eq!(format_os_display_name("os2-warp-4"), "IBM® OS/2 Warp 4");
    }

    #[test]
    fn test_custom_names() {
        assert_eq!(
            format_os_display_name("my-first-pc"),
            "Microsoft® MS-DOS / Windows 3.1 (My First PC)"
        );
    }
}
