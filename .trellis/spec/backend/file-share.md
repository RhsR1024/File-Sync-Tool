# File Share Backend

Executable contracts for `src-tauri/src/fileshare/` HTTP serving, uploads, and browser sessions.

---

## Scenario: Large Uploads And Restart-Safe Sessions

### 1. Scope / Trigger
- Trigger: changes to `src-tauri/src/fileshare/http.rs`, `mod.rs`, `ops.rs`, or auth/session plumbing that affect `/api/upload/*`, `/api/session`, `/api/auth/login`, or saved-config restarts.
- Goal: large browser uploads must not fail at framework defaults, and non-guest sessions must survive a file-share service rebuild when the account still exists and is enabled.

### 2. Signatures
```rust
pub(super) const UPLOAD_BODY_LIMIT_BYTES: usize;

fn build_router(state: Arc<HttpState>) -> Router;

async fn read_upload_parent_node_id(multipart: &mut Multipart) -> Result<String, String>;

async fn write_upload_files_from_multipart(
    multipart: &mut Multipart,
    root: &ops::ResolvedRoot,
    parent: &str,
    create_parents: bool,
    max_total_bytes: usize,
) -> Result<(), String>;

pub fn prepare_upload_target(
    root: &ResolvedRoot,
    parent: &str,
    relative_name: &str,
    create_parents: bool,
) -> Result<PathBuf, String>;
```

### 3. Contracts
- `build_router` must apply `DefaultBodyLimit::disable()`; file-share uploads are governed by `HttpState.upload_body_limit_bytes`, not Axum's default multipart limit.
- Multipart uploads must send `parent_node_id` before file fields. The web client uses `FormData.set("parent_node_id", parentNodeId)` before appending `file` entries.
- `/api/upload/files` allows flat file names only. `/api/upload/directory` allows nested relative paths and may create missing parent directories.
- Upload handlers must resolve `parent_node_id` and require root upload permission before writing file bytes to disk.
- Upload writes must stream chunks to disk and remove the partial target file when size or write errors abort the request.
- `FileShareHandle` owns the shared `Arc<Mutex<auth::SessionStore>>`; `file_share_start` and `file_share_start_saved` must clone it into each new `HttpState`.
- Existing session cookies are still revalidated against the current runtime config on each request. If an account is disabled, removed, blocked by IP rules, or no longer has permission, the session must not grant stale access.

### 4. Validation & Error Matrix
| Case | Response |
|------|----------|
| Multipart rejected as invalid | `400 Invalid Upload` |
| Upload exceeds `upload_body_limit_bytes` | `413 Upload Too Large` |
| Missing or empty `parent_node_id` | `400 Invalid Upload` |
| File upload contains nested path | `400 Invalid Upload` |
| Target already exists or path is invalid | `400 Invalid Upload` |
| Principal lacks upload permission | `403 Forbidden` |
| Valid upload | `201 Created` |

### 5. Good/Base/Bad Cases
- Good: a 3 MiB upload succeeds when `upload_body_limit_bytes` is above 3 MiB, proving the Axum default body limit is not blocking the route.
- Base: a request above `upload_body_limit_bytes` returns `413` and does not leave a completed target file.
- Bad: reading all uploaded files into `Vec<u8>` before resolving the parent directory can create memory pressure and delays permission/path validation.
- Bad: creating a fresh `SessionStore` for each service restart drops a valid admin browser session back to guest.

### 6. Tests Required
- `fileshare::http::tests::upload_routes_accept_payloads_above_axum_default_body_limit`
  - Asserts a multipart body above Axum's default limit returns `201`.
  - Asserts the uploaded file exists with the expected byte length.
- `fileshare::http::tests::upload_routes_reject_payloads_over_limit`
  - Asserts configured file-share upload limit still returns `413`.
- `fileshare::http::tests::account_session_survives_rebuilt_http_state`
  - Logs in as a real account, rebuilds `HttpState` with the same session store, and asserts `/api/session` remains that account rather than guest.
- `fileshare::ops::tests::file_upload_rejects_nested_relative_paths`
  - Asserts flat upload paths cannot smuggle nested directories.
- `fileshare::ops::tests::directory_upload_creates_nested_parent_directories`
  - Asserts directory upload paths create nested parent directories.

### 7. Wrong vs Correct
#### Wrong
```rust
let request = read_upload_request(multipart, limit).await?;
for file in request.files {
    ops::write_uploaded_file(&root, &parent, &file.relative_path, &file.contents, false)?;
}
```

#### Correct
```rust
let parent_node_id = read_upload_parent_node_id(&mut multipart).await?;
let (root, parent) = resolve_parent_directory_node(state, principal, &parent_node_id)?;
write_upload_files_from_multipart(&mut multipart, &root, &parent, false, limit).await?;
```

This resolves the parent and permission boundary before streaming the remaining file fields.

---

## Scenario: Multi-Selection Archive Downloads

### 1. Scope / Trigger
- Trigger: changes to `src/share-web/App.vue`, `src/share-web/api.ts`, `src/share-web/components/ToolbarActions.vue`, `src-tauri/src/fileshare/http.rs`, or ZIP streaming helpers in `ops.rs`.
- Goal: browser bulk download must produce one ZIP response for the selected nodes, not one browser download per selected file. Writable accounts must still see `Download all` when archive download is allowed.

### 2. Signatures
```ts
fileShareApi.downloadSelectionArchiveUrl(nodeIds: string[]): string
```

```rust
GET /api/download/selection-archive?node_id=<id>&node_id=<id>

pub struct ZipSelectionEntry {
    pub path: PathBuf,
    pub archive_path: String,
}

pub fn validate_zip_selection(entries: &[ZipSelectionEntry]) -> Result<ZipSourceStats, String>;
pub async fn stream_zip_selection(
    entries: Vec<ZipSelectionEntry>,
    writer: tokio::io::DuplexStream,
) -> Result<(), String>;
```

### 3. Contracts
- `ToolbarActions` shows `Download all` whenever the current view is not home, entries exist, and `permissions.download_archive` is true. Upload/create permissions must not hide it.
- Bulk selection download calls `downloadSelectionArchiveUrl(selectedNodeIds)` once and triggers a single browser download.
- Selection archive query accepts repeated `node_id` parameters. Node IDs are the existing URL-safe node identifiers produced by `encode_node_id`.
- Each selected file requires root `download_file`; each selected directory or share root requires root `download_archive`.
- The ZIP stream must include selected files at the archive root and selected directories under their directory name. The server must still apply existing ZIP file-count/depth limits before streaming.

### 4. Validation & Error Matrix
| Case | Response / UI |
|------|---------------|
| Writable directory with archive permission | `Download all` is visible with upload/create buttons |
| Bulk selected files | One `/api/download/selection-archive` request |
| Missing selection node IDs | `400 Selection Archive Requires Nodes` |
| Selected file lacks `download_file` | `403 Forbidden` |
| Selected directory lacks `download_archive` | `403 Forbidden` |
| Selection exceeds ZIP limits | `413 Payload Too Large` |
| Valid selection | `200 application/zip` |

### 5. Good/Base/Bad Cases
- Good: selecting 15 images downloads one ZIP stream, not 15 separate image responses.
- Good: a read/write account sees both write actions and `Download all`.
- Base: selecting one file still uses the selection archive route when invoked from the bulk action bar.
- Bad: looping over `selectedEntries` and calling `triggerDownload(entry)` causes browser download fan-out.
- Bad: tying `Download all` visibility to `!canUploadFiles && !canCreateText` makes writable accounts lose archive access.

### 6. Tests Required
- `src/share-web/api.test.ts`
  - Asserts repeated `node_id` query parameters are generated for one selection archive URL.
- `src/share-web/bulk-download.test.mjs`
  - Asserts `bulkDownload()` calls `downloadSelectionArchiveUrl(items.map((entry) => entry.node_id))`.
- `src/share-web/components/toolbar-actions.test.mjs`
  - Asserts writable permissions do not suppress `Download all`.
- `fileshare::http::tests::selection_archive_download_streams_multiple_files_as_one_zip`
  - Asserts the new endpoint returns `200 application/zip` and streams ZIP bytes.

### 7. Wrong vs Correct
#### Wrong
```ts
for (const entry of selectedEntries.value) {
  triggerDownload(entry);
}
```

#### Correct
```ts
const href = fileShareApi.downloadSelectionArchiveUrl(
  selectedEntries.value.map((entry) => entry.node_id),
);
triggerDownloadUrl(href, `${currentName.value || 'selected'}.zip`);
```

---

## Scenario: Settings Password Plaintext Echo

### 1. Scope / Trigger
- Trigger: changes to `src-tauri/src/fileshare/model.rs`, `persist.rs`, settings commands, or `src/pages/FileSharePage.vue` account-password editing.
- Goal: the local settings panel may show saved account passwords after restart. Security is intentionally relaxed for this LAN-only tool surface, while HTTP login must continue to validate against hashes.

### 2. Signatures
```rust
pub struct PersistedFileShareUser {
    pub password_plain: Option<String>,
    pub password_hash: Option<String>,
}

pub struct FileShareUserView {
    pub password_set: bool,
    pub password_plain: Option<String>,
}

pub struct FileShareUserSaveRequest {
    pub new_password: Option<String>,
    pub clear_password: bool,
}
```

```ts
export interface FileShareUserView {
  password_set: boolean;
  password_plain: string | null;
}
```

### 3. Contracts
- Saving a non-empty `new_password` must store both `password_hash` and `password_plain`.
- Loading settings must return `FileShareUserView.password_plain` when a saved plaintext exists so the UI can prefill the account password field after app restart.
- `clear_password = true` must remove both `password_hash` and `password_plain`.
- Existing hash-only configs cannot recover the old password. They must keep `password_hash`, return `password_plain = null`, and keep the password field blank until the user enters and saves a new password.
- Normalization must trim empty plaintext values to `None`. If a config contains only `password_plain`, normalization must derive `password_hash` so HTTP login remains hash-based.

### 4. Validation & Error Matrix
| Case | Result |
|------|--------|
| Save with non-empty `new_password` | Stores Argon2 hash and plaintext |
| Reload settings after restart | Password field receives plaintext |
| Save with blank `new_password` and existing hash/plain | Preserves existing password |
| Save with `clear_password = true` | Removes hash and plaintext |
| Legacy hash-only config | Keeps login working, returns `password_plain = null` |

### 5. Tests Required
- `fileshare::persist::tests::save_request_hashes_and_retains_plaintext_for_settings_ui`
  - Asserts saved plaintext differs from the hash and is exposed in `FileShareSettingsView`.
- `src/pages/FileSharePage.test.mjs`
  - Asserts the TS contract includes `password_plain` and edit fields initialize from it.
  - Asserts account password inputs are visible text fields rather than masked password inputs.
