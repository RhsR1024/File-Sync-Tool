# CHFS 风格文件共享 v2 Implementation Plan

## Implementation Status (2026-04-05)

- Automated verification now passes in the worktree:
  - `cargo test --manifest-path src-tauri/Cargo.toml fileshare -- --nocapture`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `pnpm check`
  - `pnpm lint`
  - `pnpm build:file-share-web`
  - `pnpm build`
- The desktop page, persisted v2 config flow, startup recovery, backend APIs, and embedded web manager pipeline are implemented in code.
- Remaining non-automated items are still manual acceptance scenarios from Task 7 Step 5, which should be exercised on a running app before final merge.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在现有文件共享 v1 基础上，落地 CHFS 风格的 v2：补齐持久化、账户与权限、会话时长、黑白名单、上传/新建/重命名/删除/搜索/图片预览，以及页面级与系统级自动恢复共享能力。

**Architecture:** Rust 端把当前单文件 `fileshare.rs` 拆成持久化、认证、文件操作、搜索、HTTP API 和 Web 资源模块；桌面端继续使用现有 Vue/Tauri 页面承载配置与状态；Web 端新增独立 Vue 文件管理界面，通过 Axum JSON API 访问后端，下载仍走直链流式接口。

**Tech Stack:** Tauri 2, Rust 2021, Axum 0.7, Vue 3, TypeScript, Vite, vue-i18n, existing `pnpm check` / `pnpm lint` / `cargo test` / `cargo clippy` workflows, planned `rust-embed`, `ipnet`, `trash`, `axum-extra`

---

### Task 1: Split File Share Backend And Add Persisted Config

**Files:**
- Create: `src-tauri/src/fileshare/mod.rs`
- Create: `src-tauri/src/fileshare/model.rs`
- Create: `src-tauri/src/fileshare/persist.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/fileshare/persist.rs`

- [ ] **Step 1: Write the failing persistence tests for defaults and password hashing**

```rust
#[test]
fn returns_default_v2_config_when_no_saved_settings_exist() {
    let tempdir = tempfile::tempdir().unwrap();
    let loaded = load_persisted_file_share_config_from_path(tempdir.path()).unwrap();

    assert_eq!(loaded.port, 8080);
    assert!(loaded.roots.is_empty());
    assert!(loaded.guest_access_enabled);
    assert_eq!(loaded.session_ttl_minutes, 30);
    assert_eq!(loaded.delete_mode, DeleteMode::RecycleBin);
}

#[test]
fn save_request_hashes_passwords_without_exposing_plaintext() {
    let saved = apply_save_request(
        None,
        FileShareSettingsSaveRequest {
            guest_password: Some("secret-123".into()),
            ..test_settings_request()
        },
    )
    .unwrap();

    assert!(saved.accounts.iter().any(|account| account.id == "guest" && account.password_hash.is_some()));
    assert!(saved.accounts.iter().all(|account| account.password_hash.as_deref() != Some("secret-123")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::persist -- --nocapture`  
Expected: FAIL because the v2 persisted model and persistence helpers do not exist yet

- [ ] **Step 3: Create v2 file share model types**

```rust
pub enum PermissionPreset {
    ReadOnly,
    ReadWrite,
    Custom,
}

pub enum DeleteMode {
    RecycleBin,
    Permanent,
}

pub enum IpFilterMode {
    Off,
    Whitelist,
    Blacklist,
}

pub struct FileShareRoot {
    pub id: String,
    pub alias: String,
    pub path: String,
    pub enabled: bool,
}

pub struct PersistedFileShareConfig {
    pub version: u32,
    pub port: u16,
    pub roots: Vec<FileShareRoot>,
    pub guest_access_enabled: bool,
    pub accounts: Vec<PersistedAccount>,
    pub session_ttl_minutes: u32,
    pub ip_filter_mode: IpFilterMode,
    pub ip_rules: Vec<String>,
    pub image_preview_enabled: bool,
    pub thumbnail_enabled: bool,
    pub delete_mode: DeleteMode,
    pub remember_settings: bool,
    pub auto_start_on_page_open: bool,
    pub auto_start_with_windows: bool,
}
```

- [ ] **Step 4: Implement file share persistence API and sensitive-field handling**

```rust
#[tauri::command]
pub fn file_share_load_settings(app: tauri::AppHandle) -> Result<FileShareSettingsView, String> {
    let saved = load_persisted_file_share_config(&app)?;
    Ok(FileShareSettingsView::from(saved))
}

#[tauri::command]
pub fn file_share_save_settings(
    app: tauri::AppHandle,
    request: FileShareSettingsSaveRequest,
) -> Result<FileShareSettingsView, String> {
    let saved = apply_save_request(load_persisted_file_share_config(&app).ok(), request)?;
    save_persisted_file_share_config(&app, &saved)?;
    Ok(FileShareSettingsView::from(saved))
}
```

- [ ] **Step 5: Wire the new module into the app**

```rust
mod fileshare;

tauri::generate_handler![
    fileshare::file_share_load_settings,
    fileshare::file_share_save_settings,
    fileshare::file_share_start_saved,
    fileshare::file_share_start,
    fileshare::file_share_stop,
    fileshare::file_share_get_status
]
```

- [ ] **Step 6: Run tests to verify persistence and defaults pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::persist -- --nocapture`  
Expected: PASS, including default config, round-trip persistence, and password hashing tests

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/fileshare src-tauri/src/main.rs src-tauri/Cargo.toml
git commit -m "feat(fileshare): add v2 persisted config model"
```

### Task 2: Add Auth, Session TTL, IP Filters, And Permission Enforcement

**Files:**
- Create: `src-tauri/src/fileshare/auth.rs`
- Modify: `src-tauri/src/fileshare/model.rs`
- Modify: `src-tauri/src/fileshare/mod.rs`
- Modify: `src-tauri/src/fileshare/http.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/fileshare/auth.rs`

- [ ] **Step 1: Write the failing auth tests**

```rust
#[test]
fn whitelist_mode_rejects_ip_outside_rules() {
    let rules = parse_ip_rules(&["192.168.0.0/24".into()]).expect("rules should parse");
    let allowed = is_ip_allowed(IpFilterMode::Whitelist, &rules, "10.0.0.5".parse().unwrap());
    assert!(!allowed);
}

#[test]
fn session_expires_after_ttl() {
    let mut store = SessionStore::default();
    let token = store.create("guest".into(), Duration::from_secs(1), "192.168.0.8".into());
    std::thread::sleep(Duration::from_millis(1200));
    assert!(store.validate(&token, "192.168.0.8").is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::auth -- --nocapture`  
Expected: FAIL because IP rule parsing and session store are not implemented yet

- [ ] **Step 3: Add dependencies for cookie and IP rule support**

```toml
axum-extra = { version = "0.9", features = ["cookie"] }
ipnet = "2"
```

- [ ] **Step 4: Implement auth/session primitives and permission checks**

```rust
pub struct SessionRecord {
    pub account_id: String,
    pub expires_at: Instant,
    pub client_ip: String,
}

pub fn require_permission(
    principal: &ResolvedPrincipal,
    permission: FileSharePermission,
) -> Result<(), StatusCode> {
    if principal.permissions.allows(permission) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
```

- [ ] **Step 5: Add login/logout/session endpoints and middleware hookup**

```rust
.route("/api/session", get(handler_session))
.route("/api/auth/login", post(handler_login))
.route("/api/auth/logout", post(handler_logout))
```

- [ ] **Step 6: Re-run auth tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::auth -- --nocapture`  
Expected: PASS for whitelist, blacklist, session expiry, permission gating, and guest-account cases

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/fileshare src-tauri/Cargo.toml
git commit -m "feat(fileshare): add account auth and session ttl"
```

### Task 3: Implement Safe File Management APIs

**Files:**
- Create: `src-tauri/src/fileshare/ops.rs`
- Create: `src-tauri/src/fileshare/search.rs`
- Create: `src-tauri/src/fileshare/http.rs`
- Modify: `src-tauri/src/fileshare/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/fileshare/ops.rs`
- Test: `src-tauri/src/fileshare/search.rs`

- [ ] **Step 1: Write the failing operation tests**

```rust
#[test]
fn rename_cannot_cross_share_root_boundary() {
    let roots = test_roots();
    let error = rename_entry(&roots, "root-a", "sub/a.txt", "root-b", "b.txt").unwrap_err();
    assert!(error.contains("same shared root"));
}

#[test]
fn current_page_search_filters_only_current_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("readme.txt"), b"ok").unwrap();
    std::fs::write(dir.path().join("nested").join("readme.txt"), b"ok").unwrap();

    let results = search_current_directory(dir.path(), "readme").unwrap();
    assert_eq!(results.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops fileshare::search -- --nocapture`  
Expected: FAIL because rename, delete, upload, search, and preview helpers do not exist yet

- [ ] **Step 3: Add the file operation surface**

```rust
pub fn create_directory(root: &ResolvedRoot, parent: &str, name: &str) -> Result<(), String>;
pub fn create_text_file(root: &ResolvedRoot, parent: &str, name: &str, content: &str) -> Result<(), String>;
pub fn rename_entry(root: &ResolvedRoot, from: &str, to_name: &str) -> Result<(), String>;
pub fn delete_entry(root: &ResolvedRoot, path: &str, mode: DeleteMode) -> Result<(), String>;
pub fn stream_preview(root: &ResolvedRoot, path: &str) -> Result<FilePreview, String>;
```

- [ ] **Step 4: Add dependencies for delete-to-recycle-bin**

```toml
trash = "5"
```

- [ ] **Step 5: Add Axum routes for list/search/upload/create/rename/delete/preview**

```rust
.route("/api/roots", get(handler_roots))
.route("/api/list", get(handler_list))
.route("/api/search", get(handler_search))
.route("/api/upload/files", post(handler_upload_files))
.route("/api/upload/directory", post(handler_upload_directory))
.route("/api/entries/directory", post(handler_create_directory))
.route("/api/entries/text", post(handler_create_text))
.route("/api/entries/rename", patch(handler_rename))
.route("/api/entries", delete(handler_delete))
.route("/api/preview", get(handler_preview))
.route("/download/file/*path", get(handler_file))
.route("/download/zip/*path", get(handler_zip))
```

- [ ] **Step 6: Re-run backend operation tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops fileshare::search -- --nocapture`  
Expected: PASS for safe-path checks, search scoping, delete mode, rename restrictions, and ZIP limits

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/fileshare src-tauri/Cargo.toml
git commit -m "feat(fileshare): add managed file operation api"
```

### Task 4: Add The Web File Manager Build And Asset Serving

**Files:**
- Create: `vite.file-share-web.config.ts`
- Create: `src/share-web/main.ts`
- Create: `src/share-web/App.vue`
- Create: `src/share-web/api.ts`
- Create: `src/share-web/types.ts`
- Create: `src/share-web/components/ToolbarActions.vue`
- Create: `src/share-web/components/EntryTable.vue`
- Create: `src/share-web/components/LoginDialog.vue`
- Create: `src-tauri/src/fileshare/web_assets.rs`
- Modify: `package.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/fileshare/http.rs`

- [ ] **Step 1: Write the failing asset-serving smoke check**

```rust
#[tokio::test]
async fn serves_embedded_file_share_index() {
    let app = file_share_web_router();
    let response = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run the smoke check to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml serves_embedded_file_share_index -- --nocapture`  
Expected: FAIL because embedded web assets and router fallback are not implemented yet

- [ ] **Step 3: Add a separate Vite build for the web manager**

```ts
// vite.file-share-web.config.ts
export default defineConfig({
  build: {
    outDir: 'dist/file-share-web',
    emptyOutDir: true,
  },
});
```

```json
{
  "scripts": {
    "build:file-share-web": "vite build --config vite.file-share-web.config.ts"
  }
}
```

- [ ] **Step 4: Add Rust embedded asset support**

```rust
#[derive(rust_embed::RustEmbed)]
#[folder = "../dist/file-share-web"]
struct FileShareWebAssets;

pub fn serve_web_asset(path: &str) -> Option<Response> {
    let asset = FileShareWebAssets::get(path)?;
    Some(
        Response::builder()
            .header("Content-Type", mime_guess::from_path(path).first_or_octet_stream().as_ref())
            .body(Body::from(asset.data))
            .unwrap(),
    )
}
```

- [ ] **Step 5: Add the minimal Vue web shell**

```ts
createApp(App).mount('#app');
```

```vue
<template>
  <FileManagerShell />
</template>
```

- [ ] **Step 6: Build the web assets and rerun the smoke check**

Run: `pnpm build:file-share-web`  
Expected: `dist/file-share-web` generated successfully

Run: `cargo test --manifest-path src-tauri/Cargo.toml serves_embedded_file_share_index -- --nocapture`  
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add package.json vite.file-share-web.config.ts src/share-web src-tauri/src/fileshare src-tauri/Cargo.toml dist/file-share-web
git commit -m "feat(fileshare): scaffold web file manager assets"
```

### Task 5: Build The CHFS-Style Web Manager UX

**Files:**
- Create: `src/share-web/components/SearchBar.vue`
- Create: `src/share-web/components/UploadDialog.vue`
- Create: `src/share-web/components/NewTextDialog.vue`
- Create: `src/share-web/components/RenameDialog.vue`
- Create: `src/share-web/components/DeleteConfirmDialog.vue`
- Create: `src/share-web/components/ImagePreviewDialog.vue`
- Modify: `src/share-web/App.vue`
- Modify: `src/share-web/api.ts`
- Modify: `src/share-web/types.ts`

- [ ] **Step 1: Write the failing UI-state testable utility**

```ts
export function canRenderAction(
  permissions: FileSharePermissionSet,
  action: 'upload' | 'rename' | 'delete' | 'preview' | 'searchGlobal',
): boolean {
  return false;
}
```

```ts
if (!canRenderAction(perms, 'delete')) {
  throw new Error('expected delete to be gated by permission');
}
```

- [ ] **Step 2: Run typecheck to verify the shell lacks the required UI contract**

Run: `pnpm check`  
Expected: FAIL or remain incomplete until action contracts and component props are wired

- [ ] **Step 3: Implement toolbar, search mode switch, and permission-aware entry table**

```vue
<ToolbarActions
  :permissions="session.permissions"
  :search-scope="searchScope"
  @upload-files="openUpload('files')"
  @upload-directory="openUpload('directory')"
  @create-directory="openCreateDirectory"
  @create-text="openCreateText"
/>
```

- [ ] **Step 4: Implement upload/new/rename/delete/preview dialog flows**

```ts
await api.uploadFiles(currentRootId, currentPath, files);
await api.createTextFile(currentRootId, currentPath, filename, content);
await api.renameEntry(currentRootId, currentPath, nextName);
await api.deleteEntry(currentRootId, currentPath);
```

- [ ] **Step 5: Implement current-page and server-wide search UX**

```ts
const filteredEntries = computed(() =>
  searchScope.value === 'current'
    ? entries.value.filter((entry) => entry.name.toLowerCase().includes(keyword.value.toLowerCase()))
    : globalSearchResults.value
);
```

- [ ] **Step 6: Rebuild and validate the web app**

Run: `pnpm build:file-share-web`  
Expected: PASS

Run: `pnpm check`  
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/share-web dist/file-share-web
git commit -m "feat(fileshare): add chfs-style web manager actions"
```

### Task 6: Upgrade The Desktop File Share Page And Startup Integration

**Files:**
- Modify: `src/pages/FileSharePage.vue`
- Modify: `src/pages/SettingsPage.vue`
- Modify: `src/App.vue`
- Modify: `src/lib/tauri.ts`
- Modify: `src/locales/messages.ts`
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Write the failing desktop data contract**

```ts
export interface FileShareSettingsView {
  port: number;
  roots: FileShareRoot[];
  guest_access_enabled: boolean;
  accounts: FileShareAccountView[];
  session_ttl_minutes: number;
  ip_filter_mode: 'off' | 'whitelist' | 'blacklist';
  ip_rules: string[];
  image_preview_enabled: boolean;
  thumbnail_enabled: boolean;
  delete_mode: 'recycle_bin' | 'permanent';
  remember_settings: boolean;
  auto_start_on_page_open: boolean;
  auto_start_with_windows: boolean;
}
```

- [ ] **Step 2: Run typecheck to verify the current desktop page does not satisfy the contract**

Run: `pnpm check`  
Expected: FAIL because `FileSharePage.vue`, `SettingsPage.vue`, and `App.vue` do not know the new config shape yet

- [ ] **Step 3: Expand the Tauri bridge and desktop page**

```ts
export async function fileShareLoadSettings(): Promise<FileShareSettingsView> {
  return await invoke<FileShareSettingsView>('file_share_load_settings');
}

export async function fileShareStartSaved(): Promise<string> {
  return await invoke<string>('file_share_start_saved');
}
```

```vue
<section class="fs-card">
  <h3 class="fs-section-label">权限与账号</h3>
  <AccountPermissionEditor v-model:guest="settings.guest" v-model:accounts="settings.accounts" />
</section>
```

- [ ] **Step 4: Add the Settings-page startup option**

```ts
interface AppConfig {
  launch_and_auto_scan: boolean;
  launch_and_auto_start_file_share: boolean;
}
```

```vue
<input type="checkbox" v-model="config.launch_and_auto_start_file_share" @change="save" class="sr-only peer">
```

- [ ] **Step 5: Hook app startup recovery in `App.vue`**

```ts
if (cfg?.launch_and_auto_start_file_share) {
  try {
    await fileShareStartSaved();
  } catch (e) {
    addLog(`Auto file share start failed: ${e}`, 'error');
  }
}
```

- [ ] **Step 6: Re-run frontend verification**

Run: `pnpm check`  
Expected: PASS

Run: `pnpm lint`  
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/pages/FileSharePage.vue src/pages/SettingsPage.vue src/App.vue src/lib/tauri.ts src/locales/messages.ts src-tauri/src/config.rs src-tauri/src/main.rs
git commit -m "feat(fileshare): add desktop v2 controls and startup recovery"
```

### Task 7: Full Verification, Manual Scenarios, And Final Hardening

**Files:**
- Modify: `docs/superpowers/specs/2026-04-04-file-share-chfs-style-v2-design.md`
- Modify: `docs/superpowers/plans/2026-04-04-file-share-chfs-style-v2-plan.md`
- Test: `src-tauri/src/fileshare/*`

- [ ] **Step 1: Run backend tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare -- --nocapture`  
Expected: PASS

- [ ] **Step 2: Run Rust linting**

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`  
Expected: PASS

- [ ] **Step 3: Run frontend static verification**

Run: `pnpm check`  
Expected: PASS

Run: `pnpm lint`  
Expected: PASS

- [ ] **Step 4: Run production builds**

Run: `pnpm build:file-share-web`  
Expected: PASS

Run: `pnpm build`  
Expected: PASS

- [ ] **Step 5: Execute manual acceptance checks**

```text
- 共享目录、账户、权限、IP 规则、删除模式、会话时长保存后，重启应用仍能恢复
- 打开文件共享页时，auto_start_on_page_open 生效
- 系统启动后，launch_and_auto_start_file_share 生效
- 访客只读账户看不到上传/删除/重命名入口
- 读写账户可以上传文件、上传目录、新建目录、新建文本、重命名、删除
- 黑名单 IP 被拒绝；白名单外 IP 被拒绝
- 当前页面搜索只过滤当前目录；整个服务器搜索可跨共享根返回结果
- 图片文件可预览；无预览权限时只能下载
- 删除模式为回收站时文件进入回收站，为真实删除时直接消失
- 单文件下载和目录 ZIP 下载仍然可用
```

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-04-04-file-share-chfs-style-v2-design.md docs/superpowers/plans/2026-04-04-file-share-chfs-style-v2-plan.md
git commit -m "docs(fileshare): finalize v2 implementation plan"
```

---

## Self-Review

### Spec coverage

本计划覆盖了 spec 中的全部主要需求：

1. 持久化与恢复：Task 1、Task 6、Task 7
2. 权限、账户、会话、黑白名单：Task 2、Task 6
3. 上传、新建、重命名、删除、搜索、预览：Task 3、Task 5
4. 新 Web 界面：Task 4、Task 5
5. 页面级/系统级自动启动：Task 6、Task 7

### Placeholder scan

本计划避免了未落实的空白标记；每个任务都给出了明确文件、命令和接口骨架。

### Type consistency

计划中的关键命名已统一使用：

1. `FileShareSettingsView`
2. `file_share_load_settings`
3. `file_share_save_settings`
4. `file_share_start_saved`
5. `launch_and_auto_start_file_share`

---

Plan complete and saved to `docs/superpowers/plans/2026-04-04-file-share-chfs-style-v2-plan.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
