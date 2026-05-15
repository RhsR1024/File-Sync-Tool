# Manual Copy — Batch Source Support (Design)

**Date**: 2026-05-14
**Status**: Draft / awaiting user review
**Scope**: Frontend-only change to `ManualCopyModal`; no backend changes.
**Branching**: Edits land directly on `main`.

---

## 1. Goal

Let users paste multiple version-folder paths into the "指定复制" (`ManualCopyModal`) source field and queue all of them in one round-trip, automatically disambiguating per-source target paths when the source folders share the same trailing segment.

**Worked example** (user-provided):

Target root: `E:\UMS_TEMP\1.3.9.P10`

Source list (pasted, one per line):

```
\\nt03\iCPD\版本\UMS\正式版本\V100R001B02\1.3.9.P10
\\nt03\iCPD\版本\UMS\正式版本\V100R001B08\1.3.9.P10
\\nt03\iCPD\版本\UMS\正式版本\V100R002B03\1.3.9.P10
\\nt03\iCPD\版本\VMS\正式版本\V200R001B01\1.3.9.P10
\\nt03\iCPD\版本\VMS\正式版本\V200R001B02\1.3.9.P10
\\nt03\iCPD\版本\VMS\正式版本\V200R001B05\1.3.9.P10
\\nt03\iCPD\版本\VMS\正式版本\V200R001B11\1.3.9.P10
\\nt03\iCPD\版本\VMS\正式版本\V200R001B17\1.3.9.P10
\\nt03\iCPD\版本\UMS-IPSAN\1.3.9.P10
```

Resolved final targets:

```
E:\UMS_TEMP\1.3.9.P10\V100R001B02\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V100R001B08\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V100R002B03\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V200R001B01\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V200R001B02\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V200R001B05\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V200R001B11\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V200R001B17\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\UMS-IPSAN\1.3.9.P10
```

---

## 2. UI Design

**Entry point**: `src/components/ManualCopyModal.vue` — extend the existing modal in place. No new page, no new modal.

**Source input change**:
- Replace the single-line `<input>` with a `<textarea rows="4">` (auto-grow up to ~10 rows).
- Placeholder text indicates multi-line support: `每行一个路径，支持批量 / One path per line, batch supported`.
- On `input`, split by newline, trim each line, drop empties. The trimmed-line count `N` drives the mode.

**Mode behavior**:
- `N == 1` → single mode. Identical to today's flow: submit button is "开始复制", existing inline `existingTargetPreview` warning logic still applies. **Zero regression.**
- `N >= 2` → batch mode. Submit button text becomes `预览批次 (N)`. Clicking expands an inline preview table (below the form, above the existing right-side info cards if visible). The single-source `existingTargetPreview` block is hidden; conflicts move into the preview table instead.

**Preview table** (inline expanded section, not a new modal):

| ☑ | 源路径 | 最终目标 | 状态 |
|---|---|---|---|
| ☑ | `\\nt03\...\UMS\正式版本\V100R001B02\1.3.9.P10` | `E:\UMS_TEMP\1.3.9.P10\V100R001B02\1.3.9.P10` | OK |
| ☑ | `\\nt03\...\UMS\正式版本\V100R001B08\1.3.9.P10` | `E:\UMS_TEMP\1.3.9.P10\V100R001B08\1.3.9.P10` | OK |
| ... | ... | ... | ... |
| ☐ | `\\nt03\...\UMS-IPSAN\1.3.9.P10` | `E:\UMS_TEMP\1.3.9.P10\UMS-IPSAN\1.3.9.P10` | `目标已存在` |

- Header row has a master checkbox (全选 / 反选).
- Status badge variants: `OK` (emerald), `目标已存在` (amber), `源不存在` (red), `批次内重复` (red), `路径无效` (red).
- OK rows default-checked. Problem rows default-unchecked but the user can manually check them (checking a "目标已存在" row implies `overwriteExisting=true` for that single row's invoke; checking a `批次内重复` or `源不存在` row is allowed in UI but does nothing useful — keep simple: backend will return error and the row turns red post-submit).
- Submit button at the bottom of the table: `开始复制 (X)` where X is the live count of checked rows. Disabled when X == 0.
- A secondary "返回编辑" link/button collapses the preview back to the textarea (without clearing the text).

**Filter panel**: existing extension / keyword chip selectors stay in place. Above them, a one-line clarifier appears in batch mode: `下列过滤将统一应用于全部 N 项 / Filters below apply uniformly to all N entries.`

---

## 3. Disambiguation Algorithm

**Location**: new pure module `src/lib/manualCopyBatch.ts`, importable by the modal and unit-testable in isolation.

**Public API**:

```ts
export interface BatchEntryInput {
  rawSource: string;       // user-pasted line
}

export type BatchStatus =
  | 'ok'
  | 'target_exists'        // set by caller after preview RPC
  | 'source_missing'       // set by caller after preview RPC throws
  | 'duplicate_in_batch'   // set by algorithm when it cannot disambiguate
  | 'invalid_path';        // set by algorithm when source has zero usable segments

// Files vs directories: both are valid sources (today's modal already
// supports both via source_kind in ManualCopyPreview). Source kind is
// surfaced in the row's tooltip but does not gate submission.

export interface BatchEntryResolution {
  rawSource: string;
  normalizedSegments: string[];     // for debugging / display
  tail: string;                     // last segment (the folder name backend will re-append)
  disambiguatorSegments: string[];  // K parent segments inserted between targetRoot and tail
  effectiveTargetRoot: string;      // targetRoot joined with disambiguatorSegments
  finalTarget: string;              // effectiveTargetRoot + '/' + tail (display only)
  status: 'ok' | 'duplicate_in_batch' | 'invalid_path';
}

export function resolveBatchTargets(
  rawSources: string[],
  targetRoot: string,
): BatchEntryResolution[];
```

**Algorithm** (pseudocode):

```
1. Normalize each source: replace `/` with `\`, strip leading/trailing whitespace,
   strip trailing `\`, NFC unicode, split on `\\+`, drop empty segments.
   Preserve UNC head: if rawSource starts with `\\`, treat the first two segments
   (server + share) as inseparable, but they are still segments for keying.
2. If any source has 0 segments → mark status='invalid_path', skip from
   uniqueness computation.
3. For K = 0..max(segs[i].length - 1):
     key[i] = segs[i].slice(-K-1).join('/').toLowerCase()  // case-insensitive
     if all valid key[i] are pairwise unique:
       for each valid i:
         disambiguatorSegments = segs[i].slice(
           max(0, segs[i].length - K - 1),
           segs[i].length - 1
         )
         tail = segs[i].last
         effectiveTargetRoot = pathJoin(targetRoot, ...disambiguatorSegments)
         finalTarget = pathJoin(effectiveTargetRoot, tail)
         status = 'ok'
       return
4. If loop exhausts without unique keys → for each colliding cluster,
   mark every member status='duplicate_in_batch'. Non-colliding members
   resolve normally at the deepest K that disambiguates them.
```

**Notes**:
- Case-insensitive comparison (Windows semantics); preserve original case in output.
- No numeric `_1, _2` suffix fallback — duplicates after max depth are real duplicates and surface as red rows.
- `pathJoin` uses backslash separators (this app is Windows-only).

**Worked example verification table**:

| Source tail | K=0 key | K=1 key | Result |
|---|---|---|---|
| `…\V100R001B02\1.3.9.P10` | `1.3.9.p10` (×9 collide) | `v100r001b02/1.3.9.p10` (unique) | targetRoot + `V100R001B02` |
| `…\V100R001B08\1.3.9.P10` | collide | `v100r001b08/1.3.9.p10` (unique) | targetRoot + `V100R001B08` |
| `…\V100R002B03\1.3.9.P10` | collide | `v100r002b03/1.3.9.p10` | targetRoot + `V100R002B03` |
| `…\V200R001B01\1.3.9.P10` | collide | `v200r001b01/1.3.9.p10` | targetRoot + `V200R001B01` |
| `…\V200R001B02\1.3.9.P10` | collide | `v200r001b02/1.3.9.p10` | targetRoot + `V200R001B02` |
| `…\V200R001B05\1.3.9.P10` | collide | `v200r001b05/1.3.9.p10` | targetRoot + `V200R001B05` |
| `…\V200R001B11\1.3.9.P10` | collide | `v200r001b11/1.3.9.p10` | targetRoot + `V200R001B11` |
| `…\V200R001B17\1.3.9.P10` | collide | `v200r001b17/1.3.9.p10` | targetRoot + `V200R001B17` |
| `…\UMS-IPSAN\1.3.9.P10` | collide | `ums-ipsan/1.3.9.p10` | targetRoot + `UMS-IPSAN` |

K=1 disambiguates all 9 entries; final targets match user expectation.

---

## 4. Preview RPC Wiring

After the algorithm runs (synchronous, instant), the modal calls
`previewTemporaryCopy(rawSource, effectiveTargetRoot)` once per entry, in
parallel via `Promise.all`. This returns the existing `ManualCopyPreview`
shape per entry: `source_kind`, `target_exists`, etc.

Mapping returned values onto each row's `BatchStatus`:
- preview throws (path invalid / not found) → `source_missing`
- `target_exists == true` → `target_exists`
- Otherwise → `ok` (file vs directory both fine; today's modal accepts both)

If any RPC takes > 200ms total, show a spinner inside the preview area; the algorithm step is instant, only the preview round-trips can be slow.

---

## 5. Submission Flow

On "开始复制 (X)" click:

1. Snapshot the checked rows in display order.
2. Disable the button, swap to spinner.
3. For each row in order, `await queueTemporaryCopy(rawSource, effectiveTargetRoot, overwriteExisting, fileExtensions, filenameIncludes)`:
   - `overwriteExisting` = true only for rows whose status is `target_exists` and the user has explicitly checked them.
   - On success: track in `successCount`.
   - On error: mark that row's status badge red and continue with the next row. Track in `failedRows`.
4. After the loop:
   - If `failedRows.length == 0` → close modal, clear textarea, reset selections, emit `success`. Push toast `成功入队 N 项` (existing toast helper).
   - Else → keep modal open, push toast `成功入队 {ok}/{total}，失败行请修正后重试`, leave failed rows visibly red.

Sequential await keeps backend queue order identical to display order. Each
invoke only enqueues (returns ack immediately), so N=50 still completes in
well under a second.

---

## 6. State / Reactivity in `ManualCopyModal.vue`

New refs:
- `sourceLines = computed(() => sourcePath.value.split(/\r?\n/).map(s => s.trim()).filter(Boolean))`
- `isBatchMode = computed(() => sourceLines.value.length >= 2)`
- `batchResolutions = ref<BatchEntryResolution[]>([])` — algorithm output
- `batchPreviews = ref<Map<rawSource, ManualCopyPreview | Error>>()` — RPC results
- `batchRowChecked = ref<Map<rawSource, boolean>>()` — checkbox state
- `batchPreviewOpen = ref(false)` — controls inline expansion
- `batchSubmitting = ref(false)`

Behavior:
- `sourcePath` change → reset `batchPreviewOpen`, `batchPreviews`, `batchResolutions`. (Re-clicking "预览批次" recomputes from scratch — cheap.)
- `targetRootPath` change → same.

The single-source flow's existing `existingTargetPreview` / `pendingSubmitRequest` machinery is kept but only used when `isBatchMode == false`.

---

## 7. i18n

All new strings added to `src/locales/messages.ts` under `manualCopy.batch.*` namespace with both `en` and `zh`:

| Key | en | zh |
|---|---|---|
| `manualCopy.batch.placeholder` | One path per line — multi-line enables batch | 每行一个路径 — 多行自动进入批量 |
| `manualCopy.batch.previewButton` | Preview Batch ({count}) | 预览批次 ({count}) |
| `manualCopy.batch.submitButton` | Start Copy ({count}) | 开始复制 ({count}) |
| `manualCopy.batch.backToEdit` | Back to Edit | 返回编辑 |
| `manualCopy.batch.filtersApplyAll` | Filters below apply to all {count} entries | 下列过滤将统一应用于全部 {count} 项 |
| `manualCopy.batch.colSource` | Source path | 源路径 |
| `manualCopy.batch.colTarget` | Final target | 最终目标 |
| `manualCopy.batch.colStatus` | Status | 状态 |
| `manualCopy.batch.status.ok` | OK | 可复制 |
| `manualCopy.batch.status.targetExists` | Target exists | 目标已存在 |
| `manualCopy.batch.status.sourceMissing` | Source missing | 源不存在 |
| `manualCopy.batch.status.duplicateInBatch` | Duplicate in batch | 批次内重复 |
| `manualCopy.batch.status.invalidPath` | Invalid path | 路径无效 |
| `manualCopy.batch.toastSuccessAll` | Queued {count} items | 成功入队 {count} 项 |
| `manualCopy.batch.toastPartial` | Queued {ok}/{total}; please fix failed rows and retry | 成功入队 {ok}/{total}，失败行请修正后重试 |

---

## 8. Testing

**Unit (node:test, run via `node --test`)** — `src/lib/manualCopyBatch.test.mjs`:
- `K=0` direct: 3 sources with distinct tails → all `effectiveTargetRoot == targetRoot`.
- `K=1` user example: 9 sources, all tails `1.3.9.P10`, all 倒数第二段 unique → matches verification table.
- `K=2` recursion: 2 sources with same tail AND same parent, different grandparent → grandparent inserted.
- Uneven depth: source A has 5 segments, source B has 3 segments, both share tail → algorithm uses whatever depth each has.
- Real duplicate: two identical paths → both flagged `duplicate_in_batch`.
- Case difference: `…\Foo\1.3.9.P10` and `…\foo\1.3.9.P10` → flagged duplicate (Windows case-insensitive).
- UNC head: `\\srv\share\X` and `\\srv\share\Y` resolve correctly without losing `\\` prefix.
- Invalid path: empty string, only-whitespace, only-separators → `invalid_path`.

**Component (node:test, source-string assertions)** — extend `src/components/ManualCopyModal.test.mjs` (existing file uses `readFileSync` + regex assertions against the .vue source):
- Pasting a single line → no preview table; existing inline-existing-target warning renders.
- Pasting 3 lines (1 with conflict) → preview table renders; conflict row default-unchecked; submit button reads `(2)`.
- Submitting calls `queueTemporaryCopy` 3 times (or 2 if conflict row unchecked) in display order.

**Manual smoke** (you):
- Paste the 9-path user example into the modal. Confirm preview table matches the verification table. Confirm queuing produces 9 task records in the progress panel.

---

## 9. Out of Scope

- No backend changes (no new Tauri command, no `queue_temporary_copy_batch`).
- No persistence of pasted source list (clears on submit / modal close).
- No reordering / drag-sort in preview table.
- No per-row filter override.
- No cancel-mid-enqueue button (the enqueue loop is sub-second).

---

## 10. Files Touched (estimated)

- `src/lib/manualCopyBatch.ts` (new) — algorithm + types.
- `src/lib/manualCopyBatch.test.mjs` (new) — unit tests via `node:test`.
- `src/components/ManualCopyModal.vue` (modified) — textarea, preview table, submission loop.
- `src/locales/messages.ts` (modified) — new i18n keys, en + zh.
- `src/components/ManualCopyModal.test.mjs` (modified) — source-string assertions for batch mode.

No Rust file changes.
