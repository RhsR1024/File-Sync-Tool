# M07 — Settings Page In-Place Polish (no refactor)

- **Phase**: 3 (after M01)
- **Risk**: High blast radius — file is 1946 lines and central
- **Files**: `src/pages/SettingsPage.vue`

---

## Goal

Polish without refactoring. The architectural debt (mega-component, 8+ feature areas in one page) is a separate, larger spec — explicitly out of scope here. M07 only addresses visual / a11y / i18n issues that can land without changing the page's structure.

---

## Issues

### Hardcoded zh strings

1. **Built-in command groups (lines 107-128)** have Chinese names hardcoded: `'解压安装包'`, `'卸载旧版本'`, etc. Move to i18n keys (`settings.builtinCommands.<key>.name` and `.description`). zh keeps current text; en gets translations.
2. **Status-message helper `showStatusMsg`** uses zh-only labels in a few places — audit and replace with `t()`.

### Toast / status

3. **`showStatusMsg` (3s timeout)** — migrate to `useToast` from M01. Existing inline banners that show field-level validation should stay; only transient operation feedback becomes toast.

### Navigation within the page

4. **No left-side menu / no jump-to-section.** Users scroll a 1900-line form. Add a small **sticky table-of-contents column** OR a horizontal anchor strip at the top with `#scheduler`, `#rules`, `#deploy`, `#commands`, `#clipboard`, `#updater`, etc. Keep it lightweight: ~8 anchor chips, sticky on scroll. NO router change.

### Section semantics

5. **Sections are visually separated but not semantically.** Wrap each in `<section :aria-labelledby="...">` with a heading. Helps screen readers and the new TOC.
6. **Section headings** vary in size and style — standardize on a single Tailwind class set defined in M01 tokens.

### Forms

7. **Inputs sometimes lack `<label for="...">`.** Audit every input — many use placeholder-as-label. Fix by visible labels with proper `for=`.
8. **Required fields don't have `*` indicator.** Add a small red `*` after required labels.
9. **Inline error messages**: confirm errors show below fields, not in a banner at the top.
10. **Save buttons** — primary, with loading state when async. Verify focus stays on the button after click (so users can press Enter again to retry).

### Tables / lists inside settings

11. **Server list, command groups, time ranges** — each has add/edit/delete buttons. Confirm all are reachable via keyboard, have icon + text labels.
12. **Empty states** for server list, command groups, time ranges — apply M01 extended `Empty.vue`.

### Misc

13. **`isServerManagerOpen` modal** — apply modal a11y baseline (role/aria-modal/focus-trap/ESC).
14. **Tooltips on toggles** — every toggle (`launch_and_auto_scan`, `close_to_tray`, `notify_on_new_version`, etc.) needs a `title` or info-icon tooltip explaining what it does.
15. **Min-value validations** — `interval_minutes >= 5`, `stability_check_secs >= 60`, `recent_file_guard_mins >= 3`. Confirm clamps + visible hints "最小 5 分钟 / Min 5 minutes".

---

## Recommended fixes

Direction:

- Build a `<SectionAnchorStrip />` inline component (within the same file — no new file) with the 8 sections.
- Make each section `id="settings-<key>"` and add `scroll-mt-24` so anchor jumps don't get hidden under the sticky header.
- Convert `showStatusMsg` to `pushToast`.
- For built-in command groups, define a small constant array with i18n keys instead of literal strings.
- Audit forms with a grep for `placeholder=` to find label-less inputs.

### New i18n keys

A long list — subagent generates them as it migrates strings. Examples:

| Key prefix | Purpose |
|---|---|
| `settings.section.scheduler.title` etc. | Section headings |
| `settings.section.scheduler.description` etc. | Sub-headings |
| `settings.builtinCommands.unzip.name` | Built-in command labels |
| `settings.field.interval.helpMin` | Min-value hints |
| `settings.toast.saved` | Save success toast |
| `settings.toast.invalid` | Validation error toast |

Keep en/zh parity for every new key.

---

## Out of scope

- DO NOT split SettingsPage into sub-routes or sub-components. Architectural refactor is a separate, future plan.
- DO NOT change config schema or migration logic.
- DO NOT change command registration / Tauri bindings.
- DO NOT introduce a settings router.

---

## Verification

1. `pnpm check` clean.
2. Boot app, open Settings. Tab from top — should reach Anchor Strip first, then jump to first section.
3. Click each anchor → page scrolls to the section, heading is highlighted.
4. Save a setting → toast appears (no banner).
5. Trigger a validation error → inline error visible below field, focus stays on field.
6. Switch language to en → built-in command names render in English.
7. Open Server Manager modal → ESC closes, focus returns to opener.

---

## Reporting back

Files changed, count of i18n keys added, anchor sections created, deviations. Under 250 words.
