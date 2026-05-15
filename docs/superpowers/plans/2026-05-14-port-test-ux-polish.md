# Network Tools — Port Test UX Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the laggy `all`-scan experience and re-center the port test UI around open ports — render only open-port cards when scanning more than 1024 ports, enlarge cells in small scans, and default the table-view filter to `open`.

**Architecture:** Add a new pure helper `buildOpenPortCards()` to `src/lib/portTestPresentation.ts` alongside the existing `buildPortGridCells()`. In `PortTestTab.vue`, branch the grid-view template by a single `isLargeScan` computed (based on `LARGE_SCAN_GRID_THRESHOLD = 1024`): small-scan path renders the existing overview grid with enlarged cells; large-scan path renders only-open cards in a wider grid. Default `tableFilter` flips from `'all'` to `'open'`. **Backend is untouched.**

**Tech Stack:** Vue 3 (`<script setup>` + Composition API), TypeScript, Tailwind CSS 4, Tauri 2 event streaming, `node:test` for unit tests, `lucide-vue-next` icons, `vue-i18n` for localization.

**Spec:** `docs/superpowers/specs/2026-05-14-port-test-ux-polish-design.md`

**Branching:** Land directly on `main` (per user preference for small UX tweaks).

---

## Planned File Structure

**Modified**
- `src/lib/portTestPresentation.ts` — add `OpenPortCard` type and `buildOpenPortCards()` pure function.
- `src/lib/portTestPresentation.test.mjs` — add 3 `node:test` cases for `buildOpenPortCards`.
- `src/components/network/PortTestTab.vue` — extract `LARGE_SCAN_GRID_THRESHOLD` constant, add `isLargeScan` / `openPortCards` computed, change `tableFilter` default to `'open'`, split grid template into small-scan vs large-scan branches, enlarge small-scan cells.
- `src/locales/messages.ts` — add `networkTools.port.scanningNoOpenYet` and `networkTools.port.completeNoOpen` keys in `en` and `zh`.

**Untouched**
- All Rust files (`src-tauri/...`).
- The Tauri streaming events (`port-test-result`, `port-test-complete`) — already working.

---

### Task 1: Add `buildOpenPortCards` pure function with failing tests

**Files:**
- Modify: `src/lib/portTestPresentation.ts`
- Modify: `src/lib/portTestPresentation.test.mjs`

- [ ] **Step 1: Write the failing tests**

Append to `src/lib/portTestPresentation.test.mjs`:

```mjs
test('buildOpenPortCards returns an empty array when no ports have been scanned', () => {
  const result = buildOpenPortCards(new Map());
  assert.deepEqual(result, []);
});

test('buildOpenPortCards keeps only open ports and drops closed entries', () => {
  const rows = new Map([
    [22, { port: 22, open: true, latencyMs: 0.8, name: 'SSH' }],
    [23, { port: 23, open: false, latencyMs: null, name: '' }],
    [80, { port: 80, open: true, latencyMs: 1.2, name: 'HTTP' }],
  ]);

  const result = buildOpenPortCards(rows);

  assert.equal(result.length, 2);
  assert.deepEqual(result.map((c) => c.port), [22, 80]);
});

test('buildOpenPortCards sorts open ports ascending and normalizes empty service names to null', () => {
  const rows = new Map([
    [443, { port: 443, open: true, latencyMs: 1.1, name: 'HTTPS' }],
    [22, { port: 22, open: true, latencyMs: 0.8, name: '' }],
    [3306, { port: 3306, open: true, latencyMs: 3.4, name: 'MySQL' }],
  ]);

  const result = buildOpenPortCards(rows);

  assert.deepEqual(result, [
    { port: 22, name: null, latencyMs: 0.8 },
    { port: 443, name: 'HTTPS', latencyMs: 1.1 },
    { port: 3306, name: 'MySQL', latencyMs: 3.4 },
  ]);
});
```

Also add `buildOpenPortCards` to the import line at the top of the file:

```mjs
import {
  buildOpenPortCards,
  buildPortGridCells,
  filterPortRows,
  parsePorts,
} from './portTestPresentation.ts';
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node --test src/lib/portTestPresentation.test.mjs`
Expected: FAIL with "buildOpenPortCards is not a function" or "buildOpenPortCards is not defined" on the first new test.

- [ ] **Step 3: Implement `buildOpenPortCards`**

Edit `src/lib/portTestPresentation.ts`. Append a new exported interface and function after `filterPortRows`:

```ts
export interface OpenPortCard {
  port: number;
  name: string | null;
  latencyMs: number | null;
}

export function buildOpenPortCards(
  rows: ReadonlyMap<number, SinglePortResult>,
): OpenPortCard[] {
  const open: OpenPortCard[] = [];
  for (const row of rows.values()) {
    if (!row.open) continue;
    open.push({
      port: row.port,
      name: row.name ? row.name : null,
      latencyMs: row.latencyMs,
    });
  }
  open.sort((a, b) => a.port - b.port);
  return open;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node --test src/lib/portTestPresentation.test.mjs`
Expected: PASS — 8 tests passing (5 existing + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src/lib/portTestPresentation.ts src/lib/portTestPresentation.test.mjs
git commit -m "feat(network-tools): add buildOpenPortCards helper for live open-port view"
```

---

### Task 2: Add the two new i18n keys (en + zh)

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Add the en keys**

Inside the `port: { ... }` block at line ~1397 (en, ends with `emptyDescription: '...'`), insert two new keys just before the closing `},`:

```ts
        scanningNoOpenYet: 'Scanning... no open ports found yet ({scanned} / {total})',
        completeNoOpen: 'Scan complete, no open ports found ({total} scanned)',
```

The block should look like (showing surrounding context):

```ts
        emptyTitle: 'Pick a host and one or more ports to start',
        emptyDescription: 'Common presets can quickly seed web, SSH, or database ports.',
        scanningNoOpenYet: 'Scanning... no open ports found yet ({scanned} / {total})',
        completeNoOpen: 'Scan complete, no open ports found ({total} scanned)',
      },
```

- [ ] **Step 2: Add the zh keys**

Inside the `port: { ... }` block at line ~3255 (zh, ends with `emptyDescription: '...'`), insert two new keys just before the closing `},`:

```ts
        scanningNoOpenYet: '扫描中… 暂未发现开放端口 ({scanned} / {total})',
        completeNoOpen: '扫描完成，未发现开放端口（共扫描 {total} 个）',
```

- [ ] **Step 3: Verify type-check passes**

Run: `pnpm check`
Expected: no errors. Both blocks must remain shape-compatible (vue-tsc enforces this).

- [ ] **Step 4: Commit**

```bash
git add src/locales/messages.ts
git commit -m "i18n(network-tools): add port test no-open-ports keys (en + zh)"
```

---

### Task 3: Default the table-view filter to `open`

**Files:**
- Modify: `src/components/network/PortTestTab.vue`

- [ ] **Step 1: Change the `tableFilter` ref default**

Find this line (~line 64) in `<script setup>`:

```ts
const tableFilter = ref<PortTableFilter>('all');
```

Replace with:

```ts
const tableFilter = ref<PortTableFilter>('open');
```

- [ ] **Step 2: Change the `startTest()` reset**

Find this line (~line 228) inside `async function startTest()`:

```ts
    tableFilter.value = 'all';
```

Replace with:

```ts
    tableFilter.value = 'open';
```

- [ ] **Step 3: Verify type-check passes**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/components/network/PortTestTab.vue
git commit -m "feat(network-tools): default port-test table filter to open-only"
```

---

### Task 4: Add `LARGE_SCAN_GRID_THRESHOLD`, `isLargeScan`, and `openPortCards`

**Files:**
- Modify: `src/components/network/PortTestTab.vue`

- [ ] **Step 1: Import `buildOpenPortCards`**

At the top of `<script setup>`, find the existing import block:

```ts
import {
  buildPortGridCells,
  filterPortRows,
  parsePorts,
  type PortGridCell,
  type PortGridState,
  type PortTableFilter,
} from '../../lib/portTestPresentation';
```

Replace with:

```ts
import {
  buildOpenPortCards,
  buildPortGridCells,
  filterPortRows,
  parsePorts,
  type OpenPortCard,
  type PortGridCell,
  type PortGridState,
  type PortTableFilter,
} from '../../lib/portTestPresentation';
```

- [ ] **Step 2: Add the threshold constant**

Find `const LARGE_SCAN_THRESHOLD = 1000;` (~line 30) and `const LARGE_SCAN_TIMEOUT_MS = 500;` (~line 31). Below them, add a new constant for the grid-form switch:

```ts
const LARGE_SCAN_GRID_THRESHOLD = 1024;
```

(Reuses the same threshold today's `showCellLabels` uses; we keep it as its own named constant so the two semantics — "drop labels because cells are tiny" vs "drop the full-overview grid in favor of open-only cards" — share a deliberate value.)

- [ ] **Step 3: Add `isLargeScan` and `openPortCards` computed**

Just below the existing `gridStyle` / `gridCellBaseClass` computeds (~line 112), add:

```ts
const isLargeScan = computed(() => totalPorts.value > LARGE_SCAN_GRID_THRESHOLD);
const openPortCards = computed<OpenPortCard[]>(() => buildOpenPortCards(resultRows.value));
```

- [ ] **Step 4: Update `gridStyle` to widen cells in the small-scan grid**

Replace the existing `gridStyle` computed (~line 105):

```ts
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(auto-fill, minmax(${showCellLabels.value ? '36px' : '10px'}, 1fr))`,
}));
```

with the enlarged version:

```ts
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(auto-fill, minmax(${showCellLabels.value ? '56px' : '10px'}, 1fr))`,
  gap: '6px',
}));
```

- [ ] **Step 5: Verify type-check passes**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/components/network/PortTestTab.vue
git commit -m "feat(network-tools): add isLargeScan computed and enlarge small-scan grid cells"
```

---

### Task 5: Branch the grid template — overview grid vs open-port cards

**Files:**
- Modify: `src/components/network/PortTestTab.vue`

- [ ] **Step 1: Replace the grid view block in the template**

Find the existing block (~lines 548-582):

```vue
<div v-if="viewMode === 'grid'" class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
  <div class="max-h-[520px] overflow-auto pr-1">
    <div class="grid gap-1" :style="gridStyle">
      <div
        v-for="cell in gridCells"
        :key="cell.port"
        :class="[gridCellBaseClass, portCellClass(cell.state)]"
        :aria-label="`${cell.port} ${portStatusLabel(cell.state)}`"
        @mouseenter="showTooltip(cell, $event)"
        @mouseleave="hideTooltip"
      >
        <span v-if="showCellLabels">{{ cell.port }}</span>
      </div>
    </div>
  </div>

  <div class="mt-3 flex flex-wrap gap-3 border-t border-slate-100 pt-3">
    <span class="flex items-center gap-1.5 text-xs text-slate-500">
      <span class="inline-block h-3 w-3 rounded bg-emerald-500"></span>
      {{ t('networkTools.port.open') }}
    </span>
    <span class="flex items-center gap-1.5 text-xs text-slate-500">
      <span class="inline-block h-3 w-3 rounded bg-slate-200"></span>
      {{ t('networkTools.port.closed') }}
    </span>
    <span class="flex items-center gap-1.5 text-xs text-slate-500">
      <span class="inline-block h-3 w-3 rounded bg-amber-400"></span>
      {{ t('networkTools.port.scanning') }}
    </span>
    <span class="flex items-center gap-1.5 text-xs text-slate-500">
      <span class="inline-block h-3 w-3 rounded bg-slate-700"></span>
      {{ t('networkTools.port.waiting') }}
    </span>
  </div>
</div>
```

Replace with a `v-if`-branched version that picks the right grid for the current scan size:

```vue
<div v-if="viewMode === 'grid'" class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
  <!-- Large-scan (>1024 ports): show only open-port cards as they stream in -->
  <template v-if="isLargeScan">
    <div v-if="openPortCards.length > 0" class="max-h-[520px] overflow-auto pr-1">
      <div
        class="grid gap-3"
        :style="{ gridTemplateColumns: 'repeat(auto-fill, minmax(96px, 1fr))' }"
      >
        <div
          v-for="card in openPortCards"
          :key="card.port"
          class="rounded-lg border border-emerald-200 bg-emerald-50 p-3 flex flex-col items-center gap-1"
        >
          <div class="text-lg font-mono font-bold text-emerald-700 leading-none">{{ card.port }}</div>
          <div class="text-[11px] text-slate-500 text-center leading-tight truncate w-full">
            {{ card.name || '—' }}
          </div>
          <div class="text-[10px] text-slate-400 tabular-nums">
            {{ card.latencyMs !== null ? `${card.latencyMs.toFixed(1)} ms` : '—' }}
          </div>
        </div>
      </div>
    </div>
    <Empty
      v-else
      :title="isLoading
        ? t('networkTools.port.scanningNoOpenYet', { scanned: scannedCount, total: totalPorts })
        : t('networkTools.port.completeNoOpen', { total: totalPorts })"
      dashed
    />
  </template>

  <!-- Small-scan (≤1024 ports): show the full overview grid with enlarged labelled cells -->
  <template v-else>
    <div class="max-h-[520px] overflow-auto pr-1">
      <div class="grid" :style="gridStyle">
        <div
          v-for="cell in gridCells"
          :key="cell.port"
          :class="[gridCellBaseClass, portCellClass(cell.state)]"
          :aria-label="`${cell.port} ${portStatusLabel(cell.state)}`"
          @mouseenter="showTooltip(cell, $event)"
          @mouseleave="hideTooltip"
        >
          <span v-if="showCellLabels">{{ cell.port }}</span>
        </div>
      </div>
    </div>

    <div class="mt-3 flex flex-wrap gap-3 border-t border-slate-100 pt-3">
      <span class="flex items-center gap-1.5 text-xs text-slate-500">
        <span class="inline-block h-3 w-3 rounded bg-emerald-500"></span>
        {{ t('networkTools.port.open') }}
      </span>
      <span class="flex items-center gap-1.5 text-xs text-slate-500">
        <span class="inline-block h-3 w-3 rounded bg-slate-200"></span>
        {{ t('networkTools.port.closed') }}
      </span>
      <span class="flex items-center gap-1.5 text-xs text-slate-500">
        <span class="inline-block h-3 w-3 rounded bg-amber-400"></span>
        {{ t('networkTools.port.scanning') }}
      </span>
      <span class="flex items-center gap-1.5 text-xs text-slate-500">
        <span class="inline-block h-3 w-3 rounded bg-slate-700"></span>
        {{ t('networkTools.port.waiting') }}
      </span>
    </div>
  </template>
</div>
```

- [ ] **Step 2: Enlarge `gridCellBaseClass` for the small-scan grid**

Find this computed (~line 108):

```ts
const gridCellBaseClass = computed(() =>
  showCellLabels.value
    ? 'rounded flex aspect-square items-center justify-center text-[10px] font-mono font-medium cursor-default select-none transition-colors'
    : 'rounded-[2px] aspect-square min-h-2 cursor-default transition-colors',
);
```

Replace with:

```ts
const gridCellBaseClass = computed(() =>
  showCellLabels.value
    ? 'rounded-md flex aspect-square items-center justify-center text-xs font-mono font-medium cursor-default select-none transition-colors'
    : 'rounded-[2px] aspect-square min-h-2 cursor-default transition-colors',
);
```

- [ ] **Step 3: Verify type-check passes**

Run: `pnpm check`
Expected: no errors. (Watch for any unused `showCellLabels` lint warning — should remain in use by `<span v-if="showCellLabels">` and the `gridCellBaseClass` computed.)

- [ ] **Step 4: Commit**

```bash
git add src/components/network/PortTestTab.vue
git commit -m "feat(network-tools): branch grid view into small-overview vs large-scan open-card forms"
```

---

### Task 6: Verify and run all tests

**Files:** (none modified — verification only)

- [ ] **Step 1: Run typecheck**

Run: `pnpm check`
Expected: no errors.

- [ ] **Step 2: Run all touched tests**

Run: `node --test src/lib/portTestPresentation.test.mjs`
Expected: PASS — 8 tests passing.

- [ ] **Step 3: Manual smoke (dev mode)**

Run: `pnpm tauri dev`

Verify in the running app, under Tools → Network Tools → Port Test:

1. **Small scan, default Web preset (`80,443`)**: pick a host (e.g. `127.0.0.1`). Click Start. Grid view shows enlarged cells (~56px) with port numbers visible. Legend below still lists open / closed / scanning / waiting.
2. **Mid-size scan (`1-1024`)**: ports show as before but with bigger cells. UI stays responsive throughout.
3. **All-port scan (`all`)**: scan a localhost or LAN host. The UI must NOT lag — there are no 65535 cells in the DOM. As each open port is discovered, a green card with port number, service name, and latency appears in real time. While none are discovered yet, the `<Empty>` panel shows `扫描中… 暂未发现开放端口 (X / 65535)` updating live.
4. **Switch to Table view**: filter radio defaults to `仅开放`; the table lists only open ports. Manually flip to `仅关闭` to confirm closed ports remain accessible.
5. **Re-run the scan**: filter resets back to `仅开放`. Confirmed via flipping to `全部` mid-scan then starting a new run.
6. **Empty open after complete**: scan a port range with no open ports (e.g. random unused ports). Once the scan completes, the panel shows `扫描完成，未发现开放端口（共扫描 N 个）`.

- [ ] **Step 4: Final verification build**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: build succeeds and a versioned exe is produced under `src-tauri/target/release/`.

---

## Acceptance Verification

After all tasks complete, confirm against the spec's acceptance criteria:

- [x] `all` scan remains responsive (Task 5 — open-only cards instead of 65535 cells).
- [x] `totalPorts > 1024` renders open-port cards with port / service / latency (Task 5).
- [x] `totalPorts ≤ 1024` keeps overview grid with enlarged cells (Tasks 4 & 5).
- [x] Table-view filter defaults to `open`; user can flip to `all` / `closed` (Task 3).
- [x] Empty-state messaging during in-progress and post-scan paths (Task 5).
- [x] `buildOpenPortCards` covered by 3 unit tests (Task 1).
- [x] i18n keys exist in en + zh (Task 2).
- [x] No Rust changes (verified by grepping `src-tauri` in `git diff`).
