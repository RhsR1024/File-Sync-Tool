# Appliance Access Control — Jump-Host SSH Port + Degrade-to-SSH (Design)

**Date**: 2026-07-08
**Status**: Draft / awaiting user review
**Scope**: `EnableApplianceSshPage.vue` + `messages.ts` (frontend), `tauri.ts` (types), `main.rs` (backend flow), `.trellis/spec/backend/appliance-ssh.md` (contract).
**Branching**: Edits land directly on `main`.

---

## 1. Problem

For a jump-host pair (e.g. `55 → 157`), "Appliance Access Control" today does two
serial, independent things ([main.rs:2969-3295](../../../src-tauri/src/main.rs#L2969-L3295)):

1. **Management HTTP API** (`SSH/set` on `jumpHost:23006`) — the primary step, always
   run first, meant to turn on sshd via the appliance management plane. `api_ip` is the
   jump host ([main.rs:3018](../../../src-tauri/src/main.rs#L3018)).
2. **SSH whitelist** (only reached if step 1 succeeds) — SSH into the jump host, then
   nested SSH into `target:23333`, applying iptables ([main.rs:3182-3202](../../../src-tauri/src/main.rs#L3182-L3202)).

Observed failures, all confirmed in code:

- **The "jump host uses separate SSH creds" checkbox does not skip the API.** It only
  selects which credentials the *SSH* step uses ([main.rs:3130-3145](../../../src-tauri/src/main.rs#L3130-L3145)).
- **API failure aborts the whole run.** `SSH/set` error hits a `return` at
  [main.rs:3065-3073](../../../src-tauri/src/main.rs#L3065-L3073) and never reaches the SSH step —
  so even when `55` can SSH to `157` and the iptables rules already exist, one `23006`
  timeout kills the run.
- **No jump-host SSH port field.** The SSH login port is derived from the status API
  (`SSH/get`), falling back to 23333 ([main.rs:3118](../../../src-tauri/src/main.rs#L3118),
  [main.rs:3219](../../../src-tauri/src/main.rs#L3219)); there is no UI input, and in the
  failure case the flow aborts before that fallback is ever used.

## 2. Decisions (confirmed with user)

- **Strategy**: API-first, **degrade to SSH on API failure** (mirrors the existing
  best-effort-GET philosophy). Not SSH-first.
- **Degrade action**: run only the **existing iptables whitelist step**. Do *not* run any
  "start sshd" shell command — being able to SSH into the jump host already implies its
  sshd is up, and the API step is redundant for reaching the target.
- **New field**: jump-host SSH port, default `23333`.
- **(a) Port field placement**: shown whenever a jump-host pair exists (not bound to the
  separate-creds checkbox).
- **(b) Degrade scope**: **jump-host pairs only.** Direct (non-jump-host) targets keep
  today's behavior — API failure still fails, because SSH into a direct target is itself
  blocked by the firewall this tool is meant to open (chicken-and-egg).
- **(c) Degrade + whitelist unchecked**: SSH-login probe to the jump host; login success
  ⇒ `success=true`, login failure ⇒ `success=false` with both errors. Avoids
  "degraded but did nothing".

## 3. Frontend (`EnableApplianceSshPage.vue` + `messages.ts`)

- New reactive state `jumpHostSshPort = ref<number>(23333)`.
- New numeric input "Jump host SSH port", default `23333`, validated 1–65535, **shown
  whenever `hasAnyJumpHost` is true** (independent of `useSeparateJumpHostCreds`).
- Included in the request: `jumpHostSshPort: hasAnyJumpHost.value ? jumpHostSshPort.value : undefined`.
- i18n keys added to both `en` and `zh`: `jumpHostSshPort` (label), plus placeholder/hint
  as needed.

## 4. Types & Contract

```typescript
// src/lib/tauri.ts
export interface EnableApplianceSshRequest {
  // ...existing...
  jumpHostSshPort?: number;
}
```

```rust
// src-tauri/src/main.rs
pub struct ApplianceSshRequest {
    // ...existing...
    #[serde(default)]
    pub jump_host_ssh_port: Option<u16>,
}
```

**Port resolution priority** (single helper, used by both success and degraded paths):
`user-supplied jump_host_ssh_port` → `status API port` → `23333`. The resolved port is
used for **both** the SSH login to the jump host **and** the nested SSH from jump host to
target (replacing the hardcoded `JUMP_HOST_DEFAULT_TARGET_SSH_PORT = 23333` at
[main.rs:3188](../../../src-tauri/src/main.rs#L3188)). Appliance master/backup pairs
typically share the SSH port; the user's iptables shows both at 23333.

## 5. Backend flow (`enable_appliance_ssh_for_target`)

Jump-host pair, new sequence:

1. `SSH/get` — best-effort, unchanged.
2. `SSH/set`:
   - success ⇒ existing verify + whitelist path, unchanged (except port source, §4).
   - **failure ⇒ do not `return`.** Emit a `warn`, set `degraded = true`, skip the
     verification poll, fall through to the whitelist SSH step.
3. Whitelist SSH step (reached by both success and degraded paths):
   - SSH into the jump host with jump-host creds (separate or main) on the resolved port,
     nested SSH into the target on the resolved port, apply iptables.
   - Whitelist SSH success ⇒ `success=true`; message annotated
     "(management API unavailable, completed over SSH)" when `degraded`.
   - Whitelist SSH failure ⇒ `success=false`; message carries **both** the API error and
     the SSH error.
4. Degrade + `add_whitelist_rule == false`: SSH-login probe to the jump host on the
   resolved port. Login ⇒ `success=true` ("API unavailable, SSH reachable"); no login ⇒
   `success=false` with both errors.

Direct targets: `SSH/set` failure keeps the current `return`-with-error behavior.

## 6. Logging & result presentation

- At the degrade point emit a clear `warn`:
  `[appliance-access] target=<t> management API <jumpHost>:<port> unavailable (<err>); degrading to SSH channel`.
- `result.message` on both degraded success and degraded failure explicitly states the SSH
  channel was used. UI result-chip structure is unchanged (reuses existing `message`).

## 7. Contract doc (`.trellis/spec/backend/appliance-ssh.md`)

Add contracts:
- `EnableApplianceSshRequest.jumpHostSshPort` / `ApplianceSshRequest.jump_host_ssh_port`
  signatures; serde defaults to `None`.
- Jump-host SSH port resolution priority (user → status → 23333), applied to both the
  jump-host login and the nested target hop.
- Jump-host `SSH/set` failure degrades to the SSH whitelist step instead of aborting;
  degraded result marks success/failure over the SSH channel, failure carries both errors.
- Degradation is jump-host-only; direct targets still abort on `SSH/set` failure.
- Degrade + whitelist-off ⇒ SSH-login probe determines success.

## 8. Tests & verification

- Rust unit: port-resolution priority (user > status > 23333).
- Rust unit: jump-host `SSH/set` failure enters the degraded whitelist path rather than
  returning (may require making the API call injectable, or at minimum testing the pure
  port-selection / branch-decision helpers).
- Frontend: request assembly includes `jumpHostSshPort`; default is 23333.
- Build gates: `pnpm check`, `cargo test --manifest-path src-tauri/Cargo.toml appliance_ssh`,
  then `cmd /c pnpm tauri:build:versioned-exe` per CLAUDE.md.
