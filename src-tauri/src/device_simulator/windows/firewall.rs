use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::ip_alias::Ipv4Subnet;

const RULE_NAME_PREFIX: &str = "FileSyncTool-DeviceSimulator";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallProtocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallDirection {
    Inbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallAction {
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FirewallRemoteScope {
    Addresses(Vec<Ipv4Addr>),
    SelectedSubnet(Ipv4Subnet),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirewallServiceIntent {
    /// Evidence-backed service key such as `http`, `rtsp-main`, or `discovery`.
    pub service_id: String,
    pub protocol: FirewallProtocol,
    pub local_ports: Vec<u16>,
    pub local_addresses: Vec<Ipv4Addr>,
    pub remote_scope: FirewallRemoteScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirewallRuleSpec {
    pub rule_id: String,
    pub name: String,
    pub group: String,
    pub session_id: String,
    pub service_id: String,
    pub program_path: PathBuf,
    pub direction: FirewallDirection,
    pub action: FirewallAction,
    pub protocol: FirewallProtocol,
    pub local_ports: Vec<u16>,
    pub local_addresses: Vec<Ipv4Addr>,
    pub remote_scope: FirewallRemoteScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirewallRulePlan {
    pub session_id: String,
    pub rules: Vec<FirewallRuleSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallPlanError {
    InvalidSessionId,
    InvalidServiceId(String),
    ProgramPathNotAbsolute,
    EmptyPorts(String),
    PortZero(String),
    DuplicatePort {
        service_id: String,
        port: u16,
    },
    EmptyLocalAddresses(String),
    DuplicateLocalAddress {
        service_id: String,
        address: Ipv4Addr,
    },
    EmptyRemoteAddresses(String),
    DuplicateRuleId(String),
}

impl fmt::Display for FirewallPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSessionId => {
                formatter.write_str("device_simulator.firewall.invalid_session_id")
            }
            Self::InvalidServiceId(id) => write!(
                formatter,
                "device_simulator.firewall.invalid_service_id: {id}"
            ),
            Self::ProgramPathNotAbsolute => {
                formatter.write_str("device_simulator.firewall.program_path_not_absolute")
            }
            Self::EmptyPorts(service) => write!(
                formatter,
                "device_simulator.firewall.empty_ports: {service}"
            ),
            Self::PortZero(service) => {
                write!(formatter, "device_simulator.firewall.port_zero: {service}")
            }
            Self::DuplicatePort { service_id, port } => write!(
                formatter,
                "device_simulator.firewall.duplicate_port: {service_id}/{port}"
            ),
            Self::EmptyLocalAddresses(service) => write!(
                formatter,
                "device_simulator.firewall.empty_local_addresses: {service}"
            ),
            Self::DuplicateLocalAddress {
                service_id,
                address,
            } => write!(
                formatter,
                "device_simulator.firewall.duplicate_local_address: {service_id}/{address}"
            ),
            Self::EmptyRemoteAddresses(service) => write!(
                formatter,
                "device_simulator.firewall.empty_remote_addresses: {service}"
            ),
            Self::DuplicateRuleId(id) => write!(
                formatter,
                "device_simulator.firewall.duplicate_rule_id: {id}"
            ),
        }
    }
}

impl std::error::Error for FirewallPlanError {}

pub fn plan_firewall_rules(
    session_id: &str,
    program_path: &Path,
    intents: Vec<FirewallServiceIntent>,
) -> Result<FirewallRulePlan, FirewallPlanError> {
    if !is_safe_token(session_id) {
        return Err(FirewallPlanError::InvalidSessionId);
    }
    if !program_path.is_absolute() {
        return Err(FirewallPlanError::ProgramPathNotAbsolute);
    }

    let mut rule_ids = HashSet::new();
    let mut rules = Vec::with_capacity(intents.len());
    for (index, mut intent) in intents.into_iter().enumerate() {
        if !is_safe_token(&intent.service_id) {
            return Err(FirewallPlanError::InvalidServiceId(intent.service_id));
        }
        validate_intent(&intent)?;

        intent.local_ports.sort_unstable();
        intent.local_addresses.sort_unstable();
        if let FirewallRemoteScope::Addresses(addresses) = &mut intent.remote_scope {
            addresses.sort_unstable();
        }

        let rule_id = format!(
            "device-simulator:{session_id}:{}:{index}",
            intent.service_id
        );
        if !rule_ids.insert(rule_id.clone()) {
            return Err(FirewallPlanError::DuplicateRuleId(rule_id));
        }
        let name = format!(
            "{RULE_NAME_PREFIX}-{session_id}-{}-{index}",
            intent.service_id
        );
        rules.push(FirewallRuleSpec {
            rule_id,
            name,
            group: RULE_NAME_PREFIX.to_string(),
            session_id: session_id.to_string(),
            service_id: intent.service_id,
            program_path: program_path.to_path_buf(),
            direction: FirewallDirection::Inbound,
            action: FirewallAction::Allow,
            protocol: intent.protocol,
            local_ports: intent.local_ports,
            local_addresses: intent.local_addresses,
            remote_scope: intent.remote_scope,
        });
    }

    Ok(FirewallRulePlan {
        session_id: session_id.to_string(),
        rules,
    })
}

fn validate_intent(intent: &FirewallServiceIntent) -> Result<(), FirewallPlanError> {
    if intent.local_ports.is_empty() {
        return Err(FirewallPlanError::EmptyPorts(intent.service_id.clone()));
    }
    let mut ports = HashSet::new();
    for port in intent.local_ports.iter().copied() {
        if port == 0 {
            return Err(FirewallPlanError::PortZero(intent.service_id.clone()));
        }
        if !ports.insert(port) {
            return Err(FirewallPlanError::DuplicatePort {
                service_id: intent.service_id.clone(),
                port,
            });
        }
    }

    if intent.local_addresses.is_empty() {
        return Err(FirewallPlanError::EmptyLocalAddresses(
            intent.service_id.clone(),
        ));
    }
    let mut local_addresses = HashSet::new();
    for address in intent.local_addresses.iter().copied() {
        if !local_addresses.insert(address) {
            return Err(FirewallPlanError::DuplicateLocalAddress {
                service_id: intent.service_id.clone(),
                address,
            });
        }
    }

    if matches!(
        &intent.remote_scope,
        FirewallRemoteScope::Addresses(addresses) if addresses.is_empty()
    ) {
        return Err(FirewallPlanError::EmptyRemoteAddresses(
            intent.service_id.clone(),
        ));
    }
    Ok(())
}

fn is_safe_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFirewallJournal {
    pub session_id: String,
    pub rules: Vec<FirewallRuleSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallRetentionReason {
    NotJournaled,
    ForeignSession,
    ProgramPathMismatch,
    RuleChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedFirewallRule {
    pub rule_id: String,
    pub reason: FirewallRetentionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirewallCleanupPlan {
    pub session_id: String,
    pub delete_rule_ids: Vec<String>,
    pub retained_rules: Vec<RetainedFirewallRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallOwnershipError {
    JournalSessionMismatch,
    DuplicateJournalRule(String),
}

pub fn plan_session_firewall_cleanup(
    session_id: &str,
    expected_program_path: &Path,
    journal: &SessionFirewallJournal,
    observed_rules: &[FirewallRuleSpec],
) -> Result<FirewallCleanupPlan, FirewallOwnershipError> {
    if journal.session_id != session_id {
        return Err(FirewallOwnershipError::JournalSessionMismatch);
    }

    let mut journal_by_id = HashMap::new();
    for record in &journal.rules {
        if journal_by_id
            .insert(record.rule_id.as_str(), record)
            .is_some()
        {
            return Err(FirewallOwnershipError::DuplicateJournalRule(
                record.rule_id.clone(),
            ));
        }
    }

    let mut delete_rule_ids = Vec::new();
    let mut retained_rules = Vec::new();
    for observed in observed_rules {
        let Some(recorded) = journal_by_id.get(observed.rule_id.as_str()) else {
            retained_rules.push(RetainedFirewallRule {
                rule_id: observed.rule_id.clone(),
                reason: FirewallRetentionReason::NotJournaled,
            });
            continue;
        };

        let reason = if recorded.session_id != session_id || observed.session_id != session_id {
            Some(FirewallRetentionReason::ForeignSession)
        } else if recorded.program_path != expected_program_path
            || observed.program_path != expected_program_path
        {
            Some(FirewallRetentionReason::ProgramPathMismatch)
        } else if *recorded != observed {
            Some(FirewallRetentionReason::RuleChanged)
        } else {
            None
        };

        if let Some(reason) = reason {
            retained_rules.push(RetainedFirewallRule {
                rule_id: observed.rule_id.clone(),
                reason,
            });
        } else {
            delete_rule_ids.push(observed.rule_id.clone());
        }
    }

    Ok(FirewallCleanupPlan {
        session_id: session_id.to_string(),
        delete_rule_ids,
        retained_rules,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallBackendError {
    UnsupportedPlatform,
    Native(String),
}

impl fmt::Display for FirewallBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("device_simulator.firewall.unsupported_platform")
            }
            Self::Native(details) => {
                write!(
                    formatter,
                    "device_simulator.firewall.native_error: {details}"
                )
            }
        }
    }
}

impl std::error::Error for FirewallBackendError {}

/// Mutation boundary for a future elevated Windows Firewall COM backend.
pub trait FirewallBackend: Send + Sync {
    fn list_managed_rules(&self) -> Result<Vec<FirewallRuleSpec>, FirewallBackendError>;
    fn create_rule(&self, rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError>;
    /// Replace the remote scope of an owned rule without changing its stable
    /// identity. Implementations must refuse any other field change.
    fn update_rule(&self, rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError>;
    fn delete_rule(&self, rule_id: &str) -> Result<(), FirewallBackendError>;
}

#[derive(Debug, Default)]
pub struct UnsupportedFirewallBackend;

impl FirewallBackend for UnsupportedFirewallBackend {
    fn list_managed_rules(&self) -> Result<Vec<FirewallRuleSpec>, FirewallBackendError> {
        Err(FirewallBackendError::UnsupportedPlatform)
    }

    fn create_rule(&self, _rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
        Err(FirewallBackendError::UnsupportedPlatform)
    }

    fn update_rule(&self, _rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
        Err(FirewallBackendError::UnsupportedPlatform)
    }

    fn delete_rule(&self, _rule_id: &str) -> Result<(), FirewallBackendError> {
        Err(FirewallBackendError::UnsupportedPlatform)
    }
}

#[derive(Debug, Default)]
pub struct SystemFirewallBackend;

impl FirewallBackend for SystemFirewallBackend {
    fn list_managed_rules(&self) -> Result<Vec<FirewallRuleSpec>, FirewallBackendError> {
        list_system_managed_rules()
    }

    fn create_rule(&self, rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
        create_system_rule(rule)
    }

    fn update_rule(&self, rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
        update_system_rule(rule)
    }

    fn delete_rule(&self, rule_id: &str) -> Result<(), FirewallBackendError> {
        delete_system_rule(rule_id)
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FirewallRuleMetadata {
    version: u8,
    rule_id: String,
    session_id: String,
    service_id: String,
}

#[cfg(target_os = "windows")]
struct ComApartment;

#[cfg(target_os = "windows")]
impl ComApartment {
    fn initialize() -> Result<Self, FirewallBackendError> {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|source| {
                FirewallBackendError::Native(format!(
                    "could not initialize Windows Firewall COM apartment: {source}"
                ))
            })?;
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}

#[cfg(target_os = "windows")]
fn firewall_rules(
) -> Result<windows::Win32::NetworkManagement::WindowsFirewall::INetFwRules, FirewallBackendError> {
    use windows::core::IUnknown;
    use windows::Win32::NetworkManagement::WindowsFirewall::{INetFwPolicy2, NetFwPolicy2};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    let policy: INetFwPolicy2 =
        unsafe { CoCreateInstance(&NetFwPolicy2, None::<&IUnknown>, CLSCTX_INPROC_SERVER) }
            .map_err(|source| {
                FirewallBackendError::Native(format!(
                    "could not open Windows Firewall policy: {source}"
                ))
            })?;
    unsafe { policy.Rules() }.map_err(|source| {
        FirewallBackendError::Native(format!(
            "could not enumerate Windows Firewall rules: {source}"
        ))
    })
}

#[cfg(target_os = "windows")]
fn create_system_rule(rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
    use windows::core::{IUnknown, BSTR};
    use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwRule, NetFwRule, NET_FW_ACTION_ALLOW, NET_FW_IP_PROTOCOL_TCP, NET_FW_IP_PROTOCOL_UDP,
        NET_FW_PROFILE2_ALL, NET_FW_RULE_DIR_IN,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    if !rule.program_path.is_absolute()
        || rule.direction != FirewallDirection::Inbound
        || rule.action != FirewallAction::Allow
        || rule.group != RULE_NAME_PREFIX
        || rule.name.is_empty()
    {
        return Err(FirewallBackendError::Native(
            "refused an invalid or overbroad firewall rule".into(),
        ));
    }
    let _apartment = ComApartment::initialize()?;
    let rules = firewall_rules()?;
    if unsafe { rules.Item(&BSTR::from(rule.name.as_str())) }.is_ok() {
        return Err(FirewallBackendError::Native(format!(
            "firewall rule already exists: {}",
            rule.name
        )));
    }

    let native_rule: INetFwRule =
        unsafe { CoCreateInstance(&NetFwRule, None::<&IUnknown>, CLSCTX_INPROC_SERVER) }.map_err(
            |source| {
                FirewallBackendError::Native(format!(
                    "could not create Windows Firewall rule object: {source}"
                ))
            },
        )?;
    let metadata = FirewallRuleMetadata {
        version: 1,
        rule_id: rule.rule_id.clone(),
        session_id: rule.session_id.clone(),
        service_id: rule.service_id.clone(),
    };
    let description = serde_json::to_string(&metadata).map_err(|source| {
        FirewallBackendError::Native(format!(
            "could not encode firewall rule ownership metadata: {source}"
        ))
    })?;
    let protocol = match rule.protocol {
        FirewallProtocol::Tcp => NET_FW_IP_PROTOCOL_TCP,
        FirewallProtocol::Udp => NET_FW_IP_PROTOCOL_UDP,
    };
    let local_ports = join_ports(&rule.local_ports);
    let local_addresses = join_addresses(&rule.local_addresses);
    let remote_addresses = encode_remote_scope(&rule.remote_scope);
    let program = rule.program_path.to_string_lossy().into_owned();

    let install_result = (|| -> windows::core::Result<()> {
        unsafe {
            native_rule.SetName(&BSTR::from(rule.name.as_str()))?;
            native_rule.SetDescription(&BSTR::from(description.as_str()))?;
            native_rule.SetApplicationName(&BSTR::from(program.as_str()))?;
            native_rule.SetProtocol(protocol.0)?;
            native_rule.SetLocalPorts(&BSTR::from(local_ports.as_str()))?;
            native_rule.SetLocalAddresses(&BSTR::from(local_addresses.as_str()))?;
            native_rule.SetRemoteAddresses(&BSTR::from(remote_addresses.as_str()))?;
            native_rule.SetDirection(NET_FW_RULE_DIR_IN)?;
            native_rule.SetGrouping(&BSTR::from(rule.group.as_str()))?;
            native_rule.SetProfiles(NET_FW_PROFILE2_ALL.0)?;
            native_rule.SetEdgeTraversal(VARIANT_FALSE)?;
            native_rule.SetAction(NET_FW_ACTION_ALLOW)?;
            native_rule.SetEnabled(VARIANT_TRUE)?;
            rules.Add(&native_rule)?;
        }
        Ok(())
    })();
    install_result.map_err(|source| {
        FirewallBackendError::Native(format!(
            "could not install Windows Firewall rule '{}': {source}",
            rule.name
        ))
    })?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn create_system_rule(_rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
    Err(FirewallBackendError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
fn update_system_rule(rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
    use windows::core::BSTR;

    let matching = list_system_managed_rules()?
        .into_iter()
        .filter(|observed| observed.rule_id == rule.rule_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(FirewallBackendError::Native(format!(
            "expected exactly one owned firewall rule for '{}', found {}",
            rule.rule_id,
            matching.len()
        )));
    }
    let observed = &matching[0];
    let mut expected = rule.clone();
    expected.remote_scope = observed.remote_scope.clone();
    if &expected != observed {
        return Err(FirewallBackendError::Native(format!(
            "refused to change non-scope fields of firewall rule '{}'",
            rule.rule_id
        )));
    }

    let _apartment = ComApartment::initialize()?;
    let rules = firewall_rules()?;
    let native_rule =
        unsafe { rules.Item(&BSTR::from(observed.name.as_str())) }.map_err(|source| {
            FirewallBackendError::Native(format!(
                "could not open Windows Firewall rule '{}': {source}",
                observed.name
            ))
        })?;
    let remote_addresses = encode_remote_scope(&rule.remote_scope);
    unsafe { native_rule.SetRemoteAddresses(&BSTR::from(remote_addresses.as_str())) }.map_err(
        |source| {
            FirewallBackendError::Native(format!(
                "could not update Windows Firewall rule '{}': {source}",
                observed.name
            ))
        },
    )
}

#[cfg(not(target_os = "windows"))]
fn update_system_rule(_rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
    Err(FirewallBackendError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
fn list_system_managed_rules() -> Result<Vec<FirewallRuleSpec>, FirewallBackendError> {
    use windows::core::{IUnknown, Interface, VARIANT};
    use windows::Win32::NetworkManagement::WindowsFirewall::INetFwRule;
    use windows::Win32::System::Com::IDispatch;
    use windows::Win32::System::Ole::IEnumVARIANT;

    let _apartment = ComApartment::initialize()?;
    let rules = firewall_rules()?;
    let enumerator: IEnumVARIANT = unsafe { rules._NewEnum() }
        .and_then(|unknown| unknown.cast())
        .map_err(|source| {
            FirewallBackendError::Native(format!(
                "could not enumerate Windows Firewall rules: {source}"
            ))
        })?;
    let mut managed = Vec::new();
    loop {
        let mut value = [VARIANT::default()];
        let mut fetched = 0_u32;
        let result = unsafe { enumerator.Next(&mut value, &mut fetched) };
        result.ok().map_err(|source| {
            FirewallBackendError::Native(format!("Windows Firewall enumeration failed: {source}"))
        })?;
        if fetched == 0 {
            break;
        }
        let variant_type = unsafe { value[0].as_raw().Anonymous.Anonymous.vt };
        let native_rule: INetFwRule = if variant_type == 9 {
            // IEnumVARIANT returns firewall rules as VT_DISPATCH. Borrow the
            // reference owned by VARIANT, QueryInterface a new typed reference,
            // and let the VARIANT release its original reference on drop.
            let pointer = unsafe { value[0].as_raw().Anonymous.Anonymous.Anonymous.pdispVal };
            if pointer.is_null() {
                return Err(FirewallBackendError::Native(
                    "Windows Firewall returned a null dispatch rule".into(),
                ));
            }
            let dispatch = std::mem::ManuallyDrop::new(unsafe { IDispatch::from_raw(pointer) });
            dispatch.cast().map_err(|source| {
                FirewallBackendError::Native(format!(
                    "Windows Firewall dispatch object has an unexpected type: {source}"
                ))
            })?
        } else {
            let unknown = IUnknown::try_from(&value[0]).map_err(|source| {
                FirewallBackendError::Native(format!(
                    "Windows Firewall returned an invalid rule object: {source}"
                ))
            })?;
            unknown.cast().map_err(|source| {
                FirewallBackendError::Native(format!(
                    "Windows Firewall rule object has an unexpected type: {source}"
                ))
            })?
        };
        if let Some(rule) = decode_managed_rule(&native_rule)? {
            managed.push(rule);
        }
    }
    managed.sort_by(|left, right| left.rule_id.cmp(&right.rule_id));
    Ok(managed)
}

#[cfg(not(target_os = "windows"))]
fn list_system_managed_rules() -> Result<Vec<FirewallRuleSpec>, FirewallBackendError> {
    Err(FirewallBackendError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
fn decode_managed_rule(
    rule: &windows::Win32::NetworkManagement::WindowsFirewall::INetFwRule,
) -> Result<Option<FirewallRuleSpec>, FirewallBackendError> {
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        NET_FW_ACTION_ALLOW, NET_FW_IP_PROTOCOL_TCP, NET_FW_IP_PROTOCOL_UDP, NET_FW_RULE_DIR_IN,
    };

    let name = unsafe { rule.Name() }
        .map_err(firewall_read_error)?
        .to_string();
    let group = unsafe { rule.Grouping() }
        .map_err(firewall_read_error)?
        .to_string();
    if group != RULE_NAME_PREFIX || !name.starts_with(RULE_NAME_PREFIX) {
        return Ok(None);
    }
    let description = unsafe { rule.Description() }
        .map_err(firewall_read_error)?
        .to_string();
    let metadata: FirewallRuleMetadata =
        match serde_json::from_str::<FirewallRuleMetadata>(&description) {
            Ok(metadata) if metadata.version == 1 => metadata,
            _ => return Ok(None),
        };
    let direction = unsafe { rule.Direction() }.map_err(firewall_read_error)?;
    let action = unsafe { rule.Action() }.map_err(firewall_read_error)?;
    if direction != NET_FW_RULE_DIR_IN || action != NET_FW_ACTION_ALLOW {
        return Err(FirewallBackendError::Native(format!(
            "managed firewall rule '{}' changed direction or action",
            name
        )));
    }
    let protocol = match unsafe { rule.Protocol() }.map_err(firewall_read_error)? {
        value if value == NET_FW_IP_PROTOCOL_TCP.0 => FirewallProtocol::Tcp,
        value if value == NET_FW_IP_PROTOCOL_UDP.0 => FirewallProtocol::Udp,
        value => {
            return Err(FirewallBackendError::Native(format!(
                "managed firewall rule '{}' changed to unsupported protocol {}",
                name, value
            )))
        }
    };
    let program_path = PathBuf::from(
        unsafe { rule.ApplicationName() }
            .map_err(firewall_read_error)?
            .to_string(),
    );
    let local_ports = parse_ports(
        &unsafe { rule.LocalPorts() }
            .map_err(firewall_read_error)?
            .to_string(),
    )?;
    let local_addresses = parse_addresses(
        &unsafe { rule.LocalAddresses() }
            .map_err(firewall_read_error)?
            .to_string(),
    )?;
    let remote_scope = parse_remote_scope(
        &unsafe { rule.RemoteAddresses() }
            .map_err(firewall_read_error)?
            .to_string(),
    )?;
    Ok(Some(FirewallRuleSpec {
        rule_id: metadata.rule_id,
        name,
        group,
        session_id: metadata.session_id,
        service_id: metadata.service_id,
        program_path,
        direction: FirewallDirection::Inbound,
        action: FirewallAction::Allow,
        protocol,
        local_ports,
        local_addresses,
        remote_scope,
    }))
}

#[cfg(target_os = "windows")]
fn firewall_read_error(source: windows::core::Error) -> FirewallBackendError {
    FirewallBackendError::Native(format!("could not inspect Windows Firewall rule: {source}"))
}

#[cfg(target_os = "windows")]
fn delete_system_rule(rule_id: &str) -> Result<(), FirewallBackendError> {
    use windows::core::BSTR;

    if rule_id.trim().is_empty() {
        return Err(FirewallBackendError::Native(
            "firewall rule id is empty".into(),
        ));
    }
    let matching = list_system_managed_rules()?
        .into_iter()
        .filter(|rule| rule.rule_id == rule_id || rule.name == rule_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(FirewallBackendError::Native(format!(
            "expected exactly one owned firewall rule for '{rule_id}', found {}",
            matching.len()
        )));
    }
    let _apartment = ComApartment::initialize()?;
    let rules = firewall_rules()?;
    unsafe { rules.Remove(&BSTR::from(matching[0].name.as_str())) }.map_err(|source| {
        FirewallBackendError::Native(format!(
            "could not remove Windows Firewall rule '{}': {source}",
            matching[0].name
        ))
    })
}

#[cfg(not(target_os = "windows"))]
fn delete_system_rule(_rule_id: &str) -> Result<(), FirewallBackendError> {
    Err(FirewallBackendError::UnsupportedPlatform)
}

fn join_ports(ports: &[u16]) -> String {
    ports
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn join_addresses(addresses: &[Ipv4Addr]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn encode_remote_scope(scope: &FirewallRemoteScope) -> String {
    match scope {
        FirewallRemoteScope::Addresses(addresses) => join_addresses(addresses),
        FirewallRemoteScope::SelectedSubnet(subnet) => {
            format!("{}/{}", subnet.network, subnet.prefix_len)
        }
    }
}

fn parse_ports(value: &str) -> Result<Vec<u16>, FirewallBackendError> {
    let mut ports = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse::<u16>().map_err(|_| {
                FirewallBackendError::Native(format!(
                    "managed firewall rule contains invalid port '{item}'"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() {
        return Err(FirewallBackendError::Native(
            "managed firewall rule has no local ports".into(),
        ));
    }
    Ok(ports)
}

fn invalid_ipv4_address(item: &str) -> FirewallBackendError {
    FirewallBackendError::Native(format!(
        "managed firewall rule contains invalid IPv4 address '{item}'"
    ))
}

fn invalid_subnet(item: &str) -> FirewallBackendError {
    FirewallBackendError::Native(format!(
        "managed firewall rule contains invalid subnet '{item}'"
    ))
}

/// Windows Firewall's COM API echoes addresses back in canonical forms the bare
/// `Ipv4Addr` parser rejects: a single host comes back as `IP/255.255.255.255`
/// (or `IP/32`) and a degenerate range as `IP-IP`, never as a plain `IP`.
/// Normalize one such token to its host address so a rule we wrote round-trips
/// on read-back instead of failing the whole managed-rule enumeration, which
/// would abort firewall cleanup and leave the session un-recoverable.
fn parse_host_address(token: &str) -> Result<Ipv4Addr, FirewallBackendError> {
    let token = token.trim();
    let host = if let Some((addr, suffix)) = token.split_once('/') {
        // Only a host-scoped mask is a single address; a broader mask here means
        // a subnet was stored in a field that must hold host addresses.
        if !matches!(suffix.trim(), "32" | "255.255.255.255") {
            return Err(invalid_ipv4_address(token));
        }
        addr.trim()
    } else if let Some((start, end)) = token.split_once('-') {
        if start.trim() != end.trim() {
            return Err(invalid_ipv4_address(token));
        }
        start.trim()
    } else {
        token
    };
    host.parse::<Ipv4Addr>()
        .map_err(|_| invalid_ipv4_address(token))
}

/// Convert a dotted-decimal subnet mask (e.g. `255.255.255.0`) to a prefix
/// length, or `None` if it is not a valid contiguous netmask.
fn dotted_mask_to_prefix(mask: &str) -> Option<u8> {
    let bits = u32::from(mask.trim().parse::<Ipv4Addr>().ok()?);
    // A valid netmask is contiguous 1s followed by contiguous 0s.
    if bits.leading_ones() + bits.trailing_zeros() != 32 {
        return None;
    }
    Some(bits.leading_ones() as u8)
}

fn parse_addresses(value: &str) -> Result<Vec<Ipv4Addr>, FirewallBackendError> {
    let mut addresses = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(parse_host_address)
        .collect::<Result<Vec<_>, _>>()?;
    addresses.sort_unstable();
    addresses.dedup();
    if addresses.is_empty() {
        return Err(FirewallBackendError::Native(
            "managed firewall rule has no IPv4 addresses".into(),
        ));
    }
    Ok(addresses)
}

fn parse_remote_scope(value: &str) -> Result<FirewallRemoteScope, FirewallBackendError> {
    let trimmed = value.trim();
    // A comma-separated list is always an explicit host set.
    if trimmed.contains(',') {
        return parse_addresses(trimmed).map(FirewallRemoteScope::Addresses);
    }
    if let Some((addr, suffix)) = trimmed.split_once('/') {
        let suffix = suffix.trim();
        // A host-scoped mask denotes a single explicit address, not a subnet.
        if matches!(suffix, "32" | "255.255.255.255") {
            return parse_addresses(trimmed).map(FirewallRemoteScope::Addresses);
        }
        // Otherwise a subnet, expressed as either a prefix length or a dotted
        // mask (Windows echoes SelectedSubnet scopes back as `network/mask`).
        let prefix_len = suffix
            .parse::<u8>()
            .ok()
            .or_else(|| dotted_mask_to_prefix(suffix))
            .ok_or_else(|| invalid_subnet(trimmed))?;
        let network = addr
            .trim()
            .parse::<Ipv4Addr>()
            .map_err(|_| invalid_subnet(trimmed))?;
        return Ipv4Subnet::from_address(network, prefix_len)
            .map(FirewallRemoteScope::SelectedSubnet)
            .map_err(|error| FirewallBackendError::Native(error.to_string()));
    }
    // No mask: a plain host or a degenerate `IP-IP` range.
    parse_addresses(trimmed).map(FirewallRemoteScope::Addresses)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(value: &str) -> Ipv4Addr {
        value.parse().unwrap()
    }

    fn app_path() -> PathBuf {
        PathBuf::from(r"C:\Program Files\File Sync Tool\file-sync-tool.exe")
    }

    fn http_intent() -> FirewallServiceIntent {
        FirewallServiceIntent {
            service_id: "http".into(),
            protocol: FirewallProtocol::Tcp,
            local_ports: vec![81],
            local_addresses: vec![ip("192.168.50.10"), ip("192.168.50.11")],
            remote_scope: FirewallRemoteScope::Addresses(vec![ip("192.168.50.2")]),
        }
    }

    fn rule_for(session_id: &str) -> FirewallRuleSpec {
        plan_firewall_rules(session_id, &app_path(), vec![http_intent()])
            .unwrap()
            .rules
            .remove(0)
    }

    #[test]
    fn plans_only_exact_inbound_allow_rules_for_session_resources() {
        let plan = plan_firewall_rules("session-1", &app_path(), vec![http_intent()]).unwrap();
        assert_eq!(plan.rules.len(), 1);
        let rule = &plan.rules[0];
        assert_eq!(rule.direction, FirewallDirection::Inbound);
        assert_eq!(rule.action, FirewallAction::Allow);
        assert_eq!(rule.protocol, FirewallProtocol::Tcp);
        assert_eq!(rule.local_ports, vec![81]);
        assert_eq!(
            rule.local_addresses,
            vec![ip("192.168.50.10"), ip("192.168.50.11")]
        );
        assert!(rule
            .name
            .starts_with("FileSyncTool-DeviceSimulator-session-1-http"));
        assert_eq!(rule.program_path, app_path());
    }

    #[test]
    fn invalid_or_overbroad_intents_fail_before_native_mutation() {
        let mut intent = http_intent();
        intent.local_addresses.clear();
        assert!(matches!(
            plan_firewall_rules("session-1", &app_path(), vec![intent]),
            Err(FirewallPlanError::EmptyLocalAddresses(_))
        ));

        let mut intent = http_intent();
        intent.remote_scope = FirewallRemoteScope::Addresses(vec![]);
        assert!(matches!(
            plan_firewall_rules("session-1", &app_path(), vec![intent]),
            Err(FirewallPlanError::EmptyRemoteAddresses(_))
        ));

        assert_eq!(
            plan_firewall_rules("../foreign", &app_path(), vec![http_intent()]),
            Err(FirewallPlanError::InvalidSessionId)
        );
    }

    #[test]
    fn cleanup_deletes_only_rules_owned_by_the_requested_session() {
        let owned = rule_for("session-a");
        let foreign = rule_for("session-b");
        let journal = SessionFirewallJournal {
            session_id: "session-a".into(),
            rules: vec![owned.clone()],
        };

        let cleanup = plan_session_firewall_cleanup(
            "session-a",
            &app_path(),
            &journal,
            &[owned.clone(), foreign.clone()],
        )
        .unwrap();

        assert_eq!(cleanup.delete_rule_ids, vec![owned.rule_id]);
        assert_eq!(
            cleanup.retained_rules,
            vec![RetainedFirewallRule {
                rule_id: foreign.rule_id,
                reason: FirewallRetentionReason::NotJournaled,
            }]
        );
    }

    #[test]
    fn cleanup_refuses_a_journaled_rule_that_was_changed_after_creation() {
        let owned = rule_for("session-a");
        let journal = SessionFirewallJournal {
            session_id: "session-a".into(),
            rules: vec![owned.clone()],
        };
        let mut changed = owned.clone();
        changed.local_ports = vec![81, 554];

        let cleanup =
            plan_session_firewall_cleanup("session-a", &app_path(), &journal, &[changed]).unwrap();
        assert!(cleanup.delete_rule_ids.is_empty());
        assert_eq!(
            cleanup.retained_rules[0].reason,
            FirewallRetentionReason::RuleChanged
        );

        let mut foreign_with_same_rule_id = owned.clone();
        foreign_with_same_rule_id.session_id = "session-b".into();
        let cleanup = plan_session_firewall_cleanup(
            "session-a",
            &app_path(),
            &journal,
            &[foreign_with_same_rule_id],
        )
        .unwrap();
        assert!(cleanup.delete_rule_ids.is_empty());
        assert_eq!(
            cleanup.retained_rules[0].reason,
            FirewallRetentionReason::ForeignSession
        );
    }

    #[test]
    fn cleanup_rejects_foreign_or_ambiguous_journals() {
        let owned = rule_for("session-a");
        let foreign_journal = SessionFirewallJournal {
            session_id: "session-b".into(),
            rules: vec![owned.clone()],
        };
        assert_eq!(
            plan_session_firewall_cleanup(
                "session-a",
                &app_path(),
                &foreign_journal,
                &[owned.clone()]
            ),
            Err(FirewallOwnershipError::JournalSessionMismatch)
        );

        let duplicate = SessionFirewallJournal {
            session_id: "session-a".into(),
            rules: vec![owned.clone(), owned],
        };
        assert!(matches!(
            plan_session_firewall_cleanup("session-a", &app_path(), &duplicate, &[]),
            Err(FirewallOwnershipError::DuplicateJournalRule(_))
        ));
    }

    #[test]
    fn firewall_string_contract_round_trips_exact_scopes() {
        assert_eq!(parse_ports("81,554,555").unwrap(), vec![81, 554, 555]);
        assert_eq!(
            parse_addresses("192.168.50.10,192.168.50.11").unwrap(),
            vec![ip("192.168.50.10"), ip("192.168.50.11")]
        );
        assert_eq!(
            parse_remote_scope("192.168.50.0/24").unwrap(),
            FirewallRemoteScope::SelectedSubnet(
                Ipv4Subnet::from_address(ip("192.168.50.10"), 24).unwrap()
            )
        );
        assert!(parse_remote_scope("*").is_err());
    }

    #[test]
    fn parse_accepts_windows_canonical_address_forms() {
        // Windows Firewall echoes a single host back as `IP/255.255.255.255`
        // (or `IP/32`) and a degenerate range as `IP-IP`; reading back a rule
        // we wrote must round-trip, not abort managed-rule enumeration.
        assert_eq!(
            parse_addresses("192.115.1.220/255.255.255.255").unwrap(),
            vec![ip("192.115.1.220")]
        );
        assert_eq!(
            parse_addresses("192.168.50.10/32,192.168.50.11/255.255.255.255").unwrap(),
            vec![ip("192.168.50.10"), ip("192.168.50.11")]
        );
        assert_eq!(
            parse_addresses("192.115.1.220-192.115.1.220").unwrap(),
            vec![ip("192.115.1.220")]
        );
        assert_eq!(
            parse_remote_scope("192.115.1.11/255.255.255.255").unwrap(),
            FirewallRemoteScope::Addresses(vec![ip("192.115.1.11")])
        );
        // A subnet remote scope may come back as a dotted mask instead of a prefix.
        assert_eq!(
            parse_remote_scope("192.168.50.0/255.255.255.0").unwrap(),
            FirewallRemoteScope::SelectedSubnet(
                Ipv4Subnet::from_address(ip("192.168.50.10"), 24).unwrap()
            )
        );
        // A genuinely broken address must still be rejected.
        assert!(parse_addresses("192.115.1.220/255.255.0.255").is_err());
        assert!(parse_addresses("192.115.1.220/24").is_err());
        assert!(parse_addresses("192.115.1.220-192.115.1.221").is_err());
        assert!(parse_addresses("not-an-ip").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn system_firewall_backend_supports_read_only_enumeration() {
        let rules = SystemFirewallBackend
            .list_managed_rules()
            .expect("Windows Firewall COM enumeration should succeed");
        assert!(rules.iter().all(|rule| {
            rule.group == RULE_NAME_PREFIX && rule.name.starts_with(RULE_NAME_PREFIX)
        }));
    }
}
