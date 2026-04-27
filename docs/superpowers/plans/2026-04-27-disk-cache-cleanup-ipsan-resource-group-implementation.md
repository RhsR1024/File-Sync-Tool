# Disk Cache Cleanup IPSAN Resource Group Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add IPSAN resource-group support to disk cache cleanup, keep single-device IPSAN as the default view, and replace stacked cache-detail buttons with one shared inline detail affordance across Linux, Windows, IPSAN, and resource groups.

**Architecture:** Extend the existing Tauri disk-cleanup module with one new resource-group endpoint and typed response models, then adapt the Vue page to hold a second IPSAN sub-view backed by derived relationship data. Extract the cache-status cell into one shared inline presenter so all four resource families render the same cache present/detail affordance and Redis-unavailable fallback.

**Tech Stack:** Rust + Tauri commands, Vue 3 `<script setup>`, TypeScript, node:test source assertions, Rust unit tests

---

### Task 1: Lock the new behavior with failing tests

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`
- Modify: `src/pages/DiskCacheCleanupPage.test.mjs`

- [ ] **Step 1: Add Rust tests for resource-group payload parsing**

```rust
    #[test]
    fn parse_ipsan_resource_group_payload_returns_members() {
        let body = r#"{
            "code": 0,
            "message": "Success",
            "data": {
                "groupInfoList": [
                    {
                        "groupId": "439245456753561600",
                        "groupName": "192.115.2.26",
                        "groupStatus": 1,
                        "totalCapacity": 1296,
                        "usage": 2,
                        "resourceInfoList": [
                            {
                                "IPSANId": "438596966545362944",
                                "IPSANName": "192.115.2.26",
                                "IPSANIp": "192.115.2.26",
                                "IPSANStatus": 1,
                                "capacity": 648
                            }
                        ]
                    }
                ]
            }
        }"#;

        let parsed = parse_api_payload::<IpsanResourceGroupListData>(StatusCode::OK, body).unwrap();
        assert_eq!(parsed.group_info_list.len(), 1);
        assert_eq!(parsed.group_info_list[0].resource_info_list.len(), 1);
        assert_eq!(parsed.group_info_list[0].usage, 2);
    }
```

- [ ] **Step 2: Add page-source tests for the new IPSAN sub-view and inline cache-detail component**

```js
test('disk cache cleanup renders an IPSAN sub-view switch for devices and resource groups', () => {
  assert.match(pageSource, /ipsanSubTab/);
  assert.match(pageSource, /resourceGroups/);
});

test('disk cache cleanup uses the shared inline cache-status presenter instead of stacked view-details buttons', () => {
  assert.match(pageSource, /CacheStateInline/);
  assert.doesNotMatch(pageSource, /space-y-2/);
});
```

- [ ] **Step 3: Run the focused tests and confirm they fail for the expected reason**

Run:

```powershell
@'
import { test } from "node:test";
'@ | node - > $null
node --test src/pages/DiskCacheCleanupPage.test.mjs
cargo test parse_ipsan_resource_group_payload_returns_members --manifest-path src-tauri/Cargo.toml
```

Expected:

- `node --test` fails because the page does not yet contain the IPSAN resource-group switch / shared inline presenter
- `cargo test` fails because `IpsanResourceGroupListData` does not exist yet

### Task 2: Add resource-group contracts to the Tauri layer and TS bridge

**Files:**
- Modify: `src-tauri/src/disk_cleanup.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add Rust models and the new endpoint path**

```rust
const IPSAN_RESOURCE_GROUP_LIST_PATH: &str = "/openAPI/system/v1/IPSAN/resourceGroup/list";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpsanResourceGroupMemberItem { /* ... */ }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpsanResourceGroupItem { /* ... */ }

#[derive(Debug, Deserialize)]
struct IpsanResourceGroupListData {
    #[serde(rename = "groupInfoList", default)]
    group_info_list: Vec<IpsanResourceGroupItem>,
}
```

- [ ] **Step 2: Add the Tauri command and register it**

```rust
#[tauri::command]
pub async fn disk_cleanup_list_ipsan_resource_groups(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<IpsanResourceGroupItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, IPSAN_RESOURCE_GROUP_LIST_PATH);
    let data: IpsanResourceGroupListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.group_info_list)
}
```

And in `main.rs`:

```rust
disk_cleanup::disk_cleanup_list_ipsan_resource_groups,
```

- [ ] **Step 3: Add TypeScript interfaces and invoke wrapper**

```ts
export interface IpsanResourceGroupMemberItem { /* ... */ }
export interface IpsanResourceGroupItem { /* ... */ }

export async function diskCleanupListIpsanResourceGroups(
  host: string,
  timeoutSecs: number,
): Promise<IpsanResourceGroupItem[]> {
  return await invoke<IpsanResourceGroupItem[]>('disk_cleanup_list_ipsan_resource_groups', {
    host,
    timeoutSecs,
  });
}
```

- [ ] **Step 4: Re-run the focused tests**

Run:

```powershell
cargo test parse_ipsan_resource_group_payload_returns_members --manifest-path src-tauri/Cargo.toml
```

Expected: PASS

### Task 3: Add the shared inline cache-status presenter and wire it into all four resource families

**Files:**
- Create: `src/components/CacheStateInline.vue`
- Modify: `src/pages/DiskCacheCleanupPage.vue`

- [ ] **Step 1: Create the shared inline cache-status component**

```vue
<script setup lang="ts">
defineProps<{
  present: boolean;
  redisAvailable: boolean;
  detailsLabel: string;
  detailsAriaLabel: string;
}>();

const emit = defineEmits<{
  (e: 'open-detail'): void;
}>();
</script>
```

- [ ] **Step 2: Replace the repeated cache cell markup in Linux, Windows, and IPSAN rows**

```vue
<CacheStateInline
  :present="localPresentCacheKeys.has(linuxDiskCacheKey(disk.storageId))"
  :redis-available="localRedisAvailable"
  :details-label="t('diskCacheCleanup.cache.detailCompact')"
  :details-aria-label="t('diskCacheCleanup.actions.viewDetails')"
  @open-detail="openCacheDetail(linuxDiskCacheKey(disk.storageId))"
/>
```

- [ ] **Step 3: Re-run the page-source tests**

Run:

```powershell
node --test src/pages/DiskCacheCleanupPage.test.mjs
```

Expected: PASS for the inline detail presenter assertions

### Task 4: Implement IPSAN device/resource-group sub-views and relationship derivation

**Files:**
- Modify: `src/pages/DiskCacheCleanupPage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Add IPSAN sub-tab state, derived relationship maps, and resource-group cache sets**

```ts
type IpsanSubTab = 'devices' | 'resourceGroups';

const ipsanSubTab = ref<IpsanSubTab>('devices');
const ipsanResourceGroups = ref<IpsanResourceGroupItem[]>([]);
const ipsanResourceGroupPresentCacheKeys = ref<Set<string>>(new Set());
```

- [ ] **Step 2: Fetch `IPSAN/list` and `IPSAN/resourceGroup/list` together, then derive relationships**

```ts
const [items, groups] = await Promise.all([
  diskCleanupListIpsans(host, timeoutSecs.value),
  diskCleanupListIpsanResourceGroups(host, timeoutSecs.value),
]);
```

- [ ] **Step 3: Render the IPSAN sub-tab switch and both tables**

```vue
<button @click="ipsanSubTab = 'devices'">{{ t('diskCacheCleanup.ipsan.subTabs.devices') }}</button>
<button @click="ipsanSubTab = 'resourceGroups'">{{ t('diskCacheCleanup.ipsan.subTabs.resourceGroups') }}</button>
```

- [ ] **Step 4: Update i18n for resource groups and compact detail affordance**

```ts
subTabs: {
  devices: '单台 IPSAN',
  resourceGroups: '资源组',
},
cache: {
  detailCompact: '详情',
},
```

- [ ] **Step 5: Run focused UI and Rust tests, then typecheck**

Run:

```powershell
node --test src/pages/DiskCacheCleanupPage.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml disk_cleanup
pnpm exec vue-tsc --noEmit
```

Expected:

- node tests PASS
- relevant Rust tests PASS
- `vue-tsc` PASS

