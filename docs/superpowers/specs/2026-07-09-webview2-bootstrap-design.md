# WebView2 Runtime Bootstrap Design Spec

- **Date**: 2026-07-09
- **Status**: Approved for planning
- **Owner**: codex-agent
- **Scope**: Windows pre-Tauri startup bootstrap for detecting, downloading, verifying, installing, and recovering the Microsoft Edge WebView2 Runtime when the app is distributed as a bare `.exe`.

---

## 1. Background and Goal

File Sync Tool is commonly distributed as a bare Windows executable. Tauri on Windows depends on Microsoft Edge WebView2 Runtime. If the Runtime is missing, the app can fail before the Vue UI, Tauri commands, updater dialog, or settings page can be shown.

The goal is to make a double-clicked bare `.exe` self-recover:

```text
double-click exe
-> Rust native startup code runs first
-> detect WebView2 Runtime
-> present: continue into the Tauri main window
-> missing:
   -> show a Windows native confirmation dialog
   -> user confirms
   -> download the WebView2 Runtime installer from the internal update server
   -> verify SHA-256
   -> run silent install
   -> verify Runtime again
   -> restart the current exe
```

Microsoft's WebView2 deployment guidance recommends checking whether the Runtime is present before creating a WebView2 control. It documents registry `pv` detection and silent installation for the Evergreen Standalone Installer using `MicrosoftEdgeWebView2RuntimeInstaller{X64/X86/ARM64}.exe /silent /install`.

Source: [Microsoft WebView2 Runtime distribution docs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)

---

## 2. Decisions

### 2.1 Chosen Approach

Use a pre-Tauri Rust bootstrap that runs before `tauri::Builder::default()`.

This is the only reliable layer because a missing Runtime can prevent all Tauri UI from existing. The bootstrap must use only Win32, standard Rust, registry access, filesystem IO, networking, process spawning, and startup logging.

### 2.2 Download Source

Reuse the existing `update_server_url` from app config.

The server contract is:

```text
${update_server_url}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe
${update_server_url}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256
```

The `webview2/` subdirectory keeps Runtime assets separate from the app update `manifest.json` and versioned app executables.

### 2.3 Architecture Scope

Implement x64 installer download in the first version, while keeping module names and server path construction extensible for X86 and ARM64 later.

Do not embed the WebView2 installer beside the app executable in this iteration.

### 2.4 Failure Policy

Fail closed. If WebView2 is missing and bootstrap cannot complete download, verification, installation, or post-install detection, show a native error dialog, write startup logs, and exit without starting Tauri.

Do not provide a native URL input form. The user should contact an administrator or fix the internal update server.

### 2.5 User Experience

Prefer a native Windows progress window:

- Confirm before installing.
- Show deterministic progress during download.
- Allow cancel during download.
- Show indeterminate progress during silent installation.
- Show an error dialog on failure.
- Automatically restart after success.

If the progress window cannot be created, degrade to MessageBox-based phase prompts.

### 2.6 Single-Instance Interaction (review amendment)

`main()` already runs `single_instance_guard::ensure_single_instance()` whose
guard mutex handle is intentionally leaked until process exit. The bootstrap
therefore runs BEFORE the single-instance guard:

- If bootstrap ran after the guard, the restarted child would race the dying
  parent's guard mutex, take the `notify_primary_and_exit` path, fail to find
  the plugin's hidden window because the bootstrap parent never created Tauri
  windows, and silently exit. The app would never come back after install.
- Bootstrap protects itself against double-launch with its own named mutex
  `com.filesync.tool-wv2-bootstrap` (created only when the Runtime is
  missing). A second instance during install shows an info dialog and exits.
- `FST_WEBVIEW2_BOOTSTRAP_RESTARTED` is read once at bootstrap entry and
  immediately removed from the process environment so it never propagates to
  descendants, such as the updater.bat restart chain.
- The confirmation dialog uses `MessageBoxW` `MB_YESNO` (Yes = install). The
  custom button labels from 7.1 would require TaskDialog/comctl32 v6 and are
  intentionally not used at this fragile pre-UI stage.
- The async download helper requires a Tokio runtime; bootstrap builds its own
  current-thread runtime and `block_on`s it because Tauri's runtime does not
  exist yet.

---

## 3. Startup Flow

```text
main()
-> install_panic_log_hook()
-> webview2_bootstrap::ensure_webview2_runtime()
   -> Continue: proceed
   -> Exit: return from main
-> single_instance_guard::ensure_single_instance()
-> tauri::Builder::default()
```

Detailed flow:

```text
1. Check `FST_SKIP_WEBVIEW2_BOOTSTRAP`.
   - If set to `1`, skip bootstrap. This is for local developer and test escape hatches only.

2. Detect WebView2 Runtime.
   - If present, continue with no UI.
   - If missing, continue to native prompt.

3. If `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1` is set and Runtime is still missing:
   - log the repeated failure
   - show a native error dialog
   - exit

4. Show a native confirmation dialog:
   "File Sync Tool requires Microsoft Edge WebView2 Runtime to start.
    The component was not detected on this computer.
    Install it now from the internal update server?"

5. If the user rejects:
   - log
   - exit

6. Resolve startup config and `update_server_url`.
   - empty or invalid URL fails with a native error dialog

7. Download `.sha256`, parse expected hash, then download installer to temp `.part`.

8. Verify SHA-256 while streaming download or immediately after download.
   - mismatch deletes downloaded files and exits with error dialog

9. Run:
   `MicrosoftEdgeWebView2RuntimeInstallerX64.exe /silent /install`

10. Wait for installer exit.
    - non-zero exit is an error

11. Poll Runtime detection for up to 60 seconds.
    - success restarts the current exe
    - failure shows native error dialog and exits

12. Restart `std::env::current_exe()` with original args and:
    `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1`

13. Exit the current process.
```

---

## 4. Module Layout

```text
src-tauri/src/webview2_bootstrap/
|-- mod.rs
|-- detect.rs
|-- startup_config.rs
|-- server.rs
|-- sha256_file.rs
|-- download.rs
|-- native_ui.rs
|-- install.rs
`-- restart.rs
```

Responsibilities:

- `mod.rs`: public orchestration API.
- `detect.rs`: registry-based WebView2 Runtime detection.
- `startup_config.rs`: load `update_server_url` before any `AppHandle` exists.
- `server.rs`: construct WebView2 asset URLs from the normalized update server URL.
- `sha256_file.rs`: parse `.sha256` file content.
- `download.rs`: download installer with progress and SHA-256 verification, reusing or extracting updater download helpers.
- `native_ui.rs`: native confirmation, error dialog, and progress window with MessageBox fallback.
- `install.rs`: spawn and wait for the silent installer.
- `restart.rs`: restart current executable with original args and loop-prevention env var.

If a generic downloader is extracted from `src-tauri/src/updater/download.rs`, place it in a neutral module such as `src-tauri/src/download_verify.rs` so the updater and WebView2 bootstrap can share it without semantic coupling.

---

## 5. Runtime Detection

Detection follows Microsoft WebView2 guidance by checking the WebView2 Runtime client registry `pv` value.

For 64-bit Windows in the first implementation:

```text
HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}
  pv

HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}
  pv
```

Rules:

- Runtime is present if any checked `pv` value is a non-empty version greater than `0.0.0.0`.
- Missing key, missing value, empty string, and `0.0.0.0` all mean missing.
- The detected version is written to startup logs.

Future architecture support can add:

```text
HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}
```

and choose installer filenames based on platform architecture.

---

## 6. Startup Config

The bootstrap cannot call `config::load_config(app.handle())` because no Tauri app exists yet.

It must resolve the same effective `update_server_url` with pure filesystem logic:

```text
default config root:
  %APPDATA%\com.filesync.tool\config.json

pivot file:
  %APPDATA%\com.filesync.tool\pivot.json

custom config root:
  <pivot.custom_data_dir>\config.json
```

Rules:

- If `pivot.json` has `custom_data_dir` and the directory exists, read config from `<custom_data_dir>\config.json`.
- Otherwise read the default config path.
- If config is missing or `update_server_url` is missing, use the existing default `http://192.115.1.3:8080`.
- Normalize by trimming whitespace and removing trailing `/`.
- Empty URL is allowed in settings, but bootstrap treats it as not configured and exits with a native error if WebView2 is missing.
- Non-empty URL must parse as `http://` or `https://` and include a host.

---

## 7. Native UI

The bootstrap UI is Windows native because WebView2 and Tauri UI are unavailable.

### 7.1 Confirmation Dialog

Implementation note: the bootstrap uses `MessageBoxW` with `MB_YESNO`, so the
button labels are the system Yes/No text. The custom labels below describe the
intended meaning only.

Title:

```text
File Sync Tool startup
```

Message:

```text
File Sync Tool requires Microsoft Edge WebView2 Runtime to start.
The component was not detected on this computer.

Install it now from the internal update server?
```

Buttons:

```text
Install and start
Exit
```

### 7.2 Progress Window

States:

```text
Preparing:
  Connecting to internal update server...

Downloading:
  Downloading WebView2 Runtime...
  Shows percent, downloaded bytes, and total bytes when content length exists.
  Cancel is enabled.

Verifying:
  Verifying installer integrity...

Installing:
  Installing WebView2 Runtime silently...
  Indeterminate progress.
  Cancel is disabled.

Restarting:
  Installation complete. Restarting File Sync Tool...
```

If the progress window cannot be created, use MessageBox fallback:

```text
1. "Downloading WebView2 Runtime. Please wait."
2. "Download complete. Starting installation."
3. Success restarts automatically.
4. Failure shows the final error.
```

### 7.3 Error Dialogs

Every terminal failure uses a native error dialog and startup log line. The dialog should include a short user-facing cause and point to the internal update server or administrator, not raw Rust backtraces.

---

## 8. Download and Verification

Download flow:

```text
1. GET ${base}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256
2. Parse first 64 hex chars from either:
   - "<64hex>"
   - "<64hex>  MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
3. GET ${base}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe
4. Write to:
   %TEMP%\file-sync-tool-webview2\MicrosoftEdgeWebView2RuntimeInstallerX64.exe.part
5. Hash while streaming, or hash before final rename.
6. On match, rename to final installer path.
7. On mismatch, delete `.part` and final file.
```

Progress events go only to native UI, not Tauri events.

The bootstrap should prefer no proxy behavior, matching the existing updater download client.

---

## 9. Install and Restart

Install command:

```text
MicrosoftEdgeWebView2RuntimeInstallerX64.exe /silent /install
```

Rules:

- Spawn the installer directly, not through `cmd.exe`, unless a platform limitation requires otherwise.
- Wait for process completion.
- Non-zero exit code is terminal failure.
- Do not require admin elevation. If the app is not elevated, the installer can perform a per-user install according to Microsoft Evergreen installer behavior.
- After a zero exit code, poll WebView2 detection for up to 60 seconds before deciding success.
- Only restart the app after post-install detection succeeds.

Restart command:

```text
current_exe original_args...
```

with:

```text
FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1
```

The new process still detects Runtime. If it is still missing, it exits with an error instead of looping.

---

## 10. Error Handling Matrix

| Scenario | Behavior |
| --- | --- |
| WebView2 already present | Continue to Tauri, no native UI. |
| User rejects install | Log and exit. |
| `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1` and Runtime still missing | Native error dialog and exit, no second install attempt. |
| `update_server_url` empty | Native error dialog and exit. |
| `update_server_url` invalid | Native error dialog and exit. |
| `.sha256` download fails | Native error dialog and exit. |
| `.sha256` format invalid | Native error dialog and exit. |
| Installer download fails | Delete partial file, native error dialog, exit. |
| User cancels download | Delete partial file, log, exit. |
| SHA-256 mismatch | Delete installer files, native error dialog, exit. |
| Progress window creation fails | Use MessageBox fallback. |
| Installer process spawn fails | Native error dialog and exit. |
| Installer exits non-zero | Native error dialog with exit code and exit. |
| Installer exits zero but Runtime is not detected within 60 seconds | Native error dialog and exit. |
| Non-Windows build | Bootstrap is a no-op. |

---

## 11. Tests

### Rust Unit Tests

- `detect.rs`
  - missing keys and empty `pv` values produce missing.
  - `0.0.0.0` produces missing.
  - `109.0.1518.78` produces present.
- `startup_config.rs`
  - default path resolution.
  - pivot path resolution.
  - missing config uses default update server URL.
  - custom config overrides default URL.
  - URL normalization strips trailing slashes.
- `server.rs`
  - URL construction for base URL with and without trailing slash.
- `sha256_file.rs`
  - parses bare 64-char hash.
  - parses `hash filename`.
  - rejects invalid length and non-hex content.
- `download.rs`
  - success writes final installer file.
  - cancel deletes `.part`.
  - hash mismatch deletes files.
- `install.rs`
  - command args are exactly `/silent` and `/install`.
- `restart.rs`
  - original args are preserved.
  - `FST_WEBVIEW2_BOOTSTRAP_RESTARTED=1` is set.

### Manual Windows QA

```text
[ ] Clean Windows VM without WebView2 Runtime: confirm prompt, progress UI, install, restart, main window opens.
[ ] Machine with WebView2 Runtime: startup shows no bootstrap UI.
[ ] Internal update server unreachable: native error and exit.
[ ] Empty update_server_url: native error and exit.
[ ] Wrong SHA-256: installer is deleted, native error and exit.
[ ] Installer missing on server: native error and exit.
[ ] Force progress window failure: MessageBox fallback is used.
[ ] Custom data dir configured: bootstrap reads the custom config URL.
[ ] Restart loop guard: restarted process missing Runtime exits instead of reinstalling forever.
```

---

## 12. Out of Scope

- Bundling the installer beside the app exe.
- Native URL entry or server configuration UI before Tauri exists.
- Mandatory admin elevation.
- Fixed Version WebView2 Runtime.
- X86 and ARM64 installer selection in the first implementation.
- macOS and Linux behavior beyond no-op.
- Replacing the existing in-app updater.

---

## 13. Implementation Notes

- Call bootstrap after panic logging is installed so early failures are visible in `app.log`.
- Call bootstrap before `single_instance_guard::ensure_single_instance()` to
  avoid the restarted child losing the guard-mutex race and silently exiting.
- Do not create tray icons, Tauri windows, or plugins before bootstrap succeeds.
- Do not store WebView2 install state in app config. The registry is the source of truth.
- Keep all terminal bootstrap failures visible through native dialogs because GUI builds do not have a console.
- Avoid long blocking work on a hidden Tauri main thread. This code runs before Tauri exists.
