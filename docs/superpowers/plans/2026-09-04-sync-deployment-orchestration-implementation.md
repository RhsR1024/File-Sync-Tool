# 同步、部署编排与控制台重构实施计划

对应 Spec：docs/superpowers/specs/2026-09-04-sync-deployment-orchestration-design.md  
原型：docs/design/sync-console-task-list-prototype.html

实施原则：后端状态机是真相源；先兼容扩展数据模型，再接执行链，最后替换 UI 和做性能收敛。

## 全局约束

- [x] 保留现有未提交改动，尤其是 main.rs 和 updater；重叠文件只做最小补丁。
- [x] 所有用户文案同步中英文 i18n。
- [x] 配置同步 Rust、TypeScript、默认值、normalize、patch 和测试。
- [x] 密码、Base64 密码和 token 不写日志或任务状态。
- [x] 不使用浏览器原生对话框。
- [x] 阻塞 SSH/文件操作放入阻塞线程；等待可取消且不占 Tokio 异步执行线程。
- [x] 分阶段运行针对性测试；Rust、直接前端类型检查、全仓 ESLint、相关前端测试、生产构建和 git diff --check 已通过。`pnpm` 启动器受本机共享缓存 EPERM 影响，Phase 8 使用同一项目内二进制直接执行等价检查。

## Phase 1：配置与领域模型

### 1.1 复合任务配置

文件：config.rs、tauri.ts、configDomains.ts、SyncConfigurationEditor.vue、messages.ts

- [x] 新增 ScanTaskModule 和稳定 module id。
- [x] ScanTask 增加 modules；旧字段兼容并归一化为单模块。
- [x] 新任务默认 DateMatch("%y%m%d")。
- [x] 标签改为主线（版本号匹配）和组件化（日期目录匹配）。
- [x] 增加多行路径解析、去重、预览和复合任务保存。
- [x] 组件化复合任务禁止旧日期 fallback。
- [x] 增加配置 round-trip 和 patch 测试。

### 1.2 安装后动作配置

- [x] 新增 SSH、拓扑、改密、30 秒/10 分钟配置和默认值。
- [x] 新增主机、备机、虚拟 IP、多从机结构。
- [x] 三组件新密码分离；UMS username 默认 loadmin。
- [x] 保存域使用 Windows DPAPI 保护安装后密码。

### 1.3 任务状态扩展

文件：task_domain.rs、task_manager.rs、task_events.rs、task_persist.rs、tauri.ts

- [x] 增加产品、版本、架构、源目录修改时间、build id、module id/name。
- [x] 增加父任务和模块子任务关联及批次汇总。
- [x] 增加 waiting_reboot、enabling_ssh、configuring_topology、verifying_topology、changing_passwords、unconfirmed 阶段。
- [x] 为旧 task_state 提供 serde 默认迁移。
- [x] 增加领域聚合、序列化和迁移测试。

## Phase 2：底层能力统一

### 2.1 UMS 初始密码

文件：ums_init_password.rs、UmsInitPasswordPage.vue、tauri.ts、messages.ts

- [x] 请求增加 UMS username。
- [x] 三个流程使用独立新密码。
- [x] 默认框架/CDM admin_123、UMS admin_1234。
- [x] UMS userCode/userName 使用输入用户名。
- [x] 保留独立旧密码、勾选、结果和错误。
- [x] 增加验证和前端测试。

### 2.2 SSH 连接解析

文件：新增 ssh_connection.rs，修改 deploy.rs 和最小模块注册。

- [x] 抽取 TCP/SSH 握手认证 helper。
- [x] 实现 23333/admin_123→开启 SSH→23333/admin_123→22/123456。
- [x] 手动部署、自动部署、连接测试共用解析器。
- [x] 返回有效连接和聚合诊断，不覆盖配置。
- [x] 增加决策和可注入探针测试。

## Phase 3：主备从与部署后处理

### 3.1 主备从后端

文件：新增 deployment_topology.rs、deployment_commands.rs，修改 ums_init_password.rs 和最小命令注册。

- [x] 实现 Base64 请求。
- [x] 实现主备及多从机顺序切换。
- [x] 区分请求前失败和已发送后的主动断连。
- [x] 实现框架 token 获取且不记录。
- [x] 实现 21900 列表验证和 9820 HA 验证。
- [x] 实现 30 秒、最多 20 次轮询。
- [x] 超时返回 unconfirmed，不自动重放。
- [x] 实现独立搭建、状态查询、取消和重试；未确认任务重试只查询状态，不重放切换请求。
- [x] 使用 mock HTTP 测试正常响应、请求后主动断连、明确 HTTP 失败、HA 状态和角色验证。

### 3.2 安装后动作接入

文件：deploy.rs、scanner.rs、task_manager.rs、手动部署 command

- [x] 安装命令后在持久化部署 attempt 中进入后处理阶段。
- [x] 等待首次重启恢复。
- [x] 顺序执行 SSH→拓扑→改密。
- [x] 安装成功但后处理失败时允许只重试后处理，不重新上传和安装。
- [x] 应用退出/重启将未完成 attempt 持久化为 interrupted；可恢复检查点支持从重启等待、SSH、拓扑验证或改密阶段继续。

### 3.3 部署配置 UI

文件：SyncDeliveryPage.vue、SyncConfigurationEditor.vue 或新组件、tauri.ts、messages.ts

- [x] 实现安装后操作表单和风险提示。
- [x] 实现主备、主从、主备从和多个从机。
- [x] 实现独立搭建、进度和应用内错误。
- [x] 确认弹框符合应用内模态规范。

## Phase 4：复合任务执行

文件：scanner.rs、task_manager.rs、task_domain.rs、task_events.rs

- [x] 一次运行先扫描全部模块并生成计划。
- [x] 组件化复合任务只扫当天，缺包持久化记录 no_output。
- [x] 串行执行有产出模块，失败隔离。
- [x] 父任务聚合 found/expected、current/found、字节加权进度。
- [x] merge key 加入 module id/source，避免不同产品同名构建目录错误合并。
- [x] 支持只重试失败模块，并使用模块级服务器覆盖配置。
- [x] 增加缺失、部分失败、取消和旧状态迁移测试。

## Phase 5：历史和本地包清理

文件：新增 sync_retention.rs，修改 task_manager.rs、task_commands.rs、SyncOverviewPage.vue、tauri.ts、messages.ts

- [x] 新增默认 5 天 retention。
- [x] 预览候选包、记录、大小和跳过原因。
- [x] canonicalize 并拒绝根目录、越界和链接逃逸。
- [x] 跳过活跃、待处理和可重试任务引用。
- [x] 每模块保留最近一个成功包。
- [x] 清理记录并真正执行 max_task_records。
- [x] 启动后每日最多自动执行一次。
- [x] 实现应用内确认、忙碌、错误和重试。
- [x] 增加安全和保留策略测试。

## Phase 6：同步控制台正式界面

文件：SyncOverviewPage.vue、TaskGroupsTable.vue、TaskGroupDetailPanel.vue、新增子表/流水线组件、taskStatusView.ts、taskStateStore.ts、messages.ts

- [x] 主表使用产品、版本、架构、源目录修改时间和 build id。
- [x] 父任务默认折叠，展开显示模块。
- [x] 展示今日产出、正在处理和当前模块字节进度。
- [x] no_output 使用中性状态。
- [x] 展示完整部署流水线。
- [x] 增加搜索、状态、类型和异常筛选。
- [x] 详情展示模块、路径、服务器角色、阶段检查点、历史、错误和日志。
- [x] 保留旧任务操作。
- [x] 实现键盘导航、焦点管理和非颜色表达。

## Phase 7：性能收敛

- [x] 列表分页或窗口化。
- [x] 后端列表快照 75 ms 合并节流；日志只更新详情和按组日志，不发列表全量快照。
- [x] 前端摘要采用 shallowReactive 和不可变替换，并以 revision 丢弃过期快照。
- [x] 仅当前分页中的活动行响应秒级 ticker。
- [x] 日志按 task group 索引并按需加载。
- [x] 审查 keep-alive，仅保留任务/交付配置页；高频控制台概览由全局浅状态恢复。
- [x] 增加 100/1000 条 mock 数据基线；本机列表生成分别为 0 ms/3 ms（构建测试数据不计入渲染时间）。

## Phase 8：验证

- [x] 前端类型检查：项目内 `vue-tsc -b` 通过；`pnpm check` 包装命令仅因本机 `D:\DevTools\Caches\pnpm-store` 无创建权限无法启动。
- [x] ESLint：项目内 `eslint . --ext .ts,.vue` 全仓通过；`pnpm lint` 包装命令同受上述 pnpm 缓存权限影响。
- [x] 相关前端测试（27/27）
- [x] Rust 单元和集成测试（687 通过、0 失败、3 个需现场硬件的测试忽略）
- [x] cargo fmt --check
- [x] git diff --check
- [x] 完成无现场设备依赖的人工/模拟走查：三套前端生产构建、复合任务界面与状态聚合、清理安全边界、SSH 决策探针、主备从 mock HTTP 全链路。真实一体机断电重启和共享目录产出仍需按下方现场清单验收。
- [x] 记录现场联调假设，尤其是纯主从主机 haType=1。

## 实施结果与现场联调清单（2026-09-08）

代码目标已全部完成。现场环境上线前按以下项目抽样，不需要再改动实现即可执行：

1. 手动和自动部署各验证一次 `root@IP:23333/admin_123` 直连、开启 SSH 后重试、最终 `root@IP:22/123456` 降级以及四阶段聚合报错。
2. 用真实版本安装验证首次重启耗时，确认 30 秒一次、最多 20 次的等待覆盖现场 3–5 分钟窗口；关闭应用再打开后，从控制台重试应只恢复安装后阶段。
3. 分别搭建单主备、单主从和主备从；确认切换接口主动断连后不重发，并以服务器列表和 HA 状态完成最终判定。
4. 纯主从主机暂按 `haType=1`、从机按 `haType=4`；若现场返回不同，以抓包结果调整角色映射，不改变任务模型。
5. 框架登录固定使用 `admin`，搭建密码为当前框架密码；主备 hostname 默认主机 IP 去点，备机名固定 `HA`，从机 replicaName 默认从机 IP 去点。
6. 用 10 个组件路径制造“5 个有产出、5 个无产出”样例，确认父任务显示 5/10、处理 n/5、无产出为中性状态，失败模块可单独重试。
7. 在测试同步根目录预览并清理 5 天前内容，确认活动/可恢复任务引用和每模块最近成功包被保留，远端 UNC 源不受影响。

验证证据：主应用、文件共享 Web、屏幕共享 Web 生产构建均通过；全仓 ESLint 与 Vue 类型检查通过；Rust 687/687 可运行测试通过；相关前端静态回归 27/27 通过；`git diff --check` 通过。
