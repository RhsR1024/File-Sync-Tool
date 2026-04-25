# Error Code Lookup — Design Spec

- **Date**: 2026-04-25
- **Status**: Approved (brainstorming complete)
- **Owner**: codex-agent
- **Scope**: New tool page for querying internal GitLab error-code dictionary

---

## 1. Background & Goal

The team maintains an internal GitLab repository
`http://igcode.uniview.com/RD-UNIVIEW/public/pubResList/errorcode.git` that stores
error-code dictionaries as CSV files (`10w.csv`, `20w.csv`, …). Each row has the
shape:

```
错误码,界面中文展示,界面英语展示,常见错误码解决方案,所属模块,备注说明
0,执行成功,Success.,,,
1,执行失败,Error.,,,
110,相机编码不存在,camera code not exist,,,
```

Users today have to clone the repo manually and grep CSVs to look up codes.
This feature embeds a lookup tool inside the Tauri app:

- Pull the latest CSVs from GitLab without requiring `git` on the host.
- Cache them on disk; load them into an in-memory index.
- Provide a search-first UI with three modes: single code, code range, keyword.
- Provide a manual sync button that re-pulls the archive on demand.

Non-goals:

- No automatic sync (no TTL, no startup fetch, no cron).
- No incremental fetch — every sync fully replaces the cache.
- No editing of error codes from the UI.
- No exposing the GitLab credentials in settings (hardcoded for now).

---

## 2. User Experience

### 2.1 Entry points

| Surface | Path | Icon | Notes |
|---|---|---|---|
| Tools Hub card | `/tools/error-code-lookup` | `FileSearch` (lucide) | Gradient `from-indigo-500 to-blue-600` |
| Sidebar (under `tools` group) | same | `FileSearch` | Appended after `clipboard-manager` |

i18n keys:

- `sidebar.errorCodeLookup` → `错误码查询` / `Error Code Lookup`
- `toolsHub.cards.errorCodeLookup.chip` → `开发` / `DEV`
- `toolsHub.cards.errorCodeLookup.description` → `查询内部 GitLab 错误码字典，支持单码、范围、关键字搜索`

### 2.2 Page layout

```
┌─────────────────────────────────────────────────────────────┐
│  错误码查询                                                  │
│  Description (i18n: errorCodeLookup.description)             │
│                                                              │
│  上次同步：2026-04-25 10:30  (tooltip: 共 N 文件、X 条)      │
│                                              [ 同步按钮 ]   │
│                                                              │
│  ◉ 错误码   ○ 范围   ○ 关键字                                │
│  ┌─────────────────────────────────────┐  ┌──────────┐      │
│  │ placeholder 随模式而变               │  │  查询    │      │
│  └─────────────────────────────────────┘  └──────────┘      │
│                                                              │
│  ┌────┬──────────┬──────────┬──────┬──────────┬────────┐    │
│  │ 码 │ 中文     │ 英文     │ 模块 │ 解决方案 │ 备注   │    │
│  ├────┼──────────┼──────────┼──────┼──────────┼────────┤    │
│  │... │ ...      │ ...      │ ...  │ ...      │ ...    │    │
│  └────┴──────────┴──────────┴──────┴──────────┴────────┘    │
│                                                              │
│        ◀  3 / 42  ▶     跳转到：[__]                         │
└─────────────────────────────────────────────────────────────┘
```

### 2.3 Search modes

| Mode | Placeholder | Validation | Backend behavior |
|---|---|---|---|
| `single` (default) | `输入错误码（如 300005）` | 必须是纯十进制非负整数 | 在 `BTreeMap.get(code)` 找；返回 0..n 条结果（同一码可能存在多个 CSV） |
| `range` | `输入范围（如 300000-301000）` | `START-END`，`END ≥ START`，`END - START ≤ 1000` | `BTreeMap.range(START..=END)`，按码升序，分页 50 条/页 |
| `keyword` | `输入中英文关键字` | 长度 1-50，去前后空格 | 全表线性扫描；不区分大小写匹配 `message_cn` / `message_en` / `solution` |

Mode-switch UI: a horizontal radio group above the search input. Switching mode
clears the input and any current results.

### 2.4 States

| State | Trigger | UI |
|---|---|---|
| Never synced | `meta.has_cache === false` | Centered empty state: "尚未同步" + 主按钮 "立即同步"。Search box and pagination hidden. |
| Has cache, no input | Cached data exists, no query yet | Show first page of all entries (sorted by code asc) as a "preview". Top-bar shows last sync time. |
| Search active | After clicking 查询 | Replace table content with results. If empty: friendly "未找到 …" line. |
| Sync in progress | While `error_code_sync` runs | Sync button → spinner & disabled. Search box stays usable against old cache. |
| Sync failed | Network / 401 / HTTP error / unzip error | Toast error (see §6). Old cache stays intact. |

### 2.5 Pagination

- Fixed page size: **50** (no UI to change).
- Page jumper input only accepts integers in `[1, total_pages]`.
- Pagination only visible when `total > 50`.

### 2.6 Column display strategy

| Column | Width | Truncation | Hover behavior |
|---|---|---|---|
| `code` | narrow, `font-mono`, chip-style background | none | — |
| `messageCn` | medium, flex-grow | single-line ellipsis | tooltip with full text |
| `messageEn` | medium, flex-grow | single-line ellipsis | tooltip with full text |
| `module` | narrow, gray chip | none (short by nature) | — |
| `solution` | wide, flex-grow | single-line ellipsis | tooltip with full text |
| `remark` | narrow | single-line ellipsis | tooltip with full text |

Empty cells render as a muted dash (`—`).

### 2.7 Row interaction

- Click a row → expand an inline detail panel below it showing the full text of
  every column (no truncation, with proper wrapping). Click again to collapse.
- Tooltips on truncated cells (per §2.6) provide quick peek without expansion.
- No copy button (deferred per scope decision).

---

## 3. Backend Design

### 3.1 Module layout

```
src-tauri/src/error_code/
├── mod.rs        # Re-exports + ErrorCodeState (held inside AppState)
├── gitlab.rs     # Archive API client (Basic Auth + zip download)
├── parser.rs     # CSV parsing + encoding detection
├── store.rs      # In-memory store + lazy disk loader + queries
└── commands.rs   # Tauri command handlers
```

The `ErrorCodeState` is wrapped in `Arc<Mutex<…>>` and added to `AppState` in
`main.rs`. Lazy load happens on the first call to any of the three commands.

### 3.2 Constants (hardcoded in `gitlab.rs`)

```rust
pub const GITLAB_BASE_URL: &str = "http://igcode.uniview.com";
pub const GITLAB_PROJECT_PATH: &str = "RD-UNIVIEW/public/pubResList/errorcode";
pub const GITLAB_BRANCH: &str = "main";
pub const GITLAB_USERNAME: &str = "cmo_ipc";
pub const GITLAB_PASSWORD: &str = "*Ab64799254";
```

The project path is URL-encoded when building the archive URL:

```
{BASE}/api/v4/projects/{percent-encoded-path}/repository/archive.zip?sha={BRANCH}
```

### 3.3 Tauri commands

```rust
#[tauri::command]
async fn error_code_sync(state: State<'_, AppState>) -> Result<SyncReport, String>;

#[tauri::command]
async fn error_code_query(
    state: State<'_, AppState>,
    request: QueryRequest,
) -> Result<QueryResult, String>;

#[tauri::command]
async fn error_code_get_meta(state: State<'_, AppState>) -> Result<MetaInfo, String>;
```

### 3.4 Type contracts

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct ErrorCodeEntry {
    pub code: u32,
    pub message_cn: String,
    pub message_en: String,
    pub solution: String,
    pub module: String,
    pub remark: String,
    pub source_file: String, // e.g. "10w.csv" — for diagnostics only
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum QueryRequest {
    Single { value: String, page: u32 },
    Range  { value: String, page: u32 },  // value: "START-END"
    Keyword{ value: String, page: u32 },
}

#[derive(Serialize)]
pub struct QueryResult {
    pub entries: Vec<ErrorCodeEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32, // always 50
}

#[derive(Serialize)]
pub struct SyncReport {
    pub file_count: usize,
    pub row_count: usize,
    pub last_synced_at: String, // RFC3339
}

#[derive(Serialize)]
pub struct MetaInfo {
    pub has_cache: bool,
    pub last_synced_at: Option<String>,
    pub file_count: usize,
    pub row_count: usize,
}
```

### 3.5 Sync flow (`gitlab::fetch_archive` + `parser::parse_dir`)

```
1. Build URL: GET {BASE}/api/v4/projects/{enc(PATH)}/repository/archive.zip?sha=main
   Header: Authorization: Basic base64(USER:PASSWORD)
   Timeout: 30s
2. On HTTP error → return Err(SyncError::Http(status, body)).
3. Read body bytes (expected a few MB).
4. Open zip via `zip` crate.
5. For each entry whose name ends with ".csv" (case-insensitive):
     a. Read raw bytes.
     b. chardetng::EncodingDetector to guess encoding (fallback UTF-8).
     c. Decode to String.
     d. csv::Reader::from_reader to parse rows.
     e. Map each row to ErrorCodeEntry. Skip rows whose first cell is not a
        valid u32. Log skipped lines via `log::warn!` (visible in app.log).
6. Atomically replace cache directory:
     - Write each .csv to `<app_data>/errorcode_cache/<basename>.csv` (flat).
     - Sweep: remove `*.csv` entries inside `errorcode_cache/` whose basename is
       no longer present in the archive. Non-CSV files are never touched.
     - Write `meta.json` with the new fields.
7. Replace in-memory store under the mutex.
8. Return SyncReport.
```

### 3.6 Disk layout

```
%APPDATA%/<app>/app_data/errorcode_cache/
├── 10w.csv
├── 20w.csv
├── …
└── meta.json   # { last_synced_at, file_count, row_count }
```

### 3.7 Lazy load on first query

```
if !store.loaded:
    if errorcode_cache/ exists and contains *.csv:
        parse all CSVs into store
        store.loaded = true
    else:
        store.loaded = true  # mark loaded but empty
```

This means the first `error_code_query` after app launch has higher latency
(~50-200ms for parsing), all subsequent queries are O(log n) for `single` /
`range` and O(n) for `keyword` — acceptable for ~10k rows.

### 3.8 Query implementation

```rust
fn query_single(store: &Store, code: u32, page: u32) -> QueryResult { … }
fn query_range(store: &Store, start: u32, end: u32, page: u32) -> Result<QueryResult, …> {
    if end < start || end - start > 1000 {
        return Err("range_too_large");
    }
    let all: Vec<&ErrorCodeEntry> = store.entries.range(start..=end)
        .flat_map(|(_, v)| v).collect();
    paginate(all, page, 50)
}
fn query_keyword(store: &Store, kw: &str, page: u32) -> QueryResult {
    let needle = kw.to_lowercase();
    let hits: Vec<&ErrorCodeEntry> = store.entries.values().flatten()
        .filter(|e|
            e.message_cn.to_lowercase().contains(&needle) ||
            e.message_en.to_lowercase().contains(&needle) ||
            e.solution.to_lowercase().contains(&needle))
        .collect();
    paginate(hits, page, 50)
}
```

### 3.9 New dependency

Add to `src-tauri/Cargo.toml`:

```toml
csv = "1.3"
```

`reqwest`, `zip`, `chardetng`, `encoding_rs`, `base64`, `chrono` are already
present.

---

## 4. Frontend Design

### 4.1 Files

| File | Purpose |
|---|---|
| `src/pages/ErrorCodeLookupPage.vue` | Main page component |
| `src/pages/ErrorCodeLookupPage.test.mjs` | Vitest unit tests for page logic |
| `src/lib/tauri.ts` | Add typed wrappers for the 3 commands + types |
| `src/lib/sidebarNavigation.ts` | Add `errorCodeLookup` icon key + nav item |
| `src/components/Sidebar.vue` | Map new icon key to `FileSearch` |
| `src/pages/ToolsHubPage.vue` | Add new card entry |
| `src/router/index.ts` | Register `/tools/error-code-lookup` route |
| `src/locales/messages.ts` | Add `errorCodeLookup.*` + `sidebar.errorCodeLookup` + tools-hub strings (zh + en) |

### 4.2 Type wrappers in `tauri.ts`

```ts
export type ErrorCodeMode = 'single' | 'range' | 'keyword';

export interface ErrorCodeEntry {
  code: number;
  message_cn: string;
  message_en: string;
  solution: string;
  module: string;
  remark: string;
  source_file: string;
}

export interface ErrorCodeQueryRequest {
  mode: ErrorCodeMode;
  value: string;
  page: number;
}

export interface ErrorCodeQueryResult {
  entries: ErrorCodeEntry[];
  total: number;
  page: number;
  page_size: number;
}

export interface ErrorCodeSyncReport {
  file_count: number;
  row_count: number;
  last_synced_at: string;
}

export interface ErrorCodeMetaInfo {
  has_cache: boolean;
  last_synced_at: string | null;
  file_count: number;
  row_count: number;
}

export const errorCodeApi = {
  sync:    () => invoke<ErrorCodeSyncReport>('error_code_sync'),
  query:   (req: ErrorCodeQueryRequest) =>
             invoke<ErrorCodeQueryResult>('error_code_query', { request: req }),
  getMeta: () => invoke<ErrorCodeMetaInfo>('error_code_get_meta'),
};
```

### 4.3 Page state machine

```
on mount:
  meta = errorCodeApi.getMeta()
  if meta.has_cache:
    set lastSyncedAt
    runDefaultPreview()  # mode='range', value='0-1000', page=1 — see note
  else:
    show empty state

runDefaultPreview():
  Actually for "show first page by default" we use a special invocation:
  errorCodeApi.query({ mode: 'keyword', value: '', page: 1 })
  Backend treats empty keyword as "match all" and returns first page sorted asc.

on click 同步:
  errorCodeApi.sync()
  on success: update meta + re-run default preview
  on failure: toast (see §6)

on submit search:
  validate input per mode
  errorCodeApi.query({ mode, value, page: 1 })
  render results, reset pagination

on click pagination:
  errorCodeApi.query({ mode, value, page: newPage })
```

Note: backend `keyword` mode with empty `value` is special-cased to mean
"return everything, sorted by code asc, paginated".

### 4.4 i18n

All strings under `errorCodeLookup.*` namespace. Indicative keys (final list
to be filled by implementation):

```
errorCodeLookup.title
errorCodeLookup.description
errorCodeLookup.lastSyncedAt
errorCodeLookup.neverSynced
errorCodeLookup.syncButton
errorCodeLookup.syncing
errorCodeLookup.modeLabel
errorCodeLookup.modes.single
errorCodeLookup.modes.range
errorCodeLookup.modes.keyword
errorCodeLookup.placeholder.single
errorCodeLookup.placeholder.range
errorCodeLookup.placeholder.keyword
errorCodeLookup.searchButton
errorCodeLookup.columns.code
errorCodeLookup.columns.messageCn
errorCodeLookup.columns.messageEn
errorCodeLookup.columns.module
errorCodeLookup.columns.solution
errorCodeLookup.columns.remark
errorCodeLookup.empty.unsynced
errorCodeLookup.empty.singleNotFound      # 未找到错误码 {code}
errorCodeLookup.empty.rangeNoResult       # 该范围内没有错误码
errorCodeLookup.empty.keywordNoResult     # 未找到包含 "{kw}" 的错误码
errorCodeLookup.error.invalidSingle       # 请输入纯数字错误码
errorCodeLookup.error.invalidRangeFormat  # 请输入正确范围（如 300000-301000）
errorCodeLookup.error.rangeTooLarge       # 范围跨度不能超过 1000
errorCodeLookup.error.rangeReversed       # 结束值必须大于等于开始值
errorCodeLookup.error.invalidKeyword      # 关键字长度需 1-50
errorCodeLookup.toast.networkFail         # 无法连接到错误码服务器，请检查内网连接
errorCodeLookup.toast.authFail            # 错误码服务认证失败，请联系开发者
errorCodeLookup.toast.httpError           # 同步失败：HTTP {status}
errorCodeLookup.toast.archiveError        # 服务器返回数据异常，请稍后重试
errorCodeLookup.toast.syncSuccess         # 同步成功：{rows} 条错误码
errorCodeLookup.detail.expandHint         # 点击展开详情
errorCodeLookup.detail.collapseHint       # 点击收起
errorCodeLookup.pagination.jumpTo
errorCodeLookup.pagination.pageOf         # {page} / {total}
```

Both `zh` and `en` translations must be added.

---

## 5. Data Flow Summary

```
┌─────────────┐  invoke             ┌──────────────────┐
│  Vue page   │ ─────────────────▶  │  Tauri commands  │
└─────────────┘                     └────────┬─────────┘
       ▲                                     │
       │  JSON                               ▼
       │                            ┌──────────────────┐
       │                            │ error_code::store│ ◀─── lazy load on first call
       │                            └────────┬─────────┘
       │                                     │
       │                                     ▼
       │                            ┌──────────────────┐
       │                            │  errorcode_cache/│
       │                            │   *.csv + meta   │
       │                            └────────┬─────────┘
       │                                     ▲
       │                                     │ replace on sync
       │                            ┌──────────────────┐
       │                            │ error_code::sync │
       │                            └────────┬─────────┘
       │                                     │
       │                                     ▼
       │                            ┌──────────────────┐
       │                            │  GitLab archive  │
       │                            │  (HTTP + zip)    │
       │                            └──────────────────┘
       └────────────────────── toast on error / progress
```

---

## 6. Error Handling

### Backend → frontend error mapping

| Rust error | Frontend toast key | Note |
|---|---|---|
| `reqwest::Error` (connect / dns / timeout) | `errorCodeLookup.toast.networkFail` | Old cache untouched. |
| HTTP 401 / 403 | `errorCodeLookup.toast.authFail` | Old cache untouched. |
| Other HTTP non-2xx | `errorCodeLookup.toast.httpError` (with `{status}`) | Old cache untouched. |
| `zip::ZipError` / no CSV in archive | `errorCodeLookup.toast.archiveError` | Old cache untouched. |
| Single-CSV parse error | (no toast) — log to `app.log` only | Other CSVs still loaded. |
| `range_too_large` / validation in query | Inline error under input | No request actually sent in normal flow because frontend pre-validates; backend still defends. |

### Defensive parsing

- Skip rows where the first cell is not a u32. Log `warn!`.
- Trim whitespace from all fields.
- If a row has fewer than 6 columns, fill missing ones with `""`. Log `debug!`.
- If a row has more than 6 columns, the extra columns are concatenated into
  the last field with `,`. (Standard `csv` crate behavior already handles
  quoting, so this case mostly handles malformed manual edits.)

---

## 7. Logging

All sync events emit through the existing `log-message` channel so they appear
in `MainConsole`:

- `info`: `开始同步错误码字典 …`
- `info`: `已下载错误码归档（X bytes）`
- `info`: `已解析 N 个 CSV，共 M 条错误码`
- `warn`: `跳过无效行：<source_file>:<line>: <preview>`
- `error`: `同步失败：<reason>`

Failures additionally write to `app.log` via the existing `tauri-plugin-log`.

---

## 8. Testing

### Backend (`cargo test`)

| Module | Tests |
|---|---|
| `gitlab` | URL builder produces correctly URL-encoded path; Basic Auth header is well-formed (encode/decode round-trip). Network call covered by integration test gated behind an env flag (skipped by default). |
| `parser` | UTF-8 happy path; GBK happy path (provided fixture); rows with quoted fields containing commas; rows with empty trailing fields; rows with non-numeric first cell are skipped; rows with extra columns merged into last field. |
| `store` | `query_single` returns 0/1/N entries; `query_range` enforces `END-START ≤ 1000`; `query_range` enforces `END ≥ START`; `query_keyword` matches across cn/en/solution case-insensitively; `query_keyword` with empty needle returns full set; pagination boundaries (page 0 → treated as 1; page > total → empty). |

Test fixtures live under `src-tauri/tests/fixtures/error_code/`:
- `sample_utf8.csv`
- `sample_gbk.csv`
- `sample_quoted.csv`
- `sample_malformed.csv`

### Frontend (`pnpm test`)

`ErrorCodeLookupPage.test.mjs` covers:
- Renders empty state when `getMeta` returns `has_cache: false`.
- Renders preview when `has_cache: true`.
- Mode switch resets input and results.
- Single-mode validation (non-numeric → red border, no invoke).
- Range-mode validation (bad format / reversed / too large).
- Keyword-mode validation (empty / >50 chars).
- Pagination triggers new `query` invocation with correct `page`.
- Sync button shows loading state and re-fetches preview on success.
- Toast appears for each backend error variant.

### Manual QA checklist

- [ ] Click sidebar "错误码查询" — page opens, shows empty state.
- [ ] Click 立即同步 — table populates, toast shows success.
- [ ] Disconnect network, click 同步 — toast shows network failure, old data intact.
- [ ] Single mode: input `110`, click 查询 — see `相机编码不存在 / camera code not exist`.
- [ ] Range mode: input `300000-301000` — paginated list.
- [ ] Range mode: input `0-100000` — input shows "范围跨度不能超过 1000".
- [ ] Keyword mode: input `相机` — see all entries containing 相机.
- [ ] Click a row — detail panel expands.
- [ ] Switch language to English — all i18n strings render.
- [ ] Restart app — page loads instantly from cache (no fresh sync triggered).

---

## 9. Out of Scope (deferred)

- Configurable GitLab URL/credentials (currently hardcoded — re-evaluate if
  team rotates credentials or other repos need the same UI).
- Incremental fetch via `last_commit_id`.
- Offline diff between two versions of the dictionary.
- Copy-to-clipboard buttons per row.
- Default page sort options (by module, by code desc).
- Auto-sync at app startup or after N days.

---

## 10. Migration / Rollout

- No database migration; only a new directory `errorcode_cache/` under
  `%APPDATA%`. Created lazily on first sync.
- Existing users on update will see the new sidebar entry; clicking it lands
  on the empty state until they sync.
- `pnpm tauri:build:versioned-exe` must succeed before merge.

---

## 11. Dependencies on Other Tasks

None — this feature is independent of all currently-active brainstorming
tasks (LAN share cleanup, clipboard manager redesign, etc.). The "Update
checker" feature originally requested alongside this one is split into a
separate brainstorm/spec.
