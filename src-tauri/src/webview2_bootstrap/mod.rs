//! Pre-Tauri WebView2 Runtime bootstrap. Runs before `tauri::Builder` and
//! before the single-instance guard; must not touch AppHandle/plugins.
//! Spec: docs/superpowers/specs/2026-07-09-webview2-bootstrap-design.md

pub mod detect;
pub mod download;
pub mod install;
pub mod native_ui;
pub mod restart;
pub mod server;
pub mod sha256_file;
pub mod startup_config;

use std::sync::Arc;
use std::time::Duration;

use native_ui::{Phase, ProgressState};

pub const SKIP_ENV: &str = "FST_SKIP_WEBVIEW2_BOOTSTRAP";
const BOOTSTRAP_MUTEX_NAME: &str = "com.filesync.tool-wv2-bootstrap";

#[derive(Debug, PartialEq, Eq)]
pub enum BootstrapOutcome {
    Continue,
    Exit,
}

#[derive(Debug, PartialEq, Eq)]
enum PreflightDecision {
    ContinueToApp,
    SkipRequested,
    FailRestartLoop,
    PromptInstall,
}

fn preflight(skip: bool, restarted: bool, detected: Option<&str>) -> PreflightDecision {
    if skip {
        return PreflightDecision::SkipRequested;
    }
    if detected.is_some() {
        return PreflightDecision::ContinueToApp;
    }
    if restarted {
        return PreflightDecision::FailRestartLoop;
    }
    PreflightDecision::PromptInstall
}

pub fn ensure_webview2_runtime() -> BootstrapOutcome {
    #[cfg(not(target_os = "windows"))]
    {
        BootstrapOutcome::Continue
    }
    #[cfg(target_os = "windows")]
    {
        windows_flow()
    }
}

#[cfg(target_os = "windows")]
fn windows_flow() -> BootstrapOutcome {
    let skip = std::env::var(SKIP_ENV)
        .map(|value| value == "1")
        .unwrap_or(false);
    let restarted = std::env::var(restart::RESTARTED_ENV)
        .map(|value| value == "1")
        .unwrap_or(false);
    std::env::remove_var(restart::RESTARTED_ENV);

    let detected = detect::detect_webview2_runtime();
    match preflight(skip, restarted, detected.as_deref()) {
        PreflightDecision::ContinueToApp => {
            crate::startup_log(
                "info",
                &format!(
                    "webview2 bootstrap: detected runtime pv={}",
                    detected.as_deref().unwrap_or("?")
                ),
            );
            BootstrapOutcome::Continue
        }
        PreflightDecision::SkipRequested => {
            crate::startup_log(
                "warn",
                "webview2 bootstrap: FST_SKIP_WEBVIEW2_BOOTSTRAP=1, skipping check",
            );
            BootstrapOutcome::Continue
        }
        PreflightDecision::FailRestartLoop => {
            crate::startup_log(
                "error",
                "webview2 bootstrap: runtime still missing after restart; stopping loop",
            );
            native_ui::show_error(
                "WebView2 Runtime is still missing after installation.\n\
                 Please contact your administrator.\n\n\
                 WebView2 Runtime 安装后仍未检测到，程序无法启动。\n\
                 请联系管理员检查内部更新服务器或手动安装 WebView2 Runtime。",
            );
            BootstrapOutcome::Exit
        }
        PreflightDecision::PromptInstall => install_flow(),
    }
}

#[cfg(target_os = "windows")]
fn install_flow() -> BootstrapOutcome {
    match acquire_bootstrap_mutex() {
        MutexState::Acquired => {}
        MutexState::AlreadyRunning => {
            native_ui::show_info(
                "Another File Sync Tool instance is already installing the WebView2 Runtime.\n\
                 This instance will exit.\n\n\
                 另一个 File Sync Tool 实例正在安装 WebView2 Runtime，本实例将退出。",
            );
            return BootstrapOutcome::Exit;
        }
        MutexState::Unavailable => {
            crate::startup_log(
                "warn",
                "webview2 bootstrap: bootstrap mutex unavailable; continuing without guard",
            );
        }
    }

    if !native_ui::confirm_install() {
        crate::startup_log("info", "webview2 bootstrap: user declined install");
        return BootstrapOutcome::Exit;
    }

    let base_url = match startup_config::resolve_update_server_url() {
        Ok(url) => url,
        Err(reason) => {
            crate::startup_log(
                "error",
                &format!("webview2 bootstrap: update server unavailable: {reason}"),
            );
            native_ui::show_error(&format!(
                "Cannot resolve the internal update server URL:\n{reason}\n\n\
                 无法确定内部更新服务器地址，请联系管理员。"
            ));
            return BootstrapOutcome::Exit;
        }
    };
    crate::startup_log(
        "info",
        &format!("webview2 bootstrap: using update server {base_url}"),
    );

    let state = Arc::new(ProgressState::default());
    let has_window = native_ui::try_create_progress_window(state.clone()).is_some();
    if !has_window {
        crate::startup_log(
            "warn",
            "webview2 bootstrap: progress window unavailable; using MessageBox fallback",
        );
        native_ui::show_info(
            "Downloading and installing the WebView2 Runtime. Please wait.\n\
             The app will restart automatically when installation finishes.\n\n\
             即将下载并安装 WebView2 Runtime，请稍候。完成后程序会自动重启。",
        );
    }

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let result: Arc<std::sync::Mutex<Option<Result<(), WorkerError>>>> =
        Arc::new(std::sync::Mutex::new(None));

    if has_window {
        let worker = {
            let state = state.clone();
            let result = result.clone();
            let base_url = base_url.clone();
            std::thread::spawn(move || {
                let outcome = worker_pipeline(&base_url, &state, cancel_rx, cancel_tx);
                *result.lock().unwrap() = Some(outcome);
                state.mark_done();
            })
        };
        native_ui::run_message_loop();
        let _ = worker.join();
    } else {
        let outcome = worker_pipeline(&base_url, &state, cancel_rx, cancel_tx);
        *result.lock().unwrap() = Some(outcome);
    }

    let outcome = result.lock().unwrap().take();
    let Some(outcome) = outcome else {
        let message = "WebView2 bootstrap worker aborted unexpectedly".to_string();
        crate::startup_log("error", &format!("webview2 bootstrap: {message}"));
        native_ui::show_error(&message);
        return BootstrapOutcome::Exit;
    };

    match outcome {
        Ok(()) => {
            state.set_phase(Phase::Restarting);
            restart::restart_and_exit();
        }
        Err(WorkerError::Cancelled) => {
            crate::startup_log("info", "webview2 bootstrap: user cancelled download");
            BootstrapOutcome::Exit
        }
        Err(WorkerError::Failed(message)) => {
            crate::startup_log(
                "error",
                &format!("webview2 bootstrap: installation failed: {message}"),
            );
            native_ui::show_error(&format!(
                "WebView2 Runtime installation failed.\n{message}\n\n\
                 Please contact your administrator and check the internal update server.\n\n\
                 WebView2 Runtime 安装失败，请联系管理员检查内部更新服务器。"
            ));
            BootstrapOutcome::Exit
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
enum WorkerError {
    Cancelled,
    Failed(String),
}

#[cfg(target_os = "windows")]
fn worker_pipeline(
    base_url: &str,
    state: &Arc<ProgressState>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
) -> Result<(), WorkerError> {
    state.set_phase(Phase::Downloading);
    let dir = download::default_download_dir();
    let installer = {
        let state = state.clone();
        download::download_installer_blocking(
            base_url,
            &dir,
            cancel_rx,
            move |downloaded, total| {
                state.set_progress(downloaded, total);
                if state.is_cancelled() {
                    let _ = cancel_tx.send(true);
                }
            },
        )
        .map_err(|error| match error {
            download::InstallerDownloadError::Cancelled => WorkerError::Cancelled,
            download::InstallerDownloadError::Failed(message) => WorkerError::Failed(message),
        })?
    };

    state.set_phase(Phase::Verifying);
    state.set_phase(Phase::Installing);
    crate::startup_log(
        "info",
        "webview2 bootstrap: download verified; starting silent installer",
    );
    install::run_silent_install(&installer).map_err(WorkerError::Failed)?;

    match install::wait_for_runtime(Duration::from_secs(60)) {
        Some(version) => {
            crate::startup_log(
                "info",
                &format!("webview2 bootstrap: installation succeeded pv={version}"),
            );
            Ok(())
        }
        None => Err(WorkerError::Failed(
            "runtime not detected within 60 seconds after installer exit".to_string(),
        )),
    }
}

#[cfg(target_os = "windows")]
enum MutexState {
    Acquired,
    AlreadyRunning,
    Unavailable,
}

#[cfg(target_os = "windows")]
fn acquire_bootstrap_mutex() -> MutexState {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = BOOTSTRAP_MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(_handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    MutexState::AlreadyRunning
                } else {
                    MutexState::Acquired
                }
            }
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                MutexState::AlreadyRunning
            }
            Err(_) => MutexState::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_env_bypasses_everything() {
        assert_eq!(
            preflight(true, false, None),
            PreflightDecision::SkipRequested
        );
        assert_eq!(
            preflight(true, true, Some("1.0.0.1")),
            PreflightDecision::SkipRequested
        );
    }

    #[test]
    fn present_runtime_continues() {
        assert_eq!(
            preflight(false, false, Some("109.0.1518.78")),
            PreflightDecision::ContinueToApp
        );
        assert_eq!(
            preflight(false, true, Some("109.0.1518.78")),
            PreflightDecision::ContinueToApp
        );
    }

    #[test]
    fn restarted_and_still_missing_fails_loop_guard() {
        assert_eq!(
            preflight(false, true, None),
            PreflightDecision::FailRestartLoop
        );
    }

    #[test]
    fn missing_runtime_prompts_install() {
        assert_eq!(
            preflight(false, false, None),
            PreflightDecision::PromptInstall
        );
    }
}
