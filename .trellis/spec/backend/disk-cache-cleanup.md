# Scenario: Disk Cache Cleanup Resource Contracts

## 1. Scope / Trigger

- Trigger: The disk cache cleanup tool now spans three resource families with different source APIs and different cache-key granularity.
- Why code-spec depth is required:
  - Linux local disk uses `storageId`
  - Windows local disk uses `partitionGUID`
  - IPSAN uses `IPSANId`
  - The UI shows one page, but the backend must preserve these boundary rules exactly

## 2. Signatures

Recommended Tauri command boundaries:

```rust
#[tauri::command]
async fn disk_cleanup_list_linux_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<LinuxServerItem>, String>;

#[tauri::command]
async fn disk_cleanup_list_mainline_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<LinuxServerItem>, String>;

#[tauri::command]
async fn disk_cleanup_list_linux_disks(
    host: String,
    server_ip: String,
    timeout_secs: u32,
) -> Result<Vec<LinuxDiskItem>, String>;

#[tauri::command]
async fn disk_cleanup_list_windows_disks(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<WindowsDiskItem>, String>;

#[tauri::command]
async fn disk_cleanup_list_ipsans(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<IpsanItem>, String>;

#[tauri::command]
async fn disk_cleanup_check_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyCheckResult;

#[tauri::command]
async fn disk_cleanup_delete_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyDeleteResult;
```

Return models:

```rust
pub struct CacheKeyCheckResult {
    pub present_keys: Vec<String>,
    pub redis_available: bool,
    pub error: Option<String>,
}

pub struct CacheKeyDeleteResult {
    pub deleted_count: i64,
    pub redis_available: bool,
    pub error: Option<String>,
}
```

## 3. Contracts

### 3.1 Source APIs

Linux local disk supports two modes selected by `AppConfig.disk_cleanup_linux_mode`
(`"componentized"` default, or `"mainline"`):

- Componentized (default):
  - `POST http://{host}:23011/openAPI/system/v1/disk/server/list` returns `serverList[].serverIp`
  - `POST http://{host}:23011/openAPI/system/v1/disk/list` returns disk rows for the selected `serverIp`
- Mainline:
  - `POST http://{host}/distapi/status` (no port suffix → port 80), body `{}`
  - Response envelope is `{ "ErrCode": 0, "ErrMsg": "Succeed", "Status": [...] }`
  - Each `Status[]` row maps to a `DiskServerItem`:
    - `HostName` → `serverName`
    - `IP` (e.g. `192.115.1.157:21003`) → `serverIp` with the trailing `:port` stripped
    - `Role` → `role`
    - `Serial` → `serial`
    - `Status` → `serverCode`
  - After the server is picked, the same `POST http://{host}:23011/openAPI/system/v1/disk/list`
    is used; do not invent a separate mainline disk-listing endpoint.

Windows local disk:

- `POST /openAPI/system/v1/raw-disk/list`

IPSAN:

- `POST /openAPI/system/v1/IPSAN/list`

### 3.2 Cache-key rules

The backend must preserve these exact cache-key boundaries:

| Resource | Row-level identity | Cache key |
|---|---|---|
| Linux local disk | `storageId` | `Storage:{storageId}` |
| Windows local disk | `partitionGUID` | `Storage:{partitionGUID}` |
| IPSAN | `IPSANId` | `Storage:{IPSANId}` |

### 3.3 UI-to-backend rule

- Linux cleanup button attaches to disk rows
- Windows cleanup button attaches to partition rows
- IPSAN cleanup button attaches to IPSAN rows

The Redis commands should accept normalized full keys so the Redis layer stays resource-agnostic.

### 3.4 Validation rules

- `host` must be non-empty after trim
- `server_ip` is required only for Linux disk listing
- Redis `keys` input:
  - trim each item
  - drop empty items
  - dedupe
  - reject keys not starting with `Storage:`

### 3.5 Empty-list behavior

- Empty Linux server list: success with empty list
- Empty Windows disk list: success with empty list
- Empty IPSAN list: success with empty list
- Empty Redis key list:
  - `check`: success with `present_keys = []`
  - `delete`: success with `deleted_count = 0`

## 4. Validation & Error Matrix

| Case | Layer | Required behavior |
|---|---|---|
| `host` empty | command entry | Return validation error immediately |
| Linux `server_ip` empty | command entry | Return validation error immediately |
| Windows endpoint called on Linux host | HTTP/API | Return local-disk-area error; do not auto-fallback |
| Linux endpoint called on Windows host | HTTP/API | Return local-disk-area error; do not auto-fallback |
| Redis connect timeout | Redis | `redis_available = false`, include timeout reason |
| Redis command timeout | Redis | `redis_available = false`, include timeout reason |
| Redis key has wrong prefix | validation | Reject the request as invalid input |
| One region fails, the other succeeds | UI integration | Preserve successful region data |

## 5. Good / Base / Bad Cases

### Good

- Windows host returns two disks, one disk contains two partitions, one partition key exists in Redis
- Expected:
  - UI shows disk grouping
  - cleanup button appears only on the matching partition row

### Base

- IPSAN endpoint returns five rows and none of the `Storage:{IPSANId}` keys exist
- Expected:
  - IPSAN table renders all rows
  - no row-level cleanup button appears
  - batch cleanup stays disabled

### Bad

- Frontend sends Windows disk-level `diskId` as Redis key identity
- Expected:
  - contract review fails
  - implementation is incorrect because Windows cache granularity is partition-level

## 6. Tests Required

- Key builder tests:
  - Linux uses `Storage:{storageId}`
  - Windows uses `Storage:{partitionGUID}`
  - IPSAN uses `Storage:{IPSANId}`
- API envelope parsing tests:
  - `raw-disk/list`
  - `IPSAN/list`
  - `distapi/status` envelope with `ErrCode`/`ErrMsg`/`Status` shape
  - Mainline replica IP `192.115.1.157:21003` is exposed to the frontend as `192.115.1.157`
- Validation tests:
  - reject empty host
  - reject Linux disk listing without `server_ip`
  - reject non-`Storage:` Redis keys
- Behavior tests:
  - empty key list returns success no-op
  - dedupe preserves one Redis operation per unique key

## 7. Wrong vs Correct

### Wrong

```text
Windows local disk:
- render one cleanup button on the disk row
- build Redis key as Storage:{diskId}
```

Why wrong:

- Windows cache state is partition-based, not disk-based

### Correct

```text
Windows local disk:
- render cleanup buttons on partition rows
- build Redis key as Storage:{partitionGUID}
```

### Wrong

```text
When the user switches Local Disk tab, refresh IPSAN too.
```

Why wrong:

- IPSAN is not dependent on the local-disk type selector

### Correct

```text
When the user switches Local Disk tab:
- refresh only the local-disk region
- keep IPSAN results unchanged
```
