# M12 — LAN Sharing (File & Screen) Polish

- **Phase**: 4 (after M01)
- **Risk**: Medium — runtime-active tools, server-state-dependent UI
- **Files**:
  - `src/pages/FileSharePage.vue` (999 lines)
  - `src/pages/ScreenSharePage.vue` (948 lines)

---

## Goal

Two long-running services with rich settings. Standardize the running-vs-stopped indicators, permission grids, and settings layout.

---

## Cross-page issues

1. **Service status banner** — both pages have on/off state. Standardize a "service status header" component (or visual pattern):
   - Big status dot (emerald active / slate-400 inactive / amber transitional)
   - Service name + "running on http://X:Y" if active
   - Toggle button (Start / Stop) with confirm-stop pattern
2. **Settings sections** — both have multiple tabs / sections (quality, FPS, IP filter, users, etc.). Standardize section spacing, headings, and helper text.
3. **Toast** — migrate to `useToast`.
4. **Live data refresh** — client lists, bandwidth meters. Confirm there's an indicator showing "auto-refreshing every Ns" + a manual refresh button.
5. **`prefers-reduced-motion`** — pulse / glow indicators for active state should fall back to static when motion-reduced.

## FileSharePage specifics

6. **Permission matrix** — likely a checkbox grid. Apply:
   - `<table>` with `<th scope="col">` for permission, `<th scope="row">` for user
   - keyboard space toggles
   - "select all" / "clear all" per column
   - preset chips at top (`readOnly`, `downloadOnly`, etc., already in code) — clicking applies the preset
7. **User add/edit modal** — apply modal a11y baseline.
8. **Bandwidth limit input** — slider + numeric, with unit (MB/s).
9. **Per-user-root-perms** is an active brainstorm task. Don't pre-empt that work; just polish the existing UI.

## ScreenSharePage specifics

10. **IP filter rules** — list with add/remove. Preset "Allow LAN" / "Allow specific IP".
11. **Quality / FPS sliders** — show current value next to slider, snap to common presets (e.g. 30 / 60 FPS).
12. **Client list** — show connected clients with their IP and connection time.

---

## i18n keys (new)

| Key | zh | en |
|---|---|---|
| `share.status.running` | 服务运行中 | Service running |
| `share.status.stopped` | 服务已停止 | Service stopped |
| `share.status.starting` | 启动中… | Starting… |
| `share.status.stopping` | 停止中… | Stopping… |
| `share.action.start` | 启动服务 | Start service |
| `share.action.stop` | 停止服务 | Stop service |
| `share.action.stopConfirm` | 确认停止？ | Confirm stop? |
| `share.refresh.auto` | 每 {n} 秒自动刷新 | Auto-refresh every {n}s |
| `share.refresh.manual` | 立即刷新 | Refresh now |

---

## Out of scope

- DO NOT change service start/stop logic.
- DO NOT change permission model (per-user-root-perms is in flight in another spec).
- DO NOT change SMB / streaming protocol.

---

## Verification

1. `pnpm check` clean.
2. Toggle service start / stop — status banner updates.
3. Tab through permission matrix — space toggles correctly.
4. Apply a permission preset — chip click applies; matrix updates.
5. Stop service while clients connected → confirmation dialog.
6. Reduced motion → pulse indicators are static.

---

## Reporting back

Under 200 words.
