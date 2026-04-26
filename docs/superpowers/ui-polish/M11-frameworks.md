# M11 — Frameworks (Password & SSH) Polish

- **Phase**: 4 (after M01)
- **Risk**: Medium — batch operations on real devices
- **Files**:
  - `src/pages/FrameworkPasswordPage.vue` (519 lines)
  - `src/pages/EnableApplianceSshPage.vue` (890 lines)

---

## Goal

Two utility pages that batch-operate on framework devices. Similar UX patterns; standardize them.

---

## Cross-page issues

1. **Tag-based IP input** — both pages use this; ensure `aria-label` per input, Backspace removes last tag, Enter adds, comma/semicolon separator support.
2. **Batch progress bar** — show progress as `n / total`, with elapsed time and ETA.
3. **Per-IP status** — table row per IP with status icon (queued / running / success / failure / skipped). Status icons must have screen-reader text.
4. **Result table** — sortable by status, copyable rows, "export results" optional.
5. **Cancel batch** — destructive but reversible (current batch stops, in-flight requests not killed). Make this clear in UI.
6. **Toast unification** — both pages use inline `statusTone` ref; migrate to `useToast`.
7. **Empty state** — before any batch run, drop in `Empty.vue` with illustrative icon.
8. **Error retry** — clicking on a failed row should let user retry just that IP without re-running the batch.

## FrameworkPasswordPage specifics

9. **Password fields** — show/hide toggle (`Eye` / `EyeOff` icon). Confirm autocomplete attribute is `new-password` so browsers don't suggest existing passwords.
10. **Confirmation modal** before changing N device passwords — destructive action.
11. **Old / new password match check** — block submit if old=new with inline error.

## EnableApplianceSshPage specifics

12. **SSH protocol selection** — if applicable, dropdown with protocol versions.
13. **Default port hint** — show "默认 22" / "Default 22" as muted helper text.
14. **Test connection** before batch enable — pre-flight check button per IP.

---

## i18n keys (new)

| Key | zh | en |
|---|---|---|
| `framework.batch.elapsed` | 已用时 | Elapsed |
| `framework.batch.eta` | 预计剩余 | ETA |
| `framework.batch.cancel` | 取消批量 | Cancel batch |
| `framework.batch.cancelHint` | 已请求的 IP 不会回滚 | In-flight IPs will not roll back |
| `framework.row.retry` | 重试 | Retry |
| `framework.password.show` | 显示密码 | Show password |
| `framework.password.hide` | 隐藏密码 | Hide password |
| `framework.empty.title` | 添加 IP 开始 | Add IPs to start |

---

## Out of scope

- DO NOT change SSH client / password change logic.
- DO NOT change batch concurrency limits.
- DO NOT add device discovery feature.

---

## Verification

1. `pnpm check` clean.
2. Add IPs via paste, comma-sep, semicolon-sep, Enter — all work.
3. Batch run → progress bar advances, ETA computed.
4. Cancel midway → status updates, partial results shown.
5. Retry a failed row → only that IP runs.
6. Show/hide password toggle works; ARIA labels switch correctly.

---

## Reporting back

Under 200 words.
