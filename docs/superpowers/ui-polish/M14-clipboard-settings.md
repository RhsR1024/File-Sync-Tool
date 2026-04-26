# M14 — Clipboard Settings Tabs Polish

- **Phase**: 5 (after M01)
- **Risk**: Low — settings UI, no business logic
- **Files**:
  - `src/components/clipboardSettings/GeneralTab.vue` (254 lines)
  - `src/components/clipboardSettings/DataTab.vue` (427 lines)
  - `src/components/clipboardSettings/DisplayTab.vue` (171 lines)
  - `src/components/clipboardSettings/AppFilterTab.vue` (135 lines)
  - `src/components/clipboardSettings/PreviewTab.vue` (120 lines)
  - `src/components/clipboardSettings/ShortcutsTab.vue` (120 lines)
  - `src/components/ClipboardSettingsPanel.vue` (242 lines, the host)

(Adjust paths if the actual tabs live elsewhere.)

---

## Goal

Six settings tabs sharing one panel host. Standardize tab a11y, form layouts, and toggles.

---

## Cross-tab issues

1. **Tab list a11y** — `role="tablist"`, `role="tab"`, `aria-selected`, arrow-key nav.
2. **Form field layout** — pick one: label-above OR label-left. Stick with it.
3. **Toggle component consistency** — every Settings tab in the app should use the SAME toggle visual (review once, fix all).
4. **Section grouping** within a tab — use `<fieldset><legend>` or visually equivalent groupings.
5. **Helper text below inputs** — muted slate-500, not below-the-baseline tiny.
6. **Save behavior** — explicit save button OR auto-save with subtle "已保存 / Saved" toast.
7. **Min/max validation hints** visible inline.

## Per-tab specifics

### GeneralTab.vue (254)

8. Auto-paste toggle — explain risk in helper text.
9. History limit number — show units (entries / 条).
10. Startup section: launch-on-boot checkbox, link to OS settings if needed.

### DataTab.vue (427, biggest)

11. Retention dropdown — show "保留 7 天" preset chips.
12. Cleanup button — destructive; confirm dialog.
13. Export/Import — file picker; show last export path.
14. Storage usage indicator — bar showing N MB of M MB cap.

### DisplayTab.vue (171)

15. Grid vs list toggle — radio group with visual previews (small thumbnails).
16. Icon size slider — live preview.
17. Compact mode — toggle with example.

### AppFilterTab.vue (135)

18. Per-app blocklist / allowlist — table with app icon + name + actions.
19. Add app — picker dialog or text entry; validate process name.

### PreviewTab.vue (120)

20. Preview delay slider — range 100-2000ms with live label.
21. Image size cap — number input with MB units.
22. Hover preview enabled — toggle.

### ShortcutsTab.vue (120)

23. Each shortcut row: action + current binding + edit button.
24. `ClipboardHotkeyInput` capture — clear instruction text.
25. Conflict detection — warn if two shortcuts collide.

---

## i18n keys (sample)

| Key | zh | en |
|---|---|---|
| `clipboardSettings.tab.general` | 常规 | General |
| `clipboardSettings.tab.data` | 数据 | Data |
| `clipboardSettings.tab.display` | 显示 | Display |
| `clipboardSettings.tab.appFilter` | 应用过滤 | App Filter |
| `clipboardSettings.tab.preview` | 预览 | Preview |
| `clipboardSettings.tab.shortcuts` | 快捷键 | Shortcuts |
| `clipboardSettings.saved` | 已保存 | Saved |
| `clipboardSettings.shortcuts.conflict` | 与现有快捷键冲突 | Conflicts with existing shortcut |

---

## Out of scope

- DO NOT change clipboard backend / persistence model.
- DO NOT split tabs into smaller components in this module.
- DO NOT add new settings (defer to feature work).

---

## Verification

1. `pnpm check` clean.
2. Tab through each tab — arrow-key navigation between tabs.
3. Inside a tab, tab through controls — focus rings visible.
4. Toggle a setting → "Saved" toast OR explicit save button.
5. Trigger a shortcut conflict → inline warning.
6. Image cap input rejects non-numeric → inline error.

---

## Reporting back

Under 200 words.
