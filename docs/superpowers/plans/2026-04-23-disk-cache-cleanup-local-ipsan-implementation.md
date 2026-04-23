# 硬盘缓存清理多源扩展（本地盘 Windows/Linux + IPSAN）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将现有仅支持 Linux 本地盘的“硬盘缓存清理”页面重构为统一入口 + 双区域工作台，同时支持 `Linux 本地盘`、`Windows 本地盘` 和 `IPSAN` 三类缓存状态查询与清理。

**Architecture:** Rust 后端将 `disk_cleanup.rs` 从“只认 storageId 的 Linux 本地盘实现”扩展为三类 HTTP 查询命令 + 两个 Redis key 通用命令；Vue 前端保留现有 `DiskCacheCleanupPage.vue` 作为单页面承载，但把状态拆成“共享输入态 + 本地盘区域态 + IPSAN 区域态”，并在本地盘区域内部引入 `Windows / Linux` 胶囊 Tab。

**Tech Stack:** Tauri 2.x · Rust 2021 · Tokio · reqwest · redis 0.25 · Vue 3 `<script setup>` · TypeScript · vue-i18n · Tailwind 4 · lucide-vue-next

**Spec:** [docs/superpowers/specs/2026-04-23-disk-cache-cleanup-local-ipsan-design.md](../specs/2026-04-23-disk-cache-cleanup-local-ipsan-design.md)

---

## File Map

- `src-tauri/src/disk_cleanup.rs` — 三类 HTTP 资源查询、Redis key 校验/查询/删除、单元测试
- `src-tauri/src/main.rs` — Tauri command 注册
- `src/lib/tauri.ts` — TypeScript 合同与 invoke wrapper
- `src/locales/messages.ts` — 新的顶部入口、本地盘 Tab、Windows 分区表、IPSAN 表格、区域级动作与错误文案
- `src/pages/DiskCacheCleanupPage.vue` — 顶部统一入口、本地盘区域、IPSAN 区域、分区域刷新和清理逻辑

## Task 1: 收敛后端命令边界并让项目重新编译

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/disk_cleanup.rs`
- Test: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 在 `main.rs` 先切换到新的 command 名称**

```rust
disk_cleanup::disk_cleanup_list_linux_servers,
disk_cleanup::disk_cleanup_list_linux_disks,
disk_cleanup::disk_cleanup_list_windows_disks,
disk_cleanup::disk_cleanup_list_ipsans,
disk_cleanup::disk_cleanup_check_cache_keys,
disk_cleanup::disk_cleanup_delete_cache_keys,
```

- [ ] **Step 2: 运行 Rust 测试，确认当前编译失败点就是新 command 尚未定义**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: FAIL，出现 `cannot find function` / `not found in module disk_cleanup` 之类的编译错误。

- [ ] **Step 3: 在 `disk_cleanup.rs` 补齐新的常量、数据结构和最小 command 占位实现**

```rust
const RAW_DISK_LIST_PATH: &str = "/openAPI/system/v1/raw-disk/list";
const IPSAN_LIST_PATH: &str = "/openAPI/system/v1/IPSAN/list";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowsPartitionItem {
    #[serde(rename = "partitionSeq")]
    pub partition_seq: i32,
    #[serde(rename = "partitionGUID")]
    pub partition_guid: String,
    #[serde(rename = "partitionOffset", default)]
    pub partition_offset: String,
    #[serde(default)]
    pub capacity: f64,
    #[serde(rename = "partitionStatus", default)]
    pub partition_status: i32,
    #[serde(default = "default_usage")]
    pub usage: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowsDiskItem {
    #[serde(rename = "diskId")]
    pub disk_id: String,
    #[serde(rename = "diskNumber", default)]
    pub disk_number: i32,
    #[serde(rename = "diskName", default)]
    pub disk_name: String,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: f64,
    #[serde(rename = "partitionList", default)]
    pub partition_list: Vec<WindowsPartitionItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpsanItem {
    #[serde(rename = "IPSANId")]
    pub ipsan_id: String,
    #[serde(rename = "IPSANName", default)]
    pub ipsan_name: String,
    #[serde(rename = "IPSANType", default)]
    pub ipsan_type: i32,
    #[serde(rename = "IPSANIp", default)]
    pub ipsan_ip: String,
    #[serde(rename = "IPSANStatus", default)]
    pub ipsan_status: i32,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: f64,
    #[serde(default = "default_usage")]
    pub usage: i32,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheKeyCheckResult {
    pub present_keys: Vec<String>,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheKeyDeleteResult {
    pub deleted_count: i64,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn disk_cleanup_list_linux_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    let _ = (host, timeout_secs);
    Err("not implemented".to_string())
}
```

- [ ] **Step 4: 继续补齐其余 5 个占位 command，让编译先恢复**

```rust
#[tauri::command]
pub async fn disk_cleanup_list_windows_disks(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<WindowsDiskItem>, String> {
    let _ = (host, timeout_secs);
    Err("not implemented".to_string())
}

#[tauri::command]
pub async fn disk_cleanup_check_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyCheckResult {
    let _ = (host, keys);
    CacheKeyCheckResult {
        present_keys: vec![],
        redis_available: false,
        error: Some("not implemented".to_string()),
    }
}
```

- [ ] **Step 5: 再跑一遍 Rust 测试，确认项目重新可编译**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: PASS（旧测试全部通过，新的占位 command 尚未进入功能测试）。

- [ ] **Step 6: 提交这一轮“命令边界收敛”**

```bash
git add src-tauri/src/main.rs src-tauri/src/disk_cleanup.rs
git commit -m "refactor(disk-cleanup): rename commands for multi-source cleanup"
```

## Task 2: 实现 Windows raw-disk 与 IPSAN 的 HTTP 查询合同

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`
- Test: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 先为 Windows 和 IPSAN 响应 envelope 写失败测试**

```rust
#[test]
fn parse_raw_disk_payload_returns_partition_list() {
    let body = r#"{
        "code": 0,
        "message": "Success",
        "data": {
            "diskInfoList": [
                {
                    "diskId": "302375165793144832",
                    "diskNumber": 6,
                    "diskName": "ST4000VX000-2AG166",
                    "totalCapacity": 3726.02,
                    "partitionList": [
                        {
                            "partitionSeq": 1,
                            "partitionGUID": "{6042cce1-3fa4-45a4-998d-57d44d6f8da1}",
                            "capacity": 976.56,
                            "partitionStatus": 1,
                            "usage": -1
                        }
                    ]
                }
            ]
        }
    }"#;

    let parsed = parse_api_payload::<WindowsRawDiskListData>(StatusCode::OK, body).unwrap();
    assert_eq!(parsed.disk_info_list[0].partition_list[0].partition_guid, "{6042cce1-3fa4-45a4-998d-57d44d6f8da1}");
}
```

```rust
#[test]
fn parse_ipsan_payload_returns_usage_field() {
    let body = r#"{
        "code": 0,
        "message": "Success",
        "data": {
            "IPSANInfoList": [
                {
                    "IPSANId": "436856425541537792",
                    "IPSANName": "192.115.2.29",
                    "IPSANIp": "192.115.2.29",
                    "IPSANStatus": 1,
                    "totalCapacity": 600,
                    "usage": 5
                }
            ]
        }
    }"#;

    let parsed = parse_api_payload::<IpsanListData>(StatusCode::OK, body).unwrap();
    assert_eq!(parsed.ipsan_info_list[0].usage, 5);
}
```

- [ ] **Step 2: 运行测试，确认当前失败点是缺少 data struct 和解析实现**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: FAIL，出现 `WindowsRawDiskListData` / `IpsanListData` 未定义或字段解析失败。

- [ ] **Step 3: 添加 raw-disk 与 IPSAN 的 data wrapper，并实现两个 HTTP command**

```rust
#[derive(Debug, Deserialize)]
struct WindowsRawDiskListData {
    #[serde(rename = "diskInfoList", default)]
    disk_info_list: Vec<WindowsDiskItem>,
}

#[derive(Debug, Deserialize)]
struct IpsanListData {
    #[serde(rename = "IPSANInfoList", default)]
    ipsan_info_list: Vec<IpsanItem>,
}

#[tauri::command]
pub async fn disk_cleanup_list_windows_disks(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<WindowsDiskItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, RAW_DISK_LIST_PATH);
    let data: WindowsRawDiskListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.disk_info_list)
}

#[tauri::command]
pub async fn disk_cleanup_list_ipsans(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<IpsanItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, IPSAN_LIST_PATH);
    let data: IpsanListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.ipsan_info_list)
}
```

- [ ] **Step 4: 把旧 Linux command 改名为新的 Linux command 名称**

```rust
#[tauri::command]
pub async fn disk_cleanup_list_linux_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, DISK_SERVER_LIST_PATH);
    let data: ServerListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.server_list)
}

#[tauri::command]
pub async fn disk_cleanup_list_linux_disks(
    host: String,
    server_ip: String,
    timeout_secs: u32,
) -> Result<Vec<DiskInfoItem>, String> {
    let host = normalize_host(&host)?;
    let server_ip = server_ip.trim().to_string();
    if server_ip.is_empty() {
        return Err("请选择子机 IP".to_string());
    }

    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, DISK_LIST_PATH);
    let data: DiskListData = post_json(
        &client,
        &url,
        serde_json::json!({ "serverIp": server_ip }),
    )
    .await?;
    Ok(data.storage_info_list)
}
```

- [ ] **Step 5: 重新跑 Rust 测试，确认 HTTP 查询合同通过**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: PASS，新增 raw-disk / IPSAN payload 测试通过。

- [ ] **Step 6: 提交这轮 HTTP 查询扩展**

```bash
git add src-tauri/src/disk_cleanup.rs
git commit -m "feat(disk-cleanup): add windows and IPSAN list commands"
```

## Task 3: 将 Redis 操作收敛为完整 key 的通用检查与删除

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`
- Test: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: 先写 key 规范化与校验测试**

```rust
#[test]
fn normalize_cache_keys_trims_dedupes_and_keeps_storage_prefix() {
    let keys = normalize_cache_keys(vec![
        " Storage:disk-a ".to_string(),
        "Storage:disk-a".to_string(),
        "Storage:disk-b".to_string(),
    ]).unwrap();

    assert_eq!(keys, vec!["Storage:disk-a".to_string(), "Storage:disk-b".to_string()]);
}

#[test]
fn normalize_cache_keys_rejects_non_storage_prefix() {
    let error = normalize_cache_keys(vec!["Partition:{foo}".to_string()]).unwrap_err();
    assert!(error.contains("Storage:"));
}
```

- [ ] **Step 2: 运行测试，确认当前 helper 缺失**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: FAIL，出现 `normalize_cache_keys` 未定义。

- [ ] **Step 3: 实现 key 规范化 helper，并保留旧的 `build_storage_key` 供后端测试复用**

```rust
fn normalize_cache_keys(keys: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw_key in keys {
        let key = raw_key.trim();
        if key.is_empty() || seen.contains(key) {
            continue;
        }
        if !key.starts_with(STORAGE_KEY_PREFIX) {
            return Err(format!("Redis key 必须以 {} 开头: {}", STORAGE_KEY_PREFIX, key));
        }
        seen.insert(key.to_string());
        normalized.push(key.to_string());
    }

    Ok(normalized)
}
```

- [ ] **Step 4: 用新 helper 替换 Redis command，并改成返回完整 key**

```rust
#[tauri::command]
pub async fn disk_cleanup_check_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyCheckResult {
    let host = match normalize_host(&host) {
        Ok(host) => host,
        Err(error) => {
            return CacheKeyCheckResult {
                present_keys: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let keys = match normalize_cache_keys(keys) {
        Ok(keys) => keys,
        Err(error) => {
            return CacheKeyCheckResult {
                present_keys: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    if keys.is_empty() {
        return CacheKeyCheckResult {
            present_keys: vec![],
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(conn) => conn,
        Err(error) => {
            return CacheKeyCheckResult {
                present_keys: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let mut pipe = redis::pipe();
    for key in &keys {
        pipe.cmd("EXISTS").arg(key);
    }

    let exec = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        pipe.query_async::<_, Vec<i64>>(&mut conn),
    )
    .await;

    match exec {
        Err(_) => CacheKeyCheckResult {
            present_keys: vec![],
            redis_available: false,
            error: Some("Redis 查询超时".to_string()),
        },
        Ok(Err(error)) => CacheKeyCheckResult {
            present_keys: vec![],
            redis_available: false,
            error: Some(format!("Redis EXISTS 失败: {}", error)),
        },
        Ok(Ok(flags)) => {
            let present_keys = keys
                .into_iter()
                .zip(flags.into_iter())
                .filter_map(|(key, flag)| if flag == 1 { Some(key) } else { None })
                .collect();
            CacheKeyCheckResult {
                present_keys,
                redis_available: true,
                error: None,
            }
        }
    }
}
```

```rust
#[tauri::command]
pub async fn disk_cleanup_delete_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyDeleteResult {
    let host = match normalize_host(&host) {
        Ok(host) => host,
        Err(error) => {
            return CacheKeyDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let keys = match normalize_cache_keys(keys) {
        Ok(keys) => keys,
        Err(error) => {
            return CacheKeyDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(error),
            };
        }
    };

    if keys.is_empty() {
        return CacheKeyDeleteResult {
            deleted_count: 0,
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(conn) => conn,
        Err(error) => {
            return CacheKeyDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let exec = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        redis::cmd("DEL").arg(&keys).query_async::<_, i64>(&mut conn),
    )
    .await;

    match exec {
        Err(_) => CacheKeyDeleteResult {
            deleted_count: 0,
            redis_available: false,
            error: Some("Redis 删除超时".to_string()),
        },
        Ok(Err(error)) => CacheKeyDeleteResult {
            deleted_count: 0,
            redis_available: false,
            error: Some(format!("Redis DEL 失败: {}", error)),
        },
        Ok(Ok(deleted_count)) => CacheKeyDeleteResult {
            deleted_count,
            redis_available: true,
            error: None,
        },
    }
}
```

- [ ] **Step 5: 跑 Rust 测试，确认 key 规则与 Redis no-op 分支都通过**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: PASS，新增 key 规则测试通过，旧的 Linux `Storage:{storageId}` helper 仍可复用。

- [ ] **Step 6: 提交这轮 Redis 通用化**

```bash
git add src-tauri/src/disk_cleanup.rs
git commit -m "feat(disk-cleanup): switch redis operations to validated cache keys"
```

## Task 4: 扩展 `src/lib/tauri.ts` 合同，先增加新 wrapper，不移除旧 wrapper

**Files:**
- Modify: `src/lib/tauri.ts`
- Test: `src/lib/tauri.ts`

- [ ] **Step 1: 在 `tauri.ts` 里新增 Windows / IPSAN 类型和新的 Redis result 类型**

```ts
export interface WindowsPartitionItem {
  partitionSeq: number;
  partitionGUID: string;
  partitionOffset: string;
  capacity: number;
  partitionStatus: number;
  usage: number;
}

export interface WindowsDiskItem {
  diskId: string;
  diskNumber: number;
  diskName: string;
  totalCapacity: number;
  partitionList: WindowsPartitionItem[];
}

export interface IpsanItem {
  IPSANId: string;
  IPSANName: string;
  IPSANType: number;
  IPSANIp: string;
  IPSANStatus: number;
  totalCapacity: number;
  usage: number;
}

export interface CacheKeyCheckResult {
  present_keys: string[];
  redis_available: boolean;
  error: string | null;
}
```

- [ ] **Step 2: 增加新的 invoke wrapper，暂时保留旧 wrapper 让页面在后续任务切换时保持增量迁移**

```ts
export async function diskCleanupListLinuxServers(host: string, timeoutSecs: number) {
  return await invoke<DiskServerItem[]>('disk_cleanup_list_linux_servers', { host, timeoutSecs });
}

export async function diskCleanupListWindowsDisks(host: string, timeoutSecs: number) {
  return await invoke<WindowsDiskItem[]>('disk_cleanup_list_windows_disks', { host, timeoutSecs });
}

export async function diskCleanupListIpsans(host: string, timeoutSecs: number) {
  return await invoke<IpsanItem[]>('disk_cleanup_list_ipsans', { host, timeoutSecs });
}

export async function diskCleanupCheckCacheKeys(host: string, keys: string[]) {
  return await invoke<CacheKeyCheckResult>('disk_cleanup_check_cache_keys', { host, keys });
}
```

- [ ] **Step 3: 跑 TS 类型检查，确认新增 wrapper 自己是干净的**

Run: `pnpm check`

Expected: PASS。此时页面仍然用旧 wrapper，不会因为功能未切换而报错。

- [ ] **Step 4: 提交前端 invoke 合同扩展**

```bash
git add src/lib/tauri.ts
git commit -m "feat(disk-cleanup): add frontend wrappers for windows and IPSAN"
```

## Task 5: 扩展 i18n 文案，先把新页面所需 key 补齐

**Files:**
- Modify: `src/locales/messages.ts`
- Test: `src/locales/messages.ts`

- [ ] **Step 1: 在英文与中文文案里补齐顶部入口、本地盘 Tab、Windows 分区表、IPSAN 表格和区域级动作**

```ts
diskCacheCleanup: {
  title: '硬盘缓存清理',
  description: '同时查看本地盘与 IPSAN 的 Redis Storage:* 缓存状态，并按区域或按行清理。',
  localDisk: {
    title: '本地盘',
    tabs: {
      windows: 'Windows 本地盘',
      linux: 'Linux 本地盘',
    },
    actions: {
      refresh: '刷新本地盘',
      cleanAll: '清理本地盘全部命中 ({count})',
    },
  },
  ipsan: {
    title: 'IPSAN',
    actions: {
      refresh: '刷新 IPSAN',
      cleanAll: '清理 IPSAN 全部命中 ({count})',
    },
    columns: {
      name: 'IPSAN',
      id: 'IPSANId',
      status: '状态',
      capacity: '总容量',
      usage: '用途',
      cache: '缓存',
      actions: '操作',
    },
  },
  windows: {
    diskHeader: '磁盘 {number} · {name}',
    columns: {
      partitionSeq: '分区',
      partitionGuid: 'Partition GUID',
      capacity: '容量',
      usage: '用途',
      status: '状态',
      cache: '缓存',
      actions: '操作',
    },
  },
}
```

- [ ] **Step 2: 补齐区域级错误与 Redis 告警文案**

```ts
errors: {
  localHttp: '本地盘查询失败：{reason}',
  ipsanHttp: 'IPSAN 查询失败：{reason}',
  localDelete: '本地盘缓存清理失败：{reason}',
  ipsanDelete: 'IPSAN 缓存清理失败：{reason}',
}

cache: {
  present: 'Storage 缓存存在',
  absent: '—',
  unavailable: 'Redis 不可用',
}
```

- [ ] **Step 3: 运行类型检查，确认 `messages.ts` 语法没有打断项目**

Run: `pnpm check`

Expected: PASS。

- [ ] **Step 4: 提交 i18n 扩展**

```bash
git add src/locales/messages.ts
git commit -m "feat(disk-cleanup): add i18n copy for windows and IPSAN views"
```

## Task 6: 重构 `DiskCacheCleanupPage.vue` 的状态模型和顶部统一入口，先让 Linux 路径迁移成功

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`
- Test: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 引入新的 wrapper，并把共享状态与区域状态拆开**

```ts
import {
  diskCleanupCheckCacheKeys,
  diskCleanupDeleteCacheKeys,
  diskCleanupListIpsans,
  diskCleanupListLinuxDisks,
  diskCleanupListLinuxServers,
  diskCleanupListWindowsDisks,
  type CacheKeyCheckResult,
  type IpsanItem,
  type WindowsDiskItem,
} from '../lib/tauri';

type LocalDiskTab = 'windows' | 'linux';

const localDiskTab = ref<LocalDiskTab>('windows');
const localLoading = ref(false);
const localError = ref<string | null>(null);
const localRedisAvailable = ref(true);
const localRedisError = ref<string | null>(null);

const linuxServerList = ref<DiskServerItem[]>([]);
const selectedLinuxServerIp = ref('');
const linuxDisks = ref<DiskInfoItem[]>([]);
const windowsDisks = ref<WindowsDiskItem[]>([]);
const localPresentCacheKeys = ref<Set<string>>(new Set());

const ipsans = ref<IpsanItem[]>([]);
const ipsanLoading = ref(false);
const ipsanError = ref<string | null>(null);
const ipsanRedisAvailable = ref(true);
const ipsanRedisError = ref<string | null>(null);
const ipsanPresentCacheKeys = ref<Set<string>>(new Set());
```

- [ ] **Step 2: 先迁移 Linux 流程到新的本地盘区域函数，IPSAN 先保留空壳**

```ts
function linuxDiskCacheKeys(disks: DiskInfoItem[]) {
  return disks.map((disk) => `Storage:${disk.storageId}`);
}

async function fetchLinuxLocalRegion() {
  const host = hostIp.value.trim();
  if (!host) return;

  localLoading.value = true;
  localError.value = null;

  try {
    const servers = await diskCleanupListLinuxServers(host, timeoutSecs.value);
    linuxServerList.value = servers;

    const nextServerIp = servers.find((item) => item.serverIp === selectedLinuxServerIp.value)?.serverIp
      ?? servers[0]?.serverIp
      ?? '';
    selectedLinuxServerIp.value = nextServerIp;

    if (!nextServerIp) {
      linuxDisks.value = [];
      localPresentCacheKeys.value = new Set();
      return;
    }

    const disks = await diskCleanupListLinuxDisks(host, nextServerIp, timeoutSecs.value);
    linuxDisks.value = disks;

    const result = await diskCleanupCheckCacheKeys(host, linuxDiskCacheKeys(disks));
    localRedisAvailable.value = result.redis_available;
    localRedisError.value = result.error;
    localPresentCacheKeys.value = new Set(result.present_keys ?? []);
  } catch (error) {
    localError.value = t('diskCacheCleanup.errors.localHttp', { reason: formatError(error) });
  } finally {
    localLoading.value = false;
  }
}
```

- [ ] **Step 3: 添加顶层统一入口和本地盘 Tab 样式，但先只让 Linux 视图接管旧表格**

```vue
<div class="inline-flex gap-1 rounded-full border border-slate-200 bg-slate-100 p-1">
  <button
    type="button"
    class="rounded-full px-4 py-2 text-sm font-semibold"
    :class="localDiskTab === 'windows' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500'"
    @click="localDiskTab = 'windows'"
  >
    {{ t('diskCacheCleanup.localDisk.tabs.windows') }}
  </button>
  <button
    type="button"
    class="rounded-full px-4 py-2 text-sm font-semibold"
    :class="localDiskTab === 'linux' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500'"
    @click="localDiskTab = 'linux'"
  >
    {{ t('diskCacheCleanup.localDisk.tabs.linux') }}
  </button>
</div>
```

- [ ] **Step 4: 把顶部按钮切到统一入口函数，并在 tab 切换时只刷新本地盘区域**

```ts
async function fetchAllRegions() {
  await Promise.all([
    fetchLocalRegion(),
    fetchIpsanRegion(),
  ]);
}

async function fetchLocalRegion() {
  if (localDiskTab.value === 'linux') {
    await fetchLinuxLocalRegion();
    return;
  }
  await fetchWindowsLocalRegion();
}

watch(localDiskTab, async () => {
  if (!hasFetchedServers.value) return;
  await fetchLocalRegion();
});
```

- [ ] **Step 5: 运行 `pnpm check`，确认页面在“Linux 仍然可用”的状态下完成状态重构**

Run: `pnpm check`

Expected: PASS。页面可编译，统一入口和本地盘 Tab 已出现，Linux 路径仍能工作。

- [ ] **Step 6: 提交“共享状态 + Linux 迁移”**

```bash
git add src/pages/DiskCacheCleanupPage.vue
git commit -m "refactor(disk-cleanup): split shared and local-region state"
```

## Task 7: 实现 Windows 本地盘视图、分区级缓存 key 和本地盘区域清理动作

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`
- Test: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 实现 Windows 本地盘查询与分区级 Redis key 提取**

```ts
function windowsPartitionCacheKeys(disks: WindowsDiskItem[]) {
  return disks.flatMap((disk) =>
    disk.partitionList.map((partition) => `Storage:${partition.partitionGUID}`),
  );
}

async function fetchWindowsLocalRegion() {
  const host = hostIp.value.trim();
  if (!host) return;

  localLoading.value = true;
  localError.value = null;
  linuxServerList.value = [];
  selectedLinuxServerIp.value = '';

  try {
    const disks = await diskCleanupListWindowsDisks(host, timeoutSecs.value);
    windowsDisks.value = disks;
    const result = await diskCleanupCheckCacheKeys(host, windowsPartitionCacheKeys(disks));
    localRedisAvailable.value = result.redis_available;
    localRedisError.value = result.error;
    localPresentCacheKeys.value = new Set(result.present_keys ?? []);
  } catch (error) {
    localError.value = t('diskCacheCleanup.errors.localHttp', { reason: formatError(error) });
  } finally {
    localLoading.value = false;
  }
}
```

- [ ] **Step 2: 增加本地盘区域的行级和批量清理 helper，只操作本地盘区域**

```ts
async function cleanLocalKeys(keys: string[], reasonKey: 'localDelete') {
  const host = hostIp.value.trim();
  const result = await diskCleanupDeleteCacheKeys(host, keys);
  if (!result.redis_available || result.error) {
    localRedisAvailable.value = result.redis_available;
    localRedisError.value = result.error;
    localError.value = t(`diskCacheCleanup.errors.${reasonKey}`, {
      reason: result.error ?? t('diskCacheCleanup.cache.unavailable'),
    });
    return;
  }
  await fetchLocalRegion();
}
```

- [ ] **Step 3: 在模板中新增 Windows “磁盘分组 + 分区子表”**

```vue
<section v-if="localDiskTab === 'windows'" class="space-y-4">
  <article
    v-for="disk in windowsDisks"
    :key="disk.diskId"
    class="rounded-2xl border border-slate-200 bg-white"
  >
    <header class="flex items-center justify-between border-b border-slate-200 px-4 py-3">
      <div class="font-semibold text-slate-900">
        {{ t('diskCacheCleanup.windows.diskHeader', { number: disk.diskNumber, name: disk.diskName || '--' }) }}
      </div>
      <div class="font-mono text-sm text-slate-500">{{ formatCapacity(disk.totalCapacity) }}</div>
    </header>

    <table class="w-full min-w-[920px] text-sm">
      <tbody>
        <tr v-for="partition in disk.partitionList" :key="partition.partitionGUID">
          <td>{{ partition.partitionSeq }}</td>
          <td class="font-mono">{{ partition.partitionGUID }}</td>
          <td>{{ formatCapacity(partition.capacity) }}</td>
          <td>{{ t(usageLabelKey(partition.usage)) }}</td>
          <td>{{ partition.partitionStatus }}</td>
          <td>
            <span v-if="localPresentCacheKeys.has(`Storage:${partition.partitionGUID}`)">
              {{ t('diskCacheCleanup.cache.present') }}
            </span>
            <span v-else>{{ t('diskCacheCleanup.cache.absent') }}</span>
          </td>
          <td>
            <button
              v-if="localPresentCacheKeys.has(`Storage:${partition.partitionGUID}`)"
              type="button"
              @click="cleanLocalKeys([`Storage:${partition.partitionGUID}`], 'localDelete')"
            >
              {{ t('diskCacheCleanup.actions.cleanOne') }}
            </button>
          </td>
        </tr>
      </tbody>
    </table>
  </article>
</section>
```

- [ ] **Step 4: 给本地盘区域批量按钮接上当前 tab 的 key 集合**

```ts
const localCleanableKeys = computed(() => {
  if (localDiskTab.value === 'windows') {
    return windowsPartitionCacheKeys(windowsDisks.value)
      .filter((key) => localPresentCacheKeys.value.has(key));
  }

  return linuxDiskCacheKeys(linuxDisks.value)
    .filter((key) => localPresentCacheKeys.value.has(key));
});
```

```vue
<button
  type="button"
  :disabled="!localRedisAvailable || localCleanableKeys.length === 0"
  @click="cleanLocalKeys(localCleanableKeys, 'localDelete')"
>
  {{ t('diskCacheCleanup.localDisk.actions.cleanAll', { count: localCleanableKeys.length }) }}
</button>
```

- [ ] **Step 5: 运行类型检查，确认 Windows 视图、分区 key 和本地盘批量清理全都通过**

Run: `pnpm check`

Expected: PASS。

- [ ] **Step 6: 提交 Windows 本地盘实现**

```bash
git add src/pages/DiskCacheCleanupPage.vue
git commit -m "feat(disk-cleanup): add windows local disk partition cleanup"
```

## Task 8: 实现 IPSAN 区域、独立错误态和区域级清理动作

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`
- Test: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 添加 IPSAN 查询函数和 IPSAN key 提取 helper**

```ts
function ipsanCacheKeys(items: IpsanItem[]) {
  return items.map((item) => `Storage:${item.IPSANId}`);
}

async function fetchIpsanRegion() {
  const host = hostIp.value.trim();
  if (!host) return;

  ipsanLoading.value = true;
  ipsanError.value = null;

  try {
    const items = await diskCleanupListIpsans(host, timeoutSecs.value);
    ipsans.value = items;
    const result = await diskCleanupCheckCacheKeys(host, ipsanCacheKeys(items));
    ipsanRedisAvailable.value = result.redis_available;
    ipsanRedisError.value = result.error;
    ipsanPresentCacheKeys.value = new Set(result.present_keys ?? []);
  } catch (error) {
    ipsanError.value = t('diskCacheCleanup.errors.ipsanHttp', { reason: formatError(error) });
  } finally {
    ipsanLoading.value = false;
  }
}
```

- [ ] **Step 2: 实现 IPSAN 的单条和批量清理 helper，确保只刷新 IPSAN 区域**

```ts
const ipsanCleanableKeys = computed(() =>
  ipsanCacheKeys(ipsans.value).filter((key) => ipsanPresentCacheKeys.value.has(key)),
);

async function cleanIpsanKeys(keys: string[]) {
  const host = hostIp.value.trim();
  const result = await diskCleanupDeleteCacheKeys(host, keys);
  if (!result.redis_available || result.error) {
    ipsanRedisAvailable.value = result.redis_available;
    ipsanRedisError.value = result.error;
    ipsanError.value = t('diskCacheCleanup.errors.ipsanDelete', {
      reason: result.error ?? t('diskCacheCleanup.cache.unavailable'),
    });
    return;
  }
  await fetchIpsanRegion();
}
```

- [ ] **Step 3: 在模板中新增 IPSAN 区域卡片和表格**

```vue
<section class="rounded-[24px] border border-orange-200/80 bg-white/90 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
  <div class="flex items-center justify-between border-b border-orange-100 px-5 py-5">
    <div>
      <h2 class="text-lg font-bold text-slate-900">{{ t('diskCacheCleanup.ipsan.title') }}</h2>
      <p class="mt-1 text-sm text-slate-500">{{ t('diskCacheCleanup.ipsan.description') }}</p>
    </div>
    <div class="flex gap-2">
      <button type="button" @click="fetchIpsanRegion">
        {{ t('diskCacheCleanup.ipsan.actions.refresh') }}
      </button>
      <button
        type="button"
        :disabled="!ipsanRedisAvailable || ipsanCleanableKeys.length === 0"
        @click="cleanIpsanKeys(ipsanCleanableKeys)"
      >
        {{ t('diskCacheCleanup.ipsan.actions.cleanAll', { count: ipsanCleanableKeys.length }) }}
      </button>
    </div>
  </div>

  <div class="p-5 overflow-x-auto">
    <table class="min-w-[920px] w-full text-sm">
      <tbody>
        <tr v-for="item in ipsans" :key="item.IPSANId">
          <td>{{ item.IPSANName || item.IPSANIp }}</td>
          <td class="font-mono">{{ item.IPSANId }}</td>
          <td>{{ item.IPSANStatus }}</td>
          <td>{{ formatCapacity(item.totalCapacity) }}</td>
          <td>{{ t(usageLabelKey(item.usage)) }}</td>
          <td>
            <span v-if="ipsanPresentCacheKeys.has(`Storage:${item.IPSANId}`)">
              {{ t('diskCacheCleanup.cache.present') }}
            </span>
            <span v-else>{{ t('diskCacheCleanup.cache.absent') }}</span>
          </td>
          <td>
            <button
              v-if="ipsanPresentCacheKeys.has(`Storage:${item.IPSANId}`)"
              type="button"
              @click="cleanIpsanKeys([`Storage:${item.IPSANId}`])"
            >
              {{ t('diskCacheCleanup.actions.cleanOne') }}
            </button>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</section>
```

- [ ] **Step 4: 为本地盘和 IPSAN 各自加独立错误条和 Redis 告警条**

```vue
<section
  v-if="localError"
  class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
>
  {{ localError }}
</section>
<section
  v-if="localRedisError && !localRedisAvailable"
  class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
>
  {{ localRedisError }}
</section>

<section
  v-if="ipsanError"
  class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
>
  {{ ipsanError }}
</section>
<section
  v-if="ipsanRedisError && !ipsanRedisAvailable"
  class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
>
  {{ ipsanRedisError }}
</section>
```

- [ ] **Step 5: 跑 TS 类型检查，确认 IPSAN 与本地盘互不干扰**

Run: `pnpm check`

Expected: PASS。

- [ ] **Step 6: 提交 IPSAN 区域实现**

```bash
git add src/pages/DiskCacheCleanupPage.vue
git commit -m "feat(disk-cleanup): add IPSAN region and independent cleanup actions"
```

## Task 9: 收尾清理旧 wrapper、统一摘要卡片，并完成最终验证

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/DiskCacheCleanupPage.vue`
- Test: `src-tauri/src/disk_cleanup.rs`
- Test: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: 从 `tauri.ts` 删除旧的 Linux-only wrapper，避免新旧 API 并存**

```ts
// Delete these legacy wrappers entirely:
// diskCleanupListServers
// diskCleanupListDisks
// diskCleanupCheckRedis
// diskCleanupDeleteCache
```

- [ ] **Step 2: 更新顶部摘要卡片，让数字反映新双区域模型**

```ts
const localCachedCount = computed(() => localCleanableKeys.value.length);
const ipsanCachedCount = computed(() => ipsanCleanableKeys.value.length);
const localRowCount = computed(() =>
  localDiskTab.value === 'windows'
    ? windowsDisks.value.reduce((sum, disk) => sum + disk.partitionList.length, 0)
    : linuxDisks.value.length,
);
```

```vue
<div>{{ localRowCount }}</div>
<div>{{ ipsans.length }}</div>
<div>{{ localCachedCount }}</div>
<div>{{ ipsanCachedCount }}</div>
```

- [ ] **Step 3: 跑 Rust 单测**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`

Expected: PASS。

- [ ] **Step 4: 跑前端类型检查**

Run: `pnpm check`

Expected: PASS。

- [ ] **Step 5: 跑 lint**

Run: `pnpm exec eslint src/pages/DiskCacheCleanupPage.vue src/lib/tauri.ts src/locales/messages.ts`

Expected: PASS，未出现新的 ESLint 报错。

- [ ] **Step 6: 跑最终桌面构建**

Run: `pnpm tauri:build:versioned-exe`

Expected: PASS，生成 versioned exe，`scripts/rename-tauri-exe.mjs` 正常执行完成。

- [ ] **Step 7: 按 spec 走手测回归**

Manual checklist:

```text
1. Linux 本地盘：查询 -> 选择子机 -> 行级清理
2. Windows 本地盘：查询 -> 磁盘分组 -> 分区级清理
3. IPSAN：统一查询后显示表格 -> 行级清理
4. Tab 切换只刷新本地盘，IPSAN 保留
5. 选错类型仅本地盘区域报错
6. Redis 不可用时只禁用对应区域按钮
7. 本地盘批量清理只刷新本地盘
8. IPSAN 批量清理只刷新 IPSAN
```

- [ ] **Step 8: 提交最终集成结果**

```bash
git add src/lib/tauri.ts src/pages/DiskCacheCleanupPage.vue src/locales/messages.ts src-tauri/src/disk_cleanup.rs src-tauri/src/main.rs
git commit -m "feat(disk-cleanup): support windows local disk and IPSAN cleanup"
```
