# M06 — About & Update Flow Polish

- **Phase**: 2 (after M01)
- **Risk**: Low-medium — update flow is critical-path; visual changes only, no logic touched
- **Files**:
  - `src/pages/AboutPage.vue` (308 lines)
  - `src/components/UpdateDialog.vue` (347 lines)
  - `src/components/UpdateRedDot.vue` (already touched in M02 for a11y)

---

## Goal

The update path was just shipped (commit `0e3b457`) and reviewed. Apply the polish items called out in the review without touching the just-fixed bug surface area.

---

## Issues

### AboutPage.vue

1. **Fallback `currentReleaseDate` parses `t('sidebar.version')` and replaces `.` with `-`** (lines 47-50, 71). Brittle. Pull the release date from a real config source (e.g. a build-time injected constant, or the sidebar's existing data, but as a structured value not a string parse).
2. **Banner duplicates dialog content.** When manual check finds an update, the page shows a banner AND opens the dialog with similar wording. Pick one channel:
   - Banner stays as a passive "新版本可用" pill with [立即升级] button.
   - Dialog only opens on user click.
3. **Status message timer (`showStatus`)** — migrate to `useToast` from M01.
4. **`Router` icon used as the server URL indicator** (lines 3, 191). `Globe` or `Link` matches intent better. Cosmetic.
5. **History list expand/collapse** lacks animation. Add a height-auto Vue transition or content-only fade.
6. **"测试连接" button feedback** — currently a toast; confirm it's wired through `useToast` after migration.
7. **History row hover** — confirm `hover:bg-slate-50` exists.
8. **Current version pill** — confirm `aria-current="true"` is set.
9. **Dev-mode badge text** "开发模式：更新检查已禁用" — consider em-dash `开发模式 — 更新检查已禁用` per spec.

### UpdateDialog.vue

10. **Modal a11y baseline missing** — add `role="dialog"`, `aria-modal="true"`, `aria-labelledby` (pointing at the title), focus trap, ESC dismissal.
11. **Close X button** — currently has no `aria-label`. After the bug fix in M0 (cancel-on-close), confirm the button has `:aria-label="t('common.close')"` and is hidden during downloading state (already done in bug fix).
12. **Resume copy wording drift** — `messages.ts:1979` zh `bodyResume` is "版本 {version} 已经下载完成，现在升级吗？". Spec called for "上次有未应用的更新（{version}），现在升级？". Update zh to spec.
13. **Title emojis** — spec §5.4 has 🚀 / ✅ / ❌. Implementation drops them. **Decision needed**: keep dropped (cleaner) OR add back per spec. Recommend keeping dropped + using lucide icons (`Rocket`, `CheckCircle2`, `AlertCircle`) inline for consistency with the rest of the app.
14. **Progress bar** — confirm `role="progressbar"`, `aria-valuenow`, `aria-valuemin`, `aria-valuemax`, `aria-label`.
15. **Speed/size formatting** in progress text — verify locale-aware (1.5 MB vs 1,5 MB) — this app is en/zh, both use periods, so probably fine.
16. **Action buttons** — primary CTA visually distinct from secondary; ensure focus ring works on each.
17. **Verify-failed state** retry path — already covered by spec §3.3.
18. **State machine has 7 states** (closed, found, downloading, ready, resume, verify_failed, network_error) — extras are improvements; no fix needed, just document.

### UpdateRedDot.vue

(Already covered in M02. If M02 hasn't shipped yet, do nothing here.)

---

## Recommended fixes

- Pin `currentReleaseDate` source: prefer reading from `sidebar.version` IF the sidebar has structured fields, OR use `__APP_RELEASE_DATE__` injected at build time via Vite define. Document the chosen source.
- Migrate AboutPage status to `useToast`.
- Replace `Router` icon with `Globe` or `Link2`.
- Replace zh `bodyResume` text per spec.
- Add modal a11y attrs to UpdateDialog.
- Decide on title emojis (recommend lucide icons inline).
- Wrap history-list expand in `<transition>` with a height-auto trick using `:style="{ maxHeight: ... }"`.

### New i18n keys

| Key | zh | en |
|---|---|---|
| `common.close` | 关闭 | Close |
| `updater.dialog.aria.progress` | 下载进度 | Download progress |

(Audit before adding.)

---

## Out of scope

- DO NOT change updater state machine logic.
- DO NOT change Tauri commands or events.
- DO NOT change `useUpdater.ts` listener wiring.
- DO NOT touch `tauri.ts` updater types.
- DO NOT alter the X-close cancel logic (just shipped in bug fix).

---

## Verification

1. `pnpm check` clean.
2. `node --test src/pages/about/version.test.mjs` — still passes (4 tests).
3. Open `/about` — manually:
   - Tab through page; focus rings visible everywhere.
   - Test connection → toast appears, no inline banner.
   - Found new version → banner appears AND dialog opens once (manual check).
4. Open dialog (force `dialogState='found'` via dev tool) — Tab cycles inside dialog, ESC closes it, X closes it (when not downloading).
5. Switch to dev build (`pnpm tauri dev`) — confirm dev-mode badge text.
6. Run with `prefers-reduced-motion` set — history expand has no animation OR uses opacity-only.

---

## Reporting back

Files changed, i18n keys, source for `currentReleaseDate` chosen, deviations. Under 200 words.
