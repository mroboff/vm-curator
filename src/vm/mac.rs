//! MAC address utilities.

use std::fs::File;
use std::io::Read;

/// Generate a random MAC address with QEMU's IANA-assigned OUI prefix
/// (52:54:00). Three random low-order bytes give 16M unique addresses,
/// which is plenty for any single host.
pub fn generate_random_mac() -> String {
    let mut bytes = [0u8; 3];
    if File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_err()
    {
        // /dev/urandom should never fail on Linux, but fall back to a
        // time-derived seed so we still produce *something* unique.
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        bytes[0] = (nanos & 0xff) as u8;
        bytes[1] = ((nanos >> 8) & 0xff) as u8;
        bytes[2] = ((nanos >> 16) & 0xff) as u8;
    }
    format!("52:54:00:{:02x}:{:02x}:{:02x}", bytes[0], bytes[1], bytes[2])
}

/// Validate a MAC address in canonical colon-separated hex form
/// (e.g., `52:54:00:12:34:56`). Accepts both upper and lower case.
pub fn is_valid_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return false;
    }
    parts.iter().all(|p| {
        p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn random_mac_has_qemu_oui_and_correct_format() {
        for _ in 0..100 {
            let mac = generate_random_mac();
            assert!(mac.starts_with("52:54:00:"), "wrong OUI: {}", mac);
            assert!(is_valid_mac(&mac), "not valid MAC: {}", mac);
            assert_eq!(mac.len(), 17);
        }
    }

    #[test]
    fn random_macs_are_distinct() {
        let macs: HashSet<String> = (0..200).map(|_| generate_random_mac()).collect();
        // 3 random bytes = 16M space; collisions in 200 draws are essentially impossible.
        assert!(macs.len() > 195, "too many collisions: {}", macs.len());
    }

    #[test]
    fn validator_accepts_canonical_macs() {
        assert!(is_valid_mac("52:54:00:12:34:56"));
        assert!(is_valid_mac("AA:BB:CC:DD:EE:FF"));
        assert!(is_valid_mac("aa:bb:cc:dd:ee:ff"));
        assert!(is_valid_mac("00:00:00:00:00:00"));
    }

    #[test]
    fn validator_rejects_malformed_macs() {
        assert!(!is_valid_mac(""));
        assert!(!is_valid_mac("52-54-00-12-34-56"));
        assert!(!is_valid_mac("52:54:00:12:34"));
        assert!(!is_valid_mac("52:54:00:12:34:56:78"));
        assert!(!is_valid_mac("ZZ:54:00:12:34:56"));
        assert!(!is_valid_mac("525:54:00:12:34:56"));
        assert!(!is_valid_mac("5:54:00:12:34:56"));
    }
}
