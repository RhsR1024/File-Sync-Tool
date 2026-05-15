# 交接说明：File Share Web UI 重新设计

> 目标仓库：`src/share-web/` （Vue 3 + TypeScript + Vue I18n）
> 设计保真度：**高保真**（hifi）— 颜色、字体、间距、交互均为最终态

---

## 1. 这个包是什么？

`prototype/` 下的所有文件是用 **HTML + React + Babel** 写的**设计参考原型**，用于演示新版 UI 的视觉和交互。

> ⚠️ **不要直接把这些文件部署进生产环境。** 任务是把原型呈现的视觉/交互在现有的 Vue 3 + TS 代码（`src/share-web/`）中重新实现，沿用项目现有的状态管理、API 调用、I18n、错误处理流程。

CSS 文件（`prototype/src/styles.css`）是**最重要**的参考 — 几乎可以直接拷贝到 Vue 项目，类名也可以保留。

---

## 2. 你最需要看的三个文件

| 文件 | 作用 |
|---|---|
| `prototype/File Share.html` + 浏览器打开 | **看一眼完整效果。** 右下角的 Tweaks 面板可以切换：明/暗、强调色、密度、列表/网格、访客/管理员 |
| `prototype/src/styles.css` | **所有视觉规则。** 颜色 tokens、组件样式、响应式断点 — 可大量复用到 Vue 项目 |
| `DESIGN_TOKENS.md` + `COMPONENT_MAP.md` | **设计 token 列表** 和 **原型组件 ↔ 现有 Vue 组件的映射** — 决定改哪些文件 |

---

## 3. 设计概览

### 主题方向
现代精致的"运维控制台"风格，致敬原版的绿色基调但更稳重克制：
- **强调色**：jade（茶绿）`oklch(0.58 0.10 175)`，比原版更低饱和
- **背景**：极浅的冷调白，配合两个柔和的径向渐变光晕（左上+右上）
- **字体**：Manrope（西文/数字）+ JetBrains Mono（路径/时间戳/技术信息）+ 系统中文
- 完全使用 **OKLCH 色彩空间**，明暗模式自动保持感知一致

### 主要改进点
1. **设备身份明示** — 顶栏直接显示 hostname、IP、运行时长（替代原版的水印背景）
2. **左侧导航栏** — 共享目录快速跳转 + 最近访问 + 存储卡片
3. **文件类型可视化** — 文件夹用 jade 软色块，其他文件按扩展名上色（OKLCH hue 映射）
4. **批量操作** — 行多选 → 浮出黑色胶囊批量栏（打包下载/删除/取消）
5. **网格视图** — 工具栏可切换到大图模式
6. **时间双行展示** — 绝对时间 + 相对时间（如 "2 天前"）
7. **权限/账号清晰化** — 访客模式有顶部 info 条；切换到管理员视图后操作按钮自动出现
8. **密度可调** — 紧凑 / 舒适两挡，影响行高
9. **响应式** — 980px 以下隐藏侧边栏，760px 以下表格降级为单列

---

## 4. 屏幕清单（每个区块对应的现有 Vue 文件请看 `COMPONENT_MAP.md`）

### 4.1 顶栏 `<header class="topbar">`
**用途**：品牌、当前设备状态、当前账号、刷新、切换账号

**布局**：`display: flex`，从左到右：
- 品牌组（36×36 jade 方形 logo + "File Share" 标题 + IP:端口副标题）
- 设备状态胶囊（绿色脉冲点 + hostname + 运行时长）
- spacer（`flex: 1`）
- 刷新按钮（图标 + 文字）
- 用户胶囊（首字母 avatar + username + 角色）
- 切换账号按钮

**关键样式**：
- 顶栏 `position: sticky; top: 0`，半透明 `backdrop-filter: blur(10px)`
- 设备点：`width: 8px; height: 8px;` 圆形 + 3px 同色光晕（`box-shadow: 0 0 0 3px var(--ok-soft)`）
- 用户 avatar：28×28 圆形，jade 软色背景 + jade 描边，深色文字

### 4.2 左侧导航 `<aside class="sidebar">`
**宽度**：256px；`position: sticky` 在顶栏下方；可滚动
**分组**：
1. "首页" 单项（home 图标）
2. "共享目录" — 列出 UMS_TEMP / Releases / Documents 等顶级目录，右侧带计数胶囊
3. "最近" — 列出近期访问的两个条目
4. 存储卡片 — 已用/总量数字 + 进度条 + 共享文件数 + 今日下载数

**激活态**：当前所在目录的项 = jade 软色背景 + jade ink 文字

### 4.3 面包屑 `<nav class="crumbs">`
"首页 / UMS_TEMP / 2026_05_12..."，每段都可点击跳转，最后一段加粗不可点。

### 4.4 页面标题区 `.page-head`
- H1 大标题（当前文件夹名） + 同行的"X 文件夹 · Y 文件"统计胶囊
- 标题下方 `.page-sub`：目录用途提示或最近更新时间
- 右侧 actions：根据权限显示「上传」「新建文件夹」「新建文本」（管理员），或「下载全部」（访客）

### 4.5 通知条 `.notice`
浅蓝色背景的胶囊。访客模式默认显示"当前为访客模式，仅可浏览和下载…"，右侧可关闭。

### 4.6 工具栏 `.toolbar`
**网格布局**：`grid-template-columns: minmax(260px, 1fr) auto auto`
1. 搜索框（icon + input + ⌘K kbd）
2. 范围切换（当前目录 / 全部共享）— 分段控件，激活态 jade 软色
3. 视图切换（列表 / 网格）— 图标按钮

**焦点态**：搜索框 focus-within 时显示 jade 软光环（`box-shadow: 0 0 0 4px var(--accent-soft)`）

### 4.7 列表视图 `.list-card`
顶部 meta 条：项数、文件夹/文件分布、排序方式（修改时间·最近优先）

行 grid 布局：`36px minmax(0, 1fr) 110px 170px 144px`
| 列 | 内容 |
|---|---|
| 复选框 | 18×18 圆角方块；选中态填充 jade + 白色对勾 |
| 名称 | 40×40 ext 徽标 + 文件名（含 PINNED 标签）+ 提示行 |
| 大小 | mono 字体；文件夹显示项数 |
| 修改时间 | 绝对时间 + 灰色 "X 天前" |
| 操作 | 预览 / 下载 / 重命名 / 删除 图标按钮（按权限显示） |

**选中行**：jade 软色背景 + 左侧 3px jade 竖条

### 4.8 网格视图 `.grid-card`
`grid-template-columns: repeat(auto-fill, minmax(180px, 1fr))`
每个 tile：4:3 ext 徽标 + 文件名（最多 2 行）+ meta 行（大小 / 相对时间）+ 右上角悬浮的下载按钮（hover/selected 才显示）

### 4.9 批量操作栏 `.bulkbar`
**位置**：`position: sticky; bottom: 16px`，居中
**外观**：深色（near-black）胶囊 + jade 计数圆 + 白色操作按钮（打包下载、删除、取消）

### 4.10 空状态 `.empty`
64×64 虚线圆角容器内放 search 图标 + 标题 + 子文字。区分"目录为空"和"搜索无结果"。

### 4.11 浮窗提示 `.flash`
底部居中绿色胶囊，1.8s 后自动消失。用于"已开始下载"、"已切换为管理员视图"等反馈。

---

## 5. 关键行为说明

### 角色切换
- 顶栏「切换账号」按钮 → 在现有代码里复用 `LoginDialog`（原型里用 Tweaks 模拟，生产环境保留登录弹窗即可）
- **访客**（`is_guest: true`）：隐藏 上传/新建/重命名/删除 全部 mutation 按钮；通知条提示"仅浏览下载"
- **管理员**：显示完整操作集；批量栏多出"删除"按钮

### 搜索
- 输入即时本地过滤（≥1 字符触发）；在 Vue 项目里保留现有 `executeSearch` 防抖逻辑
- 范围切换：`current` 仅在非首页可用；首页强制 `global`
- 搜索结果行的副文字应显示 `display_path`（已有字段）

### 多选
- 行内复选框点击 toggle
- 列头复选框全选/取消全选
- `selected.size > 0` 时显示底部批量栏
- 切换目录/范围/搜索词时清空选择

### 视图切换
- list / grid 状态应持久化到 `localStorage`（key 建议 `share-web:view`）
- grid 视图的 tile 点击行为：单击进入/预览/下载（与 list 一致），shift+点击多选

### 主题/密度
- `data-theme="dark"` 切到深色模式（CSS 已经写好对应 token）
- `data-density="compact"` 切到紧凑（行高 44px），默认 `cozy`（56px）
- 这两个建议存到 `localStorage` 并在「设置」菜单暴露切换；或者只读 `prefers-color-scheme`

### 强调色
原型支持 5 种 hue（jade / cobalt / amber / plum / slate），通过修改 `--accent-h` 实现。生产环境**只保留 jade**（or 给用户偏好），其他几个是设计探索用途。

---

## 6. 集成步骤建议

1. **先拷贝设计 token**：把 `prototype/src/styles.css` 顶部的 `:root` 块（约 70 行）放到 `src/share-web/style.css`，覆盖现有 token。
2. **重写 `App.vue` 的顶层布局**：从单列改为 `grid-template-areas: "topbar topbar" "sidebar main"`。
3. **拆出新组件**：
   - `components/TopBar.vue`（顶栏 — 品牌、设备、用户、切换）
   - `components/Sidebar.vue`（左侧导航 + 存储卡片）
   - `components/Breadcrumbs.vue`（面包屑 — 可能现有代码里就有，仅样式更新）
   - `components/BulkActionBar.vue`（批量操作浮条）
4. **重构现有组件**：
   - `EntryTable.vue` 加 `view: 'list' | 'grid'` prop，list 模式保留现有结构但用新样式；grid 模式渲染 tile 网格
   - `SearchBar.vue` 合并范围切换 + 视图切换，整体改用 `.toolbar` 网格
   - `ToolbarActions.vue` 移到 `.page-head` 右侧的 `.page-actions`
5. **保持 API/状态不变**：所有 `fileShareApi.*` 调用、`session/tree/searchResults` 等响应式状态全部沿用。
6. **i18n**：原型里的中文字面量需要登记到现有 `i18n.ts`/`messages.ts`。

---

## 7. 包内文件

```
design_handoff_share_web_redesign/
├── README.md                  ← 本文件
├── DESIGN_TOKENS.md           ← 颜色 / 字体 / 间距 / 阴影完整 token 列表
├── COMPONENT_MAP.md           ← 原型组件 → 现有 Vue 组件的对应关系
├── screenshots/
│   ├── README.md              ← 截图索引
│   ├── 01-list-guest-light.png
│   ├── 02-list-admin-light.png
│   ├── 03-grid-admin-light.png
│   ├── 04-selected-bulkbar.png
│   ├── 05-dark.png
│   └── 06-search-empty.png
└── prototype/
    ├── File Share.html        ← 入口；直接在浏览器打开
    └── src/
        ├── app.jsx            ← React 实现参考（行为/状态逻辑可参考）
        ├── data.js            ← Mock 数据（结构对应真实 FileShareNode）
        ├── styles.css         ← 全部 CSS — 高度可复用
        └── tweaks-panel.jsx   ← 设计工具用，生产环境忽略
```

---

## 8. 备注

- 现有项目用了 i18n，本次中文文案在 `app.jsx` 内是字面量 — 请按需迁移到 `messages.ts`
- `tweaks-panel.jsx` 是给设计师调参用的浮层，**生产环境删除**
- React 原型用 `useTweaks` 模拟用户偏好；Vue 项目用 `localStorage` + reactive 即可
- 时间显示原型里用了 `timeAgo()` 函数（基于 "2026-05-14" 计算），生产环境用 `new Date()` 替换
- 文件类型 ext 着色查表在 `app.jsx` 的 `EXT_STYLES`，可直接复制到 Vue 端 `EntryTable` 的 setup 块

如有疑问，对照原型截图最直观 — 直接在浏览器打开 `prototype/File Share.html`。
