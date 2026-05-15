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
