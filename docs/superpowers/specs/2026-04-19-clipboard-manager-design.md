# 剪贴板管理器集成设计（移植自 ElegantClipboard）

- 日期：2026-04-19
- 作者：codex-agent
- 状态：草案 → 待用户评审
- 参考：https://github.com/Y-ASLant/ElegantClipboard （MIT License，Tauri 2.0 + React + Rust）
- 目标仓库：`d:\WorkSpace\File-Sync-Tool`
- 目标分支：`feature/clipboard-manager`（通过 git worktree 隔离）

---

## 1. 背景与动机

File-Sync-Tool 的工具中心（`/tools`）目前聚合了 SSH 开启、框架密码、网络工具、屏幕共享、文件共享等能力。用户希望再加入一个**本地剪贴板管理**工具，解决：

- Windows 自带剪贴板（Win+V）唤出卡顿
- 记录条目有上限、无分类、无分组、无收藏、无法简单编辑
- 重启设备后剪贴板自动清空
- 缺少自定义信息显示

开源项目 [ElegantClipboard](https://github.com/Y-ASLant/ElegantClipboard) 已经针对上述痛点提供了成熟实现，且使用 Tauri 2.0 + Rust，与本项目技术栈高度兼容。前端层（React 19）需重写为 Vue 3，后端层（Rust）基本可直接迁移。

经过方案比对，决定采用**选项 A：深度完整移植** + **A3：双模式混合（弹出面板 + 管理后台）** + **C1：SQLite 存储** + **B3：默认启用可关闭**。

---

## 2. 目标与非目标

### 2.1 目标（In-scope）

1. 在 `/tools/clipboard` 工具卡片下新增"剪贴板管理"工具
2. 实现与 ElegantClipboard 对标的核心功能：监听 / 存储 / 搜索 / 分组 / 收藏 / 快捷键 / 粘贴 / 预览 / 拖拽排序 / 虚拟列表
3. 同时提供**独立弹出面板**（类 Win+V，由全局快捷键唤出）与**管理后台**（主窗口内页面）
4. 数据完全本地 SQLite 持久化，重启不丢
5. 支持 Win+V 系统剪贴板替代（带双重确认与自动回滚）
6. 支持以管理员权限启动，以便向高权限窗口粘贴
7. 与现有 i18n（zh/en）与浅色视觉体系完全集成
8. 隔离开发：feature 分支 + git worktree，独立合入 main

### 2.2 非目标（Out-of-scope，留到二期）

- 主题色（跟随系统 / 经典黑白 / 翡翠绿 / 天空青）多配色方案
- 暗色主题（等主窗口整体暗色方案统一再做）
- 自动更新（沿用 File-Sync-Tool 自己的发布节奏）
- 跨平台（本项目仅 Windows）

---

## 3. 功能清单（对标参考项目）

### 3.1 核心能力（M2-M3 必做）

| # | 功能 | 说明 |
|---|---|---|
| C01 | 剪贴板监听 | 文本 / HTML 富文本 / 图片（PNG）/ 文件路径四类 |
| C02 | SQLite 历史存储 | 所有记录本地持久化，重启不丢；图片以独立文件形式落盘，DB 仅存引用 |
| C03 | BLAKE3 内容去重 | 同一内容命中时更新时间戳"顶置"而非新增行 |
| C04 | 搜索 | LIKE 查询（支持 CJK）+ 运算符 DSL：`type:`/`from:`/`to:`/`app:`/`fav:`/`size:` |
| C05 | 分组标签 | 全部 / 文本 / 图片 / 文件 / 收藏 5 个 tab |
| C06 | 收藏与钉选 | `is_favorite` + `favorite_sort_index`（用于自定义排序） |
| C07 | 全局快捷键 | 默认 `Alt+C` 唤出/隐藏弹出面板；用户可自定义 |
| C08 | 粘贴模拟 | `Enter` = 原样粘贴；`Shift+Enter` = 纯文本粘贴；通过 `enigo` 模拟 Ctrl+V |
| C09 | 删除与清空 | 单条删除 / 批量删除 / 清空全部（带确认） |
| C10 | 容量限制 | 最大条数 / 保留天数 / 单条字节数上限；过期自动 GC（收藏豁免） |

### 3.2 交互增强（M4 必做）

| # | 功能 | 说明 |
|---|---|---|
| E01 | 虚拟列表 | 使用 `vue-virtual-scroller`，支撑万级记录滚动 ≥50fps |
| E02 | 图片悬浮预览 | 500ms 后弹出大图；`Ctrl+滚轮` 缩放 50%-500%，右下角显示百分比 |
| E03 | 文本悬浮预览 | 默认关闭；开启后等宽字体展示完整文本，`Ctrl+滚轮` 滚动 |
| E04 | 拖拽排序 | 仅在"收藏"分组启用；`vue-draggable-plus`；结果写回 `favorite_sort_index` |
| E05 | 窗口内快捷键 | `↑↓` 选择 / `←→` 切换 tab / `Enter` 粘贴 / `Shift+Enter` 纯文本粘贴 / `Delete` 删除 / `Ctrl+D` 收藏 / `Ctrl+F` 或 `/` 聚焦搜索 / `Esc` 清空搜索或关闭 |
| E06 | 批量操作（仅管理页） | 勾选多条后：批量删除 / 批量收藏 / 批量取消收藏 / 导出 JSON |
| E07 | 数据统计卡片 | 总大小 / 数据库大小 / 图片数量 / 总条数 |

### 3.3 系统集成（M5 必做）

| # | 功能 | 说明 |
|---|---|---|
| S01 | Win+V 系统剪贴板替代 | 写 `HKCU\...\Explorer\AllowClipboardHistory=0` + 重启 `explorer.exe` + 注册 Win+V 全局快捷键；带双重确认与失败自动回滚 |
| S02 | 启动通知 Toast | `tauri-plugin-notification`，应用启动 500ms 后提示"剪贴板监听已启动，按 Alt+C 呼出面板" |
| S03 | 管理员权限启动 | 运行时检测是否已提权；设置项开关写入开机自启（PowerShell `Start-Process -Verb RunAs`，二期优化为 Task Scheduler） |
| S04 | Mica/Acrylic 窗口特效 | 弹出面板应用 `window-vibrancy`（Win11 Mica，Win10 Acrylic 降级） |
| S05 | i18n 中英双语 | 所有文本通过 `t('clipboard.xxx')`；`src/locales/messages.ts` 新增 `clipboard` 命名空间 |

### 3.4 明确不做（当前版本）

- Win+V 替代的跨用户/系统级部署（仅当前用户 HKCU）
- 自动更新检查（特性 18）
- 多主题色（特性 14）

---

## 4. 架构设计

### 4.1 进程与窗口拓扑

```
File-Sync-Tool 主进程（tauri-plugin-single-instance 保持单实例）
├── 主窗口 MainWindow（现有）
│   └── /tools/clipboard        ← 管理后台
│
├── 剪贴板弹出面板 ClipboardPanelWindow（新增）
│   ├── label = "clipboard-panel"
│   ├── 无边框 / 420×720 / skip_taskbar / always_on_top / transparent
│   ├── URL = "index.html#/clipboard-panel"
│   ├── Mica/Acrylic 特效
│   └── 失焦自动 hide（不 close，保持热启动）
│
└── Rust 后端
    ├── AppState.clipboard: Arc<ClipboardState>     ← 新字段
    ├── 剪贴板监听线程（clipboard-master，独立 Win32 消息循环）
    ├── 全局快捷键注册（tauri-plugin-global-shortcut）
    ├── 图片存储目录：%APPDATA%\<app>\app_data\clipboard_images\
    └── SQLite 数据库：%APPDATA%\<app>\app_data\clipboard.db
```

### 4.2 数据流

**监听 → 存储 → UI**：

```
Win32 clipboard event
  → clipboard-master 回调（独立线程）
  → arboard 读取内容（text / html / image / files）
  → BLAKE3(content_bytes) 生成 hash
  → parking_lot::Mutex 获取写锁
  → 若 hash 存在：UPDATE updated_at
    否则：图片落盘 + INSERT 新行
  → Tauri emit("clipboard-item-added", ClipboardItem)
  → 两个窗口的前端监听器各自更新列表
```

**面板显示 → 粘贴**：

```
用户按 Alt+C
  → tauri-plugin-global-shortcut 触发
  → toggle_panel(app)
    若已显示：hide
    否则：根据光标位置设置 panel.set_position + show + set_focus
  → emit("clipboard-panel-shown")，前端重置搜索与焦点

用户按 Enter（面板内）
  → invoke('cb_paste', id)
  → Rust: arboard 写入系统剪贴板
       → panel.hide()
       → sleep(30ms) 等焦点回到目标窗口
       → enigo: Ctrl+V 模拟
  → 目标应用收到粘贴
```

### 4.3 关键设计原则

1. **单进程、双窗口**：不新开子进程。两个 WebviewWindow 共享同一个 Rust `AppState`、同一个 SQLite 连接池。
2. **弹出面板是"瘦客户端"**：只做列表 / 搜索 / 粘贴；管理功能（清空、导出、设置、Win+V 开关）一律在主窗口 `/tools/clipboard` 页。
3. **事件广播同步**：前端不搞跨窗口状态共享，两个窗口都订阅同一组 Tauri 事件自更新。
4. **常驻监听**：设置 `enabled=true` 时应用启动即监听；`enabled=false` 时完全释放监听线程与快捷键。

---

## 5. Rust 后端模块划分

### 5.1 文件结构

```
src-tauri/src/
├── main.rs              # 扩展：AppState 新增字段、注册 cb_* commands、创建 clipboard-panel 窗口
├── config.rs            # 扩展：AppConfig 新增 ClipboardSettings 子结构
├── clipboard/           # 新增模块（全部剪贴板逻辑）
│   ├── mod.rs           # 模块入口 + ClipboardState 结构体
│   ├── models.rs        # ClipboardItem / ContentKind / ClipboardFilter / ClipboardSettings
│   ├── db.rs            # rusqlite 连接池、schema 初始化与迁移、CRUD
│   ├── watcher.rs       # clipboard-master 监听 + arboard 读取 + BLAKE3 去重
│   ├── image_store.rs   # 图片文件落盘与 GC
│   ├── hotkey.rs        # 全局快捷键注册 / 注销 / 切换
│   ├── paste.rs         # enigo 粘贴逻辑 + 焦点恢复
│   ├── win_v.rs         # Win+V 替代（注册表 + explorer 重启），Windows only
│   ├── admin.rs         # 权限检测 + 管理员自启动配置
│   └── commands.rs      # 所有 tauri commands（18 个，cb_ 前缀）
└── scanner.rs / deploy.rs / history.rs   # 现有模块，不动
```

### 5.2 `ClipboardState` 结构

```rust
pub struct ClipboardState {
    db: Arc<parking_lot::Mutex<rusqlite::Connection>>,
    watcher_handle: parking_lot::Mutex<Option<WatcherHandle>>,
    hotkey_handle: parking_lot::Mutex<Option<HotkeyHandle>>,
    image_dir: PathBuf,
    is_enabled: AtomicBool,
    last_hash: parking_lot::Mutex<Option<[u8; 32]>>,   // 防抖：同一内容短时间内多次事件
    settings: Arc<parking_lot::RwLock<ClipboardSettings>>,
}
```

挂载到现有 `AppState`：

```rust
struct AppState {
    // ... 现有字段 ...
    clipboard: Arc<clipboard::ClipboardState>,
}
```

### 5.3 Tauri Commands 清单（18 个）

| 分类 | Command | 入参 | 返回 |
|---|---|---|---|
| 开关 | `cb_enable` | — | `()` |
| | `cb_disable` | — | `()` |
| | `cb_is_enabled` | — | `bool` |
| 查询 | `cb_list` | `ClipboardListQuery` | `ClipboardListResult` |
| | `cb_get` | `id: i64` | `ClipboardItem` |
| | `cb_get_image_path` | `id: i64` | `Option<String>` |
| 写入 | `cb_add_manual` | `text: String` | `ClipboardItem` |
| 管理 | `cb_delete` | `id: i64` | `()` |
| | `cb_delete_batch` | `ids: Vec<i64>` | `()` |
| | `cb_clear` | `{ keep_favorites: bool }` | `u64` （删除数） |
| | `cb_toggle_favorite` | `id: i64` | `ClipboardItem` |
| | `cb_reorder_favorites` | `ids: Vec<i64>` | `()` |
| 粘贴 | `cb_paste` | `id: i64` | `()` |
| | `cb_paste_plain` | `id: i64` | `()` |
| 设置 | `cb_get_settings` | — | `ClipboardSettings` |
| | `cb_save_settings` | `settings: ClipboardSettings` | `ClipboardSettings` |
| | `cb_set_hotkey` | `hotkey: String` | `ClipboardSettings` |
| Win+V | `cb_enable_win_v` | — | `()` |
| | `cb_disable_win_v` | — | `()` |
| | `cb_is_win_v_enabled` | — | `bool` |
| 面板 | `cb_toggle_panel` | — | `()` |
| 统计 | `cb_stats` | — | `{ total, db_bytes, image_count, images_bytes }` |
| 导出 | `cb_export_json` | `ids: Option<Vec<i64>>` | `String` （路径） |

### 5.4 新增 Cargo 依赖

```toml
# 数据库
rusqlite = { version = "0.32", features = ["bundled", "blob"] }

# 剪贴板
clipboard-master = "4"
arboard = "3"

# 全局快捷键
tauri-plugin-global-shortcut = "2"

# 通知
tauri-plugin-notification = "2"

# 粘贴模拟
enigo = "0.2"

# 工具
parking_lot = "0.12"
blake3 = "1"
rayon = "1"

# Windows 窗口特效
window-vibrancy = "0.5"

# windows crate 已存在，扩展 features：
# "Win32_System_Registry", "Win32_System_Threading",
# "Win32_System_ProcessStatus", "Win32_Security"
```

**体积影响**：约 +3-5MB（主要是 rusqlite bundled SQLite）。

### 5.5 与现有代码的交互

1. `AppState` 新增 `clipboard` 字段，初始化在 `main.rs::setup`
2. `config.rs::AppConfig` 新增 `clipboard: ClipboardSettings` 字段；旧配置迁移时填默认值（enabled=true，hotkey="Alt+C" 等）
3. `tauri::generate_handler![]` 追加 cb_* 系列
4. `is_quitting=true` 时先 `clipboard.shutdown()` 释放监听线程，避免僵尸
5. **保留** `tauri-plugin-clipboard-manager`：`src/pages/SettingsPage.vue` 使用其 `writeText` 做路径复制按钮，不下线该插件；新增的 `arboard` 只用于剪贴板管理器模块内部，两者职责分离（插件负责前端简单写入，arboard 负责监听与多类型读写）
6. `close_to_tray=true` 时，隐藏主窗口但保留剪贴板监听（二者独立）

### 5.6 Rust 侧已知风险

- **clipboard-master 消息循环兼容性**：该库在独立线程跑 Win32 消息循环；初始化时机必须晚于 Tauri event loop 就绪；关闭时需 `PostQuitMessage` 优雅退出。
- **rusqlite 并发**：两个窗口可能同时读；开启 WAL 模式 + 单 writer 锁解决。
- **enigo 与 UIPI**：向管理员权限进程（如任务管理器）发送键盘事件需要自身也是管理员；这是 S03 管理员自启动的主要原因。

---

## 6. 前端模块划分

### 6.1 文件结构

```
src/
├── main.ts                              # 扩展：按 window.label 选择路由入口（也可走同入口 + 路由匹配）
├── router/index.ts                      # 扩展：
│                                        #   /tools/clipboard → ClipboardManagerPage
│                                        #   /clipboard-panel → ClipboardPanelPage（不套 DefaultLayout）
├── App.vue                              # 扩展：根据路由决定是否套 Sidebar
├── pages/
│   ├── ClipboardManagerPage.vue         # 管理后台
│   ├── ClipboardPanelPage.vue           # 独立弹出面板
│   └── ToolsHubPage.vue                 # 扩展：新增"剪贴板管理"工具卡片
├── components/
│   └── clipboard/
│       ├── ClipboardList.vue            # 虚拟滚动列表容器
│       ├── ClipboardItemCard.vue        # 单条卡片（多态：text/html/image/file）
│       ├── ClipboardSearchBar.vue       # 搜索框 + 运算符提示
│       ├── ClipboardFilterTabs.vue      # 全部/文本/图片/文件/收藏 tabs
│       ├── ClipboardHoverPreview.vue    # 悬浮预览（图片/文本）
│       ├── ClipboardSettingsPanel.vue   # 设置面板（管理页）
│       ├── ClipboardHotkeyInput.vue     # 快捷键录制输入框
│       ├── ClipboardStats.vue           # 数据统计三卡片
│       └── ClipboardWinVConfirmDialog.vue  # Win+V 启用确认对话框
├── composables/
│   ├── useClipboardStore.ts             # 列表响应式状态 + invoke 封装 + 事件订阅
│   ├── useClipboardHotkey.ts            # 面板内快捷键
│   ├── useHoverPreview.ts               # 悬浮预览状态机 + 延时
│   └── useClipboardDragSort.ts          # 收藏项拖拽排序
├── lib/
│   ├── tauri.ts                         # 扩展：cb_* 命令封装 + 类型
│   └── clipboardTypes.ts                # 新增：剪贴板所有 TS 类型
└── locales/messages.ts                  # 扩展：clipboard 命名空间中英翻译
```

### 6.2 路由多入口方案

Tauri 创建两个窗口都指向同一个 dist，通过 URL 的 hash 区分：

```rust
// main.rs setup
let panel = WebviewWindowBuilder::new(
    app,
    "clipboard-panel",
    WebviewUrl::App("index.html#/clipboard-panel".into()),
)
.decorations(false)
.resizable(false)
.skip_taskbar(true)
.always_on_top(true)
.visible(false)
.inner_size(420.0, 720.0)
.transparent(true)
.build()?;

window_vibrancy::apply_mica(&panel, None).ok();  // Win11 失败则回退 Acrylic
```

`App.vue` 按路由决定布局：

```vue
<template>
  <ClipboardPanelPage v-if="$route.path === '/clipboard-panel'" />
  <DefaultLayout v-else><router-view /></DefaultLayout>
</template>
```

### 6.3 UI 风格约定

- **弹出面板**：半透明白色 + Mica 特效；`rounded-2xl`；`shadow-2xl`；紧凑密度，贴近参考项目截图
- **管理后台**：沿用 `/tools/*` 现有渐变卡片 + 圆角 + 浅色配色
- **设置面板**：折叠区块划分（基础 / 快捷键 / 数据管理 / 系统集成）
- **数据统计卡片**：3 列布局参考 ElegantClipboard "数据管理" 截图

---

## 7. 数据模型与 SQLite Schema

### 7.1 TypeScript 类型

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
  enabled: boolean;                  // 默认 true
  hotkey: string;                    // 默认 "Alt+C"
  max_items: number;                 // 默认 1000
  retain_days: number;               // 默认 30，0=不限
  max_item_bytes: number;            // 默认 10 MB
  preview_delay_ms: number;          // 默认 500
  enable_text_preview: boolean;      // 默认 false
  use_win_v_replacement: boolean;    // 默认 false
  run_as_admin: boolean;             // 默认 false
  show_startup_notification: boolean;// 默认 true
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
```

### 7.2 SQLite Schema

```sql
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
```

WAL 模式 + 外键开启：

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
```

### 7.3 去重策略

- `hash = BLAKE3(kind_bytes || content_bytes)`
  - text / html：UTF-8 字节
  - image：原始像素 RGBA 字节流
  - file：JSON 编码的路径数组字节
- 命中 UNIQUE 冲突时：`UPDATE clipboard_items SET updated_at=? WHERE hash=?`（不更新其他字段）
- 前端按 `ORDER BY COALESCE(updated_at, created_at) DESC` 排序，视觉上"重复命中的老条目自动顶置"

### 7.4 图片存储

- 目录：`%APPDATA%\<app>\app_data\clipboard_images\`
- 文件名：`<hash>.png`（首 16 字符）
- 格式：PNG 无损（用 `image` crate 从 arboard 的 RGBA buffer 编码）
- GC 策略：
  - 启动时扫描目录，对比 DB 中所有 `image_path`，删除孤儿
  - 用 `rayon::par_iter` 并行，目标 1000 张 <1s

### 7.5 容量清理

触发点：启动后 30s + 每次 INSERT 后（debounce 60s）

```
1. DELETE FROM clipboard_items
   WHERE is_favorite=0 AND created_at < (now - retain_days*86400*1000)

2. IF total_count > max_items:
   DELETE oldest non-favorite rows until count = max_items

3. 对被删除的 image_path，将图片文件标记到 GC 队列（异步删除）
```

---

## 8. 全局快捷键 · 粘贴 · Win+V 替代

### 8.1 全局快捷键

- 使用 `tauri-plugin-global-shortcut`
- 启动时读取 `settings.hotkey`，通过 `on_shortcut` 注册
- 回调内调用 `toggle_panel(app)`
- 用户修改快捷键时：先 `unregister_all` → 再 `on_shortcut` 新键 → 写配置；失败回滚并通过事件通知前端

### 8.2 弹出面板显隐

```rust
fn toggle_panel(app: &AppHandle) {
    let panel = app.get_webview_window("clipboard-panel").unwrap();
    if panel.is_visible().unwrap_or(false) {
        panel.hide().ok();
    } else {
        if let Ok(pos) = app.cursor_position() {
            let (x, y) = clamp_to_screen(pos.x, pos.y, 420, 720);
            panel.set_position(PhysicalPosition::new(x, y)).ok();
        }
        panel.show().ok();
        panel.set_focus().ok();
        panel.emit("clipboard-panel-shown", ()).ok();
    }
}
```

监听 `WindowEvent::Focused(false)` 自动 `hide()`（不 `close`，保持热启动）。

### 8.3 粘贴机制

流程：

1. 用户按 `Enter`
2. 前端 `invoke('cb_paste', id)`
3. Rust：
   - a. 读取 DB → `ClipboardItem`
   - b. `arboard.set_*()` 写入系统剪贴板（text + html 双写 / image / file list）
   - c. `panel.hide()`
   - d. `thread::sleep(Duration::from_millis(30))`
   - e. `enigo`: 模拟 `Ctrl+V`

关键细节：

- **30ms 等待**：经验值，让 Windows 焦点链切换到目标窗口。过短会粘到面板自己，过长用户感知到延迟。
- **`Shift+Enter`**：流程相同，步骤 b 只写入 text，忽略 html 字段
- **Esc / 点击外部关闭**：只 hide，不触发粘贴，剪贴板不变

### 8.4 管理员权限（S03）

**检测**：

```rust
fn is_elevated() -> bool {
    use windows::Win32::Security::*;
    use windows::Win32::System::Threading::*;
    unsafe {
        let mut token = HANDLE::default();
        if !OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).as_bool() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        ).as_bool() && elevation.TokenIsElevated != 0
    }
}
```

**以管理员启动（开机自启）**：

- 初版：HKCU\...\Run 的值前缀写 `powershell -WindowStyle Hidden -Command "Start-Process '<exe_path>' -Verb RunAs"`
- 二期：改用 Task Scheduler 创建 `LogonTrigger` + `HighestAvailable` RunLevel，避免 UAC 弹窗

**设置页显示**：当前运行权限徽章（"已提权" / "普通权限"），对标参考项目截图。

### 8.5 Win+V 替代（S01，高风险）

**启用流程**：

```rust
fn enable_win_v_replacement() -> Result<()> {
    // 1. 写 HKCU 禁用系统剪贴板历史
    registry::set_dword(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer",
        "AllowClipboardHistory",
        0,
    )?;

    // 2. 重启 explorer.exe 让注册表生效
    restart_explorer()?;

    // 3. 解绑旧快捷键，注册 Win+V
    re_register_hotkey("Win+V")?;

    Ok(())
}

fn restart_explorer() -> Result<()> {
    // 用 CreateToolhelp32Snapshot 找 explorer.exe PID
    // TerminateProcess 只杀 shell explorer（避免误杀其他 explorer 窗口）
    // CreateProcess 启动 explorer.exe
}
```

**禁用**（完全回滚）：

```rust
fn disable_win_v_replacement() -> Result<()> {
    registry::delete_value("...\\Explorer", "AllowClipboardHistory")?;
    restart_explorer()?;
    re_register_hotkey(&settings.hotkey)?;   // 回到 Alt+C 或用户自定义
    Ok(())
}
```

**用户保护层**：

1. 设置开关旁显著的 **⚠️ 橙色警告文字**："此操作会修改注册表并重启 Windows 资源管理器"
2. 点击开关弹出确认对话框：
   - 列出将执行的 3 步操作
   - 明确说明"重启资源管理器会让所有打开的资源管理器窗口关闭"
   - 要求勾选 "我已了解并同意" 后才能点"继续"
3. 启用失败自动回滚：任何一步失败都 revert 注册表 + 恢复原快捷键
4. 管理页永久显示 "当前 Win+V 状态"，用户随时可切回

**兜底**：提供 `scripts/restore-win-v.ps1` 独立脚本放进 release 包，极端情况下用户手动恢复：

```powershell
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer" `
                    -Name "AllowClipboardHistory" -ErrorAction SilentlyContinue
Stop-Process -Name explorer -Force
Start-Process explorer
```

### 8.6 启动通知（S02）

- 使用 `tauri-plugin-notification`（官方）
- 条件：`settings.enabled && settings.show_startup_notification`
- 触发：主窗口 `ready` 后 500ms
- 文本：
  - 标题：`"File-Sync-Tool 剪贴板"`（i18n key `clipboard.notification.title`）
  - 正文：`"剪贴板监听已启动，按 {{hotkey}} 呼出面板"`

---

## 9. 前端交互细节

### 9.1 搜索运算符 DSL

| 运算符 | 示例 | 语义 |
|---|---|---|
| `type:<kind>` | `type:image` | 内容类型过滤 |
| `from:<date>` | `from:2026-04-01` | 该日期之后 |
| `to:<date>` | `to:2026-04-10` | 该日期之前 |
| `app:<name>` | `app:chrome` | 来源进程名过滤 |
| `fav:` | `fav:` | 仅收藏 |
| `size:>N` / `size:<N` | `size:>1024` | 字节大小过滤 |
| 裸词 | `react hook` | `content_preview` / `content_full` LIKE AND |

- 前端解析结构化 → `{ keywords: string[], filters: {...} }`
- 传给 Rust `cb_list`，后端拼 SQL WHERE 子句（**必须参数化**，防注入）
- 不支持的运算符静默忽略

### 9.2 窗口内快捷键（仅弹出面板启用）

| 键 | 行为 |
|---|---|
| `↑` / `↓` | 列表选择（循环） |
| `←` / `→` | 切换 Filter tab |
| `Enter` | 粘贴当前项 |
| `Shift+Enter` | 纯文本粘贴 |
| `Delete` | 删除当前项 |
| `Ctrl+D` | 切换收藏 |
| `Ctrl+F` / `/` | 聚焦搜索框 |
| `Esc` | 搜索非空先清空，否则隐藏面板 |
| `Ctrl+滚轮`（悬浮预览内） | 图片缩放 / 文本滚动 |

实现：`useClipboardHotkey.ts` 统一注册 `window.addEventListener('keydown')`，根据焦点元素分发。

### 9.3 悬浮预览

- 触发：hover 列表项超过 `settings.preview_delay_ms`（默认 500ms）
- 位置：面板左侧，距离面板 12px，固定定位
- 最大尺寸：视口 60%
- 图片：`object-fit: contain`；`Ctrl+滚轮` 缩放 50%-500%；右下角显示百分比角标
- 文本：等宽字体，`white-space: pre-wrap`；仅在 `enable_text_preview=true` 时显示
- 隐藏：鼠标离开列表项且不在预览区域内 → 150ms 延迟后隐藏（防跳变闪退）

### 9.4 虚拟列表

- 库：`vue-virtual-scroller@2` 的 `DynamicScroller`（变高模式）
- 预估高度：文本 64px / 图片 120px / 文件 80px
- 性能目标：10k 条记录滚动 ≥50fps

### 9.5 拖拽排序（特性 E04）

- 库：`vue-draggable-plus`
- 启用条件：`filter === 'favorite'`，其他分组 `draggable=false`
- 完成拖拽 → 前端 computed 新顺序 → `cb_reorder_favorites(ids[])`
- Rust 事务批量更新 `favorite_sort_index`
- 管理页与面板共用同一列表组件，均支持

### 9.6 批量操作（仅管理页）

- 列表项左侧显示复选框（弹出面板不显示）
- 顶部操作栏：全选 / 反选 / 批量删除 / 批量收藏 / 批量取消 / 导出 JSON
- 批量删除前确认对话框显示 "即将删除 N 条记录"

### 9.7 i18n 键规划

```ts
clipboard: {
  tool: { title, description, chip },
  panel: { title, searchPlaceholder, empty, noMatch },
  filter: { all, text, image, file, favorite },
  search: { placeholder, operatorsHint, operatorExamples },
  actions: { paste, pastePlain, delete, favorite, unfavorite, copy,
             selectAll, clearSelection, batchDelete, batchFavorite,
             batchUnfavorite, exportJson, clearAll },
  settings: {
    sectionBasic, sectionHotkey, sectionData, sectionSystem,
    enableLabel, enableHint,
    hotkeyLabel, hotkeyPlaceholder, hotkeyConflict,
    maxItemsLabel, retainDaysLabel, maxItemBytesLabel,
    previewDelayLabel, enableTextPreviewLabel,
    winVLabel, winVWarning, winVConfirmTitle, winVConfirmBody,
                winVConfirmAgreeCheckbox, winVFailed,
    adminLabel, adminCurrentStatusElevated, adminCurrentStatusNormal,
    startupNotificationLabel,
  },
  stats: { totalItems, totalSize, dbSize, imageCount, imagesSize },
  notification: { title, body },
  errors: { hotkeyConflict, winVFailed, pasteFailed, loadFailed, dbLocked },
}
```

全部 `zh` + `en` 双语，硬性要求。

### 9.8 视觉整合要点

- 弹出面板：半透明白色 + Mica；圆角 `rounded-2xl`；投影 `shadow-2xl`
- 管理页：沿用现有 `/tools/*` 渐变卡片（浅色风格）
- 设置面板：折叠分区（基础 / 快捷键 / 数据 / 系统集成）
- 数据统计：3 列 Stat 卡片
- ⚠️ 警告色：橙色 `text-orange-500` 配合 `AlertTriangle` 图标

---

## 10. 工程流程

### 10.1 分支与 Worktree 策略

**要求**：不在 main 上实现，使用 git worktree 隔离。

**操作步骤**（实施时执行）：

```bash
# 在主仓库目录
git fetch origin
git worktree add -b feature/clipboard-manager ../File-Sync-Tool-clipboard main

# 切换到 worktree
cd ../File-Sync-Tool-clipboard

# 初次安装依赖
pnpm install
```

**位置**：`d:\WorkSpace\File-Sync-Tool-clipboard\`（主仓库同级）

**合并策略**：
- 里程碑完成后在 worktree 内自测 + 执行 `cmd /c pnpm tauri:build:versioned-exe` 验证
- 发起 PR 合回 main
- 合并完成后清理：`git worktree remove ../File-Sync-Tool-clipboard`
- 不计划 rebase；若 main 有冲突改动，在 feature 分支执行 `git merge main` 一次性处理

**Trellis 任务协同**：
- 创建 `.trellis/tasks/04-19-clipboard-manager/` 目录
- `prd.md` 引用本设计文档全文路径

### 10.2 里程碑拆分

每个里程碑结束：**git commit + `cmd /c pnpm tauri:build:versioned-exe` 构建验证**（项目硬性要求）。

#### M1 · 骨架搭建（1-2 天）

交付物：
- worktree + 分支建立
- `Cargo.toml` 新依赖增补，`cargo build` 通过
- `src-tauri/src/clipboard/` 目录与占位 `mod.rs`
- 空壳 `ClipboardManagerPage.vue` 与 `ClipboardPanelPage.vue`
- 路由 `/tools/clipboard` 与 `/clipboard-panel` 能跳转
- `ToolsHubPage.vue` 新增"剪贴板管理"卡片
- i18n 键位占位
- 构建产物成功生成

验收：`pnpm tauri dev` 启动正常，两个路由都能进，versioned-exe 构建成功。

#### M2 · 核心监听与存储（3-4 天）

交付物：
- `clipboard/db.rs`：SQLite 初始化 + schema 迁移 + CRUD
- `clipboard/watcher.rs`：clipboard-master 监听 + arboard 读取 + BLAKE3 去重
- `clipboard/models.rs`：Rust 类型
- `clipboard/image_store.rs`：图片落盘 + GC
- `clipboard/commands.rs`：`cb_enable`/`disable`/`list`/`get`/`delete`/`clear` 等
- 管理页能渲染真实剪贴板历史（简化版，无虚拟滚动）

验收：
- 复制 10 条不同类型内容（文本/HTML/图片/文件），管理页全部可见
- 重启应用数据不丢失
- 同一内容重复复制不新增，updated_at 更新
- 图片内容在 `clipboard_images/` 目录正确落盘

#### M3 · 弹出面板与快捷键（3-4 天）

交付物：
- `main.rs` 创建 `clipboard-panel` 窗口 + Mica
- `clipboard/hotkey.rs`：tauri-plugin-global-shortcut 注册 `Alt+C`
- `clipboard/paste.rs`：enigo 粘贴 + 焦点恢复 + 30ms 等待
- `ClipboardPanelPage.vue`：列表 + 搜索框 + Filter tabs
- `useClipboardHotkey.ts`：窗口内 ↑↓←→/Enter/Shift+Enter/Delete/Esc
- 启动通知（S02）

验收：
- 在全屏其他应用（VSCode/Chrome）中按 `Alt+C` 唤出面板
- 按 `Enter` 粘贴到目标应用成功
- 按 `Shift+Enter` 粘贴纯文本到目标应用成功
- 面板失焦自动隐藏
- 启动后 500ms 出现通知 Toast

#### M4 · 交互增强（3-4 天）

交付物：
- 虚拟列表接入（`vue-virtual-scroller`）
- 悬浮预览（图片 + Ctrl+滚轮缩放）
- 收藏功能 + 拖拽排序（特性 E04）
- 搜索运算符 DSL
- 批量操作（仅管理页）
- 数据统计卡片
- 设置面板（除 Win+V / 管理员外）

验收：
- 1k 条记录滚动帧率 ≥50fps
- 收藏项拖拽排序后重启保留顺序
- 搜索 `type:image from:2026-04-01` 正确过滤
- 批量删除 50 条后总数正确减少
- 数据统计数字与实际文件/DB 大小一致

#### M5 · 高风险项 + 收尾（3-4 天）

交付物：
- `clipboard/win_v.rs`：注册表操作 + explorer 重启（S01）
  - 双重确认对话框
  - 失败自动回滚
  - UI 橙色警告
- `clipboard/admin.rs`：权限检测 + Task Scheduler 或 PowerShell 开机项（S03）
  - 设置页徽章
- i18n 完整中英对照
- 空状态 / 错误处理 / 性能调优
- `cargo clippy` 零警告

验收：
- Win+V 启用后，按 `Win+V` 唤出我们的面板，系统历史不再弹出
- Win+V 禁用后，系统 `Win+V` 恢复正常
- 强制中断 explorer 重启后，应用能自恢复并回滚注册表
- 管理员模式下能向任务管理器粘贴
- 所有文本具备 zh/en 翻译
- `cmd /c pnpm tauri:build:versioned-exe` 成功产出 1.0.7 或后续版本

**总估算：15 个工作日**（≈3 周）。

### 10.3 构建/验证规则

- 每次 commit 前：`cargo fmt && cargo clippy`
- 每个里程碑末：`cmd /c pnpm tauri:build:versioned-exe`（项目硬性要求）
- Git commit message 用中文
- 手动测试清单（见下）

---

## 11. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| clipboard-master 与 Tauri 事件循环冲突 | 监听失效或崩溃 | 初始化时机晚于 Tauri ready；关闭时 PostQuitMessage |
| SQLite WAL 下双窗口并发 | 写入阻塞 | 单 writer 锁；读走 WAL 快照 |
| enigo 对 UIPI 窗口无效 | 管理员进程粘贴失败 | 通过 S03 管理员启动解决；UI 明确提示 |
| Win+V 替代失败 / explorer 无法重启 | 用户桌面卡死 | 失败自动回滚；兜底 `restore-win-v.ps1` 独立脚本 |
| 图片存储膨胀 | 磁盘占用高 | `max_items` + `retain_days` + GC；统计卡片显式展示 |
| 全局快捷键冲突 | 注册失败 | 注册失败时前端提示；建议改键 |
| 打包体积增加 3-5MB | 下载时间略长 | 接受；LTO 已开启可部分抵消 |

---

## 12. 验收标准（Acceptance Criteria）

- [ ] 所有 M1-M5 里程碑验收项通过
- [ ] 在 Win10 与 Win11 上分别冒烟测试
- [ ] 复制粘贴覆盖：纯文本 / 富文本（HTML）/ 图片（PNG/JPG/BMP 来源）/ 单文件 / 多文件
- [ ] 粘贴目标覆盖：VSCode / Chrome / Office Word / 记事本 / 任务管理器（管理员模式）
- [ ] 容量清理：设 `max_items=10` 后复制 15 条，验证保留 10 条（收藏豁免）
- [ ] 重启保留：关闭应用 + 重启系统 → 记录全部存在
- [ ] Win+V 开关：启用 / 禁用各 3 次无异常
- [ ] `cargo clippy` 零警告
- [ ] `cargo fmt` 无差异
- [ ] 所有面向用户文本均通过 `t('clipboard.xxx')`，中英对照完整
- [ ] `cmd /c pnpm tauri:build:versioned-exe` 产物可运行

---

## 13. 回滚计划

### 13.1 代码回滚

- feature 分支合并后若发现严重问题，通过 `git revert <merge-commit>` 撤销
- SQLite 数据库文件保留（不影响旧版本运行，旧版本不感知 clipboard 功能）
- 图片目录保留（浪费磁盘但无副作用）

### 13.2 Win+V 紧急恢复

- release 包内附 `scripts/restore-win-v.ps1`
- README 中增加"Win+V 无法使用？"故障排查章节

### 13.3 用户数据迁移

- 初版不做旧格式迁移（本次是新功能，无历史数据）
- 未来 schema 变更通过 `schema_meta.version` + 迁移函数处理

---

## 14. 依赖清单

### 14.1 新增 Rust Crates

| Crate | 版本 | 用途 |
|---|---|---|
| `rusqlite` | 0.32 | SQLite 绑定（bundled） |
| `clipboard-master` | 4.x | Windows 剪贴板事件监听 |
| `arboard` | 3.x | 跨类型剪贴板读写 |
| `tauri-plugin-global-shortcut` | 2.x | 全局快捷键 |
| `tauri-plugin-notification` | 2.x | 启动通知 Toast |
| `enigo` | 0.2 | 键盘模拟（Ctrl+V 粘贴） |
| `parking_lot` | 0.12 | 高性能 Mutex/RwLock |
| `blake3` | 1.x | 内容去重哈希 |
| `rayon` | 1.x | 图片 GC 并行 |
| `window-vibrancy` | 0.5 | Mica/Acrylic 窗口特效 |

### 14.2 新增 NPM 依赖

| Package | 版本 | 用途 |
|---|---|---|
| `vue-virtual-scroller` | ^2 | 虚拟列表 |
| `vue-draggable-plus` | ^0.5 | 拖拽排序（仅收藏） |
| `@tauri-apps/plugin-global-shortcut` | ^2 | 全局快捷键 JS 绑定 |
| `@tauri-apps/plugin-notification` | ^2 | 通知 JS 绑定 |

### 14.3 保留（不移除）

| Crate / Package | 原因 |
|---|---|
| `tauri-plugin-clipboard-manager`（Cargo） | `SettingsPage.vue` 使用 `writeText` 做路径复制；保留以免回归 |
| `@tauri-apps/plugin-clipboard-manager`（npm） | 同上 |

与新增的 `arboard` 共存：插件负责前端简单 `writeText`，`arboard` 负责后端监听与多类型（text/html/image/file）读写。

### 14.4 Windows Crate Features 增补

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_System_Registry",        # 新增：HKCU 操作（S01）
    "Win32_System_Threading",       # 新增：进程/Token（S03）
    "Win32_System_ProcessStatus",   # 新增：枚举 explorer.exe 进程（S01）
    "Win32_Security",               # 新增：Token 提权检测（S03）
    "Win32_System_Diagnostics_ToolHelp", # 新增：进程快照（S01）
] }
```

---

## 15. 附录：与 ElegantClipboard 的差异

| 维度 | ElegantClipboard | 本项目实现 |
|---|---|---|
| 前端框架 | React 19 + Zustand + shadcn/ui | Vue 3 + 自研 store + Tailwind 工具类 |
| 托盘 | 独立托盘 | 复用 File-Sync-Tool 现有托盘 |
| 主题色 | 跟随系统 / 黑白 / 翡翠 / 天空青 | **仅浅色**（二期再加） |
| 自动更新 | GitHub Release 检查 | **不做**（File-Sync-Tool 自有发布节奏） |
| 存储位置 | `%LOCALAPPDATA%\ElegantClipboard\` | `%APPDATA%\<app>\app_data\`（与现有 history.json 同目录） |
| 分发 | 独立 exe / 安装包 | 作为 File-Sync-Tool 的内建工具 |

---

## 16. 审批

- [ ] 用户审阅本文档
- [ ] 创建 worktree 与 feature 分支
- [ ] 进入实施阶段，按 M1-M5 推进

---

**参考资料**
- ElegantClipboard 源码：https://github.com/Y-ASLant/ElegantClipboard
- Tauri 2.0 API：https://tauri.app/develop/
- rusqlite 文档：https://docs.rs/rusqlite/
- arboard 文档：https://docs.rs/arboard/
