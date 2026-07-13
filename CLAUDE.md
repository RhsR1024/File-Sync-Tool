# CLAUDE.md

本文件是 Claude 在 File Sync Tool 仓库中的项目级开发指南。它应与 `AGENTS.md` 的项目事实保持一致，同时可以独立阅读。

## 项目概览

File Sync Tool 是基于 Tauri 2、Vue 3、TypeScript、Tailwind CSS 4 和 Rust 的 Windows 桌面工具箱。应用以文件同步和远程交付为核心，同时提供文件共享、屏幕共享、剪贴板、网络诊断、磁盘缓存清理、错误码查询、代码统计和远程产品包替换等工具。

## 开发命令

```powershell
pnpm dev                              # Vue 前端开发服务器
pnpm tauri dev                        # 完整桌面开发模式
pnpm check                            # Vue/TypeScript 类型检查
pnpm lint                             # ESLint
pnpm build                            # 主前端和文件共享 Web 生产构建
pnpm test:share-web                   # 文件共享 Web 测试
cmd /c pnpm tauri:build:versioned-exe # 版本化裸 EXE 和发布 manifest
```

Tauri 配置当前为 `bundle.active: false`。`pnpm tauri build` 生成裸 EXE；版本化脚本将其重命名为 `file-sync-tool-<version>-<时间戳>.exe`，并更新 `scripts/release-server/manifest.json`。

## 技术与代码风格

- Vue 组件使用 `<script setup>`、Composition API 和 TypeScript。
- 使用 Tailwind CSS 4 与现有设计系统；界面图标使用 `lucide-vue-next`，不要使用 Emoji 代替产品图标。
- 面向用户的文本必须通过 Vue I18n，并同时维护 `src/locales/messages.ts` 中英文内容。
- 前端 Tauri 类型和调用封装集中在 `src/lib/tauri.ts`，Rust 结构和 command 实现位于 `src-tauri/src/`。
- Rust 异步任务使用 Tokio；阻塞 I/O、SSH 和文件操作需放入合适的阻塞执行上下文。
- 跨层字段变更必须同步检查前端类型、Rust 结构、默认值、迁移、序列化和持久化。

## 代码结构

### 前端

```text
src/
├─ main.ts、App.vue          应用入口、根布局和全局事件
├─ router/index.ts           路由配置
├─ pages/sync/               同步控制台：概览、任务与策略、交付
├─ pages/                    日志、历史、设置及工具页面
├─ components/               共享组件和业务组件
├─ composables/              可复用组合式逻辑
├─ lib/tauri.ts              Tauri 调用和跨层类型
├─ lib/configStore.ts        配置加载与域级保存
├─ lib/taskStateStore.ts     任务组、运行、日志和进度状态
├─ locales/messages.ts       中英文文案
└─ share-web/                局域网文件共享 Web 前端
```

### Rust/Tauri

```text
src-tauri/src/
├─ main.rs、lib.rs           应用入口、初始化、状态和 command 注册
├─ config.rs                 配置、默认值、迁移和域补丁
├─ scanner.rs                扫描、复制和文件稳定性检查
├─ deploy.rs、local_exec.rs  SSH/SFTP 交付和本地脚本
├─ task_*.rs                 任务领域、运行时、持久化和事件
├─ screenshare.rs            屏幕共享会话与采集
├─ disk_cleanup.rs           磁盘缓存清理
├─ network.rs                网络工具
└─ code_count.rs             代码统计
```

入口文档只描述稳定边界；完整路由、command 和字段清单以源码为准。

## 同步配置契约

配置使用域级补丁保存，避免不同页面互相覆盖：

- `configStore.saveSync()` 保存任务、扫描策略、服务器、命令组和复制相关字段。
- `configStore.saveApp()` 保存通用设置和工具配置。
- Rust 的 `SyncConfigPatch` 和 `AppDomainConfigPatch` 必须保持字段互斥，并共同覆盖全部可写配置。
- 同步配置保存后会刷新配置并重启调度器。不要重新引入前端整对象 `saveConfig()` 流程。

## 修改流程

1. 先读取 `.trellis/workflow.md` 和相关 `.trellis/spec/` 规范。
2. 仓库存在 `.codegraph/` 时，定位或理解代码优先使用 CodeGraph；文本搜索优先使用 `rg`。
3. 检查 Git 状态并保护用户已有修改，不回退无关文件。
4. 只修改任务范围内的文件，避免无关重构和格式化。
5. 运行与改动相称的测试，并执行适用的 `pnpm check`、`pnpm lint`、`git diff --check`。
6. 仅在用户要求发布或生产构建验证时运行耗时较长的 Tauri release 构建。
7. 未经明确授权，不自动提交、推送或发布。

## Windows 与 PowerShell

- 默认命令环境为 PowerShell，不使用 Bash 专属命令连接方式。
- 需要执行 package script 的批处理语义时使用 `cmd /c`，例如版本化 EXE 构建。
- GitHub SSH 异常时优先检查仓库本地 `core.sshCommand` 和 Windows OpenSSH，而不是修改全局配置。
- 路径可能包含空格或反斜杠，脚本中使用安全的参数传递和字面路径。

## Worktree 共享依赖

- worktree 优先复用主工作区的 `node_modules` 和 Rust target，避免重复占用大量磁盘。
- Windows 上可使用 junction 或 symlink 将 worktree 的 `node_modules` 指向主工作区。
- Rust/Tauri 命令通过 `CARGO_TARGET_DIR` 指向共享构建目录。
- 共享路径异常时先验证 `vite`、`vue-tsc` 和 Cargo 解析，不要提交临时依赖副本。
