# 技术设计 — 主备（从）接入组

需求与验收见 [prd.md](./prd.md)。本设计为**纯前端改造**：后端 `enable_appliance_ssh` 命令、`ApplianceSshTarget { ip, jump_host }` 请求结构、执行流程（含降级、端口解析、凭据解析）全部原样复用。

## 1. 现有能力映射（为什么后端零改动）

`src-tauri/src/main.rs` 的 `enable_appliance_ssh_for_target`（约 L3057）已实现两条路径：

- **直连**（`jump_host: None`）：`get`/`set` 管理接口开 SSH → 直连 SSH 执行 `build_iptables_whitelist_command`。
- **跳板对**（`jump_host: Some(A)`）：接口开 A 的 SSH → SSH 登 A → `build_nested_iptables_whitelist_command`（A 本机加规则 && 免密 `ssh` 到 B 加规则）；接口不可达时降级为纯 SSH 通道。

主备从组按角色拆解为上述两类 target 即可，组语义只存在于前端（组装与结果展示）。

## 2. 数据模型与纯逻辑模块

新文件 `src/lib/applianceSshGroups.ts`（纯函数，无 Vue/Tauri 依赖，便于 `node --test`）：

```ts
export interface HaAccessGroup {
  master: string;   // 主机 IP，trim 后必填
  backup: string;   // 备机 IP，可空
  slaves: string[]; // 从机 IP，0..MAX_SLAVES_PER_GROUP
}

export const MAX_SLAVES_PER_GROUP = 10;

export function isValidIp(ip: string): boolean;            // 从页面内联实现迁移至此
export function normalizeGroup(g: HaAccessGroup): HaAccessGroup; // trim + 去空
export function isGroupActive(g: HaAccessGroup): boolean;  // master 非空即参与执行

// 组 → 后端 target 列表（不含去重；顺序：主/备对 → 从机）
export function buildGroupTargets(g: HaAccessGroup): ApplianceSshTarget[];

// 全量组装：手动 IP + 服务器勾选 + 各组 → 去重后的 targets
// 直连按 ip 去重；跳板对按 `${jumpHost}=>${ip}` 去重
export function composeAllTargets(
  directIps: string[],
  groups: HaAccessGroup[],
): ApplianceSshTarget[];

// 结果角色映射：key = `${jumpHost ?? ''}=>${ip}` → { groupIndex, role }
// 跳板对一行同时承载主机与备机状态，故该行角色为合并的 masterBackup（徽章"主备"）；
// 无备机时主机为直连 target，角色 master。
export type HaRole = 'masterBackup' | 'master' | 'slave';
export function buildRoleMap(groups: HaAccessGroup[]): Map<string, { groupIndex: number; role: HaRole }>;

// 最近记录序列化：`master=>backup=>s1,s2`（backup/从机可为空段）
export function serializeGroup(g: HaAccessGroup): string;
export function parseGroupEntry(raw: string): HaAccessGroup | null; // 兼容旧 `a=>b` 两段格式
```

拆解规则（`buildGroupTargets`）：

| 组形态 | 产出 targets |
|--------|--------------|
| 主+备(+从…) | `{ip: backup, jumpHost: master}` + 每从机 `{ip: slave}` |
| 主+从…（无备） | `{ip: master}` + 每从机 `{ip: slave}` |
| 仅主 | `{ip: master}`（退化为直连，允许但 UI 提示意义不大，不特殊处理） |

角色映射说明：主+备组中，主机的开启/白名单状态承载在跳板对 result 的 jump-host 字段组里（`previousEnable/currentEnable/port` 属于主机，`whitelistApplied` 属于备机）——现有结果表已按此分组渲染，只需把标签文案从"跳板机/目标"改为"主机/备机"。

## 3. 组件与页面改造

### 3.1 抽取 `src/components/IpTagInput.vue`

页面现有"手动输入 IP"标签输入逻辑（分隔符切分、Enter/Tab/空格确认、Backspace 回填、点击标签编辑、粘贴、失焦确认、无效 IP 红标）整体抽为可复用组件：

- Props：`modelValue: string[]`、`disabled`、`placeholder`、`maxTags?: number`（达到上限时拒绝新增并触发 `limit-exceeded` 事件）、`datalistId?: string`（手动 IP 场景保留最近 IP datalist）。
- 手动 IP 区块与组内从机输入共用；行为与现状逐项等价（手动 IP 场景不设 `maxTags`）。

### 3.2 组区块（替换跳板机卡片）

`EnableApplianceSshPage.vue` 中原"经由跳板机的目标"卡片位置替换为：

```
┌ 主备（从）接入组（可选） ───────────────── [+ 添加一组] ┐
│ ┌ 组 1 ──────────────────────────────────────── [×] ┐ │
│ │ [主] 192.168.1.10        [备(可选)] 192.168.1.11   │ │
│ │ [从机 0~10] (IpTagInput 标签输入)         n/10      │ │
│ └───────────────────────────────────────────────────┘ │
│ 主机 SSH 端口 [23333]        ← 仅当任一组含备机时展示     │
│ 最近使用：〔.10 → .11 ⁺²〕〔…〕      ← 胶囊，点击回填整组   │
└───────────────────────────────────────────────────────┘
```

- 状态：`haGroups = ref<HaAccessGroup[]>([])`；替换原 `jumpHostPairs`。
- `hasAnyBackup = computed(...)` 替换 `hasAnyJumpHost` 的语义（主机 SSH 端口、主机独立凭据、结果渲染均以此为开关；变量可保名或改名，UI 文案统一"主机"措辞）。
- 主/备输入框沿用现有单行 input 样式与红框校验；从机用 `IpTagInput`（`maxTags=10`，超限 toast 提示）。
- 已选目标汇总胶囊：组产出的 target 按现有 `jump → ip` / `ip` 形式展示，前缀补组内角色（如 `组1·备 .11`），非组目标不变。

### 3.3 提交与结果

- `handleExecute` 改用 `composeAllTargets(directTargetIps, activeGroups)`；请求其余字段不变（`jumpHostUseSeparateCreds`、`jumpHostSshPort` 等继续按 `hasAnyBackup` 传递）。
- 提交前用 `buildRoleMap` 生成本次执行的角色映射 `ref`；结果表渲染时按 `${result.jumpHost ?? ''}=>${result.ip}` 查角色，在 IP 单元格追加徽章 `组N·主机/备机/从机`（i18n）。查不到映射的行（手动 IP/服务器勾选）不显示徽章。
- 跳板对行现有的 `jumpHostGroupLabel` / `targetGroupLabel` / `viaJumpHost` 文案改为主机/备机措辞（键新增 `haGroup*`，旧键删除）。
- 汇总卡（总数/成功/失败）保持按 target 计数，不按组聚合（YAGNI，失败定位靠角色徽章已足够）。

### 3.4 白名单来源默认值

`whitelistSourceMode` 初始值 `'local'` → `'all'`；选中"全部放行"时的琥珀色提示保留。范围默认 `allTcp` 现状即是，不动。

### 3.5 最近使用记录

- kv 键沿用 `applianceSsh.recentJumpHostPairs`（避免迁移逻辑）：新条目 `master=>backup=>s1,s2`，`parseGroupEntry` 兼容两段旧格式；上限 5 条不变。
- 胶囊展示 `主 → 备`，有从机时追加 `⁺N`（title 展示完整从机列表）；点击回填整组（复用空组行或追加）。

## 4. i18n

`messages.ts` en/zh 同步：

- 新增 `tools.applianceSsh.haGroup*` 系列：区块标题/说明、组序号、主/备/从标签与占位、从机上限提示、角色徽章、最近使用、回填等。
- 改文案：`viaJumpHost`（"经由跳板机"→"经由主机"）等结果表措辞并入 `haGroup*` 新键。
- 删除废弃键：`jumpHostSection`、`jumpHostAdd`、`jumpHostEmptyHint`、`jumpHostRowHint`、`jumpHostIpPlaceholder`、`jumpHostTargetPlaceholder`、`jumpHostRemove`、`jumpHostRecent`、`clearRecentJumpHost`、`removeRecentJumpHost`、`jumpHostGroupLabel`、`targetGroupLabel`、`viaJumpHost`（若仍被复用则保留并改文案，以实际引用为准）。
- 保留改措辞：`jumpHostSshPort*`（→ 主机 SSH 端口）、`jumpHostSeparateCreds`（→ 主机独立凭据）、`jumpHostUsername/Password`（键名不动，仅改显示文案，后端请求字段名不变）。

## 5. 兼容性与风险

- **后端契约不变**：`tauri.ts` 的 `ApplianceSshTarget.jumpHost`、请求字段名全部保留；`git diff src-tauri/` 应为空。
- **旧最近记录**：两段格式解析为无从机组，天然兼容；不做数据迁移。
- **行为回归面**：主+备组必须与旧跳板对逐项等价（端口解析优先级、独立凭据、降级契约见 git log `docs(appliance-ssh)` 提交）——实现时不触碰后端即可保证。
- **并发**：一组最多 11 个 target（1 对 + 10 从机），由现有 `DEVICE_BATCH_CONCURRENCY_LIMIT` 控制，无新增风险。
- **回滚**：单次（或少量）提交，`git revert` 即可；无配置 schema、无持久化格式破坏。

## 6. 测试策略

- `src/lib/applianceSshGroups.test.mjs`（`node --test`）：
  - `buildGroupTargets` 三种组形态；仅主退化；空组过滤。
  - `composeAllTargets` 去重（直连跨来源重复、跳板对重复、直连与备机同 IP 不互吞——key 含 jumpHost）。
  - `serializeGroup`/`parseGroupEntry` 往返 + 旧两段格式 + 空段/脏数据容错。
  - `buildRoleMap` 角色与组号正确、主备组的 master 角色落在跳板对 key 上。
  - 从机上限常量边界（10 允许 / 11 拒绝，由调用方约束时的裁剪行为）。
- 手动验证：真实一体机环境跑 主+备+从、主+从、主+备 三种组合（结果徽章、日志 `[appliance-access]` 无异常）。
