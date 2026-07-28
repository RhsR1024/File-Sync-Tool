//! Host-liveness primitives used by the ping scan.
//!
//! Windows builds call the IP Helper API directly instead of spawning
//! `ping.exe` per address. Process creation under the scan's concurrency
//! regularly outlasted the wrapper timeout, so live hosts were killed mid-probe
//! and reported offline. These entry points are blocking and are expected to be
//! driven from a blocking task.

use std::net::Ipv4Addr;

/// Outcome of a single ICMP echo request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcmpOutcome {
    /// The target itself answered.
    Reply { round_trip_ms: u32 },
    /// A router answered "unreachable" on the target's behalf. `ping.exe` exits
    /// 0 in this case, which is why shelling out also produced false positives.
    Unreachable,
    /// Nothing came back within the timeout.
    NoReply,
}

/// A local IPv4 address together with the prefix length it is on-link for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalPrefix {
    pub address: Ipv4Addr,
    pub prefix_len: u8,
}

impl LocalPrefix {
    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        if self.prefix_len == 0 || self.prefix_len > 32 {
            return false;
        }
        let mask = u32::MAX << (32 - self.prefix_len);
        (u32::from(self.address) & mask) == (u32::from(ip) & mask)
    }
}

/// ARP only answers for addresses on the same link, so callers must gate ARP
/// probes on this. Sending ARP off-link would resolve the gateway instead and
/// mark every scanned address as occupied.
pub fn is_on_link(ip: Ipv4Addr, prefixes: &[LocalPrefix]) -> bool {
    prefixes.iter().any(|prefix| prefix.contains(ip))
}

pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<Vec<_>>()
        .join(":")
}

/// Rejects the all-zero and broadcast addresses that the APIs hand back for
/// network and broadcast targets.
fn is_usable_mac(mac: &[u8; 6]) -> bool {
    mac.iter().any(|byte| *byte != 0) && mac.iter().any(|byte| *byte != 0xFF)
}

/// The neighbour table also carries the multicast groups this machine has
/// joined (224.0.0.0/4, mapped onto 01:00:5E:.. hardware addresses). Those are
/// group registrations, not occupants of an address, so they must not count as
/// live hosts.
fn is_host_neighbor(ip: Ipv4Addr) -> bool {
    !ip.is_multicast() && !ip.is_broadcast() && !ip.is_unspecified()
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{is_host_neighbor, is_usable_mac, IcmpOutcome, LocalPrefix};
    use std::net::Ipv4Addr;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::NO_ERROR;
    use windows::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetIpNetTable2, GetUnicastIpAddressTable, IcmpCloseHandle, IcmpCreateFile,
        IcmpSendEcho, SendARP, ICMP_ECHO_REPLY, IP_SUCCESS, MIB_IPNET_TABLE2,
        MIB_UNICASTIPADDRESS_TABLE,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, NlnsIncomplete, NlnsUnreachable};

    /// Same payload size `ping.exe` uses, so targets that filter on packet size
    /// behave identically for both.
    const ECHO_PAYLOAD: [u8; 32] = [b'a'; 32];

    /// IPv4 addresses are passed to the IP Helper API as an `in_addr`, which
    /// holds the four octets in wire order.
    fn as_in_addr(ip: Ipv4Addr) -> u32 {
        u32::from_ne_bytes(ip.octets())
    }

    fn from_in_addr(raw: u32) -> Ipv4Addr {
        Ipv4Addr::from(raw.to_ne_bytes())
    }

    pub fn icmp_echo(ip: Ipv4Addr, timeout_ms: u32) -> (IcmpOutcome, Duration) {
        // The reply buffer holds an ICMP_ECHO_REPLY, the echoed payload, and the
        // extra message space MSDN requires. Backing it with u64 keeps it
        // aligned for the reply struct, which contains a pointer.
        let capacity = std::mem::size_of::<ICMP_ECHO_REPLY>() + ECHO_PAYLOAD.len() + 8;
        let mut reply = vec![0u64; capacity.div_ceil(8)];

        let started = Instant::now();
        unsafe {
            let handle = match IcmpCreateFile() {
                Ok(handle) => handle,
                Err(_) => return (IcmpOutcome::NoReply, started.elapsed()),
            };

            let replies = IcmpSendEcho(
                handle,
                as_in_addr(ip),
                ECHO_PAYLOAD.as_ptr().cast(),
                ECHO_PAYLOAD.len() as u16,
                None,
                reply.as_mut_ptr().cast(),
                (reply.len() * std::mem::size_of::<u64>()) as u32,
                timeout_ms.max(1),
            );
            let elapsed = started.elapsed();
            let _ = IcmpCloseHandle(handle);

            if replies == 0 {
                return (IcmpOutcome::NoReply, elapsed);
            }

            let echo = &*reply.as_ptr().cast::<ICMP_ECHO_REPLY>();
            let outcome = if echo.Status == IP_SUCCESS {
                IcmpOutcome::Reply {
                    round_trip_ms: echo.RoundTripTime,
                }
            } else {
                IcmpOutcome::Unreachable
            };
            (outcome, elapsed)
        }
    }

    pub fn arp_resolve(ip: Ipv4Addr) -> (Option<[u8; 6]>, Duration) {
        // SendARP writes into a ULONG array, so back the buffer with u32 to get
        // the alignment it documents.
        let mut raw = [0u32; 2];
        let mut length = std::mem::size_of_val(&raw) as u32;

        let started = Instant::now();
        // A source of 0 lets the stack pick the interface that owns the link.
        let status = unsafe { SendARP(as_in_addr(ip), 0, raw.as_mut_ptr().cast(), &mut length) };
        let elapsed = started.elapsed();

        if status != NO_ERROR.0 || length < 6 {
            return (None, elapsed);
        }

        let bytes: [u8; 8] = unsafe { std::mem::transmute(raw) };
        let mac: [u8; 6] = match bytes[..6].try_into() {
            Ok(mac) => mac,
            Err(_) => return (None, elapsed),
        };
        (is_usable_mac(&mac).then_some(mac), elapsed)
    }

    pub fn local_prefixes() -> Vec<LocalPrefix> {
        let mut prefixes = Vec::new();

        unsafe {
            let mut table: *mut MIB_UNICASTIPADDRESS_TABLE = std::ptr::null_mut();
            if GetUnicastIpAddressTable(AF_INET, &mut table) != NO_ERROR || table.is_null() {
                return prefixes;
            }

            let rows =
                std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            for row in rows {
                if row.Address.si_family != AF_INET {
                    continue;
                }
                let address = from_in_addr(row.Address.Ipv4.sin_addr.S_un.S_addr);
                if address.is_loopback() || address.is_unspecified() || address.is_link_local() {
                    continue;
                }
                prefixes.push(LocalPrefix {
                    address,
                    prefix_len: row.OnLinkPrefixLength,
                });
            }

            FreeMibTable(table.cast());
        }

        prefixes
    }

    pub fn arp_cache_neighbors() -> Vec<(Ipv4Addr, [u8; 6])> {
        let mut neighbors = Vec::new();

        unsafe {
            let mut table: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
            if GetIpNetTable2(AF_INET, &mut table) != NO_ERROR || table.is_null() {
                return neighbors;
            }

            let rows =
                std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            for row in rows {
                // Unreachable and incomplete entries are failed lookups the
                // stack is still holding on to, not evidence of a neighbour.
                if row.State == NlnsUnreachable || row.State == NlnsIncomplete {
                    continue;
                }
                if row.Address.si_family != AF_INET || row.PhysicalAddressLength < 6 {
                    continue;
                }
                let mac: [u8; 6] = match row.PhysicalAddress[..6].try_into() {
                    Ok(mac) => mac,
                    Err(_) => continue,
                };
                if !is_usable_mac(&mac) {
                    continue;
                }
                let address = from_in_addr(row.Address.Ipv4.sin_addr.S_un.S_addr);
                if !is_host_neighbor(address) {
                    continue;
                }
                neighbors.push((address, mac));
            }

            FreeMibTable(table.cast());
        }

        neighbors
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::{IcmpOutcome, LocalPrefix};
    use std::net::Ipv4Addr;
    use std::time::Duration;

    pub fn icmp_echo(_ip: Ipv4Addr, _timeout_ms: u32) -> (IcmpOutcome, Duration) {
        (IcmpOutcome::NoReply, Duration::ZERO)
    }

    pub fn arp_resolve(_ip: Ipv4Addr) -> (Option<[u8; 6]>, Duration) {
        (None, Duration::ZERO)
    }

    pub fn local_prefixes() -> Vec<LocalPrefix> {
        Vec::new()
    }

    pub fn arp_cache_neighbors() -> Vec<(Ipv4Addr, [u8; 6])> {
        Vec::new()
    }
}

pub use platform::{arp_cache_neighbors, arp_resolve, icmp_echo, local_prefixes};

#[cfg(test)]
mod tests {
    use super::{format_mac, is_host_neighbor, is_on_link, is_usable_mac, LocalPrefix};
    use std::net::Ipv4Addr;

    fn prefix(address: &str, prefix_len: u8) -> LocalPrefix {
        LocalPrefix {
            address: address.parse().unwrap(),
            prefix_len,
        }
    }

    #[test]
    fn on_link_matches_only_the_local_subnet() {
        let prefixes = vec![prefix("192.168.1.10", 24)];

        assert!(is_on_link(Ipv4Addr::new(192, 168, 1, 200), &prefixes));
        assert!(!is_on_link(Ipv4Addr::new(192, 168, 2, 200), &prefixes));
        assert!(!is_on_link(Ipv4Addr::new(10, 0, 0, 1), &prefixes));
    }

    #[test]
    fn on_link_handles_host_and_degenerate_prefix_lengths() {
        assert!(is_on_link(
            Ipv4Addr::new(192, 168, 1, 10),
            &[prefix("192.168.1.10", 32)]
        ));
        assert!(!is_on_link(
            Ipv4Addr::new(192, 168, 1, 11),
            &[prefix("192.168.1.10", 32)]
        ));
        // A zero prefix would otherwise shift by 32 and match everything.
        assert!(!is_on_link(
            Ipv4Addr::new(8, 8, 8, 8),
            &[prefix("192.168.1.10", 0)]
        ));
    }

    #[test]
    fn on_link_covers_every_local_address_including_aliases() {
        let prefixes = vec![prefix("192.168.1.10", 24), prefix("10.20.30.5", 16)];

        assert!(is_on_link(Ipv4Addr::new(10, 20, 99, 4), &prefixes));
        assert!(!is_on_link(Ipv4Addr::new(10, 21, 30, 4), &prefixes));
    }

    #[test]
    fn unusable_macs_are_rejected() {
        assert!(is_usable_mac(&[0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]));
        assert!(!is_usable_mac(&[0x00; 6]));
        assert!(!is_usable_mac(&[0xFF; 6]));
    }

    #[test]
    fn multicast_group_registrations_are_not_hosts() {
        assert!(is_host_neighbor(Ipv4Addr::new(192, 168, 0, 51)));
        // Entries Windows keeps for joined groups, seen on a normal desktop.
        assert!(!is_host_neighbor(Ipv4Addr::new(224, 0, 0, 251)));
        assert!(!is_host_neighbor(Ipv4Addr::new(239, 255, 255, 250)));
        assert!(!is_host_neighbor(Ipv4Addr::BROADCAST));
        assert!(!is_host_neighbor(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn mac_is_formatted_as_uppercase_colon_pairs() {
        assert_eq!(
            format_mac(&[0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]),
            "00:1A:2B:3C:4D:5E"
        );
    }
}
