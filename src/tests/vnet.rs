use super::*;

fn lab_nat() -> VirtualNetwork {
    VirtualNetwork::with_defaults("lab", VNetKind::Nat, "192.168.150.0/24").unwrap()
}

#[test]
fn name_validation() {
    assert!(validate_name("lab").is_ok());
    assert!(validate_name("lab-2").is_ok());
    assert!(validate_name("a").is_ok());
    // 11 chars = max (vmc- prefix + 11 = 15 = IFNAMSIZ)
    assert!(validate_name("abcdefghijk").is_ok());
    assert!(validate_name("abcdefghijkl").is_err());
    assert!(validate_name("").is_err());
    assert!(validate_name("Lab").is_err());
    assert!(validate_name("2lab").is_err());
    assert!(validate_name("-lab").is_err());
    assert!(validate_name("lab net").is_err());
    assert!(validate_name("lab_net").is_err());
}

#[test]
fn defaults_derive_gateway_and_dhcp_range() {
    let net = lab_nat();
    assert_eq!(net.subnet, "192.168.150.0/24");
    assert_eq!(net.gateway, "192.168.150.1");
    assert!(net.dhcp);
    assert_eq!(net.dhcp_start, "192.168.150.50");
    assert_eq!(net.dhcp_end, "192.168.150.254");
    assert_eq!(net.bridge_name(), "vmc-lab");
}

#[test]
fn small_subnets_disable_dhcp_by_default() {
    let net = VirtualNetwork::with_defaults("tiny", VNetKind::Isolated, "10.0.0.0/30").unwrap();
    assert!(!net.dhcp);
}

#[test]
fn non_network_address_is_rejected_with_suggestion() {
    let err = VirtualNetwork::with_defaults("lab", VNetKind::Nat, "192.168.150.5/24")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("192.168.150.0/24"),
        "suggests the network: {err}"
    );
}

#[test]
fn subnet_validation_rejects_bad_input() {
    assert!(VirtualNetwork::with_defaults("x", VNetKind::Nat, "not-a-subnet").is_err());
    assert!(VirtualNetwork::with_defaults("x", VNetKind::Nat, "192.168.1.0").is_err());
    assert!(VirtualNetwork::with_defaults("x", VNetKind::Nat, "192.168.1.0/31").is_err());
    assert!(VirtualNetwork::with_defaults("x", VNetKind::Nat, "192.168.1.0/7").is_err());
}

#[test]
fn validate_catches_gateway_and_range_outside_subnet() {
    let mut net = lab_nat();
    net.gateway = "10.0.0.1".to_string();
    assert!(net.validate().is_err());

    let mut net = lab_nat();
    net.dhcp_start = "10.0.0.5".to_string();
    assert!(net.validate().is_err());

    let mut net = lab_nat();
    net.dhcp_start = "192.168.150.200".to_string();
    net.dhcp_end = "192.168.150.100".to_string();
    assert!(net.validate().is_err());
}

#[test]
fn save_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let nat = lab_nat();
    let isolated =
        VirtualNetwork::with_defaults("airgap", VNetKind::Isolated, "10.99.0.0/24").unwrap();

    save_network(dir.path(), &nat).unwrap();
    save_network(dir.path(), &isolated).unwrap();

    let loaded = load_networks(dir.path());
    assert_eq!(loaded, vec![isolated.clone(), nat.clone()]); // sorted by name

    // Scripts exist and are executable
    for name in ["lab", "airgap"] {
        for up in [true, false] {
            let path = script_path(dir.path(), name, up);
            assert!(path.exists(), "{} missing", path.display());
            let mode = std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(&path).unwrap().permissions(),
            );
            assert_eq!(mode & 0o111, 0o111, "{} not executable", path.display());
        }
    }

    delete_network(dir.path(), "lab").unwrap();
    assert_eq!(load_networks(dir.path()).len(), 1);
}

#[test]
fn is_active_checks_bridge_presence() {
    let sys = tempfile::tempdir().unwrap();
    let net = lab_nat();
    assert!(!net.is_active_at(sys.path()));
    std::fs::create_dir(sys.path().join("vmc-lab")).unwrap();
    assert!(net.is_active_at(sys.path()));
}

#[test]
fn nat_up_script_contents() {
    let script = generate_up_script(&lab_nat());
    assert!(script.contains("ip link add name \"$BRIDGE\" type bridge"));
    assert!(script.contains("BRIDGE=vmc-lab"));
    assert!(script.contains("ip addr add \"$GATEWAY/24\""));
    // ACL management
    assert!(script.contains("allow $BRIDGE"));
    assert!(script.contains("/etc/qemu/bridge.conf"));
    // DHCP-only dnsmasq: port 0 disables DNS so it can't fight resolved
    assert!(script.contains("--port=0"));
    assert!(script.contains("--dhcp-range=192.168.150.50,192.168.150.254,12h"));
    // NAT via a dedicated, removable nftables table (dash → underscore)
    assert!(script.contains("nft add table ip vmc_lab"));
    assert!(script.contains("masquerade"));
    assert!(script.contains("sysctl -q -w net.ipv4.ip_forward=1"));
    // iptables fallback carries a comment tag for precise later deletion
    assert!(script.contains("-m comment --comment vmc-lab -j MASQUERADE"));
    // root guard and idempotence
    assert!(script.contains("$EUID -ne 0"));
    assert!(script.contains("already up"));
}

#[test]
fn isolated_up_script_drops_forwarding_and_skips_nat() {
    let net = VirtualNetwork::with_defaults("airgap", VNetKind::Isolated, "10.99.0.0/24").unwrap();
    let script = generate_up_script(&net);
    assert!(script.contains("forward iifname \"$BRIDGE\" drop"));
    assert!(script.contains("forward oifname \"$BRIDGE\" drop"));
    assert!(!script.contains("masquerade"));
    assert!(!script.contains("ip_forward"));
}

#[test]
fn down_script_reverses_everything() {
    let script = generate_down_script(&lab_nat());
    assert!(script.contains("nft delete table ip vmc_lab"));
    assert!(script.contains("ip link delete \"$BRIDGE\" type bridge"));
    assert!(script.contains("kill \"$(cat \"/run/vmc-net-lab.pid\")\""));
    assert!(script.contains("sed -i \"\\%^allow $BRIDGE$%d\" /etc/qemu/bridge.conf"));
    // Best-effort teardown: no set -e
    assert!(script.contains("set -uo pipefail"));
    // iptables deletions must textually mirror the up-script additions
    // (iptables -D only matches an identical rule spec)
    let up = generate_up_script(&lab_nat());
    let rule = "-t nat -D POSTROUTING -s \"$SUBNET\" ! -o \"$BRIDGE\" -m comment --comment vmc-lab -j MASQUERADE";
    let added = rule.replace(" -D ", " -A ");
    assert!(up.contains(&added), "up script missing: {added}");
    assert!(script.contains(rule), "down script missing: {rule}");
}

#[test]
fn up_script_marks_bridge_and_taps_unmanaged_by_networkmanager() {
    // NetworkManager can silently deactivate a tap it doesn't know is
    // supposed to stay bridged, detaching it from the bridge with no error
    // anywhere. The bridge and any tap* device must be excluded up front.
    let script = generate_up_script(&lab_nat());
    assert!(script.contains("unmanaged-devices=interface-name:vmc-*;interface-name:tap*"));
    assert!(script.contains("nmcli device set \"$BRIDGE\" managed no"));
    // Idempotent: don't clobber the shared conf file on every network's up
    assert!(script.contains("if [[ ! -f \"$NM_CONF\" ]]"));
}

/// Scripts come out of format! templates; have bash verify the syntax.
#[test]
fn generated_scripts_are_valid_bash() {
    let dir = tempfile::tempdir().unwrap();
    for net in [
        lab_nat(),
        VirtualNetwork::with_defaults("airgap", VNetKind::Isolated, "10.99.0.0/24").unwrap(),
        {
            let mut n = lab_nat();
            n.name = "nodhcp".to_string();
            n.dhcp = false;
            n
        },
    ] {
        for (label, content) in [
            ("up", generate_up_script(&net)),
            ("down", generate_down_script(&net)),
        ] {
            let path = dir.path().join(format!("{}-{}.sh", net.name, label));
            std::fs::write(&path, &content).unwrap();
            let out = std::process::Command::new("bash")
                .arg("-n")
                .arg(&path)
                .output();
            let Ok(out) = out else {
                return; // bash unavailable — skip
            };
            assert!(
                out.status.success(),
                "{}-{} script has bash syntax errors:\n{}",
                net.name,
                label,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
