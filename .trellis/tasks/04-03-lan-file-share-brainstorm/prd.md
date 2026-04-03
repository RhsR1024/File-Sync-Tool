# brainstorm: 局域网文件共享工具 (LAN File Share)

## Goal

在现有「其它工具」Hub 中新增第 6 个工具卡片「局域网文件共享」。用户选择一个或多个本地目录后，点击启动，即可通过局域网 IP + 指定端口在浏览器中浏览文件列表、下载文件、打包下载文件夹，对标 CuteHttpFileServer (CHFS) 的核心功能，但不要求完整复刻——只保留 MVP 核心路径。

---

## What I Already Know

### 用户诉求
- 选择共享目录（支持多个）
- 配置端口（默认 9800）
- 可选密码保护
- 启动/停止服务
- 其他人通过 `http://<IP>:<PORT>` 访问：浏览目录、下载文件、打包下载文件夹

### 技术基础（已确认存在）
- `axum = "0.7"` 已依赖（Cargo.toml:47）
- `tokio` 全特性已引入
- `local-ip-address = "0.6"` 已引入（用于展示访问 URL）
- `rfd = "0.14"` 已引入（目录选择对话框）
- `sha2 = "0.10"` 已引入（密码 hash，复用 screenshare 的方案）
- `screenshare.rs` 提供完整的「axum + oneshot shutdown + 连接计数」模式可复用

### 需新增依赖
- `zip = "2"` — 文件夹打包为 .zip
- `tokio-util = { version = "0.7", features = ["io"] }` — 流式响应 (ReaderStream)
- `mime_guess = "2"` — 根据扩展名设置 Content-Type

### 现有前端模式（来自 ScreenSharePage.vue）
- `<script setup>` + Composition API + Tailwind CSS 4
- 左栏配置 + 右栏状态的 5:2 grid layout
- `onMounted` 监听 Tauri 事件（`listen`）
- 类型定义在 `src/lib/tauri.ts`，i18n key 在 `src/locales/messages.ts`

### ToolsHubPage.vue 的新增卡片模式
- 向 `toolCards` 数组追加一项（titleKey / descriptionKey / path / icon / iconClasses / chipKey）
- 对应 `router/index.ts` 追加一条懒加载路由
- Sidebar.vue 可能需要追加导航项（待确认）

---

## Research Notes

### CHFS 核心功能分析
CHFS（CuteHttpFileServer）是一个单文件 Windows HTTP 文件服务器，核心功能：
1. 浏览器访问目录列表（HTML 页面，文件/文件夹图标区分）
2. 单文件直接下载
3. 文件夹打包下载（.zip，服务端动态打包）
4. 可选密码保护（cookie-based session）
5. 支持多路径挂载（每个路径作为独立根节点）

### 多目录 URL 命名策略 — 方案分析

**方案 A：目录别名（alias）**（推荐）
- 每个共享目录分配一个 URL slug，如 `/browse/project-a/subdir/`
- slug 由用户指定或由目录名自动生成（`project-a` 来自 `D:\project-a`）
- 优点：URL 稳定、可读；缺点：多目录重名时需去重

**方案 B：索引号**
- 按加入顺序编号，如 `/browse/0/subdir/` `/browse/1/subdir/`
- 优点：简单；缺点：URL 无语义，目录顺序变化导致 URL 变

**方案 C：单目录（MVP 限制为 1 个）**
- 首期只支持单共享目录，简化路由和 UI
- 优点：最简单；缺点：用户需求明确需要多目录

→ 推荐 **方案 A（目录别名）**，slug 自动从目录名生成，支持手动修改。

### 文件夹 zip 流式打包 — 方案分析

**方案 A：内存缓冲（全量 zip 到 Vec<u8>）**
- 简单，`zip::ZipWriter` 写入 `Vec<u8>`，然后作为 Response body
- 风险：大文件夹 OOM（如 10 GB 的目录）

**方案 B：临时文件 + 流式读取**
- zip 写入 `tempfile::NamedTempFile`，完成后流式传输，下载完删除
- 优点：避免内存压力；缺点：需要磁盘空间，需管理临时文件生命周期

**方案 C：管道流式 zip（channel + async_stream）**
- 在 `spawn_blocking` 中同步写 zip 到一个 `std::io::Write` 端，另一端作为 HTTP body
- 实现复杂度较高，但真正零临时文件
- 最佳实践：使用 `tokio::sync::mpsc` + `async_stream`

→ MVP 推荐 **方案 B（临时文件）**：安全、实现简单、无 OOM 风险，适合局域网场景（文件通常不超过几 GB）。

### Path Traversal 防护策略
- 所有请求路径在 handler 中 canonical_path 化，验证必须以 shared_root 开头
- 使用 `std::path::Path::starts_with` 严格比较
- 不允许 `..`（在 normalize 后检查）
- 403 如果路径逃逸

### 连接数统计策略
- 每个文件下载请求计入一次连接（非持久）
- 通过 `AtomicU32` 追踪「活跃传输数」（下载开始 +1，完成/断开 -1）
- 与 screenshare 的 viewer_count 模式一致

---

## Requirements (MVP)

### 后端（fileshare.rs）
- [ ] `FileShareConfig`: port, shared_dirs (Vec<SharedDir>), password
- [ ] `SharedDir`: { alias: String, path: String }（alias 自动从目录名生成）
- [ ] `FileShareHandle`：复用 screenshare 的 active/cancel/shutdown_tx/connection_count 模式
- [ ] Tauri Commands：
  - `file_share_start(config: FileShareConfig) -> Result<String, String>` (返回 access URL)
  - `file_share_stop() -> Result<(), String>`
  - `file_share_get_status() -> FileShareStatus`
  - `file_share_pick_directory() -> Result<Option<SharedDir>, String>`（rfd 目录选择 + 自动生成 alias）
- [ ] HTTP 路由：
  - `GET /` → 根目录列表（各共享根的入口卡片）
  - `GET /browse/*path` → 目录浏览（HTML）
  - `GET /file/*path` → 单文件下载（流式）
  - `GET /zip/*path` → 文件夹打包下载（临时文件方案）
  - `POST /auth` + `GET /login` → 密码保护（可选，复用 screenshare 方案）
- [ ] 防 path traversal：canonicalize + starts_with 校验
- [ ] 启动时自动添加防火墙规则（复用 add_firewall_rule）
- [ ] Tauri emit `file-share-status` 事件（每秒，包含 connection_count + uptime）

### 前端（FileSharePage.vue）
- [ ] Header：图标 + 标题
- [ ] 主卡片（左右 grid，col-span-3 + col-span-2）：
  - 左栏：
    - 共享目录列表（可添加多个，每个可删除，显示 alias 和 path）
    - 端口号输入
    - 密码保护开关 + 密码输入
    - 错误提示
    - 启动/停止按钮
  - 右栏：
    - 状态指示灯（Active / Idle）
    - 访问 URL + 复制按钮
    - 活跃连接数
    - 运行时长
    - 空闲占位图

### 浏览器端内嵌 HTML（嵌入在 fileshare.rs 中）
- 简洁单页，支持移动端（响应式）
- 目录列表：图标区分文件/文件夹、文件大小、修改时间
- 文件夹行操作：点击浏览 / 下载ZIP按钮
- 文件行操作：点击直接下载
- 面包屑导航
- 可选密码登录页（复用 screenshare 的 login_html 风格）
- 中英文双语：读取请求 `Accept-Language` header，优先匹配 `zh`，否则英文；支持 `?lang=zh` / `?lang=en` URL 参数强制切换（手机访问自动跟随系统语言）

### ToolsHubPage.vue
- [ ] 新增一张卡片（Share2 图标，青绿色渐变）
- [ ] router/index.ts 追加路由 `/tools/file-share`

### i18n keys（messages.ts）
```
sidebar.fileShare
tools.fileShare.description
toolsHub.cards.fileShare.chip
toolsHub.cards.fileShare.description  (同上)
tools.fileShare.* （页面内所有文本）
```

---

## Acceptance Criteria

- [ ] 可在 ToolsHub 看到文件共享卡片，点击进入页面
- [ ] 可选择 1 个或多个本地目录（rfd 目录对话框）
- [ ] 可配置端口（默认 9800），启动成功返回访问 URL
- [ ] 浏览器访问 URL 后，可看到根目录文件夹列表
- [ ] 点击子目录可导航进入，面包屑可返回
- [ ] 点击文件可下载（Content-Type 正确）
- [ ] 点击文件夹下载 ZIP 按钮，可下载完整的 .zip 文件
- [ ] 密码保护开启时，未认证访问返回登录页
- [ ] Path traversal 攻击被拒绝（返回 403）
- [ ] 停止共享后端口立即释放，防火墙规则清理

---

## Definition of Done

- [ ] `cargo fmt` + `cargo clippy` 无警告
- [ ] 前端 TypeScript 类型检查通过（无 any 逃逸）
- [ ] 所有 UI 文本有 en/zh 双语 i18n
- [ ] 构建通过（`pnpm tauri build`）
- [ ] 手动测试：局域网另一台设备访问验证

---

## Decision (ADR-lite)

**多目录 URL 策略**
- Context: 多个共享目录需要在单个 HTTP 服务中区分
- Decision: 方案 A，目录别名（alias）作为 URL 第一段，自动从目录名生成
- Consequences: URL 清晰可读；需处理 alias 重名去重

**文件夹打包策略**
- Context: 文件夹可能较大，需避免 OOM
- Decision: 方案 B，临时文件（`std::env::temp_dir()`），下载完成后异步删除
- Consequences: 需要临时磁盘空间；生命周期通过 RAII 或 tokio::spawn 清理

**浏览器端语言**
- Context: 访问者可能使用不同语言的设备（手机、电脑）
- Decision: 读取 `Accept-Language` header，包含 `zh` 则显示中文，否则英文；支持 `?lang=zh/en` URL 参数强制覆盖
- Consequences: 无需额外配置，手机访问自动匹配系统语言；实现上只需 Rust 侧判断 header 值，将 lang 变量传入内嵌 HTML 模板

**连接数统计**
- Context: 需展示当前下载活跃数
- Decision: 每个下载（/file/ 和 /zip/）用 RAII guard 追踪活跃传输数（AtomicU32）
- Consequences: 简单直观；浏览操作不计入（仅下载计入）

---

## Out of Scope (MVP 不做)

- 文件上传（CHFS 支持，但 MVP 仅只读共享）
- 剪贴板文本分享（CHFS 功能）
- 断点续传（HTTP Range 请求）
- 多语言手动切换 UI（Accept-Language 自动检测已做，URL 参数强制已做；UI 语言切换按钮不做）
- 目录别名手动编辑（MVP 自动生成）
- 访问日志持久化
- 二维码（ScreenShare 已有，可后期迁移）

---

## Technical Notes

### 关键文件
- `src-tauri/src/screenshare.rs` — 完整架构参考
- `src-tauri/Cargo.toml` — 依赖管理
- `src-tauri/src/main.rs` — AppState 定义 + Command 注册
- `src/lib/tauri.ts` — 前端类型 + invoke 封装
- `src/locales/messages.ts` — i18n 中英文
- `src/pages/ScreenSharePage.vue` — 前端页面参考
- `src/pages/ToolsHubPage.vue` — 卡片注册
- `src/router/index.ts` — 路由注册

### 新增依赖
```toml
zip = "2"
tokio-util = { version = "0.7", features = ["io"] }
mime_guess = "2"
```

### HTTP 路由完整设计
```
GET  /              → 根目录（共享目录卡片列表）
GET  /browse/*path  → 目录浏览（path = alias/subpath）
GET  /file/*path    → 文件下载（流式，正确 Content-Type）
GET  /zip/*path     → 文件夹打包下载（临时文件）
GET  /login         → 登录页（密码保护启用时）
POST /auth          → 认证（设置 cookie）
GET  /api/status    → JSON 状态（活跃连接数等）
```

### AppState 新增字段
```rust
file_share: FileShareHandle,
```

### FileShareHandle 结构（参考 ScreenShareHandle）
```rust
pub struct FileShareHandle {
    active: Arc<AtomicBool>,
    connection_count: Arc<AtomicU32>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    server_url: Mutex<String>,
    start_time: Mutex<Option<Instant>>,
    config_port: Mutex<Option<u16>>,
}
```

### FileShareStatus（前端展示用）
```rust
pub struct FileShareStatus {
    pub is_active: bool,
    pub connection_count: u32,
    pub uptime_secs: u64,
    pub server_url: String,
    pub shared_dirs: Vec<SharedDir>,
}
```

### 浏览器端内嵌 HTML 关键 UX
- 面包屑：`首页 / alias / subdir`
- 文件列表表格：图标 | 名称（可点击） | 大小 | 修改时间 | 操作（文件夹有"下载ZIP"按钮）
- 移动端：隐藏大小/时间列，保留名称+操作
- 样式与 screenshare 的内嵌 HTML 风格一致（dark slate 主题）
