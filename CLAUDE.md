# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## 项目概述

Windows 桌面文件同步工具（Tauri 2.x + Vue 3 + Rust），用于自动监控远程共享目录、增量复制版本文件夹到本地，并通过 SSH/SFTP 部署到 Linux 服务器。

---

## 开发命令

```bash
# 前端开发（热重载，无 Tauri 壳）
pnpm dev

# 完整桌面开发模式
pnpm tauri dev

# 生产构建
pnpm tauri build

# Rust 格式化与检查
cargo fmt
cargo clippy
```

**重要**：每次修改完成后必须提交 git 并执行 `cmd /c pnpm tauri:build:versioned-exe` 验证构建通过。该命令先运行 `pnpm tauri build`，再通过 `scripts/rename-tauri-exe.mjs` 将产物重命名为 `file-sync-tool-1.0.0-YYYYMMDDHHmm.exe` 格式。

---

## 技术栈

| 层次 | 技术 |
|------|------|
| 前端框架 | Vue 3 + TypeScript (`<script setup>`) |
| 构建工具 | Vite |
| 样式 | Tailwind CSS 4 |
| 图标 | lucide-vue-next |
| 国际化 | vue-i18n |
| 包管理 | pnpm |
| 桌面框架 | Tauri 2.x |
| 后端语言 | Rust (Tokio 异步运行时) |
| SSH/SFTP | ssh2 crate |
| 文件操作 | fs_extra、tokio::fs |

---

## 代码架构

### 前端文件结构

```
src/
├── main.ts                 # Vue 应用入口，挂载 i18n 和 Router
├── App.vue                 # 根组件，订阅 log-message / copy-progress 事件
├── i18n.ts                 # i18n 初始化
├── router/index.ts         # 路由配置
├── lib/
│   ├── store.ts            # 全局响应式状态（日志、TaskRecord、进度）
│   ├── tauri.ts            # Tauri invoke 封装 + 所有接口类型定义（唯一类型真相来源）
│   ├── scheduler.ts        # 前端定时调度（setInterval + executeScan）
│   └── utils.ts            # 工具函数
├── composables/
│   └── useTheme.ts         # 主题相关 composable
├── pages/
│   ├── MainConsole.vue     # 控制台日志页（默认路由 /）
│   ├── TaskStatusPage.vue  # 任务状态页（/tasks）- 启停调度器、实时进度
│   ├── ManualCopyPage.vue  # 手动复制页（/manual-copy）- 触发 temporary_copy
│   ├── HistoryPage.vue     # 历史记录页（/history）
│   └── SettingsPage.vue    # 配置页（/settings）
├── components/
│   ├── Sidebar.vue         # 侧边导航栏
│   ├── TaskRecordsPanel.vue # 任务记录面板（进度表格）
│   └── Empty.vue           # 空状态占位组件
└── locales/messages.ts     # i18n 中英翻译（所有 UI 文本在此）
```

### 后端文件结构

```
src-tauri/src/
├── main.rs     # Tauri 入口；AppState 定义；所有 Command 注册；系统托盘；开机启动
├── lib.rs      # Tauri mobile 入口（run 函数导出）
├── config.rs   # AppConfig/ScanTask/DeployServer/CommandGroup 数据结构；配置读写；旧版迁移
├── scanner.rs  # 核心扫描与复制逻辑（scan_and_copy、temporary_copy、perform_copy）
├── deploy.rs   # SSH 连接、SFTP 上传（64KB 分块）、后置命令执行
└── history.rs  # 历史事件持久化（JSON，最多 100 条）
```

### Tauri Commands（main.rs 注册）

| Command | 说明 |
|---------|------|
| `get_config` | 读取当前配置 |
| `save_config_cmd` | 验证并保存配置，同步开机启动注册表 |
| `scan_now` | 触发一次扫描复制（使用 `is_scanning` AtomicBool 防并发） |
| `cancel_scan` | 取消扫描（同时解除暂停） |
| `pause_scan` / `resume_scan` | 暂停/继续 |
| `test_ssh_connection` | 测试 SSH 连通性 |
| `manual_deploy` | 手动触发 SFTP 部署（`spawn_blocking`） |
| `temporary_copy` | 手动复制页触发临时复制（复用 scanner 逻辑） |
| `get_app_paths` | 返回 config 路径和 log 路径 |
| `open_path_parent` | 用 Explorer 打开路径所在目录 |
| `get_history` / `clear_history` / `add_system_event` | 历史记录管理 |

### AppState（并发控制）

```rust
struct AppState {
    config: Arc<Mutex<AppConfig>>,         // 热读配置（复制完成后重新读取决定是否部署）
    is_scanning: Arc<AtomicBool>,          // 防止重复扫描（temporary_copy 也用此锁）
    is_manually_deploying: Arc<AtomicBool>,
    should_cancel: Arc<AtomicBool>,        // 文件分块循环中轮询
    is_paused: Arc<AtomicBool>,
    is_quitting: Arc<AtomicBool>,          // 区分"关闭到托盘"和"真正退出"
}
```

### 事件流

```
Tauri emit → Vue listen (App.vue):
  copy-progress  → store.upsertTaskRecord() → TaskRecordsPanel 进度表格
  log-message    → store.addLog() + store.syncTaskRecordByLog() → MainConsole
```

`store.ts` 中的 `TaskRecord` 系统负责将进度事件和日志消息聚合为统一的任务记录（支持合并同路径重复记录、阶段状态机转换）。

---

## 配置数据结构

完整的 `AppConfig`（定义在 `config.rs` 和 `tauri.ts`）：

```typescript
interface AppConfig {
  tasks: ScanTask[];
  local_path: string;
  interval_minutes: number;          // 最小值 5 分钟
  time_ranges: string[];             // "HH:mm-HH:mm" 格式
  file_extensions: string[];
  filename_includes: string[];       // OR 逻辑
  deploy_enabled: boolean;
  servers: DeployServer[];
  command_groups: CommandGroup[];    // 命名命令组，替代旧版 post_commands
  stability_check_secs: number;      // 文件写入稳定等待秒数（最小 60）
  recent_file_guard_mins: number;    // 近期文件必须等待稳定（最小 3）
  launch_and_auto_scan: boolean;     // 开机启动 + 启动后自动开始调度
  close_to_tray: boolean;            // 关闭按钮隐藏到托盘而非退出
  max_log_lines: number;             // 控制台最大日志行数（默认 200）
}

interface ScanTask {
  id: string;
  enabled: boolean;
  name: string;
  remote_path: string;
  local_path: string | null;         // null 时使用全局 local_path
  rule: { type: 'VersionMatch' | 'DateMatch'; value: string };
  server_bindings: TaskServerBinding[]; // 每个服务器绑定的命令组
}
```

`CommandGroup`（替代旧版全局 `post_commands`）：每个任务通过 `TaskServerBinding` 指定对哪些服务器执行哪些命令组，命令中支持 `${filename}` 变量（自动查找 `.tar.gz` 文件名）。

---

## MatchRule 详解

- **VersionMatch**：匹配 `YYYY_MM_DD_HH_MM_(版本号)` 格式，只处理今天或昨天的最新目录
- **DateMatch**：匹配 `chrono` 格式字符串的日期目录（默认 `%y%m%d`），遍历所有子目录增量复制

---

## 系统托盘与开机启动

- 托盘图标支持"显示主窗口"和"退出"菜单
- `close_to_tray=true` 时关闭按钮隐藏窗口而非退出（通过 `is_quitting` 标志区分）
- 开机启动通过写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 注册表实现

---

## 数据存储路径

- 配置文件：`%APPDATA%\<app>\config\config.json`
- 历史记录：`%APPDATA%\<app>\app_data\history.json`（最多 100 条）
- 日志文件：`%APPDATA%\<app>\app_data\app.log`

---

## 开发规则

### 国际化 (i18n)

所有面向用户的文本必须在 `src/locales/messages.ts` 中同时添加 `en` 和 `zh` 翻译，使用 `t('key')` 调用，禁止硬编码。

### 代码风格

- Vue：`<script setup>` + Composition API + Tailwind CSS 工具类
- 类型定义：所有接口在 `src/lib/tauri.ts` 中定义，Rust 侧在 `config.rs` 中对应
- Rust：遵循 `cargo fmt` 和 `clippy` 建议

### Git 工作流

- 提交信息使用中文
- 每次修改完成后提交 git 并执行 `cmd /c pnpm tauri:build:versioned-exe` 验证
