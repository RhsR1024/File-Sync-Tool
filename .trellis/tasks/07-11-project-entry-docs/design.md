# 项目入口文档整理设计

## 1. 文档职责

### `AGENTS.md`

Codex 的仓库级执行规范。保留 CodeGraph 与 Trellis 托管块，人工维护部分使用简体中文，重点描述项目事实、开发约束、验证要求和 Windows 工作方式。

### `CLAUDE.md`

Claude 的仓库级执行规范。与 `AGENTS.md` 共享技术栈、架构边界、配置保存、国际化、验证和 worktree 规则，但不复制 Codex 专属措辞。两份代理文档允许结构相近，以降低不同代理产生行为差异的风险。

### `README.md`

面向使用者和贡献者的产品入口。介绍 File Sync Tool 当前定位、主要功能、运行要求、基本使用、开发命令和裸 EXE 产物。它不承担代理工作流说明，也不枚举内部 API。

## 2. 内容边界

- 稳定事实可以进入入口文档：技术栈、主要目录、配置域边界、标准命令、主要功能模块。
- 易漂移细节不进入入口文档：完整 Tauri command 表、完整 `AppConfig` 字段、每个路由组件、每个 Rust 内部函数。
- 未被仓库证实的信息必须删除：GitHub 发布地址、Issue 地址、许可证名称、安装包形式。
- `bundle.active: false` 意味着当前生产构建交付裸 EXE；版本化脚本在构建后重命名并更新 manifest。

## 3. 一致性规则

- `AGENTS.md` 与 `CLAUDE.md` 的共享事实必须一致，但各自保持独立可读。
- `README.md` 可引用开发命令，但不重复代理专属执行规则。
- 用户界面文字必须通过 Vue I18n，并在 `src/locales/messages.ts` 的中英文区域同步维护。
- 同步配置通过 `configStore` 的域级保存动作更新，不能在文档中继续推荐旧的整对象保存流程。

## 4. 验证

- 检查三份文件均可按 UTF-8 正常读取。
- 搜索并清除 `your-repo`、`Your License Here`、旧页面名和旧架构描述。
- 核对 README 中的命令存在于 `package.json` 或仓库脚本。
- 运行 `git diff --check`。

## 5. 回滚

修改仅涉及 Markdown 文档和本任务规划文件。如发现事实错误，可逐段回退对应文档，不影响应用代码、配置和构建产物。
