# 局域网文件共享工具设计

> **版本**：v1（基于实现后审查撰写）  
> **审查时间**：2026-04-03

---

## 背景

团队在局域网内需要快速共享文件目录，让同事、手机等设备通过浏览器直接浏览和下载，无需安装任何客户端、无需配置 FTP/SMB。共享者在本工具中选择目录后一键启动，观看者访问 `http://ip:port` 即可。

---

## 目标

1. 共享者选择一个或多个本地目录，配置别名、端口、密码后一键启动
2. 局域网内任意设备（手机、PC）浏览器访问，可浏览目录树、下载单文件、打包下载目录（ZIP）
3. 支持密码保护，防止未授权访问
4. 支持中英文双语界面（浏览器侧根据 Accept-Language 自动切换）
5. 实时显示当前连接数和运行时长

## 非目标

1. 不支持文件上传（仅下载）
2. 不支持跨广域网
3. 不支持实时同步/监控
4. 不支持 macOS/Linux（仅 Windows）

---

## 已知 Bug（v1 实现）与修复方案

### Bug 1：Windows 防火墙阻断入站连接（Critical）

**与屏幕共享工具相同问题。** `netsh advfirewall firewall add rule` 需要管理员权限，普通用户运行时静默失败。

**修复方案**：同屏幕共享 Bug 1 的修复方案——检查 netsh 退出码，失败时发送 `file-share-log` warn 事件，前端显示防火墙警告 banner。

### Bug 2：IP 地址检测不准（High）

**与屏幕共享 Bug 3 相同**：多网卡时 `local_ip_address::local_ip()` 可能返回 VPN/Docker 的 IP。

**修复方案**：枚举所有非回环 IPv4，`FileShareStatus` 新增 `all_urls: Vec<String>` 字段。

### Bug 3：connection_count 语义不准确（Medium）

**现象**：用户浏览目录时，连接数显示为 0；仅在下载文件/ZIP 时才计数。  
**根因**：`ConnectionGuard` 只在 `/file/{path}` 和 `/zip/{path}` 的流式响应中管理，`/browse/{path}` 等页面访问不计入。  
**期望语义**：`connection_count` 应该反映"当前活跃的客户端会话数"，而不仅仅是下载流数。

**修复方案 A**（简单）：将字段重命名为 `download_count`，前端标签改为"下载中"，准确反映实际含义。  
**修复方案 B**（完整）：在所有 handler 处通过中间件统计请求数（使用 `tower` layer 的请求计数器）。推荐方案 A，成本低且诚实。

### Bug 4：ZIP 下载时 temp 文件路径硬编码在系统 temp 目录（Low）

**现象**：若系统 temp 目录满或权限异常，ZIP 功能会返回 500。  
**根因**：使用 `std::env::temp_dir()`，在某些 Windows 环境下可能受限。  
**修复方案**：增加 error log，向前端 emit 错误事件，而非静默返回 500。

---

## 技术选型

| 组件 | 方案 | 原因 |
|------|------|------|
| HTTP 服务器 | `axum` (tokio) | 与项目依赖一致 |
| 文件服务 | 手工实现（异步流） | 控制下载头、安全路径校验 |
| ZIP 打包 | `zip` crate | 成熟稳定，阻塞任务用 spawn_blocking |
| 目录选择 | `rfd` crate | Tauri 生态，已有依赖 |
| MIME 类型 | `mime_guess` crate | 自动根据扩展名推断 |
| 密码认证 | SHA-256 + Cookie | 简单可靠，LocalStorage 备选 |
| 安全路径 | 手工 safe_join | 防路径穿越攻击 |

---

## 数据模型

### SharedDir

| 字段 | 类型 | 说明 |
|------|------|------|
| `alias` | `String` | URL 中的别名（URL-safe），如 `project-files` |
| `path` | `String` | 本地绝对路径 |

### FileShareConfig

| 字段 | 类型 | 说明 |
|------|------|------|
| `port` | `u16` | HTTP 端口，默认 9800 |
| `shared_dirs` | `Vec<SharedDir>` | 共享目录列表（至少 1 个） |
| `password` | `Option<String>` | 访问密码 |

### FileShareStatus

| 字段 | 类型 | 说明 |
|------|------|------|
| `is_active` | `bool` | 是否运行中 |
| `download_count` | `u32` | **修正命名**：当前活跃下载数（原 connection_count） |
| `uptime_secs` | `u64` | 运行时长 |
| `server_url` | `String` | 主访问地址 |
| `all_urls` | `Vec<String>` | **新增**：所有 LAN IP 对应的地址 |
| `shared_dirs` | `Vec<SharedDir>` | 当前共享的目录列表 |
| `firewall_ok` | `bool` | **新增**：防火墙规则是否添加成功 |

### TypeScript 侧（tauri.ts）

```typescript
export interface SharedDir {
  alias: string;
  path: string;
}

export interface FileShareConfig {
  port: number;
  shared_dirs: SharedDir[];
  password: string | null;
}

export interface FileShareStatus {
  is_active: boolean;
  download_count: number;      // 原 connection_count，重命名
  uptime_secs: number;
  server_url: string;
  all_urls: string[];           // 新增
  shared_dirs: SharedDir[];
  firewall_ok: boolean;         // 新增
}
```

---

## Tauri Commands

| Command | 参数 | 返回 | 说明 |
|---------|------|------|------|
| `file_share_pick_directory` | 无 | `Option<SharedDir>` | 打开目录选择对话框 |
| `file_share_start` | `FileShareConfig` | `Result<String, String>` | 启动服务 |
| `file_share_stop` | 无 | `Result<(), String>` | 停止服务 |
| `file_share_get_status` | 无 | `FileShareStatus` | 状态快照 |

### Tauri 事件

| 事件名 | Payload | 方向 | 说明 |
|--------|---------|------|------|
| `file-share-status` | `FileShareStatus` | Rust→Vue | 每秒推送 |
| `file-share-log` | `{ level, message }` | Rust→Vue | 日志 |

---

## HTTP 路由表

| 路径 | 方法 | 说明 |
|------|------|------|
| `/` | GET | 根页面：展示所有共享目录入口 |
| `/browse/{alias}/{*path}` | GET | 目录浏览页（支持多级） |
| `/file/{alias}/{*path}` | GET | 单文件下载（流式） |
| `/zip/{alias}/{*path}` | GET | 目录打包下载（ZIP） |
| `/login` | GET | 登录页 |
| `/auth` | POST | 密码验证，成功 Set-Cookie 跳转 `/` |

### 安全约束

- **路径穿越防护**：`safe_join()` 确保解析后的路径在 SharedDir 根目录内（`path.starts_with(root)`）
- **Cookie 校验**：所有需要认证的路由检查 `fs_auth=<sha256hash>` cookie
- **别名校验**：`find_root()` 只允许访问已注册的 alias

---

## 浏览器端 UI（axum 内嵌 HTML）

### 根页面（`/`）

展示所有共享目录列表，每个目录显示：
- 目录图标
- 别名（粗体）
- 本地路径（小字，可选隐藏）
- "浏览" 按钮 → 跳转 `/browse/{alias}/`

### 目录浏览页（`/browse/{alias}/{*path}`）

**面包屑导航**：根目录 / 子目录 / ... 可点击

**文件列表**（表格）：

| 图标 | 名称 | 类型 | 大小 | 操作 |
|------|------|------|------|------|
| 📁 | subdir | 目录 | - | [浏览] [下载ZIP] |
| 📄 | file.txt | 文件 | 12 KB | [下载] |

**操作**：
- 目录：点击名称进入子目录，[下载ZIP] 按钮打包下载
- 文件：点击名称或[下载]按钮直接下载

**布局**：深色顶部 header（显示 alias 和当前路径），白色背景内容区，移动端自适应

**语言**：根据 `Accept-Language` 自动切换中/英文（GET 参数 `?lang=zh` 可强制指定）

### 登录页（`/login`）

居中卡片，密码输入框，"进入" 按钮，错误提示（服务端通过 query param `?error=1` 控制）

---

## 共享者页面（FileSharePage.vue）

### 布局

```
┌─ Header: [Share2] 文件共享 ─────────────────────────────────┐
└─────────────────────────────────────────────────────────────┘

┌─ 白色圆角卡片 ───────────────────────────────────────────────┐
│  ┌─ 左：配置区域(3/5) ──────┐  ┌─ 右：状态面板(2/5) ───────┐ │
│  │ 共享目录列表              │  │ ● 未启动 / ● 共享中        │ │
│  │  [FolderOpen] alias      │  │                            │ │
│  │    /local/path    [删除]  │  │ [防火墙警告 banner]        │ │
│  │  ...                     │  │                            │ │
│  │  [+ 添加目录] 按钮        │  │ 访问地址                   │ │
│  │                          │  │ http://192.168.x.x:9800   │ │
│  │ 端口号 | 密码开关          │  │ [复制✓]                   │ │
│  │                          │  │                            │ │
│  │ [错误信息区域]            │  │ 下载中 | 运行时长           │ │
│  │                          │  │   0       00:05:32         │ │
│  │ [开始共享▶] / [停止■]     │  │                            │ │
│  └──────────────────────────┘  │ 备用地址（展开）            │ │
│                                └────────────────────────────┘ │
│  ─────────── 日志折叠区域 ──────────────────────────────────  │
└─────────────────────────────────────────────────────────────┘
```

### 防火墙警告 Banner（新增）

收到 `level: "warn"` 且日志含防火墙失败关键词时，在状态面板顶部显示：

```
┌─────────────────────────────────────────────────────┐
│ ⚠ 防火墙规则未自动添加。若其他设备无法连接，          │
│   请以管理员身份运行：                               │
│   [netsh advfirewall ... localport=9800] [复制命令]  │
└─────────────────────────────────────────────────────┘
```

### 状态指标修正

| 指标 | 原标签 | 修正后标签 | 说明 |
|------|--------|------------|------|
| `download_count` | 连接数 | 下载中 | 避免与"浏览"混淆 |
| `uptime_secs` | 运行时长 | 运行时长 | 不变 |

---

## 安全设计

| 威胁 | 缓解措施 |
|------|----------|
| 路径穿越 | `safe_join()` 规范化后检查前缀 |
| 未授权访问 | 密码保护 + HttpOnly Cookie |
| 大文件 OOM | 流式传输（64KB 分块），不整体加载 |
| ZIP 超时 | `spawn_blocking` + TempFile RAII 清理 |

---

## 实现分阶段

### Phase 1（Bug 修复，优先）

1. `add_firewall_rule` 检查退出码，失败时 emit warn 事件
2. 前端收到防火墙警告时显示 banner + 可复制命令
3. `FileShareStatus` 添加 `all_urls`、`firewall_ok` 字段
4. 枚举所有 LAN IP 填充 `all_urls`
5. 将 `connection_count` 重命名为 `download_count`（前后端一致更新）
6. ZIP 失败时 emit error 日志而非静默 500

### Phase 2（UI 完善）

1. 状态面板添加多 IP 备用地址折叠列表
2. 复制 URL 成功后短暂显示 "已复制 ✓" 提示（已实现 copiedUrl，确认显示时机正确）

### Phase 3（验证）

1. 构建 + 实机功能测试（局域网验证）
2. 手机浏览器测试（iOS Safari + Android Chrome）

---

## 验收标准

1. 局域网内其他设备浏览器可访问文件共享（防火墙规则生效或用户手动开放后）
2. 防火墙添加失败时显示 banner 和可复制命令
3. 目录浏览、文件下载、ZIP 打包下载均正常工作
4. 有密码时未登录跳转登录页，登录后正常访问
5. 错误密码提示服务端正确传递（`?error=1`）
6. 多网卡时所有 IP 均在状态面板展示
7. `pnpm tauri build` 构建通过
