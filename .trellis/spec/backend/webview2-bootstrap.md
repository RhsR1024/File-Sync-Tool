# WebView2 Bootstrap

> Contracts for the Windows pre-Tauri bootstrap that detects, downloads, verifies, installs, and recovers Microsoft Edge WebView2 Runtime before any Tauri window is created.

## Scenario: Pre-Tauri WebView2 Runtime recovery

### 1. Scope / Trigger

- Trigger: code under `src-tauri/src/main.rs`, `src-tauri/src/webview2_bootstrap/`, shared verified-download helpers, or startup config path resolution.
- Goal: keep the app launchable as a bare Windows `.exe` when WebView2 Runtime is missing, without depending on Vue UI or Tauri commands.
- Boundary: this bootstrap runs before `tauri::Builder::default()`. It must not require `AppHandle`, webview windows, Tauri plugins, or frontend events.

### 2. Signatures

- Rust entrypoint:
  - `fn main()`
  - calls `webview2_bootstrap::ensure_webview2_runtime()` after panic/startup logging is installed and before `tauri::Builder::default()`.
- Rust modules:
  - `src-tauri/src/webview2_bootstrap/mod.rs`
  - `src-tauri/src/webview2_bootstrap/detect.rs`
  - `src-tauri/src/webview2_bootstrap/startup_config.rs`
  - `src-tauri/src/webview2_bootstrap/server.rs`
  - `src-tauri/src/webview2_bootstrap/sha256_file.rs`
  - `src-tauri/src/webview2_bootstrap/download.rs`
  - `src-tauri/src/webview2_bootstrap/native_ui.rs`
  - `src-tauri/src/webview2_bootstrap/install.rs`
  - `src-tauri/src/webview2_bootstrap/restart.rs`
- Core API shape:
```rust
pub enum BootstrapOutcome {
    Continue,
    Exit,
}

pub fn ensure_webview2_runtime() -> BootstrapOutcome;
```
- Server assets:
  - `${update_server_url}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe`
  - `${update_server_url}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256`
- Environment keys:
  - `FST_SKIP_WEBVIEW2_BOOTSTRAP=1`
  - `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1`

### 3. Contracts

- Placement rules:
  - Bootstrap must run before any Tauri builder, plugin, tray, or window creation.
  - Bootstrap must run after startup/panic logging is installed.
  - Bootstrap must run before `single_instance_guard::ensure_single_instance()` because the guard mutex lives until process exit and would make the restarted child silently exit.
  - Bootstrap prevents concurrent installs with its own named mutex `com.filesync.tool-wv2-bootstrap`; a losing instance shows an info dialog and exits.
  - Non-Windows builds are no-op and return `BootstrapOutcome::Continue`.
- Detection rules:
  - On 64-bit Windows, check WebView2 Runtime `pv` at:
    - `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`
    - `HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`
  - Runtime is present when any checked `pv` value is non-empty and greater than `0.0.0.0`.
  - Missing keys, missing values, empty values, and `0.0.0.0` are missing.
- Config rules:
  - Startup config resolution must not require `AppHandle`.
  - Read default config from `%APPDATA%\com.filesync.tool\config.json`.
  - Read pivot from `%APPDATA%\com.filesync.tool\pivot.json`.
  - If `pivot.custom_data_dir` exists and points to a directory, read `<custom_data_dir>\config.json`.
  - Missing config or missing `update_server_url` uses `http://192.115.1.3:8080`.
  - Normalize by trimming whitespace and removing trailing `/`.
  - Empty URL is terminal failure only when WebView2 is missing.
  - Non-empty URL must parse as `http://` or `https://` and include a host.
- Server rules:
  - Runtime installer URL is `${base}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe`.
  - Hash URL is `${base}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256`.
  - `.sha256` accepts either a bare 64-char hex string or `64hex filename`.
  - Hash matching is case-insensitive.
- Download rules:
  - Download to `%TEMP%\file-sync-tool-webview2\MicrosoftEdgeWebView2RuntimeInstallerX64.exe.part`.
  - Verify SHA-256 before finalizing the installer path.
  - Delete `.part` on cancel, network error, IO error, or hash mismatch.
  - Progress goes to native bootstrap UI, not Tauri events.
- Native UI rules:
  - Missing Runtime asks for confirmation with Windows-native UI.
  - Download phase shows deterministic progress when possible and allows cancel.
  - Install phase shows indeterminate progress and does not allow cancel.
  - If progress UI creation fails, MessageBox phase prompts are the fallback.
  - Every terminal failure shows a native error dialog.
- Install rules:
  - Spawn installer with exactly `/silent /install`.
  - Wait for process completion.
  - Non-zero exit code is terminal failure.
  - Do not force UAC elevation.
  - After zero exit code, poll detection for up to 60 seconds.
- Restart rules:
  - Restart only after post-install Runtime detection succeeds.
  - Relaunch `std::env::current_exe()` with original args.
  - Set `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1` on the restarted process.
  - `FST_WEBVIEW2_BOOTSTRAP_RESTARTED` is consumed (read then `remove_var`) at bootstrap entry so it does not propagate to descendant processes.
  - If that env key is already set and Runtime is still missing, do not download/install again; show error and exit.

### 4. Validation & Error Matrix

| Case | Required behavior |
| --- | --- |
| WebView2 already present | Return `Continue`; show no bootstrap UI. |
| `FST_SKIP_WEBVIEW2_BOOTSTRAP=1` | Return `Continue`; log skip. |
| User rejects install | Log and return `Exit`. |
| Restarted process still missing Runtime | Show native error and return `Exit`; no second install attempt. |
| `update_server_url` empty | Show native error and return `Exit`. |
| `update_server_url` invalid | Show native error and return `Exit`. |
| `.sha256` download fails | Show native error and return `Exit`. |
| `.sha256` invalid | Show native error and return `Exit`. |
| Installer download fails | Delete `.part`, show native error, return `Exit`. |
| User cancels download | Delete `.part`, log cancellation, return `Exit`. |
| SHA-256 mismatch | Delete installer files, show native error, return `Exit`. |
| Progress UI creation fails | Continue with MessageBox fallback. |
| Installer spawn fails | Show native error and return `Exit`. |
| Installer exits non-zero | Show native error with exit code and return `Exit`. |
| Installer exits zero but Runtime not detected within 60 seconds | Show native error and return `Exit`. |
| Non-Windows build | Return `Continue`. |

### 5. Good / Base / Bad Cases

- Good: A clean Windows machine without WebView2 shows native confirmation, downloads from the configured internal update server, verifies SHA-256, installs silently, verifies Runtime, restarts the exe, then reaches the Tauri main window.
- Good: A machine with WebView2 already installed goes straight to Tauri without native bootstrap UI.
- Good: A custom data directory configured through `pivot.json` is honored before Tauri exists, so bootstrap uses the same `update_server_url` as Settings.
- Base: A missing config file falls back to `http://192.115.1.3:8080`.
- Base: Progress window creation failure does not abort the install; MessageBox fallback preserves user visibility.
- Bad: Calling this bootstrap from a Tauri command. Missing WebView2 can prevent commands from existing.
- Bad: Trusting a downloaded installer before SHA-256 verification succeeds.
- Bad: Persisting a boolean "webview2_installed" in config. Registry detection is the source of truth.
- Bad: Restarting without `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1`, which can create an install/restart loop.

### 6. Tests Required

- Rust:
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::detect`
    - assert empty, missing, and `0.0.0.0` versions are missing
    - assert normal version strings are present
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::startup_config`
    - assert default config path and pivot custom path resolution
    - assert missing config returns default URL
    - assert normalization trims trailing slash
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::server`
    - assert installer and hash URLs are joined without double slashes
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::sha256_file`
    - assert bare hash and `hash filename` formats parse
    - assert invalid hash content is rejected
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::download`
    - assert successful verified download finalizes file
    - assert cancel and hash mismatch delete partial files
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::install`
    - assert installer args are `/silent` and `/install`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::restart`
    - assert original args are preserved
    - assert `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1` is set
- Manual Windows smoke tests:
  - clean VM without WebView2
  - already-installed machine
  - unreachable server
  - empty update URL
  - wrong SHA-256
  - custom data dir
  - forced progress UI fallback
  - restart-loop guard

### 7. Wrong vs Correct

#### Wrong

```rust
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            webview2_bootstrap::ensure_webview2_runtime();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

#### Correct

```rust
fn main() {
    install_panic_log_hook();

    if matches!(
        webview2_bootstrap::ensure_webview2_runtime(),
        webview2_bootstrap::BootstrapOutcome::Exit
    ) {
        return;
    }

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

#### Wrong

```rust
let installer = download_file(installer_url).await?;
Command::new(installer).args(["/silent", "/install"]).status()?;
```

#### Correct

```rust
let expected = download_sha256(hash_url).await?;
let installer = download_and_verify(installer_url, expected, progress).await?;
let status = Command::new(installer).args(["/silent", "/install"]).status()?;
if !status.success() {
    return Err(format!("webview2 installer failed: {status}"));
}
```
