# M02 — Sidebar & App Shell A11y Polish

- **Phase**: 1 (foundation; can run in parallel with M01)
- **Risk**: Medium — sidebar is on every page; visual changes are visible everywhere
- **Estimated changes**: 2 files

---

## Goal

Tighten the always-visible chrome (sidebar + app shell) so the rest of the polish work doesn't sit inside a frame that has its own a11y / visual debt. No layout reshuffle, no removed features.

---

## Files

- `src/components/Sidebar.vue` (156 lines)
- `src/App.vue` (217 lines)
- `src/components/UpdateRedDot.vue` (15 lines) — small a11y nit
- `src/locales/messages.ts` — add a few missing aria/title strings

---

## Issues

### Sidebar.vue

1. **No `aria-label` / `role="navigation"` on the `<aside>` wrapper.** Screen-reader users get an unlabeled landmark.
2. **`<button>` items don't expose active state via ARIA.** The visual highlight is there but `aria-current="page"` is missing for the active route.
3. **Icon-in-button focus rings are inconsistent.** Some hover states present, but `focus-visible:` ring is absent. Keyboard users get no signal when tabbing.
4. **Version chip click target lacks accessible name.** It opens `/about` but only has the visible text — fine — and an `UpdateRedDot` whose `aria-label="update available"` is hardcoded English (NOT translated).
5. **Section labels (Common / Tools / System) are decorative `<span>`.** Use `<h2>` or attach `aria-labelledby` to each section's `<ul>` so the structure is announced.
6. **Active item glow shadow lives on a colored border + bg.** The color contrast for inactive vs active in dark mode hasn't been measured; sky-500 on `#0b1220` should be checked against WCAG.
7. **Pulsing emerald dot for runtime-active tools (screenShare, fileShare)** is purely color/animation. Add `<span class="sr-only">运行中 / Active</span>` so non-visual users know.
8. **Hover scale on icons** uses transform; respects `prefers-reduced-motion`? Currently no — add a `motion-reduce:transform-none`.
9. **Group containers use `inset shadow + border-sky-500/25` for active items but not all themes test cleanly under high-contrast OS modes.** Verify on Windows high-contrast — if it disappears, add an inset border to the active state.

### App.vue

10. **Main content `<main>` element missing.** Currently a flex container; should be `<main role="main">` (or `<main>` semantic) so skip-link can target it. No skip-link exists today either.
11. **No "skip to main content" link.** Add a visually-hidden link as the first focusable element that jumps focus past the sidebar.
12. **`UpdateDialog` is mounted unconditionally** but has no focus trap / no ESC dismissal in M06's scope; here just confirm it doesn't steal focus on mount.
13. **Keep-alive list (`MainConsole`, `CodeStatisticsPage`, `NetworkToolsPage`, `SettingsPage`) is hardcoded.** Out of scope to change, but document in a code comment why these and not others.

### UpdateRedDot.vue

14. **`aria-label="update available"` is hardcoded English.** Use i18n key `sidebar.updateAvailable`.
15. **Missing `role="status"`** so screen readers announce when it appears.

---

## Recommended fixes

Paired 1:1 with the issues above. Subagent applies them; the human reviews the diff.

| # | Fix |
|---|---|
| 1 | Wrap sidebar in `<nav role="navigation" :aria-label="t('sidebar.navigation')">` |
| 2 | Add `:aria-current="isActive ? 'page' : undefined"` to each nav button |
| 3 | Add `focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400/60` from `FOCUS_RING` token (M01) on all buttons |
| 4 | Replace hardcoded `aria-label="update available"` with `:aria-label="t('sidebar.updateAvailable')"`. Add new i18n keys (zh: `有新版本可用`, en: `Update available`) |
| 5 | Replace section label `<span>` with `<h2 class="sr-only">{{ t('sidebar.section.<key>') }}</h2>` + visible decorative span. Existing visual unchanged |
| 6 | Verify contrast: `bg-sky-500/15 text-sky-200 border-sky-500/25` on `#0b1220` — if text contrast < 4.5:1, bump text to `text-sky-100`. Apply only if measurement fails |
| 7 | Add `<span class="sr-only">{{ t('sidebar.runtimeActive') }}</span>` next to the pulse dot |
| 8 | Add `motion-reduce:transform-none motion-reduce:transition-none` to icon scale/transition classes |
| 9 | If high-contrast fails: add `outline outline-1 outline-sky-300` to active state — guarded by `forced-colors: active` media query |
| 10 | Wrap content area in `<main id="main-content" role="main" class="...">` |
| 11 | Add at the top of App.vue template: `<a href="#main-content" class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[100] focus:bg-white focus:px-3 focus:py-1 focus:rounded-md focus:shadow-lg">{{ t('common.skipToMain') }}</a>` |
| 12 | No code change needed; just verify no `autofocus` on UpdateDialog inputs at mount |
| 13 | Add a code comment block above `<keep-alive :include="...">` explaining WHY each name is kept-alive (cost of remount, scroll preservation, etc.). Do not change the list |
| 14 | Use `:aria-label="t('sidebar.updateAvailable')"` |
| 15 | Add `role="status"` to the dot wrapper |

### New i18n keys (zh + en)

| Key | zh | en |
|---|---|---|
| `sidebar.navigation` | 主导航 | Main navigation |
| `sidebar.section.common` | 常用 | Common |
| `sidebar.section.tools` | 工具 | Tools |
| `sidebar.section.system` | 系统 | System |
| `sidebar.runtimeActive` | 运行中 | Active |
| `sidebar.updateAvailable` | 有新版本可用 | Update available |
| `common.skipToMain` | 跳到主内容 | Skip to main content |

If any of these already exist (check first via grep), reuse them.

---

## Out of scope

- DO NOT change the sidebar's color scheme, gradient, layout, or item ordering.
- DO NOT change the keep-alive list.
- DO NOT touch `UpdateDialog.vue` (M06 owns it).
- DO NOT change router or sidebarNavigation.ts.
- DO NOT alter mobile / responsive behavior — this app is desktop-only.

---

## Verification

1. `pnpm check` — clean.
2. `node --test src/lib/sidebarNavigation.test.mjs` — still passes (we didn't change the nav data, just the rendering).
3. Manual a11y smoke:
   - Tab through the sidebar — every item shows a visible focus ring.
   - Press Enter on the focused item — navigates correctly.
   - Open Windows Narrator (or a dev tool's a11y inspector) — verify each section heading is announced and active item says "current page".
   - Tab from before the sidebar — first focusable element is "Skip to main content".
   - Click skip-link — focus moves to main content area.
4. Verify ALL pages still render (route-by-route smoke). The sidebar appears on every page, so any breakage is loud.

---

## Reporting back

Subagent reports:
- Files changed (paths)
- New i18n keys actually added (some may have existed)
- Test results
- Anything skipped because the spec turned out to be wrong (e.g. issue #6 contrast already passed)

Under 200 words.
