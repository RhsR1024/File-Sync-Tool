# 项目入口文档整理实施计划

> **执行要求：** 当前会话采用内联执行，按步骤修改并在最后统一检查。

**目标：** 将三个仓库入口文档整理为准确、清晰、无占位信息的中文文档。

**结构：** `AGENTS.md` 与 `CLAUDE.md` 分别服务对应开发代理并共享稳定项目事实；`README.md` 服务使用者和贡献者。内容以当前路由、目录、`package.json` 和 Tauri 配置为准。

**技术范围：** Markdown、Vue 3、Tauri 2、Rust、pnpm、PowerShell。

## 全局约束

- 保留 `AGENTS.md` 中 CodeGraph 与 Trellis 托管块的标记和语义。
- 不虚构 GitHub URL、许可证、安装器或发布渠道。
- 不修改应用代码、配置、版本号或构建行为。
- 文件使用 UTF-8；技术标识符、路径和命令保持原样。

---

### 任务 1：整理 Codex 入口规范

**文件：**

- 修改：`AGENTS.md`

- [x] 将人工维护的英文项目说明翻译并重构为中文。
- [x] 保留 CodeGraph/Trellis 托管块。
- [x] 补齐当前架构、配置域保存、验证命令、PowerShell 和 worktree 规则。
- [x] 删除可能随代码快速漂移的完整内部 API 清单。

### 任务 2：更新 Claude 入口规范

**文件：**

- 修改：`CLAUDE.md`

- [x] 删除旧页面、旧模块、旧事件流和旧配置结构说明。
- [x] 与 `AGENTS.md` 对齐稳定技术事实和开发约束。
- [x] 保留 Claude 独立读取时所需的完整上下文，不要求先读取另一份代理文档。

### 任务 3：重写项目 README

**文件：**

- 修改：`README.md`

- [x] 将产品名称改为 File Sync Tool，并概述当前 Windows 工具箱定位。
- [x] 覆盖同步控制台、交付、文件共享、屏幕共享、剪贴板和其他主要工具。
- [x] 增加准确的开发、检查、测试和版本化裸 EXE 命令。
- [x] 删除占位 GitHub 地址、许可证和安装包描述。

### 任务 4：一致性验证

**文件：**

- 检查：`AGENTS.md`
- 检查：`CLAUDE.md`
- 检查：`README.md`

- [x] 运行 `rg -n "your-repo|Your License Here|MainConsole|TaskStatusPage|save_config_cmd" AGENTS.md CLAUDE.md README.md`，预期无过时或占位命中（如保存命令仅用于解释禁止旧流程，则需人工确认语义）。
- [x] 从 `package.json` 读取 scripts，并人工核对文档命令。
- [x] 运行 `git diff --check`，预期退出码 0。
- [x] 逐份按 UTF-8 读取并检查标题、代码块和链接。
