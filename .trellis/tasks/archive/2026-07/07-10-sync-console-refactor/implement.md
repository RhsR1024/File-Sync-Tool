# 同步控制台重构 — 实施计划

## 前置条件（必须满足才可 task.py start 后动工）

- [ ] 工作区当前有 remote-package-patch（in_progress）的未提交改动，且与本任务重叠 `src/App.vue`、`src/locales/messages.ts`。**用户已决定（2026-07-10）：等该任务提交落库后在主工作区实施，不用 worktree。**
- [ ] 确认 `src/App.keepAlive.test.mjs` 已随 remote-package-patch 落库，据其断言方式同步修改。

## Phase A：后端分域保存（可独立提交）

- [ ] A1 `config.rs`：新增 `SyncConfigPatch`、`AppDomainConfigPatch`（Deserialize；字段类型逐一对照 `AppConfig` 现有定义）+ `apply_sync_patch` / `apply_app_patch` 纯函数。
- [ ] A2 `config.rs` `#[cfg(test)]`：合并函数单测——同步补丁不动应用域与 `last_update_check_at`/`pending_update`；应用补丁反向同理。
- [ ] A3 `main.rs`：新增 `update_sync_config` / `update_app_config` command 并注册到 `invoke_handler`。`update_app_config` 保留 `save_config_cmd`（L1061）的副作用：`sync_launch_on_startup`、server_url 变化时重置 `last_update_check_at` 并调 `updater::commands::handle_config_changed`。
- [ ] A4 验证：`cargo test`（src-tauri 内）、`cargo check`。**禁止** `cargo fmt` / 对 main.rs 跑 rustfmt（会递归 fmt 全 crate，见项目记忆）；只对新增代码块手工保持风格。clippy 存量 deny error 会 exit 101，不作为门禁。

## Phase B：前端配置基础设施

- [ ] B1 `src/lib/tauri.ts`：新增 `SyncConfigPatch` / `AppDomainConfigPatch` 接口与 `updateSyncConfig` / `updateAppConfig` invoke 封装。
- [ ] B2 `src/lib/configDomains.ts`：`buildSyncPatch` / `buildAppPatch` 纯函数。
- [ ] B3 `src/lib/configStore.ts`：共享响应式 store（ensureLoaded/refresh/saveSync/saveApp；saveSync 成功后 `restartSchedulerInterval()`；saveApp 成功后同步 `appStore.maxLogLines`）。
- [ ] B4 `src/lib/configDomains.test.mjs`：node --test 覆盖补丁字段完备性（sync ∪ app ∪ {last_update_check_at, pending_update} = AppConfig 全字段，且两域不相交）。

## Phase C：从 SettingsPage 提取同步域组件

- [ ] C1 建 `src/components/sync/`，按 design.md §2 逐卡提取（含弹窗与脚本逻辑），全部绑定 configStore、保存走 `saveSync()`。沿用现有 `settings.*` i18n key。
- [ ] C2 SettingsPage 瘦身：删除同步域卡片与相关脚本（内置命令组、任务/服务器/命令组/本地脚本表单、手动部署等），应用域编辑改走 configStore + `saveApp()`；确认剩余功能（启动选项、更新、语言、路径、数据目录）无回归。
- [ ] C3 `pnpm check`（vue-tsc）通过；此阶段路由未动，设置页应已只剩应用域。

## Phase D：同步控制台组装与导航

- [ ] D1 `src/pages/sync/SyncConsolePage.vue`（布局 + tab 导航 + 嵌套 router-view + 内层 keep-alive）与 5 个 tab 组件：概览（迁 TaskStatusPage 内容）、任务、策略、交付、日志（迁 MainConsole 内容）。
- [ ] D2 `router/index.ts`：`/sync` 父子路由；`/`、`/tasks`、`/manual-copy` → redirect `/sync`；删除旧 console/tasks 路由记录。
- [ ] D3 `sidebarNavigation.ts`：常用组改为 同步控制台(`/sync`, prefix) + 历史；`Sidebar.vue` iconMap 增补。
- [ ] D4 `App.vue` keep-alive include：移除 `MainConsole`、加入 `SyncConsolePage`；更新 `App.keepAlive.test.mjs` 断言。
- [ ] D5 `messages.ts`：`sidebar.syncConsole`、`sync.tabs.*` 等 en/zh 双语；删除失效的 `sidebar.tasks`/`sidebar.console`（若无他处引用）。
- [ ] D6 删除旧 `TaskStatusPage.vue` / `MainConsole.vue`（内容已迁移；确认无残留 import）。

## Phase E：全量验证（trellis-check 最后一轮全范围）

- [ ] E1 `pnpm check`、`pnpm lint`、`node --test src/lib/*.test.mjs src/*.test.mjs`、`cargo test`。
- [ ] E2 `cmd /c pnpm tauri:build:versioned-exe` 构建通过（CLAUDE.md 硬性门禁）。
- [ ] E3 手工验证清单：
  - 启动落在 `/sync` 概览；调度启停、立即扫描、手动复制可用。
  - 任务 tab 增删改任务；策略 tab 改间隔/时段/后缀/关键字；交付 tab 服务器测试连接、命令组、本地脚本、手动部署。
  - 日志 tab 实时滚动、清空、tab 切换回来滚动位置保留。
  - 控制台改 interval 保存 → 运行中调度器下次触发用新间隔（观察"下次运行"卡）。
  - 控制台改同步字段、设置页改应用字段交替保存，互不覆盖（重启后 config.json 两域均为新值）。
  - 旧地址 `#/`、`#/tasks`、`#/manual-copy` 均落到 `/sync`；设置页无同步卡片。
  - 更新服务器地址修改仍触发更新检查重置（update_app_config 副作用）。

## 提交切分与回滚点

1. Phase A 单独提交（纯新增，随时可回滚）。
2. Phase B+C 一次提交（设置页瘦身 + 组件提取，路由未动，可独立验证）。
3. Phase D 一次提交（导航切换）。
   每步提交后如需回退，revert 对应提交即可，配置文件格式不受影响。

## 风险文件

- `src/App.vue`、`src/locales/messages.ts`：与 remote-package-patch 未提交改动重叠（见前置条件）。
- `SettingsPage.vue`：2061 行大改，提取时严禁复制两份编辑器逻辑（R2）。
- `main.rs`：新增 command 注册点多（invoke_handler 列表），漏注册会运行时报错而非编译错。
