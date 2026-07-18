use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::Ipv4Addr,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ipv4Subnet {
    pub network: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub prefix_len: u8,
}

impl Ipv4Subnet {
    pub fn from_address(address: Ipv4Addr, prefix_len: u8) -> Result<Self, AddressPlanError> {
        if prefix_len > 30 {
            return Err(AddressPlanError::UnsupportedPrefix(prefix_len));
        }

        let address = u32::from(address);
        let host_bits = 32 - u32::from(prefix_len);
        let host_mask = if prefix_len == 0 {
            u32::MAX
        } else {
            (1_u32 << host_bits) - 1
        };
        let network = address & !host_mask;
        let broadcast = network | host_mask;
        Ok(Self {
            network: Ipv4Addr::from(network),
            broadcast: Ipv4Addr::from(broadcast),
            prefix_len,
        })
    }

    pub fn total_capacity(self) -> u64 {
        1_u64 << (32 - u32::from(self.prefix_len))
    }

    pub fn usable_capacity(self) -> u64 {
        self.total_capacity() - 2
    }

    pub fn first_usable(self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.network) + 1)
    }

    pub fn last_usable(self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.broadcast) - 1)
    }

    pub fn contains(self, address: Ipv4Addr) -> bool {
        let value = u32::from(address);
        value >= u32::from(self.network) && value <= u32::from(self.broadcast)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressPlan {
    pub subnet: Ipv4Subnet,
    pub start_ip: Ipv4Addr,
    pub end_ip: Ipv4Addr,
    pub remaining_usable_capacity: u64,
    pub addresses: Vec<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressPlanError {
    UnsupportedPrefix(u8),
    EmptyRequest,
    NetworkAddress(Ipv4Addr),
    BroadcastAddress(Ipv4Addr),
    OutsideSubnet {
        address: Ipv4Addr,
        subnet: Ipv4Subnet,
    },
    CrossesSubnet {
        start: Ipv4Addr,
        requested: u32,
        last_usable: Ipv4Addr,
    },
    DuplicateAddress {
        address: Ipv4Addr,
        first_index: usize,
        duplicate_index: usize,
    },
}

impl fmt::Display for AddressPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPrefix(prefix) => write!(
                formatter,
                "device_simulator.ip.unsupported_prefix: /{prefix}"
            ),
            Self::EmptyRequest => formatter.write_str("device_simulator.ip.empty_request"),
            Self::NetworkAddress(address) => write!(
                formatter,
                "device_simulator.ip.network_address: {address}"
            ),
            Self::BroadcastAddress(address) => write!(
                formatter,
                "device_simulator.ip.broadcast_address: {address}"
            ),
            Self::OutsideSubnet { address, subnet } => write!(
                formatter,
                "device_simulator.ip.outside_subnet: {address} is not in {}/{}",
                subnet.network, subnet.prefix_len
            ),
            Self::CrossesSubnet {
                start,
                requested,
                last_usable,
            } => write!(
                formatter,
                "device_simulator.ip.crosses_subnet: {requested} addresses from {start} exceed {last_usable}"
            ),
            Self::DuplicateAddress {
                address,
                first_index,
                duplicate_index,
            } => write!(
                formatter,
                "device_simulator.ip.duplicate_address: {address} at {first_index} and {duplicate_index}"
            ),
        }
    }
}

impl std::error::Error for AddressPlanError {}

pub fn plan_sequential_aliases(
    start_ip: Ipv4Addr,
    prefix_len: u8,
    count: u32,
) -> Result<AddressPlan, AddressPlanError> {
    if count == 0 {
        return Err(AddressPlanError::EmptyRequest);
    }

    let subnet = Ipv4Subnet::from_address(start_ip, prefix_len)?;
    reject_reserved_address(subnet, start_ip)?;

    let start = u64::from(u32::from(start_ip));
    let end = start
        .checked_add(u64::from(count) - 1)
        .ok_or(AddressPlanError::CrossesSubnet {
            start: start_ip,
            requested: count,
            last_usable: subnet.last_usable(),
        })?;
    if end > u64::from(u32::from(subnet.last_usable())) {
        return Err(AddressPlanError::CrossesSubnet {
            start: start_ip,
            requested: count,
            last_usable: subnet.last_usable(),
        });
    }

    let addresses = (start..=end)
        .map(|value| Ipv4Addr::from(value as u32))
        .collect::<Vec<_>>();
    validate_alias_set(subnet, &addresses)?;

    Ok(AddressPlan {
        subnet,
        start_ip,
        end_ip: *addresses.last().expect("count is non-zero"),
        remaining_usable_capacity: u64::from(u32::from(subnet.last_usable())) - end,
        addresses,
    })
}

pub fn validate_alias_set(
    subnet: Ipv4Subnet,
    addresses: &[Ipv4Addr],
) -> Result<(), AddressPlanError> {
    if addresses.is_empty() {
        return Err(AddressPlanError::EmptyRequest);
    }

    let mut first_indices = HashMap::new();
    for (index, address) in addresses.iter().copied().enumerate() {
        if !subnet.contains(address) {
            return Err(AddressPlanError::OutsideSubnet { address, subnet });
        }
        reject_reserved_address(subnet, address)?;
        if let Some(first_index) = first_indices.insert(address, index) {
            return Err(AddressPlanError::DuplicateAddress {
                address,
                first_index,
                duplicate_index: index,
            });
        }
    }
    Ok(())
}

fn reject_reserved_address(subnet: Ipv4Subnet, address: Ipv4Addr) -> Result<(), AddressPlanError> {
    if address == subnet.network {
        return Err(AddressPlanError::NetworkAddress(address));
    }
    if address == subnet.broadcast {
        return Err(AddressPlanError::BroadcastAddress(address));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictEvidenceKind {
    Local,
    Neighbor,
    Probe,
    Unknown,
}

impl ConflictEvidenceKind {
    fn priority(self) -> u8 {
        match self {
            Self::Local => 4,
            Self::Neighbor => 3,
            Self::Probe => 2,
            Self::Unknown => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictObservationResult {
    Occupied,
    Available,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictEvidence {
    pub address: Ipv4Addr,
    pub kind: ConflictEvidenceKind,
    pub result: ConflictObservationResult,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictVerdict {
    Conflict,
    Clear,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressConflictAssessment {
    pub address: Ipv4Addr,
    pub verdict: ConflictVerdict,
    pub strongest_evidence: ConflictEvidenceKind,
    pub evidence: Vec<ConflictEvidence>,
}

pub fn assess_address_conflict(
    address: Ipv4Addr,
    evidence: impl IntoIterator<Item = ConflictEvidence>,
) -> AddressConflictAssessment {
    let mut evidence = evidence
        .into_iter()
        .filter(|item| item.address == address)
        .collect::<Vec<_>>();
    evidence.sort_by_key(|item| std::cmp::Reverse(item.kind.priority()));

    let occupied = evidence
        .iter()
        .find(|item| item.result == ConflictObservationResult::Occupied);
    let (verdict, strongest_evidence) = if let Some(item) = occupied {
        (ConflictVerdict::Conflict, item.kind)
    } else if let Some(item) = evidence
        .iter()
        .find(|item| item.result == ConflictObservationResult::Available)
    {
        (ConflictVerdict::Clear, item.kind)
    } else {
        (
            ConflictVerdict::Unknown,
            evidence
                .first()
                .map(|item| item.kind)
                .unwrap_or(ConflictEvidenceKind::Unknown),
        )
    };

    AddressConflictAssessment {
        address,
        verdict,
        strongest_evidence,
        evidence,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpAliasBackendError {
    UnsupportedPlatform,
    Native(String),
}

impl fmt::Display for IpAliasBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("device_simulator.ip.unsupported_platform")
            }
            Self::Native(details) => {
                write!(formatter, "device_simulator.ip.native_error: {details}")
            }
        }
    }
}

impl std::error::Error for IpAliasBackendError {}

pub trait IpAliasBackend: Send + Sync {
    fn list_local_addresses(&self) -> Result<HashSet<Ipv4Addr>, IpAliasBackendError>;
    fn add_alias(
        &self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), IpAliasBackendError>;
    fn remove_alias(
        &self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), IpAliasBackendError>;
}

#[derive(Debug, Default)]
pub struct UnsupportedIpAliasBackend;

impl IpAliasBackend for UnsupportedIpAliasBackend {
    fn list_local_addresses(&self) -> Result<HashSet<Ipv4Addr>, IpAliasBackendError> {
        Err(IpAliasBackendError::UnsupportedPlatform)
    }

    fn add_alias(
        &self,
        _interface_id: &str,
        _address: Ipv4Addr,
        _prefix_len: u8,
    ) -> Result<(), IpAliasBackendError> {
        Err(IpAliasBackendError::UnsupportedPlatform)
    }

    fn remove_alias(
        &self,
        _interface_id: &str,
        _address: Ipv4Addr,
        _prefix_len: u8,
    ) -> Result<(), IpAliasBackendError> {
        Err(IpAliasBackendError::UnsupportedPlatform)
    }
}

#[derive(Debug, Default)]
pub struct SystemIpAliasBackend;

impl IpAliasBackend for SystemIpAliasBackend {
    fn list_local_addresses(&self) -> Result<HashSet<Ipv4Addr>, IpAliasBackendError> {
        list_system_local_addresses()
    }

    fn add_alias(
        &self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), IpAliasBackendError> {
        add_system_alias(interface_id, address, prefix_len)
    }

    fn remove_alias(
        &self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), IpAliasBackendError> {
        remove_system_alias(interface_id, address, prefix_len)
    }
}

pub fn list_system_local_addresses() -> Result<HashSet<Ipv4Addr>, IpAliasBackendError> {
    super::interfaces::list_system_interfaces()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .flat_map(|interface| interface.ipv4_addresses.into_iter())
                .map(|address| address.address)
                .collect()
        })
        .map_err(|error| IpAliasBackendError::Native(error.to_string()))
}

#[cfg(target_os = "windows")]
fn add_system_alias(
    interface_id: &str,
    address: Ipv4Addr,
    prefix_len: u8,
) -> Result<(), IpAliasBackendError> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::NetworkManagement::IpHelper::CreateUnicastIpAddressEntry;

    let row = build_unicast_row(interface_id, address, prefix_len)?;
    let status = unsafe { CreateUnicastIpAddressEntry(&row) };
    if status != ERROR_SUCCESS {
        return Err(IpAliasBackendError::Native(format!(
            "CreateUnicastIpAddressEntry({address}/{prefix_len}) failed with Win32 error {}",
            status.0
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn add_system_alias(
    _interface_id: &str,
    _address: Ipv4Addr,
    _prefix_len: u8,
) -> Result<(), IpAliasBackendError> {
    Err(IpAliasBackendError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
fn remove_system_alias(
    interface_id: &str,
    address: Ipv4Addr,
    prefix_len: u8,
) -> Result<(), IpAliasBackendError> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::NetworkManagement::IpHelper::DeleteUnicastIpAddressEntry;

    let row = build_unicast_row(interface_id, address, prefix_len)?;
    let status = unsafe { DeleteUnicastIpAddressEntry(&row) };
    if status != ERROR_SUCCESS {
        return Err(IpAliasBackendError::Native(format!(
            "DeleteUnicastIpAddressEntry({address}/{prefix_len}) failed with Win32 error {}",
            status.0
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn remove_system_alias(
    _interface_id: &str,
    _address: Ipv4Addr,
    _prefix_len: u8,
) -> Result<(), IpAliasBackendError> {
    Err(IpAliasBackendError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
fn build_unicast_row(
    interface_id: &str,
    address: Ipv4Addr,
    prefix_len: u8,
) -> Result<
    windows::Win32::NetworkManagement::IpHelper::MIB_UNICASTIPADDRESS_ROW,
    IpAliasBackendError,
> {
    use windows::Win32::Foundation::BOOLEAN;
    use windows::Win32::NetworkManagement::IpHelper::{
        InitializeUnicastIpAddressEntry, MIB_UNICASTIPADDRESS_ROW,
    };
    use windows::Win32::Networking::WinSock::{
        IpDadStatePreferred, IpPrefixOriginManual, IpSuffixOriginManual, AF_INET, IN_ADDR,
        IN_ADDR_0, IN_ADDR_0_0, SOCKADDR_IN, SOCKADDR_INET,
    };

    if prefix_len > 30 {
        return Err(IpAliasBackendError::Native(format!(
            "unsupported alias prefix /{prefix_len}"
        )));
    }
    let interface_index = super::interfaces::list_system_interfaces()
        .map_err(|error| IpAliasBackendError::Native(error.to_string()))?
        .into_iter()
        .find(|interface| interface.id.as_str() == interface_id)
        .map(|interface| interface.interface_index)
        .ok_or_else(|| {
            IpAliasBackendError::Native(format!("stable adapter id is not present: {interface_id}"))
        })?;

    let [s_b1, s_b2, s_b3, s_b4] = address.octets();
    let mut row = MIB_UNICASTIPADDRESS_ROW::default();
    unsafe { InitializeUnicastIpAddressEntry(&mut row) };
    row.InterfaceIndex = interface_index;
    row.Address = SOCKADDR_INET {
        Ipv4: SOCKADDR_IN {
            sin_family: AF_INET,
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_un_b: IN_ADDR_0_0 {
                        s_b1,
                        s_b2,
                        s_b3,
                        s_b4,
                    },
                },
            },
            ..Default::default()
        },
    };
    row.PrefixOrigin = IpPrefixOriginManual;
    row.SuffixOrigin = IpSuffixOriginManual;
    row.ValidLifetime = u32::MAX;
    row.PreferredLifetime = u32::MAX;
    row.OnLinkPrefixLength = prefix_len;
    // Simulator aliases must not become the preferred source address for the
    // user's unrelated outbound traffic.
    row.SkipAsSource = BOOLEAN(1);
    row.DadState = IpDadStatePreferred;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(value: &str) -> Ipv4Addr {
        value.parse().unwrap()
    }

    #[test]
    fn plans_non_slash_24_subnets_with_exact_capacity() {
        let plan = plan_sequential_aliases(ip("10.20.30.17"), 28, 10).unwrap();
        assert_eq!(plan.subnet.network, ip("10.20.30.16"));
        assert_eq!(plan.subnet.broadcast, ip("10.20.30.31"));
        assert_eq!(plan.subnet.usable_capacity(), 14);
        assert_eq!(plan.subnet.first_usable(), ip("10.20.30.17"));
        assert_eq!(plan.subnet.last_usable(), ip("10.20.30.30"));
        assert_eq!(plan.end_ip, ip("10.20.30.26"));
        assert_eq!(plan.remaining_usable_capacity, 4);
    }

    #[test]
    fn plans_slash_16_without_truncating_at_an_octet_boundary() {
        let plan = plan_sequential_aliases(ip("10.44.254.250"), 16, 12).unwrap();
        assert_eq!(plan.subnet.network, ip("10.44.0.0"));
        assert_eq!(plan.subnet.broadcast, ip("10.44.255.255"));
        assert_eq!(plan.end_ip, ip("10.44.255.5"));
    }

    #[test]
    fn rejects_network_broadcast_cross_boundary_and_empty_requests() {
        assert_eq!(
            plan_sequential_aliases(ip("192.168.1.0"), 24, 1),
            Err(AddressPlanError::NetworkAddress(ip("192.168.1.0")))
        );
        assert_eq!(
            plan_sequential_aliases(ip("192.168.1.255"), 24, 1),
            Err(AddressPlanError::BroadcastAddress(ip("192.168.1.255")))
        );
        assert!(matches!(
            plan_sequential_aliases(ip("172.16.4.14"), 28, 2),
            Err(AddressPlanError::CrossesSubnet { .. })
        ));
        assert_eq!(
            plan_sequential_aliases(ip("192.168.1.10"), 24, 0),
            Err(AddressPlanError::EmptyRequest)
        );
    }

    #[test]
    fn rejects_slash_31_and_slash_32_where_no_host_aliases_exist() {
        assert_eq!(
            Ipv4Subnet::from_address(ip("10.0.0.1"), 31),
            Err(AddressPlanError::UnsupportedPrefix(31))
        );
        assert_eq!(
            Ipv4Subnet::from_address(ip("10.0.0.1"), 32),
            Err(AddressPlanError::UnsupportedPrefix(32))
        );
    }

    #[test]
    fn validates_explicit_sets_for_duplicates_reserved_and_cross_subnet_addresses() {
        let subnet = Ipv4Subnet::from_address(ip("10.0.8.9"), 29).unwrap();
        assert!(matches!(
            validate_alias_set(subnet, &[ip("10.0.8.9"), ip("10.0.8.9")]),
            Err(AddressPlanError::DuplicateAddress {
                first_index: 0,
                duplicate_index: 1,
                ..
            })
        ));
        assert_eq!(
            validate_alias_set(subnet, &[ip("10.0.8.15")]),
            Err(AddressPlanError::BroadcastAddress(ip("10.0.8.15")))
        );
        assert!(matches!(
            validate_alias_set(subnet, &[ip("10.0.8.16")]),
            Err(AddressPlanError::OutsideSubnet { .. })
        ));
    }

    #[test]
    fn local_or_neighbor_conflict_cannot_be_cleared_by_a_probe() {
        let address = ip("192.168.50.10");
        let assessment = assess_address_conflict(
            address,
            [
                ConflictEvidence {
                    address,
                    kind: ConflictEvidenceKind::Probe,
                    result: ConflictObservationResult::Available,
                    details: None,
                },
                ConflictEvidence {
                    address,
                    kind: ConflictEvidenceKind::Neighbor,
                    result: ConflictObservationResult::Occupied,
                    details: Some("ARP cache contains a MAC".into()),
                },
            ],
        );
        assert_eq!(assessment.verdict, ConflictVerdict::Conflict);
        assert_eq!(
            assessment.strongest_evidence,
            ConflictEvidenceKind::Neighbor
        );

        let local = assess_address_conflict(
            address,
            [ConflictEvidence {
                address,
                kind: ConflictEvidenceKind::Local,
                result: ConflictObservationResult::Occupied,
                details: None,
            }],
        );
        assert_eq!(local.strongest_evidence, ConflictEvidenceKind::Local);
    }

    #[test]
    fn probe_clear_and_missing_evidence_remain_distinct() {
        let address = ip("192.168.50.11");
        let clear = assess_address_conflict(
            address,
            [ConflictEvidence {
                address,
                kind: ConflictEvidenceKind::Probe,
                result: ConflictObservationResult::Available,
                details: None,
            }],
        );
        assert_eq!(clear.verdict, ConflictVerdict::Clear);
        assert_eq!(clear.strongest_evidence, ConflictEvidenceKind::Probe);

        let unknown = assess_address_conflict(address, []);
        assert_eq!(unknown.verdict, ConflictVerdict::Unknown);
        assert_eq!(unknown.strongest_evidence, ConflictEvidenceKind::Unknown);
    }
}
