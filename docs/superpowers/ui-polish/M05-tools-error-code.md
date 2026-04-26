# M05 — Tools Hub & Error Code Lookup Polish

- **Phase**: 2 (after M01)
- **Risk**: Low
- **Files**:
  - `src/pages/ToolsHubPage.vue` (228 lines)
  - `src/pages/ErrorCodeLookupPage.vue` (549 lines)
  - `src/pages/errorCodeLookup/validation.ts` (no UI changes; only tested)

---

## Goal

Tools hub is the primary discovery surface — make the cards feel inviting and clickable. Error code lookup is brand-new — apply the same final polish other tool pages got.

---

## Issues

### ToolsHubPage.vue

1. **Card description 3-line clamp** — long descriptions truncate awkwardly. Add `title="<full description>"` to the card so hover reveals the full text.
2. **`isToolActive` computed in-component**, no reactive update if a tool starts mid-page-view. Confirm: if user opens hub then starts ScreenShare in another window, does the badge update? If not, subscribe to a store event or add a polling tick.
3. **No skeleton when tool status data is loading.** First paint may show all tools as "inactive". Drop a brief skeleton or default to `loading` state.
4. **Card hover transform `-translate-y-1`** — add `motion-reduce:transform-none`.
5. **Decorative blur-out circles in section header** — purely aesthetic. Confirm they're behind content (`-z-10`), don't intercept clicks, and don't blur underneath text.
6. **Per-tool gradient icons** — confirm contrast for the white glyph against each gradient pair. The amber→orange and emerald→teal pairs may fail 4.5:1. Apply white glyph + darker shadow ring if needed.
7. **CTA button arrow icon (ArrowRight)** lacks `aria-hidden="true"` since the button text already labels it. Decorative duplication.
8. **Card click target.** Currently the whole card or just the button? Make the entire card clickable (anchor tag) so users don't have to aim at the small CTA.
9. **Active pulse indicator (emerald)** lacks a `<span class="sr-only">{{ t('toolsHub.runtimeActive') }}</span>`.
10. **Cards in 4-col xl, 2-col md, 1-col sm grid.** Window narrower than ~600px? Confirm single-column doesn't stretch cards too wide.

### ErrorCodeLookupPage.vue

11. **Mode switch radio group** lacks `role="radiogroup"` + `aria-label`.
12. **Mode switching clears state instantly with no transition.** Add a 120ms fade or slide on the results section so it doesn't feel like a flash.
13. **Pagination jump-to-page input** — confirm `inputmode="numeric"` is set (good), and add `aria-label="跳转到页码"`.
14. **Empty cells render as ASCII `-`** at lines 434, 437, 443, 446, 449, 466, 473, 481, 489, 497. Replace with em-dash `—`.
15. **`en.columns.messageCn = 'Chinese'`** at messages.ts:770. Confirm with team — plan said `'中文'`. Either is defensible; pick one and stick.
16. **`'...'` instead of `'…'`** in `errorCodeLookup.syncing` (zh + en). Replace with proper ellipsis character.
17. **Sync button spinner** — already uses `animate-spin` on `RefreshCw`. ✓ Add `motion-reduce:animate-none`.
18. **Status banner (success/error)** at top — migrate to `useToast` from M01. The inline banner is OK to keep for *page-level* status, but transient sync feedback is better as a toast.
19. **Row click expands detail panel.** Add `role="button"` + `tabindex="0"` + Enter/Space handler so keyboard users can expand.
20. **Detail panel uses `dl` / `dt` / `dd`** — already semantic. Verify `dt` and `dd` pairs are in correct visual order.
21. **No skeleton on initial query** — typing then clicking 查询 shows blank then results. Drop a `<LoadingSkeleton variant="list-row" :count="5" />` while the query runs.
22. **`computeNoResultMessage` for empty preview returns `null`** (line 196 area). Verify the empty preview state is OK — should it be "Sync some data to begin" instead?

---

## Recommended fixes

- Replace ASCII dashes globally in this page (search for `: '-'` and `'—'`).
- Add `aria-label` and `aria-labelledby` to the mode radio group and table.
- Wrap row click handlers to also accept keyboard.
- Add `motion-reduce:` variants for animations introduced.
- Migrate sync feedback to toast.

### New i18n keys

| Key | zh | en |
|---|---|---|
| `toolsHub.runtimeActive` | 运行中 | Active |
| `errorCodeLookup.aria.modeGroup` | 查询模式 | Query mode |
| `errorCodeLookup.aria.jumpInput` | 跳转到页码 | Jump to page |
| `errorCodeLookup.aria.expandRow` | 展开错误码详情 | Expand error code details |

(Verify before adding; some likely exist already.)

---

## Out of scope

- DO NOT change validation.ts or its tests.
- DO NOT change Tauri commands or their wrappers.
- DO NOT change router.
- DO NOT change tools-hub card data structure.

---

## Verification

1. `pnpm check` clean.
2. `node --test src/pages/errorCodeLookup/validation.test.mjs` — still passes.
3. `node --test src/lib/sidebarNavigation.test.mjs` — still passes (sidebar tests cover tool nav).
4. Tab into ToolsHubPage — every card focusable in order, focus ring visible.
5. Click a tool card anywhere → routes correctly. Keyboard Enter on a focused card → same.
6. Open ErrorCodeLookupPage — switch mode with arrow keys (radio group convention).
7. Run a query — skeleton appears, then results.
8. Click a row → detail expands. Press Enter on a focused row → same.
9. Trigger a sync — toast appears bottom-right; in-page banner does NOT duplicate the message.
10. Visual: confirm no ASCII `-` in empty cells.

---

## Reporting back

Files changed, i18n keys added, anything that already passed (don't fix what works). Under 200 words.
