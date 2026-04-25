# Error Code Lookup

> Contracts for syncing internal GitLab error-code dictionaries into local cache and querying them through Tauri commands.

## Scenario: GitLab Sync + Query Contract

### 1. Scope / Trigger

- Trigger: Code under `src-tauri/src/error_code/` or frontend wrappers that call `error_code_sync`, `error_code_query`, or `error_code_get_meta`.
- Goal: Keep the GitLab archive fetch, local cache replacement, lazy-load behavior, and query semantics stable across backend/frontend boundaries.

### 2. Signatures

- Rust modules:
  - `src-tauri/src/error_code/mod.rs`
  - `src-tauri/src/error_code/gitlab.rs`
  - `src-tauri/src/error_code/parser.rs`
  - `src-tauri/src/error_code/cache.rs`
  - `src-tauri/src/error_code/store.rs`
  - `src-tauri/src/error_code/sync.rs`
  - `src-tauri/src/error_code/commands.rs`
- Tauri commands:
  - `error_code_sync(app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<SyncReport, String>`
  - `error_code_query(app_handle: tauri::AppHandle, state: State<'_, AppState>, request: QueryRequest) -> Result<QueryResult, String>`
  - `error_code_get_meta(app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<MetaInfo, String>`
- Shared payloads:

```rust
pub struct QueryRequest {
    pub mode: String,
    pub value: String,
    pub page: u32,
}

pub struct ErrorCodeEntry {
    pub code: u32,
    pub message_cn: String,
    pub message_en: String,
    pub solution: String,
    pub module: String,
    pub remark: String,
    pub source_file: String,
}

pub struct QueryResult {
    pub entries: Vec<ErrorCodeEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
}

pub struct SyncReport {
    pub file_count: usize,
    pub row_count: usize,
    pub last_synced_at: String,
}

pub struct MetaInfo {
    pub has_cache: bool,
    pub last_synced_at: Option<String>,
    pub file_count: usize,
    pub row_count: usize,
}
```

### 3. Contracts

#### 3.1 GitLab archive fetch

- Source repo is hardcoded in `gitlab.rs`:
  - base URL: `http://igcode.uniview.com`
  - project path: `RD-UNIVIEW/public/pubResList/errorcode`
  - branch: `main`
- Archive URL must be built as:
  - `GET {BASE}/api/v4/projects/{percent-encoded-project-path}/repository/archive.zip?sha={branch}`
- Auth must use `Authorization: Basic <base64(username:password)>`.
- HTTP timeout is 30 seconds.

#### 3.2 Cache layout

- Cache root is `app_handle.path().app_data_dir()/errorcode_cache`.
- Cache contents:
  - `*.csv` flattened by basename from the downloaded zip
  - `meta.json`
- Cache replacement rules:
  - write new CSV files first
  - remove orphaned `*.csv` files not present in the new archive
  - never remove non-CSV files from the cache directory
  - write `meta.json` with `last_synced_at`, `file_count`, `row_count`

#### 3.3 Lazy load + in-memory store

- `ErrorCodeStore.entries` is a `BTreeMap<u32, Vec<ErrorCodeEntry>>`.
- The store is lazy-loaded from cache on the first `error_code_query` or `error_code_get_meta`.
- `ensure_loaded` must parse cache exactly once per process start unless a later sync replaces the store.
- `query_keyword("")` is a supported preview path and must return all cached entries sorted by code ascending.

#### 3.4 Query semantics

- `mode = "single"`
  - `value.trim()` must parse as `u32`
  - response returns all entries stored under that exact code
- `mode = "range"`
  - `value` must contain exactly one `-` separator via `split_once('-')`
  - both sides must parse as `u32`
  - `end < start` -> `range_reversed`
  - `end - start > 1000` -> `range_too_large`
  - valid range returns all entries in inclusive ascending-code order
- `mode = "keyword"`
  - matches against `message_cn`, `message_en`, and `solution`
  - matching is case-insensitive for Latin text
  - empty keyword is allowed and means "return everything"
- Pagination:
  - `PAGE_SIZE = 50`
  - `page = 0` normalizes to page `1`
  - pages beyond the available range return `entries = []` with the requested normalized page number

#### 3.5 Parser rules

- Encoding is detected with `chardetng`; decoded text strips a leading UTF-8 BOM.
- CSV parser must be flexible on row width.
- Row handling:
  - first cell must parse as `u32` or the row is skipped
  - missing trailing cells become empty strings
  - extra cells are merged into `remark` with commas
- `source_file` must be the flattened CSV basename for diagnostics.

### 4. Validation & Error Matrix

| Case | Layer | Required behavior |
| --- | --- | --- |
| GitLab connect / DNS / timeout failure | `gitlab::fetch_archive` | Return `SyncError::Network`, preserve old cache and old in-memory data |
| HTTP 401 / 403 | `gitlab::fetch_archive` | Return `SyncError::Auth` |
| Other non-2xx response | `gitlab::fetch_archive` | Return `SyncError::Http(status)` |
| Zip has no CSV files | `sync::run_sync` | Return `SyncError::Archive("no_csv_in_archive")` |
| One CSV row has invalid code cell | `parser` | Skip row, log warn, continue loading other rows |
| `mode = "single"` with non-decimal input | `commands` | Return `Err("invalid_single")` |
| `mode = "range"` with bad separator / bad integers | `commands` | Return `Err("invalid_range_format")` |
| `mode = "range"` with `end < start` | `store` | Return `Err("range_reversed")` |
| `mode = "range"` with span > 1000 | `store` | Return `Err("range_too_large")` |
| Unknown query mode | `commands` | Return `Err(format!("unknown_mode: {mode}"))` |
| Cache directory missing | `cache` / `ensure_loaded` | Treat as empty cache, not as an error |
| Cache meta missing but CSVs exist | `error_code_get_meta` | `has_cache = true`; derive `row_count` from store and leave `last_synced_at` as store/meta fallback |

### 5. Good / Base / Bad Cases

- Good: User clicks sync, GitLab archive contains `10w.csv` and `20w.csv`, parser loads both, cache writes both CSVs plus `meta.json`, and subsequent keyword search returns merged results from both files.
- Base: App restarts after a previous successful sync; first call to `error_code_get_meta` lazy-loads cached CSVs into memory and reports row count without forcing a network request.
- Bad: Frontend sends a tagged-enum-shaped payload such as `{ mode: { single: ... } }`; the backend expects a flat `{ mode, value, page }` object and the contract is broken.
- Bad: Backend rejects `keyword=""`; that would break the default preview flow used by the page after sync and on cached startup.

### 6. Tests Required

- Rust: `cargo test --manifest-path src-tauri/Cargo.toml -p app error_code`
  - assert GitLab archive URL percent-encoding and Basic Auth header construction
  - assert UTF-8 / GBK decode paths and CSV row normalization
  - assert cache round-trip, orphan CSV sweep, and missing-dir behavior
  - assert single/range/keyword query behavior and pagination boundaries
  - assert zip extraction only keeps CSV basenames
- Node: `node --test src/lib/sidebarNavigation.test.mjs src/pages/errorCodeLookup/validation.test.mjs`
  - assert nav registration
  - assert single/range/keyword input validation behavior
- Frontend type check: `pnpm check`
  - assert route, page component, i18n keys, and typed Tauri wrappers remain compatible

### 7. Wrong vs Correct

#### Wrong

```rust
#[derive(Deserialize)]
#[serde(tag = "mode", content = "value")]
pub enum QueryRequest {
    Single { value: String, page: u32 },
    Range { value: String, page: u32 },
    Keyword { value: String, page: u32 },
}
```

This does not match the live frontend contract in `src/lib/tauri.ts`, which sends a flat object.

#### Correct

```rust
pub struct QueryRequest {
    pub mode: String,
    pub value: String,
    pub page: u32,
}
```

#### Wrong

```rust
if keyword.trim().is_empty() {
    return Err("invalid_keyword");
}
```

This breaks the cached first-page preview behavior.

#### Correct

```rust
pub fn query_keyword(&self, keyword: &str, page: u32) -> QueryResult {
    let needle = keyword.trim().to_lowercase();
    // empty needle means "return all"
    ...
}
```
