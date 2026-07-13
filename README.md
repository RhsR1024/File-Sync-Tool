# File Sync Tool

File Sync Tool 是一个面向 Windows 的桌面工具箱，使用 **Tauri 2 + Vue 3 + TypeScript + Rust** 构建。它以网络共享目录扫描、增量复制和 Linux 远程交付为核心，同时集成多种日常运维与协作工具。

## 主要功能

### 同步控制台

- 按时间间隔和生效时段扫描远程共享目录。
- 支持版本规则和日期规则，按扩展名及文件名关键字过滤。
- 将匹配文件或目录增量复制到本地，并在复制前执行文件完整性等待。
- 提供任务组状态、实时速度、运行日志、历史记录、暂停、继续、取消和重试操作。
- 支持单路径或批量路径手动复制，并在提交前预览目标冲突。

### 远程交付

- 配置多个 Linux SSH/SFTP 目标服务器并测试连接。
- 按任务绑定服务器和命令组，控制远程命令执行顺序。
- 复制完成后可执行远程部署命令、本地脚本，或组合两种后处理流程。
- 支持手动选择本地文件并发起一次性部署。

### 文件与屏幕协作

- 局域网文件共享：提供独立 Web 页面、访问控制、上传下载和共享状态管理。
- 屏幕共享：提供采集后端选择、会话状态、观看地址和断线恢复。
- 剪贴板管理：支持文本、图片、分组、固定记录和独立预览窗口。

### 运维工具

- 一体机 SSH 访问控制与框架密码修改。
- 远程产品包替换。
- 网络诊断与端口扫描。
- Windows/Linux 磁盘缓存清理。
- 代码修改统计与错误码查询。
- 应用内版本检查、下载校验和更新状态展示。

## 运行要求

- Windows 10/11 64 位。
- WebView2 运行环境。
- 使用同步功能时，需要当前 Windows 用户具备目标 UNC/网络共享目录的访问权限。
- 使用远程交付时，需要可访问目标 Linux 服务器并具备相应 SSH/SFTP 权限。

## 基本使用

1. 打开“同步控制台 → 任务与策略”，设置本地目标目录、扫描间隔和过滤规则。
2. 添加扫描任务，配置远程路径、匹配规则及可选的任务级本地路径。
3. 如需远程交付，在“交付流程”中添加服务器、命令组和本地脚本，再绑定到任务。
4. 返回“概览”启动调度器，或立即执行扫描；运行状态、速度和任务记录会在控制台中更新。
5. 运行日志位于应用根级“运行日志”页面，历史事件位于“历史记录”。

其他工具可从左侧“工具总览”进入。大多数工具都有独立配置和状态提示，不依赖同步调度器运行。

## 本地开发

### 环境

- Node.js 与 pnpm。
- Rust stable 工具链。
- Tauri 2 在 Windows 上所需的系统依赖。

安装前端依赖：

```powershell
pnpm install
```

启动仅前端开发服务器：

```powershell
pnpm dev
```

启动完整桌面开发环境：

```powershell
pnpm tauri dev
```

## 检查与测试

```powershell
# Vue/TypeScript 类型检查
pnpm check

# ESLint
pnpm lint

# 文件共享 Web 前端测试
pnpm test:share-web

# 主前端和文件共享 Web 前端生产构建
pnpm build
```

部分模块使用与源码相邻的 Node `node:test` 测试文件，可按修改范围运行：

```powershell
node --test <相关的 *.test.mjs 文件>
```

## 构建版本化裸 EXE

```powershell
cmd /c pnpm tauri:build:versioned-exe
```

该命令依次执行：

1. Tauri production build。
2. 将生成的裸可执行文件复制并重命名为 `file-sync-tool-<版本>-<时间戳>.exe`。
3. 更新 `scripts/release-server/manifest.json` 中对应版本的文件名和 SHA-256。

项目当前在 `src-tauri/tauri.conf.json` 中设置 `bundle.active: false`，因此不会生成 MSI、NSIS 等安装包。实际 target 目录取决于本机 `CARGO_TARGET_DIR` 配置；未设置时使用 Cargo 默认 target 位置。

## 项目结构

```text
src/                         Vue 主应用
src/share-web/               局域网文件共享 Web 前端
src-tauri/src/               Rust/Tauri 后端
scripts/                     构建、EXE 重命名和发布 manifest 脚本
docs/                        专题设计与开发文档
AGENTS.md                    Codex 项目指令
CLAUDE.md                    Claude 项目指令
```

## 配置与数据

应用配置和运行数据写入 Tauri 应用数据目录。具体路径可在应用“设置”或“关于”页面查看；不要依赖硬编码的 `%APPDATA%` 子目录名称。

同步配置采用域级保存：同步任务和交付配置与通用应用设置分别更新，以避免不同页面保存时覆盖彼此的字段。

## 开发约定

- 用户界面文案必须同时维护中英文翻译。
- 前端 Tauri 类型统一维护在 `src/lib/tauri.ts`，Rust 侧结构以 `src-tauri/src/` 为准。
- 修改跨层配置字段时，需要同步检查默认值、迁移、序列化和持久化。
- 提交前按改动范围运行测试、类型检查和 lint，并检查 `git diff --check`。

更详细的代理开发规则请参阅 [AGENTS.md](./AGENTS.md) 或 [CLAUDE.md](./CLAUDE.md)。
