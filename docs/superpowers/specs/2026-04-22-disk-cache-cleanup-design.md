# 硬盘缓存清理工具 + 四工具最近使用持久化 — 设计文档

**日期**: 2026-04-22
**范围**: 在「其他工具」中新增独立工具"硬盘缓存清理"；同时为另外三个已有工具补充"最近使用"输入持久化能力。
**实现分支**: `main`

---

## 1. 总体架构与数据流

新增独立页面 `/tools/disk-cache-cleanup`，挂到侧边栏「其他工具」组、ToolsHub 卡片中。

```
┌──────── Vue 页面（DiskCacheCleanupPage.vue）────────┐
│  状态：hostIp / recentHosts / serverList / picked    │
│         disks / redisAvailable / loading / errors    │
└────────┬────────────┬────────────┬───────────────────┘
         │ invoke     │ invoke     │ invoke
         ▼            ▼            ▼
┌──────────────── Tauri Commands ────────────────────┐
│  disk_cleanup_list_servers(host, timeout)           │
│    → HTTP POST  /openAPI/.../disk/server/list       │
│  disk_cleanup_list_disks(host, serverIp, timeout)   │
│    → HTTP POST  /openAPI/.../disk/list              │
│  disk_cleanup_check_redis(host, storageIds)         │
│    → Redis AUTH → Pipeline EXISTS → return set      │
│  disk_cleanup_delete_cache(host, storageIds)        │
│    → Redis AUTH → Pipeline DEL → return counts      │
└─────────────────────────────────────────────────────┘
```

要点：

- 前端通过 `save_kv('diskCacheCleanup.recentHosts', [...])` 维护最近 10 条 host IP（LRU）。
- 每次 `disk_cleanup_list_servers` 成功后把 host IP 推入最近列表。
- 两个 Redis Command 各自连接 / 关闭（短连接），不维护持久连接池。
- HTTP 超时由 `AppConfig.disk_cleanup_http_timeout_secs`（默认 5 秒）控制；Redis 连接 / 命令超时硬编码 3 秒。
- **两个 IP 概念**：
  - **接入 IP（host IP）**：HTTP API 端点 + Redis 连接目标（`hostIP:6379`）
  - **内部 serverIp**：`/disk/server/list` 返回的子机/备机 IP，仅用于 `/disk/list` 的请求体
  - Redis 始终连"接入 IP"，不连内部 serverIp

---

## 2. 页面布局与交互

页面自上而下分 4 块：

```
┌─ A. 接入 IP 选择区 ────────────────────────────────────┐
│   [输入框带建议下拉] ─ 建议项来源：                    │
│     • 最近使用（带时钟图标，LRU）                      │
│     • 已保存 SSH 服务器（带 Server 图标）              │
│   [获取硬盘列表] 主按钮（loading 态：禁用 + 旋转）     │
└────────────────────────────────────────────────────────┘

┌─ B. 内部服务器选择区（首次调用成功后出现）────────────┐
│   [下拉框] ServerName · ServerIp（role / haType 小字） │
│   右侧：[手动刷新服务器列表] 小按钮                    │
│   下拉变化立即触发 /disk/list（带小 loading 条）       │
└────────────────────────────────────────────────────────┘

┌─ C. 状态横幅（可选，根据错误显示）────────────────────┐
│   红色：HTTP 失败 / 琥珀：Redis 不可用                 │
│   附「重试」按钮（刷新整条流程）                       │
└────────────────────────────────────────────────────────┘

┌─ D. 硬盘列表（布局 C：可展开表格）────────────────────┐
│   工具栏：[刷新] [一键清理全部(N)]  —  已选择 serverIp │
│   列：▸ | 槽位 | 设备 | 容量 | 用途徽章 | 状态徽章     │
│       | 缓存 | 操作                                    │
│   展开行：storageId · 机箱 · storageType · WWN 列表    │
└────────────────────────────────────────────────────────┘
```

**关键交互**：

1. 打开页面 → `load_kv` 载入最近 10 条；输入框 placeholder 显示「输入 IP 或从建议中选择」。
2. 点击「获取硬盘列表」 → 调 `/disk/server/list`；成功则把 host IP 入最近列表。
3. 服务器下拉变化 → 并行调 `/disk/list` + `disk_cleanup_check_redis`。
4. 单条「清理缓存」 → `disk_cleanup_delete_cache([storageId])` → 完成后**完整重放第 3 步**（重查磁盘列表 + Redis）。
5. 「一键清理全部」 → 仅对当前「缓存存在」的磁盘批量 DEL；点击前弹二次确认对话框；完成后同样重放第 3 步。
6. 错误横幅在对应错误时显示。

**Redis 不可用时**：保留所有清理按钮和「一键清理全部」按钮，但**置灰不可点击**，hover tooltip 显示「Redis 不可用，无法操作」。

**视觉主题**：白底 + 浅灰背景 + Tailwind 卡片，与 NetworkTools / FrameworkPassword 一致；头部图标使用蓝紫渐变 `HardDrive`，工具卡片渐变 `from-rose-500 to-orange-600`。

**布局选择**：可展开表格（Option C）。空闲态紧凑（每行 ~42px），点击 ▸ 展开看 storageId / WWN 等细节。空磁盘列表显示空状态占位。

---

## 3. 后端 Rust 模块与命令

新增文件 `src-tauri/src/disk_cleanup.rs`。

### 3.1 数据结构（与前端 `tauri.ts` 对齐）

```rust
pub struct DiskServerItem {
    pub server_name: String,
    pub server_ip: String,
    pub role: String,
    pub serial: String,
    pub ha_type: i32,
    pub server_code: i32,
}

pub struct DiskInfoItem {
    pub storage_id: String,
    pub storage_type: i32,
    pub slot: i32,
    pub enclosure_index: i32,
    pub storage_status: i32,    // 1..23
    pub total_capacity: i64,    // GB
    pub usage: i32,             // 1..5, 255, -1
    pub device_name: String,
    pub world_wide_name_list: Vec<Wwn>,
}

pub struct Wwn { pub wwn: String, pub block_size: i64 }

pub struct CacheCheckResult {
    pub present_ids: Vec<String>,   // Redis 中存在 Storage:XXX 的 storageId
    pub redis_available: bool,
    pub error: Option<String>,
}

pub struct CacheDeleteResult {
    pub deleted_count: i64,
    pub redis_available: bool,
    pub error: Option<String>,
}
```

### 3.2 Tauri Commands

| Command | 入参 | 返回 |
|---|---|---|
| `disk_cleanup_list_servers` | `host: String, timeout_secs: u32` | `Vec<DiskServerItem>` |
| `disk_cleanup_list_disks` | `host: String, server_ip: String, timeout_secs: u32` | `Vec<DiskInfoItem>` |
| `disk_cleanup_check_redis` | `host: String, storage_ids: Vec<String>` | `CacheCheckResult` |
| `disk_cleanup_delete_cache` | `host: String, storage_ids: Vec<String>` | `CacheDeleteResult` |

### 3.3 实现要点

- **Cargo.toml** 新增：`redis = { version = "0.25", features = ["tokio-comp"] }`
- HTTP 走异步 `reqwest`，与现有一体机 SSH / 框架密码工具同构。
- Redis 短连接：每次命令新建 → `AUTH ums@redis_service` → Pipeline → 释放。
- 连接超时与命令超时 3 秒，硬编码。
- 所有错误 `Result<T, String>`，错误信息中文，区分「连接失败 / 认证失败 / 协议错误」。
- 模块内不持有全局状态。
- 接口 URL 模板：
  - `http://{host}:23011/openAPI/system/v1/disk/server/list`
  - `http://{host}:23011/openAPI/system/v1/disk/list`
- Redis key 模板：`Storage:{storageId}`

### 3.4 失败处理矩阵

| 场景 | 处理 |
|---|---|
| HTTP `/disk/server/list` 失败 | 顶部红色横幅，不进入第二步 |
| HTTP `/disk/list` 失败 | 顶部红色横幅；保留服务器下拉，可换一台再试 |
| Redis 连接失败 | 顶部琥珀色横幅；硬盘列表正常显示，所有清理按钮置灰 |
| Redis Pipeline EXISTS 部分失败 | 整批视为 Redis 失败 |
| 单条 / 批量 DEL 失败 | 弹 toast 错误，不刷新列表 |

---

## 4. 配置扩展与持久化

### 4.1 `AppConfig` 新增字段

`src-tauri/src/config.rs` 与 `src/lib/tauri.ts` 同步：

```rust
pub struct AppConfig {
    // ... 现有字段
    #[serde(default = "default_disk_cleanup_http_timeout")]
    pub disk_cleanup_http_timeout_secs: u32,    // 默认 5
}

fn default_disk_cleanup_http_timeout() -> u32 { 5 }
```

迁移策略与 `appliance_ssh_api_timeout_secs` 一致：缺失即填默认值。

### 4.2 KV 持久化键（复用 `save_kv` / `load_kv`）

| 键名 | 值 | 工具 | 写入时机 |
|---|---|---|---|
| `diskCacheCleanup.recentHosts` | `string[]` ≤10 | 硬盘缓存清理（新） | 成功调用 `/disk/server/list` 后 |
| `applianceSsh.recentIps` | `string[]` ≤10 | 一体机 SSH | 每次「开启」提交时（合并当前 `manualIpTags`） |
| `frameworkPassword.recentIps` | `string[]` ≤10 | 框架密码修改 | 每次「修改」提交时 |
| `networkTools.pingScan.recentPrefixes` | `string[]` ≤10 | Ping 网段扫描 | 每次成功启动扫描时 |

LRU 规则：新值插到数组头部，去重后截断到 10 条。读取时按数组顺序展示（最近在最上）。

### 4.3 UI 呈现

- **硬盘缓存清理**：建议下拉里分两组「📌 最近使用」和「🖥 已保存 SSH 服务器」。
- **一体机 SSH / 框架密码**：在 `manualIpInput` 框下方新增「最近使用」Chip 列表，点击直接 `addManualIpTag()`。
- **Ping 网段扫描**：`prefix` 输入框下方「最近使用」Chip 列表，点击直接填入 `prefix`。

每个 Chip 行布局：

```
最近使用： [192.168.1.10 ×] [10.0.0.5 ×] [...]    清空
```

- 点击 Chip 主体填入对应输入；
- 右侧 `×` 单条删除并 `save_kv` 回写；
- 末端「清空」一次性清除该工具的最近列表。

### 4.4 超时控件位置

- `disk_cleanup_http_timeout_secs` 字段在 `AppConfig` 持久化；UI 控件放在 `DiskCacheCleanupPage.vue` 页面底部，对齐 `EnableApplianceSshPage.vue:646` 的「API 请求超时」下拉框模式。
- 控件：`<select>`，可选 1 / 2 / 3 / 5 / 10 / 15 / 30 秒，默认 5。
- 变更立即 `saveConfig()` 写回主配置。
- **Settings 页面不新增任何字段。**

---

## 5. i18n 与状态/用途字典

### 5.1 新增翻译键（`src/locales/messages.ts`）

顶层区块 `diskCacheCleanup`（zh + en），覆盖：

```
diskCacheCleanup.title           硬盘缓存清理 / Disk Cache Cleanup
diskCacheCleanup.description     清理一体机硬盘在 Redis 中的配置残留缓存 / …
diskCacheCleanup.hostIp.label    接入 IP
diskCacheCleanup.hostIp.placeholder  输入 IP 或从建议中选择
diskCacheCleanup.hostIp.recentGroup  最近使用
diskCacheCleanup.hostIp.serversGroup 已保存 SSH 服务器
diskCacheCleanup.actions.fetch       获取硬盘列表
diskCacheCleanup.actions.refresh     刷新
diskCacheCleanup.actions.cleanOne    清理缓存
diskCacheCleanup.actions.cleanAll    一键清理全部 ({count})
diskCacheCleanup.actions.cleanAllConfirm  确定清理当前 {count} 条 Redis 缓存？
diskCacheCleanup.server.pick         选择子机
diskCacheCleanup.server.refresh      刷新服务器列表
diskCacheCleanup.disks.empty         该服务器未返回硬盘数据
diskCacheCleanup.disks.columns.*     槽位 / 设备 / 容量 / 用途 / 状态 / 缓存 / 操作
diskCacheCleanup.disks.expandHint    展开查看 storageId / WWN 列表
diskCacheCleanup.cache.present       Redis 缓存存在
diskCacheCleanup.cache.absent        —
diskCacheCleanup.cache.unavailable   Redis 不可用，无法判定
diskCacheCleanup.errors.http         获取失败：{reason}
diskCacheCleanup.errors.redis        Redis 连接失败：{reason}
diskCacheCleanup.timeout.label       API 请求超时
diskCacheCleanup.disabled.redisDown  Redis 不可用，无法操作
diskCacheCleanup.status.1..23        （23 个状态枚举中英翻译）
diskCacheCleanup.usage.{1,2,3,4,5,255,-1}  图片存储 / 录像存储 / 录像备份 / 热备 / 故障转移备份 / 其他 / 未设置
```

`sidebar` 区块：`sidebar.diskCacheCleanup`
`toolsHub` 区块：`toolsHub.cards.diskCacheCleanup.{description,chip}`

现有区块小幅扩展：`applianceSsh.recentIps`、`frameworkPassword.recentIps`、`networkTools.ping.recentPrefixes` 及对应的「清空」「最近使用」标签。

### 5.2 状态徽章配色（23 种 → 4 色桶）

| 色 | 枚举 |
|---|---|
| 绿 | 1 正常, 13 配置完成 |
| 蓝（带 loading 点动效） | 4 重建中, 7 初始化中, 8 检查资源中, 9 正在格式化, 10 配置资源中, 11 删除中, 12 资源解绑中, 16 扩容中, 20 清理资源中 |
| 琥珀 | 5 衰退, 14 分区不满足, 19 部分在线, 21 待配置, 22 锁定删除失败 |
| 红 | 2 异常, 3 离线, 6 无法使用, 15 配置失败, 17 扩容失败, 18 删除失败, 23 清理失败 |

### 5.3 用途徽章配色

| 色 | 枚举 |
|---|---|
| 蓝色描边 | 1 图片存储 |
| 绿色描边 | 2 录像存储 |
| 青色描边 | 3 录像备份 |
| 红色实心 | 4 热备 |
| 红色描边 | 5 故障转移备份 |
| 灰色 | 255 其他 |
| 不显示（空白占位） | -1 未设置 |

---

## 6. 集成点、验证与文件改动清单

### 6.1 注册到现有导航

| 文件 | 改动 |
|---|---|
| `src/router/index.ts` | 新增路由 `/tools/disk-cache-cleanup` → 懒加载 `DiskCacheCleanupPage.vue` |
| `src/lib/sidebarNavigation.ts` | tools section 加新条目（key `disk-cache-cleanup`，iconKey `diskCacheCleanup`），新增 `SidebarIconKey` 联合类型成员 |
| `src/components/Sidebar.vue` | `iconMap` 增加 `diskCacheCleanup: HardDrive` |
| `src/pages/ToolsHubPage.vue` | `toolCards` 数组追加新卡片，渐变 `from-rose-500 to-orange-600` |
| `src-tauri/src/main.rs` | 新增 `mod disk_cleanup;` + `invoke_handler` 注册 4 个 command |
| `src-tauri/Cargo.toml` | 新增 `redis` 依赖 |

### 6.2 三个已有工具的最近使用扩展

| 文件 | 改动 |
|---|---|
| `src/pages/EnableApplianceSshPage.vue` | onMounted 时 `load_kv('applianceSsh.recentIps')` → 渲染 Chip 列表；提交时把当前 `manualIpTags` 合并写回（LRU 截到 10） |
| `src/pages/FrameworkPasswordPage.vue` | 同上，键 `frameworkPassword.recentIps` |
| `src/components/network/PingScanTab.vue` | onMounted 时 `load_kv('networkTools.pingScan.recentPrefixes')` → Chip 列表；`startScan` 成功后入队 |

每个工具的 Chip 行：

```
最近使用： [192.168.1.10 ×] [10.0.0.5 ×] [...]    清空
```

### 6.3 验证清单

构建即视为通过：

- `cmd /c pnpm tauri:build:versioned-exe` 成功，产物命名正确
- `cargo clippy` 无新增警告
- `vue-tsc` 类型检查通过（`tauri.ts` 接口对齐 Rust 结构）

手测路径（无单元测试，纯桌面回归）：

1. 全新启动 → 进入硬盘缓存清理页 → 输入 IP → 获取列表 → 选服务器 → 看到 16 块盘 + 缓存徽章
2. 单条清理 → 该行清理按钮变灰 / 缓存徽章消失 → 再次刷新无 button
3. 一键清理 → 二次确认 → 全部缓存按钮消失
4. 错误 IP（HTTP 不通）→ 红色横幅，列表不出现
5. 正确 IP，Redis 端口被防火墙挡 → 琥珀横幅，列表显示但所有清理按钮置灰
6. 改超时为 1 秒 → 验证生效（重新打开页面、下次请求生效）
7. 三个旧工具：填 IP / 前缀、提交 → 关页面再回来 → 最近使用 Chip 在
8. 升级路径：旧版本 `config.json` 缺新字段时正常加载并自动补默认值

### 6.4 不做（明确划出范围）

- 不做硬盘缓存的「批量按用途/状态过滤」
- 不做 Redis 写入操作（只读 EXISTS、写 DEL 这一对）
- 不做「自动定时刷新硬盘列表」
- 不做 SSH 服务器列表的反向筛选
- 不做密码可配置（Redis 密码硬编码 `ums@redis_service`）
- 不做「接入 IP 健康检查」独立按钮（合并到「获取硬盘列表」流程里）
- 不做文件共享 Web URL 路由（独立 spec 后续处理）

---

## 7. 实现顺序建议

1. 后端 Rust：新增 `disk_cleanup.rs` + `Cargo.toml` 依赖 + `main.rs` 注册
2. 配置 `AppConfig` 新字段 + 默认值迁移
3. 前端 `tauri.ts` 接口与类型对齐
4. 新建 `DiskCacheCleanupPage.vue`（按布局 C 实现）
5. 注册路由 / 侧边栏 / ToolsHub 卡片
6. i18n 字典补全（重点：23 状态 + 7 用途）
7. 三个旧工具补「最近使用」Chip 行
8. 构建验证 + 手测回归 + 提交
