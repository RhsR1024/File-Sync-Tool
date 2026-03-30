# 其他工具页面头部视觉优化设计规格

**日期：** 2026-03-28
**文件：** `src/pages/ToolsHubPage.vue`

---

## 背景

当前「其他工具」页面顶部 banner 区域（`<section>`）在去掉工具数等统计信息后显得空旷。需要在不破坏整体白色风格的前提下，增加视觉层次感。

---

## 选定方案：C1 — 浅色渐变 + 右侧彩色图标集群

### 布局变化

头部 section 内部由当前的单列（仅标题 + 描述）改为 **左右两栏 flex 布局**：

```
[左：eyebrow + 标题 + 描述]    [右：4个工具图标]
```

### 左侧：标题区升级

当前只有标题和描述两行，优化后增加 eyebrow 标签行：

1. **Eyebrow 标签**（新增）：蓝色实心小圆点 + "工具中心" 大写小字（对应 i18n key `toolsHub.eyebrow`，已存在于 messages.ts）
2. **标题**：保持不变（`toolsHub.title`）
3. **描述**：保持不变（`toolsHub.description`）

### 右侧：工具图标集群（纯装饰）

4 个图标按钮，对应 4 个工具，从左到右排列：

| 工具 | Lucide 图标 | 渐变色 | 阴影色 |
|------|------------|--------|--------|
| 修改框架密码 | `KeyRound` | `from-amber-500 to-orange-600` | `amber-500/25` |
| 开启一体机SSH | `Shield` | `from-sky-400 to-indigo-500` | `sky-400/25` |
| 代码修改统计 | `BarChart3` | `from-emerald-500 to-teal-500` | `emerald-500/25` |
| 网络工具 | `Globe` | `from-violet-500 to-fuchsia-500` | `violet-500/25` |

每个图标按钮规格：
- 尺寸：`h-11 w-11`（44×44px）
- 圆角：`rounded-[14px]`
- 图标尺寸：`h-5 w-5`，白色
- 背景：`bg-gradient-to-br` + 对应色
- 阴影：`shadow-lg` + 对应 shadow 颜色类

### 背景装饰

保持现有两个 blur 光晕不变（已在当前代码中存在）：
- 右上角：`bg-sky-100/80 blur-3xl`
- 右下方：`bg-amber-100/70 blur-2xl`

### i18n

eyebrow 文本 `toolsHub.eyebrow` 已在 `messages.ts` 中存在（`en: 'Tool Center'` / `zh` 需补充 `'工具中心'`）。

---

## 不变的内容

- 整体页面背景渐变不变
- 工具卡片网格区不变
- 路由、事件、store 均不涉及（纯视觉改动）
- 右侧图标**仅装饰**，不可点击，不跳转

---

## 文件改动范围

| 文件 | 改动 |
|------|------|
| `src/pages/ToolsHubPage.vue` | 修改 `<section>` 内部布局，新增右侧图标集群 |
| `src/locales/messages.ts` | 补充 `toolsHub.eyebrow` 的中文翻译 `'工具中心'` |
