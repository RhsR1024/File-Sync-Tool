# Appliance SSH — Jump-Host Port + Degrade-to-SSH Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the appliance management API (`SSH/set`) is unreachable for a jump-host target, degrade to the SSH channel instead of aborting, and let the user set the jump-host SSH port (default 23333).

**Architecture:** API-first, degrade-to-SSH on `SSH/set` failure — jump-host targets only. A new user-supplied jump-host SSH port drives both the login to the jump host and the nested hop to the target, replacing today's hardcoded 23333. Pure helpers (port resolution, credential resolution, port validation) are unit-tested; the async flow rewrite and Vue wiring are verified by `cargo test` + `pnpm check` + the versioned build.

**Tech Stack:** Rust (Tauri command, `reqwest`, `ssh2` via existing helpers), Vue 3 `<script setup>` + TypeScript, vue-i18n.

## Global Constraints

- All user-facing text MUST have both `en` and `zh` entries in `src/locales/messages.ts`; call via `t('key')`, never hardcode.
- Frontend request/response types live in `src/lib/tauri.ts`; the Rust mirror lives in `src-tauri/src/main.rs` (`ApplianceSshRequest`). Both must stay in sync.
- Degradation applies to **jump-host pairs only**. Direct (non-jump-host) targets keep today's behavior: `SSH/set` failure returns an error.
- Jump-host SSH port resolution priority: user-supplied port → status-API port → `23333`.
- Rust follows `cargo fmt` / `cargo clippy`.
- Final verification per CLAUDE.md: `cmd /c pnpm tauri:build:versioned-exe`.
- Commit messages in Chinese.
- Work lands directly on `main` (no feature branch).

## File Structure

- `src-tauri/src/main.rs` — add `jump_host_ssh_port` field to `ApplianceSshRequest`; add `resolve_jump_host_ssh_port` and `resolve_appliance_ssh_creds` helpers; rewrite the jump-host branch of `enable_appliance_ssh_for_target`; thread the new port through the `enable_appliance_ssh` command. Unit tests live in the existing `#[cfg(test)] mod tests` block near the top of the file.
- `src/lib/applianceSshPresentation.ts` — add pure `isValidSshPort` helper.
- `src/lib/applianceSshPresentation.test.mjs` — add cases for `isValidSshPort`.
- `src/lib/tauri.ts` — add `jumpHostSshPort?: number` to `EnableApplianceSshRequest`.
- `src/pages/EnableApplianceSshPage.vue` — new `jumpHostSshPort` state, validation computed, submit guard, request field, and the port input in the jump-host card.
- `src/locales/messages.ts` — `jumpHostSshPort` / `jumpHostSshPortHint` / `jumpHostSshPortInvalid` in `en` and `zh`.
- `.trellis/spec/backend/appliance-ssh.md` — new contracts for the port field and the degrade path.

---

### Task 1: Backend — `jump_host_ssh_port` field + port resolver

**Files:**
- Modify: `src-tauri/src/main.rs` (`ApplianceSshRequest` struct ~2254; new helper near `JUMP_HOST_DEFAULT_TARGET_SSH_PORT` ~2533; test in `mod tests` ~2139)

**Interfaces:**
- Produces: `pub jump_host_ssh_port: Option<u16>` on `ApplianceSshRequest`; `fn resolve_jump_host_ssh_port(user_port: Option<u16>, status_port: Option<u16>) -> u16`.

- [ ] **Step 1: Add the request field**

In `ApplianceSshRequest`, after the `jump_host_password` field (`src-tauri/src/main.rs:2253-2254`):

```rust
    #[serde(default)]
    pub jump_host_password: Option<String>,
    /// SSH port used to reach the jump host and the nested hop to the target.
    /// Resolution priority: this value > status-API port > 23333.
    #[serde(default)]
    pub jump_host_ssh_port: Option<u16>,
}
```

- [ ] **Step 2: Write the failing test**

Add inside `mod tests`, before its closing `}` (currently at `src-tauri/src/main.rs:2140`):

```rust
    #[test]
    fn resolve_jump_host_ssh_port_prefers_user_then_status_then_default() {
        assert_eq!(resolve_jump_host_ssh_port(Some(2222), Some(23333)), 2222);
        assert_eq!(resolve_jump_host_ssh_port(None, Some(2200)), 2200);
        assert_eq!(resolve_jump_host_ssh_port(None, None), 23333);
        // A 0 port is treated as "unset" and falls through to the status/default.
        assert_eq!(resolve_jump_host_ssh_port(Some(0), Some(2200)), 2200);
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml resolve_jump_host_ssh_port`
Expected: FAIL — `cannot find function resolve_jump_host_ssh_port`.

- [ ] **Step 4: Implement the resolver**

Immediately after `const JUMP_HOST_DEFAULT_TARGET_SSH_PORT: u16 = 23333;` (`src-tauri/src/main.rs:2533`):

```rust
/// Resolve the SSH port used to reach the jump host and the nested target hop.
/// Priority: an explicit non-zero user port, then the status-API port, then the
/// 23333 default.
fn resolve_jump_host_ssh_port(user_port: Option<u16>, status_port: Option<u16>) -> u16 {
    user_port
        .filter(|p| *p != 0)
        .or(status_port)
        .unwrap_or(JUMP_HOST_DEFAULT_TARGET_SSH_PORT)
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml resolve_jump_host_ssh_port`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat(appliance-ssh): 新增 jump_host_ssh_port 字段与端口解析优先级"
```

---

### Task 2: Backend — credential resolver + degrade-to-SSH flow

**Files:**
- Modify: `src-tauri/src/main.rs` (new `resolve_appliance_ssh_creds` helper + test; `enable_appliance_ssh_for_target` signature and body ~2969-3295; `enable_appliance_ssh` command wiring ~3298-3358)

**Interfaces:**
- Consumes: `resolve_jump_host_ssh_port` (Task 1), existing `run_remote_command_over_ssh`, `enable_appliance_ssh_via_api`, `wait_for_appliance_ssh_enabled`.
- Produces: `fn resolve_appliance_ssh_creds(is_jump_host: bool, ssh_username: &str, ssh_password: &str, jump_host_username: Option<&str>, jump_host_password: Option<&str>) -> (String, String)`; new `jump_host_ssh_port: Option<u16>` parameter on `enable_appliance_ssh_for_target`.

- [ ] **Step 1: Write the failing credential-resolver test**

Add inside `mod tests`, before its closing `}`:

```rust
    #[test]
    fn resolve_appliance_ssh_creds_prefers_jump_host_then_falls_back() {
        assert_eq!(
            resolve_appliance_ssh_creds(true, "root", "main", Some("jump"), Some("jpass")),
            ("jump".to_string(), "jpass".to_string())
        );
        // Blank jump-host creds fall back to the main SSH creds.
        assert_eq!(
            resolve_appliance_ssh_creds(true, "root", "main", Some("  "), Some("")),
            ("root".to_string(), "main".to_string())
        );
        // Direct (non-jump-host) targets always use the main creds.
        assert_eq!(
            resolve_appliance_ssh_creds(false, "root", "main", Some("jump"), Some("jpass")),
            ("root".to_string(), "main".to_string())
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml resolve_appliance_ssh_creds`
Expected: FAIL — `cannot find function resolve_appliance_ssh_creds`.

- [ ] **Step 3: Implement the credential resolver**

Add just above `fn build_nested_iptables_whitelist_command` (`src-tauri/src/main.rs:2543`):

```rust
/// Resolve the (username, password) used for SSH. Jump-host targets prefer the
/// separate jump-host creds when non-blank, otherwise fall back to the main SSH
/// creds; direct targets always use the main creds. Username is trimmed;
/// password is used as-is (only rejected when empty).
fn resolve_appliance_ssh_creds(
    is_jump_host: bool,
    ssh_username: &str,
    ssh_password: &str,
    jump_host_username: Option<&str>,
    jump_host_password: Option<&str>,
) -> (String, String) {
    if is_jump_host {
        let user = jump_host_username
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| ssh_username.to_string());
        let pass = jump_host_password
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| ssh_password.to_string());
        (user, pass)
    } else {
        (ssh_username.to_string(), ssh_password.to_string())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml resolve_appliance_ssh_creds`
Expected: PASS.

- [ ] **Step 5: Add the `jump_host_ssh_port` parameter to the worker**

In `enable_appliance_ssh_for_target`, add a final parameter after `jump_host_password` (`src-tauri/src/main.rs:2980`):

```rust
    jump_host_username: Option<String>,
    jump_host_password: Option<String>,
    jump_host_ssh_port: Option<u16>,
) -> Option<ApplianceSshResult> {
```

- [ ] **Step 6: Declare degrade state after `api_ip`**

Immediately after the `api_ip` binding and its `emit_runtime_log` info call (after `src-tauri/src/main.rs:3029`), add:

```rust
    let is_jump_host = jump_host.is_some();
    let mut degraded_api_error: Option<String> = None;
```

- [ ] **Step 7: Rewrite the `current_status` block to degrade on jump-host SET failure**

Replace the entire `let current_status = if ... { ... };` block (`src-tauri/src/main.rs:3062-3114`) with:

```rust
    let current_status = if initial_status.as_ref().and_then(|s| s.enable) == Some(1) {
        initial_status.expect("checked enable==Some(1) above")
    } else {
        match enable_appliance_ssh_via_api(&client, &api_ip, api_version).await {
            Ok(()) => match wait_for_appliance_ssh_enabled(
                &client,
                &api_ip,
                api_version,
                10,
                Duration::from_secs(1),
            )
            .await
            {
                WaitForEnableOutcome::Enabled(status) => status,
                WaitForEnableOutcome::NotEnabled { last_status } => {
                    let observed = last_status
                        .enable
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    result.current_enable = last_status.enable;
                    result.port = last_status.port.or(result.port);
                    result.message = format!(
                        "SSH status verification failed: current enable state is {}",
                        observed
                    );
                    return Some(result);
                }
                WaitForEnableOutcome::GetFailed { last_error } => {
                    // SET succeeded but every GET (initial + verification) failed.
                    // Trust the SET success and treat the appliance as enabled.
                    emit_runtime_log(
                        &app_handle,
                        format!(
                            "[appliance-access] target={} GET unavailable after SET ({}); treating SET success as enabled",
                            ip, last_error
                        ),
                        "warn",
                    );
                    ApplianceSshStatusData {
                        enable: Some(1),
                        port: initial_status.as_ref().and_then(|s| s.port),
                    }
                }
            },
            Err(e) => {
                if is_jump_host {
                    // Management API is unreachable, but the jump-host SSH path may
                    // still work. Degrade: skip verification and fall through to the
                    // SSH whitelist/probe step instead of failing the whole run.
                    emit_runtime_log(
                        &app_handle,
                        format!(
                            "[appliance-access] target={} management API {}:{} unavailable ({}); degrading to SSH channel",
                            ip,
                            api_ip,
                            appliance_ssh_api_port(api_version),
                            e
                        ),
                        "warn",
                    );
                    degraded_api_error = Some(e);
                    ApplianceSshStatusData {
                        enable: None,
                        port: initial_status.as_ref().and_then(|s| s.port),
                    }
                } else {
                    emit_runtime_log(
                        &app_handle,
                        format!("[appliance-access] target={} SSH/set failed: {}", ip, e),
                        "error",
                    );
                    result.message = format!("Failed to enable SSH: {}", e);
                    return Some(result);
                }
            }
        }
    };
```

- [ ] **Step 8: Rewrite the port resolution + log**

Replace the block from `result.current_enable = current_status.enable;` through its `emit_runtime_log(... "info");` (`src-tauri/src/main.rs:3116-3126`) with:

```rust
    result.current_enable = current_status.enable;
    // SSH login port. Jump hosts prefer the user-supplied port, then the
    // status-reported port, then 23333; direct targets keep the historical
    // status-port-or-23333 behavior. The same port is reused for the nested
    // jump-host -> target hop below.
    let api_ssh_port = if is_jump_host {
        resolve_jump_host_ssh_port(jump_host_ssh_port, current_status.port.or(result.port))
    } else {
        current_status.port.or(result.port).unwrap_or(23333)
    };
    result.port = Some(api_ssh_port);
    emit_runtime_log(
        &app_handle,
        format!(
            "[appliance-access] target={} currentStatus enable={:?} sshPort={} degraded={}",
            ip,
            current_status.enable,
            api_ssh_port,
            degraded_api_error.is_some()
        ),
        "info",
    );
```

- [ ] **Step 9: Use the credential resolver inside the whitelist block**

Replace the inline `let (ssh_user, ssh_pass) = if jump_host.is_some() { ... } else { ... };` block (`src-tauri/src/main.rs:3130-3145`) with:

```rust
        // Resolve credentials for SSH to jump host (or direct target).
        let (ssh_user, ssh_pass) = resolve_appliance_ssh_creds(
            is_jump_host,
            &ssh_username,
            &ssh_password,
            jump_host_username.as_deref(),
            jump_host_password.as_deref(),
        );
```

- [ ] **Step 10: Use the resolved port for the nested hop**

In the jump-host branch of the whitelist command builder, replace `let target_port = JUMP_HOST_DEFAULT_TARGET_SSH_PORT;` (`src-tauri/src/main.rs:3188`) with:

```rust
            let target_port = api_ssh_port;
```

- [ ] **Step 11: Annotate whitelist success/failure messages when degraded**

Replace the success-message assignment `result.message = if jump_host.is_some() { ... } else { ... };` inside the `Ok(remote_result)` arm (`src-tauri/src/main.rs:3247-3257`) with:

```rust
                result.message = if let Some(api_err) = degraded_api_error.as_ref() {
                    format!(
                        "Management API unavailable ({}); applied the iptables whitelist rule on {} for {} ({}) over SSH via jump host {}",
                        api_err, ip, source, whitelist_scope_desc, api_ip
                    )
                } else if jump_host.is_some() {
                    format!(
                        "SSH is enabled on jump host {}. Added an iptables whitelist rule on {} for {} ({})",
                        api_ip, ip, source, whitelist_scope_desc
                    )
                } else {
                    format!(
                        "SSH is enabled. Added an iptables whitelist rule for {} ({})",
                        source, whitelist_scope_desc
                    )
                };
```

Replace the failure-message assignment `result.message = if jump_host.is_some() { ... } else { ... };` inside the `Err(e)` arm (`src-tauri/src/main.rs:3266-3276`) with:

```rust
                result.message = if let Some(api_err) = degraded_api_error.as_ref() {
                    format!(
                        "Management API unavailable ({}); failed to apply the iptables rule on {} over SSH via jump host {}: {}",
                        api_err, ip, api_ip, e
                    )
                } else if jump_host.is_some() {
                    format!(
                        "SSH is enabled on jump host {}, but failed to apply the iptables rule on {}: {}",
                        api_ip, ip, e
                    )
                } else {
                    format!(
                        "SSH is enabled, but failed to add the iptables whitelist rule for {} ({}): {}",
                        source, whitelist_scope_desc, e
                    )
                };
```

- [ ] **Step 12: Add the degraded no-whitelist SSH probe branch**

Replace the final `} else {` success-by-API branch (`src-tauri/src/main.rs:3279-3292`) with an `else if` probe branch plus the unchanged `else`:

```rust
    } else if let Some(api_err) = degraded_api_error {
        // Degraded jump-host path with no whitelist rule requested: prove the SSH
        // channel works by logging into the jump host.
        let (ssh_user, ssh_pass) = resolve_appliance_ssh_creds(
            is_jump_host,
            &ssh_username,
            &ssh_password,
            jump_host_username.as_deref(),
            jump_host_password.as_deref(),
        );
        if ssh_user.is_empty() || ssh_pass.is_empty() {
            result.message = format!(
                "Management API unavailable ({}); SSH username and password are required to verify the SSH channel",
                api_err
            );
            return Some(result);
        }
        let host_owned = api_ip.clone();
        let user_owned = ssh_user.clone();
        let password_owned = ssh_pass.clone();
        let probe = tauri::async_runtime::spawn_blocking(move || {
            run_remote_command_over_ssh(&host_owned, api_ssh_port, &user_owned, &password_owned, "true")
        })
        .await;
        match probe {
            Ok(Ok(_)) => {
                result.success = true;
                result.message = format!(
                    "Management API unavailable ({}), but jump host {} is reachable over SSH (port {})",
                    api_err, api_ip, api_ssh_port
                );
                emit_runtime_log(
                    &app_handle,
                    format!(
                        "[appliance-access] target={} degraded SSH probe ok via {}:{}",
                        ip, api_ip, api_ssh_port
                    ),
                    "success",
                );
            }
            Ok(Err(e)) => {
                result.message = format!(
                    "Management API unavailable ({}); SSH login to jump host {} also failed: {}",
                    api_err, api_ip, e
                );
                emit_runtime_log(
                    &app_handle,
                    format!("[appliance-access] target={} degraded SSH probe failed: {}", ip, e),
                    "error",
                );
            }
            Err(join_err) => {
                result.message = format!(
                    "Management API unavailable ({}); failed to run the SSH probe task: {}",
                    api_err, join_err
                );
            }
        }
    } else {
        result.success = true;
        result.message = if jump_host.is_some() {
            if result.previous_enable == Some(1) {
                format!("Jump host SSH is already enabled. Port: {}", api_ssh_port)
            } else {
                format!("Jump host SSH enabled successfully. Port: {}", api_ssh_port)
            }
        } else if result.previous_enable == Some(1) {
            format!("SSH is already enabled. Port: {}", api_ssh_port)
        } else {
            format!("SSH enabled successfully. Port: {}", api_ssh_port)
        };
    }
```

- [ ] **Step 13: Thread `jump_host_ssh_port` through the command**

In `enable_appliance_ssh`, after the `let (jump_user, jump_pass) = ...;` block (`src-tauri/src/main.rs:3325`), add:

```rust
    let jump_host_ssh_port = request.jump_host_ssh_port;
```

Then in the closure's call to `enable_appliance_ssh_for_target(...)`, add the argument after `jump_pass` (`src-tauri/src/main.rs:3349-3350`):

```rust
                    jump_user,
                    jump_pass,
                    jump_host_ssh_port,
                )
```

(`Option<u16>` is `Copy`, so it needs no per-iteration clone.)

- [ ] **Step 14: Format, then run the full appliance test suite + build**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml`
Run: `cargo test --manifest-path src-tauri/Cargo.toml appliance`
Expected: PASS — the four existing `appliance_ssh_*` tests plus the two new resolver tests.
Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds with no errors (clippy-clean; no unused-variable warnings for `JUMP_HOST_DEFAULT_TARGET_SSH_PORT`, still referenced by `resolve_jump_host_ssh_port`).

- [ ] **Step 15: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat(appliance-ssh): 跳板机接口失败降级 SSH 通道，端口可配"
```

---

### Task 3: Frontend — `isValidSshPort` helper

**Files:**
- Modify: `src/lib/applianceSshPresentation.ts`
- Test: `src/lib/applianceSshPresentation.test.mjs`

**Interfaces:**
- Produces: `export function isValidSshPort(value: unknown): boolean`.

- [ ] **Step 1: Add the failing test cases**

First, replace the existing import line at the top of `src/lib/applianceSshPresentation.test.mjs`:

```javascript
import { getApplianceSshEnableState } from './applianceSshPresentation.ts';
```

with:

```javascript
import { getApplianceSshEnableState, isValidSshPort } from './applianceSshPresentation.ts';
```

Then, append these cases to the same file immediately before the final `console.log('applianceSshPresentation tests PASSED');` line:

```javascript
assert.equal(isValidSshPort(23333), true, '23333 is a valid SSH port');
assert.equal(isValidSshPort(1), true, '1 is a valid SSH port');
assert.equal(isValidSshPort(65535), true, '65535 is a valid SSH port');
assert.equal(isValidSshPort(0), false, '0 is not a valid SSH port');
assert.equal(isValidSshPort(70000), false, '70000 is out of range');
assert.equal(isValidSshPort(1.5), false, 'non-integer is invalid');
assert.equal(isValidSshPort(Number.NaN), false, 'NaN is invalid');
assert.equal(isValidSshPort('23333'), false, 'string is invalid');
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node src/lib/applianceSshPresentation.test.mjs`
Expected: FAIL — `isValidSshPort is not a function` / import error.

- [ ] **Step 3: Implement the helper**

Append to `src/lib/applianceSshPresentation.ts`:

```typescript
export function isValidSshPort(value: unknown): boolean {
  return (
    typeof value === 'number' &&
    Number.isInteger(value) &&
    value >= 1 &&
    value <= 65535
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node src/lib/applianceSshPresentation.test.mjs`
Expected: PASS — prints `applianceSshPresentation tests PASSED`.

- [ ] **Step 5: Commit**

```bash
git add src/lib/applianceSshPresentation.ts src/lib/applianceSshPresentation.test.mjs
git commit -m "feat(appliance-ssh): 新增 isValidSshPort 端口校验工具"
```

---

### Task 4: Frontend — port field, request wiring, i18n

**Files:**
- Modify: `src/lib/tauri.ts` (`EnableApplianceSshRequest` ~582)
- Modify: `src/pages/EnableApplianceSshPage.vue` (imports ~7, state ~53, submit guard ~331, request ~360, jump-host card ~652)
- Modify: `src/locales/messages.ts` (en ~614, zh ~2531)

**Interfaces:**
- Consumes: `isValidSshPort` (Task 3); Rust `jump_host_ssh_port` (Task 2).
- Produces: request now carries `jumpHostSshPort?: number`.

- [ ] **Step 1: Add the type field**

In `src/lib/tauri.ts`, inside `EnableApplianceSshRequest` after `jumpHostPassword?: string;` (`src/lib/tauri.ts:582`):

```typescript
  jumpHostPassword?: string;
  jumpHostSshPort?: number;
}
```

- [ ] **Step 2: Add i18n keys (en + zh)**

In `src/locales/messages.ts`, after the `en` `jumpHostPassword:` line (`src/locales/messages.ts:614`):

```typescript
        jumpHostPassword: 'Jump Host SSH Password',
        jumpHostSshPort: 'Jump Host SSH Port',
        jumpHostSshPortHint: 'SSH port for logging into the jump host and reaching the target (default 23333).',
        jumpHostSshPortInvalid: 'Jump host SSH port must be between 1 and 65535.',
```

After the `zh` `jumpHostPassword:` line (`src/locales/messages.ts:2531`):

```typescript
        jumpHostPassword: '跳板机 SSH 密码',
        jumpHostSshPort: '跳板机 SSH 端口',
        jumpHostSshPortHint: '登录跳板机并连到目标机所用的 SSH 端口（默认 23333）。',
        jumpHostSshPortInvalid: '跳板机 SSH 端口必须在 1 到 65535 之间。',
```

- [ ] **Step 3: Import the helper and add state + validation**

In `src/pages/EnableApplianceSshPage.vue`, extend the presentation import (`src/pages/EnableApplianceSshPage.vue:7`):

```typescript
import { getApplianceSshEnableState, isValidSshPort } from '../lib/applianceSshPresentation';
```

After the `jumpHostPassword` state (`src/pages/EnableApplianceSshPage.vue:53`):

```typescript
const jumpHostPassword = ref<string>('');
const jumpHostSshPort = ref<number>(23333);
```

After the `hasAnyJumpHost` computed (`src/pages/EnableApplianceSshPage.vue:179`):

```typescript
const jumpHostSshPortInvalid = computed(
  () => hasAnyJumpHost.value && !isValidSshPort(jumpHostSshPort.value),
);
```

- [ ] **Step 4: Add the submit guard**

In the submit handler, right after the `hasWhitelistConfigError` guard block (`src/pages/EnableApplianceSshPage.vue:331-334`):

```typescript
  if (jumpHostSshPortInvalid.value) {
    pushToast(t('tools.applianceSsh.jumpHostSshPortInvalid'), 'warning');
    return;
  }
```

- [ ] **Step 5: Add the request field**

In the `enableApplianceSsh({ ... })` call, after `jumpHostPassword:` (`src/pages/EnableApplianceSshPage.vue:362`):

```typescript
      jumpHostPassword: useSeparateJumpHostCreds.value ? jumpHostPassword.value : undefined,
      jumpHostSshPort: hasAnyJumpHost.value ? jumpHostSshPort.value : undefined,
```

- [ ] **Step 6: Add the port input to the jump-host card**

In `src/pages/EnableApplianceSshPage.vue`, between the pairs `v-else` block's closing `</div>` (`src/pages/EnableApplianceSshPage.vue:652`) and the "Recent jump-host → target pairs" comment (`src/pages/EnableApplianceSshPage.vue:654`), insert:

```html
            <!-- Jump host SSH port (shown whenever a jump-host pair exists) -->
            <div v-if="hasAnyJumpHost" class="px-5 pb-4 pt-0">
              <label class="block text-xs font-medium text-slate-600 mb-1.5">{{ t('tools.applianceSsh.jumpHostSshPort') }}</label>
              <input
                v-model.number="jumpHostSshPort"
                type="number"
                min="1"
                max="65535"
                :disabled="isLoading"
                class="w-32 px-3 py-2 text-sm font-mono border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                :class="jumpHostSshPortInvalid ? 'border-red-300 focus:border-red-400 focus:ring-red-400/20' : ''"
              />
              <p class="text-xs text-slate-400 mt-1.5">{{ t('tools.applianceSsh.jumpHostSshPortHint') }}</p>
            </div>
```

- [ ] **Step 7: Type-check + re-run the frontend logic test**

Run: `pnpm check`
Expected: PASS — no `vue-tsc` errors.
Run: `node src/lib/applianceSshPresentation.test.mjs`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/lib/tauri.ts src/pages/EnableApplianceSshPage.vue src/locales/messages.ts
git commit -m "feat(appliance-ssh): 跳板机 SSH 端口输入框与请求透传"
```

---

### Task 5: Contract doc + final versioned build

**Files:**
- Modify: `.trellis/spec/backend/appliance-ssh.md`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Update the contract doc signatures**

In `.trellis/spec/backend/appliance-ssh.md`, add to the `EnableApplianceSshRequest` interface (after `jumpHostPassword?: string;`):

```typescript
  jumpHostSshPort?: number;
```

Add to the Rust `ApplianceSshRequest` struct (after `jump_host_password: Option<String>,`):

```rust
    pub jump_host_ssh_port: Option<u16>,
```

- [ ] **Step 2: Add the new contract bullets**

Append to section `### 3. Contracts`:

```markdown
- Jump-host SSH port resolves by priority: user-supplied `jumpHostSshPort` (non-zero) → status-API `port` → `23333`. The resolved port is used for BOTH the SSH login to the jump host AND the nested jump-host→target hop (replacing the old hardcoded `JUMP_HOST_DEFAULT_TARGET_SSH_PORT`).
- When `SSH/set` fails for a **jump-host** target, the flow does NOT abort. It logs a `warn` ("management API ... unavailable; degrading to SSH channel") and falls through to the SSH step: if a whitelist rule is requested, it applies it over SSH; otherwise it runs an SSH-login probe to the jump host. Degraded success/failure messages state the management API was unavailable, and degraded failures include both the API error and the SSH error.
- Degradation is jump-host-only. A **direct** (non-jump-host) target with a failing `SSH/set` still returns `Failed to enable SSH: ...` as before, because SSH into a direct target is blocked by the very firewall this tool opens.
- Credential resolution is centralized in `resolve_appliance_ssh_creds`: jump-host targets prefer non-blank separate jump-host creds, else fall back to the main SSH creds; direct targets always use the main creds.
```

Add to the `### 4. Validation & Error Matrix` table:

```markdown
| Jump-host `SSH/set` fails, whitelist on | Rust target worker | Log warn, degrade, apply iptables over SSH; message notes API unavailable |
| Jump-host `SSH/set` fails, whitelist off | Rust target worker | Log warn, degrade, SSH-login probe to jump host decides success |
| Direct target `SSH/set` fails | Rust target worker | Return `Failed to enable SSH: ...` (no degrade) |
| `jumpHostSshPort` supplied | Rust resolver | Use it for jump-host login + nested hop, over status/default |
```

- [ ] **Step 3: Run the full verification gate**

Run: `cargo test --manifest-path src-tauri/Cargo.toml appliance`
Expected: PASS.
Run: `pnpm check`
Expected: PASS.
Run: `node src/lib/applianceSshPresentation.test.mjs`
Expected: PASS.

- [ ] **Step 4: Versioned build (CLAUDE.md gate)**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: `pnpm tauri build` succeeds, then the exe is renamed to `file-sync-tool-1.0.0-YYYYMMDDHHmm.exe` and the manifest updates. If the build fails, fix before committing.

- [ ] **Step 5: Commit**

```bash
git add .trellis/spec/backend/appliance-ssh.md
git commit -m "docs(appliance-ssh): 补充降级契约与端口解析优先级"
```

---

## Manual verification (post-implementation, with the user)

The async flow and Vue UI are not unit-tested; confirm end-to-end in the real app:

1. Jump-host pair `55 → 157`, management API port unreachable (the reported bug). Expect: `warn` "degrading to SSH channel" in the log, then the iptables whitelist applied over SSH, result marked success with a "management API unavailable" note — no longer blocked by the `23006` timeout.
2. Jump-host SSH port field defaults to `23333`, is editable, rejects out-of-range values (red border + toast), and the entered port is used for the SSH login (visible in the `sshExec`/`command` log line).
3. Direct (non-jump-host) target with an unreachable API still fails with `Failed to enable SSH` (no degrade).
4. Happy path (API reachable) is unchanged.
