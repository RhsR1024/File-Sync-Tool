# Appliance SSH

## Scenario: API Version Port Selection

### 1. Scope / Trigger

- Trigger: the Enable Appliance SSH tool crosses Vue UI, `src/lib/tauri.ts`, the Tauri `enable_appliance_ssh` command, and device HTTP APIs.
- Use this contract when changing the SSH status/enable API request shape, management API port, or jump-host behavior.

### 2. Signatures

```typescript
// src/lib/tauri.ts
export type ApplianceSshApiVersion = 'componentized' | 'mainline';
export type ApplianceSshWhitelistScope = 'allTcp' | 'sshOnly';

export interface EnableApplianceSshRequest {
  targets: ApplianceSshTarget[];
  applianceVersion: ApplianceSshApiVersion;
  whitelistScope: ApplianceSshWhitelistScope;
  sshUsername?: string;
  sshPassword?: string;
  addWhitelistRule: boolean;
  whitelistCidr?: string;
  jumpHostUseSeparateCreds?: boolean;
  jumpHostUsername?: string;
  jumpHostPassword?: string;
}
```

```rust
// src-tauri/src/main.rs
pub enum ApplianceSshApiVersion {
    Componentized,
    Mainline,
}

pub enum ApplianceSshWhitelistScope {
    SshOnly,
    AllTcp,
}

pub struct ApplianceSshRequest {
    pub targets: Vec<ApplianceSshTarget>,
    pub ips: Vec<String>,
    pub appliance_version: ApplianceSshApiVersion,
    pub ssh_username: String,
    pub ssh_password: String,
    pub add_whitelist_rule: bool,
    pub whitelist_scope: ApplianceSshWhitelistScope,
    pub whitelist_cidr: Option<String>,
    pub jump_host_use_separate_creds: bool,
    pub jump_host_username: Option<String>,
    pub jump_host_password: Option<String>,
}
```

```jsonc
// Device status API: POST /openAPI/system/v1/network/SSH/get
{}
{
  "code": 0,
  "message": "success",
  "data": {
    "enable": 0,
    "port": 23333
  }
}
```

### 3. Contracts

- Frontend sends `applianceVersion` on every new `enableApplianceSsh()` call.
- Backend accepts missing `applianceVersion` for legacy callers and defaults to `componentized`.
- `componentized` maps to management API port `23006`.
- `mainline` maps to management API port `9007`.
- Both status (`/openAPI/system/v1/network/SSH/get`) and enable (`/openAPI/system/v1/network/SSH/set`) calls must use the same selected management API port.
- Jump-host targets use the selected version port against the jump-host IP because the management API lives on the jump host.
- Do not use the management API version port as the SSH login port. SSH login still uses the `port` returned by the status API, falling back to `23333`.
- Frontend defaults `whitelistScope` to `allTcp`; new UI submissions should therefore open every TCP port to the resolved management/source IP.
- Backend serde defaults missing `whitelistScope` to `sshOnly` so legacy callers keep the old narrow behavior.
- `allTcp` builds a source-scoped INPUT rule with no `--dport`: `iptables -C INPUT -p tcp -s <source> -j ACCEPT || iptables -I INPUT 1 -p tcp -s <source> -j ACCEPT`.
- `sshOnly` builds the old port-scoped rule with `--dport <sshPort>`.
- The SSH executor should log target, API host/port/version, status API results, resolved whitelist source, scope, SSH execution host/port, command, execution mode, output, and errors to `log-message` / app log.
- SSH command execution tries `exec`, then `exec+pty`, then interactive `shell` for appliances that reject non-interactive remote commands with messages such as `Remote command execution is not allowed`.
- The initial `SSH/get` call is best-effort: if it fails (HTTP/network/protocol error), the flow logs a `warn` and proceeds to call `SSH/set` anyway with `previous_enable=None`. This handles appliances whose access-control mode rejects `get` while still accepting `set`.
- After `SSH/set` returns success, the flow polls `SSH/get` to confirm `enable==1`. The poll has three outcomes: `Enabled` (use the returned status), `NotEnabled` (return an error with the last observed enable value), and `GetFailed` (every poll attempt errored — trust the `set` success, synthesize `enable=Some(1)`, fall back to `port=23333` if no port has ever been observed).

- Status API `data.enable` uses `0` for disabled/off and `1` for enabled/on; Vue result chips must render `0` as disabled, not unknown.

### 4. Validation & Error Matrix

| Case | Layer | Behavior |
|------|-------|----------|
| Missing `applianceVersion` | Rust serde | Default to `Componentized` / `23006` |
| `applianceVersion: 'componentized'` | Rust helper | Build URLs with `:23006` |
| `applianceVersion: 'mainline'` | Rust helper | Build URLs with `:9007` |
| Unknown version string | Rust serde | Command deserialization fails before HTTP request |
| UI default whitelist scope | Vue | Send `whitelistScope: 'allTcp'` |
| Missing `whitelistScope` | Rust serde | Default to `SshOnly` for compatibility |
| `whitelistScope: 'allTcp'` | Rust helper | Build source-only ACCEPT rule without `--dport` |
| `whitelistScope: 'sshOnly'` | Rust helper | Build source + `--dport <sshPort>` ACCEPT rule |
| SSH exec restricted | Rust SSH executor | Retry with `exec+pty`, then interactive shell, and include each failure in diagnostics if all modes fail |
| Jump-host target | Rust target worker | Call get/set on `jumpHost:selectedPort`, then SSH through jump host as before |
| Initial `SSH/get` fails | Rust target worker | Log warn, set `previous_enable=None`, still call `SSH/set` |
| `SSH/set` succeeds, every verification `SSH/get` fails | Rust target worker | Log warn, treat as success with synthesized `enable=Some(1)`; do NOT return error |
| `SSH/set` succeeds, verification `SSH/get` returns `enable!=1` | Rust target worker | Return error "current enable state is ..." (real failure, not a transport failure) |
| Device status `enable=0` | Vue result presentation | Display disabled/off (`stateDisabled`), not unknown |

### 5. Good/Base/Bad Cases

- Good: choose `mainline`, enter `192.168.1.10`, backend calls `http://192.168.1.10:9007/openAPI/system/v1/network/SSH/get` and then `set` on `9007` when needed.
- Good: choose default `allTcp`, resolved source `192.115.1.15`, backend inserts `ACCEPT tcp -- 192.115.1.15 0.0.0.0/0` before existing per-port `DROP` / `REJECT` rules.
- Good: appliance access control rejects `SSH/get` but accepts `SSH/set`; backend logs warn, runs `set`, verification GET keeps failing, result is marked success with `port=23333` (default) and `current_enable=1`.
- Good: status response `{"data":{"enable":0,"port":23333}}` is returned as `previousEnable: 0` and rendered as disabled/off in the result chip.
- Base: choose default `componentized`, backend behavior remains the historical `23006` path.
- Bad: update only the `set` call to `9007` while leaving `get` on `23006`; status polling will report the wrong device version.
- Bad: use `--dport 23333` while the user selected `allTcp`; only SSH is opened and management ports like `20012` / `5432` remain blocked.
- Bad: abort the enable flow when the initial `SSH/get` returns HTTP 403 / network error; the user explicitly asked to "treat set success as success" when the device's get endpoint is gated.
- Bad: render only `enable=1` as enabled and `enable=2` as disabled; the real off value is `0`, so the UI would show unknown before every enable run.

### 6. Tests Required

- Backend unit test: `ApplianceSshApiVersion::default()` maps to `23006`.
- Backend unit test: `ApplianceSshApiVersion::Mainline` maps to `9007`.
- Backend unit test: API URL builder uses the selected version port for both `get` and `set` paths.
- Backend unit test: `allTcp` whitelist rule omits `--dport`.
- Backend unit test: `sshOnly` whitelist rule includes the reported SSH port.
- Frontend unit test: `getApplianceSshEnableState(0)` returns `disabled`, `1` returns `enabled`, and missing values return `unknown`.
- Type/build check: `pnpm check` after adding or renaming the frontend request field.
- Rust test check: `cargo test --manifest-path src-tauri/Cargo.toml appliance_ssh`.

### 7. Wrong vs Correct

#### Wrong

```rust
let get_url = format!("http://{}:23006/openAPI/system/v1/network/SSH/get", ip);
let set_url = format!("http://{}:9007/openAPI/system/v1/network/SSH/set", ip);
```

This splits one operation across two device API versions.

#### Correct

```rust
let get_url = build_appliance_ssh_api_url(ip, api_version, "get");
let set_url = build_appliance_ssh_api_url(ip, api_version, "set");
```

One selected version controls all management API calls in the run.

#### Wrong

```typescript
if (value === 1) return t('tools.applianceSsh.stateEnabled');
if (value === 2) return t('tools.applianceSsh.stateDisabled');
return t('tools.applianceSsh.stateUnknown');
```

This treats the device's real disabled value (`0`) as unknown.

#### Correct

```typescript
if (value === 1) return 'enabled';
if (value === 0) return 'disabled';
return 'unknown';
```

The UI maps the API contract first, then translates `enabled` / `disabled` / `unknown` for display.

#### Wrong

```rust
let rule = format!(
    "iptables -I INPUT 1 -p tcp -s {source} --dport {ssh_port} -j ACCEPT"
);
```

This ignores `whitelistScope: 'allTcp'` and leaves other appliance management ports blocked.

#### Correct

```rust
let rule = build_iptables_whitelist_rule(source, ssh_port, whitelist_scope);
```

One helper owns the source-only versus source-plus-port rule shape.
