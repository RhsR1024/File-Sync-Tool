# 同步控制台重构 — 技术设计

## 1. 架构与边界

### 1.1 新信息架构（用户已确认：单入口合并）

侧边栏"常用"组由 3 项变 2 项：**同步控制台**（`/sync`，prefix 匹配）、**历史**（`/history`）。原"任务"、"控制台"两项移除；"系统 > 设置"保留但内容瘦身。

`/sync` 为父路由（布局组件 `SyncConsolePage.vue`，顶部横向 tab 导航 + 嵌套 `<router-view>`），5 个子路由：

| 子路由 | Tab | 内容来源 |
|--------|-----|----------|
| `/sync`（默认子路由） | 概览 | 现 `TaskStatusPage.vue` 全部内容（状态卡、调度启停、立即扫描、手动复制、任务组表格、详情面板） |
| `/sync/tasks` | 同步任务 | SettingsPage 扫描任务卡 + 任务编辑弹窗（L1016–L1300） |
| `/sync/strategy` | 扫描策略 | SettingsPage 本地存储卡（L993）、扫描时间卡（L1303，含 copy_buffer_size_kb）、文件后缀（L1415）、文件名关键字（L1440） |
| `/sync/delivery` | 交付流程 | SettingsPage 远程部署卡（L1468，含服务器管理/命令组/手动部署）+ 本地后置脚本卡（L1933） |
| `/sync/logs` | 运行日志 | 现 `MainConsole.vue` 日志终端 |

### 1.2 路由兼容

- `/` → redirect `/sync`
- `/tasks` → redirect `/sync`
- `/manual-copy` → redirect `/sync`（改为直指，不做链式跳转）
- 其余路由不动。

### 1.3 keep-alive 策略（App.vue L249）

- 顶层 include：移除 `MainConsole`，加入 `SyncConsolePage`（整个控制台跨路由保活，保留日志滚动位置与表单状态）。`SettingsPage` 继续保活。
- `SyncConsolePage` 内层 `<router-view>` 自带 `<keep-alive>` 包裹 5 个 tab 组件，tab 间切换不丢状态。
- 现有 `onActivated` 钩子（TaskStatusPage/MainConsole 的重载逻辑）随内容迁移进 tab 组件，行为不变。
- 注意：`src/App.keepAlive.test.mjs`（remote-package-patch 任务新增，未提交）断言 include 列表，需同步更新。

## 2. 组件提取（R2：编辑器仅一份实现）

从 `SettingsPage.vue` 提取到 `src/components/sync/`，全部改为绑定共享 configStore（不再接收本页局部 `config` ref）：

| 新组件 | 源位置（SettingsPage） |
|--------|------------------------|
| `SyncLocalStorageCard.vue` | L993–1014（local_path） |
| `SyncTasksCard.vue` + `SyncTaskEditModal.vue` | L1016–1300（任务列表、编辑弹窗、server_bindings、local_script_binding、执行顺序） |
| `SyncScheduleCard.vue` | L1303–1413（interval/time_ranges/stability/recent_guard/copy_buffer） |
| `SyncFileFiltersCard.vue` | L1415–1466（后缀 + 关键字两块合一卡或两卡） |
| `SyncRemoteDeployCard.vue`（内含服务器管理弹窗、服务器编辑弹窗、命令组弹窗、手动部署表单，可再拆子组件） | L1468–1930 |
| `SyncLocalScriptsCard.vue` + 脚本组编辑弹窗 | L1933–2050 |

对应脚本逻辑（内置命令组描述符、表单 refs、校验 computeds）随组件迁出；跨卡共用的放 `src/lib/` 或 composable。SettingsPage 删除同步域卡片与逻辑后只剩应用域内容（预计 <900 行）。

## 3. 数据流与契约

### 3.1 前端唯一配置源：`src/lib/configStore.ts`

```ts
export const configStore = reactive({
  config: AppConfig | null,
  isLoaded: boolean, isSaving: boolean,
  ensureLoaded(): Promise<void>,   // 首次加载
  refresh(): Promise<void>,        // 重新 getConfig()
  saveSync(): Promise<void>,       // buildSyncPatch(config) → updateSyncConfig → refresh → restartSchedulerInterval()
  saveApp(): Promise<void>,        // buildAppPatch(config)  → updateAppConfig  → refresh；max_log_lines>0 时同步 appStore.maxLogLines
});
```

- 补丁构造为纯函数放 `src/lib/configDomains.ts`：`buildSyncPatch(cfg): SyncConfigPatch`、`buildAppPatch(cfg): AppDomainConfigPatch` —— 可被 node --test 的 .mjs 测试直接覆盖。
- 同步控制台各卡与瘦身后的 SettingsPage 均使用 configStore；组件内不再各持 `config` ref（消除前端双缓存覆盖）。
- 字段级校验（interval≥5、stability≥60、guard≥3）保留为组件内 computed；后端 `validate_config` 仍是最终权威。
- toast 反馈沿用 `pushToast`，保存成功后照旧 `addSystemEvent('CONFIG_CHANGE', …)`。

### 3.2 域字段划分

**同步域（SyncConfigPatch，13 字段）**：`tasks`、`local_path`、`interval_minutes`、`time_ranges`、`file_extensions`、`filename_includes`、`deploy_enabled`、`servers`、`command_groups`、`local_command_groups`、`stability_check_secs`、`recent_file_guard_mins`、`copy_buffer_size_kb`。

**应用域（AppDomainConfigPatch）**：`launch_and_auto_scan`、`launch_and_auto_start_file_share`、`close_to_tray`、`max_log_lines`、`max_task_records`、`appliance_ssh_api_timeout_secs`、`framework_password_api_timeout_secs`、`disk_cleanup_http_timeout_secs`、`disk_cleanup_linux_mode`、`update_server_url`、`notify_on_new_version`、`clipboard`。

**后端专有（两个补丁都不含，UI 永不写入）**：`last_update_check_at`、`pending_update` —— 顺带修复现状"前端全量保存可能夹带过期 updater 字段"的隐患。

### 3.3 后端命令（Rust）

`config.rs` 新增两个 `Deserialize` 补丁结构 + 纯合并函数（便于单测）：

```rust
pub fn apply_sync_patch(config: &mut AppConfig, patch: SyncConfigPatch);
pub fn apply_app_patch(config: &mut AppConfig, patch: AppDomainConfigPatch);
```

`main.rs` 新增两个 command（模式对照现 `save_config_cmd`，`main.rs:1061`）：

```rust
#[tauri::command]
async fn update_sync_config(app_handle, state, patch: SyncConfigPatch) -> Result<(), String> {
    // lock → clone 当前 config → apply_sync_patch → validate → normalize
    // → 写回 state.config → config::save_config 落盘
}

#[tauri::command]
async fn update_app_config(app_handle, state, patch: AppDomainConfigPatch) -> Result<(), String> {
    // 同上合并流程，另保留 save_config_cmd 的应用域副作用：
    // update_server_url 变化 → last_update_check_at = None + updater::commands::handle_config_changed
    // launch_* 变化 → sync_launch_on_startup
}
```

- `save_config_cmd` 保留注册不动 —— FrameworkPasswordPage、FileSharePage、EnableApplianceSshPage、DiskCacheCleanupPage、PingScanTab 仍走全量保存（clipboard/工具字段），其覆盖风险为存量问题，本任务不扩散、不修复（见 PRD Out of Scope）。
- `tauri.ts` 新增 `SyncConfigPatch`/`AppDomainConfigPatch` 类型与 `updateSyncConfig`/`updateAppConfig` 封装（类型唯一真相来源仍是 tauri.ts ↔ config.rs 对应）。

### 3.4 调度联动（R4）

`configStore.saveSync()` 成功后调用 `restartSchedulerInterval()`（`scheduler.ts:93`，仅运行中生效、不触发即时扫描）——语义与现 SettingsPage `save()` 保持一致。

扫描运行中的配置热读行为（`scanner.rs:1761` 复制结束前重读 live_config 决定部署）**保持现状**，per-run 快照不在本任务范围。

## 4. i18n

- 迁移的卡片沿用现有 `settings.*` key（组件搬移 `t()` 调用即可，en/zh 无需重写）。
- 新增 key：`sidebar.syncConsole`、`sync.tabs.overview|tasks|strategy|delivery|logs`、页面标题等；`sidebar.section.*`、`sidebar.commonGroup` 计数文案核对。en/zh 同步补齐（开发规则）。

## 5. 兼容 / 迁移 / 回滚

- 配置文件格式零变化，无数据迁移；后端命令纯新增。
- 回滚 = revert 前端提交 + 后端提交；`save_config_cmd` 未动，任何中间状态下旧保存链路仍可用。
- 风险文件：`App.vue`、`src/locales/messages.ts`、`src/router/index.ts` 与 in_progress 的 remote-package-patch 任务未提交改动重叠 —— 实施前置条件见 implement.md。

## 6. 权衡记录

- **两个显式类型化补丁命令** vs 单个泛型 domain 参数命令：取前者，TS/Rust 边界类型安全、serde 校验直接。
- **概览为默认 tab**（而非日志）：用户选定单入口方案时已确认；日志缓冲在 `appStore.logs`，即便 tab 重挂载数据不丢。
- **工具页全量保存不改**：改动波及 5+ 页面，属既有风险且窗口极小，留作后续任务。

## 7. 测试点

- Rust 单测（config.rs 内 `#[cfg(test)]`）：`apply_sync_patch` 不触碰应用域/后端专有字段；`apply_app_patch` 反向同理。
- 前端 .mjs（node --test，仿 `remotePackagePatch.test.mjs` 纯逻辑模式）：`configDomains` 补丁构造完备性（字段清单 vs AppConfig key 差集恰好等于后端专有字段）；`sidebarNavigation` 结构；router redirect 表。
- `App.keepAlive.test.mjs` 更新 include 断言。
- 手工验证清单见 implement.md Phase E。
