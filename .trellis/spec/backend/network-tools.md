# Network Tools

## Scenario: Streaming Port Test Scans

### 1. Scope / Trigger

- Trigger: the port test tool crosses Vue UI, `src/lib/tauri.ts`, Tauri command registration, and Rust async networking.
- Use this contract when changing port scan range limits, progress rendering, cancellation, or result payloads.

### 2. Signatures

```rust
// src-tauri/src/network.rs
pub struct PortTestRequest {
    pub host: String,
    pub ports: Vec<u16>,
    pub timeout_ms: u64,
}

pub struct PortTestResult {
    pub host: String,
    pub resolved_ip: Option<String>,
    pub results: Vec<SinglePortResult>,
}

pub struct SinglePortResult {
    pub port: u16,
    pub open: bool,
    pub latency_ms: Option<f64>,
    pub name: String,
}

#[tauri::command]
pub async fn test_ports(
    app_handle: tauri::AppHandle,
    state: State<'_, NetworkState>,
    request: PortTestRequest,
) -> Result<PortTestResult, String>;

#[tauri::command]
pub fn cancel_port_test(state: State<'_, NetworkState>);
```

```typescript
// src/lib/tauri.ts
export async function testPorts(request: PortTestRequest): Promise<PortTestResult>;
export async function cancelPortTest(): Promise<void>;
```

### 3. Contracts

- `PortTestRequest.ports` may contain the complete TCP range `1..=65535`; do not reintroduce a 1000-port cap.
- Rust normalizes ports by sorting and deduplicating before scanning.
- `timeoutMs: 0` means backend default timeout. Non-zero values are clamped to `100..=30000` ms.
- Backend scans with bounded concurrency and emits one `port-test-result` event per completed port with `SinglePortResult` camelCase fields.
- Backend emits `port-test-complete` after the scan loop ends, including cancellation.
- `test_ports` still returns `PortTestResult` sorted by port, so callers that do not listen to events can still consume final or partial results.
- Frontend must attach event listeners before invoking `testPorts()` and detach them in both success and cancellation paths.

### 4. Validation & Error Matrix

| Case | Layer | Behavior |
|------|-------|----------|
| Empty host | Rust command | Return `Err("Host is required")` |
| Empty port list | Rust command | Return `Err("No ports specified")` |
| Duplicate ports | Rust command | Sort and deduplicate |
| `all` shortcut | Vue/helper | Expand to `[1, ..., 65535]` before invoking Tauri |
| Large scan timeout | Vue/helper | Prefer `500ms` for scans above 1000 ports |
| Cancellation | Rust command + Vue | Set `port_cancel`; UI stops loading and keeps scanned rows |

### 5. Good/Base/Bad Cases

- Good: input `all` scans every TCP port and displays live grid updates.
- Base: input `22,80,443` returns sorted rows with known service names where available.
- Bad: input `70000` produces an empty parsed list on the frontend and does not call Tauri.

### 6. Tests Required

- Frontend helper test: `parsePorts('1-1001')` returns 1001 ports.
- Frontend helper test: `parsePorts('all')` returns 65535 ports with first `1` and last `65535`.
- Frontend helper test: grid cells map results to `open`, `closed`, `scanning`, and `waiting`.
- Backend unit test: `normalize_requested_ports((1..=u16::MAX).collect())` accepts all 65535 ports.
- Backend unit test: normalization sorts and deduplicates more than 1000 requested ports.
- Type/build check: `pnpm check` and `pnpm build` must pass after payload or listener changes.

### 7. Wrong vs Correct

#### Wrong

```typescript
if (ports.length > 1000) {
  errorMsg.value = t('networkTools.port.tooManyPorts');
  return;
}
const result = await testPorts({ host, ports, timeoutMs });
```

This blocks full-port scans and leaves users waiting without progress for large ranges.

#### Correct

```typescript
await attachListeners();
const finalResult = await testPorts({ host, ports, timeoutMs });
```

Attach listeners first, then let `port-test-result` stream progress while preserving the final return value.
