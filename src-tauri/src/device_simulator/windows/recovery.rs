use crate::device_simulator::api::RecoveryResult;
use crate::device_simulator::errors::{SimulatorError, SimulatorResult};
use crate::device_simulator::session_journal::{
    SessionJournalStore, SessionResourceCleaner, WorkerProcessIdentity,
};
use crate::device_simulator::windows::firewall::{FirewallBackend, SystemFirewallBackend};
use crate::device_simulator::windows::interfaces::list_system_interfaces;
use crate::device_simulator::windows::ip_alias::{IpAliasBackend, SystemIpAliasBackend};
use std::net::Ipv4Addr;
use std::path::Path;

#[cfg(target_os = "windows")]
use std::fs::File;
#[cfg(target_os = "windows")]
use std::io::Read;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE};

#[cfg(target_os = "windows")]
struct ProcessHandle(HANDLE);

#[cfg(target_os = "windows")]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub fn recover_recorded_session(
    app_data_dir: &Path,
    session_id: &str,
) -> SimulatorResult<RecoveryResult> {
    let store = SessionJournalStore::from_app_data_dir(app_data_dir);
    let mut journal = store.load(session_id)?;
    reconcile_recorded_worker(journal.worker_process.as_ref())?;
    journal.worker_process = None;
    // Repair a stale cleanup stage before persisting, so the write guard in
    // `save` accepts the journal and recovery can go on to release the owned
    // resources. Without this, a residual journal that recorded `Complete`
    // while still owning resources would be rejected here and could never be
    // cleaned up.
    journal.reconcile_cleanup_stage();
    store.save(&journal)?;

    let mut cleaner = SystemSessionCleaner::default();
    let outcome = store.recover_session(journal, &mut cleaner, now_ms())?;
    Ok(RecoveryResult {
        session_id: outcome.journal.session_id,
        recovered: outcome.recovered,
        remaining_resources: outcome.remaining_resources,
        error: outcome.error,
    })
}

#[cfg(target_os = "windows")]
fn reconcile_recorded_worker(expected: Option<&WorkerProcessIdentity>) -> SimulatorResult<()> {
    use windows::Win32::Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
        WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    let Some(expected) = expected else {
        return Ok(());
    };
    let handle = match unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
            false,
            expected.pid,
        )
    } {
        Ok(handle) => ProcessHandle(handle),
        Err(source)
            if source.code() == windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) =>
        {
            return Ok(())
        }
        Err(source) => {
            return Err(recovery_error(
                "device_simulator.recovery.worker_inspect_failed",
                format!(
                    "could not open recorded Worker process {}: {source}",
                    expected.pid
                ),
            ))
        }
    };

    let wait = unsafe { WaitForSingleObject(handle.0, 0) };
    if wait == WAIT_OBJECT_0 {
        return Ok(());
    }
    if wait != WAIT_TIMEOUT {
        return Err(recovery_error(
            "device_simulator.recovery.worker_inspect_failed",
            format!("could not query recorded Worker process {}", expected.pid),
        ));
    }

    let mut creation = windows::Win32::Foundation::FILETIME::default();
    let mut exit = windows::Win32::Foundation::FILETIME::default();
    let mut kernel = windows::Win32::Foundation::FILETIME::default();
    let mut user = windows::Win32::Foundation::FILETIME::default();
    unsafe { GetProcessTimes(handle.0, &mut creation, &mut exit, &mut kernel, &mut user) }
        .map_err(|source| {
            recovery_error(
                "device_simulator.recovery.worker_inspect_failed",
                format!("could not read recorded Worker process times: {source}"),
            )
        })?;
    let creation_time_100ns =
        (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if creation_time_100ns != expected.creation_time_100ns {
        return Ok(());
    }

    let mut executable_buffer = vec![0_u16; 32_768];
    let mut executable_length = executable_buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(executable_buffer.as_mut_ptr()),
            &mut executable_length,
        )
    }
    .map_err(|source| {
        recovery_error(
            "device_simulator.recovery.worker_inspect_failed",
            format!("could not resolve recorded Worker executable: {source}"),
        )
    })?;
    let executable = std::path::PathBuf::from(String::from_utf16_lossy(
        &executable_buffer[..executable_length as usize],
    ));
    if executable_sha256(&executable)? != expected.executable_identity {
        return Ok(());
    }

    unsafe { TerminateProcess(handle.0, 1) }.map_err(|source| {
        recovery_error(
            "device_simulator.recovery.worker_stop_failed",
            format!(
                "could not stop orphaned Worker process {}: {source}",
                expected.pid
            ),
        )
    })?;
    if unsafe { WaitForSingleObject(handle.0, 5_000) } != WAIT_OBJECT_0 {
        return Err(recovery_error(
            "device_simulator.recovery.worker_stop_failed",
            format!("orphaned Worker process {} did not exit", expected.pid),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn reconcile_recorded_worker(_expected: Option<&WorkerProcessIdentity>) -> SimulatorResult<()> {
    Err(recovery_error(
        "device_simulator.recovery.unsupported_platform",
        "virtual device recovery is only supported on Windows",
    ))
}

#[cfg(target_os = "windows")]
fn executable_sha256(path: &Path) -> SimulatorResult<String> {
    use sha2::{Digest, Sha256};

    let mut file = File::open(path).map_err(|source| {
        recovery_error(
            "device_simulator.recovery.worker_inspect_failed",
            format!("could not open recorded Worker executable: {source}"),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = file.read(&mut buffer).map_err(|source| {
            recovery_error(
                "device_simulator.recovery.worker_inspect_failed",
                format!("could not hash recorded Worker executable: {source}"),
            )
        })?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Default)]
struct SystemSessionCleaner {
    firewall: SystemFirewallBackend,
    ip_alias: SystemIpAliasBackend,
}

impl SessionResourceCleaner for SystemSessionCleaner {
    fn stop_alarm_jobs(&mut self, _session_id: &str) -> SimulatorResult<()> {
        Ok(())
    }

    fn stop_services(&mut self, _session_id: &str) -> SimulatorResult<()> {
        Ok(())
    }

    fn firewall_rule_exists(&mut self, rule_name: &str) -> SimulatorResult<bool> {
        self.firewall
            .list_managed_rules()
            .map(|rules| {
                rules
                    .iter()
                    .any(|rule| rule.name == rule_name || rule.rule_id == rule_name)
            })
            .map_err(|source| {
                recovery_error(
                    "device_simulator.recovery.firewall_query_failed",
                    source.to_string(),
                )
            })
    }

    fn remove_firewall_rule(&mut self, rule_name: &str) -> SimulatorResult<()> {
        let rules = self.firewall.list_managed_rules().map_err(|source| {
            recovery_error(
                "device_simulator.recovery.firewall_query_failed",
                source.to_string(),
            )
        })?;
        let Some(rule) = rules
            .iter()
            .find(|rule| rule.name == rule_name || rule.rule_id == rule_name)
        else {
            return Ok(());
        };
        self.firewall.delete_rule(&rule.rule_id).map_err(|source| {
            recovery_error(
                "device_simulator.recovery.firewall_remove_failed",
                source.to_string(),
            )
        })
    }

    fn ip_address_exists(
        &mut self,
        interface_id: &str,
        address: Ipv4Addr,
    ) -> SimulatorResult<bool> {
        let interfaces = list_system_interfaces().map_err(|source| {
            recovery_error(
                "device_simulator.recovery.interface_query_failed",
                source.to_string(),
            )
        })?;
        Ok(interfaces.iter().any(|interface| {
            interface.id.as_str() == interface_id
                && interface
                    .ipv4_addresses
                    .iter()
                    .any(|item| item.address == address)
        }))
    }

    fn remove_ip_address(
        &mut self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> SimulatorResult<()> {
        self.ip_alias
            .remove_alias(interface_id, address, prefix_len)
            .map_err(|source| {
                recovery_error(
                    "device_simulator.recovery.ip_remove_failed",
                    source.to_string(),
                )
            })
    }

    fn pack_pin_exists(&mut self, _pack_id: &str, _version: &str) -> SimulatorResult<bool> {
        Ok(false)
    }

    fn release_pack_pin(&mut self, _pack_id: &str, _version: &str) -> SimulatorResult<()> {
        Ok(())
    }
}

fn recovery_error(code: &'static str, details: impl Into<String>) -> SimulatorError {
    SimulatorError::new(code, "deviceSimulator.errors.recoveryFailed").with_public_details(details)
}

fn now_ms() -> u64 {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
