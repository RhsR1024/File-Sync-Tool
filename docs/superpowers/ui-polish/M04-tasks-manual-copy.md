# M04 — Tasks & Manual Copy Polish

- **Phase**: 2 (after M01)
- **Risk**: Medium — TaskStatusPage is the operational dashboard; high visibility
- **Files**:
  - `src/pages/TaskStatusPage.vue` (498 lines)
  - `src/pages/ManualCopyPage.vue` (185 lines)
  - `src/components/ManualCopyModal.vue` (628 lines)
  - `src/components/TaskGroupsTable.vue`
  - `src/components/TaskGroupDetailPanel.vue`

---

## Goal

The "doing the work" pages — make them feel responsive, predictable, and readable. Migrate the local toast helper to the shared `useToast` from M01.

---

## Issues

### TaskStatusPage.vue

1. **Custom `showToast` (2.4s)** duplicates M01 primitives. Migrate to `useToast.pushToast()`.
2. **Top control row: Play/Stop/Refresh buttons.** Confirm each has `aria-label`, `title` tooltip, and disabled state contrast.
3. **No skeleton for initial `TaskGroupsTable` fetch.** Fresh launch shows blank space until events arrive. Drop in `<LoadingSkeleton variant="list-row" :count="3" />`.
4. **`pendingRetryRequest` retry preview pattern is indirect.** UX-wise, when user clicks Retry, they get a modal asking "target exists, replace?" — confirm the modal has clear "Yes/No" with Yes being destructive-styled.
5. **Manual copy modal integrated.** State (`manualCopyOpen` ref) is fine, but verify ESC closes it and focus returns to the trigger button.
6. **Status colors on rows.** Ensure each status (idle / scanning / copying / failed / cancelled) has a non-color secondary signal (icon or text label).
7. **Group detail panel** has its own log area. Apply M03's scrollbar-terminal class for visual consistency.

### TaskGroupsTable.vue

8. **Row hover state.** Add `hover:bg-slate-50` if missing.
9. **Table headers**: ensure `<th scope="col">` semantic is set.
10. **Truncated cells**: long task names should `truncate` with `title="<full text>"` for tooltip.
11. **Action buttons in row** (retry / cancel / open) — icon-only? If yes, add `aria-label`.

### TaskGroupDetailPanel.vue

12. **Phase breakdown likely uses color chips.** Ensure each phase has icon + text, not color only.
13. **Empty state when no group selected.** Check it uses `Empty.vue`.

### ManualCopyPage.vue

14. **Form validation feedback.** Inline errors below fields; not relying on toast.
15. **Path inputs**: ensure they use shared `DirectoryPathInput.vue` for consistency.
16. **Submit button loading state**: spinner + disable while async runs.

### ManualCopyModal.vue (628 lines, big)

17. **Modal a11y baseline** — `role="dialog"`, `aria-modal="true"`, focus trap, ESC to close, click-outside-to-close (with confirm if dirty).
18. **Filter toggles** for selected extensions / keywords — keyboard reachable, space toggles.
19. **Stability check summary (line 82-88)**: read-only text. Consider rendering as a small info card with `Info` icon for visibility.
20. **Target preview "already exists" warning.** Confirm it uses warning tone (amber) and has icon.
21. **Status message system**: replace with `useToast` from M01.
22. **Loading skeleton** while config loads (line 91-100) — drop in M01's skeleton.
23. **Submit button**: should the modal close on success? Confirm UX matches expectation.

---

## Recommended fixes

Direction notes (subagent decides exact code):

- Migrate toast first; remove the local `showToast` ref and timer.
- Add skeletons in two spots: TaskGroupsTable initial fetch, ManualCopyModal config load.
- Replace ASCII dashes with em-dashes in placeholders / empty cells.
- Standardize action-button labels via i18n. Manual copy modal in particular has Chinese inline strings worth auditing for parity.
- For the modal: lift focus management via `@vueuse/core`'s `useFocusTrap` if installed; otherwise a small homemade trap (focus first button on open, restore previous focus on close, trap Tab between first/last focusable).

### New / verified i18n keys

| Key | zh | en |
|---|---|---|
| `tasks.empty.notRunning` | 调度器未启动 | Scheduler not running |
| `tasks.empty.actionStart` | 启动调度器 | Start scheduler |
| `tasks.loading.tasks` | 加载任务中… | Loading tasks… |
| `tasks.modal.dirtyConfirm` | 有未保存的修改，确定关闭？ | Unsaved changes — close anyway? |

(Audit existing keys before duplicating.)

---

## Out of scope

- DO NOT change scheduler logic, event payloads, or backend commands.
- DO NOT add new task types or change `TaskRecord` shape.
- DO NOT split TaskStatusPage into sub-components beyond what already exists.
- DO NOT touch `lib/scheduler.ts` or `lib/store.ts`.

---

## Verification

1. `pnpm check` clean.
2. Boot app cold — confirm skeleton shows on TaskGroupsTable for the first ~500ms.
3. Open Manual Copy modal, tab through fields — focus stays inside the modal.
4. ESC inside open modal closes it; focus returns to opener.
5. Trigger a copy with invalid path — inline error visible, no toast.
6. Trigger valid copy — success toast appears bottom-right (M01 container), 3s auto-dismiss.
7. Run keyboard-only test: open page → tab → operate Play/Stop → tab to a row → Enter to open detail. No mouse needed.

---

## Reporting back

Files changed, i18n keys, deviations. Under 200 words.
