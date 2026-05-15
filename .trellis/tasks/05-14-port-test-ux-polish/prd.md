# Network Tools — Port Test UX Polish

## Goal

修复"端口测试"在 `all` 模式（65535 端口）下卡顿，并按"用户关心的是开放端口"重新组织视图：默认隐藏 closed 端口，放大可见端口的格子并居中显示端口号。

## Pain Points

1. 选 `all` 预设 → 65535 个 DOM cell 一次性 mount，卡。
2. 大扫描时 cell 缩到 10px，端口号不显示。
3. 表格视图默认 `all` 过滤，几千行 closed 把开放端口埋了。

## Spec

完整设计见 [docs/superpowers/specs/2026-05-14-port-test-ux-polish-design.md](../../../docs/superpowers/specs/2026-05-14-port-test-ux-polish-design.md)。

## Acceptance Criteria

- [ ] `all` 扫描全程不卡，进度条平滑推进。
- [ ] `totalPorts > 1024` 时网格视图只渲染开放端口大卡片（端口号 + 服务名 + 延迟），实时追加。
- [ ] `totalPorts ≤ 1024` 时保留今天的总览网格，但 cell 放大到 ~56px，端口号居中可见。
- [ ] 表格视图默认过滤器 = `仅 open`；用户可手动切回 `all` / `closed`。
- [ ] 大扫描期间没有 open 命中时，显示"扫描中… 暂未发现开放端口 ({scanned}/{total})"占位。
- [ ] `buildOpenPortCards` 纯函数有 vitest 覆盖（空、混合、按端口升序）。
- [ ] i18n 新增 key 同时含 en + zh。
- [ ] 后端零改动。

## Branching

直接在 `main` 上实施。

## Files Touched

修改：
- `src/components/network/PortTestTab.vue`
- `src/lib/portTestPresentation.ts`
- `src/lib/__tests__/portTestPresentation.test.ts`（新增 / 扩展）
- `src/locales/messages.ts`

无 Rust 改动。
