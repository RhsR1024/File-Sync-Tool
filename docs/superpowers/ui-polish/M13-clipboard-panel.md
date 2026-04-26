# M13 — Clipboard Panel Polish

- **Phase**: 5 (after M01)
- **Risk**: High — clipboard panel is performance-sensitive (frequent updates) and interaction-heavy
- **Files**:
  - `src/pages/ClipboardPanelPage.vue` (740 lines)
  - `src/pages/ClipboardManagerPage.vue` (24 lines)
  - `src/pages/ClipboardTextPreview.vue` (87 lines)
  - `src/pages/ClipboardImagePreview.vue` (159 lines)
  - `src/components/ClipboardList.vue` (623 lines)
  - `src/components/ClipboardPanelGroupMenu.vue` (295 lines)
  - `src/components/ClipboardCardMenu.vue` (103 lines)
  - `src/components/ClipboardFileDetailsDialog.vue` (130 lines)
  - `src/components/ClipboardImportExportDialog.vue` (135 lines)
  - `src/components/ClipboardMergePasteDialog.vue` (116 lines)
  - `src/components/ClipboardToolbar.vue` (99 lines)
  - `src/components/ClipboardPinnedSection.vue` (88 lines)
  - `src/components/ClipboardSearchBox.vue` (61 lines)
  - `src/components/ClipboardHoverPreview.vue` (42 lines)
  - `src/components/ClipboardHighlightText.vue` (49 lines)
  - `src/components/ClipboardAppIcon.vue` (50 lines)
  - `src/components/ClipboardHotkeyInput.vue` (64 lines)

---

## Goal

This is a feature-complete clipboard manager with auxiliary windows (preview), context menus, batch ops, hotkeys, drag handling. Polish for cohesion + a11y; do NOT alter behavior.

---

## Issues

### Performance / responsiveness

1. **`ClipboardList.vue` (623 lines)** — list virtualization status unknown. With 500+ clipboard items, scrolling may stutter. If not virtualized, drop in `vue-virtual-scroller` or document why not.
2. **Hover preview** — confirm debounce on mouse-enter so quick scroll doesn't fire 50 previews.
3. **Search filter** — debounced 200ms.
4. **Batch selection** — re-renders the whole list? Memoize the selection state.

### A11y

5. **Search box** — `aria-label`, `role="searchbox"`.
6. **List items** — `role="option"`, `aria-selected` for selected state.
7. **Context menus** (`ClipboardPanelGroupMenu`, `ClipboardCardMenu`) — `role="menu"`, `role="menuitem"`, arrow-key navigation, ESC closes.
8. **Batch mode toggle** — clear ARIA state.
9. **HotkeyInput** — confirm captured key combo is announced; provide a "clear" button.

### Modals

10. **All 3 modals** (FileDetailsDialog, ImportExportDialog, MergePasteDialog) — apply baseline a11y (role/aria-modal/focus-trap/ESC/ overlay click).

### Visual

11. **Pinned section** — clear visual separation from main list (different background or divider).
12. **Card hover** — subtle elevation (`hover:shadow-sm hover:bg-slate-50`).
13. **Card pressed state** — `active:scale-[0.99]`.
14. **Search highlight** (`ClipboardHighlightText`) — confirm contrast for highlighted match (yellow background + dark text).
15. **App icon rendering** — fallback if app icon fails to load (generic `Apps` lucide icon).
16. **Drag region** — `CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION` flag exists. Confirm panel is draggable from a clearly indicated handle area, not the entire chrome (otherwise users can't click items).
17. **Empty state** when filter returns nothing — distinct from "no clipboard history" empty.

### Auxiliary windows

18. **ClipboardTextPreview** / **ClipboardImagePreview** — minimal chrome OK; ensure the preview window can be closed via ESC, has a small close button with aria-label.
19. **Image preview zoom** — confirm `+` / `-` keyboard shortcuts work, and pinch/wheel zoom is smooth.

### Debug code

20. **Debug functions** (`debugClipboardSnapshot`, `onDebugPointerEvent`) — confirm they're guarded by a build-time flag or removed entirely if no longer needed in prod.

---

## i18n keys (sample)

| Key | zh | en |
|---|---|---|
| `clipboard.search.placeholder` | 搜索剪贴板 | Search clipboard |
| `clipboard.search.aria` | 搜索剪贴板项 | Search clipboard items |
| `clipboard.menu.copyText` | 复制文本 | Copy text |
| `clipboard.menu.delete` | 删除 | Delete |
| `clipboard.menu.pin` | 置顶 | Pin |
| `clipboard.empty.noHistory` | 无剪贴板历史 | No clipboard history |
| `clipboard.empty.noMatch` | 无匹配项 | No matches |
| `clipboard.preview.close` | 关闭预览 | Close preview |
| `clipboard.batch.select` | 批量选择 | Batch select |

---

## Out of scope

- DO NOT change clipboard listener / Tauri commands.
- DO NOT change history capacity / persistence model.
- DO NOT change preview window auxiliary mode.
- DO NOT remove debug code unless guarded — risk of regression in prod issues.

---

## Verification

1. `pnpm check` clean.
2. Cargo tests for clipboard module still pass.
3. Tab into search box, type → debounced filter; arrow keys navigate list; Enter selects.
4. Right-click a card → context menu opens; arrow keys navigate; ESC closes.
5. Open File Details modal → ESC closes; focus returns.
6. Hover preview → ~200ms delay before showing.
7. With 500+ items, scroll smoothly (via virtualization or pagination).

---

## Reporting back

Under 250 words. Note any virtualization decisions and reasons.
