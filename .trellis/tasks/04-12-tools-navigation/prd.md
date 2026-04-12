# brainstorm: tool navigation optimization

## Goal

优化左侧导航栏的工具入口体验，减少进入具体工具（如“开启一体机 SSH”）的点击层级（当前需要先点击“工具”，再点击具体工具卡片），提升效率。

## What I already know

* 当前的侧边栏 `Sidebar.vue` 有 5 个一级菜单：Console, Tasks, History, Settings, Tools。
* Tools 对应 `/tools` 路由（`ToolsHubPage.vue`）。进入后是以卡片式平铺了所有的工具（如 Framework Password, Appliance SSH, Code Statistics, Network Tools, Screen Share, File Share）。
* 目前用户抱怨进入某个具体工具需要两步。

## Assumptions (temporary)

* 工具的数量还在可控范围内（目前大概 6 个左右）。
* 我们希望保持整体界面的清爽，避免左侧菜单变得非常长。

## Requirements

* 使用“手风琴折叠菜单” (Option A) 实现侧边栏的升级
* 需要使用优雅的视觉设计（不使用大面积底色，而是采用缩进+高亮左侧引导线的方式）
* 默认展开逻辑需和当前路由绑定，如果用户停留在任何一个子工具，工具菜单都应保持展开
* 依然保留跳转 /tools 页面的一级入口能力

## Decision (ADR-lite)

**Context**: 侧边栏到子工具需要两次跳转，太繁琐。
**Decision**: 决定采用带有平滑过渡和极简引导线外观的左侧可展开子菜单 (Option A)。
**Consequences**: 这避免了对移动端的干扰，且保证了深色主题工具栏的美观。由于需要新增内部子路由联动，我们要确保 `<transition>` 动画的流畅度。

## Technical Notes

* `src/components/Sidebar.vue` 负责渲染左侧侧边栏。
* `src/pages/ToolsHubPage.vue` 负责渲染工具卡片合集页面。
* 当前使用了 Lucide Icons (`lucide-vue-next`) 和 Vue Router。

## Research Notes

### 常见的优化模式：

**Approach A: 侧边栏可展开手风琴菜单 (Collapsible Submenu)**
* **原理**：将 Sidebar 里的 "Tools" 变成一个可以展开/收起的父级菜单。展开后下方列出所有 6 个具体工具，点击可直接跳入。
* **优点**：最直观，一步搞定，所见即所得。业界最常用的管理后台模式。
* **缺点**：展开时会让侧边栏的高度变大。

**Approach B: 悬浮/弹出菜单 (Flyout Popover)**
* **原理**：悬停工具栏或点击 "Tools" 时，旁边弹出一个气泡菜单（基于目前深色侧边栏的浮动层）。
* **优点**：节省侧边栏的纵向空间，布局保持紧凑。
* **缺点**：对移动端或平板不太友好，悬停体验需要精细调整以防误触发。

**Approach C: “最近使用”或“钉选”到侧边栏 (Pinned/Recent Tools)**
* **原理**：依然保留 Tools 页面，但允许用户把常用的 1-2 个工具固定到左上角/侧边栏直接展示。
* **优点**：满足个性化高频操作需求，侧边栏保持极度干净。
* **缺点**：开发成本最高，需要记录用户配置状态。
