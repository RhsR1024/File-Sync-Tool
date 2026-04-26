# M10 — Network Tools Polish

- **Phase**: 4 (after M01)
- **Risk**: Medium — 5 tabs, each with its own state and async work
- **Files**:
  - `src/pages/NetworkToolsPage.vue` (74 lines, just a tab router)
  - `src/components/networkTools/PingScanTab.vue` (609 lines)
  - `src/components/networkTools/PortTestTab.vue` (368 lines)
  - `src/components/networkTools/SubnetCalcTab.vue` (265 lines)
  - `src/components/networkTools/TcpConnectionsTab.vue`
  - `src/components/networkTools/WakeOnLanTab.vue`

(Adjust paths if location differs.)

---

## Goal

Five operationally distinct tabs that share a wrapper. Make them feel like one cohesive product.

---

## Cross-tab issues

1. **Tab list a11y** — `role="tablist"`, `role="tab"`, `aria-selected`, arrow-key navigation between tabs, persist last active tab in localStorage if not already.
2. **Inconsistent input layouts across tabs** — each tab has its own form. Pick a shared layout: label above input, button right-aligned, helper text below. Adopt across all 5.
3. **Result tables** — each tab has its own. Standardize:
   - `<th scope="col">`
   - sortable columns where useful (`aria-sort`)
   - hover state on rows
   - empty state via `Empty.vue`
4. **Loading states** — long ping scans, port tests, etc. Skeleton or progress bar.
5. **Toast unification** — migrate any local status helpers to `useToast`.
6. **Copy-to-clipboard buttons** — many tabs likely have "copy IP" / "copy results" buttons. Add the `Copy` lucide icon, tooltip, and a subtle "Copied" toast on success.

## Per-tab issues

### PingScanTab.vue (609 lines, biggest)

7. **Subnet input** — validate CIDR format inline.
8. **Result grid** — for /24 (256 IPs), grid of color-coded cells. Add legend (alive / unreachable / pending) and tooltip on each cell with IP + ping time.
9. **Progress bar during scan** — visible at top, with cancel button.
10. **Cancel scan** — must be reachable while scanning. Confirm.

### PortTestTab.vue (368 lines)

11. **Port range input** — validate (1-65535).
12. **Common ports preset** — chips like "Web (80, 443)", "SSH (22)" for one-click selection.
13. **Result row** — port + status + service name lookup if available.

### SubnetCalcTab.vue (265 lines)

14. **CIDR input** — live calculation as user types (debounced 200ms).
15. **Result display** — copy button next to each computed field (network, broadcast, mask, host count).

### TcpConnectionsTab.vue

16. **Live connection list** — refresh interval visible to user.
17. **Filter input** — by remote IP / port / state.
18. **Connection state colors** — ESTABLISHED green, TIME_WAIT amber, etc. Add legend.

### WakeOnLanTab.vue

19. **MAC input** — validate format (`xx:xx:xx:xx:xx:xx` or `xx-xx-xx-xx-xx-xx`).
20. **Recent targets** — localStorage of recent MACs.
21. **Send confirmation** — toast on success / failure.

---

## i18n keys (sample — generate as needed)

| Key | zh | en |
|---|---|---|
| `networkTools.tab.ping` | Ping 扫描 | Ping Scan |
| `networkTools.tab.port` | 端口测试 | Port Test |
| `networkTools.tab.subnet` | 子网计算 | Subnet Calc |
| `networkTools.tab.tcp` | TCP 连接 | TCP Connections |
| `networkTools.tab.wol` | 网络唤醒 | Wake on LAN |
| `networkTools.copy.copied` | 已复制 | Copied |
| `networkTools.legend.alive` | 在线 | Alive |
| `networkTools.legend.unreachable` | 不可达 | Unreachable |
| `networkTools.legend.pending` | 检测中 | Probing |

---

## Out of scope

- DO NOT change ping/port/subnet calculation logic.
- DO NOT change Tauri commands.
- DO NOT add new tabs.
- DO NOT change networking timeouts or strategy.

---

## Verification

1. `pnpm check` clean.
2. Arrow-key navigate tabs.
3. Each tab: tab through inputs → result table → copy button.
4. Run a ping scan; cancel midway; confirm cancellation works.
5. Each tab's empty state visible before any action taken.

---

## Reporting back

Under 250 words.
