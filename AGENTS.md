<!-- CODEGRAPH_START -->
## CodeGraph

当仓库根目录存在 `.codegraph/` 时，在定位或理解代码前优先使用 CodeGraph：

- 首选 MCP 能力：`codegraph_explore`。
- Shell 备用命令：`codegraph explore "<符号名或问题>"`。

如果不存在 `.codegraph/`，则跳过 CodeGraph；是否建立索引由用户决定。
<!-- CODEGRAPH_END -->

# File Sync Tool 项目指南

本文件是 Codex 的仓库级指令入口。Codex 会自动读取作用域内的 `AGENTS.md`。`CLAUDE.md` 面向 Claude；两份文档应保持项目事实一致，但不要互相覆盖或假设另一份一定已加载。

## 项目概览

File Sync Tool 是 Windows 桌面工具箱，采用 Tauri 2、Vue 3、TypeScript、Tailwind CSS 4 和 Rust。核心能力包括：

- 扫描远程共享目录，将匹配的版本或日期目录增量复制到本地。
- 在复制完成后通过 SSH/SFTP 向 Linux 服务器交付文件，并执行远程命令或本地脚本。
- 提供运行日志、历史记录、手动复制、文件共享、屏幕共享、剪贴板管理、网络工具、磁盘缓存清理、错误码查询和产品包替换等工具。

## 常用命令

```powershell
# 仅启动 Vue 前端
pnpm dev

# 启动完整 Tauri 开发环境
pnpm tauri dev

# TypeScript/Vue 类型检查
pnpm check

# ESLint
pnpm lint

# 主前端和文件共享 Web 前端生产构建
pnpm build

# 文件共享 Web 测试
pnpm test:share-web

# 构建并生成带版本和时间戳的裸 EXE，同时更新发布 manifest
cmd /c pnpm tauri:build:versioned-exe
```

`src-tauri/tauri.conf.json` 当前设置 `bundle.active: false`，因此 Tauri 构建交付的是裸可执行文件，不是安装器。版本化命令会运行生产构建，再通过 `scripts/rename-tauri-exe.mjs` 生成 `file-sync-tool-<version>-<时间戳>.exe`，最后更新 `scripts/release-server/manifest.json`。

## 技术规范

- 前端使用 Vue 3 `<script setup>`、Composition API 和 TypeScript。
- 样式使用 Tailwind CSS 4，并延续现有界面视觉语言。
- 产品图标使用 `lucide-vue-next`，不要用 Emoji 代替界面图标。
- 所有面向用户的文案通过 Vue I18n 管理，并同步维护 `src/locales/messages.ts` 的中文和英文内容。
- 前端调用 Tauri command 的类型和封装集中在 `src/lib/tauri.ts`；Rust 数据结构以 `src-tauri/src/` 中的定义为准。
- Rust 异步逻辑基于 Tokio。阻塞文件、网络或 SSH 操作不得占用异步执行线程。
- 修改配置字段或跨层数据结构前，必须检查 TypeScript 类型、Rust 结构、默认值、迁移和持久化逻辑是否需要同步。

## 用户提示与确认弹框

- 所有面向用户的提示、警告、错误和确认弹框，严禁使用 `window.alert`、`window.confirm`、`window.prompt` 或其他会显示“tauri.localhost 显示”的 WebView/浏览器内置对话框。
- 必须使用应用内 Vue 组件实现，并延续屏幕共享远控申请弹框的交互：需要用户处理时先恢复主窗口、取消最小化并置于其他窗口前方，再在工具界面中央显示模态弹框。
- 模态弹框必须支持键盘操作、焦点约束、明确的安全默认焦点、忙碌状态和应用内错误反馈；不得在后台静默弹出，也不得让确认按钮只关闭提示而不执行其承诺的操作。
- 托盘或后台事件触发的确认必须提供完整闭环：用户确认后自动执行所需操作并进入最终状态；失败时保留同一弹框、显示可理解的错误并提供重试或取消入口。

## 当前架构

### 前端

- `src/main.ts`、`src/App.vue`：应用入口、全局监听和根布局。
- `src/router/index.ts`：路由真相来源。
- `src/pages/sync/`：同步控制台的概览、任务与策略、交付页面。
- `src/pages/`：日志、历史、设置及各类工具页面。
- `src/components/`：共享组件和业务组件。
- `src/lib/tauri.ts`：Tauri command 封装及跨层类型。
- `src/lib/configStore.ts`：配置加载和按域保存。
- `src/lib/taskStateStore.ts`：任务组、运行状态、日志和进度状态。
- `src/share-web/`：独立的局域网文件共享 Web 前端。

### 后端

- `src-tauri/src/main.rs`：桌面应用入口。
- `src-tauri/src/lib.rs`：Tauri 初始化、状态和 command 注册。
- `src-tauri/src/config.rs`：配置结构、默认值、迁移和配置域补丁。
- `src-tauri/src/scanner.rs`：扫描、复制和文件稳定性判断。
- `src-tauri/src/deploy.rs`、`local_exec.rs`：远程交付和本地脚本执行。
- `src-tauri/src/task_*.rs`：任务领域、运行时、持久化、事件和 command。
- `src-tauri/src/screenshare.rs`：屏幕共享会话和采集后端。
- `src-tauri/src/disk_cleanup.rs`、`network.rs`、`code_count.rs`：工具能力。

完整模块和接口以当前源码为准，不在入口文档中维护易过时的全量清单。

## 配置保存边界

配置分为同步域和应用域：

- 同步任务、扫描策略、服务器、命令组和复制参数通过 `configStore.saveSync()` 保存。
- 通用设置和工具配置通过 `configStore.saveApp()` 保存。
- 修改保存逻辑时，必须保持 Rust `SyncConfigPatch`、`AppDomainConfigPatch` 与前端补丁字段互斥且覆盖所有可写字段。
- 保存同步配置后需要刷新配置并重启调度器；不要恢复旧的前端整对象 `saveConfig()` 写入流程。

## 开发与验证规则

- 修改前先确认当前 Git 状态，保留用户已有改动；不得擅自回退或覆盖无关文件。
- 搜索文件或文本优先使用 `rg`；存在 `.codegraph/` 时先使用 CodeGraph 理解调用关系。
- 文件修改优先使用补丁方式，避免无关格式化或大范围机械重写。
- PowerShell 中不要假设 Bash 的 `&&` 等语法可用；需要调用批处理语义时显式使用 `cmd /c`。
- 根据改动范围运行针对性测试，并至少执行 `pnpm check`、`pnpm lint` 和 `git diff --check` 中适用的检查。
- 只有用户要求发布或改动可能影响生产构建时，才执行耗时较长的 Tauri release 构建。
- 不得因“修改完成”自动推断用户授权提交、推送、发布或覆盖外部状态。

## 诊断环境边界

- 用户提供的问题、报错、日志和命令输出，默认来自其他机器或现场环境。除非用户明确要求或说明在本机诊断，不得直接使用当前工作区所在机器的进程、端口、网络、文件系统或运行时状态推断用户现场。
- 当问题发生在本机还是其他机器并不明确，且环境归属会影响诊断结论时，必须先向用户确认；可同时继续不依赖现场位置的静态代码分析。

## Worktree 共享依赖

- 在本仓库的 Git worktree 中，优先复用主工作区依赖和构建产物，避免复制大型目录。
- 前端依赖可将 worktree 的 `node_modules` 指向仓库根目录的 `node_modules`；Windows 上优先使用 junction 或 symlink。
- Rust/Tauri 构建通过 `CARGO_TARGET_DIR` 复用共享 target 目录，优先指向主工作区的 `src-tauri/target` 或已配置的共享目录。
- 共享路径丢失或重建后，先验证 `vite`、`vue-tsc` 和 Cargo 产物解析，再判断环境是否损坏。
- 不要提交仅用于 worktree 隔离的临时依赖副本。
