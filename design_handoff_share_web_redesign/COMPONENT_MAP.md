# 组件映射：原型 → 现有 Vue 仓库

> 目标目录：`src/share-web/`
> 原型入口：`prototype/File Share.html` → 渲染 `prototype/src/app.jsx`

---

## 现有 Vue 文件清单（修改前）

```
src/share-web/
├── App.vue                              ← 顶层布局 + 状态编排（保留 script，重写 template + style）
├── api.ts                               ← 不动
├── i18n.ts / messages.ts                ← 加新文案
├── types.ts                             ← 不动（沿用 FileShareNode 等类型）
├── style.css                            ← 全量替换为新 token
└── components/
    ├── EntryTable.vue                   ← 大改：加 view list/grid prop + 多选 + 新样式
    ├── SearchBar.vue                    ← 改：合并范围切换 + 视图切换
    ├── ToolbarActions.vue               ← 改：移到 .page-head 右侧
    ├── LoginDialog.vue                  ← 仅样式更新（继承新 token）
    ├── UploadDialog.vue                 ← 仅样式更新
    ├── CreateDirectoryDialog.vue        ← 仅样式更新
    ├── NewTextDialog.vue                ← 仅样式更新
    ├── RenameDialog.vue                 ← 仅样式更新
    ├── DeleteConfirmDialog.vue          ← 仅样式更新
    └── ImagePreviewDialog.vue           ← 仅样式更新
```

---

## 新增 Vue 组件

| 新组件 | 原型对应 | 说明 |
|---|---|---|
| `components/TopBar.vue` | `<TopBar>` in app.jsx | 顶栏；接收 `session` prop，emit `refresh` / `switch-account` |
| `components/Sidebar.vue` | `<Sidebar>` in app.jsx | 左侧导航 + 存储卡片；接收 `currentPath` / `storage` / `quickLinks` props |
| `components/Breadcrumbs.vue` | `<Crumbs>` in app.jsx | 面包屑；接收 `breadcrumbs` prop，emit `navigate` |
| `components/BulkActionBar.vue` | `.bulkbar` in app.jsx | 浮动批量操作栏；接收 `count` / `permissions`，emit `download-all` / `delete-all` / `clear` |
| `components/FileTile.vue` | `<FileTile>` in app.jsx | 网格视图单 tile |
| `components/EmptyState.vue` | `.empty` 块 | 复用空状态视图 |
| `components/Flash.vue` | `.flash` | 底部 toast |

---

## 详细映射

### A. `App.vue` 顶层布局

**原型**（`app.jsx` 的 `App` 组件 `return` 块）：
```jsx
<div className="app">
  <TopBar … />
  <Sidebar … />
  <main className="main">
    <Crumbs … />
    <div className="page-head">…</div>
    <Notice … />
    <Toolbar … />
    <div className="list-card">…</div>
    {selected.size > 0 && <BulkActionBar … />}
  </main>
  <Flash … />
</div>
```

**Vue 对应**：保留现有 `App.vue` 的 `<script setup>`（所有状态/请求逻辑都对），把 `<template>` 重写为上述结构，`<style scoped>` 删除（统一用 `style.css`）。

样式类 → CSS grid：
```css
.app {
  display: grid;
  grid-template-columns: 256px 1fr;
  grid-template-rows: auto 1fr;
  grid-template-areas: "topbar topbar" "sidebar main";
}
```

### B. `EntryTable.vue` 重构

**保留**：现有的 props（`entries / session / loading / emptyText / searchActive`）和 emits（`open / preview / download / rename / delete`）。

**新增**：
- prop `view: 'list' | 'grid'`（默认 `'list'`）
- prop `selectedIds: Set<string>` + emit `toggle-select: id`、`select-all`
- 列表行 grid 改为 `36px minmax(0, 1fr) 110px 170px 144px`（含复选框列）
- 文件名前的视觉块（`.glyph`）：文件夹 → jade 软色块 + folder 图标；文件 → ext 着色徽标（参考 `app.jsx` 的 `getExtStyle()`）

**原型代码可直接复用**：
- `EXT_STYLES` 表 → 复制到组件 setup
- `getExtStyle()` / `formatSize()` / `timeAgo()` 三个工具函数 → 直接迁移成 TS

### C. `SearchBar.vue` → `Toolbar`

把原来独立的搜索框 + 范围切换，合并为 `.toolbar` 网格，多加一个视图切换：

```vue
<div class="toolbar">
  <div class="search">…</div>
  <div class="scope-toggle">
    <button :class="{ active: scope === 'current' }" :disabled="!canCurrent">当前目录</button>
    <button :class="{ active: scope === 'global' }">全部共享</button>
  </div>
  <div class="view-toggle">
    <button :class="{ active: view === 'list' }"><IconList /></button>
    <button :class="{ active: view === 'grid' }"><IconGrid /></button>
  </div>
</div>
```

### D. `ToolbarActions.vue` 迁移

从原来的位置移到 `.page-head` 右侧 `.page-actions` 容器内。按钮样式使用新的 `.btn` / `.btn.primary` 类。

按权限渲染：
```vue
<div class="page-actions">
  <button v-if="perms.upload_file" class="btn">…上传</button>
  <button v-if="perms.create_directory" class="btn">…新建文件夹</button>
  <button v-if="perms.create_text" class="btn">…新建文本</button>
  <button v-if="!perms.upload_file" class="btn primary" @click="$emit('download-all')">…下载全部</button>
</div>
```

### E. Dialog 组件

`LoginDialog / UploadDialog / RenameDialog / …` **结构不动**，仅替换样式 token：
- 背景：`background: var(--surface)`
- 描边：`border: 1px solid var(--border)`
- 圆角：`var(--r-lg)`（16px）
- 阴影：`var(--shadow-md)`
- 主按钮：`.btn.primary`
- 取消按钮：`.btn`

---

## 状态变更建议

`App.vue` 的 `<script setup>` 里新增：

```ts
import { useStorage } from '@vueuse/core'; // 或手写 localStorage 包装

const view = useStorage<'list' | 'grid'>('share-web:view', 'list');
const theme = useStorage<'light' | 'dark'>('share-web:theme', 'light');
const density = useStorage<'cozy' | 'compact'>('share-web:density', 'cozy');

const selectedIds = ref(new Set<string>());

watchEffect(() => {
  document.documentElement.dataset.theme = theme.value;
  document.documentElement.dataset.density = density.value;
});

// 切换目录/范围/搜索词时清空选择
watch([currentNodeId, activeKeyword, activeSearchScope], () => {
  selectedIds.value = new Set();
});
```

---

## I18n 文案补充

需要追加到 `messages.ts` 的中文键（按当前命名风格）：

```ts
app: {
  // 现有键…
  guestMode: '当前为访客模式，仅可浏览和下载。需要管理操作请通过右上角切换账号。',
  deviceUptime: '已运行 {n}',
  bulkDownload: '打包下载',
  bulkDelete: '删除',
  bulkClear: '取消',
  selectedCount: '{count} 项已选择',
  viewList: '列表',
  viewGrid: '网格',
  scopeCurrent: '当前目录',
  scopeGlobal: '全部共享',
  sortByModified: '修改时间 · 最近优先',
  storageLabel: '设备存储',
  filesShared: '共享文件',
  todayDownloads: '今日下载',
  recentLabel: '最近',
  pinned: 'PINNED',
}
table: {
  selectAll: '全选',
  // 现有键…
}
```

英文对应键也需要在 i18n/ 下补齐。

---

## 实施顺序建议

1. **第 1 步**：替换 `style.css`（拷贝 `prototype/src/styles.css` 的 :root + 全局重置 + .topbar 等基础块）
2. **第 2 步**：新增 `TopBar.vue` + `Sidebar.vue` + `Breadcrumbs.vue` 三个布局组件，并在 `App.vue` template 引入
3. **第 3 步**：改 `EntryTable.vue` — 加 view prop、ext glyph、复选框列
4. **第 4 步**：重写 `SearchBar.vue` → toolbar 三段布局
5. **第 5 步**：新增 `BulkActionBar.vue` + 多选状态
6. **第 6 步**：迁移 `ToolbarActions.vue` 位置 + 重新按权限分组
7. **第 7 步**：dialogs 样式过一遍
8. **第 8 步**：手测：访客 / 管理员 / 空目录 / 搜索无结果 / 紧凑密度 / 暗色模式 / 移动端
