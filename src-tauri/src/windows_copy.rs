use std::path::PathBuf;

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

#[cfg(target_os = "windows")]
mod platform {
    use super::{WindowsCopyError, WindowsCopyRequest};
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::core::{IUnknown, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOperation, IFileOperation, IFileOperationProgressSink, IShellItem,
        SHCreateItemFromParsingName, FOFX_EARLYFAILURE, FOFX_NOCOPYHOOKS, FOFX_SHOWELEVATIONPROMPT,
        FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR,
    };

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
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

    fn perform_copy(requests: &[WindowsCopyRequest]) -> Result<u64, WindowsCopyError> {
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

            let progress_message = wide(OsStr::new("File Sync Tool"));
            operation
                .SetProgressMessage(PCWSTR(progress_message.as_ptr()))
                .map_err(|error| {
                    WindowsCopyError::Failed(format!(
                        "Failed to configure the Windows copy dialog: {error}"
                    ))
                })?;

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

            operation.PerformOperations().map_err(|error| {
                WindowsCopyError::Failed(format!("Windows copy operation failed: {error}"))
            })?;

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

        for request in requests {
            let source_size = std::fs::metadata(&request.source)
                .map_err(|error| {
                    WindowsCopyError::Failed(format!(
                        "Failed to verify source file '{}': {error}",
                        request.source.display()
                    ))
                })?
                .len();
            let target_size = std::fs::metadata(&request.target)
                .map_err(|error| {
                    WindowsCopyError::Failed(format!(
                        "Windows copy did not create '{}': {error}",
                        request.target.display()
                    ))
                })?
                .len();
            if source_size != request.expected_size || target_size != request.expected_size {
                return Err(WindowsCopyError::Failed(format!(
                    "Windows copy size verification failed for '{}': expected {} bytes, source {} bytes, target {} bytes",
                    request.target.display(),
                    request.expected_size,
                    source_size,
                    target_size
                )));
            }
        }

        Ok(requests.iter().map(|request| request.expected_size).sum())
    }

    pub fn copy_files_with_dialog(
        requests: Vec<WindowsCopyRequest>,
    ) -> Result<u64, WindowsCopyError> {
        if requests.is_empty() {
            return Ok(0);
        }

        std::thread::Builder::new()
            .name("windows-native-copy".to_string())
            .spawn(move || perform_copy(&requests))
            .map_err(|error| {
                WindowsCopyError::Failed(format!(
                    "Failed to start the Windows copy thread: {error}"
                ))
            })?
            .join()
            .map_err(|_| WindowsCopyError::Failed("Windows copy thread panicked".to_string()))?
    }
}

#[cfg(target_os = "windows")]
pub use platform::copy_files_with_dialog;

#[cfg(not(target_os = "windows"))]
pub fn copy_files_with_dialog(_requests: Vec<WindowsCopyRequest>) -> Result<u64, WindowsCopyError> {
    Err(WindowsCopyError::Failed(
        "Windows native copy mode is only available on Windows".to_string(),
    ))
}
