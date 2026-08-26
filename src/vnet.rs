//! Virtual Network Manager (issue #53).
//!
//! Managed virtual networks are Linux bridges vm-curator creates and owns,
//! with optional DHCP (dnsmasq, DHCP-only mode) and either NAT (outbound
//! internet via a dedicated nftables table, iptables fallback) or explicit
//! isolation (forward-drop rules).
//!
//! Each network lives in `~/.config/vm-curator/networks/<name>/` as a
//! `network.toml` definition plus generated `net-up.sh` / `net-down.sh`
//! scripts. The TUI never modifies host networking itself: the scripts are
//! inspectable and are run with explicit sudo in a terminal.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

/// Bridge name prefix for managed networks. Kernel interface names are
/// limited to 15 bytes, so with this 4-byte prefix names get 11 bytes.
pub const BRIDGE_PREFIX: &str = "vmc-";

/// Maximum managed-network name length (IFNAMSIZ 15 minus the prefix).
pub const MAX_NAME_LEN: usize = 15 - BRIDGE_PREFIX.len();

/// Network type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VNetKind {
    /// Outbound internet via masquerade; guests reach the world, not vice versa
    #[default]
    Nat,
    /// Host-only: guests talk to each other and the host, nothing is forwarded
    Isolated,
}

impl VNetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nat => "NAT",
            Self::Isolated => "Isolated",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Nat => Self::Isolated,
            Self::Isolated => Self::Nat,
        }
    }
}

/// A managed virtual network definition (persisted as network.toml)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualNetwork {
    pub name: String,
    pub kind: VNetKind,
    /// Subnet in CIDR form, e.g. "192.168.150.0/24"
    pub subnet: String,
    /// Gateway address assigned to the bridge (defaults to first host)
    pub gateway: String,
    /// Run a DHCP-only dnsmasq for the subnet
    pub dhcp: bool,
    pub dhcp_start: String,
    pub dhcp_end: String,
}

impl VirtualNetwork {
    /// Create a network with derived gateway/DHCP defaults for a subnet.
    pub fn with_defaults(name: &str, kind: VNetKind, subnet: &str) -> Result<Self> {
        validate_name(name)?;
        let s = parse_subnet(subnet)?;
        Ok(Self {
            name: name.to_string(),
            kind,
            subnet: format!("{}/{}", s.network, s.prefix),
            gateway: s.gateway().to_string(),
            dhcp: s.supports_dhcp(),
            dhcp_start: s.dhcp_start().to_string(),
            dhcp_end: s.dhcp_end().to_string(),
        })
    }

    /// Kernel bridge interface name, e.g. "vmc-lab"
    pub fn bridge_name(&self) -> String {
        format!("{}{}", BRIDGE_PREFIX, self.name)
    }

    /// nftables table name (dashes are not valid there), e.g. "vmc_lab"
    fn nft_table(&self) -> String {
        format!("vmc_{}", self.name.replace('-', "_"))
    }

    /// True while the network's bridge exists on the host.
    pub fn is_active(&self) -> bool {
        self.is_active_at(Path::new("/sys/class/net"))
    }

    pub fn is_active_at(&self, sys_class_net: &Path) -> bool {
        sys_class_net.join(self.bridge_name()).exists()
    }

    /// One-line description for pickers, e.g. "lab (NAT 192.168.150.0/24)"
    pub fn describe(&self) -> String {
        format!("{} ({} {})", self.name, self.kind.label(), self.subnet)
    }

    fn prefix_len(&self) -> u8 {
        self.subnet
            .rsplit('/')
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(24)
    }

    /// Validate the full definition (used before saving edits).
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        let s = parse_subnet(&self.subnet)?;
        let gw: Ipv4Addr = self
            .gateway
            .parse()
            .with_context(|| format!("Invalid gateway address: {}", self.gateway))?;
        if !s.contains(gw) {
            bail!("Gateway {} is outside subnet {}", self.gateway, self.subnet);
        }
        if self.dhcp {
            let start: Ipv4Addr = self
                .dhcp_start
                .parse()
                .with_context(|| format!("Invalid DHCP range start: {}", self.dhcp_start))?;
            let end: Ipv4Addr = self
                .dhcp_end
                .parse()
                .with_context(|| format!("Invalid DHCP range end: {}", self.dhcp_end))?;
            if !s.contains(start) || !s.contains(end) {
                bail!("DHCP range must be inside subnet {}", self.subnet);
            }
            if u32::from(start) > u32::from(end) {
                bail!("DHCP range start is after its end");
            }
        }
        Ok(())
    }
}

/// Managed-network names become part of a kernel interface name: short,
/// lowercase alphanumeric + dash, starting with a letter.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        bail!("Network name must be 1-{} characters", MAX_NAME_LEN);
    }
    if !name.chars().next().unwrap().is_ascii_lowercase() {
        bail!("Network name must start with a lowercase letter");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("Network name may only contain a-z, 0-9 and '-'");
    }
    Ok(())
}

/// Parsed IPv4 subnet with derived addresses
struct Subnet {
    network: Ipv4Addr,
    prefix: u8,
}

impl Subnet {
    fn size(&self) -> u32 {
        1u32 << (32 - self.prefix)
    }

    fn contains(&self, addr: Ipv4Addr) -> bool {
        let mask = u32::MAX << (32 - self.prefix);
        (u32::from(addr) & mask) == u32::from(self.network)
    }

    fn host(&self, offset: u32) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.network) + offset)
    }

    fn gateway(&self) -> Ipv4Addr {
        self.host(1)
    }

    /// DHCP needs headroom below the range for static assignments; require
    /// at least a /28 (14 hosts).
    fn supports_dhcp(&self) -> bool {
        self.prefix <= 28
    }

    fn dhcp_start(&self) -> Ipv4Addr {
        self.host((self.size() / 4).clamp(2, 50))
    }

    fn dhcp_end(&self) -> Ipv4Addr {
        // Last usable host is size-2 (size-1 is broadcast)
        self.host(self.size() - 2)
    }
}

fn parse_subnet(input: &str) -> Result<Subnet> {
    let (addr_str, prefix_str) = input
        .split_once('/')
        .with_context(|| format!("Subnet must be CIDR (e.g. 192.168.150.0/24): {}", input))?;
    let addr: Ipv4Addr = addr_str
        .parse()
        .with_context(|| format!("Invalid subnet address: {}", addr_str))?;
    let prefix: u8 = prefix_str
        .parse()
        .with_context(|| format!("Invalid prefix length: {}", prefix_str))?;
    if !(8..=30).contains(&prefix) {
        bail!("Prefix length must be between 8 and 30");
    }
    let mask = u32::MAX << (32 - prefix);
    let network = Ipv4Addr::from(u32::from(addr) & mask);
    if network != addr {
        bail!(
            "{} is not a network address; did you mean {}/{}?",
            input,
            network,
            prefix
        );
    }
    Ok(Subnet { network, prefix })
}

// ============================================================================
// Persistence
// ============================================================================

/// Directory holding managed network definitions.
pub fn networks_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("vm-curator")
        .join("networks")
}

/// Load all managed networks from `base_dir`, sorted by name.
pub fn load_networks(base_dir: &Path) -> Vec<VirtualNetwork> {
    let mut networks = Vec::new();
    let Ok(entries) = std::fs::read_dir(base_dir) else {
        return networks;
    };
    for entry in entries.flatten() {
        let toml_path = entry.path().join("network.toml");
        let Ok(content) = std::fs::read_to_string(&toml_path) else {
            continue;
        };
        match toml::from_str::<VirtualNetwork>(&content) {
            Ok(net) => networks.push(net),
            Err(_) => continue,
        }
    }
    networks.sort_by(|a, b| a.name.cmp(&b.name));
    networks
}

/// Save a network definition and (re)generate its up/down scripts.
pub fn save_network(base_dir: &Path, net: &VirtualNetwork) -> Result<()> {
    net.validate()?;
    let dir = base_dir.join(&net.name);
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;

    let toml_content =
        toml::to_string_pretty(net).context("Failed to serialize network definition")?;
    std::fs::write(dir.join("network.toml"), toml_content)
        .context("Failed to write network.toml")?;

    for (file, content) in [
        ("net-up.sh", generate_up_script(net)),
        ("net-down.sh", generate_down_script(net)),
    ] {
        let path = dir.join(file);
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .with_context(|| format!("Failed to chmod {}", path.display()))?;
    }
    Ok(())
}

/// Remove a network's directory. The caller must ensure it is not active.
pub fn delete_network(base_dir: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    let dir = base_dir.join(name);
    std::fs::remove_dir_all(&dir).with_context(|| format!("Failed to remove {}", dir.display()))?;
    Ok(())
}

/// Path to a network's up or down script.
pub fn script_path(base_dir: &Path, name: &str, up: bool) -> PathBuf {
    base_dir
        .join(name)
        .join(if up { "net-up.sh" } else { "net-down.sh" })
}

// ============================================================================
// Script generation
// ============================================================================

fn generate_up_script(net: &VirtualNetwork) -> String {
    let bridge = net.bridge_name();
    let table = net.nft_table();
    let prefix = net.prefix_len();

    let mut s = format!(
        r#"#!/bin/bash
# net-up.sh — start vm-curator managed network '{name}' ({kind} {subnet})
# Generated by vm-curator; regenerated on every edit. Safe to inspect.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "This script must run as root: sudo $0"
    exit 1
fi

BRIDGE={bridge}
SUBNET={subnet}
GATEWAY={gateway}

if [[ -e "/sys/class/net/$BRIDGE" ]]; then
    echo "Network '{name}' is already up ($BRIDGE exists)."
    exit 0
fi

echo "Creating bridge $BRIDGE ($SUBNET)..."
ip link add name "$BRIDGE" type bridge
ip addr add "$GATEWAY/{prefix}" dev "$BRIDGE"
ip link set "$BRIDGE" up

# Allow QEMU's bridge helper to attach VMs to this bridge
mkdir -p /etc/qemu
touch /etc/qemu/bridge.conf
grep -qxF "allow $BRIDGE" /etc/qemu/bridge.conf || echo "allow $BRIDGE" >> /etc/qemu/bridge.conf

# NetworkManager doesn't know this bridge or the per-VM taps QEMU attaches
# to it later, and can "helpfully" deactivate one — silently detaching it
# from the bridge with no error anywhere, breaking VM-to-VM connectivity.
# Tell it to leave vm-curator's bridges and taps alone, system-wide.
if command -v nmcli >/dev/null 2>&1; then
    mkdir -p /etc/NetworkManager/conf.d
    NM_CONF=/etc/NetworkManager/conf.d/vm-curator-unmanaged.conf
    if [[ ! -f "$NM_CONF" ]]; then
        cat > "$NM_CONF" <<'NMEOF'
# Written by vm-curator: never manage its virtual-network bridges or the
# per-VM tap devices QEMU attaches to them.
[keyfile]
unmanaged-devices=interface-name:vmc-*;interface-name:tap*
NMEOF
        nmcli general reload conf 2>/dev/null || systemctl reload NetworkManager 2>/dev/null || true
    fi
    nmcli device set "$BRIDGE" managed no 2>/dev/null || true
fi
"#,
        name = net.name,
        kind = net.kind.label(),
        subnet = net.subnet,
        bridge = bridge,
        gateway = net.gateway,
        prefix = prefix,
    );

    if net.dhcp {
        s.push_str(&format!(
            r#"
echo "Starting DHCP (dnsmasq, DHCP-only — no DNS) on $BRIDGE..."
if ! command -v dnsmasq >/dev/null 2>&1; then
    echo "WARNING: dnsmasq not installed; skipping DHCP. Guests need static IPs."
else
    dnsmasq --interface="$BRIDGE" --bind-interfaces --except-interface=lo \
        --port=0 --dhcp-range={start},{end},12h \
        --dhcp-leasefile="/run/vmc-net-{name}.leases" \
        --pid-file="/run/vmc-net-{name}.pid"
fi
"#,
            start = net.dhcp_start,
            end = net.dhcp_end,
            name = net.name,
        ));
    }

    match net.kind {
        VNetKind::Nat => {
            s.push_str(&format!(
                r#"
echo "Enabling NAT for $SUBNET..."
sysctl -q -w net.ipv4.ip_forward=1
if command -v nft >/dev/null 2>&1; then
    nft add table ip {table}
    nft "add chain ip {table} postrouting {{ type nat hook postrouting priority srcnat; policy accept; }}"
    nft add rule ip {table} postrouting ip saddr "$SUBNET" oifname != "$BRIDGE" masquerade
    nft "add chain ip {table} forward {{ type filter hook forward priority 0; policy accept; }}"
    nft add rule ip {table} forward iifname "$BRIDGE" accept
    nft add rule ip {table} forward oifname "$BRIDGE" ct state related,established accept
else
    iptables -t nat -A POSTROUTING -s "$SUBNET" ! -o "$BRIDGE" -m comment --comment {bridge} -j MASQUERADE
    iptables -A FORWARD -i "$BRIDGE" -m comment --comment {bridge} -j ACCEPT
    iptables -A FORWARD -o "$BRIDGE" -m state --state RELATED,ESTABLISHED -m comment --comment {bridge} -j ACCEPT
fi
"#,
                table = table,
                bridge = bridge,
            ));
        }
        VNetKind::Isolated => {
            s.push_str(&format!(
                r#"
echo "Applying isolation (nothing is forwarded in or out of $BRIDGE)..."
if command -v nft >/dev/null 2>&1; then
    nft add table ip {table}
    nft "add chain ip {table} forward {{ type filter hook forward priority -10; policy accept; }}"
    nft add rule ip {table} forward iifname "$BRIDGE" drop
    nft add rule ip {table} forward oifname "$BRIDGE" drop
else
    iptables -I FORWARD -i "$BRIDGE" -m comment --comment {bridge} -j DROP
    iptables -I FORWARD -o "$BRIDGE" -m comment --comment {bridge} -j DROP
fi
"#,
                table = table,
                bridge = bridge,
            ));
        }
    }

    s.push_str(&format!(
        "\necho \"Network '{}' is up. Attach VMs via the '{}' bridge.\"\n",
        net.name, bridge
    ));
    s
}

fn generate_down_script(net: &VirtualNetwork) -> String {
    let bridge = net.bridge_name();
    let table = net.nft_table();

    let mut s = format!(
        r#"#!/bin/bash
# net-down.sh — stop vm-curator managed network '{name}'
# Generated by vm-curator; regenerated on every edit. Safe to inspect.
# Teardown is best-effort: every step tolerates the previous run having
# partially failed, so this is safe to re-run.
set -uo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "This script must run as root: sudo $0"
    exit 1
fi

BRIDGE={bridge}
SUBNET={subnet}

echo "Stopping network '{name}'..."

# Stop the DHCP server if it is running
if [[ -f "/run/vmc-net-{name}.pid" ]]; then
    kill "$(cat "/run/vmc-net-{name}.pid")" 2>/dev/null || true
    rm -f "/run/vmc-net-{name}.pid"
fi

# Remove firewall rules (both backends attempted; comments scope the match)
if command -v nft >/dev/null 2>&1; then
    nft delete table ip {table} 2>/dev/null || true
fi
iptables -t nat -D POSTROUTING -s "$SUBNET" ! -o "$BRIDGE" -m comment --comment {bridge} -j MASQUERADE 2>/dev/null || true
iptables -D FORWARD -i "$BRIDGE" -m comment --comment {bridge} -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -o "$BRIDGE" -m state --state RELATED,ESTABLISHED -m comment --comment {bridge} -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -i "$BRIDGE" -m comment --comment {bridge} -j DROP 2>/dev/null || true
iptables -D FORWARD -o "$BRIDGE" -m comment --comment {bridge} -j DROP 2>/dev/null || true

# Tear down the bridge
ip link set "$BRIDGE" down 2>/dev/null || true
ip link delete "$BRIDGE" type bridge 2>/dev/null || true

# Remove the QEMU bridge-helper ACL entry
sed -i "\%^allow $BRIDGE$%d" /etc/qemu/bridge.conf 2>/dev/null || true

echo "Network '{name}' is down."
"#,
        name = net.name,
        bridge = bridge,
        subnet = net.subnet,
        table = table,
    );

    // Trailing newline is already present from the format literal.
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
#[path = "tests/vnet.rs"]
mod tests;
