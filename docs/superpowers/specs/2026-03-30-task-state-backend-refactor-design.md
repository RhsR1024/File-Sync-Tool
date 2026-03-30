# 任务状态后端统一状态机重构设计

## 背景

当前任务列表与路径信息详情主要依赖前端 `src/lib/store.ts` 基于日志文本、进度事件和本地推断来拼装状态。这种方式已经暴露出两个结构性问题：

1. 某些服务器在真正上传前失败时，不会生成可展示的服务器结果项，导致详情缺失失败服务器和失败原因。
2. 手动补部署会生成新的前端任务记录或覆盖原有展示结果，无法稳定地并入原自动复制任务并保留完整历史。

根因不是某个条件判断写错，而是“任务状态的真相源”放在了前端，且由日志文本驱动。随着后续功能继续增加，这种模式会越来越难维护、难排查、难扩展。

## 目标

1. 将复制、部署、补部署的状态流转统一收敛到 Rust 后端。
2. 让前端只消费后端的结构化任务快照，不再根据日志文本推断状态。
3. 手动补部署并入原自动复制任务，作为同一任务下的后续部署尝试展示。
4. 每台服务器无论在连接、上传还是执行命令阶段失败，都必须有可追踪的失败记录和失败原因。
5. 为后续扩展能力预留清晰边界，例如单台服务器重试、失败历史、恢复展示、导出诊断等。

## 非目标

1. 本次不重构扫描与部署底层执行算法本身。
2. 本次不移除日志面板；日志继续保留，但不再承担状态驱动职责。
3. 本次不要求设置页“手动部署”自动猜测归并到已有任务。真正并入原任务的入口应来自任务详情中的“补部署”操作。

## 选型结论

采用“后端统一任务状态机，前端只消费结构化快照”的方案。

不采用以下方案：

1. 继续增强前端状态拼装逻辑。
   原因：状态来源分散、日志格式脆弱、后续复杂需求会继续叠加判断。
2. 前后端混合维护状态。
   原因：状态权威边界不清晰，长期依然难定位问题。

## 核心原则

1. 后端是任务状态的唯一真相源。
2. 日志只用于展示，不用于前端业务状态判断。
3. 所有部署行为都以结构化尝试记录表示，而不是依赖是否出现上传进度。
4. 同一任务的归并规则稳定且显式，不依赖路径模糊匹配。
5. 历史记录与当前汇总同时保留，汇总基于最新有效尝试，历史基于完整尝试链。

## 任务归并规则

同一任务的稳定归并键为：

`taskConfigId + normalizedLocalTargetPath + folderName`

说明：

1. `taskConfigId` 代表来源任务配置，是最稳定的业务身份。
2. `normalizedLocalTargetPath` 用于区分不同本地目标目录。
3. `folderName` 用于区分同一任务下不同版本或不同包目录。

对于没有 `taskConfigId` 的独立手动部署，不自动并入已有任务。后续如需关联已有任务，必须显式传入 `task_group_id`。

## 后端模块划分

建议新增以下 Rust 模块：

1. `task_domain.rs`
   定义领域模型、状态枚举、ID 类型、快照结构。
2. `task_manager.rs`
   统一状态机入口，负责创建任务、推进状态、聚合服务器结果、持久化调度。
3. `task_events.rs`
   定义前端消费的结构化事件和快照 DTO。
4. `task_persist.rs`
   负责任务状态文件读写、恢复和迁移。
5. `task_commands.rs`
   暴露 Tauri 命令，例如列表查询、详情查询、补部署、清理记录。

现有模块职责调整：

1. `scanner.rs`
   只负责执行复制并上报结构化复制事件。
2. `deploy.rs`
   只负责执行部署并上报结构化部署事件。
3. `main.rs`
   负责注册新命令和初始化任务管理器。

## 数据模型

### TaskGroup

用户看到的一条任务主记录。

建议字段：

1. `task_group_id`
2. `merge_key`
3. `task_config_id`
4. `source_type`
5. `display_name`
6. `folder_name`
7. `source_path`
8. `local_target_path`
9. `copy_status`
10. `deploy_status`
11. `summary_status`
12. `started_at`
13. `finished_at`
14. `elapsed_seconds`
15. `latest_run_id`
16. `had_failures`
17. `server_rollups`
18. `runs`

### TaskRun

表示一次实际执行。

建议字段：

1. `run_id`
2. `task_group_id`
3. `run_type`
4. `trigger_source`
5. `started_at`
6. `finished_at`
7. `copy_phase`
8. `deploy_phase`
9. `attempt_ids`

`run_type` 建议值：

1. `copy_and_deploy`
2. `deploy_retry`

### DeployAttempt

表示某台服务器的一次部署尝试。

建议字段：

1. `attempt_id`
2. `task_group_id`
3. `run_id`
4. `server_id`
5. `server_name`
6. `attempt_no`
7. `trigger_source`
8. `stage`
9. `status`
10. `remote_target`
11. `started_at`
12. `finished_at`
13. `elapsed_seconds`
14. `progress_percentage`
15. `error_phase`
16. `error_message`
17. `last_log_excerpt`

`stage` 建议值：

1. `pending`
2. `connecting`
3. `uploading`
4. `executing_commands`
5. `done`

`status` 建议值：

1. `running`
2. `success`
3. `failed`
4. `cancelled`

### ServerRollup

用于前端列表和详情中的汇总展示。

建议字段：

1. `server_id`
2. `server_name`
3. `latest_status`
4. `latest_attempt_id`
5. `success_count`
6. `failure_count`
7. `last_error_message`
8. `attempt_ids`

## 状态流转

### TaskGroup 总状态

建议值：

1. `queued`
2. `copying`
3. `copy_completed`
4. `deploying`
5. `partial_failed`
6. `completed`
7. `failed`
8. `cancelled`
9. `interrupted`

### 自动复制与自动部署

1. 调度命中后，按归并键找到或创建 `TaskGroup`。
2. 创建新的 `TaskRun(copy_and_deploy)`。
3. 复制开始时，任务进入 `copying`。
4. 复制完成后：
   - 若无部署服务器，直接 `completed`
   - 若有部署服务器，先为每台服务器创建 `DeployAttempt(pending)`，再逐台推进
5. 对每台服务器的状态推进顺序固定为：
   `pending -> connecting -> uploading -> executing_commands -> success/failed`
6. 所有服务器 attempt 完成后，由 `task_manager` 统一归约：
   - 全部成功：`completed`
   - 部分成功部分失败：`partial_failed`
   - 全部失败：`failed`

关键约束：

1. 服务器在 `connecting` 阶段失败时，也必须已有对应 `DeployAttempt`。
2. 后端必须记录失败阶段与失败原因，不能只记录“部署失败”。

### 手动补部署

新增显式命令：

`retry_task_group_deploy(task_group_id, server_ids, command_group_ids?)`

流程：

1. 从任务详情发起补部署，前端明确传入 `task_group_id`。
2. 后端在同一个 `TaskGroup` 下创建 `TaskRun(deploy_retry)`。
3. 仅为指定服务器创建新的 `DeployAttempt`。
4. 历史 attempt 保留不动，当前汇总基于每台服务器最新一次有效 attempt 计算。

例子：

1. 自动部署：A 成功，B 失败。
2. 手动补部署：B 第二次成功。
3. 详情中保留：
   - A attempt #1 success
   - B attempt #1 failed
   - B attempt #2 success
4. 汇总状态按最新 attempt 计算，可恢复为 `completed`，同时 `had_failures = true`。

### 应用重启恢复

应用启动时，后端加载任务状态文件，将所有仍处于运行中的状态统一转为 `interrupted`，包括：

1. `copying`
2. `deploying`
3. `connecting`
4. `uploading`
5. `executing_commands`

同时保留：

1. 最后阶段
2. 最后错误信息
3. 已耗时
4. 已完成的服务器结果

## 结构化事件与命令接口

### 推荐命令

1. `list_task_groups()`
2. `get_task_group_detail(task_group_id)`
3. `retry_task_group_deploy(task_group_id, server_ids, command_group_ids?)`
4. `clear_task_group(task_group_id)`
5. `clear_task_groups()`

### 推荐事件

优先采用“整包快照”方案，降低前端复杂度。

1. `task-groups-snapshot`
2. `task-group-detail-snapshot`
3. `task-log-appended`

日志仍然单独发出，但不再驱动任务状态。

## 前端改造边界

### 保留

1. 列表排序、筛选、展开
2. 路径/详情展示
3. 操作按钮
4. 日志面板

### 下沉到后端

以下逻辑应逐步移出前端：

1. 基于日志解析任务状态
2. 基于路径和文件夹猜测任务归并
3. 服务器成功/失败的前端推断
4. 补部署预注册认领旧任务
5. 任务终态与汇总状态的前端归约

### UI 设计约束

1. 主列表始终按 `TaskGroup` 一行展示。
2. 路径信息详情升级为“任务详情”面板。
3. 详情中按服务器显示汇总卡片，并支持展开查看历史 attempts。
4. 任务详情中提供“补部署到失败服务器”的入口。
5. 设置页手动部署在第一阶段仍保留为独立工具，不自动并入任务。

## 迁移策略

### 后端状态文件

新增独立任务状态文件，例如：

`task_state.json`

其职责与现有前端 `ui_state.json` 分离：

1. `task_state.json` 负责任务领域状态
2. `ui_state.json` 仅负责前端视图级缓存

### 旧数据兼容

首次升级时，对旧前端 `task_records` 做一次尽力迁移：

1. 每条旧记录生成一个 `TaskGroup`
2. 已有 `remoteServers` 尽量转换为简化 `DeployAttempt`
3. 历史中缺失的失败原因不补造，只保留为空

迁移目标是“不丢旧记录”，而不是“还原全部历史细节”。

## 实施顺序

### Phase A：后端立模

1. 新建任务领域模型和状态机
2. 新建任务状态持久化
3. `scanner.rs` / `deploy.rs` 接入结构化状态上报

### Phase B：前端新详情面板

1. 新增任务详情数据接口
2. 详情面板改为展示服务器汇总与历史 attempts
3. 在详情中展示失败原因

### Phase C：补部署并入原任务

1. 新增 `retry_task_group_deploy`
2. 详情中增加“补部署到失败服务器”
3. 补部署结果写入同一 `TaskGroup`

### Phase D：主列表切换到新模型

1. 列表以 `TaskGroup` 快照为数据源
2. 替换旧前端任务推断逻辑

### Phase E：移除旧逻辑

1. 删除前端日志驱动状态解析
2. 删除旧任务拼装逻辑
3. 保留日志展示功能

## 测试策略

### 后端单元测试

1. 归并键稳定性
2. 自动部署与补部署归并是否正确
3. 连接阶段失败是否创建失败 attempt
4. 汇总状态归约是否正确

### 后端场景测试

1. A 成功、B 失败
2. B 手动补部署成功
3. 任务详情中保留完整历史
4. 汇总状态正确恢复

### 前端渲染测试

1. 主列表展示汇总状态
2. 详情展示服务器历史
3. 失败原因正确渲染
4. 补部署入口行为正确

## 风险与控制

1. 风险：一次性切换过大。
   控制：后端先立模，前端分阶段切换。
2. 风险：旧数据兼容不完整。
   控制：采用尽力迁移，不强求虚构历史细节。
3. 风险：前后端短期双轨并存。
   控制：明确新旧边界，优先让详情页切到新模型，再切列表。

## 验收标准

1. 自动部署中任意服务器在任意阶段失败，都能在详情中看到该服务器条目和失败原因。
2. 手动补部署能并入原自动复制任务，不覆盖已有服务器结果。
3. 同一任务下可同时看到自动部署与补部署的完整服务器尝试历史。
4. 前端在任务展示上不再依赖日志文本推断状态。
5. 应用重启后，任务历史、服务器结果和阶段中断信息可稳定恢复。
