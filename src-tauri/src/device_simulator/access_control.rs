//! Application-layer source admission for a running simulator session.
//!
//! Managed Windows Firewall rules already scope inbound traffic to the resolved
//! platform addresses, but they are `Allow` rules: they only hold while the
//! firewall is enabled for the active profile and no broader rule exists. This
//! policy is the in-process equivalent, so a session configured for a single
//! platform stays invisible to every other platform on the same subnet
//! regardless of host firewall state.
//!
//! Only discovery and HTTP consult this policy. RTSP admission is unchanged.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crate::device_simulator::api::PlatformAccessMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPolicyError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for AccessPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AccessPolicyError {}

/// Resolved form of [`PlatformAccessMode`]: the mode plus the IPv4 addresses the
/// configured platform servers resolved to for the current applied
/// configuration. The Worker replaces this snapshot only through the explicit
/// save-and-apply flow, so an unrelated DNS change cannot widen a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformAccessPolicy {
    mode: PlatformAccessMode,
    /// Empty when the mode is `Open`; never empty otherwise.
    allowed: Arc<BTreeSet<Ipv4Addr>>,
}

impl Default for PlatformAccessPolicy {
    fn default() -> Self {
        Self::open()
    }
}

impl PlatformAccessPolicy {
    pub fn open() -> Self {
        Self {
            mode: PlatformAccessMode::Open,
            allowed: Arc::new(BTreeSet::new()),
        }
    }

    /// Admit only `platform_addresses`. Loopback stays admitted so local
    /// diagnostics and a platform co-located with the simulator keep working.
    pub fn configured_servers_only(
        platform_addresses: impl IntoIterator<Item = Ipv4Addr>,
    ) -> Result<Self, AccessPolicyError> {
        let allowed = platform_addresses.into_iter().collect::<BTreeSet<_>>();
        if allowed.is_empty() {
            // Failing closed here would block the platform the user configured,
            // which is indistinguishable from the tool being broken.
            return Err(AccessPolicyError {
                code: "device_simulator.access.allow_list_empty",
                message: "restricted platform access requires at least one resolved platform IPv4 address"
                    .into(),
            });
        }
        Ok(Self {
            mode: PlatformAccessMode::ConfiguredServersOnly,
            allowed: Arc::new(allowed),
        })
    }

    pub fn resolve(
        mode: PlatformAccessMode,
        platform_addresses: impl IntoIterator<Item = Ipv4Addr>,
    ) -> Result<Self, AccessPolicyError> {
        match mode {
            PlatformAccessMode::Open => Ok(Self::open()),
            PlatformAccessMode::ConfiguredServersOnly => {
                Self::configured_servers_only(platform_addresses)
            }
        }
    }

    pub fn mode(&self) -> PlatformAccessMode {
        self.mode
    }

    pub fn is_open(&self) -> bool {
        self.mode == PlatformAccessMode::Open
    }

    pub fn allowed_addresses(&self) -> Vec<Ipv4Addr> {
        self.allowed.iter().copied().collect()
    }

    pub fn permits(&self, peer: Ipv4Addr) -> bool {
        match self.mode {
            PlatformAccessMode::Open => true,
            PlatformAccessMode::ConfiguredServersOnly => {
                peer.is_loopback() || self.allowed.contains(&peer)
            }
        }
    }

    /// Socket variant for the HTTP listener. Device listeners bind IPv4, so an
    /// IPv6 peer can only appear as a v4-mapped address; anything else is not
    /// something the allow list can describe and is refused.
    pub fn permits_socket(&self, peer: SocketAddr) -> bool {
        match self.mode {
            PlatformAccessMode::Open => true,
            PlatformAccessMode::ConfiguredServersOnly => match peer.ip() {
                IpAddr::V4(address) => self.permits(address),
                IpAddr::V6(address) => address
                    .to_ipv4_mapped()
                    .is_some_and(|address| self.permits(address)),
            },
        }
    }

    /// Single-line summary for session logs.
    pub fn describe(&self) -> String {
        match self.mode {
            PlatformAccessMode::Open => "open".into(),
            PlatformAccessMode::ConfiguredServersOnly => format!(
                "configured_servers_only ({} address(es) plus loopback)",
                self.allowed.len()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(value: &str) -> Ipv4Addr {
        value.parse().unwrap()
    }

    #[test]
    fn open_policy_admits_every_source() {
        let policy = PlatformAccessPolicy::open();
        assert!(policy.is_open());
        assert!(policy.permits(address("203.0.113.7")));
        assert!(policy.permits_socket("[2001:db8::1]:5000".parse().unwrap()));
        assert!(policy.allowed_addresses().is_empty());
    }

    #[test]
    fn restricted_policy_admits_configured_servers_and_loopback_only() {
        let policy =
            PlatformAccessPolicy::configured_servers_only([address("192.0.2.10")]).unwrap();
        assert!(policy.permits(address("192.0.2.10")));
        assert!(policy.permits(address("127.0.0.1")));
        assert!(!policy.permits(address("192.0.2.11")));
        assert!(!policy.is_open());
    }

    #[test]
    fn restricted_policy_maps_v4_mapped_peers_and_refuses_native_ipv6() {
        let policy =
            PlatformAccessPolicy::configured_servers_only([address("192.0.2.10")]).unwrap();
        assert!(policy.permits_socket("[::ffff:192.0.2.10]:40000".parse().unwrap()));
        assert!(!policy.permits_socket("[::ffff:192.0.2.11]:40000".parse().unwrap()));
        assert!(!policy.permits_socket("[2001:db8::1]:40000".parse().unwrap()));
    }

    #[test]
    fn restricted_policy_rejects_an_empty_allow_list_instead_of_blocking_everything() {
        assert_eq!(
            PlatformAccessPolicy::configured_servers_only([])
                .unwrap_err()
                .code,
            "device_simulator.access.allow_list_empty"
        );
        assert_eq!(
            PlatformAccessPolicy::resolve(PlatformAccessMode::ConfiguredServersOnly, [])
                .unwrap_err()
                .code,
            "device_simulator.access.allow_list_empty"
        );
    }

    #[test]
    fn open_mode_resolution_ignores_the_address_list() {
        let policy =
            PlatformAccessPolicy::resolve(PlatformAccessMode::Open, [address("192.0.2.10")])
                .unwrap();
        assert!(policy.is_open());
        assert!(policy.permits(address("198.51.100.1")));
    }
}
