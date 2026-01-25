use std::collections::HashMap;
use std::path::Path;

/// ASCII art storage and loading
#[derive(Debug, Clone, Default)]
pub struct AsciiArtStore {
    pub art: HashMap<String, String>,
}

impl AsciiArtStore {
    /// Load ASCII art from a directory (for user overrides)
    pub fn load_from_dir(dir: &Path) -> Self {
        let mut store = Self::default();

        if !dir.exists() {
            return store;
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "txt" || e == "ascii").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let id = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        store.art.insert(id, content);
                    }
                }
            }
        }

        store
    }

    /// Load embedded ASCII art from assets
    pub fn load_embedded() -> Self {
        let mut store = Self::default();

        // Windows logos
        store.art.insert("windows-31".to_string(), include_str!("../../assets/ascii/windows-31.txt").to_string());
        store.art.insert("windows-95".to_string(), include_str!("../../assets/ascii/windows-95.txt").to_string());
        store.art.insert("windows-98".to_string(), include_str!("../../assets/ascii/windows-98.txt").to_string());
        store.art.insert("windows-98se".to_string(), include_str!("../../assets/ascii/windows-98se.txt").to_string());
        store.art.insert("windows-me".to_string(), include_str!("../../assets/ascii/windows-me.txt").to_string());
        store.art.insert("windows-2000".to_string(), include_str!("../../assets/ascii/windows-2000.txt").to_string());
        store.art.insert("windows-xp".to_string(), include_str!("../../assets/ascii/windows-xp.txt").to_string());
        store.art.insert("windows-vista".to_string(), include_str!("../../assets/ascii/windows-vista.txt").to_string());
        store.art.insert("windows-7".to_string(), include_str!("../../assets/ascii/windows-7.txt").to_string());
        store.art.insert("windows-10".to_string(), include_str!("../../assets/ascii/windows-10.txt").to_string());
        store.art.insert("windows-11".to_string(), include_str!("../../assets/ascii/windows-11.txt").to_string());

        // DOS logos
        store.art.insert("ms-dos".to_string(), include_str!("../../assets/ascii/ms-dos.txt").to_string());
        store.art.insert("dos".to_string(), include_str!("../../assets/ascii/ms-dos.txt").to_string());
        store.art.insert("my-first-pc".to_string(), include_str!("../../assets/ascii/ms-dos.txt").to_string());

        // Mac logos
        store.art.insert("mac-system7".to_string(), include_str!("../../assets/ascii/mac-system7.txt").to_string());
        store.art.insert("mac-os9".to_string(), include_str!("../../assets/ascii/mac-os9.txt").to_string());
        store.art.insert("mac-osx".to_string(), include_str!("../../assets/ascii/mac-osx.txt").to_string());
        store.art.insert("mac-osx-tiger".to_string(), include_str!("../../assets/ascii/mac-osx-tiger.txt").to_string());
        store.art.insert("mac-osx-leopard".to_string(), include_str!("../../assets/ascii/mac-osx-leopard.txt").to_string());

        // Linux logos
        store.art.insert("linux".to_string(), include_str!("../../assets/ascii/linux.txt").to_string());
        store.art.insert("linux-fedora".to_string(), include_str!("../../assets/ascii/linux-fedora.txt").to_string());
        store.art.insert("linux-debian".to_string(), include_str!("../../assets/ascii/linux-debian.txt").to_string());
        store.art.insert("linux-ubuntu".to_string(), include_str!("../../assets/ascii/linux-ubuntu.txt").to_string());
        store.art.insert("linux-mint".to_string(), include_str!("../../assets/ascii/linux-mint.txt").to_string());
        store.art.insert("linux-pop".to_string(), include_str!("../../assets/ascii/linux-pop.txt").to_string());
        store.art.insert("linux-zorin".to_string(), include_str!("../../assets/ascii/linux-zorin.txt").to_string());
        store.art.insert("linux-cachyos".to_string(), include_str!("../../assets/ascii/linux-cachyos.txt").to_string());
        store.art.insert("linux-garuda".to_string(), include_str!("../../assets/ascii/linux-garuda.txt").to_string());
        store.art.insert("linux-bazzite".to_string(), include_str!("../../assets/ascii/linux-bazzite.txt").to_string());
        store.art.insert("nix-os".to_string(), include_str!("../../assets/ascii/nix-os.txt").to_string());

        // BSD logos
        store.art.insert("freebsd".to_string(), include_str!("../../assets/ascii/freebsd.txt").to_string());

        // Unix logos
        store.art.insert("solaris".to_string(), include_str!("../../assets/ascii/solaris.txt").to_string());

        // IBM / OS2 logos
        store.art.insert("os2-warp3".to_string(), include_str!("../../assets/ascii/os2-warp3.txt").to_string());
        store.art.insert("os2-warp4".to_string(), include_str!("../../assets/ascii/os2-warp4.txt").to_string());

        // Be / Haiku logos
        store.art.insert("beos".to_string(), include_str!("../../assets/ascii/beos.txt").to_string());
        store.art.insert("haiku".to_string(), include_str!("../../assets/ascii/haiku.txt").to_string());

        // NeXT logos
        store.art.insert("nextstep".to_string(), include_str!("../../assets/ascii/nextstep.txt").to_string());

        // Research OS logos
        store.art.insert("plan9".to_string(), include_str!("../../assets/ascii/plan9.txt").to_string());

        store
    }

    /// Get ASCII art for a VM ID
    pub fn get(&self, id: &str) -> Option<&str> {
        self.art.get(id).map(|s| s.as_str())
    }

    /// Get ASCII art or a fallback based on OS family
    pub fn get_or_fallback(&self, id: &str) -> &str {
        if let Some(art) = self.get(id) {
            return art;
        }

        // Try to find a fallback based on the name
        let id_lower = id.to_lowercase();
        if id_lower.contains("windows") {
            return WINDOWS_FALLBACK;
        } else if id_lower.contains("mac") || id_lower.contains("osx") {
            return MAC_FALLBACK;
        } else if id_lower.contains("linux") {
            return LINUX_FALLBACK;
        } else if id_lower.contains("dos") {
            return DOS_FALLBACK;
        } else if id_lower.contains("os2") || id_lower.contains("os-2") {
            return IBM_FALLBACK;
        }

        DEFAULT_FALLBACK
    }

    /// Merge user overrides
    pub fn merge(&mut self, overrides: AsciiArtStore) {
        for (id, art) in overrides.art {
            self.art.insert(id, art);
        }
    }
}

// Fallback ASCII art (loaded from assets at compile time)
const WINDOWS_FALLBACK: &str = include_str!("../../assets/ascii/_windows.txt");
const MAC_FALLBACK: &str = include_str!("../../assets/ascii/_mac.txt");
const LINUX_FALLBACK: &str = include_str!("../../assets/ascii/_linux.txt");
const DOS_FALLBACK: &str = include_str!("../../assets/ascii/_dos.txt");
const IBM_FALLBACK: &str = include_str!("../../assets/ascii/_ibm.txt");
const DEFAULT_FALLBACK: &str = include_str!("../../assets/ascii/_default.txt");
