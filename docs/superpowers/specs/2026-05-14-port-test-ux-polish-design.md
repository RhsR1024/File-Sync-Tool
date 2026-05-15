# Network Tools — Port Test UX Polish (Design)

**Date**: 2026-05-14
**Status**: Draft / awaiting user review
**Scope**: Frontend-only change to `PortTestTab.vue` + sibling presentation lib. No backend changes.
**Branching**: Edits land directly on `main`.

---

## 1. Goals

User-observed problems with today's port test:

1. Selecting `all` (= 65535 ports) makes the UI very laggy.
2. Cells are tiny; port numbers are not visible at large scan sizes.
3. The table view's default filter shows `all` rows including thousands of closed/gray rows, which dominate the result list while the few open ports are buried.

This redesign reframes the visualization around the user's mental model:
**the goal of a port scan is to find the open ports**. Closed ports are noise
that should be hidden by default but still reachable.

---

## 2. View Model

Two top-level view modes survive (`grid` / `table` buttons in the top-right of
the result area), but the `grid` view's *content* now auto-adapts to the
scanned port count.

| Scan size | grid view content | table view content |
|---|---|---|
| `totalPorts ≤ 1024` (small scan) | **Overview grid** — every scanned port is a cell. Open=emerald, closed=slate-200, scanning=amber pulse, waiting=slate-700. Cells enlarged to ~56px; port number centered inside. Same semantics as today, just bigger. | Table rows; default filter = `open` (was `all`). |
| `totalPorts > 1024` (large scan, e.g. `all`) | **Open-port card grid** — only OPEN ports are rendered. Each is a ~96×64 emerald card with port number (bold, top), service name (mid), latency (bottom-small). Cards stream in live as ports are discovered. | Same — table rows, default filter = `open`. |

**Threshold**: 1024 — same value as today's `showCellLabels` cutoff. Extracted
into `const LARGE_SCAN_GRID_THRESHOLD = 1024` for clarity. When the user runs
multiple scans of different sizes in one session, the grid auto-switches
between the two forms based on the current scan's `totalPorts`.

Mode selection (`grid` / `table`) remains a manual user toggle. On a fresh
scan, we do not change the user's previously chosen view; if they had it on
`table`, it stays on `table`.

---

## 3. Open-port card grid (large scan)

Visual:

```
┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐
│  22  │  │  80  │  │ 443  │  │ 3306 │
│ ssh  │  │ http │  │https │  │mysql │
└──────┘  └──────┘  └──────┘  └──────┘
  0.8ms     1.2ms     1.1ms     3.4ms
```

- Container: `display: grid; grid-template-columns: repeat(auto-fill, minmax(96px, 1fr)); gap: 12px`.
- Card: `rounded-lg border border-emerald-200 bg-emerald-50 p-3 flex flex-col items-center gap-1`.
- Lines (top → bottom):
  - **Port** — `text-lg font-mono font-bold text-emerald-700`.
  - **Service name** — `text-[11px] text-slate-500`; if absent, render `—`.
  - **Latency** — `text-[10px] text-slate-400 tabular-nums`; if `null`, render `—`.
- **Live append**: existing `pendingRows` + 80ms `scheduleFlush()` batching is unchanged. The render layer subscribes to `openOnlyCells` (a new computed = `allRows.value.filter(r => r.open)` sorted ascending by port).
- **Empty state during scan**: while `isLoading == true` and `openOnlyCells.length == 0`, render `<Empty>` (existing component) with i18n text `扫描中... 暂未发现开放端口 ({scanned} / {total})` / `Scanning... no open ports found yet ({scanned} / {total})`.
- **Completed scan with zero open**: same `<Empty>` but text `扫描完成，未发现开放端口 ({total} 个已扫描)` / `Scan complete, no open ports found ({total} scanned)`.
- The result-header line (`X / N | open Y | closed Z`) and progress bar above the grid are unchanged — they remain the single source of truth for "how thorough is the scan".
- Hover tooltip: not needed in card view (all info already visible). Disable `showTooltip` for cards.

**DOM cost**: `total open ports` cards, typically dozens for a real `all`-mode
scan. 99.9 % reduction vs. today's 65535 cell pre-allocation.

---

## 4. Overview grid (small scan)

- Trigger: `totalPorts ≤ 1024`. Same condition as today's `showCellLabels`.
- Container: `grid-template-columns: repeat(auto-fill, minmax(56px, 1fr)); gap: 6px`. (Today: 36px / 4px.)
- Cell base: `rounded-md flex aspect-square items-center justify-center text-xs font-mono font-medium cursor-default select-none transition-colors`.
- Colors: unchanged (open=`bg-emerald-500 text-white`, closed=`bg-slate-200 text-slate-400`, scanning=`bg-amber-400 text-white animate-pulse`, waiting=`bg-slate-700 text-slate-500`).
- Port number always visible inside cell.
- Hover tooltip unchanged.
- Legend strip (open / closed / scanning / waiting color chips) unchanged.

For very small scans (e.g. 3 ports), `minmax(56px, 1fr)` still produces neat
columns; the `1fr` upper bound prevents one cell from stretching across the row.

---

## 5. Table view default

- `tableFilter` initial value: `'open'` (was `'all'`).
- `startTest()` resets `tableFilter.value = 'open'` (was `'all'`).
- Radio group (`all` / `open` / `closed`) UI unchanged — closed ports remain a click away.
- Result-summary copy button & row rendering unchanged.

---

## 6. Files Touched

**Modified**:
- `src/components/network/PortTestTab.vue`
  - Import `buildOpenPortCards` (new) in addition to `buildPortGridCells`.
  - Define `LARGE_SCAN_GRID_THRESHOLD = 1024` constant.
  - New `openOnlyCells` computed.
  - `isLargeScan` computed = `totalPorts.value > LARGE_SCAN_GRID_THRESHOLD`.
  - `tableFilter` ref initial `'open'`; `startTest()` resets to `'open'`.
  - Template: split grid section into two `v-if` branches by `isLargeScan`.
  - Card markup as in § 3; small-scan markup tweaked per § 4.
- `src/lib/portTestPresentation.ts`
  - Add `OpenPortCard` interface and `buildOpenPortCards(resultRows): OpenPortCard[]`.
  - Existing `buildPortGridCells` / `filterPortRows` untouched.
- `src/locales/messages.ts`
  - Add `networkTools.port.scanningNoOpenYet` (en + zh).
  - Add `networkTools.port.completeNoOpen` (en + zh).

**New**:
- (none — extend existing test files.)

**Tests modified/added**:
- `src/lib/portTestPresentation.test.mjs` (existing — extend) — add 3 cases for `buildOpenPortCards`: empty input, mixed open/closed, ordering ascending. Runner: `node --test`.
- Component-level smoke test for `PortTestTab.vue` is out of scope (no existing test; not adding one for this UX-only change).

**No Rust changes.**

---

## 7. New Pure Function

```ts
export interface OpenPortCard {
  port: number;
  name: string | null;     // IANA service name, or null
  latencyMs: number | null;
}

export function buildOpenPortCards(
  resultRows: Map<number, SinglePortResult>,
): OpenPortCard[] {
  const open: OpenPortCard[] = [];
  for (const row of resultRows.values()) {
    if (!row.open) continue;
    open.push({ port: row.port, name: row.name ?? null, latencyMs: row.latencyMs });
  }
  open.sort((a, b) => a.port - b.port);
  return open;
}
```

---

## 8. Out of Scope

- Backend changes (today's streaming `port-test-result` event is fine).
- Virtual scrolling for the open-port grid (typical open count ≤ ~100, even on aggressive scans).
- Per-port grouping or service-category coloring.
- Manual "show all 65535" mode (closed ports still reachable via table view).
- Performance work on the scan itself.

---

## 9. Acceptance Criteria

- [ ] Scanning `all` (65535 ports): UI remains responsive; results stream in as cards; cards show port number, service, latency.
- [ ] Scanning a small range (e.g. `80,443,8080`): overview grid renders with enlarged cells; port number visible in each cell.
- [ ] Switching to table view defaults to filter `open`; closed ports are reachable via the radio toggle.
- [ ] No 65535 DOM nodes are mounted at any point during a `all` scan.
- [ ] Service name and latency in cards correctly fall back to `—` when absent.
- [ ] Empty state during in-progress large scan shows "scanning... no open ports found yet ({scanned}/{total})".
- [ ] `buildOpenPortCards` unit tests pass.
