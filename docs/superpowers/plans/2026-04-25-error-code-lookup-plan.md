# Error Code Lookup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an "Error Code Lookup" tool page that pulls CSV error-code dictionaries from internal GitLab without needing system Git, caches them locally, and lets users query by single code, code range (≤ 1000 span), or free-text keyword.

**Architecture:** Rust backend module `error_code/` (gitlab fetch → CSV parse → in-memory `BTreeMap` index + disk cache under `%APPDATA%`), exposed via 3 Tauri commands. Vue page `ErrorCodeLookupPage.vue` with mode-switch search, paginated results, and a manual sync button. Validation logic extracted to a pure module for `node --test` coverage.

**Tech Stack:** Rust (reqwest, zip, csv 1.3, chardetng, base64, chrono); Vue 3 `<script setup>` + Tailwind + lucide-vue-next + vue-i18n; Tauri 2.x.

**Companion design spec:** `docs/superpowers/specs/2026-04-25-error-code-lookup-design.md` — re-read it before starting.

---

## File Structure

**Backend (`src-tauri/`):**

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Add `csv = "1.3"` dep |
| `src/error_code/mod.rs` | Public types, `ErrorCodeStore`, `ErrorCodeState`, constants |
| `src/error_code/gitlab.rs` | URL building, Basic Auth, archive fetch via reqwest, `SyncError` |
| `src/error_code/parser.rs` | Encoding detection (chardetng), CSV row parsing, BOM strip |
| `src/error_code/store.rs` | Ingestion, single/range/keyword queries, pagination |
| `src/error_code/cache.rs` | Disk read/write/sweep of `errorcode_cache/*.csv` and `meta.json` |
| `src/error_code/sync.rs` | Orchestration: fetch → unzip → parse → cache → swap store |
| `src/error_code/commands.rs` | 3 Tauri commands: `error_code_sync`, `error_code_query`, `error_code_get_meta` |
| `src/main.rs` | `mod error_code;`, AppState wiring, `invoke_handler` registration |

**Frontend (`src/`):**

| Path | Responsibility |
|---|---|
| `lib/tauri.ts` | Add `ErrorCodeEntry`, `ErrorCodeQueryRequest`, `ErrorCodeQueryResult`, `ErrorCodeMetaInfo`, `ErrorCodeSyncReport`, `errorCodeApi` |
| `pages/errorCodeLookup/validation.ts` | Pure parsers `parseSingle`, `parseRange`, `parseKeyword` |
| `pages/errorCodeLookup/validation.test.mjs` | `node --test` cases for the three parsers |
| `pages/ErrorCodeLookupPage.vue` | Page shell: mode switch, search input, table, pagination, sync button |
| `lib/sidebarNavigation.ts` | Add `errorCodeLookup` icon key + nav item under `tools` group |
| `lib/sidebarNavigation.test.mjs` | Update path-list snapshot to include new path |
| `components/Sidebar.vue` | Map icon key `errorCodeLookup` → `FileSearch` |
| `pages/ToolsHubPage.vue` | Append new card entry |
| `router/index.ts` | Register `/tools/error-code-lookup` route |
| `locales/messages.ts` | Add `errorCodeLookup.*`, `sidebar.errorCodeLookup`, `toolsHub.cards.errorCodeLookup.*` (zh + en) |

---

## Task 1: Add `csv` dependency

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the dependency line**

In `src-tauri/Cargo.toml`, find the block of dependencies that includes `reqwest` and add `csv` next to it:

```toml
csv = "1.3"
```

Place it alphabetically next to existing crates (e.g., between `chrono` and `encoding_rs` is fine — order is not strict in this project).

- [ ] **Step 2: Verify cargo can resolve it**

Run from repo root:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: completes with no errors. May show warnings; ignore them.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(error-code): add csv crate for dictionary parsing"
```

---

## Task 2: Module skeleton, types, and constants

**Files:**
- Create: `src-tauri/src/error_code/mod.rs`
- Modify: `src-tauri/src/main.rs` (add `mod error_code;`)

- [ ] **Step 1: Create the module file**

Create `src-tauri/src/error_code/mod.rs` with the following content:

```rust
//! Error code lookup feature.
//!
//! See `docs/superpowers/specs/2026-04-25-error-code-lookup-design.md`.

pub mod cache;
pub mod commands;
pub mod gitlab;
pub mod parser;
pub mod store;
pub mod sync;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeEntry {
    pub code: u32,
    pub message_cn: String,
    pub message_en: String,
    pub solution: String,
    pub module: String,
    pub remark: String,
    pub source_file: String,
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub mode: String,
    pub value: String,
    pub page: u32,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub entries: Vec<ErrorCodeEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub file_count: usize,
    pub row_count: usize,
    pub last_synced_at: String,
}

#[derive(Debug, Serialize)]
pub struct MetaInfo {
    pub has_cache: bool,
    pub last_synced_at: Option<String>,
    pub file_count: usize,
    pub row_count: usize,
}

pub const PAGE_SIZE: u32 = 50;
pub const MAX_RANGE_SPAN: u32 = 1000;

#[derive(Default)]
pub struct ErrorCodeStore {
    pub entries: BTreeMap<u32, Vec<ErrorCodeEntry>>,
    pub last_synced_at: Option<String>,
    pub loaded: bool,
}

pub type ErrorCodeState = Mutex<ErrorCodeStore>;
```

- [ ] **Step 2: Create empty stub files for sub-modules**

The `mod.rs` references six sub-modules. Create each as an empty file so the build resolves. Later tasks will fill them in:

```bash
touch src-tauri/src/error_code/cache.rs
touch src-tauri/src/error_code/commands.rs
touch src-tauri/src/error_code/gitlab.rs
touch src-tauri/src/error_code/parser.rs
touch src-tauri/src/error_code/store.rs
touch src-tauri/src/error_code/sync.rs
```

- [ ] **Step 3: Add `mod error_code;` to `main.rs`**

In `src-tauri/src/main.rs`, find the alphabetical list of `mod` declarations near the top (e.g., between `disk_cleanup` and `fileshare`). Insert:

```rust
mod error_code;
```

- [ ] **Step 4: Verify build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: compiles. Warnings about unused types are expected at this stage — ignore them.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code src-tauri/src/main.rs
git commit -m "feat(error-code): module skeleton with shared types and constants"
```

---

## Task 3: GitLab URL builder + Basic Auth header (TDD)

**Files:**
- Modify: `src-tauri/src/error_code/gitlab.rs`

- [ ] **Step 1: Write failing tests**

Add to `src-tauri/src/error_code/gitlab.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn archive_url_url_encodes_project_path_and_branch() {
        let url = build_archive_url();
        assert!(url.starts_with("http://igcode.uniview.com/api/v4/projects/"));
        assert!(url.contains("RD-UNIVIEW%2Fpublic%2FpubResList%2Ferrorcode"));
        assert!(url.ends_with("/repository/archive.zip?sha=main"));
    }

    #[test]
    fn basic_auth_header_round_trips_to_credentials() {
        let header = build_basic_auth_header();
        let b64 = header
            .strip_prefix("Basic ")
            .expect("header must begin with Basic ");
        let decoded =
            base64::engine::general_purpose::STANDARD.decode(b64).expect("valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), "cmo_ipc:*Ab64799254");
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::gitlab
```

Expected: compile error — `build_archive_url` / `build_basic_auth_header` not defined.

- [ ] **Step 3: Implement minimal code**

Replace the contents of `src-tauri/src/error_code/gitlab.rs` with:

```rust
use base64::Engine;

pub const GITLAB_BASE_URL: &str = "http://igcode.uniview.com";
pub const GITLAB_PROJECT_PATH: &str = "RD-UNIVIEW/public/pubResList/errorcode";
pub const GITLAB_BRANCH: &str = "main";
pub const GITLAB_USERNAME: &str = "cmo_ipc";
pub const GITLAB_PASSWORD: &str = "*Ab64799254";

pub fn build_archive_url() -> String {
    let encoded = percent_encode(GITLAB_PROJECT_PATH);
    format!(
        "{}/api/v4/projects/{}/repository/archive.zip?sha={}",
        GITLAB_BASE_URL, encoded, GITLAB_BRANCH
    )
}

pub fn build_basic_auth_header() -> String {
    let creds = format!("{}:{}", GITLAB_USERNAME, GITLAB_PASSWORD);
    let b64 = base64::engine::general_purpose::STANDARD.encode(creds.as_bytes());
    format!("Basic {}", b64)
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn archive_url_url_encodes_project_path_and_branch() {
        let url = build_archive_url();
        assert!(url.starts_with("http://igcode.uniview.com/api/v4/projects/"));
        assert!(url.contains("RD-UNIVIEW%2Fpublic%2FpubResList%2Ferrorcode"));
        assert!(url.ends_with("/repository/archive.zip?sha=main"));
    }

    #[test]
    fn basic_auth_header_round_trips_to_credentials() {
        let header = build_basic_auth_header();
        let b64 = header
            .strip_prefix("Basic ")
            .expect("header must begin with Basic ");
        let decoded =
            base64::engine::general_purpose::STANDARD.decode(b64).expect("valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), "cmo_ipc:*Ab64799254");
    }
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::gitlab::tests
```

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/gitlab.rs
git commit -m "feat(error-code): build GitLab archive URL and Basic Auth header"
```

---

## Task 4: GitLab archive fetch (network code, no unit test)

**Files:**
- Modify: `src-tauri/src/error_code/gitlab.rs`

- [ ] **Step 1: Add `SyncError` enum and `fetch_archive` function**

Append to `src-tauri/src/error_code/gitlab.rs` **above** the `#[cfg(test)]` block:

```rust
use std::time::Duration;

#[derive(Debug)]
pub enum SyncError {
    Network(String),
    Auth,
    Http(u16),
    Archive(String),
    Io(String),
}

impl SyncError {
    /// Maps to the i18n toast key surfaced to the user.
    pub fn toast_key(&self) -> &'static str {
        match self {
            SyncError::Network(_) => "errorCodeLookup.toast.networkFail",
            SyncError::Auth => "errorCodeLookup.toast.authFail",
            SyncError::Http(_) => "errorCodeLookup.toast.httpError",
            SyncError::Archive(_) | SyncError::Io(_) => "errorCodeLookup.toast.archiveError",
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Network(msg) => write!(f, "network: {msg}"),
            SyncError::Auth => write!(f, "auth_failed"),
            SyncError::Http(status) => write!(f, "http_{status}"),
            SyncError::Archive(msg) => write!(f, "archive: {msg}"),
            SyncError::Io(msg) => write!(f, "io: {msg}"),
        }
    }
}

impl std::error::Error for SyncError {}

pub async fn fetch_archive() -> Result<bytes::Bytes, SyncError> {
    let url = build_archive_url();
    let auth = build_basic_auth_header();
    log::info!("[error_code] 开始下载 GitLab 归档：{}", url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| SyncError::Network(e.to_string()))?;

    let response = client
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, auth)
        .send()
        .await
        .map_err(|e| SyncError::Network(e.to_string()))?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(SyncError::Auth);
    }
    if !status.is_success() {
        return Err(SyncError::Http(status.as_u16()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| SyncError::Network(e.to_string()))?;

    log::info!("[error_code] 已下载归档：{} 字节", bytes.len());
    Ok(bytes)
}
```

- [ ] **Step 2: Verify build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: compiles cleanly (other than dead-code warnings — `fetch_archive` not yet called).

- [ ] **Step 3: Re-run existing tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::gitlab::tests
```

Expected: still 2 passed (no regression).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/error_code/gitlab.rs
git commit -m "feat(error-code): fetch GitLab archive over HTTP with Basic Auth"
```

---

## Task 5: Encoding detection (TDD)

**Files:**
- Modify: `src-tauri/src/error_code/parser.rs`

- [ ] **Step 1: Write failing tests**

Replace contents of `src-tauri/src/error_code/parser.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::GBK;

    #[test]
    fn decode_ascii_is_identity() {
        let s = decode_bytes(b"hello,world");
        assert_eq!(s, "hello,world");
    }

    #[test]
    fn decode_utf8_chinese_round_trip() {
        let s = decode_bytes("执行成功".as_bytes());
        assert_eq!(s, "执行成功");
    }

    #[test]
    fn decode_gbk_chinese_round_trip() {
        let (encoded, _, had_errors) = GBK.encode("执行成功");
        assert!(!had_errors);
        let decoded = decode_bytes(&encoded);
        assert_eq!(decoded, "执行成功");
    }

    #[test]
    fn decode_strips_utf8_bom() {
        let mut bytes: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("0,执行成功".as_bytes());
        let decoded = decode_bytes(&bytes);
        assert_eq!(decoded, "0,执行成功");
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::parser
```

Expected: compile error — `decode_bytes` not defined.

- [ ] **Step 3: Implement**

Prepend (above the `#[cfg(test)]` block) the implementation:

```rust
use chardetng::EncodingDetector;
use encoding_rs::Encoding;

/// Detect the most likely encoding of `bytes` using chardetng.
/// Falls back to UTF-8 when the detector cannot decide.
pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    detector.guess(None, true)
}

/// Decode `bytes` as text, transparently stripping a leading UTF-8 BOM.
pub fn decode_bytes(bytes: &[u8]) -> String {
    let encoding = detect_encoding(bytes);
    let (cow, _, _) = encoding.decode(bytes);
    let s = cow.into_owned();
    s.strip_prefix('\u{FEFF}').map(str::to_string).unwrap_or(s)
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::parser::tests
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/parser.rs
git commit -m "feat(error-code): detect encoding and decode CSV bytes"
```

---

## Task 6: CSV row parsing (TDD)

**Files:**
- Modify: `src-tauri/src/error_code/parser.rs`

- [ ] **Step 1: Add failing tests**

Inside the existing `#[cfg(test)] mod tests` block in `parser.rs`, append:

```rust
    #[test]
    fn parse_simple_row_with_header() {
        let text = "code,cn,en,solution,module,remark\n0,执行成功,Success.,,,";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.code, 0);
        assert_eq!(e.message_cn, "执行成功");
        assert_eq!(e.message_en, "Success.");
        assert_eq!(e.solution, "");
        assert_eq!(e.module, "");
        assert_eq!(e.remark, "");
        assert_eq!(e.source_file, "10w.csv");
    }

    #[test]
    fn parse_skips_non_numeric_first_cell() {
        let text = "code,cn,en,solution,module,remark\nABC,无效,Invalid,,,";
        let entries = parse_csv_text(text, "10w.csv");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_handles_quoted_comma_in_solution() {
        let text =
            "code,cn,en,solution,module,remark\n100,异常,Error,\"重启服务,然后重试\",CORE,";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].solution, "重启服务,然后重试");
    }

    #[test]
    fn parse_pads_short_rows_with_empty_strings() {
        let text = "code,cn,en,solution,module,remark\n5,简短行";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_cn, "简短行");
        assert_eq!(entries[0].solution, "");
        assert_eq!(entries[0].module, "");
    }

    #[test]
    fn parse_csv_bytes_handles_gbk() {
        let text = "code,cn,en,solution,module,remark\n0,执行成功,Success.,,,";
        let (encoded, _, had_errors) = encoding_rs::GBK.encode(text);
        assert!(!had_errors);
        let entries = parse_csv_bytes(&encoded, "20w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_cn, "执行成功");
    }
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::parser
```

Expected: compile error — `parse_csv_text`, `parse_csv_bytes` not defined.

- [ ] **Step 3: Implement parsers**

Append (above the `#[cfg(test)]` block) in `src-tauri/src/error_code/parser.rs`:

```rust
use crate::error_code::ErrorCodeEntry;

/// Parse already-decoded CSV text. Header row is skipped.
/// Rows where the first cell does not parse as `u32` are dropped (logged at WARN).
/// Rows shorter than 6 columns are padded with empty strings.
pub fn parse_csv_text(text: &str, source_file: &str) -> Vec<ErrorCodeEntry> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());

    let mut entries = Vec::new();
    for (idx, result) in reader.records().enumerate() {
        let line_num = idx + 2; // +1 for 1-indexing, +1 for header row
        match result {
            Ok(record) => {
                if let Some(entry) = parse_row(&record, source_file) {
                    entries.push(entry);
                } else {
                    log::warn!(
                        "[error_code] 跳过无效行 {}:{} -> {:?}",
                        source_file,
                        line_num,
                        record
                    );
                }
            }
            Err(err) => {
                log::warn!(
                    "[error_code] CSV 解析错误 {}:{}: {}",
                    source_file,
                    line_num,
                    err
                );
            }
        }
    }
    entries
}

/// Decode bytes (handling encoding + BOM), then parse CSV text.
pub fn parse_csv_bytes(bytes: &[u8], source_file: &str) -> Vec<ErrorCodeEntry> {
    let text = decode_bytes(bytes);
    parse_csv_text(&text, source_file)
}

fn parse_row(record: &csv::StringRecord, source_file: &str) -> Option<ErrorCodeEntry> {
    let code_cell = record.get(0)?.trim();
    let code: u32 = code_cell.parse().ok()?;
    Some(ErrorCodeEntry {
        code,
        message_cn: cell(record, 1),
        message_en: cell(record, 2),
        solution: cell(record, 3),
        module: cell(record, 4),
        remark: cell(record, 5),
        source_file: source_file.to_string(),
    })
}

fn cell(record: &csv::StringRecord, idx: usize) -> String {
    record.get(idx).unwrap_or("").trim().to_string()
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::parser
```

Expected: 9 passed (4 from Task 5 + 5 new).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/parser.rs
git commit -m "feat(error-code): parse CSV rows into ErrorCodeEntry"
```

---

## Task 7: Store ingestion + single query + pagination (TDD)

**Files:**
- Modify: `src-tauri/src/error_code/store.rs`

- [ ] **Step 1: Write failing tests**

Replace `src-tauri/src/error_code/store.rs` with:

```rust
use crate::error_code::{ErrorCodeEntry, ErrorCodeStore, QueryResult, MAX_RANGE_SPAN, PAGE_SIZE};

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(code: u32, cn: &str, en: &str, solution: &str, source: &str) -> ErrorCodeEntry {
        ErrorCodeEntry {
            code,
            message_cn: cn.to_string(),
            message_en: en.to_string(),
            solution: solution.to_string(),
            module: String::new(),
            remark: String::new(),
            source_file: source.to_string(),
        }
    }

    fn store_with(items: Vec<ErrorCodeEntry>) -> ErrorCodeStore {
        let mut s = ErrorCodeStore::default();
        s.ingest(items);
        s.loaded = true;
        s
    }

    #[test]
    fn ingest_clears_previous_entries() {
        let mut s = ErrorCodeStore::default();
        s.ingest(vec![entry(1, "a", "a", "", "10w.csv")]);
        s.ingest(vec![entry(2, "b", "b", "", "20w.csv")]);
        assert_eq!(s.entries.len(), 1);
        assert!(s.entries.contains_key(&2));
        assert!(!s.entries.contains_key(&1));
    }

    #[test]
    fn single_query_returns_matching_entry() {
        let s = store_with(vec![
            entry(0, "执行成功", "Success.", "", "10w.csv"),
            entry(1, "执行失败", "Error.", "", "10w.csv"),
        ]);
        let r = s.query_single(1, 1);
        assert_eq!(r.total, 1);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].message_en, "Error.");
        assert_eq!(r.page, 1);
        assert_eq!(r.page_size, PAGE_SIZE);
    }

    #[test]
    fn single_query_returns_empty_when_missing() {
        let s = store_with(vec![entry(0, "执行成功", "Success.", "", "10w.csv")]);
        let r = s.query_single(999, 1);
        assert_eq!(r.total, 0);
        assert!(r.entries.is_empty());
    }

    #[test]
    fn single_query_returns_multiple_when_same_code_in_two_files() {
        let s = store_with(vec![
            entry(100, "异常 A", "Err A", "", "10w.csv"),
            entry(100, "异常 B", "Err B", "", "20w.csv"),
        ]);
        let r = s.query_single(100, 1);
        assert_eq!(r.total, 2);
        assert_eq!(r.entries.len(), 2);
    }

    #[test]
    fn pagination_returns_first_page_then_second() {
        let mut items: Vec<ErrorCodeEntry> = (0..120)
            .map(|i| entry(i, &format!("cn{i}"), "", "", "10w.csv"))
            .collect();
        items.sort_by_key(|e| e.code);
        let s = store_with(items);
        // We piggy-back on query_keyword with empty needle to get all entries; tested in Task 9.
        // For now, exercise the helper directly.
        let r = paginate_for_test(&s, 1);
        assert_eq!(r.entries.len(), 50);
        assert_eq!(r.entries[0].code, 0);
        assert_eq!(r.total, 120);
        let r = paginate_for_test(&s, 2);
        assert_eq!(r.entries.len(), 50);
        assert_eq!(r.entries[0].code, 50);
        let r = paginate_for_test(&s, 3);
        assert_eq!(r.entries.len(), 20);
    }

    #[test]
    fn pagination_normalizes_page_zero_to_one_and_clamps_overshoot() {
        let s = store_with(vec![entry(1, "a", "", "", "10w.csv")]);
        let r = paginate_for_test(&s, 0);
        assert_eq!(r.page, 1);
        assert_eq!(r.entries.len(), 1);
        let r = paginate_for_test(&s, 999);
        assert!(r.entries.is_empty());
    }

    fn paginate_for_test(s: &ErrorCodeStore, page: u32) -> QueryResult {
        let all: Vec<ErrorCodeEntry> = s.entries.values().flatten().cloned().collect();
        super::paginate(all, page)
    }
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::store
```

Expected: compile error — `ingest`, `query_single`, `paginate` not defined.

- [ ] **Step 3: Implement**

Above the `#[cfg(test)]` block in `store.rs`, add:

```rust
impl ErrorCodeStore {
    /// Replace all in-memory entries with `items` (grouped by code).
    pub fn ingest(&mut self, items: Vec<ErrorCodeEntry>) {
        self.entries.clear();
        for entry in items {
            self.entries.entry(entry.code).or_default().push(entry);
        }
    }

    pub fn row_count(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }

    pub fn query_single(&self, code: u32, page: u32) -> QueryResult {
        let hits: Vec<ErrorCodeEntry> = self
            .entries
            .get(&code)
            .map(|v| v.clone())
            .unwrap_or_default();
        paginate(hits, page)
    }
}

pub(crate) fn paginate(items: Vec<ErrorCodeEntry>, page: u32) -> QueryResult {
    let total = items.len();
    let normalized_page = page.max(1);
    let page_idx = (normalized_page - 1) as usize;
    let start = page_idx.saturating_mul(PAGE_SIZE as usize);
    let end = (start + PAGE_SIZE as usize).min(total);
    let entries = if start >= total {
        Vec::new()
    } else {
        items[start..end].to_vec()
    };
    QueryResult {
        entries,
        total,
        page: normalized_page,
        page_size: PAGE_SIZE,
    }
}

// Silence unused-import warning until Task 8 lands.
#[allow(dead_code)]
const _MAX_SPAN: u32 = MAX_RANGE_SPAN;
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::store
```

Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/store.rs
git commit -m "feat(error-code): in-memory store with single-code query and pagination"
```

---

## Task 8: Range query (TDD)

**Files:**
- Modify: `src-tauri/src/error_code/store.rs`

- [ ] **Step 1: Write failing tests**

In `store.rs`, inside the existing `#[cfg(test)] mod tests` block, append:

```rust
    #[test]
    fn range_query_returns_entries_sorted_ascending() {
        let s = store_with(vec![
            entry(300_500, "C", "C", "", "30w.csv"),
            entry(300_100, "A", "A", "", "30w.csv"),
            entry(300_900, "D", "D", "", "30w.csv"),
            entry(300_300, "B", "B", "", "30w.csv"),
            entry(400_000, "Z", "Z", "", "40w.csv"), // out of range
        ]);
        let r = s.query_range(300_000, 301_000, 1).expect("ok");
        assert_eq!(r.total, 4);
        let codes: Vec<u32> = r.entries.iter().map(|e| e.code).collect();
        assert_eq!(codes, vec![300_100, 300_300, 300_500, 300_900]);
    }

    #[test]
    fn range_query_inclusive_endpoints() {
        let s = store_with(vec![
            entry(100, "L", "", "", "10w.csv"),
            entry(200, "R", "", "", "10w.csv"),
        ]);
        let r = s.query_range(100, 200, 1).expect("ok");
        assert_eq!(r.total, 2);
    }

    #[test]
    fn range_query_rejects_span_above_1000() {
        let s = ErrorCodeStore::default();
        let err = s.query_range(0, 1_001, 1).unwrap_err();
        assert_eq!(err, "range_too_large");
    }

    #[test]
    fn range_query_accepts_span_exactly_1000() {
        let s = ErrorCodeStore::default();
        assert!(s.query_range(300_000, 301_000, 1).is_ok());
    }

    #[test]
    fn range_query_rejects_reversed_endpoints() {
        let s = ErrorCodeStore::default();
        let err = s.query_range(500, 100, 1).unwrap_err();
        assert_eq!(err, "range_reversed");
    }
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::store::tests::range_
```

Expected: compile error — `query_range` not defined.

- [ ] **Step 3: Implement**

Inside the `impl ErrorCodeStore { … }` block in `store.rs`, add:

```rust
    pub fn query_range(
        &self,
        start: u32,
        end: u32,
        page: u32,
    ) -> Result<QueryResult, &'static str> {
        if end < start {
            return Err("range_reversed");
        }
        if end - start > MAX_RANGE_SPAN {
            return Err("range_too_large");
        }
        let hits: Vec<ErrorCodeEntry> = self
            .entries
            .range(start..=end)
            .flat_map(|(_, v)| v.clone())
            .collect();
        Ok(paginate(hits, page))
    }
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::store
```

Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/store.rs
git commit -m "feat(error-code): range query with span limit and reversal guard"
```

---

## Task 9: Keyword query (TDD)

**Files:**
- Modify: `src-tauri/src/error_code/store.rs`

- [ ] **Step 1: Write failing tests**

In `store.rs`, append inside the existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn keyword_query_matches_in_message_cn_en_and_solution() {
        let s = store_with(vec![
            entry(1, "执行失败", "Error.", "", "10w.csv"),
            entry(2, "成功", "Success.", "", "10w.csv"),
            entry(3, "其他", "Other.", "请重启服务", "10w.csv"),
        ]);
        let r = s.query_keyword("失败", 1);
        assert_eq!(r.total, 1);
        assert_eq!(r.entries[0].code, 1);

        let r = s.query_keyword("Success", 1);
        assert_eq!(r.total, 1);
        assert_eq!(r.entries[0].code, 2);

        let r = s.query_keyword("重启", 1);
        assert_eq!(r.total, 1);
        assert_eq!(r.entries[0].code, 3);
    }

    #[test]
    fn keyword_query_is_case_insensitive() {
        let s = store_with(vec![entry(1, "Foo", "BarBaz", "", "10w.csv")]);
        assert_eq!(s.query_keyword("BARBAZ", 1).total, 1);
        assert_eq!(s.query_keyword("foo", 1).total, 1);
    }

    #[test]
    fn keyword_query_empty_returns_all_sorted() {
        let s = store_with(vec![
            entry(2, "B", "", "", "10w.csv"),
            entry(1, "A", "", "", "10w.csv"),
            entry(3, "C", "", "", "10w.csv"),
        ]);
        let r = s.query_keyword("", 1);
        assert_eq!(r.total, 3);
        let codes: Vec<u32> = r.entries.iter().map(|e| e.code).collect();
        assert_eq!(codes, vec![1, 2, 3]);
    }

    #[test]
    fn keyword_query_no_match_returns_empty() {
        let s = store_with(vec![entry(1, "执行失败", "Error.", "", "10w.csv")]);
        let r = s.query_keyword("nonexistent", 1);
        assert_eq!(r.total, 0);
        assert!(r.entries.is_empty());
    }
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::store::tests::keyword_
```

Expected: compile error — `query_keyword` not defined.

- [ ] **Step 3: Implement**

Inside the `impl ErrorCodeStore { … }` block in `store.rs`, add:

```rust
    pub fn query_keyword(&self, keyword: &str, page: u32) -> QueryResult {
        let needle = keyword.trim().to_lowercase();
        let hits: Vec<ErrorCodeEntry> = self
            .entries
            .values()
            .flatten()
            .filter(|e| {
                if needle.is_empty() {
                    return true;
                }
                e.message_cn.to_lowercase().contains(&needle)
                    || e.message_en.to_lowercase().contains(&needle)
                    || e.solution.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        paginate(hits, page)
    }
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::store
```

Expected: 15 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/store.rs
git commit -m "feat(error-code): case-insensitive keyword query across cn/en/solution"
```

---

## Task 10: Disk cache I/O (TDD with tempfile)

**Files:**
- Modify: `src-tauri/src/error_code/cache.rs`

- [ ] **Step 1: Write failing tests**

Replace `src-tauri/src/error_code/cache.rs` with:

```rust
use crate::error_code::ErrorCodeEntry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CacheMeta {
    pub last_synced_at: Option<String>,
    pub file_count: usize,
    pub row_count: usize,
}

pub fn cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("errorcode_cache")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        let files = vec![
            ("10w.csv".to_string(), b"code,cn,en,solution,module,remark\n0,A,A,,,".to_vec()),
            ("20w.csv".to_string(), b"code,cn,en,solution,module,remark\n200,B,B,,,".to_vec()),
        ];
        let meta = CacheMeta {
            last_synced_at: Some("2026-04-25T10:00:00+08:00".to_string()),
            file_count: 2,
            row_count: 2,
        };
        write_cache(&dir, &files, &meta).unwrap();

        assert!(dir.join("10w.csv").exists());
        assert!(dir.join("20w.csv").exists());
        assert!(dir.join("meta.json").exists());

        let entries = load_cache_entries(&dir);
        assert_eq!(entries.len(), 2);
        let read_meta = read_meta(&dir).unwrap();
        assert_eq!(read_meta.file_count, 2);
        assert_eq!(read_meta.row_count, 2);
    }

    #[test]
    fn write_cache_sweeps_orphan_csvs_only() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        // Pre-existing files
        fs::write(dir.join("legacy.csv"), b"code,cn,en,solution,module,remark\n1,X,X,,,").unwrap();
        fs::write(dir.join("README.txt"), b"keep me").unwrap();

        let files = vec![(
            "10w.csv".to_string(),
            b"code,cn,en,solution,module,remark\n0,A,A,,,".to_vec(),
        )];
        let meta = CacheMeta::default();
        write_cache(&dir, &files, &meta).unwrap();

        assert!(dir.join("10w.csv").exists());
        assert!(!dir.join("legacy.csv").exists(), "orphan CSV should be swept");
        assert!(dir.join("README.txt").exists(), "non-CSV files must be preserved");
    }

    #[test]
    fn read_meta_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        assert!(read_meta(&dir).is_none());
    }

    #[test]
    fn load_cache_entries_returns_empty_for_missing_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path()); // not created
        assert!(load_cache_entries(&dir).is_empty());
    }
}
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::cache
```

Expected: compile error — `write_cache`, `load_cache_entries`, `read_meta` not defined.

- [ ] **Step 3: Implement**

Append to `src-tauri/src/error_code/cache.rs`, above the `#[cfg(test)]` block:

```rust
use crate::error_code::parser::parse_csv_bytes;

pub fn write_cache(
    dir: &Path,
    files: &[(String, Vec<u8>)],
    meta: &CacheMeta,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;

    // Sweep orphan *.csv files that are not in the new payload.
    let new_names: std::collections::HashSet<&str> =
        files.iter().map(|(n, _)| n.as_str()).collect();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".csv") && !new_names.contains(name) {
                let _ = fs::remove_file(&path);
            }
        }
    }

    for (name, bytes) in files {
        fs::write(dir.join(name), bytes)?;
    }
    let meta_json = serde_json::to_vec_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(dir.join("meta.json"), meta_json)?;
    Ok(())
}

pub fn load_cache_entries(dir: &Path) -> Vec<ErrorCodeEntry> {
    let mut entries = Vec::new();
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return entries,
    };
    for item in read.flatten() {
        let path = item.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.to_ascii_lowercase().ends_with(".csv") {
            continue;
        }
        match fs::read(&path) {
            Ok(bytes) => entries.extend(parse_csv_bytes(&bytes, &name)),
            Err(err) => log::warn!(
                "[error_code] 读取缓存 CSV 失败 {}: {}",
                path.display(),
                err
            ),
        }
    }
    entries
}

pub fn read_meta(dir: &Path) -> Option<CacheMeta> {
    let path = dir.join("meta.json");
    let bytes = fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn has_cache(dir: &Path) -> bool {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return false,
    };
    for item in read.flatten() {
        if let Some(name) = item.file_name().to_str() {
            if name.to_ascii_lowercase().ends_with(".csv") {
                return true;
            }
        }
    }
    false
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::cache
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/cache.rs
git commit -m "feat(error-code): disk cache write/load/sweep + meta.json"
```

---

## Task 11: Sync orchestration (no unit test — integration via manual QA)

**Files:**
- Modify: `src-tauri/src/error_code/sync.rs`

- [ ] **Step 1: Implement orchestration**

Replace `src-tauri/src/error_code/sync.rs` with:

```rust
use crate::error_code::{
    cache::{self, CacheMeta},
    gitlab::{self, SyncError},
    parser,
    ErrorCodeEntry, ErrorCodeStore, SyncReport,
};
use chrono::Local;
use std::io::Read;
use std::path::Path;

/// Perform a full sync: download archive, unzip, parse, persist, and update store.
/// Returns `SyncReport` on success. On failure the store and disk cache are unchanged.
pub async fn run_sync(
    cache_root: &Path,
    store: &std::sync::Mutex<ErrorCodeStore>,
) -> Result<SyncReport, SyncError> {
    let bytes = gitlab::fetch_archive().await?;

    let files = extract_csvs_from_zip(&bytes)?;
    if files.is_empty() {
        return Err(SyncError::Archive("归档中未找到任何 CSV 文件".into()));
    }

    let mut all_entries: Vec<ErrorCodeEntry> = Vec::new();
    for (name, raw) in &files {
        let parsed = parser::parse_csv_bytes(raw, name);
        log::info!("[error_code] 解析 {} -> {} 行", name, parsed.len());
        all_entries.extend(parsed);
    }
    let row_count = all_entries.len();
    let file_count = files.len();
    let now = Local::now().to_rfc3339();

    let meta = CacheMeta {
        last_synced_at: Some(now.clone()),
        file_count,
        row_count,
    };
    let dir = cache::cache_dir(cache_root);
    cache::write_cache(&dir, &files, &meta).map_err(|e| SyncError::Io(e.to_string()))?;

    {
        let mut store = store.lock().expect("error_code store mutex poisoned");
        store.ingest(all_entries);
        store.last_synced_at = Some(now.clone());
        store.loaded = true;
    }

    log::info!(
        "[error_code] 同步完成：{} 文件 / {} 行 @ {}",
        file_count,
        row_count,
        now
    );

    Ok(SyncReport {
        file_count,
        row_count,
        last_synced_at: now,
    })
}

/// Lazy load from disk on first query after app startup.
/// Idempotent — does nothing if already loaded.
pub fn ensure_loaded(cache_root: &Path, store: &std::sync::Mutex<ErrorCodeStore>) {
    {
        let s = store.lock().expect("error_code store mutex poisoned");
        if s.loaded {
            return;
        }
    }
    let dir = cache::cache_dir(cache_root);
    let entries = cache::load_cache_entries(&dir);
    let last = cache::read_meta(&dir).and_then(|m| m.last_synced_at);
    let mut s = store.lock().expect("error_code store mutex poisoned");
    if !s.loaded {
        s.ingest(entries);
        s.last_synced_at = last;
        s.loaded = true;
    }
}

fn extract_csvs_from_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| SyncError::Archive(e.to_string()))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| SyncError::Archive(e.to_string()))?;
        if !file.is_file() {
            continue;
        }
        let name = file.name().to_string();
        let basename = match std::path::Path::new(&name).file_name().and_then(|s| s.to_str()) {
            Some(b) => b.to_string(),
            None => continue,
        };
        if !basename.to_ascii_lowercase().ends_with(".csv") {
            continue;
        }
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf)
            .map_err(|e| SyncError::Archive(e.to_string()))?;
        out.push((basename, buf));
    }
    Ok(out)
}
```

- [ ] **Step 2: Verify build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: clean compile (warnings about unused fns elsewhere are OK).

- [ ] **Step 3: Add an extraction-only unit test**

Append to `src-tauri/src/error_code/sync.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn build_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in files {
                writer.start_file(*name, opts).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn extract_csvs_filters_non_csv_and_strips_directory_prefix() {
        let zip_bytes = build_zip(&[
            ("errorcode-main-abc/10w.csv", b"code,cn,en,solution,module,remark\n0,A,A,,,"),
            ("errorcode-main-abc/README.md", b"# readme"),
            ("errorcode-main-abc/sub/20w.csv", b"code,cn,en,solution,module,remark\n200,B,B,,,"),
        ]);
        let result = extract_csvs_from_zip(&zip_bytes).unwrap();
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"10w.csv"));
        assert!(names.contains(&"20w.csv"));
        assert!(!names.iter().any(|n| n.contains("README")));
    }
}
```

- [ ] **Step 4: Run the new test**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code::sync
```

Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error_code/sync.rs
git commit -m "feat(error-code): orchestrate fetch -> unzip -> parse -> cache -> store"
```

---

## Task 12: Tauri commands

**Files:**
- Modify: `src-tauri/src/error_code/commands.rs`

- [ ] **Step 1: Implement command handlers**

Replace `src-tauri/src/error_code/commands.rs` with:

```rust
use crate::error_code::{
    cache::{self, has_cache, read_meta},
    sync as sync_mod, ErrorCodeState, MetaInfo, QueryRequest, QueryResult, SyncReport,
};
use crate::AppState;
use std::path::PathBuf;
use tauri::{Manager, State};

fn cache_root(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法解析 app_data_dir: {e}"))
}

#[tauri::command]
pub async fn error_code_sync(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SyncReport, String> {
    let root = cache_root(&app_handle)?;
    sync_mod::run_sync(&root, &state.error_code)
        .await
        .map_err(|e| format!("{}|{}", e.toast_key(), e))
}

#[tauri::command]
pub fn error_code_query(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    request: QueryRequest,
) -> Result<QueryResult, String> {
    let root = cache_root(&app_handle)?;
    sync_mod::ensure_loaded(&root, &state.error_code);

    let store = state.error_code.lock().map_err(|_| "store_poisoned".to_string())?;
    match request.mode.as_str() {
        "single" => {
            let code: u32 = request
                .value
                .trim()
                .parse()
                .map_err(|_| "invalid_single".to_string())?;
            Ok(store.query_single(code, request.page))
        }
        "range" => {
            let raw = request.value.trim();
            let (start_s, end_s) = raw
                .split_once('-')
                .ok_or_else(|| "invalid_range_format".to_string())?;
            let start: u32 = start_s
                .trim()
                .parse()
                .map_err(|_| "invalid_range_format".to_string())?;
            let end: u32 = end_s
                .trim()
                .parse()
                .map_err(|_| "invalid_range_format".to_string())?;
            store.query_range(start, end, request.page).map_err(|s| s.to_string())
        }
        "keyword" => Ok(store.query_keyword(&request.value, request.page)),
        other => Err(format!("unknown_mode: {other}")),
    }
}

#[tauri::command]
pub fn error_code_get_meta(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<MetaInfo, String> {
    let root = cache_root(&app_handle)?;
    sync_mod::ensure_loaded(&root, &state.error_code);

    let dir = cache::cache_dir(&root);
    let cache_present = has_cache(&dir);
    let store = state.error_code.lock().map_err(|_| "store_poisoned".to_string())?;
    let row_count = store.row_count();
    let last_synced_at = store
        .last_synced_at
        .clone()
        .or_else(|| read_meta(&dir).and_then(|m| m.last_synced_at));
    let file_count = read_meta(&dir).map(|m| m.file_count).unwrap_or(0);

    Ok(MetaInfo {
        has_cache: cache_present,
        last_synced_at,
        file_count,
        row_count,
    })
}

#[allow(dead_code)]
fn _state_alias_unused() -> ErrorCodeState {
    std::sync::Mutex::new(Default::default())
}
```

- [ ] **Step 2: Verify build (will fail because AppState doesn't yet have `error_code` field)**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: compile error — `error_code` field missing on `AppState`. Task 13 fixes this.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/error_code/commands.rs
git commit -m "feat(error-code): tauri commands for sync, query, and meta"
```

---

## Task 13: Wire `AppState` and register commands in `main.rs`

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Add `error_code` field to `AppState`**

In `src-tauri/src/main.rs`, find the `struct AppState { … }` definition. Add a new field at the end (before the closing `}`):

```rust
    error_code: error_code::ErrorCodeState,
```

Then locate where `AppState` is constructed (search for `AppState {`). In the construction site, append the field:

```rust
            error_code: std::sync::Mutex::new(error_code::ErrorCodeStore::default()),
```

Match the indentation of surrounding fields. There may be more than one AppState construction site (initial state and any test/manual setup) — update each.

- [ ] **Step 2: Register the three commands in the invoke handler**

Find the `tauri::generate_handler![ … ]` macro call. Append the three commands to the handler list (before the closing `]`):

```rust
            error_code::commands::error_code_sync,
            error_code::commands::error_code_query,
            error_code::commands::error_code_get_meta,
```

- [ ] **Step 3: Verify build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: clean compile.

- [ ] **Step 4: Run the full test suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app error_code
```

Expected: all tests pass (sum of cache + gitlab + parser + store + sync tests).

- [ ] **Step 5: Format and lint**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

If `clippy` flags anything in the new module, fix it before committing. Clippy issues outside `error_code/` predate this work — leave them.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat(error-code): register commands and wire ErrorCodeState into AppState"
```

---

## Task 14: Frontend type wrappers in `tauri.ts`

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add types and api object**

Open `src/lib/tauri.ts`. Find an alphabetical / logical home (e.g., near other tool-feature type blocks). Append the following block at the end of the file (or after similar tool sections, matching existing style):

```ts
// ===== Error Code Lookup =====

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
  sync: () => invoke<ErrorCodeSyncReport>('error_code_sync'),
  query: (request: ErrorCodeQueryRequest) =>
    invoke<ErrorCodeQueryResult>('error_code_query', { request }),
  getMeta: () => invoke<ErrorCodeMetaInfo>('error_code_get_meta'),
};
```

If `invoke` is not yet imported in this file's top-level scope, look at how other API blocks in the same file import it and reuse that pattern (do not duplicate imports).

- [ ] **Step 2: Type-check**

```bash
pnpm check
```

Expected: passes with no new errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(error-code): typed tauri wrappers for error code commands"
```

---

## Task 15: Validation module + tests (TDD)

**Files:**
- Create: `src/pages/errorCodeLookup/validation.ts`
- Create: `src/pages/errorCodeLookup/validation.test.mjs`

- [ ] **Step 1: Write failing tests**

Create directory `src/pages/errorCodeLookup/`. Create `src/pages/errorCodeLookup/validation.test.mjs` with:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parseSingle, parseRange, parseKeyword } from './validation.ts';

test('parseSingle accepts plain decimal', () => {
  assert.deepEqual(parseSingle('110'), { ok: true, code: 110 });
  assert.deepEqual(parseSingle('  300005  '), { ok: true, code: 300005 });
});

test('parseSingle rejects non-numeric', () => {
  assert.deepEqual(parseSingle('abc'), { ok: false, error: 'invalid_single' });
  assert.deepEqual(parseSingle(''), { ok: false, error: 'invalid_single' });
  assert.deepEqual(parseSingle('-5'), { ok: false, error: 'invalid_single' });
  assert.deepEqual(parseSingle('1.5'), { ok: false, error: 'invalid_single' });
});

test('parseRange accepts START-END within span', () => {
  assert.deepEqual(parseRange('300000-301000'), { ok: true, start: 300000, end: 301000 });
  assert.deepEqual(parseRange(' 100 - 200 '), { ok: true, start: 100, end: 200 });
});

test('parseRange rejects bad format', () => {
  assert.deepEqual(parseRange('300000'), { ok: false, error: 'invalid_range_format' });
  assert.deepEqual(parseRange('a-b'), { ok: false, error: 'invalid_range_format' });
  assert.deepEqual(parseRange('300000-'), { ok: false, error: 'invalid_range_format' });
});

test('parseRange rejects reversed endpoints', () => {
  assert.deepEqual(parseRange('500-100'), { ok: false, error: 'range_reversed' });
});

test('parseRange rejects span > 1000', () => {
  assert.deepEqual(parseRange('0-1001'), { ok: false, error: 'range_too_large' });
  assert.deepEqual(parseRange('0-1000'), { ok: true, start: 0, end: 1000 });
});

test('parseKeyword trims and accepts 1..50 chars', () => {
  assert.deepEqual(parseKeyword('  hello  '), { ok: true, keyword: 'hello' });
  assert.deepEqual(parseKeyword(''), { ok: false, error: 'invalid_keyword' });
  assert.deepEqual(parseKeyword('   '), { ok: false, error: 'invalid_keyword' });
  assert.deepEqual(parseKeyword('x'.repeat(50)), { ok: true, keyword: 'x'.repeat(50) });
  assert.deepEqual(parseKeyword('x'.repeat(51)), { ok: false, error: 'invalid_keyword' });
});
```

- [ ] **Step 2: Run to confirm failure**

```bash
node --test src/pages/errorCodeLookup/validation.test.mjs
```

Expected: failure — module not found.

- [ ] **Step 3: Implement validation**

Create `src/pages/errorCodeLookup/validation.ts`:

```ts
export type SingleResult =
  | { ok: true; code: number }
  | { ok: false; error: 'invalid_single' };

export type RangeResult =
  | { ok: true; start: number; end: number }
  | { ok: false; error: 'invalid_range_format' | 'range_reversed' | 'range_too_large' };

export type KeywordResult =
  | { ok: true; keyword: string }
  | { ok: false; error: 'invalid_keyword' };

export const MAX_RANGE_SPAN = 1000;
export const MAX_KEYWORD_LEN = 50;

const DECIMAL_RE = /^\d+$/;

export function parseSingle(raw: string): SingleResult {
  const trimmed = raw.trim();
  if (!DECIMAL_RE.test(trimmed)) {
    return { ok: false, error: 'invalid_single' };
  }
  const code = Number(trimmed);
  if (!Number.isInteger(code) || code < 0) {
    return { ok: false, error: 'invalid_single' };
  }
  return { ok: true, code };
}

export function parseRange(raw: string): RangeResult {
  const trimmed = raw.trim();
  const dash = trimmed.indexOf('-');
  if (dash <= 0 || dash === trimmed.length - 1) {
    return { ok: false, error: 'invalid_range_format' };
  }
  const startStr = trimmed.slice(0, dash).trim();
  const endStr = trimmed.slice(dash + 1).trim();
  if (!DECIMAL_RE.test(startStr) || !DECIMAL_RE.test(endStr)) {
    return { ok: false, error: 'invalid_range_format' };
  }
  const start = Number(startStr);
  const end = Number(endStr);
  if (end < start) {
    return { ok: false, error: 'range_reversed' };
  }
  if (end - start > MAX_RANGE_SPAN) {
    return { ok: false, error: 'range_too_large' };
  }
  return { ok: true, start, end };
}

export function parseKeyword(raw: string): KeywordResult {
  const trimmed = raw.trim();
  if (trimmed.length < 1 || trimmed.length > MAX_KEYWORD_LEN) {
    return { ok: false, error: 'invalid_keyword' };
  }
  return { ok: true, keyword: trimmed };
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
node --test src/pages/errorCodeLookup/validation.test.mjs
```

Expected: 7 passed.

- [ ] **Step 5: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 6: Commit**

```bash
git add src/pages/errorCodeLookup
git commit -m "feat(error-code): pure validation parsers for single/range/keyword inputs"
```

---

## Task 16: i18n strings (zh + en)

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Open the file and locate the `zh` and `en` blocks**

The file `src/locales/messages.ts` exports messages for `zh` and `en` locales. Each is a deeply nested object. Locate:

- `sidebar.*` section (where `clipboardManager` lives)
- `toolsHub.cards.*` section (where `clipboardManager.{chip,description}` lives)
- The top-level location for tool-page messages (where, for example, `clipboardManager.*` page strings live)

- [ ] **Step 2: Add `sidebar.errorCodeLookup`**

Inside the `sidebar` block of **both** `zh` and `en`, after `clipboardManager`, add:

```ts
// zh:
errorCodeLookup: '错误码查询',
// en:
errorCodeLookup: 'Error Code Lookup',
```

- [ ] **Step 3: Add `toolsHub.cards.errorCodeLookup.{chip,description}`**

Inside `toolsHub.cards` of both locales, append:

```ts
// zh:
errorCodeLookup: {
  chip: '开发',
  description: '查询内部 GitLab 错误码字典，支持单码、范围、关键字搜索。',
},
// en:
errorCodeLookup: {
  chip: 'DEV',
  description:
    'Look up internal GitLab error-code dictionaries — single code, range, or keyword search.',
},
```

- [ ] **Step 4: Add page strings under `errorCodeLookup`**

At the same nesting level as other top-level page namespaces (e.g., next to `clipboardManager: { ... }`), add:

```ts
// zh:
errorCodeLookup: {
  title: '错误码查询',
  description: '从内部 GitLab 同步错误码字典，按错误码、范围或关键字检索。',
  syncButton: '同步',
  syncing: '同步中…',
  lastSyncedAt: '上次同步：{time}',
  lastSyncedTooltip: '共 {files} 个文件、{rows} 条错误码',
  neverSynced: '尚未同步',
  syncNowAction: '立即同步',
  modeLabel: '查询模式',
  modes: {
    single: '错误码',
    range: '范围',
    keyword: '关键字',
  },
  placeholders: {
    single: '输入错误码（如 300005）',
    range: '输入范围（如 300000-301000）',
    keyword: '输入中英文关键字',
  },
  searchButton: '查询',
  columns: {
    code: '错误码',
    messageCn: '中文',
    messageEn: '英文',
    module: '模块',
    solution: '解决方案',
    remark: '备注',
  },
  empty: {
    notSynced: '尚未同步错误码字典，点击下方按钮立即同步。',
    singleNotFound: '未找到错误码 {code}',
    rangeNoResult: '该范围内没有错误码',
    keywordNoResult: '未找到包含 "{keyword}" 的错误码',
  },
  errors: {
    invalidSingle: '请输入纯数字错误码',
    invalidRangeFormat: '请输入正确范围（如 300000-301000）',
    rangeReversed: '结束值必须大于等于开始值',
    rangeTooLarge: '范围跨度不能超过 1000',
    invalidKeyword: '关键字长度需为 1-50 字符',
  },
  toast: {
    networkFail: '无法连接到错误码服务器，请检查内网连接',
    authFail: '错误码服务认证失败，请联系开发者',
    httpError: '同步失败：HTTP {status}',
    archiveError: '服务器返回数据异常，请稍后重试',
    syncSuccess: '同步成功：{files} 个文件、{rows} 条错误码',
  },
  pagination: {
    prev: '上一页',
    next: '下一页',
    pageOf: '第 {page} / {total} 页',
    jumpTo: '跳转到',
  },
  detail: {
    expand: '点击展开详情',
    collapse: '点击收起',
  },
},
// en:
errorCodeLookup: {
  title: 'Error Code Lookup',
  description:
    'Sync error-code dictionaries from internal GitLab and search by code, range, or keyword.',
  syncButton: 'Sync',
  syncing: 'Syncing…',
  lastSyncedAt: 'Last synced: {time}',
  lastSyncedTooltip: '{files} file(s), {rows} entries',
  neverSynced: 'Not synced yet',
  syncNowAction: 'Sync now',
  modeLabel: 'Query mode',
  modes: {
    single: 'Code',
    range: 'Range',
    keyword: 'Keyword',
  },
  placeholders: {
    single: 'Enter error code (e.g., 300005)',
    range: 'Enter a range (e.g., 300000-301000)',
    keyword: 'Enter a Chinese or English keyword',
  },
  searchButton: 'Search',
  columns: {
    code: 'Code',
    messageCn: '中文',
    messageEn: 'English',
    module: 'Module',
    solution: 'Solution',
    remark: 'Remark',
  },
  empty: {
    notSynced: 'No dictionary synced yet. Click the button below to sync now.',
    singleNotFound: 'Error code {code} was not found.',
    rangeNoResult: 'No error codes in this range.',
    keywordNoResult: 'No entries match "{keyword}".',
  },
  errors: {
    invalidSingle: 'Please enter a plain decimal error code.',
    invalidRangeFormat: 'Please enter a valid range (e.g., 300000-301000).',
    rangeReversed: 'End value must be greater than or equal to start.',
    rangeTooLarge: 'Range span cannot exceed 1000.',
    invalidKeyword: 'Keyword must be between 1 and 50 characters.',
  },
  toast: {
    networkFail: 'Cannot reach the error-code server. Check your intranet.',
    authFail: 'Error-code service authentication failed. Please contact the developer.',
    httpError: 'Sync failed: HTTP {status}',
    archiveError: 'Server returned unexpected data. Please try again later.',
    syncSuccess: 'Synced {files} file(s), {rows} entries.',
  },
  pagination: {
    prev: 'Previous',
    next: 'Next',
    pageOf: 'Page {page} of {total}',
    jumpTo: 'Jump to',
  },
  detail: {
    expand: 'Click to expand',
    collapse: 'Click to collapse',
  },
},
```

- [ ] **Step 5: Type-check**

```bash
pnpm check
```

Expected: passes. If TypeScript complains about missing-key parity between `zh` and `en`, fix the missing entries before continuing.

- [ ] **Step 6: Commit**

```bash
git add src/locales/messages.ts
git commit -m "feat(error-code): add zh/en strings for sidebar, tools hub, and lookup page"
```

---

## Task 17: `ErrorCodeLookupPage.vue`

**Files:**
- Create: `src/pages/ErrorCodeLookupPage.vue`

- [ ] **Step 1: Write the page**

Create `src/pages/ErrorCodeLookupPage.vue`:

```vue
<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { ChevronLeft, ChevronRight, FileSearch, RefreshCw, Search } from 'lucide-vue-next';

import {
  errorCodeApi,
  type ErrorCodeEntry,
  type ErrorCodeMode,
  type ErrorCodeMetaInfo,
} from '@/lib/tauri';
import { addLog } from '@/lib/store';
import {
  parseKeyword,
  parseRange,
  parseSingle,
} from '@/pages/errorCodeLookup/validation';

type StatusBanner = { type: 'success' | 'error'; message: string };

defineOptions({ name: 'ErrorCodeLookupPage' });

const { t, locale } = useI18n();

type LastQuery = {
  mode: ErrorCodeMode;
  value: string;
  start?: number;
  end?: number;
  code?: number;
  keyword?: string;
};

const mode = ref<ErrorCodeMode>('single');
const inputValue = ref('');
const inputError = ref<string | null>(null);
const submitting = ref(false);
const syncing = ref(false);
const meta = ref<ErrorCodeMetaInfo>({
  has_cache: false,
  last_synced_at: null,
  file_count: 0,
  row_count: 0,
});
const entries = ref<ErrorCodeEntry[]>([]);
const total = ref(0);
const currentPage = ref(1);
const pageSize = 50;
const expandedKey = ref<string | null>(null);
const lastQuery = ref<LastQuery | null>(null);
const noResultMessage = ref<string | null>(null);
const statusBanner = ref<StatusBanner | null>(null);

const totalPages = computed(() =>
  total.value === 0 ? 0 : Math.ceil(total.value / pageSize),
);

const placeholder = computed(() => t(`errorCodeLookup.placeholders.${mode.value}`));

const lastSyncedDisplay = computed(() => {
  if (!meta.value.last_synced_at) return t('errorCodeLookup.neverSynced');
  return t('errorCodeLookup.lastSyncedAt', {
    time: formatTime(meta.value.last_synced_at),
  });
});

const lastSyncedTooltip = computed(() =>
  t('errorCodeLookup.lastSyncedTooltip', {
    files: meta.value.file_count,
    rows: meta.value.row_count,
  }),
);

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return d.toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US');
  } catch {
    return iso;
  }
}

function rowKey(entry: ErrorCodeEntry, idx: number): string {
  return `${entry.source_file}:${entry.code}:${idx}`;
}

function toggleExpand(entry: ErrorCodeEntry, idx: number) {
  const key = rowKey(entry, idx);
  expandedKey.value = expandedKey.value === key ? null : key;
}

watch(mode, () => {
  inputValue.value = '';
  inputError.value = null;
  expandedKey.value = null;
});

async function loadMeta() {
  try {
    meta.value = await errorCodeApi.getMeta();
  } catch (err) {
    addLog(`[error_code] 获取元数据失败：${String(err)}`, 'error');
  }
}

async function runDefaultPreview() {
  // Empty keyword returns all entries sorted by code asc.
  await fetchPage({ mode: 'keyword', value: '', keyword: '' }, 1);
}

async function onSearch() {
  inputError.value = null;
  expandedKey.value = null;
  let prepared: LastQuery | null = null;

  if (mode.value === 'single') {
    const result = parseSingle(inputValue.value);
    if (!result.ok) {
      inputError.value = t(`errorCodeLookup.errors.${camel(result.error)}`);
      return;
    }
    prepared = { mode: 'single', value: String(result.code), code: result.code };
  } else if (mode.value === 'range') {
    const result = parseRange(inputValue.value);
    if (!result.ok) {
      inputError.value = t(`errorCodeLookup.errors.${camel(result.error)}`);
      return;
    }
    prepared = {
      mode: 'range',
      value: `${result.start}-${result.end}`,
      start: result.start,
      end: result.end,
    };
  } else {
    const result = parseKeyword(inputValue.value);
    if (!result.ok) {
      inputError.value = t(`errorCodeLookup.errors.${camel(result.error)}`);
      return;
    }
    prepared = { mode: 'keyword', value: result.keyword, keyword: result.keyword };
  }

  await fetchPage(prepared, 1);
}

async function fetchPage(query: LastQuery, page: number) {
  submitting.value = true;
  try {
    const result = await errorCodeApi.query({
      mode: query.mode,
      value: query.value,
      page,
    });
    entries.value = result.entries;
    total.value = result.total;
    currentPage.value = result.page;
    lastQuery.value = query;
    noResultMessage.value = computeNoResultMessage(query, result.total);
  } catch (err) {
    inputError.value = String(err);
  } finally {
    submitting.value = false;
  }
}

function computeNoResultMessage(query: LastQuery, count: number): string | null {
  if (count > 0) return null;
  if (query.mode === 'single') {
    return t('errorCodeLookup.empty.singleNotFound', { code: query.code });
  }
  if (query.mode === 'range') {
    return t('errorCodeLookup.empty.rangeNoResult');
  }
  if ((query.keyword ?? '').length === 0) {
    return null; // empty preview, no message
  }
  return t('errorCodeLookup.empty.keywordNoResult', { keyword: query.keyword });
}

async function changePage(delta: number) {
  if (!lastQuery.value) return;
  const next = currentPage.value + delta;
  if (next < 1 || (totalPages.value > 0 && next > totalPages.value)) return;
  await fetchPage(lastQuery.value, next);
}

const jumpInput = ref('');
async function onJump() {
  if (!lastQuery.value) return;
  const trimmed = jumpInput.value.trim();
  if (!/^\d+$/.test(trimmed)) return;
  const target = Math.min(Math.max(Number(trimmed), 1), Math.max(totalPages.value, 1));
  jumpInput.value = '';
  await fetchPage(lastQuery.value, target);
}

async function onSync() {
  syncing.value = true;
  statusBanner.value = null;
  try {
    const report = await errorCodeApi.sync();
    const successMsg = t('errorCodeLookup.toast.syncSuccess', {
      files: report.file_count,
      rows: report.row_count,
    });
    addLog(`[error_code] ${successMsg}`, 'success');
    statusBanner.value = { type: 'success', message: successMsg };
    await loadMeta();
    await runDefaultPreview();
  } catch (err) {
    const raw = String(err);
    const [keyPart, detail = ''] = raw.split('|');
    const toastKey = keyPart.startsWith('errorCodeLookup.')
      ? keyPart
      : 'errorCodeLookup.toast.archiveError';
    const statusMatch = detail.match(/http_(\d+)/);
    const message = t(toastKey, { status: statusMatch ? statusMatch[1] : '' });
    addLog(`[error_code] 同步失败：${message} (${detail})`, 'error');
    statusBanner.value = { type: 'error', message };
  } finally {
    syncing.value = false;
  }
}

function camel(snake: string): string {
  return snake.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

onMounted(async () => {
  await loadMeta();
  if (meta.value.has_cache) {
    await runDefaultPreview();
  }
});
</script>

<template>
  <div
    class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(99,102,241,0.16),_transparent_30%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_42%,_#f8fafc_100%)]"
  >
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-6 px-6 py-6 pb-10">
      <!-- Header -->
      <section
        class="rounded-[24px] border border-white/70 bg-white/85 px-6 py-5 shadow-[0_18px_60px_rgba(15,23,42,0.08)] backdrop-blur"
      >
        <div class="flex items-start justify-between gap-4">
          <div class="space-y-2">
            <div class="flex items-center gap-2 text-slate-500">
              <FileSearch class="h-4 w-4" />
              <span class="text-[11px] font-bold uppercase tracking-[0.18em]">
                {{ t('toolsHub.cards.errorCodeLookup.chip') }}
              </span>
            </div>
            <h1 class="text-2xl font-bold text-slate-950">
              {{ t('errorCodeLookup.title') }}
            </h1>
            <p class="text-sm text-slate-500">{{ t('errorCodeLookup.description') }}</p>
          </div>
          <div class="flex shrink-0 items-center gap-3">
            <span
              class="text-xs text-slate-500"
              :title="meta.has_cache ? lastSyncedTooltip : ''"
            >
              {{ lastSyncedDisplay }}
            </span>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-xl border border-indigo-200 bg-indigo-500 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-indigo-600 disabled:cursor-not-allowed disabled:opacity-60"
              :disabled="syncing"
              @click="onSync"
            >
              <RefreshCw class="h-4 w-4" :class="syncing ? 'animate-spin' : ''" />
              <span>{{ syncing ? t('errorCodeLookup.syncing') : t('errorCodeLookup.syncButton') }}</span>
            </button>
          </div>
        </div>
      </section>

      <!-- Status banner (sync success/failure) -->
      <section
        v-if="statusBanner"
        class="rounded-2xl border px-4 py-3 text-sm shadow-sm"
        :class="
          statusBanner.type === 'success'
            ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
            : 'border-rose-200 bg-rose-50 text-rose-700'
        "
      >
        {{ statusBanner.message }}
      </section>

      <!-- Empty state when no cache -->
      <section
        v-if="!meta.has_cache"
        class="flex flex-col items-center gap-4 rounded-[24px] border border-dashed border-slate-300 bg-white/70 py-16 text-center"
      >
        <FileSearch class="h-12 w-12 text-slate-400" />
        <p class="text-sm text-slate-600">{{ t('errorCodeLookup.empty.notSynced') }}</p>
        <button
          type="button"
          class="rounded-xl bg-indigo-500 px-5 py-2 text-sm font-semibold text-white shadow-sm hover:bg-indigo-600 disabled:opacity-60"
          :disabled="syncing"
          @click="onSync"
        >
          {{ t('errorCodeLookup.syncNowAction') }}
        </button>
      </section>

      <!-- Search + Table -->
      <template v-else>
        <section
          class="rounded-[24px] border border-slate-200 bg-white/90 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]"
        >
          <div class="flex items-center gap-3 text-sm text-slate-500">
            <span class="font-semibold text-slate-700">
              {{ t('errorCodeLookup.modeLabel') }}
            </span>
            <label
              v-for="m in (['single', 'range', 'keyword'] as ErrorCodeMode[])"
              :key="m"
              class="inline-flex items-center gap-2 cursor-pointer"
            >
              <input
                v-model="mode"
                type="radio"
                :value="m"
                class="text-indigo-600 focus:ring-indigo-500"
              />
              <span>{{ t(`errorCodeLookup.modes.${m}`) }}</span>
            </label>
          </div>
          <div class="mt-4 flex gap-3">
            <input
              v-model="inputValue"
              type="text"
              :placeholder="placeholder"
              class="flex-1 rounded-xl border bg-white px-4 py-2 text-sm text-slate-900 shadow-sm focus:outline-none focus:ring-2 focus:ring-indigo-500/30"
              :class="inputError ? 'border-red-400' : 'border-slate-200'"
              @keyup.enter="onSearch"
            />
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-slate-800 disabled:opacity-60"
              :disabled="submitting"
              @click="onSearch"
            >
              <Search class="h-4 w-4" />
              <span>{{ t('errorCodeLookup.searchButton') }}</span>
            </button>
          </div>
          <p v-if="inputError" class="mt-2 text-xs text-red-500">{{ inputError }}</p>
        </section>

        <section
          class="rounded-[24px] border border-slate-200 bg-white/95 shadow-[0_14px_40px_rgba(15,23,42,0.06)]"
        >
          <div v-if="entries.length === 0 && noResultMessage" class="px-5 py-12 text-center text-sm text-slate-500">
            {{ noResultMessage }}
          </div>
          <table v-else class="w-full table-fixed text-sm text-slate-700">
            <thead class="bg-slate-50 text-slate-600">
              <tr>
                <th class="w-[110px] px-4 py-3 text-left">{{ t('errorCodeLookup.columns.code') }}</th>
                <th class="px-4 py-3 text-left">{{ t('errorCodeLookup.columns.messageCn') }}</th>
                <th class="px-4 py-3 text-left">{{ t('errorCodeLookup.columns.messageEn') }}</th>
                <th class="w-[120px] px-4 py-3 text-left">{{ t('errorCodeLookup.columns.module') }}</th>
                <th class="px-4 py-3 text-left">{{ t('errorCodeLookup.columns.solution') }}</th>
                <th class="w-[160px] px-4 py-3 text-left">{{ t('errorCodeLookup.columns.remark') }}</th>
              </tr>
            </thead>
            <tbody>
              <template v-for="(entry, idx) in entries" :key="rowKey(entry, idx)">
                <tr
                  class="cursor-pointer border-t border-slate-100 hover:bg-slate-50"
                  @click="toggleExpand(entry, idx)"
                >
                  <td class="px-4 py-3 font-mono">
                    <span class="rounded bg-slate-100 px-2 py-1 text-xs text-slate-700">
                      {{ entry.code }}
                    </span>
                  </td>
                  <td class="truncate px-4 py-3" :title="entry.message_cn">
                    {{ entry.message_cn || '—' }}
                  </td>
                  <td class="truncate px-4 py-3" :title="entry.message_en">
                    {{ entry.message_en || '—' }}
                  </td>
                  <td class="px-4 py-3">
                    <span v-if="entry.module" class="rounded-full bg-slate-100 px-2 py-0.5 text-xs">
                      {{ entry.module }}
                    </span>
                    <span v-else class="text-slate-400">—</span>
                  </td>
                  <td class="truncate px-4 py-3 text-slate-600" :title="entry.solution">
                    {{ entry.solution || '—' }}
                  </td>
                  <td class="truncate px-4 py-3 text-slate-500" :title="entry.remark">
                    {{ entry.remark || '—' }}
                  </td>
                </tr>
                <tr v-if="expandedKey === rowKey(entry, idx)" class="bg-slate-50">
                  <td colspan="6" class="px-6 py-4">
                    <dl class="grid grid-cols-2 gap-x-6 gap-y-2 text-sm text-slate-700">
                      <div>
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.code') }}
                        </dt>
                        <dd class="font-mono">{{ entry.code }}</dd>
                      </div>
                      <div>
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.module') }}
                        </dt>
                        <dd>{{ entry.module || '—' }}</dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.messageCn') }}
                        </dt>
                        <dd class="whitespace-pre-wrap break-words">
                          {{ entry.message_cn || '—' }}
                        </dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.messageEn') }}
                        </dt>
                        <dd class="whitespace-pre-wrap break-words">
                          {{ entry.message_en || '—' }}
                        </dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.solution') }}
                        </dt>
                        <dd class="whitespace-pre-wrap break-words">
                          {{ entry.solution || '—' }}
                        </dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.remark') }}
                        </dt>
                        <dd class="whitespace-pre-wrap break-words">
                          {{ entry.remark || '—' }}
                        </dd>
                      </div>
                    </dl>
                  </td>
                </tr>
              </template>
            </tbody>
          </table>

          <div
            v-if="totalPages > 1"
            class="flex items-center justify-between gap-3 border-t border-slate-100 px-5 py-3 text-sm text-slate-600"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg border border-slate-200 px-3 py-1.5 disabled:opacity-50"
              :disabled="currentPage <= 1 || submitting"
              @click="changePage(-1)"
            >
              <ChevronLeft class="h-4 w-4" />
              {{ t('errorCodeLookup.pagination.prev') }}
            </button>
            <span>
              {{ t('errorCodeLookup.pagination.pageOf', { page: currentPage, total: totalPages }) }}
            </span>
            <div class="flex items-center gap-2">
              <span>{{ t('errorCodeLookup.pagination.jumpTo') }}</span>
              <input
                v-model="jumpInput"
                class="w-16 rounded-lg border border-slate-200 px-2 py-1 text-center"
                type="text"
                inputmode="numeric"
                @keyup.enter="onJump"
              />
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-lg border border-slate-200 px-3 py-1.5 disabled:opacity-50"
                :disabled="currentPage >= totalPages || submitting"
                @click="changePage(1)"
              >
                {{ t('errorCodeLookup.pagination.next') }}
                <ChevronRight class="h-4 w-4" />
              </button>
            </div>
          </div>
        </section>
      </template>
    </div>
  </div>
</template>
```

> **Note:** Sync feedback uses two channels:
> 1. `addLog(message, 'success' | 'error')` from `@/lib/store` writes a line to the MainConsole (consistent with `ManualCopyPage.vue`).
> 2. An inline `statusBanner` shows on the page itself so users get immediate feedback without leaving.

- [ ] **Step 2: Type-check**

```bash
pnpm check
```

Expected: passes. If type errors arise from `appStore.pushToast` not existing, switch the call site to the project's existing toast helper as noted above and re-run.

- [ ] **Step 3: Commit**

```bash
git add src/pages/ErrorCodeLookupPage.vue
git commit -m "feat(error-code): lookup page with mode switch, table, and pagination"
```

---

## Task 18: Sidebar nav entry + icon mapping

**Files:**
- Modify: `src/lib/sidebarNavigation.ts`
- Modify: `src/lib/sidebarNavigation.test.mjs`
- Modify: `src/components/Sidebar.vue`

- [ ] **Step 1: Update the test snapshot first (failing TDD)**

In `src/lib/sidebarNavigation.test.mjs`, find the array that asserts `toolPaths`. Append the new path so the assertion becomes:

```js
assert.deepEqual(toolPaths, [
  '/tools',
  '/tools/appliance-ssh',
  '/tools/framework-password',
  '/tools/code-statistics',
  '/tools/network',
  '/tools/screen-share',
  '/tools/file-share',
  '/tools/disk-cache-cleanup',
  '/tools/clipboard',
  '/tools/error-code-lookup',
]);
```

- [ ] **Step 2: Run the test to confirm failure**

```bash
node --test src/lib/sidebarNavigation.test.mjs
```

Expected: assertion failure (length mismatch).

- [ ] **Step 3: Add the icon key to the union type**

In `src/lib/sidebarNavigation.ts`, extend the `SidebarIconKey` type:

```ts
export type SidebarIconKey =
  | 'tasks'
  | 'console'
  | 'history'
  | 'settings'
  | 'toolsOverview'
  | 'frameworkPassword'
  | 'applianceSsh'
  | 'codeStatistics'
  | 'networkTools'
  | 'screenShare'
  | 'fileShare'
  | 'diskCacheCleanup'
  | 'clipboardManager'
  | 'errorCodeLookup';
```

- [ ] **Step 4: Append the nav item to the `tools` group**

Inside the `tools` section's `items` array (at the end), add:

```ts
      {
        key: 'error-code-lookup',
        labelKey: 'sidebar.errorCodeLookup',
        path: '/tools/error-code-lookup',
        iconKey: 'errorCodeLookup',
        matchMode: 'prefix',
      },
```

- [ ] **Step 5: Map the icon in `Sidebar.vue`**

Open `src/components/Sidebar.vue`. Search for the existing icon mapping (where `clipboardManager` maps to `Clipboard`). Add an import (alongside other lucide imports at the top of `<script setup>`):

```ts
import { FileSearch } from 'lucide-vue-next';
```

(Skip if already imported.)

In the icon-map object, add:

```ts
  errorCodeLookup: FileSearch,
```

- [ ] **Step 6: Re-run the test**

```bash
node --test src/lib/sidebarNavigation.test.mjs
```

Expected: passes.

- [ ] **Step 7: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 8: Commit**

```bash
git add src/lib/sidebarNavigation.ts src/lib/sidebarNavigation.test.mjs src/components/Sidebar.vue
git commit -m "feat(error-code): sidebar nav entry with FileSearch icon"
```

---

## Task 19: ToolsHub card + router route

**Files:**
- Modify: `src/pages/ToolsHubPage.vue`
- Modify: `src/router/index.ts`

- [ ] **Step 1: Add ToolsHub card**

Open `src/pages/ToolsHubPage.vue`. In the `<script setup>`, extend the lucide import line to include `FileSearch`:

```ts
import { ArrowRight, BarChart3, Clipboard, FileSearch, Globe, HardDrive, KeyRound, MonitorUp, Share2, Shield, type LucideIcon } from 'lucide-vue-next';
```

In the `toolCards` computed array, append a new entry **after** the `clipboard-manager` block:

```ts
    {
      key: 'error-code-lookup',
      titleKey: 'sidebar.errorCodeLookup',
      descriptionKey: 'toolsHub.cards.errorCodeLookup.description',
      path: '/tools/error-code-lookup',
      icon: markRaw(FileSearch as LucideIcon),
      iconClasses: 'from-indigo-500 to-blue-600 shadow-indigo-500/20',
      chipKey: 'toolsHub.cards.errorCodeLookup.chip',
    },
```

(No need to add the new icon to the decorative icon strip in the header section — it's purely cosmetic.)

- [ ] **Step 2: Add the route**

Open `src/router/index.ts`. Append a new route entry **after** the existing `/tools/clipboard` route (and before `/clipboard-panel` if present):

```ts
  {
    path: '/tools/error-code-lookup',
    component: () => import('../pages/ErrorCodeLookupPage.vue'),
  },
```

- [ ] **Step 3: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 4: Smoke launch (optional but recommended)**

```bash
pnpm dev
```

Open the served URL in a browser. Click the "错误码查询" sidebar item. Confirm:
- Page renders without console errors.
- Empty state shows "尚未同步" + "立即同步" button.

Stop the dev server (Ctrl+C).

- [ ] **Step 5: Commit**

```bash
git add src/pages/ToolsHubPage.vue src/router/index.ts
git commit -m "feat(error-code): tools hub card and router route"
```

---

## Task 20: Final verification

**Files:**
- None (verification only)

- [ ] **Step 1: Run full backend test suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app
```

Expected: all tests pass (no skips, no failures).

- [ ] **Step 2: Run all frontend node tests**

```bash
node --test src/lib/sidebarNavigation.test.mjs src/pages/errorCodeLookup/validation.test.mjs
```

Expected: all pass.

- [ ] **Step 3: Run formatter / clippy**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Expected: no diff from `cargo fmt`, clippy clean for the `error_code` module. Pre-existing warnings outside `error_code/` may be visible — leave them.

- [ ] **Step 4: Type-check + lint**

```bash
pnpm check
pnpm lint
```

Expected: both clean.

- [ ] **Step 5: Versioned production build**

```bash
cmd /c pnpm tauri:build:versioned-exe
```

Expected: build succeeds; produces `file-sync-tool-1.0.0-YYYYMMDDHHmm.exe` (or `1.0.7` per current Cargo.toml — match the package version).

- [ ] **Step 6: Manual QA checklist**

Launch the produced exe (or `pnpm tauri dev`) and perform every item in §8 of the spec:

```
[ ] Click sidebar "错误码查询" — page opens, shows empty state.
[ ] Click 立即同步 — table populates, toast shows success.
[ ] Disconnect network, click 同步 — toast shows network failure, old data intact.
[ ] Single mode: input "110", click 查询 — see 相机编码不存在 / camera code not exist.
[ ] Range mode: input "300000-301000" — paginated list visible.
[ ] Range mode: input "0-100000" — input shows "范围跨度不能超过 1000".
[ ] Keyword mode: input "相机" — entries containing 相机 appear.
[ ] Click a row — detail panel expands; click again — collapses.
[ ] Switch language to English — all i18n strings render.
[ ] Restart app — page loads instantly from cache (no fresh sync triggered).
```

- [ ] **Step 7: Commit any final formatting fixes (if any)**

If `cargo fmt` produced a diff or other small fixes were applied during QA:

```bash
git add -A
git commit -m "chore(error-code): final formatting and qa fixes"
```

If nothing changed, skip this step.

---

## Out of scope (do NOT implement here)

Per spec §9, these are explicitly deferred:
- Configurable GitLab URL/credentials.
- Incremental fetch via `last_commit_id`.
- Offline diff between dictionary versions.
- Copy-to-clipboard buttons per row.
- Default page sort options other than ascending by code.
- Auto-sync at app startup or after N days.

Do not add them in this plan even if they look quick.
