# 整理项目入口文档

## Goal

让仓库的三个入口文档使用正确的中文编码、反映当前项目事实，并按不同读者明确分工，使 Codex、Claude 和普通使用者都能快速获得准确而不过度重复的信息。

## Background

- `AGENTS.md` 是 Codex 的仓库级指令入口，目前主体为英文，用户明确要求转为中文。
- `CLAUDE.md` 是 Claude 的开发指令入口，当前仍包含旧前端页面、旧后端模块和过时的全量配置保存说明。
- `README.md` 面向使用者和贡献者，当前仍以 `Copy` 为标题，只介绍早期同步/部署能力，并包含 `your-repo`、`Your License Here` 等占位内容。
- 当前项目已经扩展为 Tauri 2 + Vue 3 + Rust 的 Windows 工具箱，除同步控制台外还包含文件共享、屏幕共享、剪贴板、网络工具、磁盘缓存清理、错误码查询、远程产品包替换等工具。
- `package.json` 的权威检查与构建命令包括 `pnpm check`、`pnpm lint`、`pnpm build`、`pnpm test:share-web` 和 `cmd /c pnpm tauri:build:versioned-exe`；`src-tauri/tauri.conf.json` 当前 `bundle.active` 为 `false`。

## Requirements

- 将 `AGENTS.md` 的人工维护内容转为简体中文，同时保留 CodeGraph 和 Trellis 托管块的边界与语义。
- 让 `AGENTS.md` 与 `CLAUDE.md` 共享同一组稳定项目事实和开发约束，但分别保留面向对应代理的入口说明，避免互相声称是唯一真相来源。
- 更新两份代理文档中的技术栈、目录职责、配置域保存规则、国际化要求、验证命令、Windows/PowerShell 注意事项和 worktree 共享依赖规则。
- 将 `README.md` 重写为当前产品说明，覆盖同步控制台和主要工具能力，并提供准确的本地开发、检查、构建和版本化裸 EXE 说明。
- 删除无法由仓库确认的发布地址、许可证和安装包占位信息；不虚构 GitHub URL、许可证或对外发布渠道。
- 所有说明以当前代码、路由、配置和脚本为依据；不把易漂移的完整 Tauri command 清单或完整 `AppConfig` 字段复制进入口文档。
- 使用 UTF-8 中文文本，不保留乱码或中英混排标题（技术名词、命令和标识符除外）。

## Acceptance Criteria

- [x] `AGENTS.md` 的非托管项目说明全部为清晰中文，并保留 CodeGraph/Trellis 指令块。
- [x] `CLAUDE.md` 不再引用已删除页面、旧架构或过时保存方式，且与 `AGENTS.md` 的共享规则无冲突。
- [x] `README.md` 标题、功能清单、使用入口和开发命令符合当前仓库，不再包含 `your-repo`、`Your License Here` 或乱码。
- [x] 三份文档职责明确：代理文档指导开发代理，README 服务使用者和贡献者。
- [x] 文档中的命令均能在当前 `package.json` 或仓库脚本中找到对应实现。
- [x] `git diff --check` 通过，并通过关键词检查确认旧占位符和已知过时页面名称已清理。

## Out of Scope

- 不修改应用功能、代码行为、版本号或发布服务器配置。
- 不创建新的 GitHub 仓库地址、许可证文本或发布包。
- 不为所有工具编写完整用户手册；README 只提供入口级概览与开发说明。
