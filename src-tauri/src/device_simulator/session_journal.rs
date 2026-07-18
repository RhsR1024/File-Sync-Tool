use crate::device_simulator::errors::{SimulatorError, SimulatorErrorBody, SimulatorResult};
use crate::device_simulator::models::SessionState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

pub const SESSION_JOURNAL_SCHEMA_VERSION: u32 = 1;
pub const JOURNAL_IO_ERROR: &str = "device_simulator.recovery.journal_io";
pub const JOURNAL_INVALID: &str = "device_simulator.recovery.journal_invalid";
pub const JOURNAL_NOT_FOUND: &str = "device_simulator.recovery.journal_not_found";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRequestSummary {
    pub profile_ids: Vec<String>,
    pub total_devices: u32,
    pub total_nvr_channels: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProcessIdentity {
    pub pid: u32,
    /// Windows process creation time. It prevents a recycled PID from being
    /// mistaken for the Worker that belongs to this session.
    pub creation_time_100ns: u64,
    /// A stable executable hash or other non-secret build fingerprint.
    pub executable_identity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnershipState {
    Planned,
    Owned,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedIpAddress {
    pub interface_id: String,
    pub address: Ipv4Addr,
    /// Persisted explicitly so recovery never re-derives a deletion target
    /// from mutable UI configuration.
    pub prefix_len: u8,
    pub state: ResourceOwnershipState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedFirewallRule {
    pub rule_name: String,
    pub state: ResourceOwnershipState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedPack {
    pub id: String,
    pub version: String,
    pub state: ResourceOwnershipState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedResources {
    #[serde(default)]
    pub ip_addresses: Vec<OwnedIpAddress>,
    #[serde(default)]
    pub firewall_rules: Vec<OwnedFirewallRule>,
    #[serde(default)]
    pub packs: Vec<OwnedPack>,
}

impl OwnedResources {
    pub fn has_owned_resources(&self) -> bool {
        self.ip_addresses
            .iter()
            .any(|resource| resource.state == ResourceOwnershipState::Owned)
            || self
                .firewall_rules
                .iter()
                .any(|resource| resource.state == ResourceOwnershipState::Owned)
            || self
                .packs
                .iter()
                .any(|resource| resource.state == ResourceOwnershipState::Owned)
    }

    fn remaining_labels(&self) -> Vec<String> {
        let mut remaining = Vec::new();
        remaining.extend(
            self.ip_addresses
                .iter()
                .filter(|resource| resource.state == ResourceOwnershipState::Owned)
                .map(|resource| {
                    format!(
                        "ip:{}/{}@{}",
                        resource.address, resource.prefix_len, resource.interface_id
                    )
                }),
        );
        remaining.extend(
            self.firewall_rules
                .iter()
                .filter(|resource| resource.state == ResourceOwnershipState::Owned)
                .map(|resource| format!("firewall: {}", resource.rule_name)),
        );
        remaining.extend(
            self.packs
                .iter()
                .filter(|resource| resource.state == ResourceOwnershipState::Owned)
                .map(|resource| format!("pack:{}@{}", resource.id, resource.version)),
        );
        remaining
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JournalCleanupStage {
    #[default]
    NotStarted,
    StoppingAlarms,
    StoppingServices,
    RemovingFirewall,
    RemovingIps,
    ReleasingPacks,
    Complete,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupProgress {
    pub stage: JournalCleanupStage,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionJournalV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub app_version: String,
    pub worker_version: String,
    pub interface_id: String,
    pub device_summary: DeviceRequestSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_process: Option<WorkerProcessIdentity>,
    #[serde(default)]
    pub resources: OwnedResources,
    #[serde(default)]
    pub cleanup: CleanupProgress,
    pub state: SessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<SimulatorErrorBody>,
}

impl std::fmt::Debug for SessionJournalV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionJournalV1")
            .field("schema_version", &self.schema_version)
            .field("session_id", &self.session_id)
            .field("state", &self.state)
            .field("cleanup", &self.cleanup)
            .field("resource_count", &self.remaining_resources().len())
            .field("last_error", &self.last_error)
            .finish_non_exhaustive()
    }
}

impl SessionJournalV1 {
    /// A failed session is not recoverably terminal while it still owns a
    /// resource. This is deliberately stricter than `SessionState::is_terminal`.
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, SessionState::Stopped | SessionState::Failed)
            && self.cleanup.stage == JournalCleanupStage::Complete
            && !self.resources.has_owned_resources()
    }

    pub fn remaining_resources(&self) -> Vec<String> {
        self.resources.remaining_labels()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedProcessIdentity {
    pub pid: u32,
    pub creation_time_100ns: u64,
    pub executable_identity: String,
}

pub trait WorkerProcessProbe {
    fn inspect(&mut self, pid: u32) -> SimulatorResult<Option<ObservedProcessIdentity>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerPresence {
    NotRecorded,
    Exited,
    SameProcess,
    PidReused,
}

pub fn inspect_worker_presence<P: WorkerProcessProbe>(
    journal: &SessionJournalV1,
    probe: &mut P,
) -> SimulatorResult<WorkerPresence> {
    let Some(expected) = &journal.worker_process else {
        return Ok(WorkerPresence::NotRecorded);
    };
    let Some(observed) = probe.inspect(expected.pid)? else {
        return Ok(WorkerPresence::Exited);
    };
    if observed.pid == expected.pid
        && observed.creation_time_100ns == expected.creation_time_100ns
        && observed.executable_identity == expected.executable_identity
    {
        Ok(WorkerPresence::SameProcess)
    } else {
        Ok(WorkerPresence::PidReused)
    }
}

/// Platform cleanup is injected by the caller. Implementations must make the
/// stop operations idempotent because a crash can occur after the operation
/// succeeds but before the next journal replacement is durable.
pub trait SessionResourceCleaner {
    fn stop_alarm_jobs(&mut self, session_id: &str) -> SimulatorResult<()>;
    fn stop_services(&mut self, session_id: &str) -> SimulatorResult<()>;
    fn firewall_rule_exists(&mut self, rule_name: &str) -> SimulatorResult<bool>;
    fn remove_firewall_rule(&mut self, rule_name: &str) -> SimulatorResult<()>;
    fn ip_address_exists(&mut self, interface_id: &str, address: Ipv4Addr)
        -> SimulatorResult<bool>;
    fn remove_ip_address(
        &mut self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> SimulatorResult<()>;
    fn pack_pin_exists(&mut self, pack_id: &str, version: &str) -> SimulatorResult<bool>;
    fn release_pack_pin(&mut self, pack_id: &str, version: &str) -> SimulatorResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryOutcome {
    pub journal: SessionJournalV1,
    pub recovered: bool,
    pub remaining_resources: Vec<String>,
    pub error: Option<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionJournalStore {
    sessions_dir: PathBuf,
}

impl SessionJournalStore {
    pub fn from_app_data_dir(app_data_dir: impl AsRef<Path>) -> Self {
        Self::new(
            app_data_dir
                .as_ref()
                .join("device-simulator")
                .join("sessions"),
        )
    }

    pub fn new(sessions_dir: impl Into<PathBuf>) -> Self {
        Self {
            sessions_dir: sessions_dir.into(),
        }
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }

    pub fn save(&self, journal: &SessionJournalV1) -> SimulatorResult<()> {
        validate_journal(journal, Some(&journal.session_id))?;
        fs::create_dir_all(&self.sessions_dir)
            .map_err(|error| journal_io_error("create session journal directory", error))?;
        let bytes = serde_json::to_vec_pretty(journal)
            .map_err(|error| journal_serialize_error("serialize session journal", error))?;
        self.replace_primary(&journal.session_id, &bytes)
    }

    pub fn load(&self, session_id: &str) -> SimulatorResult<SessionJournalV1> {
        validate_session_id(session_id)?;
        let paths = JournalPaths::new(&self.sessions_dir, session_id);
        let mut saw_file = false;
        let mut first_error = None;

        for (path, restore) in [
            (&paths.primary, false),
            (&paths.backup, true),
            (&paths.temporary, true),
        ] {
            match fs::read(path) {
                Ok(bytes) => {
                    saw_file = true;
                    match decode_journal(&bytes, session_id) {
                        Ok(journal) => {
                            if restore {
                                self.restore_primary(session_id, &bytes)?;
                            }
                            return Ok(journal);
                        }
                        Err(error) => {
                            if first_error.is_none() {
                                first_error = Some(error);
                            }
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(journal_io_error("read session journal", error)),
            }
        }

        if saw_file {
            Err(first_error.unwrap_or_else(|| invalid_journal("no valid journal candidate")))
        } else {
            Err(SimulatorError::new(
                JOURNAL_NOT_FOUND,
                "deviceSimulator.errors.sessionJournalNotFound",
            ))
        }
    }

    pub fn list_non_terminal(&self) -> SimulatorResult<Vec<SessionJournalV1>> {
        let entries = match fs::read_dir(&self.sessions_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(journal_io_error("list session journals", error)),
        };
        let mut session_ids = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(|error| journal_io_error("list session journals", error))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(session_id) = journal_session_id_from_name(&name) {
                session_ids.insert(session_id.to_owned());
            }
        }

        let mut journals = Vec::new();
        for session_id in session_ids {
            let journal = self.load(&session_id)?;
            if !journal.is_terminal() {
                journals.push(journal);
            }
        }
        Ok(journals)
    }

    pub fn recover_session<C: SessionResourceCleaner>(
        &self,
        mut journal: SessionJournalV1,
        cleaner: &mut C,
        now_ms: u64,
    ) -> SimulatorResult<RecoveryOutcome> {
        validate_journal(&journal, Some(&journal.session_id))?;
        if journal.is_terminal() {
            return Ok(success_outcome(journal));
        }

        journal.state = SessionState::Recovering;
        journal.cleanup.attempts = journal.cleanup.attempts.saturating_add(1);
        journal.cleanup.last_started_at_ms = Some(now_ms);
        journal.cleanup.completed_at_ms = None;
        journal.updated_at_ms = now_ms;
        journal.last_error = None;
        self.save(&journal)?;

        if journal.cleanup.stage <= JournalCleanupStage::StoppingAlarms {
            journal.cleanup.stage = JournalCleanupStage::StoppingAlarms;
            self.save(&journal)?;
            if let Err(error) = cleaner.stop_alarm_jobs(&journal.session_id) {
                return self.record_recovery_failure(journal, error, now_ms);
            }
            journal.cleanup.stage = JournalCleanupStage::StoppingServices;
            journal.updated_at_ms = now_ms;
            self.save(&journal)?;
        }

        if journal.cleanup.stage <= JournalCleanupStage::StoppingServices {
            journal.cleanup.stage = JournalCleanupStage::StoppingServices;
            self.save(&journal)?;
            if let Err(error) = cleaner.stop_services(&journal.session_id) {
                return self.record_recovery_failure(journal, error, now_ms);
            }
            journal.cleanup.stage = JournalCleanupStage::RemovingFirewall;
            journal.updated_at_ms = now_ms;
            self.save(&journal)?;
        }

        if journal.cleanup.stage <= JournalCleanupStage::RemovingFirewall {
            journal.cleanup.stage = JournalCleanupStage::RemovingFirewall;
            self.save(&journal)?;
            for index in 0..journal.resources.firewall_rules.len() {
                let resource = journal.resources.firewall_rules[index].clone();
                if resource.state == ResourceOwnershipState::Released {
                    continue;
                }
                if resource.state == ResourceOwnershipState::Owned {
                    let exists = match cleaner.firewall_rule_exists(&resource.rule_name) {
                        Ok(exists) => exists,
                        Err(error) => return self.record_recovery_failure(journal, error, now_ms),
                    };
                    if exists {
                        if let Err(error) = cleaner.remove_firewall_rule(&resource.rule_name) {
                            return self.record_recovery_failure(journal, error, now_ms);
                        }
                    }
                }
                journal.resources.firewall_rules[index].state = ResourceOwnershipState::Released;
                journal.updated_at_ms = now_ms;
                self.save(&journal)?;
            }
            journal.cleanup.stage = JournalCleanupStage::RemovingIps;
            self.save(&journal)?;
        }

        if journal.cleanup.stage <= JournalCleanupStage::RemovingIps {
            journal.cleanup.stage = JournalCleanupStage::RemovingIps;
            self.save(&journal)?;
            for index in 0..journal.resources.ip_addresses.len() {
                let resource = journal.resources.ip_addresses[index].clone();
                if resource.state == ResourceOwnershipState::Released {
                    continue;
                }
                if resource.state == ResourceOwnershipState::Owned {
                    let exists = match cleaner
                        .ip_address_exists(&resource.interface_id, resource.address)
                    {
                        Ok(exists) => exists,
                        Err(error) => return self.record_recovery_failure(journal, error, now_ms),
                    };
                    if exists {
                        if let Err(error) = cleaner.remove_ip_address(
                            &resource.interface_id,
                            resource.address,
                            resource.prefix_len,
                        ) {
                            return self.record_recovery_failure(journal, error, now_ms);
                        }
                    }
                }
                journal.resources.ip_addresses[index].state = ResourceOwnershipState::Released;
                journal.updated_at_ms = now_ms;
                self.save(&journal)?;
            }
            journal.cleanup.stage = JournalCleanupStage::ReleasingPacks;
            self.save(&journal)?;
        }

        if journal.cleanup.stage <= JournalCleanupStage::ReleasingPacks {
            journal.cleanup.stage = JournalCleanupStage::ReleasingPacks;
            self.save(&journal)?;
            for index in 0..journal.resources.packs.len() {
                let resource = journal.resources.packs[index].clone();
                if resource.state == ResourceOwnershipState::Released {
                    continue;
                }
                if resource.state == ResourceOwnershipState::Owned {
                    let exists = match cleaner.pack_pin_exists(&resource.id, &resource.version) {
                        Ok(exists) => exists,
                        Err(error) => return self.record_recovery_failure(journal, error, now_ms),
                    };
                    if exists {
                        if let Err(error) =
                            cleaner.release_pack_pin(&resource.id, &resource.version)
                        {
                            return self.record_recovery_failure(journal, error, now_ms);
                        }
                    }
                }
                journal.resources.packs[index].state = ResourceOwnershipState::Released;
                journal.updated_at_ms = now_ms;
                self.save(&journal)?;
            }
        }

        journal.cleanup.stage = JournalCleanupStage::Complete;
        journal.cleanup.completed_at_ms = Some(now_ms);
        journal.state = SessionState::Stopped;
        journal.updated_at_ms = now_ms;
        journal.last_error = None;
        self.save(&journal)?;
        Ok(success_outcome(journal))
    }

    fn record_recovery_failure(
        &self,
        mut journal: SessionJournalV1,
        error: SimulatorError,
        now_ms: u64,
    ) -> SimulatorResult<RecoveryOutcome> {
        let body = error.into_body();
        journal.state = SessionState::RecoveryRequired;
        journal.updated_at_ms = now_ms;
        journal.last_error = Some(body.clone());
        self.save(&journal)?;
        Ok(RecoveryOutcome {
            remaining_resources: journal.remaining_resources(),
            journal,
            recovered: false,
            error: Some(body),
        })
    }

    fn replace_primary(&self, session_id: &str, bytes: &[u8]) -> SimulatorResult<()> {
        let paths = JournalPaths::new(&self.sessions_dir, session_id);
        write_synced(&paths.temporary, bytes)?;
        remove_if_exists(&paths.backup)?;
        let had_primary = paths.primary.exists();
        if had_primary {
            fs::rename(&paths.primary, &paths.backup)
                .map_err(|error| journal_io_error("backup session journal", error))?;
        }
        if let Err(error) = fs::rename(&paths.temporary, &paths.primary) {
            if had_primary {
                let _ = fs::rename(&paths.backup, &paths.primary);
            }
            return Err(journal_io_error("activate session journal", error));
        }
        Ok(())
    }

    fn restore_primary(&self, session_id: &str, bytes: &[u8]) -> SimulatorResult<()> {
        let paths = JournalPaths::new(&self.sessions_dir, session_id);
        let restore = paths.primary.with_extension("json.restore");
        write_synced(&restore, bytes)?;
        remove_if_exists(&paths.primary)?;
        fs::rename(&restore, &paths.primary)
            .map_err(|error| journal_io_error("restore session journal", error))
    }
}

#[derive(Debug)]
struct JournalPaths {
    primary: PathBuf,
    temporary: PathBuf,
    backup: PathBuf,
}

impl JournalPaths {
    fn new(sessions_dir: &Path, session_id: &str) -> Self {
        let primary = sessions_dir.join(format!("{session_id}.json"));
        Self {
            temporary: sessions_dir.join(format!("{session_id}.json.tmp")),
            backup: sessions_dir.join(format!("{session_id}.json.bak")),
            primary,
        }
    }
}

fn success_outcome(journal: SessionJournalV1) -> RecoveryOutcome {
    RecoveryOutcome {
        remaining_resources: journal.remaining_resources(),
        journal,
        recovered: true,
        error: None,
    }
}

fn validate_session_id(session_id: &str) -> SimulatorResult<()> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid_journal("invalid session identifier"));
    }
    Ok(())
}

fn validate_journal(
    journal: &SessionJournalV1,
    expected_session_id: Option<&str>,
) -> SimulatorResult<()> {
    validate_session_id(&journal.session_id)?;
    if journal.schema_version != SESSION_JOURNAL_SCHEMA_VERSION {
        return Err(invalid_journal("unsupported journal schema version"));
    }
    if expected_session_id.is_some_and(|expected| expected != journal.session_id) {
        return Err(invalid_journal("journal session identifier mismatch"));
    }
    if journal.app_version.trim().is_empty()
        || journal.worker_version.trim().is_empty()
        || journal.interface_id.trim().is_empty()
    {
        return Err(invalid_journal("journal identity fields must not be empty"));
    }
    if journal.resources.ip_addresses.iter().any(|resource| {
        resource.interface_id.is_empty()
            || resource.interface_id != journal.interface_id
            || !(1..=30).contains(&resource.prefix_len)
    }) {
        return Err(invalid_journal(
            "owned IP interface/prefix does not match the journal identity",
        ));
    }
    if journal.cleanup.stage == JournalCleanupStage::Complete
        && journal.resources.has_owned_resources()
    {
        return Err(invalid_journal(
            "complete cleanup journal still contains owned resources",
        ));
    }
    Ok(())
}

fn decode_journal(bytes: &[u8], expected_session_id: &str) -> SimulatorResult<SessionJournalV1> {
    let journal: SessionJournalV1 = serde_json::from_slice(bytes)
        .map_err(|error| journal_serialize_error("decode session journal", error))?;
    validate_journal(&journal, Some(expected_session_id))?;
    Ok(journal)
}

fn journal_session_id_from_name(name: &str) -> Option<&str> {
    for suffix in [".json", ".json.bak", ".json.tmp"] {
        if let Some(session_id) = name.strip_suffix(suffix) {
            if validate_session_id(session_id).is_ok() {
                return Some(session_id);
            }
        }
    }
    None
}

fn write_synced(path: &Path, bytes: &[u8]) -> SimulatorResult<()> {
    let mut file = File::create(path)
        .map_err(|error| journal_io_error("create temporary session journal", error))?;
    file.write_all(bytes)
        .map_err(|error| journal_io_error("write temporary session journal", error))?;
    file.sync_all()
        .map_err(|error| journal_io_error("sync temporary session journal", error))
}

fn remove_if_exists(path: &Path) -> SimulatorResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(journal_io_error("remove stale session journal", error)),
    }
}

fn invalid_journal(details: &'static str) -> SimulatorError {
    SimulatorError::new(
        JOURNAL_INVALID,
        "deviceSimulator.errors.sessionJournalInvalid",
    )
    .with_public_details(details)
}

fn journal_io_error(action: &'static str, source: io::Error) -> SimulatorError {
    SimulatorError::new(JOURNAL_IO_ERROR, "deviceSimulator.errors.sessionJournalIo")
        .with_public_details(action)
        .retryable(true)
        .with_source(source)
}

fn journal_serialize_error(action: &'static str, source: serde_json::Error) -> SimulatorError {
    SimulatorError::new(
        JOURNAL_INVALID,
        "deviceSimulator.errors.sessionJournalInvalid",
    )
    .with_public_details(action)
    .with_source(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn journal(session_id: &str) -> SessionJournalV1 {
        SessionJournalV1 {
            schema_version: SESSION_JOURNAL_SCHEMA_VERSION,
            session_id: session_id.to_owned(),
            created_at_ms: 10,
            updated_at_ms: 10,
            app_version: "1.2.3".to_owned(),
            worker_version: "1.2.3".to_owned(),
            interface_id: "if-guid".to_owned(),
            device_summary: DeviceRequestSummary {
                profile_ids: vec!["camera".to_owned()],
                total_devices: 2,
                total_nvr_channels: 0,
            },
            worker_process: Some(WorkerProcessIdentity {
                pid: 42,
                creation_time_100ns: 9001,
                executable_identity: "worker-sha256".to_owned(),
            }),
            resources: OwnedResources::default(),
            cleanup: CleanupProgress::default(),
            state: SessionState::Running,
            last_error: None,
        }
    }

    #[test]
    fn save_keeps_one_synced_backup() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let mut value = journal("session-1");
        store.save(&value).unwrap();
        value.updated_at_ms = 20;
        store.save(&value).unwrap();

        assert_eq!(store.load("session-1").unwrap().updated_at_ms, 20);
        let backup = JournalPaths::new(temp.path(), "session-1").backup;
        let prior: SessionJournalV1 = serde_json::from_slice(&fs::read(backup).unwrap()).unwrap();
        assert_eq!(prior.updated_at_ms, 10);
    }

    #[test]
    fn truncated_primary_recovers_from_backup_and_restores_primary() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let mut value = journal("session-2");
        store.save(&value).unwrap();
        value.updated_at_ms = 20;
        store.save(&value).unwrap();
        let paths = JournalPaths::new(temp.path(), "session-2");
        fs::write(&paths.primary, b"{\"schema_version\":").unwrap();

        let recovered = store.load("session-2").unwrap();
        assert_eq!(recovered.updated_at_ms, 10);
        let restored: SessionJournalV1 =
            serde_json::from_slice(&fs::read(paths.primary).unwrap()).unwrap();
        assert_eq!(restored.updated_at_ms, 10);
    }

    #[test]
    fn valid_temporary_file_recovers_when_primary_and_backup_are_corrupt() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let value = journal("session-3");
        fs::create_dir_all(temp.path()).unwrap();
        let paths = JournalPaths::new(temp.path(), "session-3");
        fs::write(&paths.primary, b"broken").unwrap();
        fs::write(&paths.backup, b"also broken").unwrap();
        fs::write(&paths.temporary, serde_json::to_vec(&value).unwrap()).unwrap();

        assert_eq!(store.load("session-3").unwrap(), value);
        assert!(decode_journal(&fs::read(paths.primary).unwrap(), "session-3").is_ok());
    }

    #[test]
    fn all_corrupt_candidates_return_a_stable_error() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let paths = JournalPaths::new(temp.path(), "session-4");
        fs::write(paths.primary, b"broken").unwrap();
        fs::write(paths.backup, b"broken").unwrap();
        fs::write(paths.temporary, b"broken").unwrap();

        assert_eq!(
            store.load("session-4").unwrap_err().body().code,
            JOURNAL_INVALID
        );
    }

    #[test]
    fn non_terminal_scan_includes_failed_sessions_that_still_own_resources() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let mut complete = journal("complete");
        complete.state = SessionState::Stopped;
        complete.cleanup.stage = JournalCleanupStage::Complete;
        store.save(&complete).unwrap();
        let mut failed = journal("failed-owned");
        failed.state = SessionState::Failed;
        failed.resources.firewall_rules.push(OwnedFirewallRule {
            rule_name: "rule-1".to_owned(),
            state: ResourceOwnershipState::Owned,
        });
        store.save(&failed).unwrap();

        let found = store.list_non_terminal().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, "failed-owned");
    }

    struct Probe(Option<ObservedProcessIdentity>);

    impl WorkerProcessProbe for Probe {
        fn inspect(&mut self, _pid: u32) -> SimulatorResult<Option<ObservedProcessIdentity>> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn process_identity_rejects_pid_reuse() {
        let value = journal("session-5");
        let mut same = Probe(Some(ObservedProcessIdentity {
            pid: 42,
            creation_time_100ns: 9001,
            executable_identity: "worker-sha256".to_owned(),
        }));
        assert_eq!(
            inspect_worker_presence(&value, &mut same).unwrap(),
            WorkerPresence::SameProcess
        );

        let mut reused = Probe(Some(ObservedProcessIdentity {
            pid: 42,
            creation_time_100ns: 9002,
            executable_identity: "worker-sha256".to_owned(),
        }));
        assert_eq!(
            inspect_worker_presence(&value, &mut reused).unwrap(),
            WorkerPresence::PidReused
        );
    }

    #[derive(Default)]
    struct Cleaner {
        existing_firewall: HashSet<String>,
        existing_ips: HashSet<Ipv4Addr>,
        existing_packs: HashSet<(String, String)>,
        removed_firewall: Vec<String>,
        removed_ips: Vec<Ipv4Addr>,
        released_packs: Vec<(String, String)>,
        fail_ip_once: Option<Ipv4Addr>,
    }

    impl SessionResourceCleaner for Cleaner {
        fn stop_alarm_jobs(&mut self, _session_id: &str) -> SimulatorResult<()> {
            Ok(())
        }

        fn stop_services(&mut self, _session_id: &str) -> SimulatorResult<()> {
            Ok(())
        }

        fn firewall_rule_exists(&mut self, rule_name: &str) -> SimulatorResult<bool> {
            Ok(self.existing_firewall.contains(rule_name))
        }

        fn remove_firewall_rule(&mut self, rule_name: &str) -> SimulatorResult<()> {
            self.existing_firewall.remove(rule_name);
            self.removed_firewall.push(rule_name.to_owned());
            Ok(())
        }

        fn ip_address_exists(
            &mut self,
            _interface_id: &str,
            address: Ipv4Addr,
        ) -> SimulatorResult<bool> {
            Ok(self.existing_ips.contains(&address))
        }

        fn remove_ip_address(
            &mut self,
            _interface_id: &str,
            address: Ipv4Addr,
            _prefix_len: u8,
        ) -> SimulatorResult<()> {
            if self.fail_ip_once == Some(address) {
                self.fail_ip_once = None;
                return Err(SimulatorError::new(
                    "device_simulator.test.cleanup",
                    "deviceSimulator.errors.testCleanup",
                ));
            }
            self.existing_ips.remove(&address);
            self.removed_ips.push(address);
            Ok(())
        }

        fn pack_pin_exists(&mut self, pack_id: &str, version: &str) -> SimulatorResult<bool> {
            Ok(self
                .existing_packs
                .contains(&(pack_id.to_owned(), version.to_owned())))
        }

        fn release_pack_pin(&mut self, pack_id: &str, version: &str) -> SimulatorResult<()> {
            let pack = (pack_id.to_owned(), version.to_owned());
            self.existing_packs.remove(&pack);
            self.released_packs.push(pack);
            Ok(())
        }
    }

    #[test]
    fn partial_cleanup_is_persisted_and_retry_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let first_ip = Ipv4Addr::new(10, 0, 0, 2);
        let second_ip = Ipv4Addr::new(10, 0, 0, 3);
        let mut value = journal("recover-partial");
        value.resources.ip_addresses = vec![
            OwnedIpAddress {
                interface_id: "if-guid".to_owned(),
                address: first_ip,
                prefix_len: 24,
                state: ResourceOwnershipState::Owned,
            },
            OwnedIpAddress {
                interface_id: "if-guid".to_owned(),
                address: second_ip,
                prefix_len: 24,
                state: ResourceOwnershipState::Owned,
            },
        ];
        store.save(&value).unwrap();
        let mut cleaner = Cleaner {
            existing_ips: HashSet::from([first_ip, second_ip]),
            fail_ip_once: Some(second_ip),
            ..Cleaner::default()
        };

        let failed = store.recover_session(value, &mut cleaner, 100).unwrap();
        assert!(!failed.recovered);
        assert_eq!(failed.journal.state, SessionState::RecoveryRequired);
        assert_eq!(
            failed.journal.resources.ip_addresses[0].state,
            ResourceOwnershipState::Released
        );
        assert_eq!(
            store
                .load("recover-partial")
                .unwrap()
                .resources
                .ip_addresses[0]
                .state,
            ResourceOwnershipState::Released
        );

        let retried = store
            .recover_session(failed.journal, &mut cleaner, 200)
            .unwrap();
        assert!(retried.recovered);
        assert!(retried.remaining_resources.is_empty());
        assert_eq!(retried.journal.cleanup.stage, JournalCleanupStage::Complete);
        assert_eq!(retried.journal.state, SessionState::Stopped);
        assert_eq!(cleaner.removed_ips, vec![first_ip, second_ip]);
        assert_eq!(retried.journal.cleanup.attempts, 2);
    }

    #[test]
    fn cleanup_deletes_only_resources_that_are_owned_and_still_present() {
        let temp = TempDir::new().unwrap();
        let store = SessionJournalStore::new(temp.path());
        let present = Ipv4Addr::new(10, 0, 0, 2);
        let absent = Ipv4Addr::new(10, 0, 0, 3);
        let planned = Ipv4Addr::new(10, 0, 0, 4);
        let mut value = journal("recover-exact");
        value.resources.ip_addresses = vec![
            OwnedIpAddress {
                interface_id: "if-guid".to_owned(),
                address: present,
                prefix_len: 24,
                state: ResourceOwnershipState::Owned,
            },
            OwnedIpAddress {
                interface_id: "if-guid".to_owned(),
                address: absent,
                prefix_len: 24,
                state: ResourceOwnershipState::Owned,
            },
            OwnedIpAddress {
                interface_id: "if-guid".to_owned(),
                address: planned,
                prefix_len: 24,
                state: ResourceOwnershipState::Planned,
            },
        ];
        value.resources.firewall_rules = vec![
            OwnedFirewallRule {
                rule_name: "present-rule".to_owned(),
                state: ResourceOwnershipState::Owned,
            },
            OwnedFirewallRule {
                rule_name: "planned-rule".to_owned(),
                state: ResourceOwnershipState::Planned,
            },
        ];
        value.resources.packs.push(OwnedPack {
            id: "video".to_owned(),
            version: "1.0.0".to_owned(),
            state: ResourceOwnershipState::Owned,
        });
        let mut cleaner = Cleaner {
            existing_ips: HashSet::from([present]),
            existing_firewall: HashSet::from(["present-rule".to_owned()]),
            existing_packs: HashSet::from([("video".to_owned(), "1.0.0".to_owned())]),
            ..Cleaner::default()
        };

        let outcome = store.recover_session(value, &mut cleaner, 100).unwrap();
        assert!(outcome.recovered);
        assert_eq!(cleaner.removed_ips, vec![present]);
        assert_eq!(cleaner.removed_firewall, vec!["present-rule"]);
        assert_eq!(
            cleaner.released_packs,
            vec![("video".to_owned(), "1.0.0".to_owned())]
        );
    }
}
