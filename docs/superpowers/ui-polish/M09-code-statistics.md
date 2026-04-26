# M09 — Code Statistics Polish

- **Phase**: 3 (after M01)
- **Risk**: High — page is 1887 lines with charts and tree traversal; visual changes only
- **Files**:
  - `src/pages/CodeStatisticsPage.vue`
  - `src/components/CodeStatisticsScopeTreeNode.vue` (142 lines)

---

## Goal

A read-only stats dashboard. Polish charts, tree, and metric cards. No data calculation change.

---

## Issues

1. **Chart accessibility** — every chart needs:
   - `aria-label` summarizing the data trend
   - Keyboard-reachable data points (or at minimum a tabular alternative below)
   - Color palette that's colorblind-safe (test with simulator)
   - Legend visible (not hidden behind scroll fold)
   - Tooltip on tap/hover with exact values
2. **Tree node rendering** — `CodeStatisticsScopeTreeNode.vue` likely uses indentation. Add `role="treeitem"`, `aria-expanded`, `aria-level` for proper screen reader support.
3. **Tree expand/collapse** — keyboard arrow keys (Right/Left) should expand/collapse, Up/Down navigates siblings.
4. **Initial load** — heavy computation, suspected. Add skeleton for the first paint.
5. **Metric cards** — show a number + label. Use `font-variant-numeric: tabular-nums` so updating numbers don't shift width.
6. **Number formatting** — locale-aware (1,234 vs 1.234). Use `Intl.NumberFormat` with locale.
7. **Empty state when no project loaded** — drop in `Empty.vue`.
8. **Color palette for languages** — many distinct languages. Use a palette generator (e.g. d3 categorical) or a fixed list of 16+ colors with WCAG-checked contrast.
9. **Chart reflow on window resize** — confirm.
10. **Scrollbar consistency** — apply M03's scrollbar-terminal class for the tree scroll area.
11. **Data export option** — if export exists, ensure file-name is locale-aware.
12. **Keep-alive note** — page is in keep-alive list (App.vue). Confirm no setInterval / no listeners leak when the page is in cache mode.

### i18n keys (new)

| Key | zh | en |
|---|---|---|
| `codeStatistics.empty.noProject` | 未加载项目 | No project loaded |
| `codeStatistics.empty.actionLoad` | 选择项目 | Select a project |
| `codeStatistics.aria.scopeTree` | 项目作用域树 | Project scope tree |
| `codeStatistics.aria.metricCard` | 指标卡片：{label} {value} | Metric: {label} {value} |
| `codeStatistics.tooltip.exact` | 精确值 | Exact value |

---

## Out of scope

- DO NOT change calculation logic.
- DO NOT swap chart library.
- DO NOT change data fetching.
- DO NOT change scope-tree data shape.

---

## Verification

1. `pnpm check` clean.
2. Tab into the tree → arrow keys expand/collapse, navigate siblings.
3. Tab to a chart → Tab through legend items if interactive.
4. Switch locale → numbers reformat (1,234 ↔ 1234).
5. Resize window → charts reflow.
6. `prefers-reduced-motion` → chart entrance animations skip.

---

## Reporting back

Under 200 words.
