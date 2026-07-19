use std::path::{Path, PathBuf};

const WORKER_FLAG: &str = "--simulator-worker";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerLaunchSpec {
    pub executable: PathBuf,
    pub session_id: String,
    pub pipe_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationErrorKind {
    Unsupported,
    InvalidLaunchSpec,
    UacCancelled,
    LaunchFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevationError {
    pub kind: ElevationErrorKind,
    pub public_details: String,
}

impl WorkerLaunchSpec {
    pub fn new(
        executable: impl Into<PathBuf>,
        session_id: impl Into<String>,
        pipe_name: impl Into<String>,
    ) -> Result<Self, ElevationError> {
        let spec = Self {
            executable: executable.into(),
            session_id: session_id.into(),
            pipe_name: pipe_name.into(),
        };
        if !spec.executable.is_absolute()
            || !valid_identifier(&spec.session_id, 128)
            || !valid_identifier(&spec.pipe_name, 180)
        {
            return Err(error(
                ElevationErrorKind::InvalidLaunchSpec,
                "worker launch identifiers or executable path are invalid",
            ));
        }
        Ok(spec)
    }

    pub fn arguments(&self) -> String {
        // Identifiers are restricted to a shell-neutral ASCII subset. Secrets
        // and user-provided free text are intentionally forbidden here.
        format!(
            "{WORKER_FLAG} --session-id {} --pipe-name {}",
            self.session_id, self.pipe_name
        )
    }
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn error(kind: ElevationErrorKind, details: impl Into<String>) -> ElevationError {
    ElevationError {
        kind,
        public_details: details.into(),
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct ElevatedWorkerProcess {
    handle: windows::Win32::Foundation::HANDLE,
    process_id: u32,
}

// A process HANDLE is a reference to a kernel object and may be waited on or
// closed from a different thread. Ownership remains unique in this wrapper;
// it is never exposed as a borrowed mutable raw pointer.
#[cfg(target_os = "windows")]
unsafe impl Send for ElevatedWorkerProcess {}

#[cfg(target_os = "windows")]
impl ElevatedWorkerProcess {
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Returns `None` while the process is active, otherwise its exit code.
    pub fn try_exit_code(&self) -> Result<Option<u32>, ElevationError> {
        use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};

        let wait = unsafe { WaitForSingleObject(self.handle, 0) };
        if wait == WAIT_TIMEOUT {
            return Ok(None);
        }
        if wait != WAIT_OBJECT_0 {
            return Err(error(
                ElevationErrorKind::LaunchFailed,
                "could not query elevated worker process state",
            ));
        }
        let mut exit_code = 0;
        unsafe { GetExitCodeProcess(self.handle, &mut exit_code) }.map_err(|source| {
            error(
                ElevationErrorKind::LaunchFailed,
                format!("could not read elevated worker exit code: {source}"),
            )
        })?;
        Ok(Some(exit_code))
    }
}

#[cfg(target_os = "windows")]
impl Drop for ElevatedWorkerProcess {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(target_os = "windows")]
pub fn launch_elevated_worker(
    spec: &WorkerLaunchSpec,
) -> Result<ElevatedWorkerProcess, ElevationError> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_CANCELLED};
    use windows::Win32::System::Threading::GetProcessId;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |value: &std::ffi::OsStr| {
        value
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let verb = wide(std::ffi::OsStr::new("runas"));
    let executable = wide(spec.executable.as_os_str());
    let arguments = wide(std::ffi::OsStr::new(&spec.arguments()));
    let working_directory = spec.executable.parent().unwrap_or_else(|| Path::new("."));
    let working_directory = wide(working_directory.as_os_str());
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(arguments.as_ptr()),
        lpDirectory: PCWSTR(working_directory.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    if let Err(source) = unsafe { ShellExecuteExW(&mut execute) } {
        let kind = if unsafe { GetLastError() } == ERROR_CANCELLED {
            ElevationErrorKind::UacCancelled
        } else {
            ElevationErrorKind::LaunchFailed
        };
        return Err(error(
            kind,
            format!("elevated worker launch failed: {source}"),
        ));
    }
    let process_id = unsafe { GetProcessId(execute.hProcess) };
    if process_id == 0 {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(execute.hProcess) };
        return Err(error(
            ElevationErrorKind::LaunchFailed,
            "elevated worker process id is unavailable",
        ));
    }
    Ok(ElevatedWorkerProcess {
        handle: execute.hProcess,
        process_id,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn launch_elevated_worker(_spec: &WorkerLaunchSpec) -> Result<(), ElevationError> {
    Err(error(
        ElevationErrorKind::Unsupported,
        "elevated worker launch is only supported on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_are_minimal_and_secret_free() {
        let executable = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\File Sync Tool\file-sync-tool.exe")
        } else {
            PathBuf::from("/opt/file-sync-tool")
        };
        let spec = WorkerLaunchSpec::new(executable, "session-123", "fst-simulator-456").unwrap();
        assert_eq!(
            spec.arguments(),
            "--simulator-worker --session-id session-123 --pipe-name fst-simulator-456"
        );
        assert!(!spec.arguments().contains("password"));
        assert!(WorkerLaunchSpec::new(PathBuf::from("relative.exe"), "s", "p").is_err());
        assert!(WorkerLaunchSpec::new(
            if cfg!(windows) {
                PathBuf::from(r"C:\app.exe")
            } else {
                PathBuf::from("/app")
            },
            "bad session",
            "pipe"
        )
        .is_err());
    }
}
