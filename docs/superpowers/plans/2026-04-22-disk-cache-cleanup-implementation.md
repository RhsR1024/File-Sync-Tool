# 硬盘缓存清理工具 + 四工具最近使用持久化 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为「其他工具」新增"硬盘缓存清理"独立页面（HTTP 调 /disk/server/list + /disk/list，Redis 管理 `Storage:*` 缓存 key），并为一体机 SSH / 框架密码修改 / Ping 网段扫描三个已有工具补充"最近使用"输入持久化。

**Architecture:** Rust 后端新增 `disk_cleanup.rs` 模块暴露 4 个 Tauri Commands（HTTP 复用已有 reqwest，Redis 用新增 `redis` crate 的短连接 + pipeline）；Vue 前端新增 `DiskCacheCleanupPage.vue` 挂到 `/tools/disk-cache-cleanup`；三个旧工具通过已有 `save_kv`/`load_kv` 持久化各自的最近使用列表。

**Tech Stack:** Tauri 2.x · Rust + Tokio · reqwest · redis 0.25 · Vue 3 `<script setup>` · Tailwind 4 · vue-i18n · lucide-vue-next

**Spec:** [docs/superpowers/specs/2026-04-22-disk-cache-cleanup-design.md](../specs/2026-04-22-disk-cache-cleanup-design.md)

---

## 文件地图

**新增文件：**
- `src-tauri/src/disk_cleanup.rs` — HTTP 调用 + Redis 短连接 + 4 个 Commands
- `src/pages/DiskCacheCleanupPage.vue` — 独立工具页面

**修改文件：**
- `src-tauri/Cargo.toml` — 新增 `redis` 依赖
- `src-tauri/src/main.rs` — 注册 `mod disk_cleanup` + 4 个 invoke_handler
- `src-tauri/src/config.rs` — 新增 `disk_cleanup_http_timeout_secs` 字段与默认值
- `src/lib/tauri.ts` — 新增 4 个接口类型 + 4 个 invoke wrapper + AppConfig 字段
- `src/lib/sidebarNavigation.ts` — 添加 tools 导航项、`SidebarIconKey` 联合
- `src/components/Sidebar.vue` — `iconMap` 添加 `diskCacheCleanup: HardDrive`
- `src/pages/ToolsHubPage.vue` — `toolCards` 追加卡片
- `src/router/index.ts` — 新增路由
- `src/locales/messages.ts` — i18n 字典（zh + en）
- `src/pages/EnableApplianceSshPage.vue` — 最近使用 Chip 行
- `src/pages/FrameworkPasswordPage.vue` — 最近使用 Chip 行
- `src/components/network/PingScanTab.vue` — 最近使用 Chip 行

---

## Task 1: 添加 redis 依赖、disk_cleanup 模块骨架与 4 个 Command 占位

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/disk_cleanup.rs`
- Modify: `src-tauri/src/main.rs` (add `mod disk_cleanup;` + invoke_handler 注册)

- [ ] **Step 1: 添加 redis 依赖**

Edit `src-tauri/Cargo.toml`，在 `base64 = "0.22"` 行之后追加：

```toml
redis = { version = "0.25", features = ["tokio-comp"] }
```

- [ ] **Step 2: 创建 disk_cleanup.rs 骨架与数据结构**

Create `src-tauri/src/disk_cleanup.rs`：

```rust
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskServerItem {
    #[serde(rename = "serverName")]
    pub server_name: String,
    #[serde(rename = "serverIp")]
    pub server_ip: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub serial: String,
    #[serde(rename = "haType", default)]
    pub ha_type: i32,
    #[serde(rename = "serverCode", default)]
    pub server_code: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Wwn {
    pub wwn: String,
    #[serde(rename = "blockSize", default)]
    pub block_size: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskInfoItem {
    #[serde(rename = "storageId")]
    pub storage_id: String,
    #[serde(rename = "storageType", default)]
    pub storage_type: i32,
    #[serde(default)]
    pub slot: i32,
    #[serde(rename = "enclosureIndex", default)]
    pub enclosure_index: i32,
    #[serde(rename = "storageStatus")]
    pub storage_status: i32,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: i64,
    #[serde(default = "default_usage")]
    pub usage: i32,
    #[serde(rename = "deviceName", default)]
    pub device_name: String,
    #[serde(rename = "worldWideNameList", default)]
    pub world_wide_name_list: Vec<Wwn>,
}

fn default_usage() -> i32 {
    -1
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheCheckResult {
    pub present_ids: Vec<String>,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheDeleteResult {
    pub deleted_count: i64,
    pub redis_available: bool,
    pub error: Option<String>,
}

const REDIS_PORT: u16 = 6379;
const REDIS_PASSWORD: &str = "ums@redis_service";
const REDIS_OP_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const STORAGE_KEY_PREFIX: &str = "Storage:";

#[tauri::command]
pub async fn disk_cleanup_list_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    let _ = (host, timeout_secs);
    Err("not implemented".to_string())
}

#[tauri::command]
pub async fn disk_cleanup_list_disks(
    host: String,
    server_ip: String,
    timeout_secs: u32,
) -> Result<Vec<DiskInfoItem>, String> {
    let _ = (host, server_ip, timeout_secs);
    Err("not implemented".to_string())
}

#[tauri::command]
pub async fn disk_cleanup_check_redis(
    host: String,
    storage_ids: Vec<String>,
) -> CacheCheckResult {
    let _ = (host, storage_ids);
    CacheCheckResult {
        present_ids: vec![],
        redis_available: false,
        error: Some("not implemented".to_string()),
    }
}

#[tauri::command]
pub async fn disk_cleanup_delete_cache(
    host: String,
    storage_ids: Vec<String>,
) -> CacheDeleteResult {
    let _ = (host, storage_ids);
    CacheDeleteResult {
        deleted_count: 0,
        redis_available: false,
        error: Some("not implemented".to_string()),
    }
}
```

- [ ] **Step 3: 注册到 main.rs**

Edit `src-tauri/src/main.rs`。在现有 `mod network;` 行后添加：

```rust
mod disk_cleanup;
```

在 `invoke_handler` 列表里（`enable_appliance_ssh,` 行之后）追加：

```rust
disk_cleanup::disk_cleanup_list_servers,
disk_cleanup::disk_cleanup_list_disks,
disk_cleanup::disk_cleanup_check_redis,
disk_cleanup::disk_cleanup_delete_cache,
```

- [ ] **Step 4: 验证编译通过**

Run: `cd src-tauri && cargo check`
Expected: `Compiling app v1.0.6` 完成，无错误（warnings about unused params 可接受）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/disk_cleanup.rs src-tauri/src/main.rs
git commit -m "feat(disk-cleanup): 添加 redis 依赖与模块骨架"
```

---

## Task 2: 实现 disk_cleanup_list_servers（HTTP）

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 添加 HTTP 客户端构造与响应包装**

Edit `src-tauri/src/disk_cleanup.rs`，在 `STORAGE_KEY_PREFIX` 常量之后添加：

```rust
#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    code: i32,
    #[serde(default)]
    message: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct ServerListData {
    #[serde(rename = "serverList", default)]
    server_list: Vec<DiskServerItem>,
}

#[derive(Debug, Deserialize)]
struct DiskListData {
    #[serde(rename = "storageInfoList", default)]
    storage_info_list: Vec<DiskInfoItem>,
}

fn build_http_client(timeout_secs: u32) -> Result<reqwest::Client, String> {
    let total = Duration::from_secs(timeout_secs.max(1) as u64);
    reqwest::Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(total)
        .build()
        .map_err(|e| format!("HTTP 客户端构造失败: {}", e))
}

async fn post_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    body: serde_json::Value,
) -> Result<T, String> {
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), text.trim()));
    }
    let parsed: ApiEnvelope<T> = serde_json::from_str(text.trim())
        .map_err(|e| format!("响应解析失败: {}", e))?;
    if parsed.code != 0 {
        return Err(parsed
            .message
            .unwrap_or_else(|| format!("接口返回 code {}", parsed.code)));
    }
    parsed.data.ok_or_else(|| "响应缺少 data 字段".to_string())
}
```

- [ ] **Step 2: 替换 disk_cleanup_list_servers 实现**

Replace the function body with:

```rust
#[tauri::command]
pub async fn disk_cleanup_list_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    let client = build_http_client(timeout_secs)?;
    let url = format!("http://{}:23011/openAPI/system/v1/disk/server/list", host);
    let data: ServerListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.server_list)
}
```

Also add at the top of the file:

```rust
use serde_json;
```

(serde_json 已经在项目依赖中，直接 use 即可。)

- [ ] **Step 3: cargo check 验证**

Run: `cd src-tauri && cargo check`
Expected: 无错误。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/disk_cleanup.rs
git commit -m "feat(disk-cleanup): 实现 disk_cleanup_list_servers HTTP 调用"
```

---

## Task 3: 实现 disk_cleanup_list_disks（HTTP）

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 替换 disk_cleanup_list_disks 实现**

Edit `src-tauri/src/disk_cleanup.rs`，替换 `disk_cleanup_list_disks` 函数：

```rust
#[tauri::command]
pub async fn disk_cleanup_list_disks(
    host: String,
    server_ip: String,
    timeout_secs: u32,
) -> Result<Vec<DiskInfoItem>, String> {
    let client = build_http_client(timeout_secs)?;
    let url = format!("http://{}:23011/openAPI/system/v1/disk/list", host);
    let data: DiskListData = post_json(
        &client,
        &url,
        serde_json::json!({ "serverIp": server_ip }),
    )
    .await?;
    Ok(data.storage_info_list)
}
```

- [ ] **Step 2: cargo check 验证**

Run: `cd src-tauri && cargo check`
Expected: 无错误。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/disk_cleanup.rs
git commit -m "feat(disk-cleanup): 实现 disk_cleanup_list_disks HTTP 调用"
```

---

## Task 4: 实现 Redis 短连接 helper + disk_cleanup_check_redis

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 添加 Redis 连接 helper**

Edit `src-tauri/src/disk_cleanup.rs`，在文件末尾添加：

```rust
async fn connect_redis(host: &str) -> Result<redis::aio::Connection, String> {
    let url = format!(
        "redis://:{}@{}:{}",
        REDIS_PASSWORD, host, REDIS_PORT
    );
    let client = redis::Client::open(url).map_err(|e| format!("Redis URL 无效: {}", e))?;
    tokio::time::timeout(REDIS_OP_TIMEOUT, client.get_async_connection())
        .await
        .map_err(|_| "Redis 连接超时".to_string())?
        .map_err(|e| {
            let msg = e.to_string();
            if msg.to_lowercase().contains("auth") {
                format!("Redis 认证失败: {}", msg)
            } else {
                format!("Redis 连接失败: {}", msg)
            }
        })
}
```

- [ ] **Step 2: 替换 disk_cleanup_check_redis 实现**

Replace the function:

```rust
#[tauri::command]
pub async fn disk_cleanup_check_redis(
    host: String,
    storage_ids: Vec<String>,
) -> CacheCheckResult {
    if storage_ids.is_empty() {
        return CacheCheckResult {
            present_ids: vec![],
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(c) => c,
        Err(e) => {
            return CacheCheckResult {
                present_ids: vec![],
                redis_available: false,
                error: Some(e),
            }
        }
    };

    let mut pipe = redis::pipe();
    for id in &storage_ids {
        pipe.cmd("EXISTS").arg(format!("{}{}", STORAGE_KEY_PREFIX, id));
    }

    let exec = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        pipe.query_async::<_, Vec<i64>>(&mut conn),
    )
    .await;

    match exec {
        Err(_) => CacheCheckResult {
            present_ids: vec![],
            redis_available: false,
            error: Some("Redis 查询超时".to_string()),
        },
        Ok(Err(e)) => CacheCheckResult {
            present_ids: vec![],
            redis_available: false,
            error: Some(format!("Redis EXISTS 失败: {}", e)),
        },
        Ok(Ok(flags)) => {
            let present_ids: Vec<String> = storage_ids
                .into_iter()
                .zip(flags.into_iter())
                .filter_map(|(id, flag)| if flag == 1 { Some(id) } else { None })
                .collect();
            CacheCheckResult {
                present_ids,
                redis_available: true,
                error: None,
            }
        }
    }
}
```

- [ ] **Step 3: cargo check 验证**

Run: `cd src-tauri && cargo check`
Expected: 无错误。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/disk_cleanup.rs
git commit -m "feat(disk-cleanup): 实现 Redis EXISTS pipeline 缓存状态查询"
```

---

## Task 5: 实现 disk_cleanup_delete_cache（Redis DEL）

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 替换 disk_cleanup_delete_cache 实现**

Edit `src-tauri/src/disk_cleanup.rs`，替换函数：

```rust
#[tauri::command]
pub async fn disk_cleanup_delete_cache(
    host: String,
    storage_ids: Vec<String>,
) -> CacheDeleteResult {
    if storage_ids.is_empty() {
        return CacheDeleteResult {
            deleted_count: 0,
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(c) => c,
        Err(e) => {
            return CacheDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(e),
            }
        }
    };

    let keys: Vec<String> = storage_ids
        .iter()
        .map(|id| format!("{}{}", STORAGE_KEY_PREFIX, id))
        .collect();

    let exec = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        redis::cmd("DEL").arg(&keys).query_async::<_, i64>(&mut conn),
    )
    .await;

    match exec {
        Err(_) => CacheDeleteResult {
            deleted_count: 0,
            redis_available: false,
            error: Some("Redis 删除超时".to_string()),
        },
        Ok(Err(e)) => CacheDeleteResult {
            deleted_count: 0,
            redis_available: false,
            error: Some(format!("Redis DEL 失败: {}", e)),
        },
        Ok(Ok(count)) => CacheDeleteResult {
            deleted_count: count,
            redis_available: true,
            error: None,
        },
    }
}
```

- [ ] **Step 2: cargo check 验证**

Run: `cd src-tauri && cargo check`
Expected: 无错误。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/disk_cleanup.rs
git commit -m "feat(disk-cleanup): 实现 Redis DEL 批量清理"
```

---

## Task 6: AppConfig 新增 disk_cleanup_http_timeout_secs 字段

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: 添加字段与默认值**

Edit `src-tauri/src/config.rs`，在 `framework_password_api_timeout_secs` 字段之后（约 167 行）插入：

```rust
    /// HTTP request timeout in seconds for disk cache cleanup (/disk/server/list, /disk/list).
    /// Default: 5.
    #[serde(default = "default_disk_cleanup_http_timeout_secs")]
    pub disk_cleanup_http_timeout_secs: u64,
```

在 `default_framework_password_api_timeout_secs` 函数之后添加：

```rust
fn default_disk_cleanup_http_timeout_secs() -> u64 {
    5
}
```

在 `AppConfig` 的 `Default::default()` 实现里（`framework_password_api_timeout_secs: 5,` 行之后）插入：

```rust
            disk_cleanup_http_timeout_secs: 5,
```

- [ ] **Step 2: cargo check 验证**

Run: `cd src-tauri && cargo check`
Expected: 无错误。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat(disk-cleanup): AppConfig 新增 disk_cleanup_http_timeout_secs"
```

---

## Task 7: 前端 tauri.ts 新增接口类型、wrapper 与 AppConfig 字段

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 扩展 AppConfig 接口**

Edit `src/lib/tauri.ts`，在 `framework_password_api_timeout_secs: number;` 字段之后添加：

```ts
  /** HTTP request timeout in seconds for disk cache cleanup API. Default: 5. */
  disk_cleanup_http_timeout_secs: number;
```

- [ ] **Step 2: 在文件末尾追加 disk cleanup 类型与 wrapper**

Append at the very end of `src/lib/tauri.ts`：

```ts
// ─── Disk Cache Cleanup ────────────────────────────────

export interface DiskServerItem {
  serverName: string;
  serverIp: string;
  role: string;
  serial: string;
  haType: number;
  serverCode: number;
}

export interface Wwn {
  wwn: string;
  blockSize: number;
}

export interface DiskInfoItem {
  storageId: string;
  storageType: number;
  slot: number;
  enclosureIndex: number;
  storageStatus: number;
  totalCapacity: number;
  usage: number;
  deviceName: string;
  worldWideNameList: Wwn[];
}

export interface CacheCheckResult {
  present_ids: string[];
  redis_available: boolean;
  error: string | null;
}

export interface CacheDeleteResult {
  deleted_count: number;
  redis_available: boolean;
  error: string | null;
}

export async function diskCleanupListServers(
  host: string,
  timeoutSecs: number,
): Promise<DiskServerItem[]> {
  return await invoke<DiskServerItem[]>('disk_cleanup_list_servers', {
    host,
    timeoutSecs,
  });
}

export async function diskCleanupListDisks(
  host: string,
  serverIp: string,
  timeoutSecs: number,
): Promise<DiskInfoItem[]> {
  return await invoke<DiskInfoItem[]>('disk_cleanup_list_disks', {
    host,
    serverIp,
    timeoutSecs,
  });
}

export async function diskCleanupCheckRedis(
  host: string,
  storageIds: string[],
): Promise<CacheCheckResult> {
  return await invoke<CacheCheckResult>('disk_cleanup_check_redis', {
    host,
    storageIds,
  });
}

export async function diskCleanupDeleteCache(
  host: string,
  storageIds: string[],
): Promise<CacheDeleteResult> {
  return await invoke<CacheDeleteResult>('disk_cleanup_delete_cache', {
    host,
    storageIds,
  });
}
```

- [ ] **Step 3: 类型检查通过**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 4: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(disk-cleanup): 前端接口类型与 invoke wrapper"
```

---

## Task 8: i18n 字典 zh + en 完整补全

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: 在 zh 和 en 的 sidebar 区块分别追加**

Edit `src/locales/messages.ts`。在 zh.sidebar 区块的 `fileShare:` 同级追加：

```ts
      diskCacheCleanup: '硬盘缓存清理',
```

在 en.sidebar 区块同位置追加：

```ts
      diskCacheCleanup: 'Disk Cache Cleanup',
```

- [ ] **Step 2: 追加 toolsHub 卡片描述**

在 zh.toolsHub.cards 区块追加：

```ts
        diskCacheCleanup: {
          description: '清理一体机硬盘在 Redis 中的配置残留缓存，按槽位查看状态、用途并单条或批量清理。',
          chip: '运维',
        },
```

在 en.toolsHub.cards 同位置追加：

```ts
        diskCacheCleanup: {
          description: 'Clean up residual disk config caches (Storage:*) in Redis by slot — view status, usage and remove entries individually or in batch.',
          chip: 'OPS',
        },
```

- [ ] **Step 3: 追加顶层 diskCacheCleanup 字典（zh）**

在 zh 顶层追加整段：

```ts
    diskCacheCleanup: {
      title: '硬盘缓存清理',
      description: '清理一体机硬盘在 Redis 中的配置残留缓存',
      hostIp: {
        label: '接入 IP',
        placeholder: '输入 IP 或从建议中选择',
        recentGroup: '最近使用',
        serversGroup: '已保存 SSH 服务器',
      },
      actions: {
        fetch: '获取硬盘列表',
        fetching: '获取中...',
        refresh: '刷新',
        cleanOne: '清理缓存',
        cleanAll: '一键清理全部 ({count})',
        cleanAllConfirm: '确定清理当前 {count} 条 Redis 缓存？',
        confirm: '确定',
        cancel: '取消',
      },
      server: {
        pick: '选择子机',
        refresh: '刷新服务器列表',
      },
      disks: {
        empty: '该服务器未返回硬盘数据',
        columns: {
          slot: '槽位',
          device: '设备',
          capacity: '容量',
          usage: '用途',
          status: '状态',
          cache: '缓存',
          actions: '操作',
        },
        expandHint: '展开查看详情',
        storageId: 'Storage ID',
        enclosure: '机箱',
        wwn: 'WWN',
      },
      cache: {
        present: 'Redis 缓存存在',
        absent: '—',
        unavailable: 'Redis 不可用，无法判定',
      },
      errors: {
        http: '获取失败：{reason}',
        redis: 'Redis 连接失败：{reason}',
        deleteSingle: '清理缓存失败：{reason}',
        deleteBatch: '批量清理失败：{reason}',
        hostEmpty: '请先输入接入 IP',
      },
      timeout: {
        label: 'API 请求超时（秒）',
      },
      disabled: {
        redisDown: 'Redis 不可用，无法操作',
      },
      status: {
        '1': '正常',
        '2': '异常',
        '3': '离线',
        '4': '重建中',
        '5': '衰退',
        '6': '无法使用',
        '7': '初始化中',
        '8': '检查资源中',
        '9': '正在格式化',
        '10': '配置资源中',
        '11': '删除中',
        '12': '资源解绑中',
        '13': '配置完成',
        '14': '分区不满足配置条件',
        '15': '配置失败',
        '16': '扩容中',
        '17': '扩容失败',
        '18': '删除失败',
        '19': '部分在线',
        '20': '清理资源中',
        '21': '待配置',
        '22': '存在锁定录像，删除失败',
        '23': '清理失败',
      },
      usage: {
        '1': '图片存储',
        '2': '录像存储',
        '3': '录像备份',
        '4': '热备',
        '5': '故障转移备份',
        '255': '其他',
        '-1': '未设置',
      },
      recentHosts: {
        clear: '清空',
        label: '最近使用',
      },
    },
```

- [ ] **Step 4: 追加顶层 diskCacheCleanup 字典（en）**

在 en 顶层追加整段：

```ts
    diskCacheCleanup: {
      title: 'Disk Cache Cleanup',
      description: 'Clean Redis residual caches for appliance disks',
      hostIp: {
        label: 'Host IP',
        placeholder: 'Enter an IP or pick a suggestion',
        recentGroup: 'Recent',
        serversGroup: 'Saved SSH servers',
      },
      actions: {
        fetch: 'Fetch disk list',
        fetching: 'Fetching...',
        refresh: 'Refresh',
        cleanOne: 'Clean cache',
        cleanAll: 'Clean all ({count})',
        cleanAllConfirm: 'Delete the {count} cached Redis key(s) now?',
        confirm: 'Confirm',
        cancel: 'Cancel',
      },
      server: {
        pick: 'Pick server',
        refresh: 'Refresh server list',
      },
      disks: {
        empty: 'No disks returned for this server',
        columns: {
          slot: 'Slot',
          device: 'Device',
          capacity: 'Capacity',
          usage: 'Usage',
          status: 'Status',
          cache: 'Cache',
          actions: 'Actions',
        },
        expandHint: 'Expand to see details',
        storageId: 'Storage ID',
        enclosure: 'Enclosure',
        wwn: 'WWN',
      },
      cache: {
        present: 'Cached in Redis',
        absent: '—',
        unavailable: 'Redis unavailable — cannot determine',
      },
      errors: {
        http: 'Fetch failed: {reason}',
        redis: 'Redis connection failed: {reason}',
        deleteSingle: 'Clean failed: {reason}',
        deleteBatch: 'Batch clean failed: {reason}',
        hostEmpty: 'Enter a host IP first',
      },
      timeout: {
        label: 'API request timeout (sec)',
      },
      disabled: {
        redisDown: 'Redis unavailable — cannot operate',
      },
      status: {
        '1': 'Normal',
        '2': 'Abnormal',
        '3': 'Offline',
        '4': 'Rebuilding',
        '5': 'Degrading',
        '6': 'Unusable',
        '7': 'Initializing',
        '8': 'Checking resources',
        '9': 'Formatting',
        '10': 'Configuring resources',
        '11': 'Deleting',
        '12': 'Unbinding resource',
        '13': 'Completed',
        '14': 'Partition not eligible',
        '15': 'Config failed',
        '16': 'Expanding',
        '17': 'Expand failed',
        '18': 'Delete failed',
        '19': 'Partially online',
        '20': 'Cleaning resources',
        '21': 'Waiting configuration',
        '22': 'Locked recording — delete failed',
        '23': 'Clean failed',
      },
      usage: {
        '1': 'Image storage',
        '2': 'Video storage',
        '3': 'Video backup',
        '4': 'Hot spare',
        '5': 'Video failover',
        '255': 'Other',
        '-1': 'Not set',
      },
      recentHosts: {
        clear: 'Clear',
        label: 'Recent',
      },
    },
```

- [ ] **Step 5: 追加三个旧工具的最近使用文案**

在 zh.applianceSsh 区块追加（或与现有对象合并）：

```ts
      recentIps: {
        label: '最近使用',
        clear: '清空',
      },
```

同样在 zh.frameworkPassword 和 zh.networkTools.ping 中追加相应 `recentIps` / `recentPrefixes` 条目。en 版本同步翻译（`label: 'Recent'`、`clear: 'Clear'`，Ping 键名 `recentPrefixes`）。

具体键路径：
- `applianceSsh.recentIps.{label,clear}`
- `frameworkPassword.recentIps.{label,clear}`
- `networkTools.ping.recentPrefixes.{label,clear}`

- [ ] **Step 6: 类型检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 7: Commit**

```bash
git add src/locales/messages.ts
git commit -m "feat(disk-cleanup,i18n): 新增硬盘缓存清理与四工具最近使用字典"
```

---

## Task 9: 注册 sidebar、iconMap、ToolsHub 卡片与路由

**Files:**
- Modify: `src/lib/sidebarNavigation.ts`
- Modify: `src/components/Sidebar.vue`
- Modify: `src/pages/ToolsHubPage.vue`
- Modify: `src/router/index.ts`

- [ ] **Step 1: 扩展 SidebarIconKey 与导航项**

Edit `src/lib/sidebarNavigation.ts`。在 `SidebarIconKey` 联合类型中追加：

```ts
  | 'diskCacheCleanup';
```

在 tools section 的 items 数组中（`file-share` 之后）插入：

```ts
      {
        key: 'disk-cache-cleanup',
        labelKey: 'sidebar.diskCacheCleanup',
        path: '/tools/disk-cache-cleanup',
        iconKey: 'diskCacheCleanup',
        matchMode: 'prefix',
      },
```

- [ ] **Step 2: 扩展 Sidebar.vue iconMap**

Edit `src/components/Sidebar.vue`。在 `lucide-vue-next` import 中追加 `HardDrive`：

```ts
import {
  Activity,
  BarChart3,
  Globe,
  HardDrive,
  History,
  KeyRound,
  ListChecks,
  MonitorUp,
  Server,
  Settings,
  Share2,
  Shield,
  ShieldCheck,
} from 'lucide-vue-next';
```

在 `iconMap` 对象末尾追加：

```ts
  diskCacheCleanup: HardDrive,
```

- [ ] **Step 3: ToolsHub 追加卡片**

Edit `src/pages/ToolsHubPage.vue`。在 `lucide-vue-next` import 中加入 `HardDrive`（跟随现有 import 格式）。

在 `toolCards` computed 数组末尾（`file-share` 卡片之后）追加：

```ts
  {
    key: 'disk-cache-cleanup',
    titleKey: 'sidebar.diskCacheCleanup',
    descriptionKey: 'toolsHub.cards.diskCacheCleanup.description',
    path: '/tools/disk-cache-cleanup',
    icon: markRaw(HardDrive as LucideIcon),
    iconClasses: 'from-rose-500 to-orange-600 shadow-rose-500/20',
    chipKey: 'toolsHub.cards.diskCacheCleanup.chip',
  },
```

注意保留现有 `<div>` 头部图标条数量一致或酌情添加一枚 `HardDrive` 迷你图标（与其他 6 个并列，可省略）。

- [ ] **Step 4: 注册路由**

Edit `src/router/index.ts`。在 `/tools/file-share` 路由之后追加：

```ts
  {
    path: '/tools/disk-cache-cleanup',
    component: () => import('../pages/DiskCacheCleanupPage.vue'),
  },
```

- [ ] **Step 5: Commit（页面文件下一步创建，先提交其余改动会编译失败，故延后到 Task 10 完成后一起验证）**

不在此任务单独 commit；留到 Task 10 末尾统一验证 + 提交。

---

## Task 10: 创建 DiskCacheCleanupPage.vue 页面骨架与接入 IP 输入区

**Files:**
- Create: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 创建页面骨架（仅接入 IP + 超时 + 整体壳）**

Create `src/pages/DiskCacheCleanupPage.vue` with:

```vue
<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  HardDrive,
  Loader,
  RefreshCw,
  Server,
  Clock,
  AlertCircle,
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Database,
  Trash2,
  X as XIcon,
} from 'lucide-vue-next';
import {
  getConfig,
  saveConfig,
  diskCleanupListServers,
  diskCleanupListDisks,
  diskCleanupCheckRedis,
  diskCleanupDeleteCache,
  type AppConfig,
  type DiskServerItem,
  type DiskInfoItem,
} from '../lib/tauri';

defineOptions({ name: 'DiskCacheCleanupPage' });

const { t } = useI18n();

const RECENT_KEY = 'diskCacheCleanup.recentHosts';
const MAX_RECENT = 10;

const config = ref<AppConfig | null>(null);
const timeoutSecs = ref<number>(5);

const hostIp = ref<string>('');
const suggestOpen = ref<boolean>(false);
const recentHosts = ref<string[]>([]);
const savedSshHosts = computed(() =>
  (config.value?.servers ?? [])
    .filter((s) => s.enabled)
    .map((s) => s.host)
    .filter((h, i, a) => h && a.indexOf(h) === i),
);

const fetching = ref<boolean>(false);
const httpError = ref<string | null>(null);

const serverList = ref<DiskServerItem[]>([]);
const pickedServerIp = ref<string>('');

const disks = ref<DiskInfoItem[]>([]);
const disksLoading = ref<boolean>(false);
const expandedIds = ref<Set<string>>(new Set());

const redisAvailable = ref<boolean>(true);
const redisError = ref<string | null>(null);
const presentCacheIds = ref<Set<string>>(new Set());

const cleaningIds = ref<Set<string>>(new Set());
const batchCleaning = ref<boolean>(false);
const confirmBatchOpen = ref<boolean>(false);

// ── mount: load config + recent hosts ──────────────────

onMounted(async () => {
  try {
    config.value = await getConfig();
    timeoutSecs.value = config.value.disk_cleanup_http_timeout_secs ?? 5;
  } catch (e) {
    console.error('Failed to load config', e);
  }
  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_KEY });
    if (Array.isArray(saved)) {
      recentHosts.value = saved.slice(0, MAX_RECENT);
    }
  } catch {
    /* ignore */
  }
});

// ── recent hosts LRU ──────────────────

async function pushRecentHost(ip: string) {
  const trimmed = ip.trim();
  if (!trimmed) return;
  const next = [trimmed, ...recentHosts.value.filter((x) => x !== trimmed)].slice(0, MAX_RECENT);
  recentHosts.value = next;
  try {
    await invoke('save_kv', { key: RECENT_KEY, value: next });
  } catch {
    /* ignore */
  }
}

async function removeRecentHost(ip: string) {
  recentHosts.value = recentHosts.value.filter((x) => x !== ip);
  try {
    await invoke('save_kv', { key: RECENT_KEY, value: recentHosts.value });
  } catch {
    /* ignore */
  }
}

async function clearRecentHosts() {
  recentHosts.value = [];
  try {
    await invoke('save_kv', { key: RECENT_KEY, value: [] });
  } catch {
    /* ignore */
  }
}

// ── timeout persistence ──────────────────

async function saveTimeout() {
  if (!config.value) return;
  config.value.disk_cleanup_http_timeout_secs = timeoutSecs.value;
  try {
    await saveConfig(config.value);
  } catch (e) {
    console.error('Failed to save timeout', e);
  }
}

// ── fetch server list ──────────────────

async function fetchServers() {
  const ip = hostIp.value.trim();
  if (!ip) {
    httpError.value = t('diskCacheCleanup.errors.hostEmpty');
    return;
  }
  httpError.value = null;
  fetching.value = true;
  serverList.value = [];
  pickedServerIp.value = '';
  disks.value = [];
  try {
    const list = await diskCleanupListServers(ip, timeoutSecs.value);
    serverList.value = list;
    await pushRecentHost(ip);
    if (list.length > 0) {
      pickedServerIp.value = list[0].serverIp;
      await fetchDisksFor(list[0].serverIp);
    }
  } catch (e: unknown) {
    httpError.value = t('diskCacheCleanup.errors.http', {
      reason: typeof e === 'string' ? e : String(e),
    });
  } finally {
    fetching.value = false;
  }
}

// ── placeholders: implemented in later tasks ──────────

async function fetchDisksFor(serverIp: string) {
  void serverIp;
  // implemented in Task 11
}

function pickSuggestion(ip: string) {
  hostIp.value = ip;
  suggestOpen.value = false;
}

// ── close suggest dropdown on outside click ──────────

const inputBoxRef = ref<HTMLDivElement | null>(null);
function onDocClick(ev: MouseEvent) {
  if (!inputBoxRef.value) return;
  if (!inputBoxRef.value.contains(ev.target as Node)) {
    suggestOpen.value = false;
  }
}
onMounted(() => document.addEventListener('mousedown', onDocClick));
onUnmounted(() => document.removeEventListener('mousedown', onDocClick));
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-50 to-slate-100 overflow-y-auto">
    <div class="max-w-6xl w-full mx-auto p-6 pb-10 space-y-5">
      <!-- Header -->
      <div class="flex items-center gap-3">
        <div class="w-10 h-10 rounded-xl bg-gradient-to-br from-rose-500 to-orange-600 flex items-center justify-center shadow-sm">
          <HardDrive class="w-5 h-5 text-white" />
        </div>
        <div>
          <h1 class="text-2xl font-bold text-slate-900">{{ t('diskCacheCleanup.title') }}</h1>
          <p class="text-sm text-slate-500">{{ t('diskCacheCleanup.description') }}</p>
        </div>
      </div>

      <!-- A. Host IP input + fetch button -->
      <div class="bg-white border border-slate-200/80 rounded-xl shadow-sm p-5">
        <label class="text-sm font-semibold text-slate-700">{{ t('diskCacheCleanup.hostIp.label') }}</label>
        <div ref="inputBoxRef" class="relative mt-2 flex items-center gap-3">
          <div class="relative flex-1">
            <input
              v-model="hostIp"
              type="text"
              :placeholder="t('diskCacheCleanup.hostIp.placeholder')"
              class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-rose-400/40 focus:border-rose-400"
              @focus="suggestOpen = true"
              @keyup.enter="fetchServers"
            />
            <!-- suggest dropdown -->
            <div
              v-if="suggestOpen && (recentHosts.length || savedSshHosts.length)"
              class="absolute z-20 mt-1 w-full max-h-64 overflow-auto rounded-lg border border-slate-200 bg-white shadow-lg"
            >
              <div v-if="recentHosts.length" class="px-3 pt-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-slate-400">
                {{ t('diskCacheCleanup.hostIp.recentGroup') }}
              </div>
              <button
                v-for="ip in recentHosts"
                :key="`r-${ip}`"
                type="button"
                class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-rose-50"
                @click="pickSuggestion(ip)"
              >
                <Clock class="h-3.5 w-3.5 text-slate-400" />
                <span class="flex-1 truncate">{{ ip }}</span>
                <XIcon
                  class="h-3.5 w-3.5 text-slate-300 hover:text-rose-500"
                  @click.stop="removeRecentHost(ip)"
                />
              </button>
              <div v-if="savedSshHosts.length" class="px-3 pt-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-slate-400">
                {{ t('diskCacheCleanup.hostIp.serversGroup') }}
              </div>
              <button
                v-for="ip in savedSshHosts"
                :key="`s-${ip}`"
                type="button"
                class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-slate-50"
                @click="pickSuggestion(ip)"
              >
                <Server class="h-3.5 w-3.5 text-slate-400" />
                <span class="flex-1 truncate">{{ ip }}</span>
              </button>
            </div>
          </div>
          <button
            type="button"
            class="inline-flex items-center gap-2 rounded-lg bg-rose-500 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-rose-600 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="fetching"
            @click="fetchServers"
          >
            <Loader v-if="fetching" class="h-4 w-4 animate-spin" />
            <HardDrive v-else class="h-4 w-4" />
            {{ fetching ? t('diskCacheCleanup.actions.fetching') : t('diskCacheCleanup.actions.fetch') }}
          </button>
        </div>
      </div>

      <!-- Error banner (HTTP) -->
      <div v-if="httpError" class="flex items-start gap-2 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
        <AlertCircle class="mt-0.5 h-4 w-4 flex-shrink-0" />
        <span>{{ httpError }}</span>
      </div>

      <!-- Timeout footer -->
      <div class="flex items-center justify-end gap-3 pt-2 text-sm text-slate-500">
        <label>{{ t('diskCacheCleanup.timeout.label') }}</label>
        <select
          v-model.number="timeoutSecs"
          class="rounded-md border border-slate-300 bg-white px-2 py-1 text-sm text-slate-700 focus:outline-none focus:ring-2 focus:ring-rose-400/40"
          @change="saveTimeout"
        >
          <option :value="1">1</option>
          <option :value="2">2</option>
          <option :value="3">3</option>
          <option :value="5">5</option>
          <option :value="10">10</option>
          <option :value="15">15</option>
          <option :value="30">30</option>
        </select>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 2: 构建前端类型检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 3: Commit（含 Task 9 集成改动）**

```bash
git add src/lib/sidebarNavigation.ts src/components/Sidebar.vue src/pages/ToolsHubPage.vue src/router/index.ts src/pages/DiskCacheCleanupPage.vue
git commit -m "feat(disk-cleanup): 新增页面骨架、侧边栏、ToolsHub 卡片与路由"
```

---

## Task 11: 服务器下拉 + 自动加载硬盘列表

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 替换 `fetchDisksFor` 实现并补一个刷新服务器方法**

Edit `src/pages/DiskCacheCleanupPage.vue`。把占位 `fetchDisksFor` 替换为：

```ts
async function fetchDisksFor(serverIp: string) {
  if (!serverIp || !hostIp.value.trim()) return;
  disksLoading.value = true;
  disks.value = [];
  presentCacheIds.value = new Set();
  redisError.value = null;
  redisAvailable.value = true;
  try {
    const list = await diskCleanupListDisks(hostIp.value.trim(), serverIp, timeoutSecs.value);
    disks.value = list;
    if (list.length > 0) {
      const ids = list.map((d) => d.storageId);
      const check = await diskCleanupCheckRedis(hostIp.value.trim(), ids);
      redisAvailable.value = check.redis_available;
      redisError.value = check.error;
      presentCacheIds.value = new Set(check.present_ids);
    }
  } catch (e: unknown) {
    httpError.value = t('diskCacheCleanup.errors.http', {
      reason: typeof e === 'string' ? e : String(e),
    });
  } finally {
    disksLoading.value = false;
  }
}

async function refreshServers() {
  if (!hostIp.value.trim()) return;
  await fetchServers();
}

function onPickServer(ip: string) {
  pickedServerIp.value = ip;
  void fetchDisksFor(ip);
}
```

- [ ] **Step 2: 在 template 中添加 B（服务器下拉）与 C（Redis 横幅）段落**

在"Error banner (HTTP)" div 之后追加：

```vue
      <!-- B. Server picker -->
      <div v-if="serverList.length" class="bg-white border border-slate-200/80 rounded-xl shadow-sm p-5">
        <div class="flex items-center gap-3">
          <label class="text-sm font-semibold text-slate-700 shrink-0">{{ t('diskCacheCleanup.server.pick') }}</label>
          <select
            :value="pickedServerIp"
            @change="onPickServer(($event.target as HTMLSelectElement).value)"
            class="flex-1 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-rose-400/40"
          >
            <option v-for="s in serverList" :key="s.serverIp" :value="s.serverIp">
              {{ s.serverName }} · {{ s.serverIp }} · {{ s.role }}
            </option>
          </select>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-50"
            @click="refreshServers"
          >
            <RefreshCw class="h-3.5 w-3.5" />
            {{ t('diskCacheCleanup.server.refresh') }}
          </button>
        </div>
      </div>

      <!-- C. Redis banner -->
      <div
        v-if="redisError && !redisAvailable"
        class="flex items-start gap-2 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
      >
        <AlertTriangle class="mt-0.5 h-4 w-4 flex-shrink-0" />
        <span>{{ t('diskCacheCleanup.errors.redis', { reason: redisError }) }}</span>
      </div>
```

- [ ] **Step 3: vue-tsc 检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 4: Commit**

```bash
git add src/pages/DiskCacheCleanupPage.vue
git commit -m "feat(disk-cleanup): 子机下拉 + 硬盘列表自动加载 + Redis 横幅"
```

---

## Task 12: 硬盘表格（可展开行 + 状态/用途徽章）

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 添加徽章样式助手函数**

Edit `src/pages/DiskCacheCleanupPage.vue`。在 `<script setup>` 末尾（`onUnmounted` 之前）追加：

```ts
const STATUS_GREEN = new Set([1, 13]);
const STATUS_BLUE = new Set([4, 7, 8, 9, 10, 11, 12, 16, 20]);
const STATUS_AMBER = new Set([5, 14, 19, 21, 22]);
const STATUS_RED = new Set([2, 3, 6, 15, 17, 18, 23]);

function statusBadgeClass(code: number): string {
  if (STATUS_GREEN.has(code)) return 'bg-emerald-50 text-emerald-700 border-emerald-200';
  if (STATUS_BLUE.has(code)) return 'bg-blue-50 text-blue-700 border-blue-200';
  if (STATUS_AMBER.has(code)) return 'bg-amber-50 text-amber-700 border-amber-200';
  if (STATUS_RED.has(code)) return 'bg-red-50 text-red-700 border-red-200';
  return 'bg-slate-50 text-slate-600 border-slate-200';
}

function statusIsLoading(code: number): boolean {
  return STATUS_BLUE.has(code);
}

function usageBadgeClass(code: number): string {
  switch (code) {
    case 1:
      return 'border border-blue-400 text-blue-600';
    case 2:
      return 'border border-emerald-400 text-emerald-600';
    case 3:
      return 'border border-cyan-400 text-cyan-600';
    case 4:
      return 'bg-red-500 text-white border border-red-500';
    case 5:
      return 'border border-red-400 text-red-500';
    case 255:
      return 'border border-slate-300 text-slate-500';
    default:
      return '';
  }
}

function formatCapacity(gb: number): string {
  if (gb >= 1024) return `${(gb / 1024).toFixed(2)} TB`;
  return `${gb} GB`;
}

function toggleExpand(storageId: string) {
  const next = new Set(expandedIds.value);
  if (next.has(storageId)) next.delete(storageId);
  else next.add(storageId);
  expandedIds.value = next;
}

const cleanableIds = computed(() =>
  disks.value.filter((d) => presentCacheIds.value.has(d.storageId)).map((d) => d.storageId),
);
```

- [ ] **Step 2: 在 template 里添加 D（硬盘表格）段落**

在 `Redis banner` 之后追加：

```vue
      <!-- D. Disk table -->
      <div v-if="disksLoading || disks.length || pickedServerIp" class="bg-white border border-slate-200/80 rounded-xl shadow-sm overflow-hidden">
        <!-- Toolbar -->
        <div class="flex items-center justify-between px-5 py-3 border-b border-slate-200 bg-slate-50/60">
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm text-slate-700 hover:bg-slate-50"
              :disabled="disksLoading"
              @click="fetchDisksFor(pickedServerIp)"
            >
              <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': disksLoading }" />
              {{ t('diskCacheCleanup.actions.refresh') }}
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-semibold text-white transition-colors"
              :class="[
                !redisAvailable || cleanableIds.length === 0 || batchCleaning
                  ? 'bg-slate-300 cursor-not-allowed'
                  : 'bg-rose-500 hover:bg-rose-600',
              ]"
              :title="!redisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
              :disabled="!redisAvailable || cleanableIds.length === 0 || batchCleaning"
              @click="confirmBatchOpen = true"
            >
              <Trash2 class="h-3.5 w-3.5" />
              {{ t('diskCacheCleanup.actions.cleanAll', { count: cleanableIds.length }) }}
            </button>
          </div>
          <div class="text-xs text-slate-400 font-mono">{{ pickedServerIp }}</div>
        </div>

        <!-- Empty / loading -->
        <div v-if="disksLoading" class="py-8 text-center text-sm text-slate-400">
          <Loader class="mx-auto mb-2 h-5 w-5 animate-spin text-slate-300" />
          ...
        </div>
        <div v-else-if="disks.length === 0" class="py-10 text-center text-sm text-slate-400">
          {{ t('diskCacheCleanup.disks.empty') }}
        </div>

        <!-- Table -->
        <table v-else class="w-full text-sm">
          <thead>
            <tr class="bg-slate-50 text-[11px] uppercase tracking-wider text-slate-500">
              <th class="w-8 px-3 py-2"></th>
              <th class="w-12 px-3 py-2 text-left">{{ t('diskCacheCleanup.disks.columns.slot') }}</th>
              <th class="px-3 py-2 text-left">{{ t('diskCacheCleanup.disks.columns.device') }}</th>
              <th class="px-3 py-2 text-right">{{ t('diskCacheCleanup.disks.columns.capacity') }}</th>
              <th class="px-3 py-2 text-left">{{ t('diskCacheCleanup.disks.columns.usage') }}</th>
              <th class="px-3 py-2 text-left">{{ t('diskCacheCleanup.disks.columns.status') }}</th>
              <th class="px-3 py-2 text-left">{{ t('diskCacheCleanup.disks.columns.cache') }}</th>
              <th class="w-32 px-3 py-2 text-right">{{ t('diskCacheCleanup.disks.columns.actions') }}</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="d in disks" :key="d.storageId">
              <tr class="border-t border-slate-100 hover:bg-slate-50/50">
                <td class="px-3 py-2">
                  <button
                    type="button"
                    class="flex h-5 w-5 items-center justify-center rounded border border-slate-200 text-slate-500 hover:bg-slate-100"
                    @click="toggleExpand(d.storageId)"
                  >
                    <ChevronDown v-if="expandedIds.has(d.storageId)" class="h-3 w-3" />
                    <ChevronRight v-else class="h-3 w-3" />
                  </button>
                </td>
                <td class="px-3 py-2 font-semibold text-slate-800">{{ d.slot }}</td>
                <td class="px-3 py-2 font-mono text-xs text-slate-600">{{ d.deviceName }}</td>
                <td class="px-3 py-2 text-right tabular-nums">{{ formatCapacity(d.totalCapacity) }}</td>
                <td class="px-3 py-2">
                  <span
                    v-if="d.usage !== -1"
                    class="inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-semibold"
                    :class="usageBadgeClass(d.usage)"
                  >
                    {{ t(`diskCacheCleanup.usage.${d.usage}`) }}
                  </span>
                </td>
                <td class="px-3 py-2">
                  <span
                    class="inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] font-semibold"
                    :class="statusBadgeClass(d.storageStatus)"
                  >
                    <span
                      class="h-1.5 w-1.5 rounded-full bg-current"
                      :class="{ 'animate-pulse': statusIsLoading(d.storageStatus) }"
                    ></span>
                    {{ t(`diskCacheCleanup.status.${d.storageStatus}`) }}
                  </span>
                </td>
                <td class="px-3 py-2">
                  <span
                    v-if="presentCacheIds.has(d.storageId)"
                    class="inline-flex items-center gap-1 text-[11px] font-semibold text-indigo-600"
                  >
                    <Database class="h-3 w-3" />
                    Redis
                  </span>
                  <span v-else class="text-xs text-slate-300">—</span>
                </td>
                <td class="px-3 py-2 text-right">
                  <!-- clean button implemented in Task 13 -->
                </td>
              </tr>
              <tr v-if="expandedIds.has(d.storageId)" class="bg-slate-50/60">
                <td></td>
                <td colspan="7" class="px-3 py-2 text-xs text-slate-600">
                  <div>
                    <span class="font-semibold">{{ t('diskCacheCleanup.disks.storageId') }}:</span>
                    <span class="font-mono ml-1">{{ d.storageId }}</span>
                    <span class="ml-4 font-semibold">{{ t('diskCacheCleanup.disks.enclosure') }}:</span>
                    <span class="ml-1">{{ d.enclosureIndex }}</span>
                    <span class="ml-4 font-semibold">Type:</span>
                    <span class="ml-1">{{ d.storageType }}</span>
                  </div>
                  <div class="mt-1">
                    <span class="font-semibold">{{ t('diskCacheCleanup.disks.wwn') }}:</span>
                    <div class="font-mono text-[11px] text-slate-500 mt-0.5">
                      <div v-for="w in d.worldWideNameList" :key="w.wwn">
                        {{ w.wwn }} ({{ w.blockSize }})
                      </div>
                    </div>
                  </div>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
      </div>
```

- [ ] **Step 2: vue-tsc 检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 3: Commit**

```bash
git add src/pages/DiskCacheCleanupPage.vue
git commit -m "feat(disk-cleanup): 硬盘可展开表格 + 状态与用途徽章"
```

---

## Task 13: 清理按钮（单条 + 批量）与二次确认

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 添加清理函数**

Edit `src/pages/DiskCacheCleanupPage.vue`。在 `cleanableIds` computed 之后追加：

```ts
async function cleanOne(storageId: string) {
  if (!redisAvailable.value) return;
  if (cleaningIds.value.has(storageId)) return;
  const next = new Set(cleaningIds.value);
  next.add(storageId);
  cleaningIds.value = next;
  try {
    const res = await diskCleanupDeleteCache(hostIp.value.trim(), [storageId]);
    if (!res.redis_available || res.error) {
      httpError.value = t('diskCacheCleanup.errors.deleteSingle', {
        reason: res.error ?? 'unknown',
      });
      return;
    }
    if (pickedServerIp.value) {
      await fetchDisksFor(pickedServerIp.value);
    }
  } catch (e: unknown) {
    httpError.value = t('diskCacheCleanup.errors.deleteSingle', {
      reason: typeof e === 'string' ? e : String(e),
    });
  } finally {
    const done = new Set(cleaningIds.value);
    done.delete(storageId);
    cleaningIds.value = done;
  }
}

async function cleanBatch() {
  confirmBatchOpen.value = false;
  const ids = cleanableIds.value;
  if (!ids.length || !redisAvailable.value) return;
  batchCleaning.value = true;
  try {
    const res = await diskCleanupDeleteCache(hostIp.value.trim(), ids);
    if (!res.redis_available || res.error) {
      httpError.value = t('diskCacheCleanup.errors.deleteBatch', {
        reason: res.error ?? 'unknown',
      });
      return;
    }
    if (pickedServerIp.value) {
      await fetchDisksFor(pickedServerIp.value);
    }
  } catch (e: unknown) {
    httpError.value = t('diskCacheCleanup.errors.deleteBatch', {
      reason: typeof e === 'string' ? e : String(e),
    });
  } finally {
    batchCleaning.value = false;
  }
}
```

- [ ] **Step 2: 替换操作列占位为真实按钮**

在 template 中找到 `<!-- clean button implemented in Task 13 -->` 行，替换为：

```vue
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-[11px] font-semibold text-white transition-colors"
                    :class="[
                      !redisAvailable || !presentCacheIds.has(d.storageId) || cleaningIds.has(d.storageId)
                        ? 'bg-slate-300 cursor-not-allowed'
                        : 'bg-rose-500 hover:bg-rose-600',
                    ]"
                    :title="
                      !redisAvailable
                        ? t('diskCacheCleanup.disabled.redisDown')
                        : !presentCacheIds.has(d.storageId)
                        ? undefined
                        : undefined
                    "
                    :disabled="!redisAvailable || !presentCacheIds.has(d.storageId) || cleaningIds.has(d.storageId)"
                    @click="cleanOne(d.storageId)"
                  >
                    <Loader v-if="cleaningIds.has(d.storageId)" class="h-3 w-3 animate-spin" />
                    <Trash2 v-else class="h-3 w-3" />
                    {{ t('diskCacheCleanup.actions.cleanOne') }}
                  </button>
```

- [ ] **Step 3: 在页面根部（最外层 div 前）添加批量确认模态**

在模板根 `<div>` 结束之前追加：

```vue
      <!-- Batch confirm modal -->
      <div
        v-if="confirmBatchOpen"
        class="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/40 backdrop-blur-sm"
        @click.self="confirmBatchOpen = false"
      >
        <div class="w-full max-w-sm rounded-2xl border border-slate-200 bg-white p-6 shadow-xl">
          <div class="flex items-start gap-3">
            <div class="flex h-10 w-10 items-center justify-center rounded-full bg-rose-100 text-rose-600">
              <Trash2 class="h-5 w-5" />
            </div>
            <div class="flex-1">
              <div class="text-base font-semibold text-slate-900">
                {{ t('diskCacheCleanup.actions.cleanAll', { count: cleanableIds.length }) }}
              </div>
              <div class="mt-1 text-sm text-slate-500">
                {{ t('diskCacheCleanup.actions.cleanAllConfirm', { count: cleanableIds.length }) }}
              </div>
            </div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <button
              type="button"
              class="rounded-lg border border-slate-200 bg-white px-4 py-2 text-sm text-slate-700 hover:bg-slate-50"
              @click="confirmBatchOpen = false"
            >
              {{ t('diskCacheCleanup.actions.cancel') }}
            </button>
            <button
              type="button"
              class="rounded-lg bg-rose-500 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-600"
              @click="cleanBatch"
            >
              {{ t('diskCacheCleanup.actions.confirm') }}
            </button>
          </div>
        </div>
      </div>
```

- [ ] **Step 4: vue-tsc 检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 5: Commit**

```bash
git add src/pages/DiskCacheCleanupPage.vue
git commit -m "feat(disk-cleanup): 单条/批量清理按钮与二次确认"
```

---

## Task 14: EnableApplianceSshPage 最近使用 Chip 行

**Files:**
- Modify: `src/pages/EnableApplianceSshPage.vue`

- [ ] **Step 1: 引入 KV wrapper 与响应式状态**

Edit `src/pages/EnableApplianceSshPage.vue`。在 `import { ref, computed, onMounted, nextTick }` 一行确认含 `onMounted`（已有）。在 `import { enableApplianceSsh, getConfig, saveConfig, type AppConfig, ... }` 下方增加：

```ts
import { invoke } from '@tauri-apps/api/core';
```

在 `manualIpInput = ref<string>('')` 附近新增：

```ts
const RECENT_IPS_KEY = 'applianceSsh.recentIps';
const MAX_RECENT_IPS = 10;
const recentIps = ref<string[]>([]);
```

- [ ] **Step 2: onMounted 加载、提交后写回**

在 `onMounted(async () => { ... })` 钩子里添加（紧接在 `apiTimeoutSecs.value = ...` 之后）：

```ts
  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_IPS_KEY });
    if (Array.isArray(saved)) recentIps.value = saved.slice(0, MAX_RECENT_IPS);
  } catch {
    /* ignore */
  }
```

在真正触发 `enableApplianceSsh(...)` 提交的函数入口（用 Grep 定位 `enableApplianceSsh(` 调用点）内，提交**之前**插入：

```ts
  const tagsSnapshot = [...manualIpTags.value];
  if (tagsSnapshot.length) {
    const next = [...tagsSnapshot, ...recentIps.value]
      .filter((v, i, a) => v && a.indexOf(v) === i)
      .slice(0, MAX_RECENT_IPS);
    recentIps.value = next;
    invoke('save_kv', { key: RECENT_IPS_KEY, value: next }).catch(() => undefined);
  }
```

- [ ] **Step 3: 添加 Chip 行 template**

在 `manualIpInput` 输入框所在的 `<div>` 底部添加：

```vue
        <div v-if="recentIps.length" class="mt-2 flex flex-wrap items-center gap-1.5">
          <span class="text-[11px] font-semibold text-slate-500">{{ t('applianceSsh.recentIps.label') }}:</span>
          <button
            v-for="ip in recentIps"
            :key="`r-${ip}`"
            type="button"
            class="group inline-flex items-center gap-1 rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5 text-xs text-slate-600 hover:border-slate-300 hover:bg-slate-100"
            @click="addManualIpTag(ip)"
          >
            {{ ip }}
            <XIcon
              class="h-3 w-3 text-slate-400 hover:text-red-500"
              @click.stop="removeRecentIp(ip)"
            />
          </button>
          <button
            type="button"
            class="text-[11px] text-slate-400 underline hover:text-slate-600"
            @click="clearRecentIps"
          >
            {{ t('applianceSsh.recentIps.clear') }}
          </button>
        </div>
```

- [ ] **Step 4: 添加 remove / clear 函数**

在 `<script setup>` 的合适位置添加：

```ts
function removeRecentIp(ip: string) {
  recentIps.value = recentIps.value.filter((x) => x !== ip);
  invoke('save_kv', { key: RECENT_IPS_KEY, value: recentIps.value }).catch(() => undefined);
}

function clearRecentIps() {
  recentIps.value = [];
  invoke('save_kv', { key: RECENT_IPS_KEY, value: [] }).catch(() => undefined);
}
```

注意：`XIcon` 已在 lucide import 中（参见文件现有 import）。若缺则追加。

- [ ] **Step 5: vue-tsc 检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 6: Commit**

```bash
git add src/pages/EnableApplianceSshPage.vue
git commit -m "feat(appliance-ssh): 最近使用 IP 持久化与 Chip 行"
```

---

## Task 15: FrameworkPasswordPage 最近使用 Chip 行

**Files:**
- Modify: `src/pages/FrameworkPasswordPage.vue`

- [ ] **Step 1: 复制 Task 14 的模式到 FrameworkPasswordPage**

Edit `src/pages/FrameworkPasswordPage.vue` 进行等价改动：

- KV 常量：
  ```ts
  const RECENT_IPS_KEY = 'frameworkPassword.recentIps';
  const MAX_RECENT_IPS = 10;
  const recentIps = ref<string[]>([]);
  ```
- `onMounted` 内同样的 `load_kv` 逻辑
- `changeFrameworkPassword(...)` 调用**之前**插入合并 snapshot 并 `save_kv`
- 在 `manualIpInput` 输入区下方添加等价 Chip 行，i18n 键使用 `frameworkPassword.recentIps.{label,clear}`
- 加上 `removeRecentIp` / `clearRecentIps` 函数
- 确保从 `lucide-vue-next` 中 import `X as XIcon`（检查文件；若已有 `X`，用 `X as XIcon` 或直接 `X`）

- [ ] **Step 2: vue-tsc 检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 3: Commit**

```bash
git add src/pages/FrameworkPasswordPage.vue
git commit -m "feat(framework-password): 最近使用 IP 持久化与 Chip 行"
```

---

## Task 16: PingScanTab 最近网段前缀 Chip 行

**Files:**
- Modify: `src/components/network/PingScanTab.vue`

- [ ] **Step 1: 添加最近前缀状态与持久化**

Edit `src/components/network/PingScanTab.vue`。在 script 顶部 `const KV_KEY = 'networkTools.pingScanConfig';` 附近新增：

```ts
const RECENT_PREFIXES_KEY = 'networkTools.pingScan.recentPrefixes';
const MAX_RECENT_PREFIXES = 10;
const recentPrefixes = ref<string[]>([]);
```

在 `onMounted(async () => { ... })` 钩子末尾追加：

```ts
  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_PREFIXES_KEY });
    if (Array.isArray(saved)) recentPrefixes.value = saved.slice(0, MAX_RECENT_PREFIXES);
  } catch {
    /* ignore */
  }
```

找到扫描启动函数（搜 `pingScan(` 调用点），在成功发起调用**之前**追加：

```ts
  const p = prefix.value.trim();
  if (p) {
    const next = [p, ...recentPrefixes.value.filter((x) => x !== p)].slice(0, MAX_RECENT_PREFIXES);
    recentPrefixes.value = next;
    invoke('save_kv', { key: RECENT_PREFIXES_KEY, value: next }).catch(() => undefined);
  }
```

添加 remove / clear 函数：

```ts
function removePrefix(p: string) {
  recentPrefixes.value = recentPrefixes.value.filter((x) => x !== p);
  invoke('save_kv', { key: RECENT_PREFIXES_KEY, value: recentPrefixes.value }).catch(() => undefined);
}

function clearRecentPrefixes() {
  recentPrefixes.value = [];
  invoke('save_kv', { key: RECENT_PREFIXES_KEY, value: [] }).catch(() => undefined);
}
```

- [ ] **Step 2: template 中的 prefix 输入框下方追加 Chip 行**

紧跟 `prefixError` 错误提示之后插入：

```vue
        <div v-if="recentPrefixes.length" class="mt-1 flex flex-wrap items-center gap-1.5">
          <span class="text-[11px] font-semibold text-slate-500">{{ t('networkTools.ping.recentPrefixes.label') }}:</span>
          <button
            v-for="p in recentPrefixes"
            :key="`rp-${p}`"
            type="button"
            class="inline-flex items-center gap-1 rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5 text-xs text-slate-600 hover:border-blue-300 hover:bg-blue-50"
            @click="prefix = p"
          >
            {{ p }}
            <span
              class="text-slate-400 hover:text-red-500"
              @click.stop="removePrefix(p)"
            >×</span>
          </button>
          <button
            type="button"
            class="text-[11px] text-slate-400 underline hover:text-slate-600"
            @click="clearRecentPrefixes"
          >
            {{ t('networkTools.ping.recentPrefixes.clear') }}
          </button>
        </div>
```

- [ ] **Step 3: vue-tsc 检查**

Run: `pnpm vue-tsc --noEmit`
Expected: 无错误。

- [ ] **Step 4: Commit**

```bash
git add src/components/network/PingScanTab.vue
git commit -m "feat(network-tools): Ping 扫描最近网段前缀持久化与 Chip 行"
```

---

## Task 17: 集成构建与手测回归

**Files:** 无代码改动（仅构建 + 手测）。

- [ ] **Step 1: 运行完整构建**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: 成功构建，产物命名为 `file-sync-tool-1.0.6-YYYYMMDDHHmm.exe`（位于 `src-tauri/target/release`）。

- [ ] **Step 2: clippy 检查**

Run: `cd src-tauri && cargo clippy --release -- -D warnings`
Expected: 无新增警告 / 错误。若存在现有警告则至少确保本次引入的 `disk_cleanup.rs` 无 warning。

- [ ] **Step 3: 手测场景表**

逐项验证（由人工在打包产物中运行）：

1. 侧边栏出现「硬盘缓存清理」条目，点击打开页面无白屏。
2. 输入有效接入 IP → 点「获取硬盘列表」→ 成功显示 serverList 下拉 + 首个 server 的硬盘（16 块）。
3. 最近使用下拉：关闭再打开页面，IP 仍在，点击能直接填入。
4. 硬盘行可展开/收起，显示 storageId / WWN / 机箱。
5. 有 Redis 缓存的行显示 `Redis` 徽章 + 清理按钮；无缓存行灰色 "—"。
6. 点单条「清理缓存」→ 成功后重放 /disk/list + EXISTS → 该行徽章消失、按钮置灰。
7. 点「一键清理全部」→ 弹确认框 → 确定 → 全部缓存徽章消失。
8. HTTP 不通的 IP → 红色横幅，列表不出现。
9. Redis 端口被挡 / 密码错 → 琥珀横幅 + 列表正常 + 所有清理按钮置灰（hover tooltip 说明原因）。
10. 改超时为 1 秒 → 关闭页面再打开 → 下拉显示为 1。
11. 一体机 SSH 页：填 IP 标签并点击执行 → 回来能看到最近使用 Chip；× 删除、清空生效。
12. 框架密码页：同样验证。
13. Ping 网段扫描：填前缀 192.168.5 启动扫描 → 结束后该前缀进入最近列表 → 再次打开可点击填入。

- [ ] **Step 4: 升级验证**

用旧版本 `%APPDATA%\app\config\config.json`（缺 `disk_cleanup_http_timeout_secs` 字段）启动新二进制，验证：

- 正常加载，无报错
- 进入硬盘缓存清理页 → 超时下拉显示为 5（默认）
- 保存配置后回写字段完整

- [ ] **Step 5: Commit（若手测发现小问题随手修）**

若无改动则跳过；有改动则：

```bash
git add -A
git commit -m "chore(disk-cleanup): 手测回归修复"
```

---

## 参考信息

- Spec: [docs/superpowers/specs/2026-04-22-disk-cache-cleanup-design.md](../specs/2026-04-22-disk-cache-cleanup-design.md)
- 现有模式参考：
  - `src-tauri/src/main.rs:1815-1854`（HTTP client 构造与 JSON 响应解析）
  - `src/pages/EnableApplianceSshPage.vue:646`（API 请求超时下拉控件）
  - `src/components/network/PingScanTab.vue:14-46`（save_kv / load_kv 模式）
  - `src-tauri/src/persist.rs:44-66`（KV store 实现）
