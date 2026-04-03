# 任务状态机前端硬切与手动任务并轨设计

## 背景

Phase A 已经在后端建立了 `TaskGroup` / `TaskRun` / `DeployAttempt` 为核心的统一任务状态机，并提供了任务列表、详情和快照事件。

但当前前端任务页仍然主要依赖旧的 `src/lib/store.ts`：

1. 通过 `log-message`、`copy-progress`、`manual-copy-task-state` 推断任务阶段。
2. 通过 `folder`、`localPath` 等弱定位字段操作任务。
3. 将调度任务、手动复制、手动部署视为不同来源的 UI 记录，而不是统一的后端任务实体。

这导致后端状态机虽然存在，但还没有真正成为系统的唯一任务真相源。

## 目标

1. 将任务页列表、详情、状态推进、失败原因展示全部切换到后端状态机。
2. 将手动复制、手动部署纳入同一套 `TaskGroup` / `TaskRun` 模型。
3. 将前端任务操作入口全部切换为基于 `task_group_id` / `run_id` 的命令。
4. 删除前端基于日志和旧事件的任务状态推断逻辑，不保留兼容期。
5. 保留完整、清晰的日志展示能力，并支持后续复杂状态扩展。

## 非目标

1. 本次不重写底层复制或 SSH/SFTP 执行算法。
2. 本次不移除日志面板；日志继续保留，但不再驱动任务状态。
3. 本次不引入长期双轨兼容方案。
4. 本次不以“修补旧 `taskRecords` 体系”为过渡方案。

## 总体决策

### 决策 1：后端是唯一任务真相源

前端不再自行计算任务 phase。任务列表和详情的所有业务状态都只能来自后端快照。

允许前端做的只有：

1. 首次拉取列表与详情。
2. 订阅后端 snapshot 事件并更新本地展示缓存。
3. 在用户发起操作后展示“提交中”或“加载中”状态。

不允许前端做的包括：

1. 根据日志文本决定任务 phase。
2. 根据进度事件决定任务终态。
3. 用 `folder`、`localPath` 反推当前操作目标。

### 决策 2：日志与状态彻底解耦

`log-message`、`copy-progress`、`manual-copy-task-state` 不再承担任务状态驱动职责。

日志仍需要完整、清晰，因此建议新增结构化任务日志事件，供全局日志面板和详情页按任务过滤展示。即使后续为了排查暂时保留旧日志事件，前端也不得再用它们计算任务状态。

### 决策 3：手动任务并入统一模型

手动复制和手动部署不再是游离操作。

统一要求：

1. 手动复制创建或命中 `Manual` 类型的 `TaskGroup`，并创建对应 `TaskRun`。
2. 手动部署也必须落入某个 `TaskGroup` 历史。
3. 从任务详情发起的补部署必须显式指定 `task_group_id`。
4. 独立手动部署如无现有 group，可新建 `Manual` group。

### 决策 4：当前分支采用硬切

本分支最终形态是：

1. 任务页显示层完全切到后端状态机。
2. 任务页操作层完全切到后端状态机。
3. 旧 `taskRecords` 体系与相关 phase 推断逻辑删除。
4. 不保留前端兼容期。

## 目标架构

### 后端层

后端继续以 `TaskManager` 为唯一状态入口。

职责：

1. 管理 `TaskGroup`、`TaskRun`、`DeployAttempt` 生命周期。
2. 持久化 `task_state.json`。
3. 负责状态聚合、重启恢复和 snapshot 事件发射。
4. 提供查询命令、执行命令和运行控制命令。
5. 为日志提供结构化关联信息。

### 前端层

前端新增专用任务状态 store，替代现有 `taskRecords` 业务职责。

职责：

1. 保存 `TaskGroupListItem[]`。
2. 保存当前选中的 `TaskGroupDetail`。
3. 管理 `selectedTaskGroupId`。
4. 处理首屏 hydration。
5. 订阅后端 snapshot 和 task log 事件。
6. 暴露面向 `task_group_id` / `run_id` 的操作方法。

`TaskStatusPage` 只消费新 store，不再直接操作旧 `appStore.taskRecords`。

## 数据与命令契约

### 查询与订阅契约

保留并继续使用：

1. `list_task_groups() -> TaskGroupListItem[]`
2. `get_task_group_detail(task_group_id) -> TaskGroup`
3. `task-groups-snapshot`
4. `task-group-detail-snapshot`

规则：

1. 任务页加载时先调用 `list_task_groups`。
2. 用户选择某个任务时调用 `get_task_group_detail`。
3. 后续更新优先依赖 snapshot 事件，而不是重新从日志推断。

### 手动复制命令契约

新增：

`start_manual_copy_task(request) -> TaskRunHandle`

建议 request 字段：

1. `source_path`
2. `target_root_path`
3. `overwrite_existing`
4. `file_extensions`
5. `filename_includes`
6. `deploy_targets` 可选
7. `display_name` 可选

后端行为：

1. 校验请求。
2. 计算 merge key。
3. 创建或命中 `TaskGroup(source_type=manual)`。
4. 创建一个新的 `TaskRun`。
5. 返回 `task_group_id` / `run_id`。
6. 后续由 worker 消费该 run 并推进状态。

### 手动部署命令契约

新增：

`start_manual_deploy_task(request) -> TaskRunHandle`

建议 request 字段：

1. `task_group_id` 可选
2. `local_path`
3. `servers`
4. `post_command_group_ids` 可选
5. `display_name` 可选

规则：

1. 如果来自任务详情中的补部署，必须带 `task_group_id`。
2. 如果是独立手动部署，可不带 `task_group_id`，由后端创建新的 manual group。
3. 新 run 建议使用 `TaskRunType::ManualDeploy`。

### 运行控制命令契约

需要新增或重构为以下方向：

1. `cancel_task_run(task_group_id, run_id)`
2. `pause_task_run(task_group_id, run_id)`
3. `resume_task_run(task_group_id, run_id)`
4. `retry_task_group_deploy(task_group_id, server_ids, command_group_ids?)`
5. `clear_task_group(task_group_id)`
6. `clear_task_groups()`

统一规则：

1. 前端不再传 `folder`、`localPath` 作为任务操作主定位字段。
2. 所有任务操作都以 `task_group_id` / `run_id` 为主键。
3. 底层如果暂时仍有全局单活执行器限制，限制由后端校验并返回明确错误。

### 结构化任务日志契约

建议新增事件：

`task-log`

建议字段：

1. `task_group_id: string | null`
2. `run_id: string | null`
3. `server_id: string | null`
4. `server_name: string | null`
5. `level: info | success | warn | error | command`
6. `message: string`
7. `timestamp: string`

用途：

1. 全局日志面板继续展示所有日志。
2. 任务详情页可按 `task_group_id` / `run_id` 过滤。
3. 服务器明细视图可进一步按 `server_id` 过滤。

## 前端状态层设计

建议新增独立 task-state store，至少提供以下状态：

1. `groups`
2. `selectedTaskGroupId`
3. `selectedGroupDetail`
4. `isHydrated`
5. `isLoadingDetail`
6. `taskLogs`

建议提供以下动作：

1. `hydrateTaskState()`
2. `selectTaskGroup(task_group_id)`
3. `clearSelectedTaskGroup()`
4. `subscribeTaskStateEvents()`
5. `startManualCopyTask(...)`
6. `startManualDeployTask(...)`
7. `cancelTaskRun(...)`
8. `pauseTaskRun(...)`
9. `resumeTaskRun(...)`
10. `retryTaskGroupDeploy(...)`
11. `clearTaskGroup(...)`
12. `clearTaskGroups()`

明确删除的旧职责：

1. `markTaskRecord*`
2. 旧任务 phase 合成逻辑
3. 日志正则驱动状态流转
4. 基于 `folder` 的任务定位操作

## 分阶段实施

### Phase B：前端状态消费层重建

目标：

1. 新建 task-state store。
2. 任务页列表与详情完全基于 snapshot DTO。
3. 详情展示 runs、attempts、server rollups、失败原因和时间信息。
4. 日志面板与任务状态脱钩。

主要改动：

1. `TaskStatusPage.vue`
2. 新 task-state store
3. `src/lib/tauri.ts` 增加任务状态相关 invoke 包装
4. 详情面板 UI 改为基于 `TaskGroup` 模型渲染

完成标准：

1. 任务页不再依赖 `taskRecords` 渲染。
2. 仅靠 `list_task_groups` 和详情快照即可展示历史任务。

### Phase C：手动复制与手动部署并入状态机

目标：

1. 手动复制队列进入 `TaskManager`。
2. 手动部署进入 `TaskManager`。
3. 手动任务可以在任务页中与调度任务统一查看。

主要改动：

1. 新增 `start_manual_copy_task`
2. 新增 `start_manual_deploy_task`
3. 调整手动 worker，使其推进后端 run 状态
4. 为手动任务建立与日志的结构化关联

完成标准：

1. 前端不依赖 `manual-copy-task-state` 也能展示手动复制状态。
2. 手动部署结果不再是游离动作，而是出现在某个 `TaskGroup` 历史中。

### Phase D：操作层切换到 group/run

目标：

1. 所有任务操作都围绕 `task_group_id` / `run_id`。
2. 前端按钮不再操作旧 `taskRecords`。

主要改动：

1. 补部署按钮改走 `retry_task_group_deploy`
2. 清理按钮改走 `clear_task_group` / `clear_task_groups`
3. 取消、暂停、恢复切到新命令
4. 前端对“当前运行任务”的识别改为后端返回结果

完成标准：

1. 前端代码中不存在基于 `folder` / `localPath` 的任务主操作路径。
2. 所有按钮以 group/run 为定位依据。

### Phase E：删除旧逻辑

目标：

1. 删除旧 `taskRecords` 体系。
2. 删除前端任务状态推断器。
3. 保留日志展示，但去掉日志驱动状态职责。

主要改动：

1. 删除 `manual-copy-task-state` 的前端状态消费
2. 删除 `copy-progress` 对 phase 的影响
3. 删除 `log-message` 的任务状态正则处理
4. `save_ui_state` / `load_ui_state` 去掉 `taskRecords`

完成标准：

1. 前端代码中不存在“根据日志内容决定任务 phase”的逻辑。
2. 应用重启后，任务页只依赖后端持久化状态恢复。

## 错误处理规则

1. 前端不做任务状态乐观写入，只显示操作提交中。
2. `get_task_group_detail` 若返回不存在，前端自动清空当前详情。
3. 参数校验失败时不创建 run，直接返回命令错误。
4. 一旦 run 已创建，后续执行失败必须记录到 run / attempt，不允许只弹提示。
5. 旧日志事件丢失不得影响任务终态。
6. 对不支持的操作，后端必须返回明确错误而不是让前端猜测。

## 迁移规则

1. 本分支不保留双轨兼容。
2. 允许在开发中短时间同时存在新旧代码，但任务页只能逐步切到新 store，不允许继续扩展旧 `taskRecords` 体系。
3. 旧 UI 持久化中的 `taskRecords` 视为遗留数据，读取时忽略，不再继续保存。
4. 日志持久化可以继续保留。

## 验收标准

1. 打开任务页后，即使没有任何新日志到来，也能仅凭 `list_task_groups` 正确展示历史任务。
2. 选中任意任务后，详情页能展示 runs、attempts、server rollups、失败阶段和错误原因。
3. 调度任务执行时，前端不依赖日志文本匹配也能看到状态推进。
4. 手动复制执行时，前端不依赖 `manual-copy-task-state` 也能看到状态推进。
5. 手动部署执行时，会落入某个 `TaskGroup` 历史，而不是成为游离动作。
6. 取消、暂停、恢复、清理、补部署都基于 `task_group_id` / `run_id` 工作。
7. 应用重启后，未完成任务会从持久化 snapshot 恢复并显示为 `interrupted`。
8. 日志仍完整、清晰，且可以在任务详情中定位到对应 run / server。
9. 前端代码中不存在“根据日志内容决定任务 phase”的逻辑。

## 测试策略

### 后端

1. `TaskManager` 单元测试覆盖新命令与状态推进。
2. 手动复制进入 group/run 的场景测试。
3. 手动部署进入 group/run 的场景测试。
4. `retry_task_group_deploy` 归并与历史保留测试。
5. 重启恢复与中断标记测试。

### 前端

1. store 单元测试覆盖 hydration、快照订阅、详情切换和事件合并。
2. 任务页组件测试覆盖列表、详情、失败原因和服务器 attempts 展示。
3. 删除旧推断后，任务页在无日志文本辅助情况下仍能正确渲染。

### 联调

1. 调度复制成功并自动部署成功。
2. 调度复制成功，部分服务器部署失败，再进行补部署。
3. 手动复制成功，无部署目标。
4. 手动复制成功后执行手动部署。
5. 执行中关闭应用并重启，任务显示为 interrupted。

## 风险与约束

1. 当前系统可能仍有全局执行器和全局暂停/取消标记，Phase D 命令设计需要先包装为 group/run 语义，再逐步收敛到底层实现。
2. 手动部署是否一律创建新 group，还是允许挂到已有 group，必须由命令契约显式决定，不能依赖前端猜测。
3. 结构化任务日志如果不补，虽然不影响状态机切换，但会削弱后续复杂需求下的可观测性。

## 最终结论

本次改造不是“在现有前端任务页上接一个新接口”，而是将任务页的显示层、操作层和手动任务入口全部切换到后端统一状态机。

Phase A 提供的是后端基础设施；Phase B 到 Phase E 的职责是把这个基础设施真正变成系统的唯一任务状态来源，并彻底移除旧的前端日志驱动状态模型。
