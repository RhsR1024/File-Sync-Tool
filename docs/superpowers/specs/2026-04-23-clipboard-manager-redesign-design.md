# Clipboard Manager 重设计 — 面板边框 / 设置页精简 / 自复制回写策略

## 背景

当前剪贴板管理工具存在三个独立但相邻的问题：

1. **Alt+C 快捷面板窗口的边框不一致**：左 / 右 / 下三边可见一条深色细线，上边没有。视觉上不协调。
2. **工具页（`/tools/clipboard`，即 `ClipboardManagerPage.vue`）与 Alt+C 快捷面板功能严重重复**：搜索、筛选、分组侧栏、置顶区、列表、批量操作、上下文菜单 — 几乎一比一复刻。两套 UI 维护同一数据源，成本高且 UX 定位模糊。
3. **在 Alt+C 中点击"复制"后，该条目会跳回列表顶部，而且历史记录里没有可识别的来源标签**。行为是否合理存疑，且目前没有让用户自主选择的开关。

这三个问题都指向剪贴板面板的 UX 一致性，合并为一次设计处理。

## 目标

- WI-1：统一 Alt+C 面板四边的视觉边框，消除"上边无线、三边有线"的不协调。
- WI-2：将 `/tools/clipboard` 改造为**只承载"设置"**的专用页，把 CRUD 的职责完全交给 Alt+C 快捷面板。
- WI-3：引入**"复制后是否回写为最新记录"**的设置开关，并在回写时把历史记录的来源标记为"本工具"，让来源可识别。

## 非目标

- 不改变 Alt+C 面板的功能集（搜索、筛选、分组、批量仍然保留）。
- 不改动现有去重策略（`MoveToTop / Ignore / AlwaysNew`）的语义 — 它们只影响**外部来源**的剪贴板变更。
- 不做剪贴板管理相关的数据迁移脚本；`from_self` 在 DB 层默认为 `false`，旧数据无需回填。
- 不改造 Win32 来源捕获逻辑；本次只新增"是否自写"的旁路信号。

---

## WI-1：Alt+C 面板边框一致性

### 现状分析

- `src-tauri/src/main.rs:2701-2713` 创建面板时使用 `decorations(false) + resizable(false) + always_on_top(true)`。
- 由于 `decorations=false`，Windows 会自行绘制一条 1px 的系统级边框（resize edge 的视觉提示）。左 / 右 / 下三边可见；顶边因 `ClipboardPanelPage.vue` 的 `<header>` 自带 `border-b` 而与系统线对齐或遮挡，给人"顶部没有线"的错觉。
- 面板根元素当前样式：`class="flex h-screen w-screen flex-col overflow-hidden bg-white"` —— **没有任何自绘边框**。

### 方案

在 **CSS 层** 统一处理，不改 Rust 端的窗口参数（避免影响阴影、focus-loss 行为等）：

1. `ClipboardPanelPage.vue` 根 `<div>` 改为包含 `border border-slate-200 rounded-xl`，四边各 1px，颜色与现有 UI 的 `slate-200` 语汇一致。
2. 去掉现有 `<header>` 上的 `border-b` —— 改由根元素的圆角边框统一承担外边，`<header>` 只保留背景色和内边距。
3. 需要保留"header 和 body 之间的分隔感"时，在 body 顶部加 `border-t border-slate-100`（浅灰内分割），与外边框形成层级。
4. 若在少数机型上系统线依然透过 CSS 边框可见，再在 Rust 端补 `.shadow(false)`；作为次选项，不是首选。

### 验收

- Alt+C 面板四边视觉一致：同颜色、同粗细、同圆角。
- 顶部不再有"header-b 的深色横线"与"外边框"双重线。
- 在浅色 / 深色桌面背景下皆无系统残留线透出。

---

## WI-2：工具页改造为设置专用页

### 现状分析

- `ClipboardManagerPage.vue` 当前同时承担：
  - 页头（title + description）
  - 可折叠的设置面板（`<details>` 包裹 `ClipboardSettingsPanel`）
  - 统计卡 (`ClipboardStats`)
  - 工具条 (搜索 + 筛选 + 批量 + 齿轮)
  - 批量操作条
  - 分组侧栏 + 置顶区 + 列表
  - 右键菜单、合并粘贴、文件详情弹窗等
- 双重标题："clipboard.tool.title" 出现在页头，"clipboard.settings.title" 又出现在 `<details>` summary 和 `ClipboardSettingsPanel` 内部 — 三层叠加，视觉冗余。

### 方案

`ClipboardManagerPage.vue` 改造后的最终结构（自上而下）：

```
<div.bg-gradient-to-b.from-slate-50.to-white>
  <div.container.max-w-5xl>
    <header>
      <h1>剪贴板管理</h1>
      <p>打开 Alt+C 使用快捷面板；此页面仅配置剪贴板行为。</p>
    </header>

    <ClipboardSettingsPanel />  <!-- 完全展开，不再折叠 -->
  </div>
</div>
```

删除：

- `<details>` 折叠容器及其 summary
- `ClipboardStats`
- 所有搜索 / 筛选 / 工具条 / 批量 UI
- `ClipboardGroupSidebar`、`ClipboardPinnedSection`、`ClipboardList`
- `ClipboardCardMenu`、`ClipboardFileDetailsDialog`、`ClipboardMergePasteDialog`
- 相关 composable 调用 (`useClipboardContextMenu`)、本地状态 (`selectedId / reloadCounter / copyToast` 等)、CRUD handler
- 相关 i18n 文案如果不再被其它页面引用，同步清理

保留：

- `ClipboardSettingsPanel.vue` 本体（它已经是完整的 tab 容器）。
- `ClipboardSettingsPanel` 内部的面板标题 "剪贴板管理 / 常规" 保留不变 — 它是**设置面板自身**的标题，不再与页头冲突，因为页头改为提示性描述。
- Tab 内的所有现有功能（常规 / 显示 / 快捷键 / 数据 / 预览 / 应用过滤 / 关于）。

### 布局细节

- 页面主容器从 `max-w-7xl` 收窄到 `max-w-5xl`，让设置内容不会被拉得过宽（tab 栏密度适中）。
- 主标题 `clipboard.tool.title` 保留；描述文案改为：
  - zh：`打开 Alt+C 使用剪贴板快捷面板；此页面仅配置剪贴板行为。`
  - en：`Press Alt+C to open the clipboard quick panel. Use this page to configure clipboard behavior.`
- `ClipboardSettingsPanel` 外侧仍保留圆角卡片边框，不做额外包裹。
- 底部"统计卡（总记录 / DB 大小 / 图片数量）"并入 Data tab 或 About tab（后续由 ui-ux-pro-max 视觉调优时决定归属）。**本次迁移不丢数据**，只是位置换。

### 验收

- `/tools/clipboard` 页面打开后，除页头外只显示设置面板；完全没有列表 / 搜索 / 分组 UI。
- 没有重复出现的"剪贴板管理"或"设置"标题（页头 1 处 + 面板自身 1 处，各有其语义，不视为冗余）。
- 所有设置 tab 功能行为与改造前完全一致。
- Alt+C 面板功能不受影响。

### 视觉调优

WI-2 落地后，进一步用 `ui-ux-pro-max` 技能生产设置页的视觉 mockup：重点放在 tab 栏节奏、表单行密度、卡片分组布局、统计卡摆放位置。视觉调优 PR 可以在功能改造合并后单独追加。

---

## WI-3：复制后是否回写 + 来源标记

### 现状分析

- 在 Alt+C 点击条目调用 `cb_copy`（`src-tauri/src/clipboard/commands.rs:558`）→ `copy_item`（`src-tauri/src/clipboard/paste.rs:37`）→ `write_to_clipboard`。
- 我们的 watcher（`watcher.rs:82` 的 `on_clipboard_change`）监听 Win32 剪贴板事件，**分不清**这次变更是外部应用触发还是我们自己写的。
- 结果：我们自己写进去的数据，也被当作一次新 capture，走 `upsert_item_with_dedup`。默认策略 `MoveToTop` → 原条目被移到顶部。
- `source_app` 是通过 Win32 的前台窗口捕获的；我们写入时前台窗口可能是目标应用（如记事本），也可能是我们自己，因此来源标签表现不稳定、缺失或错误。

### 方案

增加一个**自写旁路通道**，让 watcher 能识别"刚才这次变更是我们发起的"，再根据设置决定是写入还是跳过。

#### 数据模型

1. `src-tauri/src/clipboard/models.rs`：
   - `ClipboardSettings` 增加字段：
     ```rust
     pub reinsert_on_self_copy: bool,   // 默认 false
     ```
   - `ClipboardItem` 增加字段：
     ```rust
     pub from_self: bool,               // 默认 false
     ```
   - `NewItem` 同步增加 `from_self: bool`。
2. `src-tauri/src/clipboard/db.rs`：
   - `clipboard_items` 表新增列 `from_self INTEGER NOT NULL DEFAULT 0`；在现有 migration 链上追加一次升级。
   - 所有 `select` 查询补充该列；所有 `insert` 带上该列的写入。
3. `src/lib/clipboardTypes.ts` 与 `src/lib/clipboardTypes.contract.test.ts`：对齐 Rust 的字段变更，保证契约测试通过。

#### 自写旁路通道

- 在 `ClipboardState` 上新增：
  ```rust
  pub pending_self_write: Mutex<Option<String>>, // 最近一次自写的内容 hash
  ```
- `paste.rs` 的 `write_to_clipboard / write_text_to_clipboard / write_image_to_clipboard / write_files_to_clipboard` 在真正写入系统剪贴板 **之前** 计算 `hash`（沿用现有 `db::hash_item` 或等价逻辑），然后写入 `pending_self_write`。
  - 写入顺序：**先** 更新 `pending_self_write`，**再** 写系统剪贴板。避免 watcher 抢在旗标之前读到事件。
  - 为避免旗标永久滞留（比如外部程序此时也写入了剪贴板，把 hash 弄岔了），`pending_self_write` 带 500ms 的时间戳；超时自动失效。实现方式：把字段改为 `Mutex<Option<(String, Instant)>>`。
- `watcher.rs:82` 的 `on_clipboard_change` 在 capture + 计算 hash 之后、调用 `upsert_item` 之前：
  1. 取出并清空 `pending_self_write`。
  2. 如果 `pending_self_write` 的 hash 与当前 capture 的 hash 一致，且未超时：这是一次自写事件。
     - 读取 `ClipboardSettings.reinsert_on_self_copy`。
     - `false` （默认）：**直接 return**，不 insert，不发 `clipboard-item-added` 事件。
     - `true`：继续走 `upsert_item_with_dedup`，但在构造 `NewItem` 前把 `from_self = true`、`source_app = None`、`source_app_icon = None`。
  3. 如果不是自写：保持现有行为不变。

#### UI 渲染

- `ClipboardItem` 在列表 / 卡片上显示来源时，**优先检查 `from_self`**：
  - `from_self === true` → 渲染专用徽章 "本工具" / "This tool"，图标用 lucide `Package` 或当前 app 图标；颜色区别于真实应用（如 slate 底 + slate-700 文字）。
  - `from_self === false` → 走现有 `source_app` / `source_app_icon` 分支。
- i18n 新增：
  - `clipboard.source.self` → zh: `本工具`，en: `This tool`
- 设置 UI（General tab）新增一行开关：
  - 标签：`clipboard.settings.general.reinsertOnSelfCopy`
    - zh：`复制后回写为最新记录`
    - en：`Re-insert as newest record after self-copy`
  - 描述：
    - zh：`关闭时：在快捷面板复制条目不改变历史顺序；开启时：会把该条目移到顶部，并把来源标记为"本工具"。`
    - en：`Off: clicking to copy in the quick panel doesn't change history order. On: the item moves to the top and is tagged as "This tool".`

### 验收

- 默认状态下（`reinsert_on_self_copy = false`）：
  - 在 Alt+C 点击任一条目 → 系统剪贴板内容切换；该条目在 Alt+C 列表中的位置**不动**；历史记录没有新增重复条目。
  - 外部应用的复制仍然正常捕获、正常插入 / MoveToTop。
- 开启 `reinsert_on_self_copy = true` 时：
  - 点击条目 → 历史记录顶部出现一条新记录 (或 MoveToTop)；该条目徽章显示为"本工具"。
  - 外部应用的复制仍然显示正确的 `source_app`。
- 开关切换即时生效（读取 `state.settings` 即可），无需重启 watcher。
- 无论开关状态如何，`paste` / `paste_plain` / `pastePlain` / `paste_as_files` / `paste_as_path` 等"粘贴"路径也一并进入自写旁路 —— 它们同样会写系统剪贴板，逻辑与 `copy` 一致。

---

## 交付与实施顺序

三个 WI 彼此独立。推荐顺序：

1. **WI-1**（最小、视觉无副作用） → 立即合并。
2. **WI-3**（数据模型 + 行为变更，最容易引出回归） → 第二个合并，在主要场景跑一轮再动 UI。
3. **WI-2**（UI 精简 + 大量删除） → 最后合并。因为它删除的代码可能依赖 WI-3 的上层 UI 设定，先让 WI-3 稳定再精简。

每个 WI 都对应一次 `pnpm tauri:build:versioned-exe` 验证和一次中文 commit。

## 风险

- **WI-3 数据库 migration**：新增列需要幂等 migration 和对应测试；老库首次启动要自动加列。
- **WI-3 旗标超时**：500ms 窗口过短可能漏判（如 watcher 线程繁忙），过长则可能在连续快速复制时错判。初版选 500ms，留出调参空间。
- **WI-2 删除代码范围**：务必确认被删除的 composable / dialog 组件没有在其它页面被引用 —— `ClipboardCardMenu`、`ClipboardFileDetailsDialog`、`ClipboardMergePasteDialog` 都在 Alt+C 面板里还有用，**不要删组件本身**，只删它们在工具页的使用。
