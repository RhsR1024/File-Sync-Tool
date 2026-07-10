# 同步控制台重构

## Goal

把文件同步的全部配置能力（扫描任务、扫描策略、过滤规则、远程部署、本地后置脚本）从"设置"页迁入单入口的"同步控制台"，使运行控制与同步配置同处一处；"设置"页只保留应用级选项。采用用户选定的方案 2：迁移编辑能力、复用现有 AppConfig 配置模型、后端增加分域保存命令避免整体覆盖，不拆配置文件/后端服务（方案 3 排除）。

## Background（代码勘察确认的事实）

- 现有信息架构（`src/lib/sidebarNavigation.ts`）：常用组 = 任务 `/tasks`、控制台 `/`、历史 `/history`；`MainConsole.vue`（139 行）纯日志终端；`TaskStatusPage.vue`（527 行）已是运行控制中心（状态卡、调度启停、立即扫描、手动复制、任务组表格含逐 run 暂停/恢复/取消/重试、详情面板）。路由已有重定向先例 `/manual-copy → /tasks`。
- `SettingsPage.vue`（2061 行）同步域卡片：本地存储 `local_path`（L998）、扫描任务+编辑弹窗（L1023/L1089）、扫描时间（L1303，含 `copy_buffer_size_kb` L1370）、文件后缀（L1415）、文件名关键字（L1440）、远程部署+服务器/命令组/手动部署（L1468）、本地后置脚本（L1933）。应用域卡片：启动选项（含 `max_log_lines`/`max_task_records`）、更新、语言、配置路径、自定义数据目录。
- 保存链路：设置页所有子编辑器共用 `save()` 整体提交 → `save_config_cmd`（`src-tauri/src/main.rs:1061`）validate → normalize → **全量替换** AppState.config 并落盘；保存后前端调 `restartSchedulerInterval()`（`src/lib/scheduler.ts:93`）。双页各持 config 缓存会互相覆盖；后端 updater 写入的 `pending_update`/`last_update_check_at` 也可能被前端全量保存夹带过期值（存量隐患）。
- 调度为前端 `setInterval`（scheduler.ts）；扫描运行中 scanner 在复制结束前热读 live_config 决定部署（`src-tauri/src/scanner.rs:1761`）。
- 另有 5 处工具页/组件也做全量 `saveConfig`（FrameworkPasswordPage、FileSharePage、EnableApplianceSshPage、DiskCacheCleanupPage、PingScanTab），属存量风险。
- App.vue 顶层 keep-alive include 含 `MainConsole`、`SettingsPage`（App.vue L249）；`src/App.keepAlive.test.mjs` 为 remote-package-patch 任务未提交的新测试。

## Decisions（用户已确认）

- **导航形态（2026-07-10 确认）**：单入口合并——侧边栏"任务"+"控制台"合并为"同步控制台"（`/sync`），内部子路由五分区：概览（现任务页内容）、同步任务、扫描策略、交付流程、运行日志（现控制台终端）；`/`、`/tasks`、`/manual-copy` 重定向到 `/sync`。
- 设置页只保留应用级选项；历史记录保留独立入口；Tools 区（含远程产品包替换）不动。
- 分域保存粒度：同步域一个命令 `update_sync_config` + 应用域一个命令 `update_app_config`（技术细节见 design.md §3）。

## Requirements

- R1 同步控制台按五分区承载：概览/运行控制、同步任务、扫描策略（本地存储+时间+缓冲+过滤）、交付流程（远程部署+本地脚本+手动部署）、运行日志。
- R2 从 SettingsPage 提取同步域编辑器为可复用组件，编辑器只有一份实现，不允许两页各复制一份。
- R3 建立唯一前端配置源（共享 configStore），同步控制台与设置页共用；后端分域保存在 Rust 端读最新配置合并对应域字段后落盘，UI 保存不再写 `pending_update`/`last_update_check_at`。
- R4 保存扫描间隔等调度字段后仍触发 `restartSchedulerInterval()`；`update_app_config` 保留 `save_config_cmd` 的应用域副作用（开机启动注册表、更新服务器地址变更联动）。
- R5 旧路由 `/`、`/tasks`、`/manual-copy` 保持可达（重定向到 `/sync`），侧边栏与 keep-alive 列表同步更新。
- R6 所有新增/迁移文案在 `src/locales/messages.ts` en/zh 双语齐全；迁移卡片尽量沿用现有 `settings.*` key。

## Acceptance Criteria

- [ ] 同步控制台内可完成：任务增删改与启停、扫描间隔/时间段/稳定性/复制缓冲编辑、文件后缀与关键字过滤编辑、服务器与命令组管理、本地脚本组管理、手动部署、调度器启停与立即扫描、实时日志查看。
- [ ] 设置页不再出现同步域卡片，仅剩应用级选项（启动、更新、语言、路径、数据目录、日志/记录上限），原功能无回归。
- [ ] 控制台改同步字段、设置页改应用字段交替保存互不覆盖（重启后 config.json 两域均为新值）；修改更新服务器地址仍触发更新检查重置。
- [ ] 调度运行中修改 `interval_minutes` 保存后，下一次触发采用新间隔。
- [ ] 旧地址 `#/`、`#/tasks`、`#/manual-copy` 均落到同步控制台；日志 tab 切走再切回保留滚动位置（keep-alive）。
- [ ] `cargo test` 含分域合并单测通过；`pnpm check`、`node --test` 前端测试通过；`cmd /c pnpm tauri:build:versioned-exe` 构建通过。

## Out of Scope

- 拆分配置文件或后端服务（方案 3）。
- 为每次扫描运行固定交付配置快照（scanner.rs:1761 热读行为保持现状）。
- 修复 5 处工具页全量 `saveConfig` 的存量覆盖风险（留作后续任务）。
- Tools 区归属调整、历史记录页改造。

## Constraints

- 实施前置：等 remote-package-patch 任务未提交改动（重叠 `src/App.vue`、`src/locales/messages.ts`）落库，或在独立 worktree 实施（worktree 依 CLAUDE.md 共享依赖）。
- Rust 侧禁止对 main.rs 跑 rustfmt / cargo fmt（会递归 fmt 全 crate）；clippy 存量 deny error 不作门禁。
