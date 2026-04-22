# 剪贴板管理器实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 File-Sync-Tool 的 `/tools` 下新增"剪贴板管理"工具，深度移植 [ElegantClipboard](https://github.com/Y-ASLant/ElegantClipboard) 的全部能力（监听 / SQLite 存储 / 搜索 / 收藏 / 快捷键 / 弹出面板 / Win+V 替代 / 管理员自启动）。

**Architecture:** 双模式混合 —— 主窗口 `/tools/clipboard` 管理后台 + 独立 `clipboard-panel` 弹出窗口（类 Win+V），单 Rust 进程、双 WebviewWindow 共享 `ClipboardState`；SQLite + 图片文件存储；通过 `tauri-plugin-global-shortcut` + `enigo` 实现全局唤出与粘贴。

**Tech Stack:** Rust（rusqlite, clipboard-master, arboard, enigo, blake3, parking_lot, window-vibrancy, rayon）+ Vue 3（vue-virtual-scroller, vue-draggable-plus）+ Tauri 2.10 + `tauri-plugin-global-shortcut` + `tauri-plugin-notification`。

**Spec:** `docs/superpowers/specs/2026-04-19-clipboard-manager-design.md`（16 节完整设计，本计划引用其章节号）

**分支策略：** `feature/clipboard-manager` worktree，位于 `d:\WorkSpace\File-Sync-Tool-clipboard\`。

---

## 前置准备（Task 0）

### Task 0: Worktree 依赖安装 + 构建基线验证

**Files:** 无代码改动，仅环境准备。

- [ ] **Step 0.1：在 worktree 中安装 npm 依赖**

```bash
cd d:/WorkSpace/File-Sync-Tool-clipboard
pnpm install
```

Expected: 依赖完成安装，生成 `node_modules/`，无报错。

- [ ] **Step 0.2：建立构建基线（未改动代码时必须能通过）**

```bash
cd d:/WorkSpace/File-Sync-Tool-clipboard
cmd /c pnpm tauri:build:versioned-exe
```

Expected: 产物 `src-tauri/target/release/bundle/...` 生成，文件名含时间戳。

- [ ] **Step 0.3：确认 main.rs 当前版本号（用于 M5 末尾 bump）**

```bash
grep -n '"version"' src-tauri/tauri.conf.json
```

Expected: `"version": "1.0.6"`。记录该基线，M5 结束时升为 `1.0.7`。

- [ ] **Step 0.4：确认现有 Cargo deps 无冲突（首轮 cargo check）**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: `Finished`，零错误。

无需 commit（纯环境验证）。

---

## M1 · 骨架搭建

目标：依赖就绪 + 空壳路由 + 工具卡片可点进去。

### Task 1.1: 新增 Rust 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1.1.1：在 `[dependencies]` 末尾追加新增 crate**

参考 spec §14.1。编辑 `src-tauri/Cargo.toml`，在现有依赖块尾部追加：

```toml
# Clipboard manager tool (spec §14.1)
rusqlite = { version = "0.32", features = ["bundled", "blob"] }
clipboard-master = "4"
arboard = "3"
enigo = "0.2"
parking_lot = "0.12"
blake3 = "1"
tauri-plugin-global-shortcut = "2"
tauri-plugin-notification = "2"
window-vibrancy = "0.5"
```

注：`rayon` 在 blake3 worktree 里是 transitive 依赖，但显式加入以供 image_store.rs 使用：

```toml
rayon = "1"
```

`fs_extra`、`chrono`、`regex`、`uuid`、`image` 等已存在不需要动。

- [ ] **Step 1.1.2：扩展 `windows` crate features**

编辑同一文件 `[target.'cfg(windows)'.dependencies]` 块的 `windows` features 数组（spec §14.4）：

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_System_Registry",
    "Win32_System_Threading",
    "Win32_System_ProcessStatus",
    "Win32_Security",
    "Win32_System_Diagnostics_ToolHelp",
] }
```

- [ ] **Step 1.1.3：运行 `cargo check` 验证依赖拉取**

```bash
cd src-tauri && cargo check 2>&1 | tail -15
```

Expected: 所有新 crate 被下载编译，`Finished`，零错误。

- [ ] **Step 1.1.4：Commit**

```bash
cd d:/WorkSpace/File-Sync-Tool-clipboard
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(clipboard/M1): 新增 Rust 依赖 rusqlite/arboard/enigo 等"
```

### Task 1.2: 新增 npm 依赖

**Files:**
- Modify: `package.json`

- [ ] **Step 1.2.1：安装前端依赖**

```bash
cd d:/WorkSpace/File-Sync-Tool-clipboard
pnpm add vue-virtual-scroller@^2.0.0-beta.8 vue-draggable-plus@^0.5.6 @tauri-apps/plugin-global-shortcut @tauri-apps/plugin-notification
```

Expected: `package.json` 自动增补 dependencies，`pnpm-lock.yaml` 更新，安装成功。

- [ ] **Step 1.2.2：Commit**

```bash
git add package.json pnpm-lock.yaml
git commit -m "feat(clipboard/M1): 新增 Vue 依赖 virtual-scroller/draggable-plus"
```

### Task 1.3: 创建 Rust clipboard 模块骨架

**Files:**
- Create: `src-tauri/src/clipboard/mod.rs`
- Create: `src-tauri/src/clipboard/models.rs`
- Create: `src-tauri/src/clipboard/db.rs`
- Create: `src-tauri/src/clipboard/watcher.rs`
- Create: `src-tauri/src/clipboard/image_store.rs`
- Create: `src-tauri/src/clipboard/hotkey.rs`
- Create: `src-tauri/src/clipboard/paste.rs`
- Create: `src-tauri/src/clipboard/win_v.rs`
- Create: `src-tauri/src/clipboard/admin.rs`
- Create: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/main.rs`（顶部 `mod clipboard;`）

- [ ] **Step 1.3.1：创建 `clipboard/mod.rs` 带占位 ClipboardState**

```rust
//! Clipboard manager module (spec §5).
//! See docs/superpowers/specs/2026-04-19-clipboard-manager-design.md

pub mod models;
pub mod db;
pub mod watcher;
pub mod image_store;
pub mod hotkey;
pub mod paste;
pub mod win_v;
pub mod admin;
pub mod commands;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use parking_lot::{Mutex, RwLock};

use models::ClipboardSettings;

pub struct ClipboardState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub image_dir: PathBuf,
    pub is_enabled: AtomicBool,
    pub last_hash: Mutex<Option<[u8; 32]>>,
    pub settings: Arc<RwLock<ClipboardSettings>>,
    pub watcher_handle: Mutex<Option<watcher::WatcherHandle>>,
    pub hotkey_handle: Mutex<Option<hotkey::HotkeyHandle>>,
}

impl ClipboardState {
    pub fn shutdown(&self) {
        if let Some(h) = self.watcher_handle.lock().take() {
            h.stop();
        }
        if let Some(h) = self.hotkey_handle.lock().take() {
            h.unregister();
        }
    }
}
```

- [ ] **Step 1.3.2：创建其余占位文件**

每个模块至少有 mod 级 doc 注释 + 占位类型/函数，使编译通过。例如 `clipboard/watcher.rs`：

```rust
//! Clipboard listener (spec §5.1, §8.2).

pub struct WatcherHandle;

impl WatcherHandle {
    pub fn stop(self) {
        // TODO(M2): 关闭 clipboard-master 线程
    }
}
```

其他占位（`db.rs`, `image_store.rs`, `hotkey.rs`, `paste.rs`, `win_v.rs`, `admin.rs`, `commands.rs`）用同样模板：带 `pub struct XxxHandle;` 或空 `pub fn init()` 函数以保证 `mod` 声明有效。

`models.rs` 首次就把 `ClipboardSettings` 写全（spec §7.1）：

```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text, Html, Image, File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        }
    }
}
```

- [ ] **Step 1.3.3：在 `main.rs` 顶部注册模块**

在 `src-tauri/src/main.rs` 现有 `mod xxx;` 列表之后（通常在 `mod config; mod scanner; mod deploy; mod history;` 附近）追加：

```rust
mod clipboard;
```

- [ ] **Step 1.3.4：验证编译**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

Expected: `Finished`，可能有 dead_code warnings（占位未用），允许。

- [ ] **Step 1.3.5：Commit**

```bash
git add src-tauri/src/clipboard/ src-tauri/src/main.rs
git commit -m "feat(clipboard/M1): 新增 Rust clipboard 模块骨架"
```

### Task 1.4: 扩展 AppConfig 增加 ClipboardSettings

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1.4.1：在 `config.rs` 中新增子结构**

在现有 `AppConfig` 结构体定义附近（查找 `pub struct AppConfig`），新增：

```rust
use crate::clipboard::models::ClipboardSettings;

// 在 AppConfig struct 中追加字段：
#[serde(default)]
pub clipboard: ClipboardSettings,
```

`#[serde(default)]` 确保旧配置文件缺失 `clipboard` 字段时自动使用 `Default` 实现（spec §2.1 隐含向后兼容）。

- [ ] **Step 1.4.2：测试旧配置能正常反序列化**

临时运行（仅检查编译）：

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: `Finished`。

- [ ] **Step 1.4.3：Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat(clipboard/M1): AppConfig 新增 clipboard 字段（向后兼容）"
```

### Task 1.5: 新增 Vue 占位页面和路由

**Files:**
- Create: `src/pages/ClipboardManagerPage.vue`
- Create: `src/pages/ClipboardPanelPage.vue`
- Create: `src/lib/clipboardTypes.ts`
- Modify: `src/router/index.ts`
- Modify: `src/App.vue`

- [ ] **Step 1.5.1：创建 `src/lib/clipboardTypes.ts` 完整类型**

直接从 spec §7.1 抄录完整 TypeScript 类型定义到新文件：

```ts
export type ClipboardContentKind = 'text' | 'html' | 'image' | 'file';
export type ClipboardFilter = 'all' | 'text' | 'image' | 'file' | 'favorite';

export interface ClipboardItem {
  id: number;
  kind: ClipboardContentKind;
  content_preview: string;
  content_full: string | null;
  html: string | null;
  image_path: string | null;
  image_width: number | null;
  image_height: number | null;
  file_paths: string[] | null;
  byte_size: number;
  hash: string;
  source_app: string | null;
  is_favorite: boolean;
  favorite_sort_index: number | null;
  created_at: number;
  updated_at: number;
}

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
}

export interface ClipboardListQuery {
  filter: ClipboardFilter;
  search: string;
  offset: number;
  limit: number;
}

export interface ClipboardListResult {
  items: ClipboardItem[];
  total: number;
}

export interface ClipboardStats {
  total: number;
  db_bytes: number;
  image_count: number;
  images_bytes: number;
}
```

- [ ] **Step 1.5.2：创建占位 `ClipboardManagerPage.vue`**

最小可渲染版本：

```vue
<script setup lang="ts">
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-8 px-6 py-6 pb-10">
      <h1 class="text-2xl font-bold tracking-tight text-slate-950">
        {{ t('clipboard.tool.title') }}
      </h1>
      <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      <div class="rounded-2xl border border-slate-200 bg-white p-6 text-slate-400">
        {{ t('common.comingSoon') }}
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 1.5.3：创建占位 `ClipboardPanelPage.vue`**

```vue
<script setup lang="ts">
defineOptions({ name: 'ClipboardPanelPage' });
</script>

<template>
  <div class="flex h-screen w-screen flex-col bg-white/80 backdrop-blur-xl rounded-2xl shadow-2xl">
    <div class="flex items-center justify-between px-4 py-3 border-b border-slate-200/60">
      <span class="text-sm font-semibold text-slate-700">Clipboard</span>
    </div>
    <div class="flex-1 overflow-hidden p-4 text-sm text-slate-400">
      Panel placeholder (M3)
    </div>
  </div>
</template>
```

- [ ] **Step 1.5.4：注册路由**

编辑 `src/router/index.ts`，在 `/tools/file-share` 之后追加：

```ts
{
  path: '/tools/clipboard',
  component: () => import('../pages/ClipboardManagerPage.vue'),
},
{
  path: '/clipboard-panel',
  component: () => import('../pages/ClipboardPanelPage.vue'),
  meta: { noLayout: true },
},
```

- [ ] **Step 1.5.5：`App.vue` 按路由决定是否套 Layout**

查看现有 `App.vue`，识别主布局（Sidebar + router-view）。在模板根节点按 `$route.meta.noLayout` 分支：

```vue
<template>
  <router-view v-if="$route.meta?.noLayout" />
  <DefaultAppLayout v-else />
</template>
```

（`DefaultAppLayout` 是现有主布局的组件名或内联模板。若 App.vue 当前是内联模板，则把主 template 包成 v-else-分支的 div 即可。）

- [ ] **Step 1.5.6：浏览器验证**

```bash
pnpm dev
```

访问 `http://localhost:1420/#/tools/clipboard` 与 `http://localhost:1420/#/clipboard-panel`，应分别渲染占位页面，不套 Sidebar 的是 panel。

- [ ] **Step 1.5.7：Commit**

```bash
git add src/pages/ClipboardManagerPage.vue src/pages/ClipboardPanelPage.vue src/lib/clipboardTypes.ts src/router/index.ts src/App.vue
git commit -m "feat(clipboard/M1): 新增占位路由 /tools/clipboard 与 /clipboard-panel"
```

### Task 1.6: 工具中心卡片入口

**Files:**
- Modify: `src/pages/ToolsHubPage.vue`
- Modify: `src/locales/messages.ts`
- Modify: `src/components/Sidebar.vue` (若有工具子菜单)

- [ ] **Step 1.6.1：`ToolsHubPage.vue` 新增卡片**

在 `toolCards` 数组中（紧随 `file-share` 项之后）追加：

```ts
{
  key: 'clipboard-manager',
  titleKey: 'sidebar.clipboardManager',
  descriptionKey: 'toolsHub.cards.clipboardManager.description',
  path: '/tools/clipboard',
  icon: markRaw(Clipboard as LucideIcon),
  iconClasses: 'from-rose-500 to-pink-600 shadow-rose-500/20',
  chipKey: 'toolsHub.cards.clipboardManager.chip',
},
```

import 顶部增加：

```ts
import { ... , Clipboard } from 'lucide-vue-next';
```

- [ ] **Step 1.6.2：i18n 键位占位（中英）**

编辑 `src/locales/messages.ts`，在 `sidebar` 节中英各自增加：

```ts
// zh
sidebar: {
  // ...
  clipboardManager: '剪贴板管理',
},
// en
sidebar: {
  // ...
  clipboardManager: 'Clipboard Manager',
},
```

在 `toolsHub.cards` 节下新增 `clipboardManager`：

```ts
// zh
toolsHub: {
  cards: {
    // ...
    clipboardManager: {
      description: '本地剪贴板历史：搜索、分组、收藏、快捷键呼出。',
      chip: 'TOOL',
    },
  },
},
// en
toolsHub: {
  cards: {
    // ...
    clipboardManager: {
      description: 'Local clipboard history: search, group, favorite, hotkey panel.',
      chip: 'TOOL',
    },
  },
},
```

顶层命名空间 `clipboard` 先加极少量键（后续任务会继续扩充）：

```ts
// zh
clipboard: {
  tool: {
    title: '剪贴板管理',
    description: '监听系统剪贴板，本地持久化历史，按 Alt+C 呼出快速面板。',
  },
},
// en
clipboard: {
  tool: {
    title: 'Clipboard Manager',
    description: 'Monitor system clipboard, persist history locally, press Alt+C to open quick panel.',
  },
},
```

- [ ] **Step 1.6.3：检查 Sidebar 侧边栏是否需要同步加入子项**

查看 `src/components/Sidebar.vue`，如现有"工具"手风琴菜单列表枚举了 framework-password / appliance-ssh 等，需追加 clipboard-manager；若只跳转到 `/tools`，无需改动。

（根据 `2026-04-12-tools-navigation-design.md` spec，Sidebar 是手风琴折叠菜单；进入 sidebar.vue 找对应 tools 列表加入。）

- [ ] **Step 1.6.4：前端类型检查**

```bash
pnpm check 2>&1 | tail -10
```

Expected: 零错误。

- [ ] **Step 1.6.5：浏览器回归验证**

```bash
pnpm dev
```

访问 `/tools`，应出现 7 张卡片，最后一张是"剪贴板管理"；点击跳转到 `/tools/clipboard`。

- [ ] **Step 1.6.6：Commit**

```bash
git add src/pages/ToolsHubPage.vue src/locales/messages.ts src/components/Sidebar.vue
git commit -m "feat(clipboard/M1): ToolsHub 新增剪贴板管理卡片入口"
```

### M1 · 里程碑验证

- [ ] **Step M1.V1：完整构建通过**

```bash
cd d:/WorkSpace/File-Sync-Tool-clipboard
cmd /c pnpm tauri:build:versioned-exe
```

Expected: 成功产出 `file-sync-tool-1.0.6-YYYYMMDDHHmm.exe`。

- [ ] **Step M1.V2：运行验证产物**

手动运行产物，点击工具中心 → 剪贴板管理，应能进入占位页面。Esc 或返回正常。

- [ ] **Step M1.V3：M1 结束提交（tag）**

```bash
git tag m1-skeleton-done
```

（无需 push tag；本地标记用于回滚参照。）

---

## M2 · 核心监听与存储

目标：SQLite 起数据库、clipboard-master 监听剪贴板、管理页能展示真实历史。

### Task 2.1: SQLite schema 初始化

**Files:**
- Modify: `src-tauri/src/clipboard/db.rs`
- Modify: `src-tauri/src/clipboard/mod.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 2.1.1：实现 `db.rs` 连接与 schema**

完整实现按 spec §7.2：

```rust
use std::path::Path;
use rusqlite::{Connection, Result as SqlResult, params};

pub fn open(db_path: &Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS clipboard_items (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            kind                TEXT NOT NULL CHECK (kind IN ('text','html','image','file')),
            content_preview     TEXT NOT NULL,
            content_full        TEXT,
            html                TEXT,
            image_path          TEXT,
            image_width         INTEGER,
            image_height        INTEGER,
            file_paths_json     TEXT,
            byte_size           INTEGER NOT NULL DEFAULT 0,
            hash                TEXT NOT NULL UNIQUE,
            source_app          TEXT,
            is_favorite         INTEGER NOT NULL DEFAULT 0,
            favorite_sort_index INTEGER,
            created_at          INTEGER NOT NULL,
            updated_at          INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cb_kind       ON clipboard_items(kind);
        CREATE INDEX IF NOT EXISTS idx_cb_favorite   ON clipboard_items(is_favorite) WHERE is_favorite = 1;
        CREATE INDEX IF NOT EXISTS idx_cb_created_at ON clipboard_items(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_cb_fav_sort   ON clipboard_items(favorite_sort_index) WHERE is_favorite = 1;
        CREATE TABLE IF NOT EXISTS schema_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('version', '1');
    "#)?;
    Ok(())
}
```

- [ ] **Step 2.1.2：新增单元测试（内存数据库）**

在 `db.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_applies_cleanly_in_memory() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).expect("migrate should succeed");
        let v: String = conn.query_row(
            "SELECT value FROM schema_meta WHERE key='version'",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(v, "1");
    }

    #[test]
    fn indexes_created() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).expect("migrate");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_cb_%'",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 4);
    }
}
```

- [ ] **Step 2.1.3：运行 `cargo test`**

```bash
cd src-tauri && cargo test -p app_lib clipboard::db 2>&1 | tail -10
```

Expected: 2 tests passed。

- [ ] **Step 2.1.4：ClipboardState 初始化函数**

在 `clipboard/mod.rs` 实现：

```rust
impl ClipboardState {
    pub fn init(app_data_dir: &Path, settings: ClipboardSettings) -> Result<Arc<Self>, String> {
        let db_path = app_data_dir.join("clipboard.db");
        let image_dir = app_data_dir.join("clipboard_images");
        std::fs::create_dir_all(&image_dir)
            .map_err(|e| format!("create image dir: {e}"))?;

        let conn = db::open(&db_path).map_err(|e| format!("open db: {e}"))?;

        Ok(Arc::new(Self {
            db: Arc::new(Mutex::new(conn)),
            image_dir,
            is_enabled: AtomicBool::new(settings.enabled),
            last_hash: Mutex::new(None),
            settings: Arc::new(RwLock::new(settings)),
            watcher_handle: Mutex::new(None),
            hotkey_handle: Mutex::new(None),
        }))
    }
}
```

- [ ] **Step 2.1.5：在 `main.rs` setup 中挂载到 AppState**

找到 `AppState` 结构体定义，新增字段：

```rust
pub clipboard: Arc<clipboard::ClipboardState>,
```

找到 `.setup(...)` 闭包，在其中 AppState 构建之前读取 config 的 clipboard 字段并初始化：

```rust
let app_data_dir = app.path().app_data_dir()
    .map_err(|e| format!("app_data_dir: {e}"))?;
let clipboard_state = clipboard::ClipboardState::init(
    &app_data_dir,
    config.clipboard.clone(),
)?;
```

把 `clipboard_state` 放进 `AppState { ..., clipboard: clipboard_state }`。

- [ ] **Step 2.1.6：`cargo check` 验证**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: `Finished`。

- [ ] **Step 2.1.7：Commit**

```bash
git add src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/mod.rs src-tauri/src/main.rs
git commit -m "feat(clipboard/M2): SQLite schema 初始化 + ClipboardState 挂载"
```

### Task 2.2: models.rs 与 CRUD 操作

**Files:**
- Modify: `src-tauri/src/clipboard/models.rs`
- Modify: `src-tauri/src/clipboard/db.rs`

- [ ] **Step 2.2.1：扩展 models.rs 增加 ClipboardItem / 查询结构**

在 `models.rs` 增加：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub hash: String,
    pub source_app: Option<String>,
    pub is_favorite: bool,
    pub favorite_sort_index: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardFilter { All, Text, Image, File, Favorite }

#[derive(Debug, Clone, Deserialize)]
pub struct ClipboardListQuery {
    pub filter: ClipboardFilter,
    pub search: String,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardListResult {
    pub items: Vec<ClipboardItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardStats {
    pub total: i64,
    pub db_bytes: i64,
    pub image_count: i64,
    pub images_bytes: i64,
}

// ContentKind 增加 SQL 字符串转换
impl ContentKind {
    pub fn as_sql(&self) -> &'static str {
        match self { Self::Text=>"text", Self::Html=>"html", Self::Image=>"image", Self::File=>"file" }
    }
    pub fn from_sql(s: &str) -> Self {
        match s { "text"=>Self::Text, "html"=>Self::Html, "image"=>Self::Image, "file"=>Self::File, _=>Self::Text }
    }
}
```

- [ ] **Step 2.2.2：在 db.rs 增加 CRUD 与查询函数**

```rust
use crate::clipboard::models::{ClipboardItem, ContentKind, ClipboardFilter, ClipboardListQuery, ClipboardListResult};

pub fn insert_item(conn: &Connection, item: &NewItem) -> SqlResult<i64> {
    let now = chrono::Utc::now().timestamp_millis();
    let paths_json = item.file_paths.as_ref()
        .map(|p| serde_json::to_string(p).unwrap_or_default());
    conn.execute(
        "INSERT INTO clipboard_items
          (kind, content_preview, content_full, html, image_path, image_width, image_height,
           file_paths_json, byte_size, hash, source_app, is_favorite, favorite_sort_index,
           created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,0,NULL,?12,?12)",
        params![
            item.kind.as_sql(),
            item.content_preview,
            item.content_full,
            item.html,
            item.image_path,
            item.image_width,
            item.image_height,
            paths_json,
            item.byte_size,
            item.hash,
            item.source_app,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub struct NewItem {
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub hash: String,
    pub source_app: Option<String>,
}

pub fn touch_item_by_hash(conn: &Connection, hash: &str) -> SqlResult<bool> {
    let now = chrono::Utc::now().timestamp_millis();
    let affected = conn.execute(
        "UPDATE clipboard_items SET updated_at=?1 WHERE hash=?2",
        params![now, hash],
    )?;
    Ok(affected > 0)
}

pub fn list_items(conn: &Connection, q: &ClipboardListQuery) -> SqlResult<ClipboardListResult> {
    let (where_sql, params_sql) = build_where(q);
    let count_sql = format!("SELECT COUNT(*) FROM clipboard_items WHERE {where_sql}");
    let list_sql = format!(
        "SELECT id, kind, content_preview, content_full, html, image_path, image_width, image_height,
                file_paths_json, byte_size, hash, source_app, is_favorite, favorite_sort_index,
                created_at, updated_at
         FROM clipboard_items
         WHERE {where_sql}
         ORDER BY
           CASE WHEN is_favorite=1 THEN COALESCE(favorite_sort_index, 9999999) END ASC,
           COALESCE(updated_at, created_at) DESC
         LIMIT ?{} OFFSET ?{}",
        params_sql.len() + 1, params_sql.len() + 2,
    );
    let total: i64 = conn.query_row(&count_sql, rusqlite::params_from_iter(&params_sql), |r| r.get(0))?;

    let mut stmt = conn.prepare(&list_sql)?;
    let mut full_params = params_sql.clone();
    full_params.push(Box::new(q.limit));
    full_params.push(Box::new(q.offset));
    let items = stmt.query_map(rusqlite::params_from_iter(&full_params), row_to_item)?
        .collect::<SqlResult<Vec<_>>>()?;
    Ok(ClipboardListResult { items, total })
}

fn build_where(q: &ClipboardListQuery) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    match q.filter {
        ClipboardFilter::All => {},
        ClipboardFilter::Text => { clauses.push("kind IN ('text','html')".into()); },
        ClipboardFilter::Image => { clauses.push("kind='image'".into()); },
        ClipboardFilter::File => { clauses.push("kind='file'".into()); },
        ClipboardFilter::Favorite => { clauses.push("is_favorite=1".into()); },
    }

    if !q.search.trim().is_empty() {
        let like = format!("%{}%", q.search.trim().replace('%', r"\%").replace('_', r"\_"));
        clauses.push(format!("(content_preview LIKE ?{} ESCAPE '\\\\' OR content_full LIKE ?{} ESCAPE '\\\\')",
            params.len()+1, params.len()+2));
        params.push(Box::new(like.clone()));
        params.push(Box::new(like));
    }

    let where_sql = if clauses.is_empty() { "1=1".into() } else { clauses.join(" AND ") };
    (where_sql, params)
}

fn row_to_item(r: &rusqlite::Row) -> SqlResult<ClipboardItem> {
    let kind_str: String = r.get(1)?;
    let file_paths_json: Option<String> = r.get(8)?;
    let file_paths = file_paths_json.and_then(|s| serde_json::from_str(&s).ok());
    Ok(ClipboardItem {
        id: r.get(0)?,
        kind: ContentKind::from_sql(&kind_str),
        content_preview: r.get(2)?,
        content_full: r.get(3)?,
        html: r.get(4)?,
        image_path: r.get(5)?,
        image_width: r.get(6)?,
        image_height: r.get(7)?,
        file_paths,
        byte_size: r.get(9)?,
        hash: r.get(10)?,
        source_app: r.get(11)?,
        is_favorite: r.get::<_, i64>(12)? != 0,
        favorite_sort_index: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

pub fn delete_item(conn: &Connection, id: i64) -> SqlResult<()> {
    conn.execute("DELETE FROM clipboard_items WHERE id=?1", params![id])?;
    Ok(())
}

pub fn delete_batch(conn: &mut Connection, ids: &[i64]) -> SqlResult<()> {
    let tx = conn.transaction()?;
    for id in ids {
        tx.execute("DELETE FROM clipboard_items WHERE id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn clear_all(conn: &Connection, keep_favorites: bool) -> SqlResult<u64> {
    let sql = if keep_favorites {
        "DELETE FROM clipboard_items WHERE is_favorite=0"
    } else {
        "DELETE FROM clipboard_items"
    };
    let affected = conn.execute(sql, [])?;
    Ok(affected as u64)
}

pub fn toggle_favorite(conn: &Connection, id: i64) -> SqlResult<ClipboardItem> {
    conn.execute(
        "UPDATE clipboard_items SET is_favorite = CASE is_favorite WHEN 1 THEN 0 ELSE 1 END WHERE id=?1",
        params![id],
    )?;
    get_item(conn, id)
}

pub fn get_item(conn: &Connection, id: i64) -> SqlResult<ClipboardItem> {
    conn.query_row(
        "SELECT id, kind, content_preview, content_full, html, image_path, image_width, image_height,
                file_paths_json, byte_size, hash, source_app, is_favorite, favorite_sort_index,
                created_at, updated_at
         FROM clipboard_items WHERE id=?1",
        params![id], row_to_item,
    )
}
```

- [ ] **Step 2.2.3：新增集成测试验证 insert + list**

追加到 `db.rs` 的 `#[cfg(test)] mod tests`：

```rust
#[test]
fn insert_and_list_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();

    let item = NewItem {
        kind: ContentKind::Text,
        content_preview: "hello".into(),
        content_full: Some("hello world".into()),
        html: None, image_path: None, image_width: None, image_height: None,
        file_paths: None, byte_size: 11,
        hash: "abc".into(),
        source_app: None,
    };
    let id = insert_item(&conn, &item).unwrap();
    assert!(id > 0);

    let q = ClipboardListQuery {
        filter: ClipboardFilter::All,
        search: "".into(),
        offset: 0, limit: 10,
    };
    let result = list_items(&conn, &q).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].content_preview, "hello");
}

#[test]
fn search_filters_by_text() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();

    for (i, text) in ["apple", "banana", "cherry"].iter().enumerate() {
        let _ = insert_item(&conn, &NewItem {
            kind: ContentKind::Text,
            content_preview: text.to_string(),
            content_full: Some(text.to_string()),
            html: None, image_path: None, image_width: None, image_height: None,
            file_paths: None, byte_size: text.len() as i64,
            hash: format!("hash_{i}"),
            source_app: None,
        });
    }

    let q = ClipboardListQuery {
        filter: ClipboardFilter::All,
        search: "ana".into(),
        offset: 0, limit: 10,
    };
    let result = list_items(&conn, &q).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].content_preview, "banana");
}
```

- [ ] **Step 2.2.4：运行测试**

```bash
cd src-tauri && cargo test -p app_lib clipboard::db 2>&1 | tail -10
```

Expected: 4 tests passed。

- [ ] **Step 2.2.5：Commit**

```bash
git add src-tauri/src/clipboard/models.rs src-tauri/src/clipboard/db.rs
git commit -m "feat(clipboard/M2): models + db CRUD/查询/搜索实现"
```

### Task 2.3: 图片存储 image_store.rs

**Files:**
- Modify: `src-tauri/src/clipboard/image_store.rs`

- [ ] **Step 2.3.1：实现图片落盘**

```rust
use std::path::{Path, PathBuf};
use image::{ImageBuffer, Rgba, ImageFormat};

/// 保存 RGBA buffer 为 PNG，文件名用 hash 前 16 字符。
/// 返回保存的完整路径。
pub fn save_image_png(
    image_dir: &Path,
    hash_hex: &str,
    width: u32, height: u32,
    rgba: &[u8],
) -> Result<PathBuf, String> {
    let file_name = format!("{}.png", &hash_hex[..16.min(hash_hex.len())]);
    let path = image_dir.join(file_name);
    if path.exists() {
        return Ok(path); // 去重命中，无需重写
    }
    let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "RGBA buffer size mismatch".to_string())?;
    buf.save_with_format(&path, ImageFormat::Png)
        .map_err(|e| format!("save image: {e}"))?;
    Ok(path)
}

/// GC: 删除 image_dir 中所有未被 DB 引用的 .png 文件。
pub fn gc_orphan_images(
    image_dir: &Path,
    referenced_paths: &std::collections::HashSet<String>,
) -> Result<u64, String> {
    use rayon::prelude::*;

    let files: Vec<PathBuf> = std::fs::read_dir(image_dir)
        .map_err(|e| format!("read dir: {e}"))?
        .filter_map(|r| r.ok().map(|d| d.path()))
        .filter(|p| p.extension().map_or(false, |e| e == "png"))
        .collect();

    let deleted: u64 = files.par_iter()
        .filter(|p| {
            let s = p.to_string_lossy().to_string();
            !referenced_paths.contains(&s)
        })
        .map(|p| {
            if std::fs::remove_file(p).is_ok() { 1u64 } else { 0 }
        })
        .sum();

    Ok(deleted)
}
```

- [ ] **Step 2.3.2：单元测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_image_writes_png() {
        let dir = TempDir::new().unwrap();
        let rgba = vec![255u8; 4 * 2 * 2];
        let path = save_image_png(dir.path(), "deadbeef1234567890", 2, 2, &rgba).unwrap();
        assert!(path.exists());
        assert!(path.file_name().unwrap().to_string_lossy().ends_with(".png"));
    }

    #[test]
    fn gc_removes_orphans() {
        let dir = TempDir::new().unwrap();
        let rgba = vec![255u8; 4];
        save_image_png(dir.path(), "referenced_hash", 1, 1, &rgba).unwrap();
        save_image_png(dir.path(), "orphan_hash_abcd", 1, 1, &rgba).unwrap();

        let mut referenced = std::collections::HashSet::new();
        let ref_path = dir.path().join("referenced_hash.png");
        referenced.insert(ref_path.to_string_lossy().to_string());

        let deleted = gc_orphan_images(dir.path(), &referenced).unwrap();
        assert_eq!(deleted, 1);
        assert!(ref_path.exists());
    }
}
```

- [ ] **Step 2.3.3：为测试新增 dev-dependency `tempfile`**

编辑 `src-tauri/Cargo.toml` 的 `[dev-dependencies]`：

```toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
tempfile = "3"
```

- [ ] **Step 2.3.4：运行测试**

```bash
cd src-tauri && cargo test -p app_lib clipboard::image_store 2>&1 | tail -10
```

Expected: 2 tests passed。

- [ ] **Step 2.3.5：Commit**

```bash
git add src-tauri/src/clipboard/image_store.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(clipboard/M2): image_store PNG 落盘 + 并行 GC"
```

### Task 2.4: Watcher 剪贴板监听

**Files:**
- Modify: `src-tauri/src/clipboard/watcher.rs`
- Modify: `src-tauri/src/clipboard/mod.rs`

- [ ] **Step 2.4.1：实现 watcher.rs**

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use parking_lot::Mutex;
use arboard::Clipboard;
use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use tauri::{AppHandle, Emitter};

use crate::clipboard::{ClipboardState, db, image_store};
use crate::clipboard::models::{ContentKind};

pub struct WatcherHandle {
    stop_flag: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
}

impl WatcherHandle {
    pub fn stop(self) {
        self.stop_flag.store(true, Ordering::Release);
        // clipboard-master 在下一次事件或轮询会退出；避免死锁不 join
    }
}

struct Handler {
    app: AppHandle,
    state: Arc<ClipboardState>,
    stop_flag: Arc<AtomicBool>,
}

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        if self.stop_flag.load(Ordering::Acquire) {
            return CallbackResult::Stop;
        }
        if !self.state.is_enabled.load(Ordering::Acquire) {
            return CallbackResult::Next;
        }
        if let Err(e) = try_capture(&self.app, &self.state) {
            eprintln!("[clipboard] capture failed: {e}");
        }
        CallbackResult::Next
    }
    fn on_clipboard_error(&mut self, err: std::io::Error) -> CallbackResult {
        eprintln!("[clipboard] error: {err}");
        CallbackResult::Next
    }
}

fn try_capture(app: &AppHandle, state: &ClipboardState) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;

    // 尝试读取顺序：图片 > 文件 > HTML > 文本
    if let Ok(img) = cb.get_image() {
        let hash = compute_hash(b"image", &img.bytes);
        if skip_duplicate(state, hash) { return Ok(()); }
        let path = image_store::save_image_png(
            &state.image_dir, &hex(&hash), img.width as u32, img.height as u32, &img.bytes
        )?;
        insert_new(state, db::NewItem {
            kind: ContentKind::Image,
            content_preview: format!("[Image {}x{}]", img.width, img.height),
            content_full: None, html: None,
            image_path: Some(path.to_string_lossy().to_string()),
            image_width: Some(img.width as u32),
            image_height: Some(img.height as u32),
            file_paths: None,
            byte_size: img.bytes.len() as i64,
            hash: hex(&hash),
            source_app: None,
        })?;
        notify_added(app);
        return Ok(());
    }

    if let Ok(text) = cb.get_text() {
        if text.trim().is_empty() { return Ok(()); }
        let hash = compute_hash(b"text", text.as_bytes());
        if skip_duplicate(state, hash) { return Ok(()); }
        let preview = text.chars().take(200).collect::<String>();
        insert_new(state, db::NewItem {
            kind: ContentKind::Text,
            content_preview: preview,
            content_full: Some(text.clone()),
            html: None,
            image_path: None, image_width: None, image_height: None,
            file_paths: None,
            byte_size: text.len() as i64,
            hash: hex(&hash),
            source_app: None,
        })?;
        notify_added(app);
    }
    Ok(())
}

fn compute_hash(prefix: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(prefix);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn skip_duplicate(state: &ClipboardState, hash: [u8; 32]) -> bool {
    let mut last = state.last_hash.lock();
    if Some(hash) == *last { return true; }
    *last = Some(hash);
    false
}

fn insert_new(state: &ClipboardState, item: db::NewItem) -> Result<i64, String> {
    let conn = state.db.lock();
    if db::touch_item_by_hash(&conn, &item.hash).map_err(|e| e.to_string())? {
        return Ok(-1); // 已存在，只更新 updated_at
    }
    db::insert_item(&conn, &item).map_err(|e| e.to_string())
}

fn notify_added(app: &AppHandle) {
    let _ = app.emit("clipboard-item-added", ());
}

pub fn start(app: AppHandle, state: Arc<ClipboardState>) -> WatcherHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let app_clone = app.clone();
    let state_clone = state.clone();
    let thread = thread::spawn(move || {
        let handler = Handler {
            app: app_clone,
            state: state_clone,
            stop_flag: stop_flag_clone,
        };
        // Master 在 Windows 上跑独立消息循环；Drop 时 PostQuitMessage
        let mut master = Master::new(handler);
        if let Err(e) = master.run() {
            eprintln!("[clipboard] watcher exit: {e}");
        }
    });
    WatcherHandle { stop_flag, _thread: thread }
}
```

- [ ] **Step 2.4.2：在 ClipboardState::init 内部不自动启动，让 commands 控制**

在 `clipboard/mod.rs` 增加方法：

```rust
impl ClipboardState {
    pub fn enable(self: &Arc<Self>, app: tauri::AppHandle) {
        use std::sync::atomic::Ordering;
        if self.is_enabled.swap(true, Ordering::AcqRel) {
            return; // already enabled
        }
        let handle = watcher::start(app, self.clone());
        *self.watcher_handle.lock() = Some(handle);
    }

    pub fn disable(&self) {
        use std::sync::atomic::Ordering;
        self.is_enabled.store(false, Ordering::Release);
        if let Some(h) = self.watcher_handle.lock().take() {
            h.stop();
        }
    }
}
```

- [ ] **Step 2.4.3：启动时按 settings.enabled 自动 enable**

在 `main.rs setup` 中、`AppState` 构建完成后：

```rust
if config.clipboard.enabled {
    state.clipboard.enable(app.handle().clone());
}
```

注：`state` 是已 `.manage(AppState)` 过的引用；直接用临时 clone 即可，这里按现有项目风格写。

- [ ] **Step 2.4.4：`cargo check`**

```bash
cd src-tauri && cargo check 2>&1 | tail -8
```

Expected: `Finished`。

- [ ] **Step 2.4.5：Commit**

```bash
git add src-tauri/src/clipboard/watcher.rs src-tauri/src/clipboard/mod.rs src-tauri/src/main.rs
git commit -m "feat(clipboard/M2): clipboard-master 监听线程 + BLAKE3 去重"
```

### Task 2.5: Tauri Commands（M2 子集）

**Files:**
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 2.5.1：在 commands.rs 实现 M2 所需命令**

```rust
use std::sync::Arc;
use tauri::{AppHandle, State, Emitter};
use crate::AppState;
use crate::clipboard::{db, ClipboardState};
use crate::clipboard::models::*;

#[tauri::command]
pub fn cb_is_enabled(state: State<'_, AppState>) -> bool {
    state.clipboard.is_enabled.load(std::sync::atomic::Ordering::Acquire)
}

#[tauri::command]
pub fn cb_enable(app: AppHandle, state: State<'_, AppState>) {
    state.clipboard.enable(app);
}

#[tauri::command]
pub fn cb_disable(state: State<'_, AppState>) {
    state.clipboard.disable();
}

#[tauri::command]
pub fn cb_list(state: State<'_, AppState>, query: ClipboardListQuery) -> Result<ClipboardListResult, String> {
    let conn = state.clipboard.db.lock();
    db::list_items(&conn, &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_get(state: State<'_, AppState>, id: i64) -> Result<ClipboardItem, String> {
    let conn = state.clipboard.db.lock();
    db::get_item(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.clipboard.db.lock();
    db::delete_item(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_delete_batch(state: State<'_, AppState>, ids: Vec<i64>) -> Result<(), String> {
    let mut conn = state.clipboard.db.lock();
    db::delete_batch(&mut conn, &ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_clear(state: State<'_, AppState>, keep_favorites: bool) -> Result<u64, String> {
    let conn = state.clipboard.db.lock();
    db::clear_all(&conn, keep_favorites).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_toggle_favorite(state: State<'_, AppState>, id: i64) -> Result<ClipboardItem, String> {
    let conn = state.clipboard.db.lock();
    db::toggle_favorite(&conn, id).map_err(|e| e.to_string())
}
```

- [ ] **Step 2.5.2：在 main.rs `tauri::generate_handler![]` 注册新命令**

找到现有 `.invoke_handler(tauri::generate_handler![...])` 块，在末尾追加：

```rust
    clipboard::commands::cb_is_enabled,
    clipboard::commands::cb_enable,
    clipboard::commands::cb_disable,
    clipboard::commands::cb_list,
    clipboard::commands::cb_get,
    clipboard::commands::cb_delete,
    clipboard::commands::cb_delete_batch,
    clipboard::commands::cb_clear,
    clipboard::commands::cb_toggle_favorite,
```

- [ ] **Step 2.5.3：`cargo check`**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: `Finished`。

- [ ] **Step 2.5.4：Commit**

```bash
git add src-tauri/src/clipboard/commands.rs src-tauri/src/main.rs
git commit -m "feat(clipboard/M2): 注册核心 cb_* commands（list/get/delete/favorite）"
```

### Task 2.6: 前端 tauri.ts 封装 + 简单列表 UI

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/ClipboardManagerPage.vue`
- Create: `src/composables/useClipboardStore.ts`

- [ ] **Step 2.6.1：在 `src/lib/tauri.ts` 追加 cb_* 封装**

```ts
import type {
  ClipboardItem, ClipboardListQuery, ClipboardListResult,
  ClipboardSettings, ClipboardStats
} from './clipboardTypes';

export const clipboardApi = {
  isEnabled: () => invoke<boolean>('cb_is_enabled'),
  enable: () => invoke<void>('cb_enable'),
  disable: () => invoke<void>('cb_disable'),
  list: (query: ClipboardListQuery) =>
    invoke<ClipboardListResult>('cb_list', { query }),
  get: (id: number) => invoke<ClipboardItem>('cb_get', { id }),
  delete: (id: number) => invoke<void>('cb_delete', { id }),
  deleteBatch: (ids: number[]) => invoke<void>('cb_delete_batch', { ids }),
  clear: (keepFavorites: boolean) =>
    invoke<number>('cb_clear', { keepFavorites }),
  toggleFavorite: (id: number) =>
    invoke<ClipboardItem>('cb_toggle_favorite', { id }),
};
```

（注：`invoke` 从现有 `@tauri-apps/api/core` 导入；参考文件内其他 API 封装的 import 写法。）

- [ ] **Step 2.6.2：创建 `useClipboardStore.ts`**

```ts
import { ref, computed } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardItem, ClipboardFilter } from '@/lib/clipboardTypes';

export function useClipboardStore() {
  const items = ref<ClipboardItem[]>([]);
  const total = ref(0);
  const filter = ref<ClipboardFilter>('all');
  const search = ref('');
  const loading = ref(false);

  async function reload() {
    loading.value = true;
    try {
      const r = await clipboardApi.list({
        filter: filter.value,
        search: search.value,
        offset: 0,
        limit: 200,
      });
      items.value = r.items;
      total.value = r.total;
    } finally {
      loading.value = false;
    }
  }

  async function toggleFavorite(id: number) {
    await clipboardApi.toggleFavorite(id);
    await reload();
  }

  async function remove(id: number) {
    await clipboardApi.delete(id);
    await reload();
  }

  function startListening() {
    return listen('clipboard-item-added', () => { reload(); });
  }

  return {
    items, total, filter, search, loading,
    reload, toggleFavorite, remove, startListening,
  };
}
```

- [ ] **Step 2.6.3：ClipboardManagerPage.vue 渲染真实列表**

```vue
<script setup lang="ts">
import { onMounted, onBeforeUnmount } from 'vue';
import { useI18n } from 'vue-i18n';
import { useClipboardStore } from '@/composables/useClipboardStore';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
const store = useClipboardStore();
let unlisten: (() => void) | null = null;

onMounted(async () => {
  unlisten = await store.startListening();
  await store.reload();
});

onBeforeUnmount(() => { unlisten?.(); });

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

function setFilter(f: ClipboardFilter) {
  store.filter.value = f;
  store.reload();
}

function onSearchInput(e: Event) {
  store.search.value = (e.target as HTMLInputElement).value;
  store.reload();
}
</script>

<template>
  <div class="flex-1 overflow-y-auto">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-4 px-6 py-6">
      <h1 class="text-2xl font-bold">{{ t('clipboard.tool.title') }}</h1>

      <div class="flex items-center gap-2">
        <input
          type="search"
          :placeholder="t('clipboard.search.placeholder')"
          class="flex-1 rounded-lg border border-slate-200 px-3 py-2"
          @input="onSearchInput"
        />
      </div>

      <div class="flex gap-2">
        <button
          v-for="f in filters"
          :key="f"
          class="rounded-full px-3 py-1 text-sm"
          :class="store.filter.value === f ? 'bg-slate-900 text-white' : 'bg-slate-100 text-slate-600'"
          @click="setFilter(f)"
        >
          {{ t(`clipboard.filter.${f}`) }}
        </button>
      </div>

      <div class="rounded-2xl border border-slate-200 bg-white">
        <div v-if="store.loading.value" class="p-6 text-slate-400">{{ t('common.loading') }}</div>
        <div v-else-if="store.items.value.length === 0" class="p-6 text-slate-400">
          {{ t('clipboard.panel.empty') }}
        </div>
        <ul v-else class="divide-y divide-slate-100">
          <li v-for="it in store.items.value" :key="it.id" class="flex items-center gap-3 p-3">
            <span class="inline-flex rounded bg-slate-100 px-2 py-0.5 text-xs text-slate-500">{{ it.kind }}</span>
            <span class="flex-1 truncate text-sm text-slate-800">{{ it.content_preview }}</span>
            <button class="text-xs text-slate-400 hover:text-amber-500" @click="store.toggleFavorite(it.id)">
              {{ it.is_favorite ? '★' : '☆' }}
            </button>
            <button class="text-xs text-slate-400 hover:text-rose-500" @click="store.remove(it.id)">×</button>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 2.6.4：i18n 补齐 M2 所需键**

在 `messages.ts` 的 `clipboard` 命名空间追加：

```ts
// zh
filter: { all: '全部', text: '文本', image: '图片', file: '文件', favorite: '收藏' },
search: { placeholder: '搜索剪贴板...' },
panel: { empty: '暂无记录' },
// en
filter: { all: 'All', text: 'Text', image: 'Image', file: 'File', favorite: 'Favorite' },
search: { placeholder: 'Search clipboard...' },
panel: { empty: 'No records' },
```

如 `common.loading` 不存在，则 fallback：`const label = t('common.loading', 'Loading...')`。

- [ ] **Step 2.6.5：端到端手动测试**

```bash
pnpm tauri dev
```

复制一段文本 / 一张图片 / 一些文件，进入 `/tools/clipboard`，应看到条目。Reload 应显示全部。

- [ ] **Step 2.6.6：Commit**

```bash
git add src/lib/tauri.ts src/composables/useClipboardStore.ts src/pages/ClipboardManagerPage.vue src/locales/messages.ts
git commit -m "feat(clipboard/M2): 管理页渲染真实剪贴板历史（含过滤/搜索/收藏/删除）"
```

### Task 2.7: 容量清理任务（retain_days + max_items）

**Files:**
- Create/Modify: `src-tauri/src/clipboard/retention.rs`
- Modify: `src-tauri/src/clipboard/mod.rs`
- Modify: `src-tauri/src/clipboard/watcher.rs`

- [ ] **Step 2.7.1：新增 retention.rs**

```rust
use rusqlite::{Connection, params, Result as SqlResult};
use crate::clipboard::models::ClipboardSettings;

/// 按 retain_days + max_items 清理非收藏条目，返回 (按天数删, 按上限删)。
pub fn run_cleanup(conn: &Connection, settings: &ClipboardSettings) -> SqlResult<(u64, u64)> {
    let mut deleted_by_age = 0u64;
    if settings.retain_days > 0 {
        let cutoff = chrono::Utc::now().timestamp_millis()
            - (settings.retain_days as i64) * 86_400_000;
        deleted_by_age = conn.execute(
            "DELETE FROM clipboard_items
             WHERE is_favorite=0 AND COALESCE(updated_at, created_at) < ?1",
            params![cutoff],
        )? as u64;
    }

    let mut deleted_by_cap = 0u64;
    if settings.max_items > 0 {
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM clipboard_items WHERE is_favorite=0",
            [], |r| r.get(0),
        )?;
        let excess = total - (settings.max_items as i64);
        if excess > 0 {
            deleted_by_cap = conn.execute(
                "DELETE FROM clipboard_items
                 WHERE id IN (
                   SELECT id FROM clipboard_items
                   WHERE is_favorite=0
                   ORDER BY COALESCE(updated_at, created_at) ASC
                   LIMIT ?1
                 )",
                params![excess],
            )? as u64;
        }
    }

    Ok((deleted_by_age, deleted_by_cap))
}
```

- [ ] **Step 2.7.2：测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use crate::clipboard::db;
    use crate::clipboard::models::*;

    #[test]
    fn cleanup_respects_max_items_and_keeps_favorites() {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        for i in 0..10 {
            db::insert_item(&conn, &db::NewItem {
                kind: ContentKind::Text,
                content_preview: format!("item {i}"),
                content_full: None, html: None,
                image_path: None, image_width: None, image_height: None,
                file_paths: None, byte_size: 0,
                hash: format!("h{i}"),
                source_app: None,
            }).unwrap();
        }
        // 标记 2 条为收藏
        conn.execute("UPDATE clipboard_items SET is_favorite=1 WHERE id IN (1,2)", []).unwrap();

        let mut s = ClipboardSettings::default();
        s.max_items = 5;
        s.retain_days = 0;
        let (_, cap) = run_cleanup(&conn, &s).unwrap();
        assert_eq!(cap, 3); // 删除 3 条非收藏
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 7);
    }
}
```

注意：`db::migrate` 需要导出 `pub`，如之前为私有则改为 `pub`。

- [ ] **Step 2.7.3：挂到 watcher 每次插入后调用（debounced 模拟）**

在 `watcher::insert_new` 成功路径后加：

```rust
let _ = {
    let settings = state.settings.read().clone();
    crate::clipboard::retention::run_cleanup(&state.db.lock(), &settings)
};
```

（初版不做 debounce，后期 M4 如有性能问题再加。）

- [ ] **Step 2.7.4：把 retention 加入 `mod.rs`**

```rust
pub mod retention;
```

- [ ] **Step 2.7.5：测试通过**

```bash
cd src-tauri && cargo test -p app_lib clipboard::retention 2>&1 | tail -10
```

Expected: 1 test passed。

- [ ] **Step 2.7.6：Commit**

```bash
git add src-tauri/src/clipboard/retention.rs src-tauri/src/clipboard/mod.rs src-tauri/src/clipboard/watcher.rs src-tauri/src/clipboard/db.rs
git commit -m "feat(clipboard/M2): 容量清理（retain_days + max_items，收藏豁免）"
```

### M2 · 里程碑验证

- [ ] **Step M2.V1：单元测试全绿**

```bash
cd src-tauri && cargo test -p app_lib clipboard 2>&1 | tail -10
```

Expected: 7+ tests passed。

- [ ] **Step M2.V2：`tauri dev` 手动 E2E**

复制 10 条各类内容，确认：
- 管理页能看到所有条目
- 重复复制不新增（hash 去重生效）
- 关闭应用重新打开，数据仍在
- 切换过滤 tab 生效
- 搜索关键字过滤生效
- 收藏/取消生效
- 删除单条生效

- [ ] **Step M2.V3：构建验证**

```bash
cmd /c pnpm tauri:build:versioned-exe
```

- [ ] **Step M2.V4：打 tag**

```bash
git tag m2-core-store-done
```

---

## M3 · 弹出面板与快捷键

目标：独立 Panel 窗口 + Alt+C 全局快捷键 + Enter 粘贴 + 启动通知。

### Task 3.1: 创建 clipboard-panel 窗口

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 3.1.1：`tauri.conf.json` 不用静态声明 panel 窗口**

保持现有主窗口声明不变；panel 通过 `setup` 闭包动态创建（更灵活，便于按 config.clipboard.enabled 决定初始显隐）。

- [ ] **Step 3.1.2：在 `main.rs setup` 创建 panel window**

```rust
use tauri::{WebviewWindowBuilder, WebviewUrl, PhysicalPosition, PhysicalSize};

let panel = WebviewWindowBuilder::new(
    app,
    "clipboard-panel",
    WebviewUrl::App("index.html#/clipboard-panel".into()),
)
.title("Clipboard")
.inner_size(420.0, 720.0)
.decorations(false)
.resizable(false)
.skip_taskbar(true)
.always_on_top(true)
.visible(false)
.transparent(true)
.build()?;

// Mica/Acrylic 特效（Win11 优先 Mica）
#[cfg(target_os = "windows")]
{
    if let Err(_) = window_vibrancy::apply_mica(&panel, Some(true)) {
        let _ = window_vibrancy::apply_acrylic(&panel, Some((255, 255, 255, 125)));
    }
}

// 失焦自动隐藏
let panel_label = "clipboard-panel".to_string();
let panel_clone = panel.clone();
panel.on_window_event(move |ev| {
    if let tauri::WindowEvent::Focused(false) = ev {
        let _ = panel_clone.hide();
    }
});
```

- [ ] **Step 3.1.3：添加 cb_toggle_panel command**

在 `clipboard/commands.rs`：

```rust
use tauri::{Manager, PhysicalPosition};

#[tauri::command]
pub fn cb_toggle_panel(app: AppHandle) -> Result<(), String> {
    let panel = app.get_webview_window("clipboard-panel")
        .ok_or_else(|| "panel not found".to_string())?;
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
    } else {
        // 在光标附近定位，限制到当前屏幕可视区域
        if let Ok(pos) = app.cursor_position() {
            if let Ok(monitor) = panel.current_monitor() {
                let (x, y) = clamp_to_screen(
                    pos.x as i32, pos.y as i32,
                    420, 720,
                    monitor.as_ref(),
                );
                let _ = panel.set_position(PhysicalPosition::new(x, y));
            }
        }
        let _ = panel.show();
        let _ = panel.set_focus();
        let _ = app.emit("clipboard-panel-shown", ());
    }
    Ok(())
}

fn clamp_to_screen(
    x: i32, y: i32, w: i32, h: i32,
    monitor: Option<&tauri::Monitor>,
) -> (i32, i32) {
    let (sw, sh) = monitor.map(|m| {
        let s = m.size();
        (s.width as i32, s.height as i32)
    }).unwrap_or((1920, 1080));
    let cx = x.min(sw - w - 10).max(10);
    let cy = y.min(sh - h - 10).max(10);
    (cx, cy)
}
```

记得在 main.rs handler 列表中注册 `cb_toggle_panel`。

- [ ] **Step 3.1.4：`cargo check`**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

- [ ] **Step 3.1.5：Commit**

```bash
git add src-tauri/src/main.rs src-tauri/src/clipboard/commands.rs
git commit -m "feat(clipboard/M3): 创建 clipboard-panel 独立窗口 + Mica 特效 + toggle command"
```

### Task 3.2: 全局快捷键 hotkey.rs

**Files:**
- Modify: `src-tauri/src/clipboard/hotkey.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 3.2.1：实现 hotkey.rs**

```rust
use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use parking_lot::Mutex;

pub struct HotkeyHandle {
    registered: Arc<Mutex<Option<Shortcut>>>,
    app: AppHandle,
}

impl HotkeyHandle {
    pub fn unregister(self) {
        if let Some(s) = self.registered.lock().take() {
            let _ = self.app.global_shortcut().unregister(s);
        }
    }
}

pub fn register(app: AppHandle, hotkey_str: &str) -> Result<HotkeyHandle, String> {
    let gs = app.global_shortcut();
    let shortcut = Shortcut::from_str(hotkey_str)
        .map_err(|e| format!("parse hotkey: {e}"))?;

    let app_clone = app.clone();
    gs.on_shortcut(shortcut.clone(), move |app, _, _| {
        // 与 cb_toggle_panel 同逻辑
        let _ = tauri::async_runtime::spawn(async move {});
        let _ = crate::clipboard::commands::cb_toggle_panel_internal(app.clone());
    }).map_err(|e| format!("register hotkey: {e}"))?;

    Ok(HotkeyHandle {
        registered: Arc::new(Mutex::new(Some(shortcut))),
        app: app_clone,
    })
}

pub fn change(
    app: AppHandle,
    current: &Mutex<Option<HotkeyHandle>>,
    new_hotkey: &str,
) -> Result<(), String> {
    // 先 unregister 旧的
    if let Some(old) = current.lock().take() {
        old.unregister();
    }
    // 注册新的
    let h = register(app, new_hotkey)?;
    *current.lock() = Some(h);
    Ok(())
}
```

- [ ] **Step 3.2.2：把 `cb_toggle_panel` 的核心逻辑抽成 `cb_toggle_panel_internal`**

在 `clipboard/commands.rs`：

```rust
pub fn cb_toggle_panel_internal(app: AppHandle) -> Result<(), String> {
    // 把之前 cb_toggle_panel 的函数体移到这里
    // ...
}

#[tauri::command]
pub fn cb_toggle_panel(app: AppHandle) -> Result<(), String> {
    cb_toggle_panel_internal(app)
}
```

- [ ] **Step 3.2.3：初始化快捷键插件**

在 `main.rs` `.plugin(...)` 链上追加：

```rust
.plugin(tauri_plugin_global_shortcut::Builder::new().build())
```

- [ ] **Step 3.2.4：在 setup 中按 settings.hotkey 注册**

```rust
if config.clipboard.enabled {
    match clipboard::hotkey::register(app.handle().clone(), &config.clipboard.hotkey) {
        Ok(h) => *state.clipboard.hotkey_handle.lock() = Some(h),
        Err(e) => eprintln!("[clipboard] hotkey register failed: {e}"),
    }
}
```

- [ ] **Step 3.2.5：增加 `cb_set_hotkey` command**

```rust
#[tauri::command]
pub fn cb_set_hotkey(app: AppHandle, state: State<'_, AppState>, hotkey: String) -> Result<(), String> {
    crate::clipboard::hotkey::change(
        app,
        &state.clipboard.hotkey_handle,
        &hotkey,
    )?;
    state.clipboard.settings.write().hotkey = hotkey;
    // TODO(M4): 同步保存到 config.json
    Ok(())
}
```

在 main.rs handler 注册。

- [ ] **Step 3.2.6：Commit**

```bash
git add src-tauri/src/clipboard/hotkey.rs src-tauri/src/clipboard/commands.rs src-tauri/src/main.rs
git commit -m "feat(clipboard/M3): 全局快捷键 Alt+C 注册 + 动态切换"
```

### Task 3.3: 粘贴机制 paste.rs

**Files:**
- Modify: `src-tauri/src/clipboard/paste.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`

- [ ] **Step 3.3.1：实现 paste.rs**

```rust
use std::thread;
use std::time::Duration;
use arboard::{Clipboard, ImageData};
use enigo::{Enigo, Key, Keyboard, Settings, Direction::{Press, Release, Click}};
use tauri::{AppHandle, Manager};

use crate::clipboard::models::{ClipboardItem, ContentKind};

pub fn paste_item(app: &AppHandle, item: &ClipboardItem, plain_text: bool) -> Result<(), String> {
    // 1. 把内容写回系统剪贴板
    write_to_clipboard(item, plain_text)?;

    // 2. 隐藏面板
    if let Some(panel) = app.get_webview_window("clipboard-panel") {
        let _ = panel.hide();
    }

    // 3. 等焦点回到目标窗口
    thread::sleep(Duration::from_millis(30));

    // 4. 模拟 Ctrl+V
    simulate_paste()
}

fn write_to_clipboard(item: &ClipboardItem, plain_text: bool) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;

    match item.kind {
        ContentKind::Text => {
            let text = item.content_full.as_deref().unwrap_or(&item.content_preview);
            cb.set_text(text).map_err(|e| format!("set text: {e}"))?;
        }
        ContentKind::Html => {
            let text = item.content_full.as_deref().unwrap_or(&item.content_preview);
            if plain_text {
                cb.set_text(text).map_err(|e| format!("set text: {e}"))?;
            } else {
                let html = item.html.as_deref().unwrap_or(text);
                cb.set_html(html, Some(text)).map_err(|e| format!("set html: {e}"))?;
            }
        }
        ContentKind::Image => {
            let path = item.image_path.as_deref()
                .ok_or_else(|| "image path missing".to_string())?;
            let img = image::open(path).map_err(|e| format!("open image: {e}"))?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let data = ImageData {
                width: w as usize,
                height: h as usize,
                bytes: std::borrow::Cow::Owned(rgba.into_raw()),
            };
            cb.set_image(data).map_err(|e| format!("set image: {e}"))?;
        }
        ContentKind::File => {
            // arboard 暂不支持 file list，回退到文本路径拼接
            let paths = item.file_paths.as_ref()
                .ok_or_else(|| "file paths missing".to_string())?;
            cb.set_text(paths.join("\n")).map_err(|e| format!("set text: {e}"))?;
        }
    }
    Ok(())
}

fn simulate_paste() -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {e}"))?;
    enigo.key(Key::Control, Press).map_err(|e| format!("ctrl press: {e}"))?;
    enigo.key(Key::Unicode('v'), Click).map_err(|e| format!("v click: {e}"))?;
    enigo.key(Key::Control, Release).map_err(|e| format!("ctrl release: {e}"))?;
    Ok(())
}
```

- [ ] **Step 3.3.2：新增 cb_paste / cb_paste_plain commands**

在 `commands.rs`：

```rust
use crate::clipboard::paste;

#[tauri::command]
pub fn cb_paste(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    paste::paste_item(&app, &item, false)
}

#[tauri::command]
pub fn cb_paste_plain(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    paste::paste_item(&app, &item, true)
}
```

注册到 main.rs handler。

- [ ] **Step 3.3.3：Commit**

```bash
git add src-tauri/src/clipboard/paste.rs src-tauri/src/clipboard/commands.rs src-tauri/src/main.rs
git commit -m "feat(clipboard/M3): enigo 粘贴 + 多类型剪贴板回写"
```

### Task 3.4: 弹出面板 UI

**Files:**
- Modify: `src/pages/ClipboardPanelPage.vue`
- Create: `src/composables/useClipboardHotkey.ts`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 3.4.1：扩展 clipboardApi 加入 paste/toggle_panel**

```ts
// src/lib/tauri.ts 的 clipboardApi 对象追加
  paste: (id: number) => invoke<void>('cb_paste', { id }),
  pastePlain: (id: number) => invoke<void>('cb_paste_plain', { id }),
  togglePanel: () => invoke<void>('cb_toggle_panel'),
```

- [ ] **Step 3.4.2：创建 `useClipboardHotkey.ts`**

```ts
import { onMounted, onBeforeUnmount, type Ref } from 'vue';
import type { ClipboardItem, ClipboardFilter } from '@/lib/clipboardTypes';

interface Options {
  items: Ref<ClipboardItem[]>;
  selectedIndex: Ref<number>;
  filter: Ref<ClipboardFilter>;
  onPaste: (id: number, plain: boolean) => void;
  onDelete: (id: number) => void;
  onFavorite: (id: number) => void;
  onClose: () => void;
  onFocusSearch: () => void;
  searchValue: Ref<string>;
  onFilterChange: (dir: 1 | -1) => void;
}

export function useClipboardHotkey(opts: Options) {
  function handler(e: KeyboardEvent) {
    const { items, selectedIndex } = opts;
    const list = items.value;
    const isInput = (e.target as HTMLElement)?.tagName === 'INPUT';

    switch (e.key) {
      case 'ArrowDown':
        if (isInput) return;
        selectedIndex.value = (selectedIndex.value + 1) % list.length;
        e.preventDefault();
        break;
      case 'ArrowUp':
        if (isInput) return;
        selectedIndex.value = (selectedIndex.value - 1 + list.length) % list.length;
        e.preventDefault();
        break;
      case 'ArrowLeft':
        if (isInput) return;
        opts.onFilterChange(-1); e.preventDefault();
        break;
      case 'ArrowRight':
        if (isInput) return;
        opts.onFilterChange(1); e.preventDefault();
        break;
      case 'Enter':
        if (list[selectedIndex.value]) {
          opts.onPaste(list[selectedIndex.value].id, e.shiftKey);
          e.preventDefault();
        }
        break;
      case 'Delete':
        if (isInput) return;
        if (list[selectedIndex.value]) opts.onDelete(list[selectedIndex.value].id);
        e.preventDefault();
        break;
      case 'd':
      case 'D':
        if (e.ctrlKey && list[selectedIndex.value]) {
          opts.onFavorite(list[selectedIndex.value].id); e.preventDefault();
        }
        break;
      case 'f':
      case 'F':
        if (e.ctrlKey) { opts.onFocusSearch(); e.preventDefault(); }
        break;
      case '/':
        if (!isInput) { opts.onFocusSearch(); e.preventDefault(); }
        break;
      case 'Escape':
        if (opts.searchValue.value) {
          opts.searchValue.value = ''; e.preventDefault();
        } else {
          opts.onClose(); e.preventDefault();
        }
        break;
    }
  }

  onMounted(() => window.addEventListener('keydown', handler));
  onBeforeUnmount(() => window.removeEventListener('keydown', handler));
}
```

- [ ] **Step 3.4.3：实现 ClipboardPanelPage.vue 完整交互**

（完整代码较长，按 spec §6.1 和 §9.2 实现；关键是列表 + 搜索 + filter tabs + 快捷键接入 + 点击粘贴）

```vue
<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from 'vue-i18n';
import { useClipboardStore } from '@/composables/useClipboardStore';
import { useClipboardHotkey } from '@/composables/useClipboardHotkey';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardPanelPage' });

const { t } = useI18n();
const store = useClipboardStore();
const selectedIndex = ref(0);
const searchInput = ref<HTMLInputElement | null>(null);

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

async function paste(id: number, plain: boolean) {
  if (plain) await clipboardApi.pastePlain(id);
  else await clipboardApi.paste(id);
}

function close() {
  getCurrentWindow().hide();
}

function changeFilter(dir: 1 | -1) {
  const idx = filters.indexOf(store.filter.value);
  const next = filters[(idx + dir + filters.length) % filters.length];
  store.filter.value = next;
  store.reload();
}

useClipboardHotkey({
  items: store.items,
  selectedIndex,
  filter: store.filter,
  onPaste: paste,
  onDelete: (id) => store.remove(id),
  onFavorite: (id) => store.toggleFavorite(id),
  onClose: close,
  onFocusSearch: () => searchInput.value?.focus(),
  searchValue: store.search,
  onFilterChange: changeFilter,
});

let unlistenShown: (() => void) | null = null;
let unlistenAdded: (() => void) | null = null;

onMounted(async () => {
  unlistenShown = await listen('clipboard-panel-shown', () => {
    store.search.value = '';
    selectedIndex.value = 0;
    store.reload();
    searchInput.value?.focus();
  });
  unlistenAdded = await store.startListening();
  await store.reload();
});

onBeforeUnmount(() => {
  unlistenShown?.();
  unlistenAdded?.();
});
</script>

<template>
  <div class="flex h-screen w-screen flex-col bg-white/85 backdrop-blur-xl rounded-2xl shadow-2xl overflow-hidden">
    <!-- 搜索 -->
    <div class="px-3 pt-3 pb-2">
      <input
        ref="searchInput"
        v-model="store.search.value"
        type="search"
        :placeholder="t('clipboard.search.placeholder')"
        class="w-full rounded-lg border border-slate-200/70 bg-white/60 px-3 py-1.5 text-sm outline-none focus:border-slate-400"
        @input="store.reload()"
      />
    </div>

    <!-- Filter tabs -->
    <div class="flex gap-1 px-3 pb-2">
      <button
        v-for="f in filters"
        :key="f"
        class="rounded-full px-2.5 py-0.5 text-xs transition-colors"
        :class="store.filter.value === f
          ? 'bg-slate-900 text-white'
          : 'bg-slate-200/60 text-slate-600 hover:bg-slate-200'"
        @click="store.filter.value = f; store.reload()"
      >
        {{ t(`clipboard.filter.${f}`) }}
      </button>
    </div>

    <!-- 列表 -->
    <div class="flex-1 overflow-y-auto px-2">
      <div v-if="store.items.value.length === 0" class="p-6 text-center text-sm text-slate-400">
        {{ t('clipboard.panel.empty') }}
      </div>
      <button
        v-for="(it, idx) in store.items.value"
        :key="it.id"
        class="w-full text-left rounded-lg px-3 py-2 transition-colors"
        :class="idx === selectedIndex ? 'bg-slate-100 ring-1 ring-slate-300' : 'hover:bg-slate-50'"
        @click="paste(it.id, false)"
      >
        <div class="flex items-center gap-2">
          <span class="inline-flex rounded bg-slate-200/60 px-1.5 py-0 text-[10px] uppercase text-slate-600">{{ it.kind }}</span>
          <span v-if="it.is_favorite" class="text-amber-500 text-xs">★</span>
          <span class="flex-1 truncate text-xs text-slate-700">{{ it.content_preview }}</span>
        </div>
      </button>
    </div>
  </div>
</template>
```

- [ ] **Step 3.4.4：手动 E2E 测试**

```bash
pnpm tauri dev
```

- 启动后按 `Alt+C`，应弹出面板
- 输入关键字搜索
- `↑↓` 选择
- `Enter` 粘贴（切到记事本验证）
- `Esc` 关闭

- [ ] **Step 3.4.5：Commit**

```bash
git add src/pages/ClipboardPanelPage.vue src/composables/useClipboardHotkey.ts src/lib/tauri.ts
git commit -m "feat(clipboard/M3): 弹出面板 UI + 窗口内快捷键完整交互"
```

### Task 3.5: 启动通知 S02

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `src/locales/messages.ts`

- [ ] **Step 3.5.1：注册通知插件**

```rust
.plugin(tauri_plugin_notification::init())
```

- [ ] **Step 3.5.2：setup 末尾发送启动通知**

```rust
use tauri_plugin_notification::NotificationExt;

if config.clipboard.enabled && config.clipboard.show_startup_notification {
    let handle = app.handle().clone();
    let hotkey = config.clipboard.hotkey.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = handle.notification()
            .builder()
            .title("File-Sync-Tool 剪贴板")
            .body(format!("剪贴板监听已启动，按 {hotkey} 呼出面板"))
            .show();
    });
}
```

- [ ] **Step 3.5.3：i18n 补齐 `clipboard.notification`**

spec §9.7 清单中的 `notification.title` / `notification.body`（后者用插值 `{{hotkey}}`）。

- [ ] **Step 3.5.4：Commit**

```bash
git add src-tauri/src/main.rs src/locales/messages.ts
git commit -m "feat(clipboard/M3): 启动通知 S02（可在设置关闭）"
```

### M3 · 里程碑验证

- [ ] **Step M3.V1：构建 + 手动 E2E**

```bash
cmd /c pnpm tauri:build:versioned-exe
```

运行产物：
- 启动后 500ms 出现通知
- `Alt+C` 在任意应用中可唤出面板
- `Enter` 粘贴到 VSCode / Chrome / 记事本 成功
- `Shift+Enter` 粘贴纯文本
- 失焦自动隐藏

- [ ] **Step M3.V2：tag**

```bash
git tag m3-panel-hotkey-done
```

---

## M4 · 交互增强

目标：虚拟列表 + 悬浮预览 + 收藏拖拽 + 搜索运算符 + 批量操作 + 统计 + 设置面板。

### Task 4.1: 虚拟列表接入

**Files:**
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/pages/ClipboardManagerPage.vue`
- Create: `src/components/clipboard/ClipboardList.vue`

- [ ] **Step 4.1.1：创建 ClipboardList.vue 统一复用组件**

```vue
<script setup lang="ts">
import { DynamicScroller, DynamicScrollerItem } from 'vue-virtual-scroller';
import 'vue-virtual-scroller/dist/vue-virtual-scroller.css';
import type { ClipboardItem } from '@/lib/clipboardTypes';

defineProps<{
  items: ClipboardItem[];
  selectedId: number | null;
  compact: boolean;
}>();

const emit = defineEmits<{
  select: [id: number];
  activate: [id: number];
  favorite: [id: number];
  remove: [id: number];
}>();

function heightOf(it: ClipboardItem): number {
  if (it.kind === 'image') return 120;
  if (it.kind === 'file') return 80;
  return 64;
}
</script>

<template>
  <DynamicScroller
    :items="items"
    :min-item-size="64"
    key-field="id"
    class="h-full w-full"
  >
    <template #default="{ item, active }">
      <DynamicScrollerItem :item="item" :active="active" :size-dependencies="[item.content_preview, item.kind]">
        <button
          class="w-full rounded-lg px-3 py-2 text-left transition-colors"
          :class="item.id === selectedId ? 'bg-slate-100 ring-1 ring-slate-300' : 'hover:bg-slate-50'"
          :style="{ minHeight: `${heightOf(item)}px` }"
          @click="emit('activate', item.id)"
          @mouseenter="emit('select', item.id)"
        >
          <!-- 多态渲染: 文本/HTML/图片/文件 -->
          <div v-if="item.kind === 'image' && item.image_path" class="flex items-center gap-3">
            <img :src="`asset://${item.image_path.replace(/\\/g, '/')}`"
                 class="h-24 w-32 object-cover rounded" />
            <span class="text-xs text-slate-500">{{ item.image_width }}×{{ item.image_height }}</span>
          </div>
          <div v-else-if="item.kind === 'file'">
            <span class="text-xs font-mono text-slate-600">{{ item.content_preview }}</span>
          </div>
          <div v-else class="text-sm text-slate-700">
            {{ item.content_preview }}
          </div>
        </button>
      </DynamicScrollerItem>
    </template>
  </DynamicScroller>
</template>
```

注：`asset://` 需要在 `tauri.conf.json` 的 `assetProtocol.scope` 加入 `$APPDATA\**\clipboard_images\**`。

- [ ] **Step 4.1.2：扩展 tauri.conf.json 的 assetProtocol**

在 `app.security` 下增加：

```json
"assetProtocol": {
  "enable": true,
  "scope": ["$APPDATA/**/clipboard_images/**"]
}
```

- [ ] **Step 4.1.3：替换两个页面的列表实现**

把 ClipboardPanelPage 和 ClipboardManagerPage 的简单 `<ul>` 列表替换为 `<ClipboardList>` 组件。

- [ ] **Step 4.1.4：性能验证**

手动向 DB 插入 1000+ 条假数据（可临时写一个测试 command `cb_seed_debug`），滚动检查帧率 ≥ 50fps。

- [ ] **Step 4.1.5：Commit**

```bash
git add src/components/clipboard/ src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue src-tauri/tauri.conf.json
git commit -m "feat(clipboard/M4): 虚拟列表（vue-virtual-scroller）接入"
```

### Task 4.2: 悬浮预览

**Files:**
- Create: `src/components/clipboard/ClipboardHoverPreview.vue`
- Create: `src/composables/useHoverPreview.ts`
- Modify: `src/pages/ClipboardPanelPage.vue`

- [ ] **Step 4.2.1：实现 composable**

```ts
import { ref, onBeforeUnmount } from 'vue';
import type { ClipboardItem } from '@/lib/clipboardTypes';

export function useHoverPreview(delayMs: number = 500) {
  const activeItem = ref<ClipboardItem | null>(null);
  const scale = ref(1);
  let timer: number | null = null;

  function onEnter(item: ClipboardItem) {
    clearTimer();
    if (item.kind !== 'image') {
      // 文本预览视 settings.enable_text_preview 由调用方决定是否调用
    }
    timer = window.setTimeout(() => { activeItem.value = item; }, delayMs);
  }

  function onLeave() {
    clearTimer();
    timer = window.setTimeout(() => { activeItem.value = null; }, 150);
  }

  function onWheelZoom(e: WheelEvent) {
    if (!e.ctrlKey || !activeItem.value) return;
    e.preventDefault();
    scale.value = Math.max(0.5, Math.min(5, scale.value + (e.deltaY < 0 ? 0.1 : -0.1)));
  }

  function clearTimer() {
    if (timer !== null) { clearTimeout(timer); timer = null; }
  }

  onBeforeUnmount(clearTimer);

  return { activeItem, scale, onEnter, onLeave, onWheelZoom };
}
```

- [ ] **Step 4.2.2：实现预览组件**

```vue
<script setup lang="ts">
import type { ClipboardItem } from '@/lib/clipboardTypes';

defineProps<{
  item: ClipboardItem;
  scale: number;
}>();
</script>

<template>
  <div
    class="fixed top-6 right-[440px] max-h-[80vh] max-w-[60vw] overflow-hidden rounded-xl bg-white/95 shadow-2xl border border-slate-200/60 p-3 z-50 pointer-events-none"
  >
    <img
      v-if="item.kind === 'image' && item.image_path"
      :src="`asset://${item.image_path.replace(/\\/g, '/')}`"
      :style="{ transform: `scale(${scale})`, transformOrigin: 'top left' }"
      class="transition-transform"
    />
    <pre
      v-else-if="item.kind === 'text' || item.kind === 'html'"
      class="whitespace-pre-wrap text-xs text-slate-700 max-h-[70vh] overflow-y-auto font-mono"
    >{{ item.content_full || item.content_preview }}</pre>

    <span class="absolute bottom-2 right-3 text-[10px] text-slate-500 bg-white/80 rounded px-1.5 py-0.5">
      {{ Math.round(scale * 100) }}%
    </span>
  </div>
</template>
```

- [ ] **Step 4.2.3：Panel 页挂接 hover + wheel**

在 ClipboardPanelPage 的 `<ClipboardList>` 使用处外包装：

```vue
<div @mouseleave="preview.onLeave" @wheel="preview.onWheelZoom">
  <ClipboardList
    :items="store.items.value"
    ...
    @select="(id) => preview.onEnter(findItem(id))"
  />
</div>
<ClipboardHoverPreview v-if="preview.activeItem.value" :item="preview.activeItem.value" :scale="preview.scale.value" />
```

只在 `settings.enable_text_preview || item.kind === 'image'` 时才调用 `onEnter`。

- [ ] **Step 4.2.4：Commit**

```bash
git add src/composables/useHoverPreview.ts src/components/clipboard/ClipboardHoverPreview.vue src/pages/ClipboardPanelPage.vue
git commit -m "feat(clipboard/M4): 悬浮预览（图片/文本 + Ctrl+滚轮缩放）"
```

### Task 4.3: 收藏拖拽排序

**Files:**
- Modify: `src-tauri/src/clipboard/db.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 4.3.1：`db::reorder_favorites` 事务**

```rust
pub fn reorder_favorites(conn: &mut Connection, ids: &[i64]) -> SqlResult<()> {
    let tx = conn.transaction()?;
    for (idx, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE clipboard_items SET favorite_sort_index=?1 WHERE id=?2 AND is_favorite=1",
            params![idx as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4.3.2：cb_reorder_favorites command**

```rust
#[tauri::command]
pub fn cb_reorder_favorites(state: State<'_, AppState>, ids: Vec<i64>) -> Result<(), String> {
    let mut conn = state.clipboard.db.lock();
    db::reorder_favorites(&mut conn, &ids).map_err(|e| e.to_string())
}
```

注册 handler。

- [ ] **Step 4.3.3：ClipboardList 使用 vue-draggable-plus**

```vue
<script setup lang="ts">
import { VueDraggable } from 'vue-draggable-plus';
// ...
</script>

<template>
  <VueDraggable
    v-if="filter === 'favorite'"
    v-model="items"
    @end="(e) => emit('reorder', items.map(i => i.id))"
  >
    <!-- 列表项 -->
  </VueDraggable>
  <DynamicScroller v-else ...>...</DynamicScroller>
</template>
```

注：收藏分组条目通常不会太多（< 100），可不用虚拟滚动。

- [ ] **Step 4.3.4：TS 类型检查**

```bash
pnpm check 2>&1 | tail -5
```

- [ ] **Step 4.3.5：Commit**

```bash
git add src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/commands.rs src/components/clipboard/ClipboardList.vue src/lib/tauri.ts
git commit -m "feat(clipboard/M4): 收藏拖拽排序（仅 favorite 分组）"
```

### Task 4.4: 搜索运算符 DSL

**Files:**
- Create: `src/lib/clipboardSearchParser.ts`
- Modify: `src-tauri/src/clipboard/db.rs`
- Modify: `src-tauri/src/clipboard/models.rs`
- Modify: `src/composables/useClipboardStore.ts`

- [ ] **Step 4.4.1：前端解析器**

```ts
// src/lib/clipboardSearchParser.ts
export interface ParsedSearch {
  keywords: string[];
  filters: {
    type?: string;
    from?: string;
    to?: string;
    app?: string;
    fav?: boolean;
    sizeGt?: number;
    sizeLt?: number;
  };
}

export function parseSearch(input: string): ParsedSearch {
  const result: ParsedSearch = { keywords: [], filters: {} };
  const tokens = input.split(/\s+/).filter(Boolean);
  for (const t of tokens) {
    const m = /^(\w+):(.*)$/.exec(t);
    if (m) {
      const [, key, val] = m;
      switch (key) {
        case 'type': result.filters.type = val; break;
        case 'from': result.filters.from = val; break;
        case 'to': result.filters.to = val; break;
        case 'app': result.filters.app = val; break;
        case 'fav': result.filters.fav = true; break;
        case 'size':
          if (val.startsWith('>')) result.filters.sizeGt = parseInt(val.slice(1));
          else if (val.startsWith('<')) result.filters.sizeLt = parseInt(val.slice(1));
          break;
        default: result.keywords.push(t);
      }
    } else {
      result.keywords.push(t);
    }
  }
  return result;
}
```

- [ ] **Step 4.4.2：扩展后端 `ClipboardListQuery`**

```rust
// models.rs
#[derive(Debug, Clone, Deserialize)]
pub struct ClipboardListQuery {
    pub filter: ClipboardFilter,
    pub search: String,              // 保留用于向后兼容
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub op_type: Option<String>,
    #[serde(default)]
    pub op_from_ms: Option<i64>,
    #[serde(default)]
    pub op_to_ms: Option<i64>,
    #[serde(default)]
    pub op_app: Option<String>,
    #[serde(default)]
    pub op_fav_only: bool,
    #[serde(default)]
    pub op_size_gt: Option<i64>,
    #[serde(default)]
    pub op_size_lt: Option<i64>,
    pub offset: i64,
    pub limit: i64,
}
```

- [ ] **Step 4.4.3：扩展 `build_where` 支持新运算符**

在 db.rs 的 `build_where` 中按各新字段追加 `clauses` + `params`（参数化绑定，SQL 注入安全）。

- [ ] **Step 4.4.4：前端 composable 调用解析器**

```ts
// useClipboardStore.ts 中
import { parseSearch } from '@/lib/clipboardSearchParser';

async function reload() {
  const p = parseSearch(search.value);
  const fromMs = p.filters.from ? new Date(p.filters.from).getTime() : null;
  const toMs = p.filters.to ? new Date(p.filters.to).getTime() : null;

  const r = await clipboardApi.list({
    filter: filter.value,
    search: p.keywords.join(' '),
    keywords: p.keywords,
    op_type: p.filters.type ?? null,
    op_from_ms: fromMs,
    op_to_ms: toMs,
    op_app: p.filters.app ?? null,
    op_fav_only: !!p.filters.fav,
    op_size_gt: p.filters.sizeGt ?? null,
    op_size_lt: p.filters.sizeLt ?? null,
    offset: 0, limit: 200,
  });
  // ...
}
```

注意 TS 类型需要同步扩展 `ClipboardListQuery`。

- [ ] **Step 4.4.5：单元测试（前端）**

```ts
// src/lib/clipboardSearchParser.test.mjs
import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { parseSearch } from './clipboardSearchParser';

test('parses type operator', () => {
  const r = parseSearch('type:image hello');
  assert.equal(r.filters.type, 'image');
  assert.deepEqual(r.keywords, ['hello']);
});

test('parses size operator', () => {
  const r = parseSearch('size:>1024');
  assert.equal(r.filters.sizeGt, 1024);
});

test('plain keywords', () => {
  const r = parseSearch('react hook');
  assert.deepEqual(r.keywords, ['react', 'hook']);
});
```

（参考项目已有 .test.mjs 规范，用 node:test。）

- [ ] **Step 4.4.6：Commit**

```bash
git add src/lib/clipboardSearchParser.ts src/lib/clipboardSearchParser.test.mjs src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/models.rs src/composables/useClipboardStore.ts src/lib/tauri.ts
git commit -m "feat(clipboard/M4): 搜索运算符 DSL（type/from/to/app/fav/size）"
```

### Task 4.5: 批量操作（管理页）

**Files:**
- Modify: `src/pages/ClipboardManagerPage.vue`
- Create: `src/components/clipboard/ClipboardStats.vue`

- [ ] **Step 4.5.1：管理页增加复选框列 + 工具栏**

```vue
<script setup lang="ts">
const selectedIds = ref<Set<number>>(new Set());

function toggleSelect(id: number) {
  if (selectedIds.value.has(id)) selectedIds.value.delete(id);
  else selectedIds.value.add(id);
}

async function batchDelete() {
  if (!confirm(t('clipboard.actions.batchDeleteConfirm', { n: selectedIds.value.size }))) return;
  await clipboardApi.deleteBatch([...selectedIds.value]);
  selectedIds.value.clear();
  await store.reload();
}

async function batchFavorite() {
  for (const id of selectedIds.value) await clipboardApi.toggleFavorite(id);
  selectedIds.value.clear();
  await store.reload();
}
</script>
```

在列表每项前加 checkbox，顶部加操作栏。

- [ ] **Step 4.5.2：ClipboardStats 组件**

```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { clipboardApi } from '@/lib/tauri';

const stats = ref<{ total: number; db_bytes: number; image_count: number; images_bytes: number } | null>(null);

async function reload() { stats.value = await clipboardApi.stats(); }

onMounted(reload);
defineExpose({ reload });
</script>

<template>
  <div v-if="stats" class="grid grid-cols-3 gap-3">
    <div class="rounded-xl border border-slate-200 bg-white p-4">
      <div class="text-xs text-slate-500">{{ $t('clipboard.stats.totalItems') }}</div>
      <div class="mt-1 text-xl font-bold">{{ stats.total }}</div>
    </div>
    <div class="rounded-xl border border-slate-200 bg-white p-4">
      <div class="text-xs text-slate-500">{{ $t('clipboard.stats.dbSize') }}</div>
      <div class="mt-1 text-xl font-bold">{{ (stats.db_bytes / 1024 / 1024).toFixed(2) }} MB</div>
    </div>
    <div class="rounded-xl border border-slate-200 bg-white p-4">
      <div class="text-xs text-slate-500">{{ $t('clipboard.stats.imageCount') }}</div>
      <div class="mt-1 text-xl font-bold">{{ stats.image_count }} · {{ (stats.images_bytes / 1024 / 1024).toFixed(2) }} MB</div>
    </div>
  </div>
</template>
```

- [ ] **Step 4.5.3：后端 cb_stats**

```rust
#[tauri::command]
pub fn cb_stats(state: State<'_, AppState>) -> Result<ClipboardStats, String> {
    let conn = state.clipboard.db.lock();
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let image_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM clipboard_items WHERE kind='image'",
        [], |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    let db_path = state.clipboard.db_path_for_stats();
    let db_bytes = std::fs::metadata(&db_path).map(|m| m.len() as i64).unwrap_or(0);

    let images_bytes: i64 = std::fs::read_dir(&state.clipboard.image_dir)
        .map(|rd| rd.filter_map(|r| r.ok())
            .filter_map(|d| d.metadata().ok().map(|m| m.len() as i64))
            .sum::<i64>())
        .unwrap_or(0);

    Ok(ClipboardStats { total, db_bytes, image_count, images_bytes })
}
```

在 `ClipboardState` 增加 `db_path` 字段以便 stats 读取文件大小（已经有 `image_dir`，类似增加 `db_path: PathBuf`）。

- [ ] **Step 4.5.4：Commit**

```bash
git add src/pages/ClipboardManagerPage.vue src/components/clipboard/ClipboardStats.vue src-tauri/src/clipboard/commands.rs src-tauri/src/clipboard/mod.rs
git commit -m "feat(clipboard/M4): 批量操作 + 数据统计卡片"
```

### Task 4.6: 设置面板

**Files:**
- Create: `src/components/clipboard/ClipboardSettingsPanel.vue`
- Create: `src/components/clipboard/ClipboardHotkeyInput.vue`
- Modify: `src-tauri/src/clipboard/commands.rs` (cb_get_settings / cb_save_settings)
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/ClipboardManagerPage.vue`

- [ ] **Step 4.6.1：后端 cb_get_settings / cb_save_settings**

```rust
#[tauri::command]
pub fn cb_get_settings(state: State<'_, AppState>) -> ClipboardSettings {
    state.clipboard.settings.read().clone()
}

#[tauri::command]
pub fn cb_save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: ClipboardSettings,
) -> Result<ClipboardSettings, String> {
    // 若 hotkey 变化，重新注册
    let old = state.clipboard.settings.read().clone();
    if settings.hotkey != old.hotkey {
        crate::clipboard::hotkey::change(app.clone(), &state.clipboard.hotkey_handle, &settings.hotkey)?;
    }
    // 若 enabled 变化
    if settings.enabled && !old.enabled { state.clipboard.enable(app.clone()); }
    if !settings.enabled && old.enabled { state.clipboard.disable(); }

    *state.clipboard.settings.write() = settings.clone();

    // 同步到 config.json
    let mut cfg = state.config.lock();
    cfg.clipboard = settings.clone();
    crate::config::save(&cfg).map_err(|e| e.to_string())?;

    Ok(settings)
}
```

（`state.config` 是现有 AppState.config；参考 save_config_cmd 写法。）

- [ ] **Step 4.6.2：前端设置面板组件**

（按 spec §6.3 管理页布局草图）

```vue
<template>
  <div class="space-y-5 rounded-2xl border border-slate-200 bg-white p-5">
    <h3 class="text-lg font-semibold">{{ t('clipboard.settings.sectionBasic') }}</h3>

    <label class="flex items-center justify-between">
      <span>{{ t('clipboard.settings.enableLabel') }}</span>
      <input type="checkbox" v-model="model.enabled" @change="save" />
    </label>

    <label class="flex items-center gap-3">
      <span>{{ t('clipboard.settings.hotkeyLabel') }}</span>
      <ClipboardHotkeyInput v-model="model.hotkey" @change="save" />
    </label>

    <!-- 其他字段类似 -->
    <h3 class="text-lg font-semibold">{{ t('clipboard.settings.sectionSystem') }}</h3>

    <label class="block">
      <span class="text-orange-600 flex items-center gap-1 text-sm">
        ⚠️ {{ t('clipboard.settings.winVWarning') }}
      </span>
      <div class="flex items-center justify-between mt-1">
        <span>{{ t('clipboard.settings.winVLabel') }}</span>
        <input type="checkbox" v-model="model.use_win_v_replacement" @change="onWinVToggle" />
      </div>
    </label>

    <!-- ... -->
  </div>
</template>
```

具体 UI 细节按 spec §6.3 实现。

- [ ] **Step 4.6.3：快捷键录制输入框**

```vue
<script setup lang="ts">
const model = defineModel<string>({ required: true });
const emit = defineEmits<{ change: [] }>();

const recording = ref(false);
const display = computed(() => recording.value ? '按下快捷键...' : model.value);

function onKeyDown(e: KeyboardEvent) {
  if (!recording.value) return;
  e.preventDefault();
  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  if (e.metaKey) parts.push('Meta');
  if (e.key.length === 1 || ['F1','F2','F3','F4','F5','F6','F7','F8','F9','F10','F11','F12'].includes(e.key)) {
    parts.push(e.key.toUpperCase());
    model.value = parts.join('+');
    recording.value = false;
    emit('change');
  }
}
</script>
<template>
  <input
    readonly
    :value="display"
    @click="recording = true"
    @keydown="onKeyDown"
    class="rounded border border-slate-300 px-3 py-1 text-sm w-32"
  />
</template>
```

- [ ] **Step 4.6.4：Commit**

```bash
git add src/components/clipboard/ClipboardSettingsPanel.vue src/components/clipboard/ClipboardHotkeyInput.vue src-tauri/src/clipboard/commands.rs src/lib/tauri.ts src/pages/ClipboardManagerPage.vue
git commit -m "feat(clipboard/M4): 设置面板（基础/快捷键/数据/系统集成占位）"
```

### M4 · 里程碑验证

- [ ] **Step M4.V1：性能 & UI 验证**

- 1000+ 条记录滚动 ≥ 50fps
- 悬浮预览 + Ctrl+滚轮缩放
- 收藏拖拽持久化
- 搜索 `type:image from:2026-04-01` 正确过滤
- 批量删除 + 数据统计一致
- 设置修改立即生效并持久化

- [ ] **Step M4.V2：构建 & tag**

```bash
cmd /c pnpm tauri:build:versioned-exe
git tag m4-enhanced-ux-done
```

---

## M5 · Win+V 替代 + 管理员 + 收尾

### Task 5.1: Win+V 替代

**Files:**
- Modify: `src-tauri/src/clipboard/win_v.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Create: `src/components/clipboard/ClipboardWinVConfirmDialog.vue`
- Create: `scripts/restore-win-v.ps1`

- [ ] **Step 5.1.1：实现 win_v.rs**

按 spec §8.5 完整实现 3 步操作 + 失败回滚：

```rust
#[cfg(target_os = "windows")]
use windows::Win32::System::Registry::*;

pub fn enable_win_v_replacement() -> Result<(), String> {
    write_disable_clipboard_history(0)?;
    restart_explorer()
        .map_err(|e| {
            // 回滚注册表
            let _ = delete_allow_clipboard_history();
            format!("restart explorer: {e}")
        })?;
    Ok(())
}

pub fn disable_win_v_replacement() -> Result<(), String> {
    delete_allow_clipboard_history()?;
    restart_explorer()?;
    Ok(())
}

pub fn is_win_v_replacement_enabled() -> bool {
    // 读取 HKCU AllowClipboardHistory 值
    read_allow_clipboard_history().map(|v| v == 0).unwrap_or(false)
}

fn write_disable_clipboard_history(value: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer"),
            0, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE,
            None, &mut key, None,
        ).ok().map_err(|e| e.to_string())?;
        let bytes = value.to_le_bytes();
        RegSetValueExW(
            key, w!("AllowClipboardHistory"), 0, REG_DWORD,
            Some(&bytes),
        ).ok().map_err(|e| e.to_string())?;
        RegCloseKey(key).ok().ok();
    }
    Ok(())
}

fn delete_allow_clipboard_history() -> Result<(), String> {
    // 类似上面，RegDeleteValueW
    Ok(())
}

fn read_allow_clipboard_history() -> Option<u32> {
    // RegQueryValueExW
    None
}

fn restart_explorer() -> Result<(), String> {
    use std::process::Command;
    Command::new("taskkill").args(["/IM", "explorer.exe", "/F"])
        .status().map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    Command::new("explorer.exe").spawn().map_err(|e| e.to_string())?;
    Ok(())
}
```

- [ ] **Step 5.1.2：Commands**

```rust
#[tauri::command]
pub fn cb_enable_win_v(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    crate::clipboard::win_v::enable_win_v_replacement()?;
    // 重新注册快捷键为 Win+V
    crate::clipboard::hotkey::change(app, &state.clipboard.hotkey_handle, "Win+V")?;
    state.clipboard.settings.write().use_win_v_replacement = true;
    Ok(())
}

#[tauri::command]
pub fn cb_disable_win_v(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    crate::clipboard::win_v::disable_win_v_replacement()?;
    let hotkey = state.clipboard.settings.read().hotkey.clone();
    crate::clipboard::hotkey::change(app, &state.clipboard.hotkey_handle, &hotkey)?;
    state.clipboard.settings.write().use_win_v_replacement = false;
    Ok(())
}

#[tauri::command]
pub fn cb_is_win_v_enabled() -> bool {
    crate::clipboard::win_v::is_win_v_replacement_enabled()
}
```

- [ ] **Step 5.1.3：双重确认对话框 UI**

按 spec §8.5 用户保护层清单实现：
- 列出 3 步操作
- 强调"所有资源管理器窗口会关闭"
- 勾选 "我已了解并同意" 才能点"继续"

- [ ] **Step 5.1.4：restore-win-v.ps1 脚本**

```powershell
# scripts/restore-win-v.ps1
# 紧急恢复脚本：移除自定义的 Win+V 替代，让系统剪贴板历史恢复。
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer" `
                    -Name "AllowClipboardHistory" -ErrorAction SilentlyContinue
Stop-Process -Name explorer -Force
Start-Sleep -Milliseconds 500
Start-Process explorer
Write-Host "Win+V 恢复完成" -ForegroundColor Green
```

- [ ] **Step 5.1.5：打包脚本进 release**

修改 `tauri.conf.json` `resources` 加入：

```json
"resources": {
  "scripts/restore-win-v.ps1": "scripts/restore-win-v.ps1"
}
```

- [ ] **Step 5.1.6：Commit**

```bash
git add src-tauri/src/clipboard/win_v.rs src-tauri/src/clipboard/commands.rs src/components/clipboard/ClipboardWinVConfirmDialog.vue scripts/restore-win-v.ps1 src-tauri/tauri.conf.json
git commit -m "feat(clipboard/M5): Win+V 替代（注册表 + explorer 重启 + 双重确认 + 恢复脚本）"
```

### Task 5.2: 管理员启动

**Files:**
- Modify: `src-tauri/src/clipboard/admin.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src/components/clipboard/ClipboardSettingsPanel.vue`

- [ ] **Step 5.2.1：实现 admin.rs**

```rust
#[cfg(target_os = "windows")]
pub fn is_elevated() -> bool {
    use windows::Win32::Security::*;
    use windows::Win32::System::Threading::*;
    use windows::Win32::Foundation::HANDLE;

    unsafe {
        let mut token: HANDLE = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token, TokenElevation,
            Some(&mut elev as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ).is_ok() && elev.TokenIsElevated != 0;
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok
    }
}

pub fn set_autostart_as_admin(exe_path: &str, enable: bool) -> Result<(), String> {
    // 写 HKCU\...\Run
    let reg_value = if enable {
        format!(
            r#"powershell -WindowStyle Hidden -Command "Start-Process -FilePath '{}' -Verb RunAs""#,
            exe_path
        )
    } else {
        exe_path.to_string()
    };
    // 通过 windows::Win32::System::Registry API 写入
    // ...
    Ok(())
}
```

- [ ] **Step 5.2.2：Commands + 配置项联动**

```rust
#[tauri::command]
pub fn cb_is_elevated() -> bool { admin::is_elevated() }

// cb_save_settings 内若 run_as_admin 变化，调用 admin::set_autostart_as_admin
```

- [ ] **Step 5.2.3：设置面板显示徽章**

```vue
<span :class="isElevated ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-100 text-slate-600'">
  {{ isElevated ? t('clipboard.settings.adminCurrentStatusElevated') : t('clipboard.settings.adminCurrentStatusNormal') }}
</span>
```

- [ ] **Step 5.2.4：Commit**

```bash
git add src-tauri/src/clipboard/admin.rs src-tauri/src/clipboard/commands.rs src/components/clipboard/ClipboardSettingsPanel.vue
git commit -m "feat(clipboard/M5): 管理员权限检测 + 以管理员自启动配置"
```

### Task 5.3: i18n 完整中英 + 空状态 + 错误处理

**Files:**
- Modify: `src/locales/messages.ts`
- Modify: 各 Vue 页面/组件补全 i18n

- [ ] **Step 5.3.1：按 spec §9.7 补齐所有 clipboard 键位（中英）**

spec 已列出全部需要的键（tool / panel / filter / search / actions / settings / stats / notification / errors），对照逐一补齐 zh + en。

- [ ] **Step 5.3.2：验证所有 Vue 组件都通过 `t('clipboard.xxx')` 访问**

```bash
pnpm grep -r "clipboard\." src/components/clipboard src/pages/Clipboard* 2>&1 | grep -v "t('clipboard" | head -20
```

若发现硬编码文字，替换为 i18n。

- [ ] **Step 5.3.3：错误处理 toasts**

在 `useClipboardStore.ts` 的各 catch 中用 `alert(t('clipboard.errors.xxx'))` 或现有 toast 机制。

- [ ] **Step 5.3.4：Commit**

```bash
git add src/locales/messages.ts src/pages src/components/clipboard
git commit -m "feat(clipboard/M5): i18n 完整中英 + 错误提示"
```

### Task 5.4: 收尾 - 清理、lint、构建

**Files:** 全局清理。

- [ ] **Step 5.4.1：cargo clippy 零警告**

```bash
cd src-tauri && cargo clippy --all-targets 2>&1 | tail -20
```

按提示修复每条 warning。

- [ ] **Step 5.4.2：cargo fmt**

```bash
cd src-tauri && cargo fmt
git diff --stat  # 验证
```

- [ ] **Step 5.4.3：pnpm check**

```bash
pnpm check 2>&1 | tail -5
```

- [ ] **Step 5.4.4：版本升级 1.0.6 → 1.0.7**

修改 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 三处版本号为 `1.0.7`。

- [ ] **Step 5.4.5：最终构建**

```bash
cd d:/WorkSpace/File-Sync-Tool-clipboard
cmd /c pnpm tauri:build:versioned-exe
```

Expected: `file-sync-tool-1.0.7-YYYYMMDDHHmm.exe` 成功生成。

- [ ] **Step 5.4.6：运行产物手动 E2E（覆盖 spec §12 全部验收项）**

照 spec §12 验收清单逐项勾选：
- [ ] Win10 / Win11 各一次
- [ ] 文本 / HTML / 图片 / 文件 复制粘贴
- [ ] 粘贴到 VSCode / Chrome / Word / 记事本 / 任务管理器（管理员）
- [ ] max_items=10 容量测试
- [ ] 关闭应用 + 重启系统保留数据
- [ ] Win+V 启用/禁用 3 次
- [ ] `cargo clippy` / `cargo fmt` 零差异

- [ ] **Step 5.4.7：Commit**

```bash
git add -u
git commit -m "chore(clipboard/M5): 1.0.7 发版收尾（fmt/clippy/i18n 补全）"
```

- [ ] **Step 5.4.8：tag**

```bash
git tag m5-release-done
```

### 合入 main

- [ ] **Step 5.5.1：Push feature 分支**

```bash
git push -u origin feature/clipboard-manager
git push origin m1-skeleton-done m2-core-store-done m3-panel-hotkey-done m4-enhanced-ux-done m5-release-done
```

- [ ] **Step 5.5.2：发起 PR**

在 GitHub 上创建 PR `feature/clipboard-manager` → `main`，描述引用 spec + plan 路径。

- [ ] **Step 5.5.3：清理 worktree（合并后）**

```bash
cd d:/WorkSpace/File-Sync-Tool
git worktree remove ../File-Sync-Tool-clipboard
git branch -d feature/clipboard-manager
```

---

## 附录：跨任务验证命令速查

| 场景 | 命令 |
|---|---|
| Rust 编译检查 | `cd src-tauri && cargo check` |
| Rust 单元测试 | `cd src-tauri && cargo test -p app_lib clipboard` |
| Rust 格式化 | `cd src-tauri && cargo fmt` |
| Rust lint | `cd src-tauri && cargo clippy --all-targets` |
| 前端类型检查 | `pnpm check` |
| 前端测试 | `pnpm test` 或 `node --test src/**/*.test.mjs` |
| 开发模式 | `pnpm tauri dev` |
| 生产构建+改名 | `cmd /c pnpm tauri:build:versioned-exe` |

---

## 自检记录

- [x] 覆盖 spec §2.1 所有目标
- [x] 每个 feature（C01-C10 / E01-E07 / S01-S05）都有对应任务
- [x] 每步有明确代码/命令/预期
- [x] 每个里程碑有验证步骤
- [x] 15 工作日估算与 spec 一致
- [x] 合入 main 前有明确的构建与 E2E 验证

## 备注

- 每步预留 2-5 分钟颗粒；复杂步骤（如 watcher 实现）可能超出，整体节奏控制到每天 ~5-10 task steps。
- 测试覆盖：Rust 后端侧重单元测试（db/retention/image_store），前端侧重解析器测试；UI 层依赖手动 E2E（Tauri 桌面特性难以自动化）。
- Win+V 和管理员启动由于高风险，对应任务务必在干净 VM 或快照环境下先测。
