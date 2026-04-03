# 本地后置脚本执行设计

## 背景

当前系统在复制远程版本包到本地后，仅支持通过 SSH/SFTP 部署到远程 Linux 服务器并执行远程命令。用户有需求在复制完成后执行本地 Windows 脚本（py/ps1/bat 等），用于本地构建、打包、通知等操作。该功能与远程部署相互独立，可单独执行也可组合执行，执行顺序可配置。

## 目标

1. 复制完成后支持执行本地 Windows 脚本，作为后置操作。
2. 本地脚本组采用与远程命令组对称的设计：命名组、可排序、可复用。
3. 本地脚本执行与远程部署不冲突，执行时机可配置（先本地、先远程、并行）。
4. 每个脚本组可独立配置失败策略（继续或中止）。
5. 支持自动检测脚本解释器，也支持用户直接写完整命令行。
6. 集成到 Phase A 任务状态机，本地脚本执行有独立的状态追踪。

## 前置依赖

本功能的状态机集成部分依赖 Phase A 任务状态后端重构（`task_domain.rs` / `task_manager.rs`）。后端执行引擎和配置模型不依赖 Phase A，可独立开发。

## 非目标

1. 本次不支持 Linux/macOS 本地脚本执行（仅 Windows）。
2. 本次不支持脚本执行超时配置（后续可扩展）。
3. 本次不支持脚本的交互式输入（stdin）。
4. 本次不修改远程部署的 `CommandGroup` 结构。

## 选型结论

采用"独立类型 + 新模块"方案：

1. `LocalCommandGroup` 作为独立类型，与远程 `CommandGroup` 平级但不混合。
2. 新建 `local_exec.rs` 模块处理脚本执行逻辑。
3. `scanner.rs` 负责编排本地脚本与远程部署的执行顺序。

不采用以下方案：

1. 统一 `CommandGroup` + 区分字段：远程 SSH 和本地 subprocess 语义差异大，合并会导致绑定模型混乱、现有代码到处加条件分支、需要迁移配置格式。
2. 执行逻辑内联到 `scanner.rs`：scanner.rs 已超 1500 行，继续膨胀不利于维护和测试。

## 数据模型

### LocalCommandGroup

本地脚本命令组，定义在 `AppConfig.local_command_groups` 中。

字段：

1. `id: String` — 唯一标识
2. `name: String` — 显示名称
3. `commands: Vec<String>` — 有序命令列表（脚本路径或完整命令行）
4. `on_failure: OnFailure` — 失败策略

### OnFailure

枚举值：

1. `continue` — 记录失败但继续执行后续组
2. `abort` — 中止后续本地组的执行

### LocalScriptBinding

每个 `ScanTask` 的本地脚本绑定配置。

字段：

1. `command_group_ids: Vec<String>` — 有序的 `LocalCommandGroup` ID 列表

### PostCopyExecutionOrder

每个 `ScanTask` 的后置操作执行时机。

枚举值：

1. `local_first` — 先执行本地脚本，再执行远程部署
2. `remote_first` — 先执行远程部署，再执行本地脚本
3. `parallel` — 并行执行

### AppConfig 变更

新增字段：

1. `local_command_groups: Vec<LocalCommandGroup>` — 与 `command_groups` 平级

### ScanTask 变更

新增字段：

1. `local_script_binding: Option<LocalScriptBinding>` — 本地脚本绑定（None 表示不执行）
2. `post_copy_execution_order: PostCopyExecutionOrder` — 默认 `local_first`

## 变量替换

本地脚本命令中支持以下变量：

1. `${folder_name}` — 复制的文件夹名（如 `Release_01`）
2. `${local_target}` — 本地复制目标完整路径（如 `E:\Builds\Release_01`）
3. `${source_path}` — 远程源路径
4. `${filename}` — 文件夹内第一个 `.tar.gz` 文件名（复用现有逻辑）

## 解释器自动检测

命令解析规则：

1. 如果命令以已知脚本扩展名的文件路径开头，自动添加解释器前缀：
   - `.py` → `python <path> <args>`
   - `.ps1` → `powershell -ExecutionPolicy Bypass -File <path> <args>`
   - `.bat` / `.cmd` → `cmd /c <path> <args>`
2. 如果命令是完整命令行（不以已知脚本扩展名结尾，或包含解释器前缀），直接通过 `cmd /c` 执行。
3. 变量替换在解释器检测之后执行。

## 执行流程

### Post-Copy 编排逻辑

复制完成后，`scanner.rs` 读取 live config 判断执行路径：

1. 检查 `local_script_binding` 是否存在且有绑定组。
2. 检查 `deploy_enabled` 且 `server_bindings` 非空。
3. 根据 `(has_local, has_remote, execution_order)` 分发：

| has_local | has_remote | 行为 |
|-----------|-----------|------|
| true | true | 按 `post_copy_execution_order` 执行 |
| true | false | 仅执行本地脚本 |
| false | true | 仅执行远程部署（现有行为） |
| false | false | 无后置操作 |

### 执行顺序详情

**local_first：**

1. 执行本地脚本组（按绑定顺序）
2. 如果某组 `on_failure: abort` 且该组失败，跳过剩余本地组并跳过远程部署
3. 如果所有本地组完成（无论 `continue` 组是否失败），执行远程部署

**remote_first：**

1. 执行远程部署
2. 执行本地脚本组（远程部署失败不阻断本地脚本）

**parallel：**

1. 同时启动本地脚本和远程部署
2. 分别等待完成
3. `on_failure: abort` 仅影响本地组内的后续组，不影响已启动的远程部署

### 子进程执行

每条命令通过 `std::process::Command` 执行：

1. 工作目录设为 `local_target`（复制目标路径）
2. 捕获 stdout 和 stderr，通过 `log-message` 事件发送到前端日志面板
3. 检查退出码，非零视为失败
4. 执行前检查 `should_cancel` 标志，支持用户取消

## 后端模块设计

### 新增模块：local_exec.rs

公开 API：

1. `execute_local_scripts()` — 主入口，接收绑定、命令组、路径上下文，返回 `LocalExecResult`
2. `resolve_command()` — 解析命令，自动检测解释器或直接传递
3. `run_single_command()` — 执行单条命令，捕获输出，发送日志

返回类型：

```
LocalExecResult {
    success: bool,
    group_results: Vec<GroupResult>,
    aborted: bool,
}

GroupResult {
    group_id: String,
    group_name: String,
    success: bool,
    command_results: Vec<CommandResult>,
}

CommandResult {
    command: String,
    exit_code: Option<i32>,
    stdout_excerpt: String,
    stderr_excerpt: String,
    elapsed_seconds: f64,
}
```

### 修改模块：scanner.rs

在 `perform_copy` 的复制完成后插入编排逻辑：

1. 从 live config 读取 `local_script_binding` 和 `post_copy_execution_order`
2. 根据组合条件分发到 `local_first` / `remote_first` / `parallel` 路径
3. 收集结果并上报给 TaskManager

### 修改模块：config.rs

1. 新增 `LocalCommandGroup`、`OnFailure`、`LocalScriptBinding`、`PostCopyExecutionOrder` 类型
2. `AppConfig` 新增 `local_command_groups` 字段（默认空数组）
3. `ScanTask` 新增 `local_script_binding`（默认 None）和 `post_copy_execution_order`（默认 `local_first`）
4. 旧配置兼容：缺失新字段时使用默认值，无需迁移

### 修改模块：task_domain.rs / task_manager.rs

任务状态机扩展：

1. `TaskSummaryStatus` 新增 `LocalExecuting` 值
2. `TaskRun` 新增 `local_exec_phase` 字段（类似 `copy_phase` / `deploy_phase`）
3. `TaskManager` 新增方法：`begin_local_exec()`、`mark_local_exec_completed()`、`mark_local_exec_failed()`
4. 状态归约规则更新：同时考虑 copy + local_exec + deploy 三个阶段的结果

## 前端改造

### Settings 页面

1. 新增"Local Script Groups"管理区域，位于现有"Command Groups"之后
2. 每个组：名称输入、命令列表（可拖拽排序）、`on_failure` 下拉选择、删除按钮
3. 支持新增组和新增命令

### Task Editor

1. 新增"Post-Copy Execution Order"按钮组（local_first / remote_first / parallel）
2. 新增"Local Script Groups"绑定列表，支持从已有组中选择并排序
3. 位于现有"Server Bindings"之后

### Task Detail

1. 展示三段式进度：Copy → Local Scripts → Remote Deploy
2. 本地脚本按组显示执行状态（pending / running / success / failed）
3. 失败时显示 stderr 摘要

### TypeScript 类型（tauri.ts）

新增接口：

1. `LocalCommandGroup { id, name, commands, on_failure }`
2. `LocalScriptBinding { command_group_ids }`
3. `PostCopyExecutionOrder` 类型

`AppConfig` 和 `ScanTask` 接口同步更新。

### 国际化（messages.ts）

所有新增 UI 文本需同时添加 `en` 和 `zh` 翻译。

## 实施顺序

### Phase 1：后端数据模型与执行引擎

1. `config.rs` 新增类型和字段
2. 新建 `local_exec.rs` 实现脚本执行
3. `scanner.rs` 接入编排逻辑
4. `task_domain.rs` / `task_manager.rs` 扩展状态机

### Phase 2：前端配置 UI

1. Settings 页新增 Local Script Groups 管理
2. Task Editor 新增绑定和执行顺序配置
3. TypeScript 类型同步

### Phase 3：前端状态展示

1. Task Detail 展示本地脚本执行进度
2. 集成日志面板显示脚本输出

## 配置兼容性

旧配置文件缺失新字段时的默认值：

1. `local_command_groups` → `[]`
2. `local_script_binding` → `None`（不执行本地脚本）
3. `post_copy_execution_order` → `local_first`

无需配置迁移，serde 默认值即可处理。

## 验收标准

1. 复制完成后能自动执行配置的本地脚本（py/ps1/bat），支持变量替换。
2. 执行顺序按 `post_copy_execution_order` 配置正确执行（local_first / remote_first / parallel）。
3. `on_failure: abort` 的组失败时正确中止后续本地组，且在 `local_first` 模式下阻断远程部署。
4. `on_failure: continue` 的组失败时记录日志但不影响后续执行。
5. 脚本执行状态在任务详情中有独立追踪，失败时显示错误信息。
6. 不绑定本地脚本组的任务行为与改动前完全一致（向后兼容）。
7. Settings 页可管理本地脚本组（增删改排序），Task Editor 可绑定和配置执行顺序。
