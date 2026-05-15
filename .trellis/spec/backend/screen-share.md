# Screen Share

## Scenario: Capture Session Startup and Recovery

### 1. Scope / Trigger

- Trigger: changes to `src-tauri/src/screenshare.rs` that start, stop, recreate, or report the screen-capture session.
- Problem prevented: a timed-out or stale capture thread must not keep retrying `scrap::Capturer::new` after a new screen-share start begins, because that can leave the DXGI desktop duplication resource busy and surface as `PermissionDenied`.

### 2. Signatures

```rust
pub struct ScreenShareHandle {
    active: Arc<AtomicBool>,
    starting: AtomicBool,
    cancel: Arc<AtomicBool>,
    session_id: AtomicU64,
}

fn begin_screen_share_start(
    handle: &Arc<ScreenShareHandle>,
) -> Result<ScreenShareStartGuard, String>;

fn is_current_session(handle: &ScreenShareHandle, session_id: u64) -> bool;

fn wait_for_capture_retry_delay(
    delay: Duration,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
) -> bool;

fn format_capture_conflict_diagnostics(
    conflicts: &[ScreenShareConflictProcess],
) -> String;

fn capture_blocking_conflict_error(
    conflicts: &[ScreenShareConflictProcess],
) -> Option<String>;

enum CaptureBackendKind {
    Dxgi,
    Wgc,
}

pub enum ScreenShareBackendMode {
    Auto,
    Wgc,
    Dxgi,
}

fn create_capture_source(
    monitor_index: usize,
    show_cursor: bool,
    backend_mode: ScreenShareBackendMode,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
    app_handle: &AppHandle,
) -> Result<CaptureSource, String>;

fn format_capture_backend_fallback_message(
    from: CaptureBackendKind,
    to: CaptureBackendKind,
    session_id: u64,
    monitor_index: usize,
    cause: &str,
) -> String;
```

### 3. Contracts

- `screen_share_start` must reserve startup with `begin_screen_share_start` before monitor enumeration, listener bind, or capture-thread spawn.
- `active` only means a fully started service. The separate `starting` flag blocks duplicate starts while capture initialization is still pending.
- Every start attempt receives a new `session_id`; `reset_runtime_state` invalidates the current session.
- Capture-loop retry waits must call `wait_for_capture_retry_delay`, not raw `thread::sleep`, when waiting before capture creation or recreation.
- Capture failure shutdown, HTTP server cleanup, and status reporting must check `is_current_session` before mutating shared runtime state.
- `CAPTURE_STARTUP_TIMEOUT` must be longer than the full capture-create retry delay window, with at least 2 seconds of slack.
- Capture creation must emit target-machine diagnostics at start, on every failed attempt, on cancellation, and on success. Diagnostics must include `session_id`, attempt index, elapsed milliseconds, retryability, runtime state, and conflict scan results.
- If a real `Capturer::new` attempt fails and the same target machine reports visible force-close capture conflicts, return an actionable close-and-retry error immediately instead of waiting through the full retry window.
- `create_capture_source` must try DXGI (`scrap`) first and, on Windows, automatically fall back to Windows Graphics Capture (WGC) when DXGI creation fails.
- `ScreenShareConfig.capture_backend_mode` is a serialized cross-layer contract using `snake_case` values: `auto`, `wgc`, and `dxgi`.
- `capture_backend_mode=auto` is the recommended default. It must try a short DXGI probe window first, then fall back to WGC on Windows if DXGI still cannot start.
- `capture_backend_mode=wgc` must skip DXGI entirely and start directly with WGC on Windows.
- `capture_backend_mode=dxgi` must use only DXGI. If DXGI cannot start, the screen-share start must fail without automatic WGC fallback.
- Automatic mode's DXGI retry window must remain intentionally short (currently `[0, 200, 400]` milliseconds) so users do not wait through the full DXGI-only retry budget before WGC fallback.
- WGC fallback must log the transition from `DXGI` to `WGC`, log WGC creation start, log WGC failure with the failing stage/HRESULT when applicable, and log the selected backend on success.
- WGC monitor capture must use Win32 interop (`IGraphicsCaptureItemInterop::CreateForMonitor`) rather than `TryCreateFromDisplayId`, so Windows 10 Enterprise 21H2 remains supported.
- `CAPTURE_STARTUP_TIMEOUT` must leave enough room for the full DXGI retry window plus WGC fallback startup slack.

### 4. Validation & Error Matrix

| Case | Expected Result |
| --- | --- |
| Start called while `active == true` | Return `Err("Screen share is already active")` |
| Start called while `starting == true` | Return `Err("Screen share is already starting")` |
| Startup timeout fires | Set `cancel`, reset runtime state, and invalidate the session |
| Old capture thread later reports failure | Ignore it if its `session_id` is stale |
| Stop called during startup | Set `cancel`, reset runtime state, and let the pending start fail as cancelled |
| Capture recreation delay is pending and user stops | Delay exits promptly without waiting for the full retry duration |
| `Capturer::new` fails on another PC | Log that PC's conflict scan snapshot and every retry attempt; do not infer from the developer machine |
| `Capturer::new` fails and target PC reports `blocking_count > 0` | Return an error naming the blocking processes and asking the user to close them and retry |
| DXGI creation fails on Windows | Emit a fallback log and attempt WGC before failing the screen-share start |
| `capture_backend_mode=auto` and DXGI cannot start | Retry only within the short automatic DXGI probe window, then attempt WGC |
| `capture_backend_mode=wgc` | Start with WGC directly and do not emit a DXGI fallback transition first |
| `capture_backend_mode=dxgi` and DXGI cannot start | Fail startup without attempting WGC |
| WGC creation fails after DXGI failed | Final error includes both DXGI and WGC failure causes |
| WGC creation succeeds after DXGI failed | Continue sharing with backend `WGC` and log the selected backend |

### 5. Good/Base/Bad Cases

- Good: a browser-triggered connection reset causes one capture recreation attempt, but a stop or new start invalidates the old worker before it can take the monitor again.
- Base: a normal start transitions `starting -> active`, stores URLs and start time, then starts HTTP serving and status reporting for the same session. On Windows, DXGI remains the preferred backend when it is available.
- Fallback: if DXGI is blocked by another capture stack, WGC is attempted and selected without requiring the user to close DingTalk first.
- Bad: using only `active` to block duplicate starts, because `active` is false during the capture startup window.

### 6. Tests Required

- Unit test: second `begin_screen_share_start` call is rejected until the first startup guard is released.
- Unit test: `reset_runtime_state` invalidates a previously reserved capture session.
- Unit test: `wait_for_capture_retry_delay` exits immediately when cancelled or stale.
- Unit test: `CAPTURE_STARTUP_TIMEOUT` covers `capture_create_retry_window() + 2s`.
- Unit test: `CAPTURE_STARTUP_TIMEOUT` covers the full DXGI retry window plus WGC fallback startup slack.
- Unit test: conflict diagnostics include target-machine process count and process details.
- Unit test: blocking conflict errors include the process details and close/retry action.
- Unit test: backend fallback messages include `DXGI`, `WGC`, `session_id`, `monitor_index`, and the DXGI failure cause.
- Unit test: backend failure messages include the backend name and WGC failure cause.
- Unit test: backend mode labels serialize to `auto`, `wgc`, and `dxgi`.
- Unit test: automatic mode uses a shorter DXGI retry window than DXGI-only mode.
- Full backend verification: `cargo test --manifest-path src-tauri/Cargo.toml`.

### 7. Wrong vs Correct

#### Wrong

```rust
if handle.active.load(Ordering::SeqCst) {
    return Err("Screen share is already active".into());
}
std::thread::sleep(Duration::from_millis(500));
match create_capturer(monitor_index) { /* ... */ }
```

This misses the startup window and lets stale capture workers continue sleeping and retrying after cancellation.

#### Correct

```rust
let start_guard = begin_screen_share_start(handle)?;
let session_id = start_guard.session_id();

if !wait_for_capture_retry_delay(delay, &cancel, &runtime_handle, session_id) {
    return Err("screen capture init cancelled".to_string());
}
```

The startup guard blocks overlapping starts, and the session-aware wait prevents stale workers from mutating the new runtime.
