# M03 — Console & History Polish

- **Phase**: 2 (after M01)
- **Risk**: Low
- **Files**: `src/pages/MainConsole.vue` (106 lines), `src/pages/HistoryPage.vue` (155 lines)

---

## Goal

Two simple display pages. Tighten typography, add proper empty/loading states, fix scrollbar consistency. No structural change.

---

## Issues

### MainConsole.vue

1. **No empty state.** When `logs` is empty (fresh install), the terminal is blank. User unsure if app is broken or just idle.
2. **Custom scrollbar styling is local.** Other scrollable terminals reuse this look-and-feel by copy-paste. Move to a Tailwind utility or global CSS class so consistency is centralized.
3. **Auto-scroll threshold (60px from bottom) is a magic number.** Pull it to a named constant at the top of `<script setup>` so it's grep-able.
4. **Log type icons (`CheckCircle2`, `AlertCircle`, etc.) lack `aria-label`.** Each log line should announce its type for screen readers (e.g. `aria-label="success"` on the icon).
5. **Timestamp format is locale-naive.** `new Date().toLocaleTimeString()` ignores `useI18n().locale`. Fix: pass `locale.value === 'zh' ? 'zh-CN' : 'en-US'`.
6. **No "clear console" affordance.** History page has Clear; console has none. Add a small `Eraser` icon button in the OS-window-chrome area.
7. **Color-only signals.** Errors and successes differentiated only by icon color. Add a visually-hidden `<span class="sr-only">` with the type name.

### HistoryPage.vue

8. **No pagination / no virtualization.** Inventory said "assumes history list is manageable size". Backend caps at 100 entries (per CLAUDE.md), so OK — but add a small footer line "显示最近 N 条" so users know.
9. **Expanded entry's file list has no visual scroll cap.** A history entry with 200 files explodes the card. Cap at `max-h-80 overflow-y-auto` with the same scrollbar style as console.
10. **`getIcon` / `getIconColor` map literal `event.kind` strings.** Unknown kind defaults silently — add a fallback icon (`HelpCircle` muted slate) so a future event type is visible.
11. **Date display uses `new Date(...).toLocaleString()`** without locale. Same fix as Console issue #5.
12. **Empty state.** Already uses `Empty.vue` ✓. After M01, switch to the extended Empty with `actionLabel="刷新"` / `"Refresh"` and emit refresh.
13. **Clear button is destructive without confirm.** Add a small inline confirm (a second click within 3s commits, or use a tiny dropdown asking "确定清空？").
14. **Hover state on history rows is missing.** Add `hover:bg-slate-50` so users get feedback that the row is interactive (it expands on click).

---

## Recommended fixes

Map 1:1 to issues. The general direction:

- Move scrollbar styling to a global `.scrollbar-terminal` class in App.vue's `<style>` or a new `src/styles/scrollbar.css`.
- Add `EMPTY_STATE_THRESHOLD` const at the top of `<script setup>`.
- Use new `Empty.vue` with action prop (M01 dependency).
- Use new locale-aware date helper. Suggest creating `src/lib/formatters.ts` with `formatDateTime(iso, locale)` to avoid duplicating the ternary across files.
- Confirmation pattern: a transient secondary state on the Clear button (`待确认`) for 3s before it commits.

### New i18n keys (zh + en)

| Key | zh | en |
|---|---|---|
| `console.empty.title` | 暂无日志 | No logs yet |
| `console.empty.description` | 启动调度器或运行扫描后将在此显示 | Logs appear here once the scheduler runs or you trigger a scan |
| `console.clear` | 清空 | Clear |
| `console.logKind.info` | 信息 | Info |
| `console.logKind.success` | 成功 | Success |
| `console.logKind.error` | 错误 | Error |
| `console.logKind.command` | 命令 | Command |
| `history.recentN` | 显示最近 {n} 条 | Showing the most recent {n} entries |
| `history.clearConfirm` | 再次点击确认清空 | Click again to confirm |
| `history.empty.actionLabel` | 刷新 | Refresh |

---

## Out of scope

- DO NOT change the log data structure or event subscription.
- DO NOT change the `addLog` / `syncTaskRecordByLog` logic in `store.ts`.
- DO NOT add filtering / search to console (separate feature).
- DO NOT add export-to-file (separate feature).

---

## Verification

1. `pnpm check` clean.
2. Boot app, verify console renders empty state on first launch.
3. Run a scan; verify logs appear with icons, timestamps, and screen-reader labels.
4. Open History; verify date locale switches when language toggle changes.
5. Click Clear once → text changes to "再次点击确认"; wait 3s → reverts. Click twice within 3s → list clears.
6. Tab through Console and History — focus rings visible on Clear button and history rows.

---

## Reporting back

Files changed, i18n keys added, manual smoke results. Under 150 words.
