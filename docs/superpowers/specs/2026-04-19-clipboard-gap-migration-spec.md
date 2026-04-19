# 剪贴板管理器 · 与 ElegantClipboard 全面对齐与超越 — 迁移 Spec

- 日期：2026-04-19
- 作者：codex-agent
- 状态：草案 → 待用户评审
- 参考源码：`D:\WorkSpace\File-Sync-Tool-clipboard\ElegantClipboard-main`（MIT, React 19 + Rust + Tauri 2）
- 原始设计：[2026-04-19-clipboard-manager-design.md](./2026-04-19-clipboard-manager-design.md)（1010 行骨架设计）
- 原始里程碑计划：[2026-04-19-clipboard-manager.md](../plans/2026-04-19-clipboard-manager.md)（3599 行任务计划）
- 目标分支：`feature/clipboard-manager`（在 `d:\WorkSpace\File-Sync-Tool-clipboard` worktree 实施）
- 验收目标：**功能完整度 + 界面易用性 ≥ ElegantClipboard，最终超越**
- 本 spec 聚焦"差异补齐"，前置两份文档（设计 + 里程碑计划）仍然有效，本文补全未覆盖部分

---

## 0. 目录

- [1. 执行摘要](#1-执行摘要)
- [2. 全局差距矩阵](#2-全局差距矩阵)
- [3. 详细功能对比（A-J）](#3-详细功能对比a-j)
- [4. UI / UX 设计对比](#4-ui--ux-设计对比)
- [5. 迁移优先级与里程碑（M6–M9）](#5-迁移优先级与里程碑m6m9)
- [6. 数据模型变更](#6-数据模型变更)
- [7. Tauri 命令扩展](#7-tauri-命令扩展)
- [8. 前端架构变更](#8-前端架构变更)
- [9. 系统集成迁移](#9-系统集成迁移)
- [10. 性能/数据层迁移](#10-性能数据层迁移)
- [11. 测试与验收](#11-测试与验收)
- [12. 明确不做](#12-明确不做)
- [13. 风险与对策](#13-风险与对策)
- [14. 附录 A：文件级迁移清单](#14-附录-a文件级迁移清单)
- [15. 附录 B：新增依赖清单](#15-附录-b新增依赖清单)

---

## 1. 执行摘要

### 1.1 现状评估

| 维度 | 我们 | ElegantClipboard | 差距 |
|------|-----|------------------|------|
| 核心监听（text/image） | ✅ | ✅ | 0 |
| 扩展类型（html/files/rtf + 源应用图标） | ⚠️ 仅 html | ✅ 四类齐全 + 图标提取 | **高** |
| 搜索（DSL + 高亮） | ⚠️ 有 DSL，无高亮 | ✅ 高亮 + 字段智能选择 | **中** |
| 面板交互（右键菜单 / Alt+N / Shift 连选 / 合并粘贴） | ❌ | ✅ 全套 | **高** |
| 悬浮预览 | ⚠️ Vue overlay | ✅ 独立原生窗口 | **中** |
| 卡片显示个性化 | ⚠️ 仅 compact 二级密度 | ✅ 密度/行数/格式/开关 10+ 项 | **高** |
| 窗口管理（记忆/位置偏好/Mica/动画） | ⚠️ 无记忆、无 Mica | ✅ 齐全 | **中** |
| 系统集成（托盘/免UAC/非抢焦点） | ⚠️ 无托盘、PowerShell UAC、会抢焦点 | ✅ 齐全 | **高** |
| 性能（读写分离/孤儿 GC/VACUUM） | ⚠️ 单连接，GC 未启用 | ✅ 齐全 | **中** |
| 数据管理（导入导出/分组/三级清理） | ❌ | ✅ 齐全 | **高** |
| 主题色 / 暗色 | ❌ | ✅ | 本轮**不做** |
| 在线更新 | ❌ | ✅ | 本轮**不做** |

**总结**：约 **65 个差异点**，其中 **P1（阻塞可用性）12 项**、**P2（体验关键）18 项**、**P3（打磨）15 项**，明确**不做**约 **20 项**（主题 + 更新 + 跨平台 + 便携版 + 跨用户部署等）。

### 1.2 迁移总目标

1. **补齐所有核心能力**：P1 + P2 全量实现，确保"完整度 ≥ 对标项目"。
2. **UI 重构**：卡片显示、右键菜单、设置页信息架构向 ElegantClipboard 看齐；中文布局更紧凑。
3. **实现上的优化点同步**：读写分离、免 UAC 任务计划、非抢焦点 `SWP_NOACTIVATE`、独立预览窗口。
4. **超越点（我们新增，对方没有）**：
   - 内嵌在文件同步工具中，与任务调度/日志体系打通
   - 中英 i18n 天然支持（对方仅中/英，我们体系更完整）
   - 更严格的 fmt/clippy/构建验证流水线
   - 全量 TypeScript 类型（对方部分使用 any）

### 1.3 推荐落地顺序（高层）

| 里程碑 | 范围 | 工期估 | 里程碑验收点 |
|--------|------|--------|-------------|
| M6 · 内容补全 | A3/A4/A5/A6（html/files/rtf + 图标）、G2（读写分离） | 3–4 天 | 复制 exe、图片文件、多文件选中都能正确识别并显示图标 |
| M7 · 交互补齐 | B3/B4/B5/B6/B7/B8/B10/B18（右键菜单、粘贴为路径、资源管理器、详情、合并、Shift 连选、Alt+N） | 3–5 天 | 面板各类右键操作齐全，Alt+1..9 快速粘贴 |
| M8 · 显示个性化 | D1-D9、B13→原生窗口改造、B14/B15/B16（位置偏好+高亮）、C5/C6/C8/C11 | 4–5 天 | 设置页"显示"tab 拥有密度/行数/格式/开关等 10+ 项 |
| M9 · 系统与数据 | F1（托盘）/F2（非抢焦点）/F5（任务计划免UAC）/G7/G8/G10、A9（导入导出）、A10/A11/A12 | 4–5 天 | 托盘、免 UAC 启动、导入导出、分组、pinned/favorite 拆分 |

M6–M9 合计 **14–19 个工作日**。

---

## 2. 全局差距矩阵

### 2.1 图例

- ✅ 已实现
- ⚠️ 部分实现或有缺陷
- ❌ 未实现
- ⛔ 明确不做
- 🆕 我们实现后会超越对方

### 2.2 统一矩阵（功能完整度）

**A. 数据/内容类型**（15 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| A1 | text 类型 | ✅ | ✅ | — |
| A2 | image 类型（PNG 落盘） | ✅ | ✅ | — |
| A3 | html 富文本（独立存储） | ⚠️ schema 有 html 字段，watcher 未读取 | ✅ | **M6** |
| A4 | files 类型（多文件列表 + 有效性检查） | ⚠️ schema 有 file_paths_json，watcher 未捕获 | ✅ | **M6** |
| A5 | rtf 类型 | ❌ | ✅ | **M6** |
| A6 | 来源应用图标（从 exe 提取落盘） | ❌ 仅进程名 | ✅ SHGetFileInfoW+GDI→PNG | **M6** |
| A7 | 字符数索引列 char_count | ⚠️ 前端计算 | ✅ DB 列 | **M8** |
| A8 | 去重策略（置顶/忽略/总是新建三选一） | ⚠️ 固定置顶 | ✅ | **M6** |
| A9 | 数据导入/导出 JSON/ZIP | ❌ | ✅ zip + 原子 rename | **M9** |
| A10 | 文本编辑（双击修改已存记录） | ❌ | ✅ `update_text_content` | **M9** |
| A11 | 自定义分组（groups 表 + 增删改移动） | ❌ 固定 5 tab | ✅ | **M9** |
| A12 | pinned 与 favorite 分离 | ❌ 仅 favorite | ✅ 两列独立 | **M9** |
| A13 | 来源应用名称（进程名） | ✅ | ✅ | — |
| A14 | 内容哈希去重（BLAKE3） | ✅ | ✅ | — |
| A15 | 单条字节上限保护 | ✅ | ✅ | — |

**B. 面板交互**（20 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| B1 | 卡片点击即粘贴 | ✅ | ✅ | — |
| B2 | 纯文本粘贴 Shift+Enter | ✅ | ✅ | — |
| B3 | 右键上下文菜单 | ❌ | ✅ Radix-UI | **M7** |
| B4 | 粘贴为路径（文件类型） | ❌ | ✅ `paste_as_path` | **M7** |
| B5 | 在资源管理器中显示 | ❌ | ✅ | **M7** |
| B6 | 另存为（图片） | ❌ | ✅ | **M7** |
| B7 | 文件详情对话框 | ❌ | ✅ 含失效红标 | **M7** |
| B8 | 合并粘贴（批量多选合并文本） | ❌ | ✅ `merge_paste_content` + 分隔符 | **M7** |
| B9 | 管理页批量收藏/取消收藏/导出 | ⚠️ 仅批量删除 | ✅ | **M9** |
| B10 | Shift 连选 | ❌ | ✅ lastSelectedIndex | **M7** |
| B11 | 虚拟列表 | ✅ vue-virtual-scroller | ✅ react-virtuoso | — |
| B12 | 拖拽排序（仅收藏） | ✅ | ✅ | — |
| B13 | 图片悬浮预览（独立窗口 + Ctrl+滚轮缩放） | ⚠️ Vue overlay | ✅ 独立窗口 | **M8 改造** |
| B14 | 文本悬浮预览（独立窗口 + Ctrl+滚轮滚动） | ⚠️ Vue overlay | ✅ 独立窗口 | **M8 改造** |
| B15 | 悬浮预览位置偏好（自动/左/右） | ❌ | ✅ | **M8** |
| B16 | 搜索关键词高亮 | ❌ | ✅ `HighlightText` 组件 | **M8** |
| B17 | 搜索运算符 DSL | ✅ | ✅ | — |
| B18 | Alt+数字键快速粘贴（1-9） | ❌ 序号已显示 | ✅ 可自定义 | **M7** |
| B19 | 窗口内键盘导航 | ✅ 完整 | ✅ | — |
| B20 | 搜索框 X 清空按钮 | ❌ 仅浏览器原生 | ✅ 自定义样式 | **M8** |

**C. 窗口管理**（11 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| C1 | 全局快捷键 | ✅ | ✅ | — |
| C2 | Win+V 替换 | ✅ | ✅ | — |
| C3 | 点击外部隐藏 | ✅ | ✅ | — |
| C4 | 窗口锁定 | ✅ | ✅ | — |
| C5 | 跟随光标 on/off 开关 | ⚠️ 硬编码跟随 | ✅ 设置项 | **M8** |
| C6 | 记住窗口大小 + 重启恢复 | ❌ 固定 420×720 | ✅ 位置/尺寸持久化 | **M8** |
| C7 | 多显示器边界裁剪 | ✅ | ✅ | — |
| C8 | 窗口切入/切出动画 | ❌ 直接 show/hide | ✅ | **M8** |
| C9 | 搜索自动清空 + 滚动重置 | ⚠️ 只有搜索清空 | ✅ | **M8** |
| C10 | 搜索框自动聚焦（可选） | ✅ 默认开 | ✅ | — |
| C11 | Mica/Acrylic 毛玻璃 | ❌ 为拖动移除 | ✅ window-vibrancy | **M8（可选开关）** |

**D. 卡片显示设置**（10 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| D1 | 卡片密度（紧凑/标准/宽松） | ⚠️ 仅二级 | ✅ 三级 | **M8** |
| D2 | 预览行数（1-10） | ❌ 固定 line-clamp-2 | ✅ | **M8** |
| D3 | 时间格式（绝对/相对可选） | ❌ 硬编码相对 | ✅ | **M8** |
| D4 | 显示字符数开关 | ❌ 永远显示 | ✅ | **M8** |
| D5 | 显示字节大小开关 | ❌ 永远显示 | ✅ | **M8** |
| D6 | 显示来源应用（icon/name/both） | ⚠️ 仅 name | ✅ 三态 | **M8** |
| D7 | 图片最大高度设置 | ❌ 硬编码 max-h-24 | ✅ | **M8** |
| D8 | 图片自适应高度开关 | ❌ | ✅ | **M8** |
| D9 | 图片文件名覆盖层 | ⚠️ 显示"ImageWxH" | ✅ | — |
| D10 | 拖拽区域可视化指示器 | ❌ | ✅ | **M8** |

**E. 搜索**（4 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| E1 | LIKE + CJK | ✅ | ✅ | — |
| E2 | 智能字段选择（仅预览+路径） | ⚠️ 搜 preview+full，未搜 paths | ✅ | **M6** |
| E3 | 关键词高亮 | ❌ | ✅ | **M8** |
| E4 | 运算符 DSL | ✅ | ✅ | — |

**F. 系统集成**（8 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| F1 | 系统托盘（左键切换 + 右键菜单） | ✅（主窗口有，面板 N/A） | ✅（剪贴板专属） | **M9 联动** |
| F2 | 非焦点窗口（不抢焦点） | ⚠️ set_focus 会抢 | ✅ SWP_NOACTIVATE | **M9** |
| F3 | SendInput 模拟粘贴 | ✅ enigo | ✅ SendInput/enigo | — |
| F4 | 启动通知 | ✅ | ✅ | — |
| F5 | 管理员提权免 UAC（Task Scheduler LogonTrigger） | ⚠️ PowerShell RunAs | ✅ schtasks | **M9** |
| F6 | 便携版支持 | ❌ | ✅ | ⛔ |
| F7 | 自动更新 | ❌ | ✅ | ⛔ |
| F8 | 系统代理读取 | ❌ | ✅ | ⛔ |

**G. 性能/数据**（10 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| G1 | WAL 模式 | ✅ | ✅ | — |
| G2 | 读写分离连接池（分别 cache） | ❌ 单 Mutex | ✅ 写 64MB / 读 32MB + mmap | **M6** |
| G3 | 部分/降序/复合索引 | ⚠️ 4 个基本索引 | ✅ | **M6** |
| G4 | BLAKE3 去重 | ✅ | ✅ | — |
| G5 | 虚拟滚动 | ✅ | ✅ | — |
| G6 | 容量清理 | ✅ | ✅ | — |
| G7 | 图片孤儿扫描 GC | ⚠️ `gc_orphan_images` 存在但未启用 | ✅ | **M9 启用** |
| G8 | DB OPTIMIZE/VACUUM 手动触发 | ❌ | ✅ | **M9** |
| G9 | 数据统计 | ✅ | ✅ | — |
| G10 | 三级数据清理 | ⚠️ 仅清空历史 | ✅ 清空/恢复默认/重置所有 | **M9** |

**H. 外观/主题**（6 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| H1 | 跟随系统强调色 | ❌ | ✅ | ⛔ |
| H2 | 预设主题色 | ❌ | ✅ | ⛔ |
| H3 | 暗色模式 | ❌ | ✅ | ⛔（二期） |
| H4 | 直角模式 | ❌ | ✅ | **M8** |
| H5 | 字号设置 | ❌ | ✅ | **M8**（仅字号，不做字体） |
| H6 | 音效反馈 | ❌ | ✅ | **M8**（可选） |

**I. 其他工具栏/设置**（5 项）

| # | 功能 | 我们 | 对方 | 本轮 |
|---|------|------|------|------|
| I1 | 工具栏按钮自定义 | ❌ 固定 4 个 | ✅ | **M8** |
| I2 | 自定义存储路径 + 迁移 | ❌ | ✅ | **M9** |
| I3 | 单条内容大小上限 | ✅ | ✅ | — |
| I4 | 键盘导航总开关 | ❌ | ✅ | **M8** |
| I5 | 应用黑名单（不监听指定 exe） | ❌ | ✅ 黑/白名单 + 通配符 | **M9** |

**本轮合计需要实施：约 45 项。** 其中 M6=7、M7=7、M8=18、M9=13（含跨里程碑 UI 项）。

---

## 3. 详细功能对比（A-J）

> 以下为每个"本轮待实施"项的对比详情：ElegantClipboard 怎么做 / 我们现状 / 目标实现方案。UI 参考统一放到 §4。

### A. 数据/内容类型

#### A3 · HTML 富文本

- **对方实现**：
  - 文件：`ElegantClipboard-main/src-tauri/src/clipboard/handler.rs`
  - 类型：`ClipboardContent::Html{html: String, text: String}`
  - DB 列：`html_content TEXT`, `text_content TEXT`（fallback），`kind = "html"`
  - watcher 捕获路径：`arboard::Clipboard::get().html()` 成功时优先 HTML
- **我们现状**：`models.rs` 定义了 kind，`db.rs` 有 html 列，但 `watcher.rs` 未调用 arboard 的 HTML 获取。
- **迁移方案**：
  1. 在 [watcher.rs](../../../src-tauri/src/clipboard/watcher.rs) 的 `on_clipboard_change` 中添加优先级链：先尝试读 HTML，再退回纯文本。
  2. 如果 HTML 存在，写入 `html_content` 列，`content_preview` = 从 HTML strip 后前 500 字符。
  3. 粘贴时（[paste.rs](../../../src-tauri/src/clipboard/paste.rs)）：根据 kind，HTML 类型同时写 HTML + 纯文本到剪贴板；`Shift+Enter` 强制仅写纯文本。

#### A4 · 文件类型

- **对方实现**：
  - `handler.rs::process_files()` 序列化文件路径数组为 JSON 字符串存入 `file_paths TEXT`。
  - 失效检测：`rayon::par_iter` 检查 `std::fs::metadata` 存在性，返回 `Vec<{path, exists, size}>`，前端用红色标注。
  - 粘贴：作为 CF_HDROP 格式写回剪贴板 → Ctrl+V 到目标窗口可直接粘贴文件（非路径文本）。
- **我们现状**：
  - watcher 未捕获 `CF_HDROP`；paste.rs 对文件类型只是把路径 newline join 当文本粘贴。
- **迁移方案**：
  1. [watcher.rs](../../../src-tauri/src/clipboard/watcher.rs)：接 `arboard` ≥3.4 的 files API 或自实现 `GetClipboardData(CF_HDROP)` + `DragQueryFileW`。
  2. 新 DB 列用现有 `file_paths_json`，内容是 `["C:\\a.txt","D:\\b.png"]`。
  3. [paste.rs](../../../src-tauri/src/clipboard/paste.rs) 新增 `paste_as_files(id)` → 写入 CF_HDROP → SendInput Ctrl+V。
  4. 新 Command `cb_check_file_paths(ids: Vec<i64>) -> Vec<FilePathStatus>` 用 rayon 并行检查。
  5. 前端卡片：`CardTypeBadge` 区分 `image` / `single-image-file`（唯一路径是 .png/.jpg 等）/ `files`（多路径）三种。

#### A5 · RTF

- **对方实现**：`ClipboardContent::Rtf{rtf, text}`，`rtf_content TEXT` 列。
- **迁移方案**：
  - [models.rs](../../../src-tauri/src/clipboard/models.rs) 新增 `kind = "rtf"`、`rtf_content: Option<String>`。
  - [db.rs](../../../src-tauri/src/clipboard/db.rs) `ALTER TABLE items ADD COLUMN rtf_content TEXT`（带 migration）。
  - watcher 优先级：`CF_RTF > CF_HTML > CF_UNICODETEXT > 其他`。
  - paste.rs：`Shift+Enter` 强制纯文本；否则按 kind 写回。

#### A6 · 来源应用图标

- **对方实现**：
  - 文件：`src-tauri/src/clipboard/source_app.rs`
  - 函数：`extract_and_cache_icon(exe_path: &str) -> Option<PathBuf>`
  - Win32 调用序列：`SHGetFileInfoW(SHGFI_ICON | SHGFI_LARGEICON)` → `CreateCompatibleDC` → `GetDIBits` → `image::DynamicImage::to_vec()` → PNG 写盘。
  - 缓存键：`blake3(exe_path)[..12]`；存储目录：`%APPDATA%\<app>\app_data\clipboard_icons\<hash>.png`。
  - DB 列：`source_app_icon TEXT`（存相对路径）。
- **我们现状**：`source.rs` 只取进程名与路径，未提取图标。
- **迁移方案**：
  1. [source.rs](../../../src-tauri/src/clipboard/source.rs) 新增 `extract_icon(exe_path: &Path) -> Result<PathBuf>`。
  2. 新增 `icon_store.rs`（仿 `image_store.rs`），包含 cache 目录初始化与 hash 命名。
  3. watcher 首次捕获到某 exe 时调用，结果路径写入 `source_app_icon` 列。
  4. Tauri 不能直接引用 `windows` crate 的 GDI——增加 `windows = { version = "0.60", features = ["Win32_UI_Shell", "Win32_Graphics_Gdi", "Win32_UI_WindowsAndMessaging"] }`。
  5. 前端 `<img :src="convertFileSrc(item.sourceAppIcon)">` 即可显示。

#### A7 · 字符数索引列

- **对方实现**：`char_count INTEGER` 列，写入时计算，索引用于排序过滤。
- **迁移方案**：ALTER TABLE 加 `char_count`；搜索 DSL `size:>100` 改为基于此列而非 LENGTH(content_full)。

#### A8 · 去重策略三选一

- **对方实现**：
  - settings 表 `dedup_strategy TEXT` ∈ `"move_to_top" | "ignore" | "always_new"`。
  - `handler.rs` 命中 hash 时 switch：
    - `"move_to_top"`: `UPDATE updated_at = NOW() WHERE hash = ?`
    - `"ignore"`: 不做任何事
    - `"always_new"`: 强制 INSERT 新行，hash 列不唯一
- **迁移方案**：
  1. 配置项 `clipboard.dedup_strategy` 新增（默认 `move_to_top`）。
  2. [db.rs](../../../src-tauri/src/clipboard/db.rs) `upsert_item` 拆分为三条路径。
  3. `hash UNIQUE` 约束仅在 `move_to_top`/`ignore` 模式生效；`always_new` 模式下用自增 id 作唯一。

#### A9 · 数据导入导出

- **对方实现**：
  - 文件：`src-tauri/src/commands/data_transfer.rs`
  - 命令：`export_clipboard_data(path: String)`、`import_clipboard_data(path: String)`
  - 格式：ZIP，内含 `clipboard.db` + `images/` + `icons/`。
  - 导入策略：先释放到 `.db.import` 临时文件，校验 schema 版本 → 原子 rename 覆盖现库（或合并模式）。
- **迁移方案**：
  1. 新增 `src-tauri/src/clipboard/data_transfer.rs`。
  2. 依赖 `zip = "0.6"`。
  3. 命令：`cb_export(path: String, include_images: bool)`、`cb_import(path: String, mode: "replace" | "merge")`。
  4. 管理页「数据」子页新增"导入 / 导出 / 合并导入"三按钮。
  5. 失败回滚：导入前自动备份 `clipboard.db.bak.<timestamp>`。

#### A10 · 文本编辑

- **对方实现**：`update_text_content(id, new_text)` → 重算 hash、char_count；`new_text.is_empty()` 则删除该行。
- **迁移方案**：
  - 新增 Command `cb_update_text(id: i64, text: String)`.
  - 前端新增 `ClipboardEditDialog.vue`：Ctrl+E 或右键"编辑" → 模态框 → Textarea → 保存。
  - 仅对 `kind in ("text", "html", "rtf")` 启用。

#### A11 · 自定义分组

- **对方实现**：
  - DB schema：`groups(id INTEGER PK, name TEXT, sort_index INTEGER, created_at INTEGER)`。
  - items 表加 `group_id INTEGER REFERENCES groups(id) ON DELETE SET NULL`。
  - 命令：`create_group(name)` / `rename_group(id, name)` / `delete_group(id)` / `move_item_to_group(item_id, group_id|null)`。
  - "favorite" 是魔法字符串 `__favorites__`，不占 groups 行。
- **迁移方案**：
  1. migration v2：新增 `clipboard_groups` 表 + `group_id` 列。
  2. Rust 新增 `groups.rs`。
  3. 前端左侧 tab 栏 → 分组 + 重命名 + 拖拽重排；右键菜单多一项"移动到分组"。

#### A12 · pinned vs favorite 拆分

- **对方实现**：
  - 列：`is_pinned INTEGER DEFAULT 0`、`is_favorite INTEGER DEFAULT 0`。
  - pinned：永远置顶，不受 retain_days 与 max_items 约束，UI 顶部独立区域。
  - favorite：逻辑分组，仅在 "收藏" tab 出现，也豁免清理。
- **迁移方案**：
  1. migration：新增 `is_pinned` 列（默认 0）。
  2. Command：`cb_toggle_pin(id)`；清理（[retention.rs](../../../src-tauri/src/clipboard/retention.rs)）同时豁免 pinned 与 favorite。
  3. 前端列表：pinned 项固定渲染在最上方（独立区），拖拽可跨入/跨出。

### B. 面板交互

#### B3 · 右键上下文菜单

- **对方实现**：
  - 文件：`src/components/ui/context-menu.tsx`（Radix-UI 封装）
  - 条目（依 kind 动态）：
    - 通用：粘贴 / 纯文本粘贴 / 复制 / 编辑（文本类） / 置顶 / 收藏 / 移动到分组 / 删除
    - files：粘贴为路径 / 在资源管理器中显示 / 文件详情
    - image：另存为
- **迁移方案**：
  1. 新建 `src/components/ClipboardCardMenu.vue`，基于 Headless UI / 自实现浮层。
  2. 触发：右键 `contextmenu` 或 `Menu` 键；位置跟随鼠标。
  3. 条目 enable 规则配置化（根据 `item.kind`）。
  4. 键盘可达（Menu 键打开 + ↑↓ Enter）。

#### B4 · 粘贴为路径

- **对方实现**：`paste_as_path(id)` → 读取 `file_paths`，newline join → 写 CF_UNICODETEXT → SendInput。
- **迁移方案**：paste.rs 新增 `cb_paste_as_path(id)` Command。右键菜单触发。

#### B5 · 在资源管理器中显示

- **对方实现**：`open_in_explorer(path)` → `explorer.exe /select,"<path>"`.
- **迁移方案**：复用现有 `open_path_parent`，但新增 `/select,` 选项的 `open_and_select_in_explorer(path: &str)` 放在 [main.rs](../../../src-tauri/src/main.rs) 或 `os_util.rs`。

#### B6 · 另存为（图片）

- **对方实现**：`save_image_as(id, target_path)` → 读 `image_path` → copy。
- **迁移方案**：新增 Command `cb_save_image_as(id, target_path)`。前端 `tauri.dialog.save({filters:[{name:'PNG',extensions:['png']}]})`。

#### B7 · 文件详情对话框

- **对方实现**：`FileDetailsDialog` 组件；文件列表 + 每文件状态图标（✓/✗）+ 单文件 `show_in_explorer` 按钮。
- **迁移方案**：`ClipboardFileDetailsDialog.vue`；输入 `file_paths_json`；调用 `cb_check_file_paths` 取 exists/size。

#### B8 · 合并粘贴

- **对方实现**：`merge_paste_content(ids: Vec<i64>, separator: Option<String>)`。
- **迁移方案**：
  1. 后端新增 `cb_merge_paste(ids: Vec<i64>, separator: Option<String>)`。
  2. 前端批量模式工具栏新增"合并粘贴"按钮（仅在已勾选 ≥2 且都是文本类时可点）。
  3. 分隔符默认 `\n`，弹窗可选 ` `、`, `、`\n\n`、自定义。

#### B10 · Shift 连选

- **对方实现**：`toggleSelect(id, index, shiftKey)` 逻辑：if shiftKey && lastSelectedIndex>=0 → 选中 [min, max] 闭区间。
- **迁移方案**：在 `store.ts` 的 selection 逻辑中加 `lastToggledIndex`，Shift+click 时 range-fill。

#### B13/B14 · 悬浮预览改为独立原生窗口

- **对方实现**：
  - 命令：`show_image_preview(id)` / `show_text_preview(id)` → 创建或复用窗口（label `image-preview` / `text-preview`）。
  - 窗口属性：`decorations=false, transparent=true, skip_taskbar=true, focused=false, always_on_top=true`。
  - 定位：相对原卡片位置 + 用户偏好（auto/left/right）+ 多显示器 clamp。
  - 失效防抖：token（int）递增，回调校验 token 未变才渲染，避免快速移动残影。
- **迁移方案**：
  1. 新建 Rust 模块 `src-tauri/src/clipboard/preview.rs`：`show_image_preview(id, anchor_x, anchor_y)`、`hide_image_preview()`、`show_text_preview(id, ...)`、`hide_text_preview()`。
  2. 新增两个 Tauri 窗口配置（在 `tauri.conf.json` 或运行时 `WebviewWindowBuilder`）：`clipboard-image-preview`、`clipboard-text-preview`，URL = `index.html#/clipboard-preview/image` / `#/clipboard-preview/text`。
  3. 新建路由组件 `src/pages/ClipboardImagePreview.vue`、`ClipboardTextPreview.vue`，窗口内部完成 `Ctrl+滚轮` 缩放/滚动。
  4. 失焦自动 `hide`；卡片 `mouseleave` 300ms 内未再悬停则 hide。

#### B15 · 预览位置偏好

- **对方实现**：设置 `preview_position ∈ "auto" | "left" | "right"`。
- **迁移方案**：`clipboard.preview.position`；`auto` = 依面板贴屏边而定；`left`/`right` 强制。

#### B16 · 搜索关键词高亮

- **对方实现**：`HighlightText` 组件，regex `escapeRegExp` + split → `<mark class="search-highlight">`.
- **迁移方案**：新建 `src/components/ClipboardHighlightText.vue`。对 `content_preview` 与（可选） `source_app_name` 字段高亮。

#### B18 · Alt+数字快速粘贴

- **对方实现**：
  - `input_monitor.rs` 低级钩子注册 Alt+1..Alt+9。
  - 或 `tauri-plugin-global-shortcut` 9 个独立注册（对方走 low-level hook）。
  - 用户可在 Shortcuts 设置改绑。
- **迁移方案**：
  1. `hotkey.rs` 在面板显示时注册 Alt+1..Alt+9 → emit `clipboard-panel-quick-paste` event (index) → 前端定位当前过滤后列表第 N 项 → `cb_paste(id)`。
  2. 面板隐藏时注销。
  3. 卡片编号（现已显示 1-based）在非批量模式下可见。

#### B20 · 搜索框清空按钮

- **对方实现**：`type="text"` + 自定义 ✕ 按钮 + 键盘 Esc 等价。
- **迁移方案**：`ClipboardSearchBox.vue`，改用 `<div class="relative">` + 自带 clear。

### C. 窗口管理

#### C5 · 跟随光标开关

- **对方实现**：设置 `position_mode ∈ "follow_cursor" | "screen_center" | "fixed_position"`。
- **迁移方案**：config `clipboard.panel.position_mode`；`fixed_position` 用户可拖动后自动保存。

#### C6 · 记住窗口大小/位置

- **对方实现**：
  - settings: `window_width/height/x/y` + `persist_window_size/position bool`。
  - 隐藏时：`save_window_size_if_enabled()` 保存到 settings。
  - 显示时：读取 settings 应用。
- **迁移方案**：
  1. 配置 `clipboard.panel.size_persist: bool`、`panel_width/height/x/y`。
  2. hide 回调写入；show 读取。
  3. 面板可拖拉 resize（移除 `resizable: false`，加 min/max 约束）。

#### C8 · 动画过渡

- **对方实现**：CSS transition（opacity + translate）0.15s。
- **迁移方案**：Vue `<Transition>` 包裹面板根；进入从上方 -8px fade-in，离开反之。

#### C9 · 滚动位置重置

- **对方实现**：hide 事件回调：`setSearch("")` + `virtualizer.scrollTo(0)`.
- **迁移方案**：已有搜索清空；补 `scroller.scrollToItem(0)`（`DynamicScroller` API）。

#### C11 · Mica/Acrylic（带开关）

- **对方实现**：`window_vibrancy::apply_mica(&window, Some(false))` 在窗口创建后调用；设置可切换 `mica/acrylic/tabbed/none`。
- **迁移方案**：
  1. Cargo 加 `window-vibrancy = "0.5"`。
  2. config `clipboard.panel.vibrancy: "mica" | "acrylic" | "tabbed" | "none"`。
  3. 窗口创建后按配置 apply。**注意**：上一轮为"面板拖动跟手"去掉了特效（见 commit 7625ec7）——改用"可选开关 + 默认 none"，让用户自主选择。

### D. 卡片显示设置

统一放到 §4.3 UI 设计章节并发映射到实现。

### F. 系统集成

#### F2 · 非抢焦点显示

- **对方实现**：面板显示后调用 `SetWindowPos(hwnd, HWND_TOPMOST, 0,0,0,0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)`。
- **迁移方案**：
  1. `commands.rs` show 流程替换 `window.set_focus()` 为 `force_topmost_no_activate()`。
  2. Rust 侧 `windows::Win32::UI::WindowsAndMessaging::SetWindowPos` 调用。
  3. 前端 focus 靠键盘 hook（面板捕获 keyboard events 即使不抢焦点）。

#### F5 · 免 UAC 提权（任务计划程序）

- **对方实现**：
  - `task_scheduler.rs` 用 `schtasks.exe` CLI：
    - `create`: `schtasks /Create /TN ElegantClipboard_AdminElevation /TR "\"C:\\path.exe\"" /SC ONCE /RL HIGHEST /IT /F`
    - `run`: `schtasks /Run /TN ElegantClipboard_AdminElevation`
  - 首次创建弹一次 UAC，之后每次 `/Run` 无 UAC。
- **迁移方案**：
  1. 新增 `src-tauri/src/clipboard/task_scheduler.rs`。
  2. 任务名 `FileSyncTool_ClipboardAdmin`。
  3. 配置项 `clipboard.admin_startup: bool`：
     - 开启：首次点击"应用"时调用 create（弹 UAC），成功后写入开机启动使用 `schtasks /Run`。
     - 关闭：删除任务。
  4. 兜底：若 `schtasks` 调用失败 → 回退到 PowerShell `Start-Process -Verb RunAs`。

### G. 性能/数据

#### G2 · 读写分离

- **对方实现**：
  - `Database { write_conn: Mutex<Connection>, read_conn: Mutex<Connection> }`
  - 写连接：`journal_mode=WAL; synchronous=NORMAL; cache_size=-65536; mmap_size=268435456`
  - 读连接：`SQLITE_OPEN_READ_ONLY; cache_size=-32768; mmap_size=268435456`
- **迁移方案**：
  1. [db.rs](../../../src-tauri/src/clipboard/db.rs) `ClipboardDb` 拆两条连接。
  2. 查询（list/search/stats）走 read_conn；upsert/delete/favorite 走 write_conn。
  3. `Mutex` 仍保留，因为 rusqlite Connection 非 Send+Sync（或用 `r2d2`）。
  4. 初始化时对读连接执行 `PRAGMA query_only = ON`。

#### G3 · 索引优化

- **对方实现**：
  ```sql
  CREATE INDEX idx_items_created_desc ON items(created_at DESC);
  CREATE INDEX idx_items_fav ON items(is_favorite) WHERE is_favorite = 1;  -- 部分索引
  CREATE INDEX idx_items_pin ON items(is_pinned) WHERE is_pinned = 1;
  CREATE INDEX idx_items_kind_created ON items(kind, created_at DESC);
  CREATE INDEX idx_items_group ON items(group_id) WHERE group_id IS NOT NULL;
  CREATE INDEX idx_items_hash ON items(hash);
  ```
- **迁移方案**：在 db schema 初始化 SQL 中添加部分索引与复合索引。

#### G7 · 图片孤儿 GC

- **对方实现**：
  - `enforce_max_count` / `delete_older_than` 删除 DB 行后调用 `cleanup_image_files()`。
  - 启动时扫描一次：DB 所有 `image_path` set → 磁盘所有文件 set → 差集删除（rayon 并行）。
- **迁移方案**：
  1. 启用现有 `gc_orphan_images`（去掉 `#[allow(dead_code)]`）。
  2. 启动时后台调用一次；每次清理后调用。
  3. 加 log：`image GC removed N orphans`。

#### G8 · DB OPTIMIZE/VACUUM

- **对方实现**：`optimize()` → `PRAGMA optimize;`；`vacuum()` → `VACUUM;`。
- **迁移方案**：新增 `cb_db_optimize()` 与 `cb_db_vacuum()` Command；设置页「数据」tab 两个按钮。

#### G10 · 三级清理

- **对方实现**：
  - `clear_history()`：`DELETE FROM items`。
  - `reset_settings()`：`DELETE FROM settings` 并插入默认值。
  - `reset_all_data()`：前两个 + 删除 `groups` + 删除 `images/` `icons/` 目录 + `VACUUM`。
- **迁移方案**：分别对应 `cb_clear_history()` / `cb_reset_config()` / `cb_reset_all()`；设置页"数据"tab 三个带 **双重确认** 的按钮。

### H. 外观

#### H4 · 直角模式

- **对方实现**：`sharpCorners: bool`；true 时全局 `border-radius: 0`。
- **迁移方案**：全局 CSS 类 `.clipboard-root.sharp-corners *, .clipboard-root.sharp-corners` 覆盖 `border-radius: 0 !important`（限定范围内）。

#### H5 · 字号

- **对方实现**：`font_scale ∈ "small" | "normal" | "large"`。
- **迁移方案**：根元素 class `cb-font-scale-sm/md/lg`，对应 14/15/16px。

#### H6 · 音效

- **对方实现**：`src/stores/audio.ts` + 内嵌 base64 wav；设置可选 copy/paste。
- **迁移方案**：
  1. `public/sounds/click.mp3`（小于 4KB）。
  2. 设置 `clipboard.sound.enabled: bool`、`clipboard.sound.volume: 0-100`。
  3. 前端 `useClipboardSound()` composable，监听粘贴/复制事件播放。

### I. 其他

#### I1 · 工具栏自定义

- **对方实现**：`toolbar_buttons: string[]`（顺序 + 包含决定显示），设置页提供勾选 + 拖拽排序。
- **迁移方案**：面板工具栏按钮改为从配置数组渲染；`clipboard.toolbar.items: string[]` 默认 `["search","filter","batch","settings","lock"]`。

#### I2 · 自定义存储路径 + 迁移

- **对方实现**：选新目录 → 复制当前 `clipboard.db` + `images/` + `icons/` → 修改 config → 重启。
- **迁移方案**：
  1. 配置 `clipboard.data_dir: Option<String>`（None 默认 %APPDATA%）。
  2. 设置页「数据」子页「更改路径」：FilePicker → 调 `cb_migrate_data_dir(new_path)` → 弹重启确认。
  3. 迁移策略：copy → 验证行数一致 → 切换 config → 重启进程。

#### I4 · 键盘导航总开关

- **迁移方案**：`clipboard.panel.keyboard_nav_enabled: bool`（默认 true）。`useClipboardHotkey` 开头检查。

#### I5 · 应用黑名单

- **对方实现**：
  - 配置：`app_filter_enabled: bool`、`app_filter_mode: "blacklist"|"whitelist"`、`app_filter_list: string`（逗号分隔，支持 `*` `?` 通配符）。
  - watcher 读取到 source app 后 `is_source_app_excluded()` 决定是否丢弃事件。
- **迁移方案**：
  1. config 新增对应字段。
  2. [watcher.rs](../../../src-tauri/src/clipboard/watcher.rs) 在 hash 计算前先检查 exclude。
  3. 设置页"应用过滤"tab，带 AppFilter 列表 + 测试输入（"此应用当前会被记录吗"）。

### J. 特殊项

前述 §A 已覆盖（A9 导入导出、A10 编辑、A11 分组、A12 pinned vs favorite 拆分）。

---

## 4. UI / UX 设计对比

### 4.1 主面板布局

**ElegantClipboard**（观测自其 README 截图与 DisplayTab.tsx）：

```
┌ ClipboardPanel 420×720（默认，可调）──────────────────────┐
│ ┌ 顶部 48px ──────────────────────────────────────────┐ │
│ │ [搜索 + X 清空]  [筛选 ▾]  [批量]  [锁]  [⚙设置]   │ │
│ └─────────────────────────────────────────────────────┘ │
│ ┌ 分组栏 40px ────────────────────────────────────────┐ │
│ │ 全部 文本 图片 文件 HTML RTF 收藏 [我的分组 ▾]      │ │  <- 自定义分组
│ └─────────────────────────────────────────────────────┘ │
│ ┌ 置顶区（若有 pinned 项） ───────────────────────────┐ │
│ │ [📌 卡片 ×N]                                        │ │
│ └─────────────────────────────────────────────────────┘ │
│ ┌ 列表区（虚拟滚动） ─────────────────────────────────┐ │
│ │ [1] 🟢FireFox  12:34 · 256字 · 4KB       ⭐ ⋮      │ │
│ │ ├────────────────────────────────────────────────┤ │ │
│ │ │ 预览文本，可自定义 1-10 行 line-clamp         │ │ │
│ │ └────────────────────────────────────────────────┘ │ │
│ │ [2] 🖼️Screenshot.png  12:30 · 1920×1080 · 420KB   │ │
│ │ ├────────────────────────────────────────────────┤ │ │
│ │ │     [可配置 max-height 的缩略图]                │ │ │
│ │ └────────────────────────────────────────────────┘ │ │
│ │ ...                                                 │ │
│ └─────────────────────────────────────────────────────┘ │
│ ┌ 底栏 32px（可选） ──────────────────────────────────┐ │
│ │ 共 1234 条 · 4.2MB · [Alt+数字快速粘贴]             │ │
│ └─────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

**我们**（当前）：

```
┌ ClipboardPanel 420×720 固定 ────────────────────────────┐
│ ┌ 顶部 48px ──────────────────────────────────────────┐ │
│ │ [搜索(type=search)]  [⚙设置]  [锁]                 │ │
│ └─────────────────────────────────────────────────────┘ │
│ ┌ 分组栏 40px ────────────────────────────────────────┐ │
│ │ 全部 文本 图片 文件 收藏                            │ │  <- 仅固定 5 个
│ └─────────────────────────────────────────────────────┘ │
│ ┌ 列表区（虚拟滚动） ─────────────────────────────────┐ │
│ │ ...（卡片）                                         │ │
│ └─────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

**差距**：
- 工具栏缺 X 清空、筛选下拉、批量按钮
- 分组栏缺 HTML/RTF 细分类（待 A3/A5 落地后自然补齐）+ 自定义分组入口
- pinned 置顶区缺失
- 底栏缺失（stats + Alt+N 提示）

**本轮 UI 改造目标**：参照 ElegantClipboard 的四层结构（工具栏 / 分组 / 置顶 / 列表），追加底栏。

### 4.2 卡片详细对比

**ElegantClipboard 卡片**（紧凑模式 ~72px）：

```
┌─────────────────────────────────────────────────┐
│ [1] [🦊] FireFox  · 12:34  · 256字 · 4KB  ⭐ ⋮  │
│ ├─────────────────────────────────────────────┤ │
│ │ 这是预览文本，最多 N 行 line-clamp         │ │
│ └─────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

- 元信息行：编号 · 应用图标 · 应用名 · 时间 · 字符数 · 字节 · 收藏 · 菜单按钮
- 每个元素可独立隐藏（见 §D4-D6）
- 时间格式可选"相对/绝对"
- 图标来自 `source_app_icon` 提取
- `⋮` 按钮等价右键菜单（鼠标操作）

**我们卡片**（当前 88px 标准 / 72px 紧凑）：

```
┌─────────────────────────────────────────────────┐
│ [1] FireFox · 刚刚 · 256字 · 4KB · ⭐           │
│ ├─────────────────────────────────────────────┤ │
│ │ 这是预览文本，固定 line-clamp-2             │ │
│ └─────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

**差距 & 目标**：
1. 追加 `[🦊]` 应用图标（16×16，来自 A6）。
2. 支持时间"相对/绝对"切换（D3）。
3. 字符/字节/应用名独立开关（D4-D6）。
4. 预览行数 1-10 可调（D2）。
5. 图片卡片 max-height 可调 + 自适应模式（D7/D8）。
6. 追加 `⋮` 菜单按钮（B3）。
7. 密度三级（D1）：紧凑 60px / 标准 80px / 宽松 104px。

### 4.3 设置页信息架构

**ElegantClipboard 设置页结构**（`src/components/settings/*.tsx`）：

```
Settings (Modal / Page)
├── GeneralTab      · 开机启动 / 管理员 / Win+V / 通知 / 路径迁移 / 便携
├── DisplayTab      · 密度 / 行数 / 时间格式 / 显示开关（字符/字节/应用名） / 图标尺寸 / 直角 / 字号 / 图片最大高度 / 图片自适应
├── ShortcutsTab    · 唤出键 / Alt+N 1..9 绑定 / 粘贴 / 纯文本粘贴 / 编辑 / 删除 / 收藏 / 搜索 / 关闭 / 键盘总开关
├── DataTab         · 数据统计 / 清空历史 / 恢复默认 / 重置所有 / 导入 / 导出 / OPTIMIZE / VACUUM / 最大条数 / 保留天数 / 单条字节 / 去重策略
├── ThemeTab        · 跟随系统强调色 / 预设配色 / 毛玻璃 (⛔本轮不做)
├── AppFilterTab    · 黑/白名单 / 通配符 / 测试工具
├── AudioTab        · 音效开关 / 音量 / 测试
├── UpdateDialog    · (⛔本轮不做)
└── AboutTab        · 版本 / 开源信息 / 反馈链接
```

**我们设置页**（当前 tab：基础/快捷键/数据/系统集成）：

```
Settings (Page)
├── 基础         · 开机启动 / 通知 / 跟随光标（硬编码）/ 最大条数 / 保留天数 / 单条字节
├── 快捷键       · 唤出键
├── 数据         · 清空历史 / 统计
└── 系统集成     · Win+V 替代（占位）
```

**本轮改造目标**：重构为 8 tab（去掉主题/更新）：

```
Settings (Page)
├── 常规           · 开机启动 / 管理员启动 / Win+V 替代 / 启动通知 / 便携模式（N/A）/ 存储路径迁移
├── 显示           · 密度 / 行数 / 时间格式 / 显示开关组 / 图片最大高度 / 图片自适应 / 直角 / 字号 / 拖拽指示器
├── 快捷键         · 唤出键 / Alt+N 绑定 / 9 个内部键绑定 / 键盘总开关
├── 数据           · 数据统计 / 清空/恢复/重置 三级清理 / 导入/导出 / 去重策略 / 最大条数 / 保留天数 / 单条字节 / OPTIMIZE/VACUUM
├── 预览           · 图片预览开关 / 文本预览开关 / 延时 / 缩放步进 / 位置偏好
├── 应用过滤       · 启用 / 黑白名单模式 / 列表（通配符）/ 测试输入
├── 音效           · 启用 / 音量 / 测试
└── 关于           · 版本 / 开源信息 / 反馈
```

### 4.4 右键菜单设计

**ElegantClipboard 菜单条目**（按 kind 动态）：

| kind | 菜单项 |
|------|--------|
| text | 粘贴 / 纯文本粘贴 / 复制 / 编辑 / 置顶 / 收藏 / 移动到分组 / 删除 |
| html | 同 text + "以 HTML 粘贴" / "以纯文本粘贴" |
| rtf | 同 html（html→rtf） |
| image | 粘贴 / 复制 / 另存为 / 置顶 / 收藏 / 移动到分组 / 删除 |
| files | 粘贴 / 粘贴为路径 / 复制 / 在资源管理器中显示 / 文件详情 / 置顶 / 收藏 / 移动到分组 / 删除 |

**我们落地方案**：新建 `src/components/ClipboardCardMenu.vue`，根据 `item.kind` 动态启用条目，与上表一致。

### 4.5 i18n 键位扩展

- 所有新增 UI 文案一律在 [src/locales/messages.ts](../../../src/locales/messages.ts) 中 zh/en 同步添加到 `clipboard.*` 命名空间。
- 新增 key 约 **150 条**，分组如下：
  - `clipboard.menu.*`（右键菜单 10+）
  - `clipboard.settings.general.*` / `display.*` / `shortcuts.*` / `data.*` / `preview.*` / `appFilter.*` / `audio.*` / `about.*`（8 tab × 10~20 项）
  - `clipboard.dialog.*`（编辑/导入/导出/详情）
  - `clipboard.toast.*`（成功/失败）
  - `clipboard.tooltip.*`（工具栏提示）

---

## 5. 迁移优先级与里程碑（M6–M9）

> 前置 M1-M5 已完成；本轮新增 4 个里程碑。

### 5.1 M6 · 内容补全（3–4 天）

| Task | 内容 | 验收 |
|------|------|------|
| M6.1 | A5 RTF 类型：schema + watcher + paste | 复制 Word 段落看到 rtf kind 入库 |
| M6.2 | A3 HTML 富文本：watcher 读取 HTML | 复制网页看到 html kind，HTML 粘贴到 Word 保留格式 |
| M6.3 | A4 Files 类型：CF_HDROP 读写 | 选中文件复制 → 面板显示 files 卡片 → 粘贴到资源管理器出现文件 |
| M6.4 | A6 应用图标提取 | 复制后卡片显示 FireFox/Chrome 图标 |
| M6.5 | A7 char_count 列 + migration | 检查 DB 列存在 |
| M6.6 | A8 去重策略 3 选一 + 配置项 | 切换模式后行为正确 |
| M6.7 | G2 读写分离 + G3 索引优化 | `EXPLAIN QUERY PLAN` 用到部分索引 |
| M6.8 | E2 搜索字段收敛 | 搜索命中 file_paths 中的字符 |

### 5.2 M7 · 交互补齐（3–5 天）

| Task | 内容 | 验收 |
|------|------|------|
| M7.1 | B3 右键菜单组件 | 右键卡片弹出菜单 |
| M7.2 | B4 粘贴为路径 | files 卡片右键→粘贴为路径，到记事本显示路径 |
| M7.3 | B5 在资源管理器显示 | 右键打开 explorer 并选中 |
| M7.4 | B6 另存为（图片） | 图片卡片右键 → 保存对话框 |
| M7.5 | B7 文件详情对话框 | 显示文件列表 + 失效标红 |
| M7.6 | B8 合并粘贴 | 多选文本→合并→目标窗口出现 joined 文本 |
| M7.7 | B10 Shift 连选 | 第一张 → Shift+第十张 → 选中 10 条 |
| M7.8 | B18 Alt+1..9 快速粘贴 | 面板显示时按 Alt+3 粘贴第 3 条 |

### 5.3 M8 · 显示个性化（4–5 天）

| Task | 内容 | 验收 |
|------|------|------|
| M8.1 | D1 三级密度 | 设置切换可见差异 |
| M8.2 | D2 预览行数 1-10 | 滑块实时生效 |
| M8.3 | D3 时间格式切换 | 绝对/相对可切 |
| M8.4 | D4/D5/D6 显示开关（char/size/sourceApp） | 独立隐藏 |
| M8.5 | D7/D8 图片最大高度 + 自适应 | 数值生效 |
| M8.6 | D10 拖拽区域指示器 | favorite tab 下卡片边缘拖拽手柄可见 |
| M8.7 | B13 图片预览改独立窗口 | 独立窗口，Ctrl+滚轮缩放，Alt+Tab 不切换走 |
| M8.8 | B14 文本预览改独立窗口 | 同上 |
| M8.9 | B15 预览位置偏好 | 设置 left/right 生效 |
| M8.10 | B16 搜索高亮 | 关键词黄底 |
| M8.11 | B20 搜索清空按钮 | X 按钮可点击清空 |
| M8.12 | C5 跟随光标开关 | screen_center 模式居中 |
| M8.13 | C6 窗口尺寸/位置记忆 | 调整后重开恢复 |
| M8.14 | C8 动画 | 显示/隐藏淡入淡出 |
| M8.15 | C11 Mica/Acrylic 开关 | 三选一生效 |
| M8.16 | H4 直角模式 | 全局 border-radius=0 |
| M8.17 | H5 字号 | 三档切换 |
| M8.18 | H6 音效 | 粘贴时听到 click |
| M8.19 | I1 工具栏自定义 | 设置隐藏某按钮生效 |
| M8.20 | I4 键盘导航总开关 | 关闭后所有面板快捷键失效 |

### 5.4 M9 · 系统 + 数据 + 特殊（4–5 天）

| Task | 内容 | 验收 |
|------|------|------|
| M9.1 | F2 SWP_NOACTIVATE 非抢焦点 | 唤出面板时活动窗口输入光标保持 |
| M9.2 | F5 任务计划程序免 UAC | 设置开启后重启系统，自动启动无 UAC |
| M9.3 | A9 JSON/ZIP 导入导出 | 导出 → 新设备导入 → 数据一致 |
| M9.4 | A10 文本编辑对话框 | 编辑后 DB 更新、UI 刷新 |
| M9.5 | A11 自定义分组 | 创建"工作"分组 → 移入 → 切 tab 仅见工作项 |
| M9.6 | A12 pinned vs favorite 拆分 | 置顶条目永远在列表顶部 |
| M9.7 | G7 图片孤儿 GC | 手动删 image_path 记录 → 启动后文件被清 |
| M9.8 | G8 DB OPTIMIZE/VACUUM | 按钮点击后 DB 文件体积减少 |
| M9.9 | G10 三级清理 | 三按钮 + 双重确认 |
| M9.10 | B9 管理页批量收藏/导出 | 管理页选 5 条 → 批量收藏 |
| M9.11 | I2 存储路径迁移 | 切换路径重启后数据完整 |
| M9.12 | I5 应用黑/白名单 | 黑名单加 chrome.exe 后不记录 |
| M9.13 | 设置页 8 tab 重构 | 所有新设置归位 |

### 5.5 最终收尾

- cargo fmt + clippy 无 warning
- `pnpm tauri:build:versioned-exe` 通过
- 新增 i18n 键中英覆盖率 100%
- 合并 `feature/clipboard-manager` → `main`（git merge --no-ff）

---

## 6. 数据模型变更

### 6.1 Migration v2（新增列）

```sql
-- items 表
ALTER TABLE clipboard_items ADD COLUMN rtf_content TEXT;
ALTER TABLE clipboard_items ADD COLUMN char_count INTEGER DEFAULT 0;
ALTER TABLE clipboard_items ADD COLUMN is_pinned INTEGER DEFAULT 0;
ALTER TABLE clipboard_items ADD COLUMN source_app_icon TEXT;
ALTER TABLE clipboard_items ADD COLUMN group_id INTEGER;

-- 新表
CREATE TABLE IF NOT EXISTS clipboard_groups (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  sort_index INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

-- 索引（ElegantClipboard 对齐）
CREATE INDEX IF NOT EXISTS idx_items_created_desc ON clipboard_items(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_fav ON clipboard_items(is_favorite) WHERE is_favorite = 1;
CREATE INDEX IF NOT EXISTS idx_items_pin ON clipboard_items(is_pinned) WHERE is_pinned = 1;
CREATE INDEX IF NOT EXISTS idx_items_kind_created ON clipboard_items(kind, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_group ON clipboard_items(group_id) WHERE group_id IS NOT NULL;
```

### 6.2 Config 扩展（config.rs）

```rust
pub struct ClipboardConfig {
    // 已有字段...
    pub enabled: bool,
    pub hotkey: String,
    pub max_items: u32,
    pub retain_days: u32,
    pub max_item_bytes: u64,
    pub show_startup_notification: bool,
    pub replace_winv: bool,

    // 新增（M6-M9）
    pub dedup_strategy: DedupStrategy,             // A8: MoveToTop | Ignore | AlwaysNew
    pub panel: PanelConfig,                        // 面板相关聚合
    pub display: DisplayConfig,                    // 卡片显示
    pub preview: PreviewConfig,                    // 悬浮预览
    pub shortcuts: ShortcutsConfig,                // 内部快捷键绑定
    pub app_filter: AppFilterConfig,               // 应用黑白名单
    pub audio: AudioConfig,                        // 音效
    pub admin_startup: bool,                       // F5 管理员启动（任务计划）
    pub data_dir: Option<String>,                  // I2 自定义存储路径
    pub toolbar_items: Vec<String>,                // I1 工具栏顺序
    pub keyboard_nav_enabled: bool,                // I4 键盘总开关
    pub sharp_corners: bool,                       // H4 直角
    pub font_scale: FontScale,                     // H5 字号
}

pub struct PanelConfig {
    pub position_mode: PositionMode,               // follow_cursor | screen_center | fixed
    pub fixed_x: Option<i32>,
    pub fixed_y: Option<i32>,
    pub size_persist: bool,
    pub width: u32,
    pub height: u32,
    pub vibrancy: Vibrancy,                        // mica | acrylic | tabbed | none
    pub animation_enabled: bool,
    pub auto_hide_on_blur: bool,
}

pub struct DisplayConfig {
    pub density: CardDensity,                      // compact | standard | spacious
    pub preview_lines: u8,                         // 1-10
    pub time_format: TimeFormat,                   // relative | absolute
    pub show_char_count: bool,
    pub show_byte_size: bool,
    pub show_source_app: SourceAppDisplay,         // none | name | icon | both
    pub image_max_height: u32,                     // px
    pub image_auto_height: bool,
    pub drag_indicator: bool,
}

pub struct PreviewConfig {
    pub image_enabled: bool,
    pub text_enabled: bool,
    pub delay_ms: u32,                             // 500 默认
    pub zoom_step: u8,                             // 5-50 (%)
    pub position: PreviewPosition,                 // auto | left | right
}

pub struct ShortcutsConfig {
    pub quick_paste: Vec<String>,                  // ["Alt+1",...,"Alt+9"]
    pub paste: String,                             // "Enter"
    pub plain_paste: String,                       // "Shift+Enter"
    pub delete: String,                            // "Delete"
    pub favorite: String,                          // "Ctrl+D"
    pub edit: String,                              // "Ctrl+E"
    pub focus_search: Vec<String>,                 // ["Ctrl+F","/"]
    pub close: String,                             // "Escape"
}

pub struct AppFilterConfig {
    pub enabled: bool,
    pub mode: FilterMode,                          // blacklist | whitelist
    pub patterns: Vec<String>,                     // "chrome.exe", "1pass*"
}

pub struct AudioConfig {
    pub enabled: bool,
    pub volume: u8,                                // 0-100
    pub on_copy: bool,
    pub on_paste: bool,
}
```

向后兼容：旧配置文件读取时缺省字段自动填默认值（使用 serde `#[serde(default)]`）。

---

## 7. Tauri 命令扩展

### 7.1 新增 Command（约 28 个）

| Command | 参数 | 说明 |
|---------|------|------|
| `cb_update_text` | id, text | A10 编辑文本 |
| `cb_paste_as_path` | id | B4 粘贴路径 |
| `cb_paste_as_files` | id | A4 粘贴为文件列表 |
| `cb_save_image_as` | id, target_path | B6 另存为 |
| `cb_check_file_paths` | ids | B7 详情对话框查失效 |
| `cb_open_in_explorer` | path | B5 |
| `cb_merge_paste` | ids, separator | B8 |
| `cb_toggle_pin` | id | A12 |
| `cb_groups_list` | — | A11 |
| `cb_groups_create` | name | A11 |
| `cb_groups_rename` | id, name | A11 |
| `cb_groups_delete` | id | A11 |
| `cb_move_to_group` | item_id, group_id | A11 |
| `cb_export` | path, include_images | A9 |
| `cb_import` | path, mode | A9 |
| `cb_db_optimize` | — | G8 |
| `cb_db_vacuum` | — | G8 |
| `cb_clear_history` | — | G10 |
| `cb_reset_config` | — | G10 |
| `cb_reset_all` | — | G10 |
| `cb_migrate_data_dir` | new_path | I2 |
| `cb_show_image_preview` | id, anchor_x, anchor_y | B13 |
| `cb_hide_image_preview` | — | B13 |
| `cb_show_text_preview` | id, anchor_x, anchor_y | B14 |
| `cb_hide_text_preview` | — | B14 |
| `cb_admin_task_create` | — | F5 |
| `cb_admin_task_remove` | — | F5 |
| `cb_admin_task_status` | — | F5 |

### 7.2 修改的 Command

- `cb_list` → 新增可选参数 `group_id: Option<i64>`、`pinned_only: Option<bool>`。
- `cb_paste` → 读取 `preview.zoom_step` 等无关参数无需修改；仍按 kind 智能写入。
- 搜索：统一更名 `cb_search` → 参数支持 DSL；返回结果包含高亮用的关键词数组。

### 7.3 事件扩展

| 事件名 | 载荷 | 说明 |
|--------|------|------|
| `clipboard-item-updated` | `{id, text?}` | A10 编辑后广播 |
| `clipboard-groups-changed` | `[{id,name}]` | A11 分组变化 |
| `clipboard-panel-quick-paste` | `{index: 1..9}` | B18 面板前端监听 |
| `clipboard-system-accent-color-changed` | `{argb}` | （本轮不做，预留） |
| `clipboard-admin-task-status` | `{installed: bool, last_error?}` | F5 |

---

## 8. 前端架构变更

### 8.1 新增组件

```
src/components/clipboard/
├── ClipboardCardMenu.vue              <- B3 右键菜单
├── ClipboardFileDetailsDialog.vue     <- B7 文件详情
├── ClipboardEditDialog.vue            <- A10 编辑
├── ClipboardMergePasteDialog.vue      <- B8 分隔符选择
├── ClipboardImportExportDialog.vue    <- A9
├── ClipboardGroupSidebar.vue          <- A11 分组 tab
├── ClipboardHighlightText.vue         <- B16
├── ClipboardSearchBox.vue             <- B20（带 X 清空）
├── ClipboardToolbar.vue               <- I1 可配置顺序
├── ClipboardAppIcon.vue               <- A6 图标
└── ClipboardPinnedSection.vue         <- A12 置顶区

src/pages/
├── ClipboardImagePreview.vue          <- B13 独立预览窗口
└── ClipboardTextPreview.vue           <- B14 独立预览窗口

src/components/clipboard-settings/
├── GeneralTab.vue
├── DisplayTab.vue
├── ShortcutsTab.vue
├── DataTab.vue
├── PreviewTab.vue
├── AppFilterTab.vue
├── AudioTab.vue
└── AboutTab.vue
```

### 8.2 State 扩展

[src/lib/store.ts](../../../src/lib/store.ts) 新增：

```ts
export const clipboardUIState = reactive({
  groups: [] as ClipboardGroup[],
  currentGroupId: null as number | null,
  isBatchMode: false,
  lastToggledIndex: -1,
  selectedIds: new Set<number>(),
  pinnedIds: new Set<number>(),
});
```

### 8.3 Composables

```
src/composables/
├── useClipboardContextMenu.ts     <- B3 触发逻辑
├── useClipboardHotkey.ts          <- 扩展 Alt+N / 键盘总开关
├── useClipboardHoverPreview.ts    <- 改造为调独立窗口 Command
├── useClipboardGroups.ts          <- A11
├── useClipboardSound.ts           <- H6
├── useClipboardPanelPosition.ts   <- C5/C6
└── useClipboardImportExport.ts    <- A9
```

### 8.4 路由

```
/ (主窗口)
├── /tools/clipboard               管理后台
└── /clipboard-panel               弹出面板

新增：
├── /clipboard-preview/image       B13 独立预览窗口
└── /clipboard-preview/text        B14 独立预览窗口
```

---

## 9. 系统集成迁移

### 9.1 非抢焦点显示（F2）

```rust
// src-tauri/src/clipboard/commands.rs
use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};

fn show_without_focus(window: &WebviewWindow) {
    window.show().ok();
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let _ = SetWindowPos(
                HWND(hwnd.0 as isize),
                HWND_TOPMOST,
                0, 0, 0, 0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
    }
}
```

- 调用位置：面板 toggle 显示路径（替换现 `window.set_focus()`）。
- 前端键盘事件即使窗口未聚焦也通过 WebView 事件冒泡接收；若出现 hotkey 失效，再通过 `tauri-plugin-global-shortcut` 在面板打开时动态注册。

### 9.2 任务计划程序免 UAC（F5）

新建 `src-tauri/src/clipboard/task_scheduler.rs`：

```rust
const TASK_NAME: &str = "FileSyncTool_ClipboardAdmin";

pub fn create_task() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = Command::new("schtasks")
        .args([
            "/Create", "/TN", TASK_NAME,
            "/TR", &format!("\"{}\" --admin-from-task", exe.display()),
            "/SC", "ONLOGON",      // 开机登录触发
            "/RL", "HIGHEST",      // 最高权限
            "/IT",                 // 仅交互会话
            "/F",                  // 覆盖
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(())
}

pub fn run_task() -> Result<(), String> { ... /Run /TN ... }
pub fn remove_task() -> Result<(), String> { ... /Delete /TN ... /F }
pub fn is_installed() -> bool { ... /Query ... }
```

- 首次创建弹一次 UAC；之后 `schtasks /Run /TN ...` 免 UAC。
- 开机自启分支：注册表 Run 改为调用 `schtasks /Run /TN FileSyncTool_ClipboardAdmin`；普通模式保持现逻辑。
- **回退**：若 `schtasks` 失败，切回 PowerShell `Start-Process -Verb RunAs` 并记录日志。

### 9.3 系统托盘联动

现有主窗口托盘菜单新增"剪贴板面板"条目 → 调 toggle_panel。设置页"常规"tab 增"在系统托盘菜单中显示"开关（默认 on）。

---

## 10. 性能/数据层迁移

### 10.1 读写分离（G2）

```rust
pub struct ClipboardDb {
    write_conn: Arc<Mutex<Connection>>,
    read_conn: Arc<Mutex<Connection>>,
}

impl ClipboardDb {
    pub fn open(path: &Path) -> Result<Self> {
        let write = Connection::open(path)?;
        write.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -65536;
            PRAGMA mmap_size = 268435456;
            PRAGMA temp_store = MEMORY;
        ")?;
        let read = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        read.execute_batch("
            PRAGMA cache_size = -32768;
            PRAGMA mmap_size = 268435456;
            PRAGMA query_only = ON;
        ")?;
        Ok(Self { write_conn: Arc::new(Mutex::new(write)), read_conn: Arc::new(Mutex::new(read)) })
    }

    pub fn list(...) -> ... { let c = self.read_conn.lock(); ... }
    pub fn upsert(...) -> ... { let c = self.write_conn.lock(); ... }
}
```

### 10.2 图片孤儿 GC 启用（G7）

- 去掉 `image_store::gc_orphan_images` 上的 `#[allow(dead_code)]`。
- 在 `ClipboardState::new` 启动后一次 `tokio::spawn(gc_orphan_images_task())`。
- 在 `enforce_capacity`、`delete_older_than`、`delete_item` 之后触发。
- 实现：rayon 并行 `std::fs::remove_file`，错误仅 log。

### 10.3 DB OPTIMIZE/VACUUM（G8）

```rust
pub fn db_optimize(&self) -> Result<()> {
    self.write_conn.lock().execute("PRAGMA optimize;", [])?;
    Ok(())
}
pub fn db_vacuum(&self) -> Result<()> {
    self.write_conn.lock().execute("VACUUM;", [])?;
    Ok(())
}
```

前端设置页「数据」tab 两按钮 + 运行时间 toast。

### 10.4 三级清理（G10）

- `cb_clear_history`：`DELETE FROM clipboard_items`；保留 groups、favorites 逻辑；清理 `clipboard_images/` 目录。
- `cb_reset_config`：重置所有 `ClipboardConfig` 到 Default；重启应用提示。
- `cb_reset_all`：`DROP TABLE clipboard_items, clipboard_groups` → 重建 schema → 删除 `clipboard_images/` 与 `clipboard_icons/` → VACUUM → 提示重启。

每个操作前端都要双重确认（模态框 + 输入"DELETE" / "RESET" 字样）。

---

## 11. 测试与验收

### 11.1 单元/集成测试（cargo test）

- `clipboard/db.rs`：insert → search CJK → 命中 hash 的三种 dedup 行为。
- `clipboard/source.rs`：对一个已知 exe（如 `explorer.exe`）提取 icon，生成 PNG 大小 >0。
- `clipboard/data_transfer.rs`：export → import（replace）→ 行数一致；merge 模式不丢数据。
- `clipboard/task_scheduler.rs`：create → status=installed → remove → status=not-installed（CI 里可能需要 mock）。

### 11.2 手工功能回归清单（按里程碑）

**M6**：
- [ ] 复制 Word 格式文字 → kind=rtf，粘贴到 WPS 保留格式
- [ ] 复制网页选区 → kind=html，粘贴到 Word 保留样式
- [ ] 选中 3 个文件复制 → kind=files，显示 3 路径，粘贴到 Explorer 出现文件
- [ ] 同一文本复制 3 次 → dedup=move_to_top：1 条；=ignore：仍 1 条但时间不变；=always_new：3 条
- [ ] 卡片左上角显示 Chrome/FireFox/VSCode 图标

**M7**：
- [ ] 右键文本卡片 → 菜单完整 8 项
- [ ] 右键 files 卡片 → 「粘贴为路径」→ 记事本显示路径
- [ ] 右键 files 卡片 → 「在资源管理器中显示」→ 正确选中
- [ ] 右键图片 → 另存为 → 磁盘有 png
- [ ] 批量模式选 5 条 → 合并粘贴 → 目标 textarea 得到 `\n`-join
- [ ] 选中第 1 条 → Shift+第 10 条 → 10 条都选中
- [ ] 面板显示时按 Alt+3 → 粘贴第 3 条

**M8**：
- [ ] 设置密度=宽松 → 卡片 104px
- [ ] 预览行数=5 → 文本卡片显示 5 行
- [ ] 时间=绝对 → 显示 "04-19 12:34"
- [ ] 关闭字符数 / 字节 / 应用名 → 卡片元信息行相应隐藏
- [ ] 图片最大高度=80px → 大图缩在 80px
- [ ] 悬浮图片 → 独立窗口（查看任务管理器有额外 WebView）
- [ ] Ctrl+滚轮缩放 → 右下角百分比
- [ ] 预览位置=left → 面板右侧弹出预览
- [ ] 搜索 "hello" → 结果黄底高亮
- [ ] 搜索框 X → 清空
- [ ] 拖动面板后关闭再开 → 位置记忆
- [ ] 调整面板大小后关闭再开 → 尺寸记忆
- [ ] Mica 开 → Win11 看到透背
- [ ] 字号=大 → 整体 16px
- [ ] 音效开 → 粘贴听到 click
- [ ] 工具栏隐藏"批量" → 工具栏少一个按钮

**M9**：
- [ ] 非抢焦点：正在输入 → 按 Alt+C → 输入光标仍在原窗口 → Enter 粘贴
- [ ] 开启"管理员启动" → 首次弹 UAC → 设置成功 → 重启后启动无 UAC 且 UAC 标记为已提升
- [ ] 导出 → 新路径 → 导入（replace）→ 行数一致
- [ ] 导入（merge）→ 不重复（hash 命中忽略）
- [ ] 双击文本卡片 → 编辑 → 保存 → 列表更新
- [ ] 创建分组"工作" → 右键卡片移入 → 切到"工作"仅见这条
- [ ] 右键卡片置顶 → 顶部区显示 → retain_days=1 等待 2 天后仍在
- [ ] GC：手动删一条 image DB 行 → 重启 → images/ 少一个 png
- [ ] VACUUM → DB 文件大小减小
- [ ] 三级清理 每项按钮 + 输入"DELETE" 确认生效
- [ ] 改路径 → 重启 → 数据完整
- [ ] 黑名单加 "chrome.exe" → 在 Chrome 复制文本不入库

### 11.3 性能验收

- 搜索 10000 条记录 ≤ 80ms
- 面板首屏渲染 ≤ 150ms
- 图片悬浮预览弹出 ≤ 150ms（独立窗口启动）
- 滚动 10000 条 ≥ 50fps

### 11.4 CI 验证

每里程碑合并前：
```bash
cargo fmt --check
cargo clippy -- -D warnings
cmd /c pnpm tauri:build:versioned-exe
```

---

## 12. 明确不做

本轮明确不做以下功能，记录理由：

| 功能 | 对方实现 | 不做理由 |
|------|----------|---------|
| 跟随系统强调色 H1 | 读注册表 DWM/ColorizationColor | 主窗口整体视觉体系本轮不变 |
| 预设主题色 H2 | 黑白/翡翠/天空青 | 同上 |
| 暗色模式 H3 | 自动跟随系统 | 整体暗色方案延后一期 |
| 自动更新 F7 | Tauri updater + 系统代理 | CLAUDE.md 已决策不做，沿用整体发布节奏 |
| 系统代理读取 F8 | 注册表 ProxySettings | 无更新功能则无需 |
| 便携版 F6 | unins000.exe 检测 | 本项目统一走安装包 |
| 跨用户 / 系统级部署 | — | 仅当前用户 HKCU |
| 跨平台 | macOS/Linux | 本项目仅 Windows |

---

## 13. 风险与对策

| # | 风险 | 影响 | 对策 |
|---|------|------|------|
| R1 | `arboard` 读 HTML/RTF/Files 版本兼容 | 部分格式取不到 | 自实现 `GetClipboardData(CF_*)` 兜底；补单元测试 |
| R2 | 图标提取失败（某些 UWP 应用） | 卡片显示默认图标 | 失败静默，缓存 "fallback.png" |
| R3 | 读写分离 → 短时 write 未 flush 前 read 看不到 | 刚复制的内容 UI 晚显示 | WAL + write_conn 用 `PRAGMA wal_checkpoint(PASSIVE)` 主动 checkpoint |
| R4 | 独立预览窗口启动延迟感 | 悬浮后 100ms 才弹 | 预先创建隐藏 + 复用；首次 prewarm |
| R5 | 任务计划创建需要首次 UAC | 用户体验不够"一键" | 明确 UI 说明"首次需要管理员授权，之后免" |
| R6 | SWP_NOACTIVATE 后键盘事件不响应 | 面板不可用 | `tauri-plugin-global-shortcut` 面板打开时注册 Esc/↑/↓/Enter/Alt+N/Ctrl+F |
| R7 | Mica 开启回到拖动卡顿 | 用户投诉 | 默认 none，开关由用户选择，文档提示副作用 |
| R8 | 自定义分组删除时 cascade 删光 items | 数据丢失 | 改 `ON DELETE SET NULL`，items 变 ungrouped |
| R9 | 数据目录迁移过程中断电 | 数据双份或损坏 | 两阶段 copy：新库完整 → 切 config → 旧库改 `.bak` 保留 7 天 |
| R10 | 文本编辑对 hash 去重冲突 | 两条相同内容 | 编辑后若 hash 命中其他行，`always_new` 模式允许；其他模式提示"已存在，是否覆盖" |

---

## 14. 附录 A：文件级迁移清单

### 14.1 Rust（src-tauri/src/clipboard/）

| 文件 | 操作 | 内容 |
|------|------|------|
| `mod.rs` | 修改 | 导出新子模块 `data_transfer`、`task_scheduler`、`preview`、`groups`、`icon_store` |
| `db.rs` | 大改 | 读写分离；schema migration v2；新表；新索引；新 CRUD（groups, pin, update_text）|
| `models.rs` | 扩展 | `ClipboardKind::{Rtf, Files}` 补齐；`ClipboardItem` 新字段；`ClipboardGroup` |
| `watcher.rs` | 大改 | HTML/RTF/Files 捕获；图标提取 hook |
| `paste.rs` | 扩展 | `paste_as_path`, `paste_as_files`, `merge_paste`；基于 kind 的优先级链 |
| `source.rs` | 扩展 | `extract_icon`；缓存命中逻辑 |
| `icon_store.rs` | 新建 | 图标 hash 命名与 GC |
| `image_store.rs` | 修改 | 启用 gc_orphan_images 定时任务 |
| `retention.rs` | 修改 | pinned 豁免 |
| `commands.rs` | 大改 | 新增 §7.1 全部 Command |
| `hotkey.rs` | 修改 | Alt+1..9 动态注册/注销 |
| `admin.rs` | 修改 | 接入 task_scheduler；回退 PowerShell |
| `task_scheduler.rs` | 新建 | F5 |
| `data_transfer.rs` | 新建 | A9 import/export zip |
| `preview.rs` | 新建 | B13/B14 独立窗口管理 |
| `groups.rs` | 新建 | A11 分组 CRUD |
| `win_v.rs` | 不变 | — |

### 14.2 Rust（src-tauri/src/）

| 文件 | 操作 | 内容 |
|------|------|------|
| `main.rs` | 修改 | 注册新 Command；在托盘菜单加"剪贴板面板" |
| `config.rs` | 扩展 | §6.2 新结构；`#[serde(default)]` |

### 14.3 Frontend（src/）

| 文件 | 操作 | 内容 |
|------|------|------|
| `lib/tauri.ts` | 扩展 | 新类型 + 新 Command 封装 |
| `lib/store.ts` | 扩展 | 分组、batch 状态 |
| `composables/*` | 新建 5 个 | §8.3 |
| `components/clipboard/*` | 新建 11 个 | §8.1 |
| `pages/ClipboardPanelPage.vue` | 大改 | 整合右键菜单、置顶区、分组栏、工具栏可配置 |
| `pages/ClipboardManagementPage.vue` | 大改 | 批量增收藏/导出；编辑入口 |
| `pages/ClipboardImagePreview.vue` | 新建 | B13 |
| `pages/ClipboardTextPreview.vue` | 新建 | B14 |
| `pages/ClipboardSettingsPage.vue` | 拆分 | 8 tab 重构 |
| `components/clipboard-settings/*` | 新建 8 个 | §8.1 |
| `locales/messages.ts` | 扩展 | ~150 新 key × 2 语言 |
| `router/index.ts` | 扩展 | 新增 /clipboard-preview/image、/text |

### 14.4 配置与构建

| 文件 | 操作 | 内容 |
|------|------|------|
| `src-tauri/Cargo.toml` | 依赖 | 新增 `window-vibrancy`、`zip`、`windows` 追加 feature、可选 `r2d2`/`r2d2_sqlite` |
| `src-tauri/tauri.conf.json` | 新窗口 | `clipboard-image-preview`、`clipboard-text-preview` |
| `package.json` | 可选 | 新增 `tauri-plugin-dialog`（若本项目尚未引入）供 save_as |

---

## 15. 附录 B：新增依赖清单

### Rust

```toml
[dependencies]
window-vibrancy = "0.5"
zip = "0.6"
windows = { version = "0.60", features = [
    "Win32_UI_Shell",
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_Com",
    "Win32_System_Registry",
] }
# 可选：rusqlite 连接池
# r2d2 = "0.8"
# r2d2_sqlite = "0.22"
```

### 前端

已有依赖：`vue-virtual-scroller`、`vue-draggable-plus`、`lucide-vue-next`、`vue-i18n`。
**无新增**（右键菜单自实现，不引入 Radix-like 库）。

可选：`@tauri-apps/plugin-dialog`（若 `save_as` 需要原生保存对话框）。

---

## 16. 结语

完成 M6–M9 后，目标指标：

- ✅ 功能完整度：覆盖 ElegantClipboard 约 **95%**（仅缺主题/更新/便携/跨平台 4 项明确不做）
- ✅ 实现质量：读写分离、孤儿 GC、免 UAC、SWP_NOACTIVATE 等实现细节对齐
- ✅ UI 易用性：卡片个性化、右键菜单、独立预览窗口全量对齐
- 🆕 超越点：更完整的 i18n、内嵌任务中心、更严格的构建与清理

达到本 spec 目标即视为"不低于 ElegantClipboard 源码实现，且在融合工具链上超越"。
