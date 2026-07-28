use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize};

#[derive(Debug, Clone)]
pub struct WindowsCopyRequest {
    pub source: PathBuf,
    pub target: PathBuf,
    pub expected_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsCopyError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for WindowsCopyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "Windows copy was cancelled by the user"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

/// Live state published by the shell progress sink.
///
/// The shell can leave `PerformOperations` blocked forever when the user force-closes
/// the progress dialog (for example clicking the close box a second time while the
/// operation is already unwinding a cancel). The sink reports completion independently
/// of that call returning, so the caller can still resolve the copy to a terminal state
/// instead of pinning the task run in a "copying" phase until the app restarts.
#[derive(Default)]
struct CopyProgressState {
    bytes_done: AtomicU64,
    items_done: AtomicUsize,
    finished: AtomicBool,
    finish_hr: AtomicI32,
    /// Win32 thread id running `PerformOperations`, or 0 before it starts. The shell
    /// hosts its progress dialog on that thread, so the presence of a visible window
    /// there is what tells "dialog still up" apart from "user force-closed it".
    worker_thread_id: AtomicU32,
    /// Set when the app itself cancels the run (task list "cancel", app exit). The
    /// progress sink reads it and fails its next callback, which is the only way to
    /// stop the shell's copy engine and close its dialog from outside the dialog.
    cancel_requested: AtomicBool,
}

/// How long to keep waiting for `PerformOperations` after the sink reported that the
/// operation itself is over. Covers normal teardown without hanging on a wedged dialog.
const FINISH_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
/// How long to wait after the shell progress dialog disappears before resolving the
/// copy without `PerformOperations`. Long enough for a normal return to win the race,
/// short enough that a force-closed dialog never pins the run in "copying".
const DIALOG_GONE_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
/// How long to let the shell act on an app-side cancel before reporting the run
/// cancelled anyway. The sink aborts the operation on its next callback, so this only
/// has to cover teardown; a shell that ignores it must not pin the run either.
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_secs(10);
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// The copy engine's own "the user stopped this" code. Returned from the progress sink
/// to abort an in-flight shell operation, and recognised when it comes back out of
/// `PerformOperations`.
const COPYENGINE_E_USER_CANCELLED: i32 = 0x80270000u32 as i32;

fn is_cancel_hresult(code: i32) -> bool {
    const ERROR_CANCELLED_HRESULT: i32 = 0x800704C7u32 as i32;
    const E_ABORT: i32 = 0x80004004u32 as i32;
    matches!(
        code,
        ERROR_CANCELLED_HRESULT | COPYENGINE_E_USER_CANCELLED | E_ABORT
    )
}

/// Why a post-copy verification could not confirm every queued file.
#[derive(Debug)]
enum VerifyFailure {
    /// The target is missing or short — what a stopped copy leaves behind.
    Incomplete,
    /// Something else is wrong, and the copy really did fail.
    Failed(String),
}

/// Confirm every queued file landed at its expected size.
fn verify_copied(requests: &[WindowsCopyRequest]) -> Result<u64, VerifyFailure> {
    for request in requests {
        let source_size = std::fs::metadata(&request.source)
            .map_err(|error| {
                VerifyFailure::Failed(format!(
                    "Failed to verify source file '{}': {error}",
                    request.source.display()
                ))
            })?
            .len();
        if source_size != request.expected_size {
            return Err(VerifyFailure::Failed(format!(
                "Copy source '{}' changed while copying: expected {} bytes, source is now {} bytes",
                request.source.display(),
                request.expected_size,
                source_size
            )));
        }
        let Ok(target_size) = std::fs::metadata(&request.target).map(|metadata| metadata.len())
        else {
            return Err(VerifyFailure::Incomplete);
        };
        if target_size != request.expected_size {
            return Err(VerifyFailure::Incomplete);
        }
    }

    Ok(requests.iter().map(|request| request.expected_size).sum())
}

/// Verify the copy and report an unfinished result as a cancel.
///
/// The shell only leaves target files missing or short when the operation was stopped:
/// clicking the progress dialog's close box, including the force-close that can wedge
/// `PerformOperations` and make it report success. Those are user cancels, so the task
/// run must end "cancelled" rather than "failed".
fn verify_copy_outcome(requests: &[WindowsCopyRequest]) -> Result<u64, WindowsCopyError> {
    verify_copied(requests).map_err(|failure| match failure {
        VerifyFailure::Incomplete => WindowsCopyError::Cancelled,
        VerifyFailure::Failed(message) => WindowsCopyError::Failed(message),
    })
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{
        is_cancel_hresult, verify_copy_outcome, CopyProgressState, WindowsCopyError,
        WindowsCopyRequest, CANCEL_GRACE, COPYENGINE_E_USER_CANCELLED, DIALOG_GONE_GRACE,
        FINISH_GRACE, POLL_INTERVAL,
    };
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use windows::core::{implement, IUnknown, Result as WinResult, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    use windows::Win32::UI::Shell::{
        FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
        IShellItem, SHCreateItemFromParsingName, FOFX_EARLYFAILURE, FOFX_NOCOPYHOOKS,
        FOFX_SHOWELEVATIONPROMPT, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible,
    };

    /// Window class of the shell's file-operation progress dialog on Windows 8 and later.
    const PROGRESS_DIALOG_CLASS: &str = "OperationStatusWindow";

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    /// Shell callback that mirrors the operation's progress into `CopyProgressState`.
    ///
    /// Item sizes are taken from the queued request list by completion order: items are
    /// handed to `CopyItem` in order, so `PostCopyItem` reports them back in that order.
    /// This avoids querying the shell property system just to learn a file size.
    #[implement(IFileOperationProgressSink)]
    struct ProgressSink {
        state: Arc<CopyProgressState>,
        sizes: Vec<u64>,
    }

    impl ProgressSink {
        /// Fail the current callback once the app has asked for a cancel.
        ///
        /// The shell only stops a running operation when one of its sink callbacks
        /// returns a failure, so this is what makes the app's own cancel button close
        /// the Windows copy dialog instead of leaving it copying in the background.
        fn abort_if_cancelled(&self) -> WinResult<()> {
            if self.state.cancel_requested.load(Ordering::SeqCst) {
                return Err(windows::core::Error::from(HRESULT(
                    COPYENGINE_E_USER_CANCELLED,
                )));
            }
            Ok(())
        }
    }

    #[allow(non_snake_case)]
    impl IFileOperationProgressSink_Impl for ProgressSink_Impl {
        fn StartOperations(&self) -> WinResult<()> {
            self.abort_if_cancelled()
        }

        fn FinishOperations(&self, hrresult: HRESULT) -> WinResult<()> {
            self.state.finish_hr.store(hrresult.0, Ordering::SeqCst);
            self.state.finished.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn PreRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> WinResult<()> {
            Ok(())
        }

        fn PostRenameItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Option<&IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }

        fn PreMoveItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: Option<&IShellItem>,
            _: &PCWSTR,
        ) -> WinResult<()> {
            Ok(())
        }

        fn PostMoveItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: Option<&IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Option<&IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }

        fn PreCopyItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: Option<&IShellItem>,
            _: &PCWSTR,
        ) -> WinResult<()> {
            self.abort_if_cancelled()
        }

        fn PostCopyItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: Option<&IShellItem>,
            _: &PCWSTR,
            hrcopy: HRESULT,
            _: Option<&IShellItem>,
        ) -> WinResult<()> {
            let index = self.state.items_done.fetch_add(1, Ordering::SeqCst);
            if hrcopy.is_ok() {
                if let Some(size) = self.sizes.get(index) {
                    self.state.bytes_done.fetch_add(*size, Ordering::SeqCst);
                }
            }
            self.abort_if_cancelled()
        }

        fn PreDeleteItem(&self, _: u32, _: Option<&IShellItem>) -> WinResult<()> {
            Ok(())
        }

        fn PostDeleteItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: HRESULT,
            _: Option<&IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }

        fn PreNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> WinResult<()> {
            Ok(())
        }

        fn PostNewItem(
            &self,
            _: u32,
            _: Option<&IShellItem>,
            _: &PCWSTR,
            _: &PCWSTR,
            _: u32,
            _: HRESULT,
            _: Option<&IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }

        /// Called continuously while a single item is being written, so this is the
        /// callback that makes a cancel land mid-file instead of only between files.
        fn UpdateProgress(&self, _: u32, _: u32) -> WinResult<()> {
            self.abort_if_cancelled()
        }

        fn ResetTimer(&self) -> WinResult<()> {
            Ok(())
        }

        fn PauseTimer(&self) -> WinResult<()> {
            Ok(())
        }

        fn ResumeTimer(&self) -> WinResult<()> {
            Ok(())
        }
    }

    unsafe extern "system" fn note_visible_thread_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd).as_bool() {
            *(lparam.0 as *mut bool) = true;
            return BOOL(0);
        }
        BOOL(1)
    }

    unsafe extern "system" fn note_visible_progress_dialog(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut owner_pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));
        if owner_pid != GetCurrentProcessId() {
            return BOOL(1);
        }
        let mut class_name = [0u16; 64];
        let length = GetClassNameW(hwnd, &mut class_name);
        if length > 0
            && String::from_utf16_lossy(&class_name[..length as usize]) == PROGRESS_DIALOG_CLASS
        {
            *(lparam.0 as *mut bool) = true;
            return BOOL(0);
        }
        BOOL(1)
    }

    /// Whether the shell is still showing a progress dialog for this operation.
    ///
    /// Checked two ways because neither is guaranteed on its own: the dialog normally
    /// lives on the thread that called `PerformOperations`, but the shell is also free
    /// to fold several operations into one already-open status window. Reporting
    /// "present" whenever either check hits keeps a live copy from being mistaken for
    /// an abandoned one.
    fn progress_dialog_visible(worker_thread_id: u32) -> bool {
        let mut found = false;
        unsafe {
            let _ = EnumThreadWindows(
                worker_thread_id,
                Some(note_visible_thread_window),
                LPARAM(&mut found as *mut bool as isize),
            );
            if !found {
                let _ = EnumWindows(
                    Some(note_visible_progress_dialog),
                    LPARAM(&mut found as *mut bool as isize),
                );
            }
        }
        found
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(iter::once(0)).collect()
    }

    fn shell_item(path: &Path) -> Result<IShellItem, WindowsCopyError> {
        let path_wide = wide(path.as_os_str());
        unsafe {
            SHCreateItemFromParsingName(PCWSTR(path_wide.as_ptr()), None).map_err(|error| {
                WindowsCopyError::Failed(format!(
                    "Windows could not open shell item '{}': {error}",
                    path.display()
                ))
            })
        }
    }

    fn perform_copy(
        requests: &[WindowsCopyRequest],
        owner_hwnd: Option<isize>,
        state: Arc<CopyProgressState>,
    ) -> Result<u64, WindowsCopyError> {
        state
            .worker_thread_id
            .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

        let initialize_result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if initialize_result.is_err() {
            return Err(WindowsCopyError::Failed(format!(
                "Failed to initialize the Windows copy service: {initialize_result:?}"
            )));
        }
        let _apartment = ComApartment;

        let operation: IFileOperation = unsafe {
            CoCreateInstance(&FileOperation, None::<&IUnknown>, CLSCTX_INPROC_SERVER).map_err(
                |error| {
                    WindowsCopyError::Failed(format!(
                        "Failed to create the Windows copy operation: {error}"
                    ))
                },
            )?
        };

        unsafe {
            operation
                .SetOperationFlags(
                    FOF_NOCONFIRMATION
                        | FOF_NOCONFIRMMKDIR
                        | FOFX_EARLYFAILURE
                        | FOFX_NOCOPYHOOKS
                        | FOFX_SHOWELEVATIONPROMPT,
                )
                .map_err(|error| {
                    WindowsCopyError::Failed(format!(
                        "Failed to configure the Windows copy operation: {error}"
                    ))
                })?;

            // Owning the progress dialog to the app window keeps the shell in charge of its
            // lifetime. An unowned dialog can be torn down while the copy engine is still
            // unwinding a cancel, which wedges PerformOperations.
            if let Some(handle) = owner_hwnd {
                let _ = operation.SetOwnerWindow(HWND(handle as *mut _));
            }

            let sink: IFileOperationProgressSink = ProgressSink {
                state: Arc::clone(&state),
                sizes: requests
                    .iter()
                    .map(|request| request.expected_size)
                    .collect(),
            }
            .into();
            let _cookie = operation.Advise(&sink).map_err(|error| {
                WindowsCopyError::Failed(format!(
                    "Failed to attach the Windows copy progress sink: {error}"
                ))
            })?;

            // SetProgressMessage is unimplemented by Windows and returns E_NOTIMPL;
            // the standard progress dialog is shown by PerformOperations without it.
            for request in requests {
                let destination_parent = request.target.parent().ok_or_else(|| {
                    WindowsCopyError::Failed(format!(
                        "Copy target has no parent directory: {}",
                        request.target.display()
                    ))
                })?;
                std::fs::create_dir_all(destination_parent).map_err(|error| {
                    WindowsCopyError::Failed(format!(
                        "Failed to create target directory '{}': {error}",
                        destination_parent.display()
                    ))
                })?;

                let source_item = shell_item(&request.source)?;
                let destination_item = shell_item(destination_parent)?;
                operation
                    .CopyItem(
                        &source_item,
                        &destination_item,
                        PCWSTR::null(),
                        None::<&IFileOperationProgressSink>,
                    )
                    .map_err(|error| {
                        WindowsCopyError::Failed(format!(
                            "Failed to queue '{}' for Windows copy: {error}",
                            request.source.display()
                        ))
                    })?;
            }

            if let Err(error) = operation.PerformOperations() {
                // Closing the dialog surfaces here as a cancel HRESULT on most Windows
                // builds, and as an aborted flag on the rest. Both mean "user cancelled",
                // and neither should be reported as a copy failure.
                if is_cancel_hresult(error.code().0) {
                    return Err(WindowsCopyError::Cancelled);
                }
                return Err(WindowsCopyError::Failed(format!(
                    "Windows copy operation failed: {error}"
                )));
            }

            if operation
                .GetAnyOperationsAborted()
                .map_err(|error| {
                    WindowsCopyError::Failed(format!(
                        "Failed to read the Windows copy result: {error}"
                    ))
                })?
                .as_bool()
            {
                return Err(WindowsCopyError::Cancelled);
            }
        }

        verify_copy_outcome(requests)
    }

    /// Resolve the copy from sink state alone, for when `PerformOperations` is wedged.
    fn resolve_from_sink(
        requests: &[WindowsCopyRequest],
        state: &CopyProgressState,
    ) -> Result<u64, WindowsCopyError> {
        let finish_hr = state.finish_hr.load(Ordering::SeqCst);
        if is_cancel_hresult(finish_hr) {
            return Err(WindowsCopyError::Cancelled);
        }
        // A non-cancel failure HRESULT is reported as-is; otherwise fall back to checking
        // what actually landed on disk, which also catches a cancel the shell reported as
        // success-with-missing-files.
        if finish_hr != 0 && HRESULT(finish_hr).is_err() {
            return Err(WindowsCopyError::Failed(format!(
                "Windows copy operation failed: {}",
                windows::core::Error::from(HRESULT(finish_hr))
            )));
        }
        verify_copy_outcome(requests)
    }

    pub fn copy_files_with_dialog(
        requests: Vec<WindowsCopyRequest>,
        owner_hwnd: Option<isize>,
        should_cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(u64),
    ) -> Result<u64, WindowsCopyError> {
        if requests.is_empty() {
            return Ok(0);
        }
        // Exit can set cancellation after the caller's last preflight check but before
        // this function starts. Do not create a detached COM worker in that window: it
        // could begin writing files while the application is already shutting down.
        if should_cancel.load(Ordering::SeqCst) {
            return Err(WindowsCopyError::Cancelled);
        }

        let state = Arc::new(CopyProgressState::default());
        let (sender, receiver) = mpsc::channel();

        let worker_requests = requests.clone();
        let worker_state = Arc::clone(&state);
        // Detached rather than joined: a wedged shell dialog must not pin this task run.
        std::thread::Builder::new()
            .name("windows-native-copy".to_string())
            .spawn(move || {
                let result = perform_copy(&worker_requests, owner_hwnd, worker_state);
                let _ = sender.send(result);
            })
            .map_err(|error| {
                WindowsCopyError::Failed(format!(
                    "Failed to start the Windows copy thread: {error}"
                ))
            })?;

        let mut reported_bytes = 0u64;
        let mut finished_at: Option<std::time::Instant> = None;
        let mut dialog_seen = false;
        let mut dialog_gone_at: Option<std::time::Instant> = None;
        let mut cancel_requested_at: Option<std::time::Instant> = None;

        loop {
            match receiver.recv_timeout(POLL_INTERVAL) {
                Ok(result) => {
                    if let Ok(total) = &result {
                        on_progress(*total);
                    }
                    return result;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(WindowsCopyError::Failed(
                        "Windows copy thread ended unexpectedly".to_string(),
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }

            let bytes_done = state.bytes_done.load(Ordering::SeqCst);
            if bytes_done != reported_bytes {
                reported_bytes = bytes_done;
                on_progress(bytes_done);
            }

            // The app's own cancel button stays responsive even if the shell dialog is stuck.
            // Returning right away would abandon the shell operation, which then keeps
            // copying behind its own dialog after the task already reads as cancelled, so
            // first ask the progress sink to abort and give the copy engine a moment to
            // unwind and take its dialog down.
            if should_cancel.load(Ordering::SeqCst) {
                let requested_at = cancel_requested_at.get_or_insert_with(|| {
                    state.cancel_requested.store(true, Ordering::SeqCst);
                    std::time::Instant::now()
                });
                if requested_at.elapsed() >= CANCEL_GRACE {
                    return Err(WindowsCopyError::Cancelled);
                }
            }

            if state.finished.load(Ordering::SeqCst) {
                let since = finished_at.get_or_insert_with(std::time::Instant::now);
                if since.elapsed() >= FINISH_GRACE {
                    return resolve_from_sink(&requests, &state);
                }
            }

            // Force-closing the progress dialog — clicking its close box again while the
            // first click is still unwinding the cancel — can leave `PerformOperations`
            // blocked forever, and in that state the sink never reports FinishOperations
            // either. The dialog's own window is then the only remaining signal that the
            // operation is over, so once a dialog that was up disappears, resolve the copy
            // from what actually landed on disk instead of waiting on the wedged call.
            let worker_thread_id = state.worker_thread_id.load(Ordering::SeqCst);
            if worker_thread_id != 0 {
                if progress_dialog_visible(worker_thread_id) {
                    dialog_seen = true;
                    dialog_gone_at = None;
                } else if dialog_seen {
                    let since = dialog_gone_at.get_or_insert_with(std::time::Instant::now);
                    if since.elapsed() >= DIALOG_GONE_GRACE {
                        return resolve_from_sink(&requests, &state);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform::copy_files_with_dialog;

#[cfg(not(target_os = "windows"))]
pub fn copy_files_with_dialog(
    _requests: Vec<WindowsCopyRequest>,
    _owner_hwnd: Option<isize>,
    _should_cancel: &AtomicBool,
    _on_progress: &mut dyn FnMut(u64),
) -> Result<u64, WindowsCopyError> {
    Err(WindowsCopyError::Failed(
        "Windows native copy mode is only available on Windows".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{verify_copy_outcome, WindowsCopyError, WindowsCopyRequest};
    #[cfg(target_os = "windows")]
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    fn request(
        source: std::path::PathBuf,
        target: std::path::PathBuf,
        size: u64,
    ) -> WindowsCopyRequest {
        WindowsCopyRequest {
            source,
            target,
            expected_size: size,
        }
    }

    #[test]
    fn a_fully_copied_file_verifies() {
        let root = tempdir().unwrap();
        let source = root.path().join("build.tar.gz");
        let target = root.path().join("copy/build.tar.gz");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&source, b"payload").unwrap();
        std::fs::write(&target, b"payload").unwrap();

        let total = verify_copy_outcome(&[request(source, target, 7)]).unwrap();

        assert_eq!(total, 7);
    }

    #[test]
    fn a_missing_target_reads_as_a_cancel() {
        let root = tempdir().unwrap();
        let source = root.path().join("build.tar.gz");
        std::fs::write(&source, b"payload").unwrap();

        let error =
            verify_copy_outcome(&[request(source, root.path().join("copy/build.tar.gz"), 7)])
                .unwrap_err();

        assert_eq!(error, WindowsCopyError::Cancelled);
    }

    #[test]
    fn a_partially_written_target_reads_as_a_cancel() {
        let root = tempdir().unwrap();
        let source = root.path().join("build.tar.gz");
        let target = root.path().join("build-copy.tar.gz");
        std::fs::write(&source, b"payload").unwrap();
        std::fs::write(&target, b"pay").unwrap();

        let error = verify_copy_outcome(&[request(source, target, 7)]).unwrap_err();

        assert_eq!(error, WindowsCopyError::Cancelled);
    }

    #[test]
    fn a_source_that_changed_mid_copy_is_still_a_failure() {
        let root = tempdir().unwrap();
        let source = root.path().join("build.tar.gz");
        let target = root.path().join("build-copy.tar.gz");
        std::fs::write(&source, b"payload-grew").unwrap();
        std::fs::write(&target, b"payload").unwrap();

        let error = verify_copy_outcome(&[request(source, target, 7)]).unwrap_err();

        assert!(matches!(error, WindowsCopyError::Failed(_)));
    }

    #[test]
    fn an_unreadable_source_is_still_a_failure() {
        let root = tempdir().unwrap();

        let error = verify_copy_outcome(&[request(
            root.path().join("gone.tar.gz"),
            root.path().join("copy.tar.gz"),
            7,
        )])
        .unwrap_err();

        assert!(matches!(error, WindowsCopyError::Failed(_)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_pre_cancelled_shell_copy_does_not_start_a_worker() {
        let root = tempdir().unwrap();
        let source = root.path().join("missing-source.bin");
        let target = root.path().join("copy.bin");
        let should_cancel = AtomicBool::new(true);
        let mut progress_called = false;

        let result = super::copy_files_with_dialog(
            vec![request(source, target.clone(), 7)],
            None,
            &should_cancel,
            &mut |_| progress_called = true,
        );

        assert_eq!(result, Err(WindowsCopyError::Cancelled));
        assert!(!progress_called);
        assert!(!target.exists());
    }
}
