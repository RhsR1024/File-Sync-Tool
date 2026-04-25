# Update Checker & Release History — Design Spec

- **Date**: 2026-04-25
- **Status**: Approved (brainstorming complete)
- **Owner**: codex-agent
- **Scope**: In-app update checker, downloader, replacer, and release-history viewer for the Tauri desktop app.

---

## 1. Background & Goal

The app is distributed as a single `.exe` over an internal LAN (no installer,
no code signing, no GitHub releases). Today users have to ask the developer
for a new build URL whenever they hear about an update. We want the app to:

- Detect a newer release on a configurable internal server.
- Show what changed across recent versions (replaces ad-hoc Wiki / IM messages).
- Download and self-replace with one click — no manual file copy.

Hard constraints:

- **No nginx / no heavy server** — a single `python3 -m http.server` (or our
  bundled `serve.py`) is sufficient.
- **Internal HTTP only** (HTTPS supported by `reqwest` for free, but not
  required).
- **No code signing** — integrity protected by SHA-256 in the manifest.
- **Simple to operate** — publishing a new release is `cp xxx.exe DIR/` plus
  one append to `manifest.json`; no service restart.

Non-goals:

- Differential / delta updates.
- Mandatory updates that block app use until applied.
- Any update flow on macOS/Linux (Windows-only — matches the rest of the app).
- Tracking per-machine "I installed version X on date Y" history.

---

## 2. Server-Side Conventions

### 2.1 Directory layout

```
/opt/file-sync-tool-releases/
├── manifest.json
├── file-sync-tool-1.0.8-202604261000.exe
├── file-sync-tool-1.0.7-202604191000.exe
└── file-sync-tool-1.0.6-202604151000.exe
```

The `*.exe` files are exactly what `pnpm tauri:build:versioned-exe` already
produces — no naming change required.

### 2.2 Hosting (zero-code option)

Drop the directory contents on a Linux box and run either:

**Quick start (no service):**

```bash
cd /opt/file-sync-tool-releases
python3 -m http.server 8080
```

**Persistent (systemd unit):**

```ini
# /etc/systemd/system/file-sync-tool-releases.service
[Unit]
Description=File Sync Tool Release Server
After=network.target

[Service]
WorkingDirectory=/opt/file-sync-tool-releases
ExecStart=/usr/bin/python3 -m http.server 8080
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

```bash
systemctl enable --now file-sync-tool-releases
```

This spec also ships `scripts/release-server/serve.py` — a 30-line Python
file the user can copy to a server. It is functionally identical to
`python3 -m http.server 8080` but pins the working directory and port and
prints helpful logs. The client does not care which one is running.

Publishing a new release:

1. Build: `pnpm tauri:build:versioned-exe`
2. Copy: `scp src-tauri/target/release/file-sync-tool-X.Y.Z-*.exe server:/opt/file-sync-tool-releases/`
3. Edit: prepend a new entry to `manifest.json` `versions[]` and bump
   `latest`.

No service restart is needed — `python -m http.server` reads the directory
on every request.

### 2.3 manifest.json schema

```json
{
  "latest": "1.0.8",
  "versions": [
    {
      "version": "1.0.8",
      "url": "file-sync-tool-1.0.8-202604261000.exe",
      "sha256": "ab12...ef",
      "released_at": "2026-04-26",
      "changelog": [
        "修复磁盘清理偶发崩溃",
        "新增错误码查询工具",
        "剪贴板预览支持图片缩放"
      ]
    },
    {
      "version": "1.0.7",
      "url": "file-sync-tool-1.0.7-202604191000.exe",
      "sha256": "cd34...ab",
      "released_at": "2026-04-19",
      "changelog": ["重构剪贴板管理器", "..."]
    }
  ]
}
```

Field rules:

| Field | Type | Notes |
|---|---|---|
| `latest` | string | semver. Must equal `versions[0].version` (client tolerates mismatch by trusting `versions[0]`). |
| `versions[]` | array | Newest first. Empty array is treated as "no updates". |
| `versions[].version` | string | semver, parsed by `semver` crate. Pre-release tags (`-beta.1`) supported but unused. |
| `versions[].url` | string | **Absolute or relative**. Relative is resolved against the configured base URL. |
| `versions[].sha256` | string | Hex, lowercase. 64 chars. |
| `versions[].released_at` | string | ISO date `YYYY-MM-DD`. Display only — not parsed. |
| `versions[].changelog` | array of strings | Each item is one bullet. **Plain text, no markdown.** |

Unknown extra fields are ignored. Missing required fields cause that version
entry to be silently dropped (logged at WARN); the manifest as a whole is
still usable.

---

## 3. User Experience

### 3.1 Triggers for "check for update"

| Trigger | Behavior |
|---|---|
| App startup, **release build only** | Spawn background task, wait 5 s, check `last_update_check_at` throttle (24 h); if not throttled, fetch manifest. |
| User clicks "立即检查" on `/about` | Synchronous fetch with loading indicator. Result is shown in-page + dialog if applicable. |
| User opens settings → toggles `notify_on_new_version` ON | No immediate check (toggle only governs popup behavior). |
| Debug / dev build (`cfg!(debug_assertions)`) | All triggers are no-ops. About page shows badge "开发模式 — 更新检查已禁用". |

### 3.2 Notification policy

Two states governed by **AppConfig.notify_on_new_version** (default: **OFF**):

- **OFF (default)** — When startup auto-check finds a newer version, the app
  is silent except for a **red dot + up-arrow icon** rendered next to the
  version chip in the bottom-left of the sidebar. Clicking the chip opens
  `/about`.
- **ON** — Same red-dot indicator **plus** an `UpdateDialog` modal pops up
  automatically (non-blocking — user can dismiss).

Manual checks (button click on `/about`) **always** open `UpdateDialog` if a
newer version exists, regardless of the toggle.

The red-dot is cleared when:
- The current version meets or exceeds `manifest.latest`, or
- Manifest fetch returns no newer version on the next check.

The "稍后提醒" button does **not** clear the red-dot — only an actual upgrade
or a server-side rollback does.

### 3.3 UpdateDialog state machine

```
┌─────────────────────────────────────────┐
│ State 1: NEW VERSION FOUND              │
│  Title: 🚀 发现新版本                   │
│  Body: current → latest, released_at,   │
│        bulleted changelog               │
│  Buttons: [稍后提醒]  [立即升级]         │
└─────────────────────────────────────────┘
         │ 立即升级
         ▼
┌─────────────────────────────────────────┐
│ State 2: DOWNLOADING                    │
│  Progress bar (% + bytes + speed)       │
│  Buttons: [取消]                        │
└─────────────────────────────────────────┘
         │ 完成 + SHA256 校验
         ▼
┌──────────────────────┐  ┌──────────────────────┐
│ 3a READY             │  │ 3b VERIFY FAILED     │
│ ✅ 已下载并校验通过   │  │ ❌ 文件校验失败       │
│ [稍后]  [立即重启升级] │  │           [关闭]      │
└──────────────────────┘  └──────────────────────┘
         │ 立即重启升级
         ▼
   spawn helper.bat → exit(0)

State 3c: NETWORK / HTTP ERROR (alternate from State 2)
  ❌ 下载失败：<原因>
  Buttons: [重试]  [关闭]
```

State 3a "稍后" persists the downloaded file and `pending_update` config
field. On the **next app startup**, if the temp file still exists and its
SHA-256 still matches, the dialog opens directly in **State 3a** with text
"上次有未应用的更新（{version}），现在升级？".

### 3.4 `/about` page layout

```
┌─────────────────────────────────────────────────────────┐
│  ← 返回                              🛡️ 关于与更新       │
│                                                         │
│  当前版本：1.0.7                                        │
│  发布日期：2026-04-19                                   │
│  更新服务器：http://192.115.1.3:8080      [测试连接]    │
│                                            [立即检查]   │
│                                                         │
│  ┌─ 发现新版本 1.0.8 ────────────────────────────┐     │
│  │ 2026-04-26 发布                               │     │
│  │ • 修复磁盘清理崩溃                            │     │
│  │ • 新增错误码查询                              │     │
│  │ • 剪贴板预览支持图片缩放                      │     │
│  │                                  [立即升级]    │     │
│  └────────────────────────────────────────────────┘     │
│                                                         │
│  📜 历史版本                                            │
│  ┌──────────────────────────────────────────────┐       │
│  │ ▼ 1.0.7  • 当前  • 2026-04-19              │       │
│  │   • 重构剪贴板管理器                       │       │
│  │   • 稳定文件共享 UI                        │       │
│  ├──────────────────────────────────────────────┤       │
│  │ ▸ 1.0.6  • 2026-04-15                       │       │
│  ├──────────────────────────────────────────────┤       │
│  │ ▸ 1.0.5  • 2026-04-10                       │       │
│  └──────────────────────────────────────────────┘       │
│                                                         │
│  Debug build (only): "开发模式 — 更新检查已禁用"          │
└─────────────────────────────────────────────────────────┘
```

- The "发现新版本" banner is shown only when `has_update === true`.
- Each history row is collapsible. The current version is auto-expanded.
- "测试连接" button: `GET ${url}/manifest.json` with 5 s timeout; success
  shows `"连接成功"` toast/log; failure shows the precise reason.
- "立即检查" calls `check_update` regardless of throttle.
- Server URL on this page is **read-only**; users edit it on the Settings
  page.

### 3.5 Sidebar version chip

Existing layout:

```
🛡 1.0.7 · 2026.04.19
```

After this feature:

```
🛡 1.0.7 · 2026.04.19    🔴↑   ← red dot + arrow when has_update
```

The whole chip area becomes a button → `router.push('/about')`. When there is
no update available, the chip looks identical to today (no dot). The hover
tooltip uses i18n key `sidebar.versionChipTooltip` → zh `"打开关于与更新页"`,
en `"Open About & Updates"`.

### 3.6 Settings page additions

A new section **"更新检查"** under the existing settings list, with two
fields:

| Label (zh / en) | Type | Default | Validation |
|---|---|---|---|
| 有新版本时弹窗提示 / Notify on new version | toggle | OFF | — |
| 更新服务器地址 / Update server URL | text input | `http://192.115.1.3:8080` | Must parse as `http(s)://host[:port][/path]`; trailing `/` stripped on save |

If the URL is empty after trim, the auto-check is disabled and the about
page shows "未配置更新服务器" instead of new-version banners.

---

## 4. Backend Architecture

### 4.1 Module layout

```
src-tauri/src/updater/
├── mod.rs           # Re-exports, types, UpdaterState
├── manifest.rs      # Fetch + parse + URL resolve + version compare
├── download.rs      # Streaming download + progress + SHA-256 verify + cancel
├── installer.rs     # helper.bat template + spawn + exit(0)
├── pending.rs       # Persist / restore pending_update across app restarts
└── commands.rs      # Tauri command handlers
```

`UpdaterState` lives inside `AppState` and carries the in-memory snapshot of
the latest manifest, current download task handle (for cancel), and a
broadcast `tokio::sync::watch` for download progress.

### 4.2 Tauri commands

| Command | Async | Returns | Notes |
|---|---|---|---|
| `check_update` | yes | `UpdateCheckResult` | Always re-fetches manifest. Updates `last_update_check_at`. |
| `start_update_download` | yes | `()` | Starts a tokio task; emits `update-download-progress` events. Errors via `Err`. |
| `cancel_update_download` | yes | `()` | Sets the cancel flag; the task aborts and cleans up. |
| `apply_update_now` | yes | `()` (never returns on success) | Spawns helper.bat, clears pending_update, calls `std::process::exit(0)`. |
| `test_update_server` | yes | `TestServerResult` | `GET ${url}/manifest.json` with 5 s timeout, parse JSON. Used by the about-page button. |
| `get_update_state` | sync | `UpdateState` | UI mount-time read of: manifest snapshot, has_update, pending_update, server URL. |

### 4.3 Type contracts (shared with frontend)

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct ManifestVersion {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub released_at: String,
    pub changelog: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub latest: String,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Serialize, Clone)]
pub struct UpdateCheckResult {
    pub has_update: bool,
    pub current: String,
    pub latest: Option<String>,
    pub manifest: Option<Manifest>,
}

#[derive(Serialize, Clone)]
pub struct TestServerResult {
    pub ok: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PendingUpdate {
    pub target_version: String,
    pub temp_path: String,
    pub sha256: String,
    pub downloaded_at: String, // RFC3339
}

#[derive(Serialize, Clone)]
pub struct UpdateState {
    pub current: String,
    pub server_url: String,
    pub manifest: Option<Manifest>,
    pub has_update: bool,
    pub last_checked_at: Option<String>,
    pub pending_update: Option<PendingUpdate>,
    pub debug_build: bool,
}
```

### 4.4 Events (Rust → frontend)

| Event | Payload | When |
|---|---|---|
| `update-state-changed` | `UpdateState` | After every check / download / pending_update mutation |
| `update-download-progress` | `{ downloaded: u64, total: Option<u64>, speed_bps: u64 }` | At most every 100 ms during download |
| `update-download-complete` | `{ version: String, temp_path: String, sha256_ok: bool, error: Option<String> }` | Once per download |

The frontend subscribes via `listen()` from `@tauri-apps/api/event`, just
like existing pages do for clipboard events.

### 4.5 Manifest fetch + version compare

```
1. Resolve URL: ${server_url}/manifest.json (server_url has trailing / stripped)
2. reqwest GET with 10 s timeout, no auth
3. On HTTP error → return enum UpdaterError
4. Parse JSON; drop entries missing required fields (log WARN)
5. For each version, normalize url to absolute by joining with server_url if relative
6. Compute current_sv = semver::Version::parse(env!("CARGO_PKG_VERSION"))
   latest_sv = semver::Version::parse(manifest.latest)
   has_update = latest_sv > current_sv
7. Update UpdaterState; persist last_update_check_at via existing config.save flow
8. Emit update-state-changed
```

Edge cases:

- `manifest.latest` differs from `versions[0].version` → trust `versions[0]`.
- `versions[]` is empty → `has_update = false`, `manifest` still stored
  (used by the history list, which would show nothing).
- `manifest.latest` is older than current → `has_update = false`.

### 4.6 Streaming download + SHA-256

```
1. resolve ManifestVersion of target version (must equal manifest.latest)
2. Build full URL (absolute or join with server_url)
3. reqwest::Client::get(...).send().await
4. Read response.headers().get(CONTENT_LENGTH) for total
5. Open BufWriter on %TEMP%/file-sync-tool-update-<random>.exe
6. Sha256 hasher init
7. Loop: response.chunk().await:
     - check cancel flag → break
     - write to file
     - update sha256
     - update progress counter
     - emit progress event (throttled to 100 ms)
8. Close file
9. Compare hex(sha256) to manifest entry's sha256 (case-insensitive)
10. If mismatch → delete temp file, return Err(VerifyFailed)
11. Else: store temp_path on UpdaterState; emit update-download-complete; return Ok
```

The cancel flag is a `tokio::sync::watch::Sender<bool>` held in
`UpdaterState`. `cancel_update_download` flips it; the download loop sees
the next chunk boundary and aborts.

### 4.7 Apply update (helper.bat orchestration)

```
1. Verify pending download exists (or use temp_path passed via state)
2. Determine current exe path: std::env::current_exe()
3. Compose helper.bat string from include_str! template
4. Write to %TEMP%/fst-update-<random>.bat
5. Spawn: std::process::Command::new("cmd.exe")
            .args(["/c", "start", "", "/min",
                   helper_bat_path, &pid.to_string(), &temp_path, &exe_path])
            .spawn()
6. Sleep 50 ms (give cmd time to launch)
7. Clear pending_update from config (it has been "consumed")
8. Save config
9. Call window.close() on all webview windows for graceful shutdown
10. std::process::exit(0)
```

`helper.bat` template (embedded via `include_str!("updater.bat")`):

```bat
@echo off
setlocal
:wait
tasklist /FI "PID eq %~1" /NH 2>nul | find " %~1 " >nul
if %errorlevel% equ 0 ( timeout /t 1 /nobreak >nul & goto wait )
timeout /t 1 /nobreak >nul
if exist "%~3.old" del /f /q "%~3.old" 2>nul
if exist "%~3" move /y "%~3" "%~3.old" >nul 2>nul
move /y "%~2" "%~3" >nul
start "" "%~3"
(goto) 2>nul & del "%~f0"
```

Notes:
- `find " %~1 "` (with surrounding spaces) avoids partial matches in the
  memory-usage column. tasklist's `/NH` flag prefixes header.
- The new exe inherits no parent process. `tauri-plugin-single-instance`
  acquires its mutex cleanly because the parent is fully gone before the
  bat calls `start`.
- The renamed `*.old` file is left on disk. The new exe **does not** clean
  it up (avoids race where users immediately re-launch the old one). It can
  be deleted manually if disk space matters.

### 4.8 pending_update lifecycle

| Transition | Effect |
|---|---|
| `start_update_download` succeeds + verified | Write `pending_update = { … }` to config |
| `cancel_update_download` | Delete temp file; clear `pending_update`; save config |
| `apply_update_now` succeeds | Clear `pending_update`; save config; exit |
| App startup, `pending_update.is_some()` | Verify temp file still exists + sha256 still matches; if so, open dialog in State 3a; else clear field |

### 4.9 Throttle & startup background task

```
On app startup:
  if cfg!(debug_assertions): return
  spawn tokio task:
    sleep 5 s
    let last = config.last_update_check_at
    if last is Some && (now - last) < 24h: return
    let result = manifest::check(server_url).await
    match result:
      Ok(check_result):
        emit update-state-changed
        if check_result.has_update && config.notify_on_new_version:
          // Tell frontend to open the dialog
          emit "open-update-dialog"
      Err(_):
        log WARN; do nothing user-facing
```

The 5 s delay avoids contending with the existing app-init work (clipboard
DB warmup, scanner init, etc.).

### 4.10 New dependency

Add to `src-tauri/Cargo.toml`:

```toml
semver = "1"
```

`reqwest` (with stream feature already enabled), `sha2`, `chrono`,
`tokio` are already present.

---

## 5. Frontend Architecture

### 5.1 Files

| Path | Purpose |
|---|---|
| `src/pages/AboutPage.vue` | Route `/about`, version + history + manual check + banner |
| `src/components/UpdateDialog.vue` | Global state-machine modal (3 states + variants) |
| `src/components/UpdateRedDot.vue` | Tiny indicator used inside `Sidebar.vue` |
| `src/composables/useUpdater.ts` | Thin store wrapping `UpdateState` + listeners |
| `src/lib/tauri.ts` | Add 6 typed wrappers + types from §4.3 |
| `src/lib/sidebarNavigation.ts` | (no nav entry — about is reached via chip click; route still registered) |
| `src/components/Sidebar.vue` | Make version chip clickable + render `UpdateRedDot` |
| `src/router/index.ts` | Register `/about` route |
| `src/pages/SettingsPage.vue` | Add "更新检查" section with toggle + URL input |
| `src/locales/messages.ts` | Add `about.*`, `updater.*`, `settings.update.*` (zh + en) |

### 5.2 `useUpdater` composable

A small reactive wrapper over `UpdateState`. Mounts listeners on first call,
exposes refs the rest of the app reads:

```ts
import { ref, readonly } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { updaterApi, type UpdateState, type DownloadProgress } from '@/lib/tauri';

const state = ref<UpdateState | null>(null);
const progress = ref<DownloadProgress | null>(null);
const dialogOpen = ref(false);
const dialogState = ref<'found' | 'downloading' | 'ready' | 'verify_failed' | 'error'>('found');
const dialogError = ref<string | null>(null);

let initialized = false;
async function init() {
  if (initialized) return;
  initialized = true;
  state.value = await updaterApi.getState();
  await listen<UpdateState>('update-state-changed', (e) => { state.value = e.payload; });
  await listen<DownloadProgress>('update-download-progress', (e) => { progress.value = e.payload; });
  await listen<{ ok: boolean; error: string | null }>('update-download-complete', (e) => {
    dialogState.value = e.payload.ok ? 'ready' : 'verify_failed';
    dialogError.value = e.payload.error;
  });
  await listen('open-update-dialog', () => {
    if (state.value?.has_update) { dialogState.value = 'found'; dialogOpen.value = true; }
  });
}

export function useUpdater() {
  if (!initialized) void init();
  return {
    state: readonly(state),
    progress: readonly(progress),
    dialogOpen,
    dialogState,
    dialogError,
  };
}
```

`useUpdater()` is called by `App.vue` (so the listeners are mounted globally)
and by `AboutPage.vue` / `Sidebar.vue` / `UpdateDialog.vue` for reading.

### 5.3 `tauri.ts` additions

```ts
export interface ManifestVersion {
  version: string;
  url: string;
  sha256: string;
  released_at: string;
  changelog: string[];
}

export interface Manifest {
  latest: string;
  versions: ManifestVersion[];
}

export interface PendingUpdate {
  target_version: string;
  temp_path: string;
  sha256: string;
  downloaded_at: string;
}

export interface UpdateState {
  current: string;
  server_url: string;
  manifest: Manifest | null;
  has_update: boolean;
  last_checked_at: string | null;
  pending_update: PendingUpdate | null;
  debug_build: boolean;
}

export interface DownloadProgress {
  downloaded: number;
  total: number | null;
  speed_bps: number;
}

export interface UpdateCheckResult {
  has_update: boolean;
  current: string;
  latest: string | null;
  manifest: Manifest | null;
}

export interface TestServerResult {
  ok: boolean;
  status: number | null;
  error: string | null;
}

export const updaterApi = {
  getState: () => invoke<UpdateState>('get_update_state'),
  check: () => invoke<UpdateCheckResult>('check_update'),
  startDownload: () => invoke<void>('start_update_download'),
  cancelDownload: () => invoke<void>('cancel_update_download'),
  applyNow: () => invoke<void>('apply_update_now'),
  testServer: () => invoke<TestServerResult>('test_update_server'),
};
```

### 5.4 i18n keys (illustrative — full list in implementation)

```
about.title                   "关于与更新" / "About & Updates"
about.currentVersion          "当前版本：{version}"
about.releasedOn              "发布日期：{date}"
about.serverLabel             "更新服务器：{url}"
about.testConnection          "测试连接" / "Test Connection"
about.checkNow                "立即检查" / "Check Now"
about.devModeBadge            "开发模式 — 更新检查已禁用"
about.bannerTitle             "发现新版本 {version}"
about.bannerReleasedOn        "{date} 发布"
about.upgradeCta              "立即升级"
about.history                 "历史版本"
about.currentTag              "当前"
about.serverNotConfigured     "未配置更新服务器，请到设置页填写"

updater.dialog.titleFound     "🚀 发现新版本"
updater.dialog.titleDownloading "正在下载 {version}…"
updater.dialog.titleReady     "✅ 已下载并校验通过"
updater.dialog.titleVerifyFail "❌ 文件校验失败"
updater.dialog.titleError     "❌ 下载失败"
updater.dialog.bodyCurrentLatest "当前版本：{current}    最新版本：{latest}（{date} 发布）"
updater.dialog.changelogHeader "更新内容："
updater.dialog.actionLater    "稍后提醒"
updater.dialog.actionUpgrade  "立即升级"
updater.dialog.actionCancel   "取消"
updater.dialog.actionRestart  "立即重启升级"
updater.dialog.actionRetry    "重试"
updater.dialog.actionClose    "关闭"
updater.dialog.actionLaterRestart "稍后"
updater.dialog.progress       "{percent}%  ·  {downloaded} / {total}  ·  {speed}/s"
updater.dialog.verifyHint     "下载的文件可能损坏。请稍后重试。"
updater.dialog.resumePrompt   "上次有未应用的更新（{version}），现在升级？"

updater.toast.upToDate        "已是最新版本"
updater.toast.networkFail     "无法连接到更新服务器：{detail}"
updater.toast.testOk          "连接成功"
updater.toast.testFail        "连接失败：{detail}"
updater.toast.cancelled       "已取消下载"

settings.update.section       "更新检查"
settings.update.notifyToggle  "有新版本时弹窗提示"
settings.update.serverLabel   "更新服务器地址"
settings.update.serverPlaceholder "http://192.115.1.3:8080"
settings.update.serverHint    "支持 http/https，留空将禁用自动检查"
```

Both `zh` and `en` translations must be provided.

---

## 6. Data Flow Summary

```
                Startup (release build)
                       │
                       ▼
       wait 5 s ──► throttle check ──► fetch manifest
                                          │
                                          ▼
                              compare versions; emit state
                                          │
                  has_update?  ───── no ─► (silent)
                       │ yes
                       ▼
              notify_on_new_version?
                       │
            ┌────── yes ─┴── no ──────┐
            ▼                          ▼
       open dialog              red dot only
                                          ▲
                                          │
                  user clicks chip ───────┘
                       │
                       ▼
                   /about page
                       │
                       ▼
   user clicks "立即升级" → start_update_download
                       │
        progress events ◄──── tokio task
                       │
                       ▼
              SHA-256 verify
                       │
        ┌────── ok ────┴──── fail ──────┐
        ▼                                ▼
     state 3a                         state 3b
        │                                  │
"立即重启升级"                           "关闭"
        │
        ▼
 write helper.bat → spawn → exit(0)
        │
        ▼
   helper.bat: wait PID → rename → start new exe
```

---

## 7. Error Handling Matrix

| Scenario | Backend behavior | User-visible behavior |
|---|---|---|
| Server URL empty/blank | `check_update` returns `Err("server_not_configured")` | Toast `"请先在设置页配置更新服务器"`; about page banner replaced with hint |
| DNS / connect / timeout | `Err("network: …")` | Auto-check: silent log only. Manual check: toast `updater.toast.networkFail` |
| HTTP 4xx/5xx | `Err("http: <status>")` | Manual: toast with status. Auto: silent log. |
| Manifest JSON malformed | Drop entries that fail; if **all** entries fail, `Err("manifest_invalid")` | Manual: toast. Auto: silent log. |
| `latest` ≤ current | `has_update = false` | No banner, no dot. About page history list still renders. |
| Debug build | All commands return synthetic state with `debug_build: true` and `has_update: false` | About page shows `开发模式` badge; sidebar no dot. |
| Concurrent download attempt | `start_update_download` returns `Err("already_in_progress")` | Upgrade button is disabled while task is running, so only happens via direct invoke |
| User cancels download | Task aborts; partial file deleted | Dialog closes; toast `updater.toast.cancelled` |
| SHA-256 mismatch | Temp file deleted; `Err("verify_failed")` | Dialog → state 3b |
| Disk full during download | Write error; partial file deleted | Toast / dialog 3c with reason `"磁盘空间不足"` |
| `current_exe()` fails | `apply_update_now` returns `Err("exe_path_unknown")` | Dialog → state 3c with retry |
| helper.bat write fails | Same as above | Same |
| pending_update file missing on restart | Clear field silently | Same as no pending update |
| pending_update sha256 mismatch on restart | Clear field; delete file | Same |
| User renames the `.exe` (e.g., `我的工具.exe`) | `current_exe()` returns the renamed path; helper writes to that path | Upgrade succeeds; new file keeps the renamed name |

---

## 8. Configuration Migration

`AppConfig` gains four fields:

```rust
update_server_url: String,
notify_on_new_version: bool,
last_update_check_at: Option<String>,
pending_update: Option<PendingUpdate>,
```

Migration: when reading an old `config.json` that lacks these fields, populate
them with `Default` values:

- `update_server_url`: `"http://192.115.1.3:8080"`
- `notify_on_new_version`: `false`
- `last_update_check_at`: `None`
- `pending_update`: `None`

The existing `config::migrate` flow (see `config.rs`) covers this via serde's
`#[serde(default)]`.

---

## 9. Logging

All updater events flow through `log::*` macros (already routed by
`tauri-plugin-log` to `app.log`). Notable lines:

```
INFO  [updater] 启动检查更新（自动）
INFO  [updater] 跳过：24h 节流命中
INFO  [updater] 拉取 manifest http://192.115.1.3:8080/manifest.json
INFO  [updater] 当前 1.0.7  远端最新 1.0.8  has_update=true
INFO  [updater] 用户开始下载 1.0.8
INFO  [updater] 下载完成 7.7 MB  耗时 2.3s  SHA256 校验通过
INFO  [updater] 写 helper.bat 至 C:\Users\…\Temp\fst-update-xxxx.bat
INFO  [updater] 启动 helper，主进程退出
WARN  [updater] manifest 字段缺失：versions[2] 缺 sha256
ERROR [updater] 下载失败：connection reset
```

Also surfaced to MainConsole via existing `log-message` event when relevant.

---

## 10. Testing

### Backend unit tests (`cargo test`)

| Module | Tests |
|---|---|
| `manifest` | URL join (relative + absolute), JSON parse with malformed entries, `has_update` true/false/equal, empty `versions[]`, latest mismatch tolerance |
| `download` | SHA-256 of fixture bytes; cancel flag aborts loop; chunk-by-chunk progress aggregation |
| `installer` | `helper.bat` template content matches snapshot; PID/path arguments are correctly quoted (no spaces, no special chars) |
| `pending` | Round-trip `PendingUpdate` through serde; restart logic clears stale entries when temp file missing |

Mock servers use `wiremock` (add as dev-dependency).

### Frontend tests (`node --test`)

| File | Coverage |
|---|---|
| `src/pages/about/version.test.mjs` | Pure helpers: `formatReleaseDate`, `compareVersionsAsc`, `isCurrent` |
| `src/pages/about/changelog.test.mjs` | Helper that bullets the changelog array into `<li>` data |
| `src/composables/useUpdater.test.mjs` | If pure logic is extractable. Listener wiring is exercised manually. |

### Manual QA checklist (rehearsed against a real `python -m http.server`)

```
[ ] Configure URL pointing at a manifest with 1.0.8.
[ ] Restart app — within ~5 s see red dot in sidebar.
[ ] Toggle "弹窗提示" ON in settings; restart — dialog auto-pops.
[ ] Toggle OFF; restart — only red dot.
[ ] Click sidebar chip — /about opens; banner shows; history list shows ≥3 versions.
[ ] Click "测试连接" with valid URL → 连接成功.
[ ] Set URL to nonsense → 连接失败 with detail.
[ ] Click "立即检查" — dialog opens regardless of toggle.
[ ] Click "立即升级" — progress bar moves; cancel works.
[ ] Let download finish; verify check passes; click "立即重启升级" — app restarts as new version.
[ ] Repeat but click "稍后" instead → restart app → resume prompt appears.
[ ] Tamper with manifest sha256 → click 立即升级 → state 3b.
[ ] Disconnect mid-download → state 3c → click 重试 succeeds.
[ ] Run debug build (`pnpm tauri dev`) — about page shows 开发模式 badge; no checks fired.
```

---

## 11. Out of Scope (deferred)

Per Q10 and §1 non-goals:

- Differential / patch updates (always full `.exe` swap).
- Mandatory blocking updates.
- Automatic background downloads (download is always user-initiated).
- HTTPS certificate pinning.
- Proxy auto-detection.
- macOS / Linux support.
- Background "update available" tray notifications (the in-app dot + dialog
  cover the use cases).
- Per-machine install history page (manifest history is enough).
- "Skip this version" persisting state (Q5 chose option A — only a 24-h
  re-prompt).

---

## 12. Dependencies on Other Tasks

This feature is independent of the **Error Code Lookup** spec
(`docs/superpowers/specs/2026-04-25-error-code-lookup-design.md`). They
share no code paths, no state, and can be implemented in either order.

If both ship in the same release, the new "1.0.8" `manifest.json` will
naturally describe both features in its `changelog`.

---

## 13. Rollout Notes

- First version that *contains* the updater (e.g., 1.0.8) cannot
  auto-upgrade older builds (1.0.x without the feature) — those have to be
  upgraded manually one last time. After that, the chain self-sustains.
- Recommend publishing `manifest.json` *before* announcing the release
  internally so the in-app updater can pick it up immediately.
- The first server-side `manifest.json` should include the **current**
  shipped version as the only entry, so the history view isn't empty on
  first launch.
