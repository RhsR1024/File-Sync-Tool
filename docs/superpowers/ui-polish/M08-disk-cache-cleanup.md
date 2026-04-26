# M08 — Disk Cache Cleanup Polish

- **Phase**: 3 (after M01)
- **Risk**: Medium — page is 1599 lines, complex tabs / state
- **Files**: `src/pages/DiskCacheCleanupPage.vue`

---

## Goal

Tame the visual complexity of a tab-based, multi-host, batch-operation page. No logic change.

---

## Issues

1. **Status color sets (lines 48-56) are hardcoded numeric ranges.** Status codes 1-23 mapped to 4 color groups. Users have no legend. **Fix**: Add a hoverable legend chip near the cache list, OR a fixed legend panel at the top of the cache details section. Each chip: color + range + meaning text.
2. **Tab interface (Local Disks / IPSAN)** — confirm tabs have `role="tablist"`, `role="tab"`, `aria-selected`, and arrow-key navigation between tabs.
3. **Multiple ref splits** (`localError`, `ipsanError`, `cacheDetailError`, etc.) — fine architecturally; UI-wise, errors should be displayed in a consistent inline component, not 3 different patterns. Standardize the error display block.
4. **Recent hosts in localStorage** — good UX; confirm there's a UI affordance to clear the recent list.
5. **Initial host list fetch** — no skeleton. Drop M01's skeleton.
6. **Batch clean button** — destructive action. Confirm it has a confirmation modal or inline "double-click to confirm" pattern. Use `bg-rose-500` for destructive button, not `bg-red-500` (consistency with rest of app).
7. **Cache key list virtualization** — if more than ~50 entries, drop a virtualization library OR document why not (cap at N entries, paginate).
8. **Selection checkboxes** — keyboard space toggles, indeterminate state shown for partial selection.
9. **Status chip (with the numeric range mapping)** — confirm each chip has both color AND text, not color-only.
10. **Loading state during clean** — show a progress bar with item count, not just a spinner.
11. **Request sequence (`localRequestSeq` etc.)** — defensive race-prevention. UI doesn't need to expose this, but verify failed requests don't leave stale results visible.
12. **Empty state when no host selected** — drop in `Empty.vue`.

### i18n keys (new)

| Key | zh | en |
|---|---|---|
| `diskCacheCleanup.legend.title` | 状态说明 | Status legend |
| `diskCacheCleanup.legend.normal` | 正常 | Normal |
| `diskCacheCleanup.legend.pending` | 等待中 | Pending |
| `diskCacheCleanup.legend.warning` | 异常 | Warning |
| `diskCacheCleanup.legend.error` | 错误 | Error |
| `diskCacheCleanup.batch.confirm` | 再次点击确认清理 | Click again to confirm cleanup |
| `diskCacheCleanup.empty.noHost` | 选择主机查看缓存 | Select a host to view cache |
| `diskCacheCleanup.recent.clear` | 清除最近主机 | Clear recent hosts |

---

## Out of scope

- DO NOT change Redis / disk command interfaces.
- DO NOT change cache status code mapping (just expose it via legend).
- DO NOT split into sub-pages.
- DO NOT change localStorage key or schema.

---

## Verification

1. `pnpm check` clean.
2. Cargo tests for disk_cleanup module still pass.
3. Tab between Local / IPSAN with arrow keys.
4. Hover legend → shows color → meaning mapping.
5. Trigger a batch clean → confirmation step required.
6. Disconnect network during fetch → inline error visible, retry possible.

---

## Reporting back

Under 200 words.
