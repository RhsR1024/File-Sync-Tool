# 执行计划 — 主备（从）接入组

前置阅读顺序：[prd.md](./prd.md) → [design.md](./design.md)。全程不改 `src-tauri/`。

## 步骤清单（按序执行，每步可独立验证）

### 1. 纯逻辑模块 + 单测

- [ ] 新建 `src/lib/applianceSshGroups.ts`：`HaAccessGroup`、`MAX_SLAVES_PER_GROUP`、`isValidIp`（自页面迁移）、`normalizeGroup`、`isGroupActive`、`buildGroupTargets`、`composeAllTargets`、`buildRoleMap`、`serializeGroup`、`parseGroupEntry`（签名与拆解规则见 design.md §2）。
- [ ] 新建 `src/lib/applianceSshGroups.test.mjs`，覆盖 design.md §6 列出的用例。
- 验证：`node --test src/lib/applianceSshGroups.test.mjs`

### 2. 抽取 IpTagInput 组件

- [ ] 新建 `src/components/IpTagInput.vue`（props/事件见 design.md §3.1），逻辑从 `EnableApplianceSshPage.vue` 手动 IP 区块平移，不改行为。
- [ ] 页面手动 IP 区块替换为该组件（保留 recent datalist、样式等价）。
- 验证：`pnpm check`；`pnpm dev` 手动过一遍手动 IP 的增删改/粘贴/Backspace/失焦行为。

### 3. 组区块替换跳板机卡片

- [ ] `EnableApplianceSshPage.vue`：删除 `jumpHostPairs` 相关状态与模板，新增 `haGroups` 状态 + 组卡片 UI（主/备单行输入、从机 `IpTagInput maxTags=10`、加/删组、组序号）。
- [ ] `hasAnyJumpHost` 语义改为"任一组含备机"（`hasAnyBackup`）；主机 SSH 端口、主机独立凭据展示条件与请求传参随之切换。
- [ ] `handleExecute` 改用 `composeAllTargets`；执行前生成角色映射供结果表使用。
- [ ] 已选目标汇总胶囊补组内角色前缀。
- [ ] 白名单来源默认值 `whitelistSourceMode = ref('all')`。
- 验证：`pnpm check`；`pnpm dev` 组装三种组形态观察请求 payload（devtools 或运行日志 `[appliance-access]`）。

### 4. 最近使用记录

- [ ] 组记录读写切换为 `serializeGroup`/`parseGroupEntry`（kv 键沿用 `applianceSsh.recentJumpHostPairs`，上限 5）；胶囊展示 `主 → 备 ⁺N` 并支持整组回填、单条删除、清空。
- 验证：手动写入旧格式 `a=>b` 的 kv 后加载页面，确认兼容展示与回填。

### 5. 结果表角色徽章与措辞

- [ ] IP 单元格按角色映射追加 `组N·主机/备机/从机` 徽章；跳板对行的分组标签改为主机/备机措辞。
- 验证：`pnpm dev` 触发一次含失败目标的执行（可用不可达 IP），检查徽章与失败行展示。

### 6. i18n

- [ ] `src/locales/messages.ts` en/zh 同步新增 `haGroup*` 键、更新保留键文案、删除废弃键（清单见 design.md §4；删除前全局搜索确认无引用）。
- 验证：`pnpm check`；切换中英文过一遍页面无裸 key。

### 7. 全量质量门

- [ ] `node --test src/lib/applianceSshGroups.test.mjs src/lib/applianceSshPresentation.test.mjs src/lib/sidebarNavigation.test.mjs`
- [ ] `pnpm check` && `pnpm lint`
- [ ] `git diff --stat src-tauri/` 确认为空
- [ ] 提交 git（中文提交信息），执行 `cmd /c pnpm tauri:build:versioned-exe` 验证构建通过

## 评审门

- 步骤 1 完成后：单测通过即可继续，无需人工评审。
- 步骤 3 完成后：截图组卡片 UI 供用户确认布局（主/备/从三输入 + 从机标签输入的排布）。
- 步骤 7 完成后：走 Trellis Phase 3（spec 更新 + 提交 + 收尾）。

## 回滚点

- 每步独立提交或至少步骤 2（组件抽取）单独提交，出现回归可只回滚后续 UI 步骤。
- 全部回滚：`git revert` 对应提交；无持久化/配置迁移需要清理。
