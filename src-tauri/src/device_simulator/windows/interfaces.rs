use std::{fmt, net::Ipv4Addr};

use serde::{Deserialize, Serialize};

const MAX_INTERFACE_ID_LENGTH: usize = 128;

/// Stable adapter identity persisted by the simulator settings.
///
/// Windows implementations should prefer the adapter GUID. Interface names,
/// display names, and interface indices are not stable enough to persist.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StableInterfaceId(String);

impl StableInterfaceId {
    pub fn from_adapter_guid(guid: &str) -> Result<Self, InterfaceIdentityError> {
        let normalized = guid.trim().trim_matches(['{', '}']).to_ascii_lowercase();
        if normalized.is_empty()
            || normalized.len() > MAX_INTERFACE_ID_LENGTH
            || !is_canonical_guid_body(&normalized)
        {
            return Err(InterfaceIdentityError::InvalidAdapterGuid);
        }
        Ok(Self(format!("guid:{normalized}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_canonical_guid_body(value: &str) -> bool {
    let expected_lengths = [8, 4, 4, 4, 12];
    let groups = value.split('-').collect::<Vec<_>>();
    groups.len() == expected_lengths.len()
        && groups
            .iter()
            .zip(expected_lengths)
            .all(|(group, expected)| {
                group.len() == expected
                    && group.chars().all(|character| character.is_ascii_hexdigit())
            })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceIdentityError {
    InvalidAdapterGuid,
}

impl fmt::Display for InterfaceIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAdapterGuid => {
                formatter.write_str("device_simulator.interface.invalid_adapter_guid")
            }
        }
    }
}

impl std::error::Error for InterfaceIdentityError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceIpv4Address {
    pub address: Ipv4Addr,
    pub prefix_len: u8,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkInterfaceInfo {
    pub id: StableInterfaceId,
    pub name: String,
    pub description: String,
    pub interface_index: u32,
    pub is_enabled: bool,
    pub is_up: bool,
    pub mac_address: Option<String>,
    pub ipv4_addresses: Vec<InterfaceIpv4Address>,
}

fn is_usable_alias_adapter(interface: &NetworkInterfaceInfo) -> bool {
    interface.is_enabled
        && interface.is_up
        && interface.mac_address.is_some()
        && !interface.ipv4_addresses.is_empty()
        && !interface
            .ipv4_addresses
            .iter()
            .all(|address| address.address.is_loopback())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceProviderError {
    UnsupportedPlatform,
    Native(String),
}

impl fmt::Display for InterfaceProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("device_simulator.interface.unsupported_platform")
            }
            Self::Native(details) => write!(
                formatter,
                "device_simulator.interface.native_error: {details}"
            ),
        }
    }
}

impl std::error::Error for InterfaceProviderError {}

/// Read-only boundary for the later Windows IP Helper implementation.
pub trait NetworkInterfaceProvider: Send + Sync {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterfaceInfo>, InterfaceProviderError>;
}

/// Safe placeholder used until the native adapter enumerator is connected.
#[derive(Debug, Default)]
pub struct UnsupportedNetworkInterfaceProvider;

impl NetworkInterfaceProvider for UnsupportedNetworkInterfaceProvider {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterfaceInfo>, InterfaceProviderError> {
        Err(InterfaceProviderError::UnsupportedPlatform)
    }
}

#[derive(Debug, Default)]
pub struct SystemNetworkInterfaceProvider;

impl NetworkInterfaceProvider for SystemNetworkInterfaceProvider {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterfaceInfo>, InterfaceProviderError> {
        list_system_interfaces()
    }
}

#[cfg(target_os = "windows")]
pub fn list_system_interfaces() -> Result<Vec<NetworkInterfaceInfo>, InterfaceProviderError> {
    use std::mem::{align_of, size_of};
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_ALL_INTERFACES, GAA_FLAG_INCLUDE_PREFIX,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::{IfOperStatusNotPresent, IfOperStatusUp};
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_INCLUDE_PREFIX | GAA_FLAG_INCLUDE_ALL_INTERFACES;
    let mut required_bytes = 0_u32;
    let first =
        unsafe { GetAdaptersAddresses(AF_INET.0 as u32, flags, None, None, &mut required_bytes) };
    if first != ERROR_BUFFER_OVERFLOW.0 && first != 0 {
        return Err(InterfaceProviderError::Native(format!(
            "GetAdaptersAddresses sizing failed with Win32 error {first}"
        )));
    }
    if required_bytes == 0 {
        return Ok(Vec::new());
    }

    // The API requires native structure alignment; a u64-backed allocation is
    // sufficient for IP_ADAPTER_ADDRESSES on supported Windows targets.
    debug_assert!(align_of::<u64>() >= align_of::<IP_ADAPTER_ADDRESSES_LH>());
    let word_count = (required_bytes as usize).div_ceil(size_of::<u64>());
    let mut storage = vec![0_u64; word_count];
    let head = storage.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    let mut actual_bytes = required_bytes;
    let status = unsafe {
        GetAdaptersAddresses(AF_INET.0 as u32, flags, None, Some(head), &mut actual_bytes)
    };
    if status != 0 {
        return Err(InterfaceProviderError::Native(format!(
            "GetAdaptersAddresses failed with Win32 error {status}"
        )));
    }

    let mut interfaces = Vec::new();
    let mut current = head;
    while !current.is_null() {
        let adapter = unsafe { &*current };
        let raw_adapter_name = unsafe { narrow_ptr_to_string(adapter.AdapterName.0) };
        let guid = extract_adapter_guid(&raw_adapter_name).ok_or_else(|| {
            InterfaceProviderError::Native(format!(
                "adapter identity is not a GUID: {raw_adapter_name}"
            ))
        })?;
        let id = StableInterfaceId::from_adapter_guid(&guid).map_err(|_| {
            InterfaceProviderError::Native(format!("adapter GUID could not be normalized: {guid}"))
        })?;
        let name = unsafe { wide_ptr_to_string(adapter.FriendlyName.0) };
        let description = unsafe { wide_ptr_to_string(adapter.Description.0) };
        let interface_index = unsafe { adapter.Anonymous1.Anonymous.IfIndex };
        let physical_length =
            (adapter.PhysicalAddressLength as usize).min(adapter.PhysicalAddress.len());
        let mac_address = (physical_length > 0).then(|| {
            adapter.PhysicalAddress[..physical_length]
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        });

        let mut ipv4_addresses = Vec::new();
        let mut unicast = adapter.FirstUnicastAddress;
        while !unicast.is_null() {
            let entry = unsafe { &*unicast };
            if !entry.Address.lpSockaddr.is_null()
                && entry.Address.iSockaddrLength as usize >= size_of::<SOCKADDR_IN>()
            {
                let socket = unsafe { &*(entry.Address.lpSockaddr.cast::<SOCKADDR_IN>()) };
                if socket.sin_family == AF_INET {
                    let bytes = unsafe { socket.sin_addr.S_un.S_un_b };
                    ipv4_addresses.push(InterfaceIpv4Address {
                        address: Ipv4Addr::new(bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4),
                        prefix_len: entry.OnLinkPrefixLength,
                        is_primary: ipv4_addresses.is_empty(),
                    });
                }
            }
            unicast = entry.Next;
        }

        let interface = NetworkInterfaceInfo {
            id,
            name: if name.is_empty() {
                raw_adapter_name.clone()
            } else {
                name
            },
            description,
            interface_index,
            is_enabled: adapter.OperStatus != IfOperStatusNotPresent,
            is_up: adapter.OperStatus == IfOperStatusUp,
            mac_address,
            ipv4_addresses,
        };

        // Windows can expose protocol/filter bindings as separate rows here.
        // Only operational L2 adapters can safely own the simulator IP aliases.
        if is_usable_alias_adapter(&interface) {
            interfaces.push(interface);
        }
        current = adapter.Next;
    }
    interfaces.sort_by(|left, right| {
        right
            .is_up
            .cmp(&left.is_up)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    Ok(interfaces)
}

#[cfg(not(target_os = "windows"))]
pub fn list_system_interfaces() -> Result<Vec<NetworkInterfaceInfo>, InterfaceProviderError> {
    Err(InterfaceProviderError::UnsupportedPlatform)
}

fn extract_adapter_guid(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            return Some(trimmed[start + 1..end].to_owned());
        }
    }
    let candidate = trimmed.strip_prefix("guid:").unwrap_or(trimmed);
    is_canonical_guid_body(candidate).then(|| candidate.to_owned())
}

#[cfg(target_os = "windows")]
unsafe fn wide_ptr_to_string(pointer: *mut u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0_usize;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
}

#[cfg(target_os = "windows")]
unsafe fn narrow_ptr_to_string(pointer: *mut u8) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0_usize;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(pointer, length) }).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_guid_is_normalized_to_a_stable_serialized_id() {
        let id = StableInterfaceId::from_adapter_guid(" {A0B1C2D3-1234-5678-90AB-010203040506} ")
            .unwrap();
        assert_eq!(id.as_str(), "guid:a0b1c2d3-1234-5678-90ab-010203040506");
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            format!("\"{}\"", id.as_str())
        );
    }

    #[test]
    fn display_name_cannot_be_used_as_a_persisted_identity() {
        assert_eq!(
            StableInterfaceId::from_adapter_guid("Ethernet 2"),
            Err(InterfaceIdentityError::InvalidAdapterGuid)
        );
        assert_eq!(
            StableInterfaceId::from_adapter_guid("{}"),
            Err(InterfaceIdentityError::InvalidAdapterGuid)
        );
        assert_eq!(
            StableInterfaceId::from_adapter_guid("abcd"),
            Err(InterfaceIdentityError::InvalidAdapterGuid)
        );
    }

    #[test]
    fn unsupported_provider_fails_without_touching_the_host() {
        assert_eq!(
            UnsupportedNetworkInterfaceProvider.list_interfaces(),
            Err(InterfaceProviderError::UnsupportedPlatform)
        );
    }

    #[test]
    fn adapter_guid_is_extracted_from_ip_helper_names_without_using_display_text() {
        assert_eq!(
            extract_adapter_guid("{A0B1C2D3-1234-5678-90AB-010203040506}"),
            Some("A0B1C2D3-1234-5678-90AB-010203040506".into())
        );
        assert_eq!(
            extract_adapter_guid(r"\\DEVICE\\TCPIP_{A0B1C2D3-1234-5678-90AB-010203040506}"),
            Some("A0B1C2D3-1234-5678-90AB-010203040506".into())
        );
        assert_eq!(extract_adapter_guid("Ethernet 2"), None);
    }

    #[test]
    fn alias_adapter_filter_rejects_filter_bindings_and_loopback() {
        let mut interface = NetworkInterfaceInfo {
            id: StableInterfaceId::from_adapter_guid("a0b1c2d3-1234-5678-90ab-010203040506")
                .unwrap(),
            name: "Ethernet".into(),
            description: "Physical adapter".into(),
            interface_index: 7,
            is_enabled: true,
            is_up: true,
            mac_address: Some("001122334455".into()),
            ipv4_addresses: vec![InterfaceIpv4Address {
                address: Ipv4Addr::new(192, 168, 1, 2),
                prefix_len: 24,
                is_primary: true,
            }],
        };
        assert!(is_usable_alias_adapter(&interface));

        interface.mac_address = None;
        assert!(!is_usable_alias_adapter(&interface));
        interface.mac_address = Some("001122334455".into());
        interface.ipv4_addresses[0].address = Ipv4Addr::LOCALHOST;
        assert!(!is_usable_alias_adapter(&interface));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn system_provider_performs_read_only_ip_helper_enumeration() {
        let interfaces = SystemNetworkInterfaceProvider
            .list_interfaces()
            .expect("Windows IP Helper interface enumeration should succeed");
        assert!(interfaces.iter().all(|interface| {
            interface.id.as_str().starts_with("guid:") && interface.interface_index > 0
        }));
    }
}
