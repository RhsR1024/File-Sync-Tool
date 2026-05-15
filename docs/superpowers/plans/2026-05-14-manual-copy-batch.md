# Manual Copy — Batch Source with Auto-Disambiguation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `ManualCopyModal` so users can paste multiple source paths (one per line); automatically disambiguate per-source target paths when trailing folder names collide; queue all checked rows through the existing single-source backend command.

**Architecture:** A new pure module `src/lib/manualCopyBatch.ts` owns the disambiguation algorithm and is fully unit-tested via `node:test`. `ManualCopyModal.vue` switches its source input to a `<textarea>`, detects multi-line input, calls the algorithm, runs `previewTemporaryCopy` per row in parallel for conflict detection, and surfaces an inline preview table. On submit, the modal serially calls the existing `queueTemporaryCopy(source, target_root, …)` once per checked row. **Backend is untouched.**

**Tech Stack:** Vue 3 (`<script setup>` + Composition API), TypeScript, Tailwind CSS 4, Tauri 2 invoke API, `node:test` for unit tests, `lucide-vue-next` icons, `vue-i18n` for localization.

**Spec:** `docs/superpowers/specs/2026-05-14-manual-copy-batch-design.md`

**Branching:** Land directly on `main` (per user preference for small UX tweaks).

---

## Planned File Structure

**New**
- `src/lib/manualCopyBatch.ts` — types + `resolveBatchTargets()` pure function.
- `src/lib/manualCopyBatch.test.mjs` — unit tests via `node:test`.

**Modified**
- `src/components/ManualCopyModal.vue` — replace single-line `<input>` with `<textarea>`; new computed properties for batch detection, resolution, and preview; new preview-table template region; submission loop replaces single-call path.
- `src/locales/messages.ts` — new `manualCopy.batch.*` keys (en + zh).
- `src/components/ManualCopyModal.test.mjs` — extend source-string assertions to cover new textarea + preview-table markers.

**Untouched**
- All Rust files (`src-tauri/...`).
- `src/lib/tauri.ts` — existing `previewTemporaryCopy` / `queueTemporaryCopy` are reused as-is.

---

### Task 1: Create the algorithm module skeleton with failing tests

**Files:**
- Create: `src/lib/manualCopyBatch.ts`
- Create: `src/lib/manualCopyBatch.test.mjs`

- [ ] **Step 1: Write the first failing test (single source, no collision)**

Create `src/lib/manualCopyBatch.test.mjs`:

```mjs
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { resolveBatchTargets } from './manualCopyBatch.ts';

test('resolveBatchTargets returns a single OK entry for a single source with no collision', () => {
  const result = resolveBatchTargets(
    ['\\\\nt03\\share\\UMS\\1.3.9.P10'],
    'E:\\UMS_TEMP',
  );

  assert.equal(result.length, 1);
  assert.equal(result[0].status, 'ok');
  assert.equal(result[0].tail, '1.3.9.P10');
  assert.deepEqual(result[0].disambiguatorSegments, []);
  assert.equal(result[0].effectiveTargetRoot, 'E:\\UMS_TEMP');
  assert.equal(result[0].finalTarget, 'E:\\UMS_TEMP\\1.3.9.P10');
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test src/lib/manualCopyBatch.test.mjs`
Expected: FAIL with "Cannot find module './manualCopyBatch.ts'".

- [ ] **Step 3: Create the minimal module to make the test pass**

Create `src/lib/manualCopyBatch.ts`:

```ts
export type BatchStatus = 'ok' | 'duplicate_in_batch' | 'invalid_path';

export interface BatchEntryResolution {
  rawSource: string;
  normalizedSegments: string[];
  tail: string;
  disambiguatorSegments: string[];
  effectiveTargetRoot: string;
  finalTarget: string;
  status: BatchStatus;
}

const SEGMENT_SPLIT = /[\\\/]+/;

function normalizeSegments(raw: string): string[] {
  const trimmed = raw.replace(/^\s+|\s+$/g, '').replace(/[\\\/]+$/g, '');
  if (!trimmed) return [];
  // Preserve UNC prefix `\\server\share` semantics: keep the first two
  // segments as-is. The split will already produce them as separate items.
  return trimmed.split(SEGMENT_SPLIT).filter((s) => s.length > 0);
}

function joinWindowsPath(parts: string[]): string {
  return parts.join('\\');
}

function buildKey(segs: string[], depth: number): string {
  const start = Math.max(0, segs.length - depth - 1);
  return segs.slice(start).join('/').toLowerCase();
}

export function resolveBatchTargets(
  rawSources: string[],
  targetRoot: string,
): BatchEntryResolution[] {
  const normalized = rawSources.map((raw) => ({
    raw,
    segs: normalizeSegments(raw),
  }));

  const resolutions: BatchEntryResolution[] = normalized.map(({ raw, segs }) => ({
    rawSource: raw,
    normalizedSegments: segs,
    tail: segs[segs.length - 1] ?? '',
    disambiguatorSegments: [],
    effectiveTargetRoot: '',
    finalTarget: '',
    status: segs.length === 0 ? 'invalid_path' : 'ok',
  }));

  const validIndices = resolutions
    .map((r, i) => (r.status === 'ok' ? i : -1))
    .filter((i) => i >= 0);

  if (validIndices.length === 0) return resolutions;

  const maxSegs = Math.max(...validIndices.map((i) => normalized[i].segs.length));

  for (let depth = 0; depth < maxSegs; depth++) {
    const keys = validIndices.map((i) => buildKey(normalized[i].segs, depth));
    const seen = new Map<string, number>();
    let collided = false;
    for (let k = 0; k < keys.length; k++) {
      if (seen.has(keys[k])) {
        collided = true;
        break;
      }
      seen.set(keys[k], k);
    }
    if (!collided) {
      for (let k = 0; k < validIndices.length; k++) {
        const i = validIndices[k];
        const segs = normalized[i].segs;
        const start = Math.max(0, segs.length - depth - 1);
        const disambig = segs.slice(start, segs.length - 1);
        const tail = segs[segs.length - 1];
        const effRoot = joinWindowsPath([targetRoot.replace(/[\\\/]+$/g, ''), ...disambig]);
        resolutions[i].disambiguatorSegments = disambig;
        resolutions[i].effectiveTargetRoot = effRoot;
        resolutions[i].finalTarget = joinWindowsPath([effRoot, tail]);
      }
      return resolutions;
    }
  }

  // Could not disambiguate at max depth → mark colliding entries.
  const finalKeys = validIndices.map((i) =>
    buildKey(normalized[i].segs, normalized[i].segs.length - 1),
  );
  const groups = new Map<string, number[]>();
  finalKeys.forEach((key, k) => {
    const arr = groups.get(key) ?? [];
    arr.push(validIndices[k]);
    groups.set(key, arr);
  });
  for (const [, members] of groups) {
    if (members.length > 1) {
      for (const i of members) resolutions[i].status = 'duplicate_in_batch';
    }
  }
  // Non-duplicate entries (singletons) at max depth still need values; resolve them.
  for (const i of validIndices) {
    if (resolutions[i].status === 'duplicate_in_batch') continue;
    const segs = normalized[i].segs;
    const disambig = segs.slice(0, segs.length - 1);
    const tail = segs[segs.length - 1];
    const effRoot = joinWindowsPath([targetRoot.replace(/[\\\/]+$/g, ''), ...disambig]);
    resolutions[i].disambiguatorSegments = disambig;
    resolutions[i].effectiveTargetRoot = effRoot;
    resolutions[i].finalTarget = joinWindowsPath([effRoot, tail]);
  }
  return resolutions;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test src/lib/manualCopyBatch.test.mjs`
Expected: PASS — 1 test passing.

- [ ] **Step 5: Commit**

```bash
git add src/lib/manualCopyBatch.ts src/lib/manualCopyBatch.test.mjs
git commit -m "feat(manual-copy): add batch disambiguation algorithm scaffolding"
```

---

### Task 2: Cover the K=1 worked example from the spec

**Files:**
- Test: `src/lib/manualCopyBatch.test.mjs`

- [ ] **Step 1: Add the K=1 test (user's 9-path UMS/VMS example)**

Append to `src/lib/manualCopyBatch.test.mjs`:

```mjs
test('resolveBatchTargets disambiguates the 9-path UMS/VMS example at depth 1', () => {
  const sources = [
    '\\\\nt03\\iCPD\\版本\\UMS\\正式版本\\V100R001B02\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\UMS\\正式版本\\V100R001B08\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\UMS\\正式版本\\V100R002B03\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B01\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B02\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B05\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B11\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B17\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\UMS-IPSAN\\1.3.9.P10',
  ];

  const result = resolveBatchTargets(sources, 'E:\\UMS_TEMP\\1.3.9.P10');

  assert.equal(result.length, 9);
  result.forEach((r) => assert.equal(r.status, 'ok'));

  const finals = result.map((r) => r.finalTarget);
  assert.deepEqual(finals, [
    'E:\\UMS_TEMP\\1.3.9.P10\\V100R001B02\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V100R001B08\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V100R002B03\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B01\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B02\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B05\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B11\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B17\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\UMS-IPSAN\\1.3.9.P10',
  ]);
});
```

- [ ] **Step 2: Run the tests**

Run: `node --test src/lib/manualCopyBatch.test.mjs`
Expected: PASS — 2 tests passing.

- [ ] **Step 3: Commit**

```bash
git add src/lib/manualCopyBatch.test.mjs
git commit -m "test(manual-copy): cover 9-path UMS/VMS disambiguation example"
```

---

### Task 3: Cover edge cases — K=2 recursion, duplicates, case, invalid

**Files:**
- Test: `src/lib/manualCopyBatch.test.mjs`

- [ ] **Step 1: Add K=2 recursion test**

Append to `src/lib/manualCopyBatch.test.mjs`:

```mjs
test('resolveBatchTargets recurses to depth 2 when the immediate parent also collides', () => {
  const sources = [
    'C:\\repos\\foo\\V100R001B02\\1.3.9.P10',
    'C:\\releases\\foo\\V100R001B02\\1.3.9.P10',
  ];

  const result = resolveBatchTargets(sources, 'E:\\OUT');

  assert.equal(result.length, 2);
  result.forEach((r) => assert.equal(r.status, 'ok'));
  assert.deepEqual(result.map((r) => r.finalTarget), [
    'E:\\OUT\\repos\\foo\\V100R001B02\\1.3.9.P10',
    'E:\\OUT\\releases\\foo\\V100R001B02\\1.3.9.P10',
  ]);
});

test('resolveBatchTargets flags two identical paths as duplicate_in_batch', () => {
  const sources = [
    'C:\\share\\X\\1.3.9.P10',
    'C:\\share\\X\\1.3.9.P10',
  ];

  const result = resolveBatchTargets(sources, 'E:\\OUT');

  assert.equal(result.length, 2);
  result.forEach((r) => assert.equal(r.status, 'duplicate_in_batch'));
});

test('resolveBatchTargets treats Windows case differences as the same path', () => {
  const sources = [
    'C:\\share\\Foo\\1.3.9.P10',
    'C:\\share\\foo\\1.3.9.P10',
  ];

  const result = resolveBatchTargets(sources, 'E:\\OUT');

  assert.equal(result.length, 2);
  result.forEach((r) => assert.equal(r.status, 'duplicate_in_batch'));
});

test('resolveBatchTargets marks empty or whitespace-only sources as invalid_path', () => {
  const result = resolveBatchTargets(['', '   ', '\\\\', 'C:\\real\\X'], 'E:\\OUT');

  assert.equal(result[0].status, 'invalid_path');
  assert.equal(result[1].status, 'invalid_path');
  assert.equal(result[2].status, 'invalid_path');
  assert.equal(result[3].status, 'ok');
  assert.equal(result[3].finalTarget, 'E:\\OUT\\X');
});

test('resolveBatchTargets handles uneven path depths', () => {
  const sources = [
    '\\\\srv\\share\\deep\\nested\\foo\\1.3.9.P10',
    '\\\\srv\\share\\bar\\1.3.9.P10',
  ];

  const result = resolveBatchTargets(sources, 'E:\\OUT');

  result.forEach((r) => assert.equal(r.status, 'ok'));
  assert.deepEqual(result.map((r) => r.finalTarget), [
    'E:\\OUT\\foo\\1.3.9.P10',
    'E:\\OUT\\bar\\1.3.9.P10',
  ]);
});

test('resolveBatchTargets returns targetRoot directly when tails are all unique', () => {
  const sources = [
    'C:\\share\\Alpha',
    'C:\\share\\Beta',
    'C:\\share\\Gamma',
  ];

  const result = resolveBatchTargets(sources, 'E:\\OUT');

  result.forEach((r) => {
    assert.equal(r.status, 'ok');
    assert.deepEqual(r.disambiguatorSegments, []);
    assert.equal(r.effectiveTargetRoot, 'E:\\OUT');
  });
  assert.deepEqual(result.map((r) => r.finalTarget), [
    'E:\\OUT\\Alpha',
    'E:\\OUT\\Beta',
    'E:\\OUT\\Gamma',
  ]);
});
```

- [ ] **Step 2: Run the tests**

Run: `node --test src/lib/manualCopyBatch.test.mjs`
Expected: PASS — 8 tests passing. If any case-difference test fails, inspect `buildKey` for `toLowerCase()` usage. If duplicate test fails, inspect the K-exhaustion fallback in `resolveBatchTargets`.

- [ ] **Step 3: Commit**

```bash
git add src/lib/manualCopyBatch.test.mjs
git commit -m "test(manual-copy): cover K=2, duplicates, case, invalid, uneven depth, K=0"
```

---

### Task 4: Add i18n keys for the batch UI

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Inspect existing structure**

Run: `grep -n "manualCopy:" src/locales/messages.ts`
Expected output: lines like `1435:    manualCopy: {`, `3293:    manualCopy: {`. The first is `en`, the second is `zh`.

- [ ] **Step 2: Add the en keys**

Inside the `manualCopy: { ... }` block at line ~1435 (en), append the following entries before the closing `},`:

```ts
      // --- Batch mode (multi-line paste support) ---
      batch: {
        placeholder: 'One path per line — multi-line enables batch',
        previewButton: 'Preview Batch ({count})',
        submitButton: 'Start Copy ({count})',
        backToEdit: 'Back to Edit',
        filtersApplyAll: 'Filters below apply to all {count} entries',
        colSource: 'Source path',
        colTarget: 'Final target',
        colStatus: 'Status',
        statusOk: 'OK',
        statusTargetExists: 'Target exists',
        statusSourceMissing: 'Source missing',
        statusDuplicateInBatch: 'Duplicate in batch',
        statusInvalidPath: 'Invalid path',
        toastSuccessAll: 'Queued {count} items',
        toastPartial: 'Queued {ok}/{total}; please fix failed rows and retry',
        emptyPreviewHint: 'Click "Preview Batch" to validate paths before queuing.',
        selectAll: 'Select all',
      },
```

- [ ] **Step 3: Add the zh keys**

Inside the second `manualCopy: { ... }` block at line ~3293 (zh), append the same nested `batch` key with Chinese values before the closing `},`:

```ts
      // --- 批量模式（多行粘贴） ---
      batch: {
        placeholder: '每行一个路径 — 多行自动进入批量',
        previewButton: '预览批次 ({count})',
        submitButton: '开始复制 ({count})',
        backToEdit: '返回编辑',
        filtersApplyAll: '下列过滤将统一应用于全部 {count} 项',
        colSource: '源路径',
        colTarget: '最终目标',
        colStatus: '状态',
        statusOk: '可复制',
        statusTargetExists: '目标已存在',
        statusSourceMissing: '源不存在',
        statusDuplicateInBatch: '批次内重复',
        statusInvalidPath: '路径无效',
        toastSuccessAll: '成功入队 {count} 项',
        toastPartial: '成功入队 {ok}/{total}，失败行请修正后重试',
        emptyPreviewHint: '点击"预览批次"先校验路径再入队。',
        selectAll: '全选',
      },
```

- [ ] **Step 4: Verify type-check passes**

Run: `pnpm check`
Expected: no errors. (vue-tsc compiles `messages.ts` as TS — both blocks must remain shape-compatible.)

- [ ] **Step 5: Commit**

```bash
git add src/locales/messages.ts
git commit -m "i18n(manual-copy): add batch mode keys (en + zh)"
```

---

### Task 5: Swap source input to a textarea and derive batch state

**Files:**
- Modify: `src/components/ManualCopyModal.vue`

- [ ] **Step 1: Update the `<script setup>` block — imports & computed**

At the top of `<script setup lang="ts">` in `src/components/ManualCopyModal.vue` (line 1), add the algorithm import:

```ts
import { resolveBatchTargets, type BatchEntryResolution } from '@/lib/manualCopyBatch';
```

Place it after the existing `import { pushToast, type ToastTone } from '@/composables/useToast';` line.

Find the `const canSubmit = computed(...)` block (~line 62) and *replace* the current source-related refs/computed by inserting these new ones just below `selectedKeywords`:

```ts
// --- Batch mode state ---
// sourceLines: each trimmed non-empty line of the textarea = one batch entry.
const sourceLines = computed(() =>
  sourcePath.value.split(/\r?\n/).map((s) => s.trim()).filter(Boolean),
);
const isBatchMode = computed(() => sourceLines.value.length >= 2);

const batchResolutions = ref<BatchEntryResolution[]>([]);
type BatchPreviewStatus =
  | 'ok'
  | 'target_exists'
  | 'source_missing'
  | 'duplicate_in_batch'
  | 'invalid_path';
const batchRowPreview = ref<Map<string, { status: BatchPreviewStatus; finalTarget: string; errored?: boolean }>>(new Map());
const batchRowChecked = ref<Map<string, boolean>>(new Map());
const batchPreviewOpen = ref(false);
const batchSubmitting = ref(false);
```

Update the existing `canSubmit` to disable when batch mode is active (the batch flow uses its own submit button):

```ts
const canSubmit = computed(
  () =>
    !isBatchMode.value
    && sourcePath.value.trim().length > 0
    && targetRootPath.value.trim().length > 0
    && !isSubmitting.value
    && !existingTargetPreview.value,
);
```

Add a `watch` after the existing `watch([sourcePath, targetRootPath], ...)` (~line 326) to reset batch state when inputs change:

```ts
watch([sourcePath, targetRootPath], () => {
  batchPreviewOpen.value = false;
  batchResolutions.value = [];
  batchRowPreview.value = new Map();
  batchRowChecked.value = new Map();
});
```

- [ ] **Step 2: Update the template — swap input for textarea**

Find the source-path `<input>` block (~line 419) and replace the entire `<input id="manual-copy-source" ... />` element with:

```vue
<textarea
  id="manual-copy-source"
  ref="sourceInputRef"
  v-model="sourcePath"
  rows="3"
  :disabled="isSubmitting || batchSubmitting || Boolean(existingTargetPreview)"
  class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all motion-reduce:transition-none disabled:cursor-not-allowed disabled:bg-slate-100 font-mono text-sm resize-y min-h-[3.25rem] max-h-[12rem]"
  :placeholder="isBatchMode ? t('manualCopy.batch.placeholder') : t('manualCopy.sourcePlaceholder')"
  :aria-invalid="Boolean(inlineError) || undefined"
/>
```

Also update `sourceInputRef`'s type at line 44 from `HTMLInputElement` to the wider type that accepts both:

```ts
const sourceInputRef = ref<HTMLInputElement | HTMLTextAreaElement | null>(null);
```

- [ ] **Step 3: Run the type checker**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/components/ManualCopyModal.vue
git commit -m "feat(manual-copy): swap source input to textarea + batch state scaffolding"
```

---

### Task 6: Add the "Preview Batch" action and inline preview table UI

**Files:**
- Modify: `src/components/ManualCopyModal.vue`

- [ ] **Step 1: Add the `previewBatch()` action in `<script setup>`**

Insert this function just above `async function submitCopy()` (~line 266):

```ts
async function previewBatch() {
  inlineError.value = '';
  batchSubmitting.value = false;

  const target = targetRootPath.value.trim();
  if (!target) {
    inlineError.value = t('manualCopy.fillRequired');
    return;
  }

  const sources = sourceLines.value;
  if (sources.length === 0) {
    inlineError.value = t('manualCopy.fillRequired');
    return;
  }

  const resolutions = resolveBatchTargets(sources, target);
  batchResolutions.value = resolutions;

  const previewMap = new Map<string, { status: BatchPreviewStatus; finalTarget: string; errored?: boolean }>();
  const checkedMap = new Map<string, boolean>();

  await Promise.all(
    resolutions.map(async (r) => {
      if (r.status === 'invalid_path') {
        previewMap.set(r.rawSource, { status: 'invalid_path', finalTarget: '' });
        checkedMap.set(r.rawSource, false);
        return;
      }
      if (r.status === 'duplicate_in_batch') {
        previewMap.set(r.rawSource, { status: 'duplicate_in_batch', finalTarget: r.finalTarget });
        checkedMap.set(r.rawSource, false);
        return;
      }
      try {
        const preview = await previewTemporaryCopy(r.rawSource, r.effectiveTargetRoot);
        const status: BatchPreviewStatus = preview.target_exists ? 'target_exists' : 'ok';
        previewMap.set(r.rawSource, { status, finalTarget: preview.resolved_target_path });
        checkedMap.set(r.rawSource, status === 'ok');
      } catch (error) {
        previewMap.set(r.rawSource, { status: 'source_missing', finalTarget: r.finalTarget, errored: true });
        checkedMap.set(r.rawSource, false);
      }
    }),
  );

  batchRowPreview.value = previewMap;
  batchRowChecked.value = checkedMap;
  batchPreviewOpen.value = true;
}

function backToBatchEdit() {
  batchPreviewOpen.value = false;
}

function toggleAllBatchRows(checked: boolean) {
  const next = new Map<string, boolean>();
  for (const r of batchResolutions.value) {
    if (r.status === 'invalid_path' || r.status === 'duplicate_in_batch') {
      next.set(r.rawSource, false);
      continue;
    }
    next.set(r.rawSource, checked);
  }
  batchRowChecked.value = next;
}

function toggleBatchRow(rawSource: string) {
  const next = new Map(batchRowChecked.value);
  next.set(rawSource, !next.get(rawSource));
  batchRowChecked.value = next;
}

const checkedBatchCount = computed(() => {
  let n = 0;
  for (const checked of batchRowChecked.value.values()) if (checked) n++;
  return n;
});

const allBatchRowsChecked = computed(() => {
  // Selectable = anything except invalid_path (invalid rows have no
  // resolvable target so they cannot be enqueued and stay disabled).
  const selectable = batchResolutions.value.filter((r) => r.status !== 'invalid_path');
  if (selectable.length === 0) return false;
  return selectable.every((r) => batchRowChecked.value.get(r.rawSource) === true);
});

function batchStatusLabel(status: BatchPreviewStatus): string {
  if (status === 'ok') return t('manualCopy.batch.statusOk');
  if (status === 'target_exists') return t('manualCopy.batch.statusTargetExists');
  if (status === 'source_missing') return t('manualCopy.batch.statusSourceMissing');
  if (status === 'duplicate_in_batch') return t('manualCopy.batch.statusDuplicateInBatch');
  return t('manualCopy.batch.statusInvalidPath');
}

function batchStatusClass(status: BatchPreviewStatus): string {
  if (status === 'ok') return 'bg-emerald-100 text-emerald-700 border-emerald-200';
  if (status === 'target_exists') return 'bg-amber-100 text-amber-700 border-amber-200';
  return 'bg-red-100 text-red-700 border-red-200';
}
```

- [ ] **Step 2: Add the batch preview-table template region**

In the template, insert a new region between the source/target input block (after line ~459) and the "Inline error message" block (around line ~462). The exact spot is right after the closing `</div>` that wraps the source+target input pair. Insert:

```vue
<!-- Batch mode preview (only when N >= 2 lines pasted) -->
<div
  v-if="isBatchMode"
  class="rounded-xl border border-blue-200 bg-blue-50/40 px-5 py-4 space-y-4"
>
  <div class="flex items-center justify-between gap-3">
    <span class="text-sm font-medium text-blue-700">
      {{ t('manualCopy.batch.filtersApplyAll', { count: sourceLines.length }) }}
    </span>
    <button
      v-if="!batchPreviewOpen"
      type="button"
      @click="previewBatch"
      :disabled="batchSubmitting || isSubmitting"
      class="inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
    >
      <Play class="w-4 h-4" aria-hidden="true" />
      {{ t('manualCopy.batch.previewButton', { count: sourceLines.length }) }}
    </button>
    <button
      v-else
      type="button"
      @click="backToBatchEdit"
      class="text-sm text-slate-500 hover:text-slate-700"
    >
      {{ t('manualCopy.batch.backToEdit') }}
    </button>
  </div>

  <div v-if="batchPreviewOpen" class="space-y-2">
    <table class="w-full text-sm border border-slate-200 rounded-lg overflow-hidden bg-white">
      <thead class="bg-slate-50 text-slate-600 text-xs uppercase tracking-wide">
        <tr>
          <th class="px-3 py-2 text-left w-10">
            <input
              type="checkbox"
              :checked="allBatchRowsChecked"
              @change="(e) => toggleAllBatchRows((e.target as HTMLInputElement).checked)"
              :aria-label="t('manualCopy.batch.selectAll')"
            />
          </th>
          <th class="px-3 py-2 text-left">{{ t('manualCopy.batch.colSource') }}</th>
          <th class="px-3 py-2 text-left">{{ t('manualCopy.batch.colTarget') }}</th>
          <th class="px-3 py-2 text-left w-32">{{ t('manualCopy.batch.colStatus') }}</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-slate-100">
        <tr
          v-for="r in batchResolutions"
          :key="r.rawSource"
          class="hover:bg-slate-50"
        >
          <td class="px-3 py-2 align-top">
            <input
              type="checkbox"
              :checked="batchRowChecked.get(r.rawSource) === true"
              :disabled="batchRowPreview.get(r.rawSource)?.status === 'invalid_path'"
              @change="toggleBatchRow(r.rawSource)"
            />
          </td>
          <td class="px-3 py-2 font-mono text-xs break-all text-slate-700">{{ r.rawSource }}</td>
          <td class="px-3 py-2 font-mono text-xs break-all text-slate-600">
            {{ batchRowPreview.get(r.rawSource)?.finalTarget || r.finalTarget || '—' }}
          </td>
          <td class="px-3 py-2 align-top">
            <span
              class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium border"
              :class="batchStatusClass(batchRowPreview.get(r.rawSource)?.status ?? r.status as BatchPreviewStatus)"
            >
              {{ batchStatusLabel(batchRowPreview.get(r.rawSource)?.status ?? r.status as BatchPreviewStatus) }}
            </span>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
  <div v-else class="text-xs text-slate-500">
    {{ t('manualCopy.batch.emptyPreviewHint') }}
  </div>
</div>
```

- [ ] **Step 3: Update the footer submit button to handle batch mode**

Find the modal footer (~line 582) and replace the existing single submit `<button>` with a conditional that swaps to a batch-submit button when in batch mode + preview open:

```vue
<button
  v-if="!isBatchMode"
  @click="submitCopy"
  :disabled="!canSubmit"
  class="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors motion-reduce:transition-none disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 focus-visible:ring-offset-1"
>
  <Loader2 v-if="isSubmitting" class="w-4 h-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
  <Play v-else class="w-4 h-4" aria-hidden="true" />
  {{ isSubmitting ? t('manualCopy.submitting') : t('manualCopy.startCopy') }}
</button>
<button
  v-else
  @click="submitBatch"
  :disabled="!batchPreviewOpen || checkedBatchCount === 0 || batchSubmitting"
  class="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors motion-reduce:transition-none disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 focus-visible:ring-offset-1"
>
  <Loader2 v-if="batchSubmitting" class="w-4 h-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
  <Play v-else class="w-4 h-4" aria-hidden="true" />
  {{ t('manualCopy.batch.submitButton', { count: checkedBatchCount }) }}
</button>
```

`submitBatch` is added in Task 7 — leaving it referenced here keeps the template stable for the next commit.

- [ ] **Step 4: Add a placeholder `submitBatch` to keep typecheck happy until Task 7**

Insert this stub immediately after `previewBatch()`:

```ts
async function submitBatch() {
  // Implemented in next commit (Task 7). Placeholder keeps typecheck green.
}
```

- [ ] **Step 5: Run type-check**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/components/ManualCopyModal.vue
git commit -m "feat(manual-copy): add batch preview action and inline preview table UI"
```

---

### Task 7: Implement the batch submission loop

**Files:**
- Modify: `src/components/ManualCopyModal.vue`

- [ ] **Step 1: Replace the `submitBatch` stub with the real implementation**

Replace the placeholder `async function submitBatch() { ... }` body with:

```ts
async function submitBatch() {
  if (!batchPreviewOpen.value || checkedBatchCount.value === 0) return;
  inlineError.value = '';
  batchSubmitting.value = true;
  const exts = [...selectedExtensions.value];
  const kws = [...selectedKeywords.value];

  const ordered = batchResolutions.value.filter(
    (r) => batchRowChecked.value.get(r.rawSource) === true,
  );

  const total = ordered.length;
  let ok = 0;
  const failedRows: string[] = [];

  for (const r of ordered) {
    const preview = batchRowPreview.value.get(r.rawSource);
    const overwrite = preview?.status === 'target_exists';
    try {
      await queueTemporaryCopy(r.rawSource, r.effectiveTargetRoot, overwrite, exts, kws);
      ok++;
    } catch (error) {
      failedRows.push(r.rawSource);
      const nextPreview = new Map(batchRowPreview.value);
      nextPreview.set(r.rawSource, {
        status: 'source_missing',
        finalTarget: r.finalTarget,
        errored: true,
      });
      batchRowPreview.value = nextPreview;
      // Log to console only; toast summary is pushed below.
      console.warn('queueTemporaryCopy failed for', r.rawSource, error);
    }
  }

  batchSubmitting.value = false;

  if (failedRows.length === 0) {
    notify(t('manualCopy.batch.toastSuccessAll', { count: ok }), 'success');
    // Reset and close like the single-source success flow.
    sourcePath.value = '';
    batchPreviewOpen.value = false;
    batchResolutions.value = [];
    batchRowPreview.value = new Map();
    batchRowChecked.value = new Map();
    emit('success');
    emit('close');
  } else {
    notify(
      t('manualCopy.batch.toastPartial', { ok, total }),
      'error',
    );
    // Keep the modal open; failed rows remain visibly red for the user.
  }
}
```

- [ ] **Step 2: Run type-check**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/components/ManualCopyModal.vue
git commit -m "feat(manual-copy): wire batch submission loop to queueTemporaryCopy"
```

---

### Task 8: Extend the modal source-string test for batch mode markers

**Files:**
- Modify: `src/components/ManualCopyModal.test.mjs`

- [ ] **Step 1: Append assertions that the new batch markers exist in the .vue source**

Append to `src/components/ManualCopyModal.test.mjs`:

```mjs
test('manual copy modal switches the source input to a textarea for batch paste', () => {
  // The single-line <input id="manual-copy-source" ...> was replaced with a
  // <textarea> so users can paste multiple paths separated by newlines.
  assert.match(modalSource, /<textarea[^>]*id="manual-copy-source"/);
  assert.doesNotMatch(modalSource, /<input[^>]*id="manual-copy-source"/);
});

test('manual copy modal renders batch preview controls', () => {
  // Preview button + back-to-edit live inside the v-if="isBatchMode" region.
  assert.match(modalSource, /isBatchMode/);
  assert.match(modalSource, /manualCopy\.batch\.previewButton/);
  assert.match(modalSource, /manualCopy\.batch\.submitButton/);
  assert.match(modalSource, /manualCopy\.batch\.backToEdit/);
});

test('manual copy modal exposes batch row status helpers', () => {
  assert.match(modalSource, /batchStatusLabel/);
  assert.match(modalSource, /batchStatusClass/);
  assert.match(modalSource, /manualCopy\.batch\.statusOk/);
  assert.match(modalSource, /manualCopy\.batch\.statusTargetExists/);
  assert.match(modalSource, /manualCopy\.batch\.statusDuplicateInBatch/);
});
```

- [ ] **Step 2: Run the test**

Run: `node --test src/components/ManualCopyModal.test.mjs`
Expected: PASS — 5 tests passing (2 existing + 3 new).

- [ ] **Step 3: Commit**

```bash
git add src/components/ManualCopyModal.test.mjs
git commit -m "test(manual-copy): assert batch mode markers in modal source"
```

---

### Task 9: Verify single-source regression and run full check

**Files:** (none modified — verification only)

- [ ] **Step 1: Run typecheck**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 2: Run all test files we touched**

Run: `node --test src/lib/manualCopyBatch.test.mjs src/components/ManualCopyModal.test.mjs`
Expected: all tests pass (8 algorithm tests + 5 modal markers = 13 total).

- [ ] **Step 3: Manual smoke (dev mode)**

Run: `pnpm tauri dev`

Verify in the running app:
1. Open Manual Copy modal. Paste a **single** path. The textarea behaves like the old input. Press Start Copy → single task enqueued. Target-exists dialog still appears as expected.
2. Paste the 9-line UMS/VMS example from spec § 1. The textarea grows. A blue "filters apply to all 9 entries" banner appears. Click "Preview Batch (9)". After RPC, a preview table appears with 9 OK rows; each `Final target` column matches the verification table.
3. Manually create a target collision (pick a target root that already contains one of the resolved subfolders). Click Preview again: that row shows `Target exists` (amber), is default-unchecked. Check it manually → submit. Watch console; only the checked row should call `queueTemporaryCopy` with `overwriteExisting=true`.
4. Paste a duplicate (same path twice) and a missing path. Confirm `Duplicate in batch` (red, unchecked) and `Source missing` (red, unchecked).
5. After a successful all-OK submit, the modal closes and 9 task records appear in the Tasks page in the same order as the preview table.

- [ ] **Step 4: Final commit (only if anything was tweaked during smoke)**

```bash
git status   # if clean, skip this step entirely
```

If smoke required additional tweaks, commit them with a descriptive message.

- [ ] **Step 5: Final verification build**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: build succeeds and a versioned exe is produced under `src-tauri/target/release/`.

---

## Acceptance Verification

After all tasks complete, confirm against the spec's acceptance criteria:

- [x] Single-line paste behaves identical to today (Task 5 keeps `submitCopy()` flow intact; Task 6 guards `isBatchMode == false`).
- [x] Multi-line paste shows "Preview Batch (N)" then preview table (Task 6).
- [x] Disambiguation matches the verification table for the 9-path UMS/VMS example (Task 2 test).
- [x] Recursive disambiguation handles K=2 (Task 3 test).
- [x] Duplicates flagged as `批次内重复` (Task 3 test).
- [x] Serial enqueue, per-row failure recovery (Task 7).
- [x] Toast summary for partial success (Task 7).
- [x] Algorithm tested via `node:test` with 8 cases (Tasks 1-3).
- [x] i18n keys present in en + zh (Task 4).
- [x] No Rust changes (verified by grepping `src-tauri` in `git diff`).
