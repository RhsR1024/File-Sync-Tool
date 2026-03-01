# Copy — 自动文件同步与远程部署工具

完整开发文档（包含产品规范、技术架构、代码规则）

---

## 产品概述

这是一个 Windows 桌面文件同步工具，用于自动监控和拷贝远程共享目录中的特定版本文件夹到本地指定路径。工具支持定时扫描、版本过滤、状态监控、远程部署等功能，为开发团队提供自动化的版本文件获取和部署解决方案。

**目标用户**：需要在 Windows 环境下自动获取和部署远程版本文件的开发、测试、运维人员。通过自动化复制和部署减少手动操作，提高工作效率。

---

## 核心功能

### 文件扫描与复制
- **定时调度**：按配置间隔（分钟）自动触发扫描任务
- **时间范围限制**：可配置只在指定时段内运行（如 `05:00-09:00`）
- **两种匹配规则**：
  - **VersionMatch**：扫描 `YYYY_MM_DD_HH_MM_(版本号)` 格式的目录，匹配指定版本并限定在今天/昨天范围内
  - **DateMatch**：扫描当日日期格式的目录（如 `260211`），支持多构建子目录的增量检测
- **增量复制**：只复制目标目录中尚不存在的新文件，避免重复传输
- **文件过滤**：
  - 按扩展名过滤（如 `exe`、`tar.gz`）
  - 按文件名关键字过滤（OR 逻辑，如 `UMS`、`VMS`）
- **暂停 / 继续 / 取消**：支持对正在进行的复制任务实时控制

### 远程部署（Linux SSH）
- **多服务器支持**：可配置多个部署目标，按顺序串行执行
- **自动部署**：文件复制完成后，热检测当前部署开关状态，若已启用则立即触发远程部署（无需重启调度器）
- **手动部署**：在设置页面可直接指定本地路径和远程路径进行一次性部署
- **SFTP 文件上传**：64 KB 分块上传，支持暂停/取消
- **后置命令**：上传完成后在远程服务器执行 Shell 命令，支持 `${filename}` 变量替换（自动查找 `.tar.gz` 文件名）
- **连接测试**：保存服务器配置前可一键测试 SSH 连通性

### 进度监控
- **实时进度面板**：显示文件名、进度条、已传/总量、传输速度、预计剩余时间、已用时间
- **本地路径 / 远程路径**：实时显示当前操作的两端路径，带一键复制按钮
- **历史记录**：持久化存储最近 100 条操作记录（复制开始/完成/取消、部署事件）

### 其他
- **中英双语**：内置 i18n 国际化支持（中文 / English）
- **单实例保护**：防止多开重复运行
- **日志文件**：写入 `%APPDATA%/app/app.log`，带时间戳和级别

---

## 主流程

1. 用户配置远程路径、版本规则和扫描间隔
2. 启动定时任务，后台开始周期性扫描
3. 每次扫描时检查所有配置的远程路径
4. 匹配文件夹名称格式：`YYYY_MM_DD_HH_MM(Version)` 或按日期格式
5. 筛选符合指定版本且日期为当天或昨天的文件夹
6. 将匹配的文件夹增量复制到本地指定目录
7. 如启用远程部署，通过 SSH/SFTP 上传至 Linux 服务器
8. 更新日志和状态显示

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
| 序列化 | serde / serde_json |
| 唯一 ID | uuid |
| 时间处理 | chrono |
| 正则表达式 | regex |

### 技术选型理由

**为什么选择 Rust + Tauri 而非 Go**

**Rust + Tauri 优势：**
1. **原生性能**：Rust的系统级性能，适合大量文件IO操作
2. **小体积**：Tauri打包后exe文件通常小于10MB，远小于Go的50MB+
3. **内存安全**：Rust的内存安全机制避免运行时错误
4. **WebView2集成**：直接利用Windows内置的WebView2，无需额外运行时
5. **前端技术栈**：完美支持Vue 3，开发效率高

**Go方案劣势：**
1. 需要额外的WebView库（如Lorca、WebView），增加复杂度
2. 最终打包体积较大
3. GUI开发体验不如Web技术成熟

---

## 代码架构

```
Copy/
├── src/                        # 前端 (Vue 3 + TypeScript)
│   ├── main.ts                 # Vue 应用入口，挂载 i18n 和 Router
│   ├── App.vue                 # 根组件，订阅 log-message / copy-progress 事件
│   ├── i18n.ts                 # i18n 初始化
│   ├── router/
│   │   └── index.ts            # 路由配置（控制台 / 任务 / 历史 / 设置）
│   ├── lib/
│   │   ├── store.ts            # 全局响应式状态（日志、进度、调度器状态）
│   │   ├── tauri.ts            # Tauri invoke 封装 + 所有接口类型定义
│   │   └── scheduler.ts        # 前端定时调度逻辑（setInterval + executeScan）
│   ├── pages/
│   │   ├── TaskStatusPage.vue  # 任务状态页（启动/停止调度器、实时进度表格）
│   │   ├── SettingsPage.vue    # 配置页（任务管理、部署服务器、过滤规则）
│   │   ├── MainConsole.vue     # 控制台日志页
│   │   └── HistoryPage.vue     # 历史记录页
│   ├── components/
│   │   ├── Sidebar.vue         # 侧边导航栏
│   │   └── Empty.vue           # 空状态占位组件
│   └── locales/
│       └── messages.ts         # i18n 中英翻译
│
└── src-tauri/                  # 后端 (Rust + Tauri)
    └── src/
        ├── main.rs             # Tauri 入口；AppState 定义；所有 Command 注册
        ├── config.rs           # AppConfig / ScanTask / DeployServer 数据结构；
        │                       # 配置读写；旧版配置自动迁移
        ├── scanner.rs          # 核心扫描与复制逻辑；调用 deploy_to_remote
        ├── deploy.rs           # SSH 连接、SFTP 上传、后置命令执行
        └── history.rs          # 历史事件持久化（JSON 文件，最多 100 条）
```

### 后端架构

```
Tauri Command Handler (main.rs)
  ├─ scan_now()           → scanner::scan_and_copy()
  ├─ manual_deploy()      → deploy::deploy_manual()
  ├─ get_config()         → config::load_config()
  └─ save_config()        → config::save_config()
     │
     ▼
  Scheduler Service (scanner.rs)
     │
     ├─ VersionMatch:  扫描目录 → 筛选版本 → perform_copy()
     └─ DateMatch:     扫描日期目录 → 子目录循环 → perform_copy()
        │
        ▼
     File Copy Service (perform_copy)
        ├─ 收集待复制文件（扩展名 + 关键字过滤）
        ├─ 检测本地文件是否存在（增量判断）
        ├─ 64KB 分块复制，发送进度事件
        ├─ 写入历史记录
        └─ 热读配置，检测 deploy_enabled
           │
           ▼
        Deploy Service (deploy_to_remote)
           ├─ SSH 连接
           ├─ SFTP 上传（分块 + 进度）
           └─ 执行后置命令（变量替换）
```

---

## 核心数据流

```
前端调度器 (scheduler.ts)
  └─ setInterval → executeScan()
       └─ invoke('scan_now')
            │
            ▼
       main.rs: scan_now()
         ├─ 从 Arc<Mutex<AppConfig>> 读取最新配置
         └─ scanner::scan_and_copy(config, live_config, ...)
              │
              ├─ 检查时间范围
              ├─ 遍历启用的 ScanTask
              │    ├─ VersionMatch: 找最新版本目录 → perform_copy()
              │    └─ DateMatch: 扫描日期子目录 → perform_copy() × N
              │
              └─ perform_copy()
                   ├─ 收集待复制文件（过滤 + 增量判断）
                   ├─ 64KB 分块复制，emit copy-progress 事件
                   ├─ 写入历史记录 (COPY_COMPLETED)
                   └─ 重新读取 live_config（热检测）
                        └─ deploy_enabled=true → deploy_to_remote()
                             └─ 逐服务器 SSH 连接 → SFTP 上传 → 后置命令

事件流 (Tauri emit → Vue listen):
  copy-progress  → App.vue → appStore.progress → TaskStatusPage 进度表格
  log-message    → App.vue → appStore.logs     → MainConsole 日志面板
```

---

## 配置文件结构

配置存储于 `%APPDATA%\<app>\config\config.json`：

```json
{
  "tasks": [
    {
      "id": "uuid",
      "enabled": true,
      "name": "任务名称",
      "remote_path": "\\\\server\\share\\builds",
      "local_path": "E:\\local_target",
      "rule": {
        "type": "VersionMatch",
        "value": "1.3.9.P02"
      }
    },
    {
      "id": "uuid2",
      "enabled": true,
      "name": "日期任务",
      "remote_path": "\\\\server\\daily",
      "local_path": null,
      "rule": {
        "type": "DateMatch",
        "value": "%y%m%d"
      }
    }
  ],
  "local_path": "E:\\UMS_TEMP",
  "interval_minutes": 10,
  "time_ranges": ["05:00-09:00"],
  "file_extensions": ["exe", "tar.gz"],
  "filename_includes": ["UMS", "VMS"],
  "deploy_enabled": true,
  "servers": [
    {
      "id": "uuid",
      "enabled": true,
      "name": "生产服务器",
      "host": "192.168.1.100",
      "port": 22,
      "user": "deploy",
      "password": "encrypted_password",
      "remote_path": "/home/deploy/uploads"
    }
  ],
  "post_commands": [
    "cd /home/deploy/uploads && tar -zxvf ${filename}.tar.gz",
    "systemctl restart myservice"
  ]
}
```

历史记录存储于 `%APPDATA%\<app>\app_data\history.json`，日志写入 `%APPDATA%\<app>\app_data\app.log`。

---

## MatchRule 详解

### VersionMatch

扫描目录名格式：`YYYY_MM_DD_HH_MM_(版本号)`

示例：`2026_02_26_08_30(1.3.9.P02)`

规则：只处理目录日期为**今天或昨天**的最新条目，避免处理历史构建包。

### DateMatch

扫描目录名格式：由 `chrono` 格式字符串控制（默认 `%y%m%d`，即 `260226`）。

目录结构示意：
```
\\server\daily\
  └─ 260226\
       ├─ C1001\                ← 子目录逐个调用 perform_copy
       │    ├─ A.exe            ← 第一次扫描时复制
       │    └─ ...
       ├─ C1002\                ← 第二次扫描时复制（新子目录）
       │    ├─ B.exe            ← 直接复制
       │    └─ ...
       └─ ...
```

每次扫描时会遍历所有子目录，**只复制本地尚不存在的文件**。同一子目录内追加新文件时也会被检测并复制。

---

## 后置命令变量替换

在 `post_commands` 中可使用 `${filename}` 占位符：

- 程序会扫描已上传的本地目录，查找第一个 `.tar.gz` 文件
- 将其文件名（去掉 `.tar.gz` 后缀）替换到命令中

示例：
```bash
# 配置
"tar -zxvf ${filename}.tar.gz -C /opt/app"

# 若目录中存在 UMS_1.3.9.P02.tar.gz，实际执行：
tar -zxvf UMS_1.3.9.P02.tar.gz -C /opt/app
```

---

## UI 设计规范

### 设计风格
- **主色调**：深蓝色（#1E40AF）配合灰色（#6B7280）
- **按钮样式**：圆角矩形，主要操作为蓝色，危险操作为红色
- **字体**：系统默认字体，标题14px，正文12px
- **布局风格**：左侧导航栏 + 右侧内容区的经典桌面应用布局
- **图标风格**：使用简洁的线性图标，符合Windows设计规范
- **响应式**：桌面优先设计，固定窗口大小，针对Windows桌面环境优化

### 页面模块

| 页面名称 | 模块名称 | 功能描述 |
|---------|---------|---------|
| 任务状态 | 状态卡片 | 显示当前任务执行状态（运行中/停止）、下次执行时间 |
| 任务状态 | 进度表格 | 实时显示任务执行进度、本地/远程路径、速度、ETA |
| 任务状态 | 操作按钮 | 启动/停止定时任务、立即执行一次扫描、暂停/继续/取消 |
| 设置页面 | 任务列表 | 表格形式显示所有扫描任务，支持添加、编辑、删除操作 |
| 设置页面 | 部署服务器 | 管理多个 SSH 部署目标，支持测试连通性 |
| 设置页面 | 过滤规则 | 配置文件扩展名、文件名关键字、时间范围 |
| 控制台页面 | 日志显示 | 实时显示任务执行日志，按时间戳显示，支持复制和清空 |
| 历史页面 | 历史记录 | 显示最近 100 条操作记录（复制/部署事件），支持清空 |

---

## 开发规则

### 国际化 (i18n)

1. **双语支持**：所有面向用户的文本必须同时支持英语 (en) 和中文 (zh)
2. **键值一致性**：新功能必须在 `src/locales/messages.ts` 中为两种语言添加对应的键
3. **避免硬编码**：不要在 Vue 组件中硬编码文本。使用 `t('key')` 模式

### 代码风格

1. **Vue Composition API**：使用 `<script setup>` 和 Composition API
2. **Tailwind CSS**：使用 Tailwind 工具类进行样式设计
3. **类型安全**：为数据结构使用 TypeScript 接口（例如 `AppConfig`, `ScanTask`, `DeployServer`, `HistoryEntry`）
4. **Rust 风格**：遵循 `cargo fmt` 和 `clippy` 建议

### Git 工作流

1. **提交信息**：使用中文编写描述性的提交信息
2. **保持整洁**：提交前尽可能确保没有未使用的导入或警告
3. **功能分支**：为新功能创建 feature 分支，完成后提 PR 到 main
4. **每次修改完成必须**：提交 git 并执行 `pnpm tauri build` 验证构建通过

### 开发命令

```bash
# 前端开发
pnpm dev

# 启动 Tauri 开发
pnpm tauri dev

# 构建生产版本
pnpm tauri build

# 代码格式化（Rust）
cargo fmt

# Rust 检查（Rust）
cargo clippy
```

---

## 关键设计决策

| 问题 | 决策 | 理由 |
|------|------|------|
| 调度器启动后才启用远程部署 | 使用 `Arc<Mutex<AppConfig>>` 在复制完成后热读最新配置 | 无需重启调度器即可响应配置变更 |
| 多服务器部署 | 串行执行，避免 SSH/SFTP 并发冲突 | 确保稳定性和 UI 进度清晰 |
| 大文件传输 | 64 KB 分块 + 每 200ms 推送一次进度事件 | 平衡精度与性能 |
| 增量同步 | 以本地文件是否存在作为判断依据 | 简单可靠，无需维护额外的元数据 |
| 配置迁移 | 自动将旧版字段迁移为新格式 | 保证向后兼容性 |
| 取消/暂停 | 使用 `AtomicBool` 标志，在文件分块循环中轮询 | 确保低延迟响应 |
| Windows UNC 路径 | 使用系统默认 API 访问网络路径 | 充分利用系统能力，无需额外依赖 |

---

## 开发与构建

### 环境要求

- [Node.js](https://nodejs.org/) >= 18
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) + Cargo（stable）
- [Tauri CLI](https://tauri.app/start/prerequisites/)

### 安装依赖

```bash
pnpm install
```

### 开发模式

```bash
pnpm tauri dev
```

### 生产构建

```bash
pnpm tauri build
```

构建产物位于 `src-tauri/target/release/`，安装包位于 `src-tauri/target/release/bundle/`。

---

## 性能优化策略

1. **增量扫描**：记录已扫描的文件夹，避免重复处理
2. **并发处理**：同时扫描多个远程路径（Tokio 异步）
3. **内存管理**：流式复制大文件（64KB 分块），控制内存使用
4. **网络优化**：设置合理的超时时间和重试机制
5. **日志轮转**：自动清理过期日志，避免日志文件过大

---

## 错误处理和监控

- **网络错误**：处理 UNC 路径不可访问、权限不足、SSH 连接失败等问题
- **文件系统错误**：处理磁盘空间不足、文件被占用等情况
- **配置错误**：验证配置格式，提供友好的错误提示
- **运行时监控**：记录关键指标和错误日志，便于问题排查

