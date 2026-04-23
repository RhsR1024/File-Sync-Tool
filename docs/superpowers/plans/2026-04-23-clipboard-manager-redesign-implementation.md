# Clipboard Manager 重设计 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一 Alt+C 快捷面板边框、把 `/tools/clipboard` 收敛为设置专用页，并增加“自复制后是否回写为最新记录”的可配置行为与“本工具”来源标记。

**Architecture:** 保留 Alt+C 面板作为唯一的剪贴板 CRUD / 检索 / 批量操作入口，工具页仅保留设置容器。Rust 后端通过 `ClipboardState` 上的“自写旁路”标记识别由本工具发起的系统剪贴板写入，再按设置决定跳过捕获还是写回历史；前端把 `from_self` 贯通到类型、列表来源渲染和设置 UI。

**Tech Stack:** Tauri 2.x · Rust 2021 · rusqlite · arboard / clipboard-master · Vue 3 `<script setup>` · TypeScript · vue-i18n · Tailwind 4 · lucide-vue-next · node:test

**Spec:** [docs/superpowers/specs/2026-04-23-clipboard-manager-redesign-design.md](../specs/2026-04-23-clipboard-manager-redesign-design.md)

**Execution Branch:** `main` — 按用户明确要求，直接在 `main` 上实现，不新建工作树或功能分支。

---

## Preflight

- 当前 `main` 工作区里仍有未提交的硬盘缓存清理收尾改动：`src/lib/tauri.ts`、`src/pages/DiskCacheCleanupPage.vue`。
- 在开始本计划任何代码任务前，先把这两处现有改动提交到 `main`，确保剪贴板改造从干净工作区开始；不要把两条需求线揉进同一个提交。

## File Map

- `src/pages/ClipboardPanelPage.vue` — Alt+C 面板外壳、header、顶部与内容区分割。
- `src/pages/ClipboardManagerPage.vue` — `/tools/clipboard` 页面；改造成仅承载说明头部 + `ClipboardSettingsPanel`。
- `src/components/clipboard-settings/GeneralTab.vue` — 新增 `reinsert_on_self_copy` 设置开关。
- `src/components/clipboard/ClipboardList.vue` — 条目来源区域；优先显示“本工具”徽章。
- `src/components/clipboard/ClipboardSettingsPanel.vue` — 结构保持不变，仅承载更新后的 tabs 内容。
- `src/components/clipboard/ClipboardStats.vue` — 只被工具页使用；页面精简后应删除。
- `src/components/clipboard/ClipboardGroupSidebar.vue` — 只被工具页使用；页面精简后应删除。
- `src/lib/clipboardListPresentation.ts` — 可测试的来源呈现 helper；新增“本工具”分支。
- `src/lib/clipboardListPresentation.test.mjs` — 来源呈现 helper 的纯函数测试。
- `src/lib/clipboardTypes.ts` — `ClipboardSettings` / `ClipboardItem` 前端契约字段。
- `src/lib/clipboardTypes.contract.test.ts` — TS 合同样例；约束新增字段。
- `src/locales/messages.ts` — 工具页新描述、General tab 开关文案、“本工具”来源文案。
- `src-tauri/src/clipboard/mod.rs` — `ClipboardState`；新增 `pending_self_write` 状态。
- `src-tauri/src/clipboard/models.rs` — `ClipboardSettings` / `ClipboardItem` 模型字段与默认值。
- `src-tauri/src/clipboard/db.rs` — `clipboard_items.from_self` 列、schema 升级、select / insert / dedup 更新。
- `src-tauri/src/clipboard/data_transfer.rs` — 导入导出行映射；带上 `from_self`。
- `src-tauri/src/clipboard/paste.rs` — 写入系统剪贴板前记录“自写 hash”。
- `src-tauri/src/clipboard/watcher.rs` — 识别自写事件并决定跳过 / 作为 `from_self` 写入。
- `src-tauri/src/clipboard/commands.rs` — 更新 paste/copy 相关调用签名，把 `ClipboardState` 传给 `paste.rs`。

## Task 1: 统一 Alt+C 面板四边边框

**Files:**
- Modify: `src/pages/ClipboardPanelPage.vue`

- [ ] **Step 1: 调整面板根容器和 header / body 分隔实现**

```vue
<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden rounded-xl border border-slate-200 bg-white">
    <header
      class="flex select-none items-center justify-between px-3 py-2.5"
      :data-tauri-drag-region="CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION ? '' : undefined"
      @mousedown="onHeaderMouseDown"
    >
      <span class="pointer-events-none truncate text-sm font-semibold text-slate-700">
        {{ t('clipboard.tool.title') }}
      </span>
      <!-- toolbar + close button 保持原样 -->
    </header>

    <div class="flex min-h-0 flex-1 flex-col overflow-hidden border-t border-slate-100">
      <!-- 原有 batch bar / search / list / footer 全部保留 -->
    </div>
  </div>
</template>
```

- [ ] **Step 2: 运行类型检查，确认纯样式改动没有破坏页面编译**

Run: `pnpm check`

Expected: PASS，`ClipboardPanelPage.vue` 无模板或类型错误。

- [ ] **Step 3: 运行桌面构建，确认窗口壳层改动不影响打包**

Run: `pnpm tauri:build:versioned-exe`

Expected: PASS，产出新的 versioned exe。

- [ ] **Step 4: 手测 Alt+C 面板四边边框**

Manual checklist:

```text
1. 打开 Alt+C 面板，观察四边是否同色同粗细。
2. 顶部 header 下方只保留一条浅色内分隔线，不出现双线。
3. 在浅色 / 深色桌面背景下检查是否仍有系统残留线透出。
```

- [ ] **Step 5: 提交 WI-1**

```bash
git add src/pages/ClipboardPanelPage.vue
git commit -m "fix(clipboard): unify quick panel frame border"
```

## Task 2: 补齐 `reinsert_on_self_copy` / `from_self` 的模型、数据库与契约

**Files:**
- Modify: `src-tauri/src/clipboard/models.rs`
- Modify: `src-tauri/src/clipboard/db.rs`
- Modify: `src-tauri/src/clipboard/data_transfer.rs`
- Modify: `src/lib/clipboardTypes.ts`
- Modify: `src/lib/clipboardTypes.contract.test.ts`

- [ ] **Step 1: 先写会失败的模型 / 迁移 / 合同测试**

```rust
// src-tauri/src/clipboard/models.rs
#[test]
fn clipboard_settings_default_disables_self_reinsert() {
    let settings = ClipboardSettings::default();
    assert!(!settings.reinsert_on_self_copy);
}
```

```rust
// src-tauri/src/clipboard/db.rs
#[test]
fn migrate_adds_from_self_column_with_zero_default_for_existing_rows() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO schema_meta(key, value) VALUES ('version', '2');
        CREATE TABLE clipboard_groups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            sort_index INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE clipboard_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            content_preview TEXT NOT NULL,
            content_full TEXT,
            rtf_content TEXT,
            html TEXT,
            image_path TEXT,
            image_width INTEGER,
            image_height INTEGER,
            file_paths_json TEXT,
            byte_size INTEGER NOT NULL DEFAULT 0,
            char_count INTEGER NOT NULL DEFAULT 0,
            hash TEXT NOT NULL UNIQUE,
            source_app TEXT,
            source_app_icon TEXT,
            group_id INTEGER,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            favorite_sort_index INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        INSERT INTO clipboard_items(kind, content_preview, byte_size, char_count, hash, created_at, updated_at)
        VALUES ('text', 'legacy', 6, 6, 'legacy-hash', 1, 1);
        "#,
    ).unwrap();

    migrate(&conn).unwrap();

    let from_self: i64 = conn.query_row(
        "SELECT from_self FROM clipboard_items WHERE hash = 'legacy-hash'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(from_self, 0);
}
```

```rust
// src-tauri/src/clipboard/data_transfer.rs
#[test]
fn replace_import_preserves_from_self_flag() {
    let export_dir = TempDir::new().unwrap();
    let export_db_path = export_dir.path().join("clipboard.db");
    let export_conn = db::open(&export_db_path).unwrap();

    let mut item = sample_text_item("self-row");
    item.from_self = true;
    db::insert_item(&export_conn, &item).unwrap();

    let archive_path = export_dir.path().join("clipboard-export.zip");
    export_bundle(
        &export_db_path,
        &export_dir.path().join("clipboard_images"),
        &export_dir.path().join("clipboard_icons"),
        &archive_path,
        false,
    ).unwrap();

    let import_dir = TempDir::new().unwrap();
    let import_db_path = import_dir.path().join("clipboard.db");
    let import_conn = db::open(&import_db_path).unwrap();

    import_bundle(
        &import_conn,
        &import_db_path,
        &import_dir.path().join("clipboard_images"),
        &import_dir.path().join("clipboard_icons"),
        &archive_path,
        ImportMode::Replace,
    ).unwrap();

    let imported = list_all_items(&import_conn);
    assert_eq!(imported[0].hash, "self-row");
    assert!(imported[0].from_self);
}
```

```ts
// src/lib/clipboardTypes.contract.test.ts
export const clipboardSettingsContract: ClipboardSettings = {
  // ...existing fields
  reinsert_on_self_copy: false,
};

export const clipboardItemContract: ClipboardItem = {
  // ...existing fields
  from_self: false,
};
```

- [ ] **Step 2: 跑测试确认当前缺口就是新增字段 / 列还没接通**

Run: `cargo test clipboard --manifest-path src-tauri/Cargo.toml`

Expected: FAIL，报 `reinsert_on_self_copy` / `from_self` 字段不存在、或 `from_self` 列缺失。

Run: `pnpm check`

Expected: FAIL，报 `ClipboardSettings` / `ClipboardItem` 类型缺少新增字段。

- [ ] **Step 3: 以最小实现补齐 Rust / TS 契约和数据库升级**

```rust
// src-tauri/src/clipboard/models.rs
pub struct ClipboardSettings {
    pub enabled: bool,
    pub hotkey: String,
    pub max_items: u32,
    pub retain_days: u32,
    pub max_item_bytes: u64,
    pub preview_delay_ms: u32,
    pub enable_text_preview: bool,
    pub use_win_v_replacement: bool,
    pub run_as_admin: bool,
    pub show_startup_notification: bool,
    pub dedup_strategy: ClipboardDedupStrategy,
    pub reinsert_on_self_copy: bool,
    #[serde(default)]
    pub display: ClipboardDisplaySettings,
    // ...
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: "Alt+C".to_string(),
            max_items: 1000,
            retain_days: 30,
            max_item_bytes: 10 * 1024 * 1024,
            preview_delay_ms: 500,
            enable_text_preview: false,
            use_win_v_replacement: false,
            run_as_admin: false,
            show_startup_notification: true,
            dedup_strategy: ClipboardDedupStrategy::MoveToTop,
            reinsert_on_self_copy: false,
            display: ClipboardDisplaySettings::default(),
            // ...
        }
    }
}

pub struct ClipboardItem {
    pub id: i64,
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub rtf_content: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub char_count: i64,
    pub hash: String,
    pub source_app: Option<String>,
    pub source_app_icon: Option<String>,
    pub from_self: bool,
    pub group_id: Option<i64>,
    pub is_favorite: bool,
    pub is_pinned: bool,
    pub favorite_sort_index: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
```

```rust
// src-tauri/src/clipboard/db.rs
const CLIPBOARD_SCHEMA_VERSION: i64 = 3;

fn migrate(conn: &Connection) -> SqlResult<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(/* existing schema_meta bootstrap */)?;
    ensure_clipboard_groups_table(conn)?;

    let version = read_schema_version(conn)?;
    let needs_rebuild = clipboard_items_needs_rebuild(conn)?;
    if version < 2 || needs_rebuild {
        migrate_clipboard_items_v2(conn)?;
        set_schema_version(conn, 2)?;
    }

    if version < 3 && !table_has_column(conn, "clipboard_items", "from_self")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN from_self INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if version < CLIPBOARD_SCHEMA_VERSION {
        set_schema_version(conn, CLIPBOARD_SCHEMA_VERSION)?;
    }

    ensure_clipboard_indexes(conn)?;
    Ok(())
}

pub struct NewItem {
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub rtf_content: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub hash: String,
    pub source_app: Option<String>,
    pub source_app_icon: Option<String>,
    pub from_self: bool,
}
```

```rust
// src-tauri/src/clipboard/data_transfer.rs
struct ImportedItem {
    kind: ContentKind,
    content_preview: String,
    content_full: Option<String>,
    rtf_content: Option<String>,
    html: Option<String>,
    image_path: Option<String>,
    image_width: Option<u32>,
    image_height: Option<u32>,
    file_paths: Option<Vec<String>>,
    byte_size: i64,
    char_count: i64,
    hash: String,
    source_app: Option<String>,
    source_app_icon: Option<String>,
    from_self: bool,
    group_id: Option<i64>,
    is_favorite: bool,
    is_pinned: bool,
    favorite_sort_index: Option<i64>,
    created_at: i64,
    updated_at: i64,
}
```

```ts
// src/lib/clipboardTypes.ts
export interface ClipboardSettings {
  enabled: boolean;
  hotkey: string;
  max_items: number;
  retain_days: number;
  max_item_bytes: number;
  preview_delay_ms: number;
  enable_text_preview: boolean;
  use_win_v_replacement: boolean;
  run_as_admin: boolean;
  show_startup_notification: boolean;
  dedup_strategy: ClipboardDedupStrategy;
  reinsert_on_self_copy: boolean;
  display: ClipboardDisplaySettings;
  preview: ClipboardPreviewSettings;
  panel: ClipboardPanelSettings;
  shortcuts: ClipboardShortcutsSettings;
  navigation: ClipboardNavigationSettings;
  toolbar: ClipboardToolbarSettings;
  data: ClipboardDataSettings;
  app_filter: ClipboardAppFilterSettings;
}

export interface ClipboardItem {
  id: number;
  kind: ClipboardContentKind;
  content_preview: string;
  content_full: string | null;
  rtf_content: string | null;
  html: string | null;
  image_path: string | null;
  image_width: number | null;
  image_height: number | null;
  file_paths: string[] | null;
  byte_size: number;
  char_count: number;
  hash: string;
  source_app: string | null;
  source_app_icon: string | null;
  from_self: boolean;
  group_id: number | null;
  is_favorite: boolean;
  is_pinned: boolean;
  favorite_sort_index: number | null;
  created_at: number;
  updated_at: number;
}
```

- [ ] **Step 4: 重新运行契约与数据库测试**

Run: `cargo test clipboard --manifest-path src-tauri/Cargo.toml`

Expected: PASS，新增模型 / DB / data_transfer 测试全部通过。

Run: `pnpm check`

Expected: PASS，TS 合同样例与前端类型全部通过。

- [ ] **Step 5: 提交契约与存储层**

```bash
git add src-tauri/src/clipboard/models.rs src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/data_transfer.rs src/lib/clipboardTypes.ts src/lib/clipboardTypes.contract.test.ts
git commit -m "feat(clipboard): add self-copy settings and item source contract"
```

## Task 3: 为 copy / paste / watcher 加入自写旁路

**Files:**
- Modify: `src-tauri/src/clipboard/mod.rs`
- Modify: `src-tauri/src/clipboard/paste.rs`
- Modify: `src-tauri/src/clipboard/watcher.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`

- [ ] **Step 1: 先写会失败的纯函数测试，锁定“自写 hash + 超时判定”行为**

```rust
// src-tauri/src/clipboard/paste.rs
#[test]
fn clipboard_write_hash_matches_plain_text_payload_for_file_path_paste() {
    let mut item = sample_item(ContentKind::File);
    item.file_paths = Some(vec!["C:\\demo\\a.txt".into(), "C:\\demo\\b.txt".into()]);
    item.content_preview = "ignored".into();
    item.content_full = Some("ignored".into());

    assert_eq!(
        clipboard_write_hash(&item, true).unwrap(),
        hex(&compute_hash(b"text", b"C:\\demo\\a.txt\nC:\\demo\\b.txt")),
    );
}
```

```rust
// src-tauri/src/clipboard/watcher.rs
#[test]
fn resolve_self_write_match_skips_capture_when_setting_is_disabled() {
    let now = std::time::Instant::now();
    let decision = resolve_self_write_match(
        Some(("same-hash".to_string(), now)),
        "same-hash",
        false,
        now,
    );
    assert_eq!(decision, SelfWriteDecision::Skip);
}

#[test]
fn resolve_self_write_match_marks_capture_as_self_when_setting_is_enabled() {
    let now = std::time::Instant::now();
    let decision = resolve_self_write_match(
        Some(("same-hash".to_string(), now)),
        "same-hash",
        true,
        now,
    );
    assert_eq!(decision, SelfWriteDecision::CaptureAsSelf);
}

#[test]
fn resolve_self_write_match_ignores_stale_marker() {
    let now = std::time::Instant::now();
    let decision = resolve_self_write_match(
        Some(("same-hash".to_string(), now - std::time::Duration::from_millis(900))),
        "same-hash",
        true,
        now,
    );
    assert_eq!(decision, SelfWriteDecision::None);
}
```

- [ ] **Step 2: 运行 Rust 测试，确认当前失败点在新 helper / 状态还没实现**

Run: `cargo test clipboard --manifest-path src-tauri/Cargo.toml`

Expected: FAIL，报 `clipboard_write_hash` / `resolve_self_write_match` / `SelfWriteDecision` 未定义，或 `pending_self_write` 缺失。

- [ ] **Step 3: 以可测试 helper 的方式实现自写旁路，并把 state 传入 paste API**

```rust
// src-tauri/src/clipboard/mod.rs
pub struct ClipboardState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub read_db: Arc<Mutex<rusqlite::Connection>>,
    pub write_db: Arc<Mutex<rusqlite::Connection>>,
    pub db_path: PathBuf,
    pub image_dir: PathBuf,
    pub icon_dir: PathBuf,
    pub is_enabled: AtomicBool,
    pub panel_pinned: AtomicBool,
    pub settings: Arc<RwLock<ClipboardSettings>>,
    pub pending_self_write: Mutex<Option<(String, std::time::Instant)>>,
    pub watcher_handle: Mutex<Option<watcher::WatcherHandle>>,
    pub hotkey_handle: Mutex<Option<hotkey::HotkeyHandle>>,
}
```

```rust
// src-tauri/src/clipboard/paste.rs
pub fn paste_item(
    app: &AppHandle,
    clipboard: &crate::clipboard::ClipboardState,
    item: &ClipboardItem,
    plain_text: bool,
) -> Result<(), String> {
    write_to_clipboard(clipboard, item, plain_text)?;
    finish_paste(app)
}

pub fn copy_item(
    clipboard: &crate::clipboard::ClipboardState,
    item: &ClipboardItem,
) -> Result<(), String> {
    write_to_clipboard(clipboard, item, false)
}

fn write_to_clipboard(
    clipboard: &crate::clipboard::ClipboardState,
    item: &ClipboardItem,
    plain_text: bool,
) -> Result<(), String> {
    let hash = clipboard_write_hash(item, plain_text)?;
    *clipboard.pending_self_write.lock() = Some((hash, std::time::Instant::now()));

    match item.kind {
        ContentKind::Text => write_text_to_clipboard(preferred_text(item))?,
        ContentKind::Html => { /* existing branch */ }
        ContentKind::Rtf => { /* existing branch */ }
        ContentKind::Image => { /* existing branch */ }
        ContentKind::File => { /* existing branch */ }
    }

    Ok(())
}
```

```rust
// src-tauri/src/clipboard/watcher.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfWriteDecision {
    None,
    Skip,
    CaptureAsSelf,
}

fn resolve_self_write_match(
    pending: Option<(String, std::time::Instant)>,
    captured_hash: &str,
    reinsert_on_self_copy: bool,
    now: std::time::Instant,
) -> SelfWriteDecision {
    let Some((pending_hash, created_at)) = pending else {
        return SelfWriteDecision::None;
    };
    if now.duration_since(created_at) > std::time::Duration::from_millis(500) {
        return SelfWriteDecision::None;
    }
    if pending_hash != captured_hash {
        return SelfWriteDecision::None;
    }
    if reinsert_on_self_copy {
        SelfWriteDecision::CaptureAsSelf
    } else {
        SelfWriteDecision::Skip
    }
}

fn try_capture(app: &AppHandle, state: &ClipboardState) -> Result<(), String> {
    // ... capture snapshot and hash_hex first
    let self_write_decision = {
        let pending = state.pending_self_write.lock().take();
        let settings = state.settings.read();
        resolve_self_write_match(
            pending,
            &hash_hex,
            settings.reinsert_on_self_copy,
            std::time::Instant::now(),
        )
    };

    if matches!(self_write_decision, SelfWriteDecision::Skip) {
        return Ok(());
    }

    let source_capture = if matches!(self_write_decision, SelfWriteDecision::CaptureAsSelf) {
        CaptureSource {
            source_app: None,
            source_app_icon: None,
        }
    } else {
        build_source_capture(state, source_info)
    };

    let mut item = /* existing build_*_item branch */;
    if matches!(self_write_decision, SelfWriteDecision::CaptureAsSelf) {
        item.from_self = true;
    }

    upsert_item(state, item)?;
    notify_added(app);
    Ok(())
}
```

```rust
// src-tauri/src/clipboard/commands.rs
crate::clipboard::paste::paste_item(&app, state.clipboard.as_ref(), &item, false)?;
crate::clipboard::paste::paste_item(&app, state.clipboard.as_ref(), &item, true)?;
crate::clipboard::paste::copy_item(state.clipboard.as_ref(), &item)?;
crate::clipboard::paste::paste_file_item(&app, state.clipboard.as_ref(), &item)?;
crate::clipboard::paste::paste_file_paths_as_text(&app, state.clipboard.as_ref(), &item)?;
crate::clipboard::paste::paste_text(&app, state.clipboard.as_ref(), &merged)?;
```

- [ ] **Step 4: 跑完整 clipboard Rust 测试，确认旁路逻辑和旧路径都不回归**

Run: `cargo test clipboard --manifest-path src-tauri/Cargo.toml`

Expected: PASS，`paste.rs` / `watcher.rs` / `commands.rs` 以及原有 dedup / copy / paste 测试全部通过。

- [ ] **Step 5: 提交 WI-3 的行为层**

```bash
git add src-tauri/src/clipboard/mod.rs src-tauri/src/clipboard/paste.rs src-tauri/src/clipboard/watcher.rs src-tauri/src/clipboard/commands.rs
git commit -m "feat(clipboard): detect self-copy writes before watcher capture"
```

## Task 4: 把“本工具”来源徽章和开关暴露到前端

**Files:**
- Modify: `src/components/clipboard-settings/GeneralTab.vue`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src/lib/clipboardListPresentation.ts`
- Modify: `src/lib/clipboardListPresentation.test.mjs`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: 先写会失败的来源呈现 helper 测试**

```js
// src/lib/clipboardListPresentation.test.mjs
import {
  resolveClipboardSourceBadge,
} from './clipboardListPresentation.ts';

test('resolveClipboardSourceBadge prefers the self-tool badge over app metadata', () => {
  assert.deepEqual(
    resolveClipboardSourceBadge('both', {
      from_self: true,
      source_app: 'Code',
      source_app_icon: 'C:/icons/code.png',
    }),
    {
      kind: 'self',
      showIcon: true,
      showName: true,
      label: 'This tool',
    },
  );
});
```

- [ ] **Step 2: 运行 node 测试，确认缺口就在新 helper / 新文案分支**

Run: `node --test src/lib/clipboardListPresentation.test.mjs`

Expected: FAIL，报 `resolveClipboardSourceBadge` 未定义，或返回结构不匹配。

- [ ] **Step 3: 实现 helper、列表徽章、General tab 开关和 i18n**

```ts
// src/lib/clipboardListPresentation.ts
export interface ClipboardSourceBadge {
  kind: 'none' | 'app' | 'self';
  showIcon: boolean;
  showName: boolean;
  label: string | null;
}

export function resolveClipboardSourceBadge(
  mode: ClipboardSourceAppDisplay,
  item: {
    from_self: boolean;
    source_app: string | null;
    source_app_icon: string | null;
  },
): ClipboardSourceBadge {
  if (item.from_self) {
    return {
      kind: 'self',
      showIcon: true,
      showName: true,
      label: 'This tool',
    };
  }

  const presentation = resolveSourceAppPresentation(mode, item.source_app, item.source_app_icon);
  if (!presentation.showIcon && !presentation.showName) {
    return { kind: 'none', showIcon: false, showName: false, label: null };
  }

  return {
    kind: 'app',
    showIcon: presentation.showIcon,
    showName: presentation.showName,
    label: item.source_app,
  };
}
```

```vue
<!-- src/components/clipboard/ClipboardList.vue -->
<script setup lang="ts">
import { Package } from 'lucide-vue-next';
import { resolveClipboardSourceBadge } from '@/lib/clipboardListPresentation';
</script>

<template>
  <div class="flex shrink-0 items-center gap-1.5">
    <span
      v-if="resolveClipboardSourceBadge(displaySettings.show_source_app, item).kind === 'self'"
      class="inline-flex items-center gap-1 rounded-full bg-slate-100 px-2 py-0.5 text-[11px] font-medium text-slate-700"
    >
      <Package class="h-3 w-3" />
      {{ t('clipboard.source.self') }}
    </span>
    <span
      v-else-if="resolveClipboardSourceBadge(displaySettings.show_source_app, item).kind === 'app'"
      class="flex items-center gap-1 text-slate-500"
    >
      <ClipboardAppIcon
        v-if="resolveClipboardSourceBadge(displaySettings.show_source_app, item).showIcon"
        :icon-path="item.source_app_icon"
        :source-app="item.source_app"
      />
      <span
        v-if="resolveClipboardSourceBadge(displaySettings.show_source_app, item).showName"
        class="max-w-[96px] truncate"
      >{{ item.source_app }}</span>
    </span>
  </div>
</template>
```

```vue
<!-- src/components/clipboard-settings/GeneralTab.vue -->
<label class="flex items-start justify-between gap-4">
  <div>
    <div class="text-sm text-slate-700">
      {{ t('clipboard.settings.general.reinsertOnSelfCopy') }}
    </div>
    <div class="mt-1 text-xs leading-5 text-slate-500">
      {{ t('clipboard.settings.general.reinsertOnSelfCopyHint') }}
    </div>
  </div>
  <input
    type="checkbox"
    :checked="props.settings.reinsert_on_self_copy"
    @change="patch({ reinsert_on_self_copy: ($event.target as HTMLInputElement).checked })"
  >
</label>
```

```ts
// src/locales/messages.ts
clipboard: {
  tool: {
    description: 'Press Alt+C to open the clipboard quick panel. Use this page to configure clipboard behavior.',
  },
  source: {
    self: 'This tool',
  },
  settings: {
    general: {
      reinsertOnSelfCopy: 'Re-insert as newest record after self-copy',
      reinsertOnSelfCopyHint: 'Off: clicking to copy in the quick panel does not change history order. On: the item moves to the top and is tagged as "This tool".',
    },
  },
}
```

```ts
// zh messages
clipboard: {
  tool: {
    description: '打开 Alt+C 使用剪贴板快捷面板；此页面仅配置剪贴板行为。',
  },
  source: {
    self: '本工具',
  },
  settings: {
    general: {
      reinsertOnSelfCopy: '复制后回写为最新记录',
      reinsertOnSelfCopyHint: '关闭时：在快捷面板复制条目不改变历史顺序；开启时：会把该条目移到顶部，并把来源标记为“本工具”。',
    },
  },
}
```

- [ ] **Step 4: 重新运行前端测试与类型检查**

Run: `node --test src/lib/clipboardListPresentation.test.mjs`

Expected: PASS，新增“本工具”来源分支通过。

Run: `pnpm check`

Expected: PASS，`ClipboardList.vue` / `GeneralTab.vue` / i18n 修改全部通过。

Run: `pnpm exec eslint src/components/clipboard/ClipboardList.vue src/components/clipboard-settings/GeneralTab.vue src/lib/clipboardListPresentation.ts src/locales/messages.ts`

Expected: PASS，无新增 ESLint 报错。

- [ ] **Step 5: 手测自复制开关**

Manual checklist:

```text
1. 默认关闭时，在 Alt+C 点击“复制”后，原条目不跳到顶部。
2. 开启后，在 Alt+C 点击“复制”后，条目回到顶部或按 dedup 规则更新。
3. 开启后，该条目来源显示“本工具”而不是随机前台应用名。
4. 外部应用复制时，来源仍显示真实 source_app。
```

- [ ] **Step 6: 提交前端来源与设置层**

```bash
git add src/components/clipboard-settings/GeneralTab.vue src/components/clipboard/ClipboardList.vue src/lib/clipboardListPresentation.ts src/lib/clipboardListPresentation.test.mjs src/locales/messages.ts
git commit -m "feat(clipboard): expose self-copy source tag and toggle"
```

## Task 5: 把 `/tools/clipboard` 收敛为设置专用页，并删掉页面专用死代码

**Files:**
- Modify: `src/pages/ClipboardManagerPage.vue`
- Delete: `src/components/clipboard/ClipboardStats.vue`
- Delete: `src/components/clipboard/ClipboardGroupSidebar.vue`

- [ ] **Step 1: 直接把工具页改写为“页头 + 设置面板”**

```vue
<!-- src/pages/ClipboardManagerPage.vue -->
<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import ClipboardSettingsPanel from '@/components/clipboard/ClipboardSettingsPanel.vue';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-6 pb-10">
      <header class="space-y-2">
        <h1 class="text-2xl font-bold tracking-tight text-slate-950">
          {{ t('clipboard.tool.title') }}
        </h1>
        <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      </header>

      <ClipboardSettingsPanel />
    </div>
  </div>
</template>
```

- [ ] **Step 2: 删除只剩工具页在用的组件文件**

```text
Delete: src/components/clipboard/ClipboardStats.vue
Delete: src/components/clipboard/ClipboardGroupSidebar.vue
```

- [ ] **Step 3: 搜索确认未误删面板仍在使用的组件**

Run: `rg -n "ClipboardStats.vue|ClipboardGroupSidebar.vue|ClipboardStats|ClipboardGroupSidebar" src`

Expected: `ClipboardStats.vue` / `ClipboardGroupSidebar.vue` 不再被页面引用；`ClipboardCardMenu`、`ClipboardFileDetailsDialog`、`ClipboardMergePasteDialog` 等仍保留给 Alt+C 面板使用。

- [ ] **Step 4: 运行类型检查和构建**

Run: `pnpm check`

Expected: PASS，`ClipboardManagerPage.vue` 改造后没有遗留未使用导入或模板错误。

Run: `pnpm tauri:build:versioned-exe`

Expected: PASS，工具页大幅删除后桌面端仍能构建。

- [ ] **Step 5: 手测 `/tools/clipboard` 与 Alt+C 分工**

Manual checklist:

```text
1. 打开 /tools/clipboard，只剩说明头部 + 设置面板。
2. 页面中不再出现搜索、筛选、分组侧栏、列表、批量栏。
3. Data tab 仍能看到统计信息。
4. Alt+C 面板仍保留搜索、筛选、分组、列表、批量、右键菜单。
```

- [ ] **Step 6: 提交 WI-2**

```bash
git add src/pages/ClipboardManagerPage.vue
git add -u src/components/clipboard/ClipboardStats.vue src/components/clipboard/ClipboardGroupSidebar.vue
git commit -m "refactor(clipboard): turn tools page into settings-only view"
```

## Task 6: 在 `main` 上完成最终验证

**Files:**
- Verify only: `src/pages/ClipboardPanelPage.vue`
- Verify only: `src/pages/ClipboardManagerPage.vue`
- Verify only: `src/components/clipboard-settings/GeneralTab.vue`
- Verify only: `src/components/clipboard/ClipboardList.vue`
- Verify only: `src/lib/clipboardListPresentation.ts`
- Verify only: `src/locales/messages.ts`
- Verify only: `src-tauri/src/clipboard/*.rs`

- [ ] **Step 1: 跑完整 Rust clipboard 测试集**

Run: `cargo test clipboard --manifest-path src-tauri/Cargo.toml`

Expected: PASS，models / db / data_transfer / paste / watcher / commands 全绿。

- [ ] **Step 2: 跑前端类型检查**

Run: `pnpm check`

Expected: PASS。

- [ ] **Step 3: 跑与本次改动直接相关的前端单测**

Run: `node --test src/lib/clipboardListPresentation.test.mjs`

Expected: PASS。

- [ ] **Step 4: 跑 ESLint**

Run: `pnpm exec eslint src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue src/components/clipboard-settings/GeneralTab.vue src/components/clipboard/ClipboardList.vue src/lib/clipboardListPresentation.ts src/locales/messages.ts`

Expected: PASS。

- [ ] **Step 5: 跑最终桌面构建**

Run: `pnpm tauri:build:versioned-exe`

Expected: PASS，产出新的 versioned exe。

- [ ] **Step 6: 完整手测**

Manual checklist:

```text
1. Alt+C 面板四边边框一致，无顶部双线。
2. Alt+C 的复制 / 粘贴 / 纯文本粘贴 / 文件粘贴 / 路径粘贴都仍可用。
3. 默认关闭自复制回写时，点击复制不改变历史顺序。
4. 开启后，点击复制会回到顶部，并显示“本工具”来源。
5. 外部应用复制仍能被正常捕获并显示真实来源。
6. /tools/clipboard 仅展示设置页，不再展示 CRUD 列表。
7. Data tab 统计可见，Alt+C 面板功能未回归。
8. `git status --short` 为空，确认 `main` 干净。
```

