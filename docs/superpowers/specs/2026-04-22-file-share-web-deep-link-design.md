# 文件共享 Web 界面 URL 路由 / 深链设计

- 日期：2026-04-22
- 范围：`src/share-web/*`、`src-tauri/src/fileshare/{ops,http}.rs`
- 不在范围：主 app（`src/main.ts` / `src/router/*`）、屏幕共享、剪贴板、其它工具

---

## 1. 背景与问题

当前文件共享 Web 界面（独立 Vite 入口 `src/share-web/`）没有任何前端路由：

- 浏览器地址栏永远是 `http://<host>:<port>/`，不论用户进入到哪一级目录、是否在搜索
- 刷新 / 书签 / 分享给同事的链接都会回到 home（roots 列表）
- 无法把"某个特定目录"或"某次搜索结果"作为可分享链接发出去

参考工具 CHFS 使用 hash 路径（如 `http://host/#/UMS_TEMP/sub`），用户对该形态已熟悉。

## 2. 目标

- URL ↔ 当前目录状态双向同步
- 直接打开 / 刷新 / 分享深链可以还原目录与搜索状态
- 浏览器后退、前进按钮自然工作
- 不破坏现有的会话、权限、错误回退体验

非目标：

- 不把 dialog（上传 / 重命名 / 删除 / 预览）开关状态进 URL
- 不与 CHFS 的 query 参数命名互通
- 不改变后端节点 ID 编码方案

## 3. URL 形态

### 3.1 形式

```
#/<root>/<seg1>/<seg2>?q=<keyword>&scope=<global|current>
```

| 场景 | URL |
|---|---|
| Home（roots 列表） | `#/` 或空 hash |
| 进入一个 root | `#/UMS_TEMP` |
| 子目录 | `#/UMS_TEMP/2026/Q2` |
| 当前目录搜索 | `#/UMS_TEMP/2026?q=%E6%8A%A5%E8%A1%A8&scope=current` |
| 全局搜索（home 下） | `#/?q=%E6%8A%A5%E8%A1%A8&scope=global` |

### 3.2 编码规则

- 每段路径用 `encodeURIComponent` 单独编码后用 `/` 拼接
- query 用 `URLSearchParams` 序列化（自动 percent-encode）
- 解析时 `decodeURIComponent` 逐段还原；空段（`//` 之间）跳过
- query 中 `scope` 仅接受 `global` / `current`，其它值降级为目录默认（home → global，其他 → current）
- query 中 `q` 为空字符串视为无搜索

### 3.3 Push vs Replace 策略

| 触发方式 | 方法 | 原因 |
|---|---|---|
| 用户点目录、点面包屑、点搜索结果中的目录 | `pushState` | 期望"后退回上级" |
| 提交搜索 / 清空搜索 | `replaceState` | 同目录内的子状态 |
| Bootstrap 后 canonical 校正 | `replaceState` | 程序自我修正 |
| 路径 fallback 到 home（404 / 无权限） | `replaceState` | 程序触发的位置漂移 |
| 删除当前目录后回退 | `replaceState` | 同上 |
| 重命名当前目录 / 祖先后位置变化 | `replaceState` | 同上 |
| 登录后还原原 URL | 不操作 URL | 直接用当前 hash 重新 bootstrap |

### 3.4 Canonical 化

后端 `resolve` 接口返回 canonical segments（根据磁盘真实 entry 名）。前端拿到后若与原 URL 不一致，`replaceState` 校正。

例：用户访问 `#/UMS_TEMP/sub`，磁盘上是 `#/UMS_TEMP/Sub` → 校正成 `#/UMS_TEMP/Sub`。

Root 段不做 canonical 校正——按现有 `find_root` 的行为（大小写敏感匹配 `id` 或 `alias`），任何不匹配直接 404，不存在"找到但大小写不同"的中间态。

## 4. 架构

```
┌──────────────────────────────────────────────────────────────┐
│  浏览器地址栏: http://host:8080/#/UMS_TEMP/sub?q=报表&scope=current
└──────────────────────────────────────────────────────────────┘
            ↓ hashchange / popstate              ↑ replaceState/pushState
┌──────────────────────────────────────────────────────────────┐
│  src/share-web/lib/url-state.ts                              │
│  - parseHash() : { segments, q, scope }                      │
│  - serialize(state) : string                                 │
│  - subscribe(cb)    : popstate + hashchange → cb             │
│  - pushPath / replacePath helpers                            │
└──────────────────────────────────────────────────────────────┘
            ↓ 解析后的 { segments, q, scope }     ↑ 当前 state
┌──────────────────────────────────────────────────────────────┐
│  App.vue                                                     │
│  - onMounted: 读 hash → bootstrapFromUrl(state)              │
│  - subscribe: 浏览器后退 → 重解析 → applyUrlState            │
│  - 在 loadTree / executeSearch / clearSearch 后写回 hash      │
└──────────────────────────────────────────────────────────────┘
            ↓ GET /api/fs/resolve?path=...     ↓ GET /api/fs/tree?node_id=...
┌──────────────────────────────────────────────────────────────┐
│  src-tauri/src/fileshare/                                    │
│  - ops.rs : resolve_path_segments(...) → ResolvedNode         │
│  - http.rs: GET /api/fs/resolve                              │
└──────────────────────────────────────────────────────────────┘
```

**关键不变量**：

- URL 是 state 的"显示层"，唯一真相仍是 `tree.value`（`node_id` 驱动后端）
- `node_id ↔ path` 由后端 tree 响应里的 `breadcrumbs` 桥接：进入目录后从 breadcrumbs 拼 path 写回 URL；从 URL 进来时调 `resolve` 拿 `node_id`
- 所有前端 state 改动 → 写 URL；URL 被外部修改（用户/后退）→ 重新加载

## 5. 前端：`src/share-web/lib/url-state.ts`

新文件，约 80 行，零依赖。

### 5.1 接口

```ts
export type SearchScope = 'global' | 'current';

export interface UrlState {
  segments: string[];     // ['UMS_TEMP', 'sub'] 或 [] 表示 home
  q: string;              // '' 表示无搜索
  scope: SearchScope;     // 默认 'global'
}

export function parseHash(hash?: string): UrlState;

export function serialize(state: UrlState): string;
//   返回值不含 '#'，调用方决定写到哪里
//   - segments=[] q='' → ''
//   - segments=['UMS_TEMP'] q='' → '/UMS_TEMP'
//   - q 非空 → 附加 '?q=...&scope=...'

export function pushPath(state: UrlState): void;
//   history.pushState(null, '', '#' + serialize(state))

export function replacePath(state: UrlState): void;
//   history.replaceState(null, '', '#' + serialize(state))

export function subscribe(cb: (state: UrlState) => void): () => void;
//   绑定 window.popstate + hashchange，返回 unsubscribe
```

### 5.2 解析规则细节

- 空 hash 或 `#`/`#/` → `{ segments: [], q: '', scope: 'global' }`
- `#/UMS_TEMP/sub//c` → `['UMS_TEMP', 'sub', 'c']`（跳过空段）
- 形如 `#?q=foo` 视为 `#/?q=foo`
- 解码失败的段（malformed `%`）整体降级为空 segments（视作 home），写入校正

### 5.3 自写自读循环防护

App.vue 在 watch 里写 URL，subscribe 又监听变化 → 死循环。

- `pushPath` / `replacePath` 内部记录最后一次写出的 hash 字符串
- subscribe 触发时若 `location.hash === lastWritten` 则忽略
- 浏览器后退恢复到旧 hash 时 `lastWritten` 不命中，正常触发

## 6. 前端：`App.vue` 集成点

### 6.1 onMounted

```ts
onMounted(async () => {
  const initial = parseHash();
  await bootstrapFromUrl(initial);
  unsubscribe = subscribe(handleExternalUrlChange);
});
onUnmounted(() => unsubscribe?.());
```

### 6.2 新增 `bootstrapFromUrl(state)`

替代当前裸 `bootstrap()`：

1. 调 `getSession()`——失败走原有 401/403 分支（401 弹 login 但不动 hash；403 显示禁止 IP）
2. 若 `state.segments` 非空：调 `fileShareApi.resolvePath(state.segments)` → 拿 `node_id` 和 canonical segments
   - 失败（404 / 403）→ `pageError = t('app.directoryNotFound')`，`replacePath({ segments: [], q: '', scope: 'global' })`，loadTree(null)
3. 走原有 `loadTree(node_id)`
4. 若 `state.q` 非空且对应 scope 对当前会话有权限：恢复 `keyword.value` / `searchScope.value` 后调 `executeSearch`
   - 若搜索权限被禁，丢弃 q 与 scope 校正 URL
5. canonical 校正：若 resolve 返回的 segments 与原 URL 不一致 → `replacePath` 覆写

### 6.3 登录回调

```ts
async function handleLogin(payload) {
  await fileShareApi.login(...);
  await bootstrapFromUrl(parseHash());
}
```

登录前不动 hash，登录后用当前 hash 重新引导，实现"分享深链 → 弹 login → 登录后直达目标"。

### 6.4 `handleExternalUrlChange(state)`

浏览器后退/前进、或外部代码改 hash：

- 抽出共用 helper `applyUrlState(state, { skipSession })`
- 外部变更走 `skipSession=true` —— 跳过 `getSession()` 重新拉取会话，只走 resolve + loadTree + 搜索还原
- 若 resolve / loadTree / executeSearch 在 `skipSession=true` 路径上返回 401（会话已过期）→ 弹 login，**不动 hash**；登录成功后由 `handleLogin` 用当前 hash 重新 bootstrap，等同于"分享深链 → 登录 → 直达"流程
- 若返回 403 / 404 → 走 §9 错误回退表对应行为

### 6.5 写回 URL 的时机

集中在状态稳定后：

| 现有函数 | 新增动作 |
|---|---|
| `loadTree` 成功后 | 从 `tree.value.breadcrumbs` 拼出 segments；按 `urlAction` 决定 push/replace |
| `executeSearch` 成功后 | `replacePath`（同目录子状态） |
| `clearSearch` | `replacePath` |
| `bootstrapFromUrl` 完成 | `replacePath`（canonical 校正） |

`loadTree` 接受新参数 `urlAction: 'push' | 'replace' | 'none'`：

- `navigate(nodeId)` / `openEntry`（点目录） → `'push'`
- `refreshCurrentView` / 各类 mutation 后的隐式重载 → `'replace'`
- `bootstrapFromUrl` / `applyUrlState` 内部 → `'replace'`

### 6.6 Breadcrumbs → segments

`breadcrumbs` 后端返回结构：`[{node_id: null, name: 'home'}, {node_id, name: <root_alias>}, {node_id, name: <dir>}, ...]`

- 跳过首段（kind=home，node_id=null）
- 其余 `name` 序列即 segments
- 后端已返回 canonical name（来自磁盘读取）

### 6.7 不进 URL 的状态（明确列出）

防误用：

- 任何 dialog 开关：upload / rename / delete / preview / login / new-text / create-directory
- `flashMessage`、各类 `*Error`
- `loadingEntries` / `mutating` / `searching` 等 transient 标志
- `previewSrc` / `previewTitle` / `renameTarget` / `deleteTarget`

## 7. 后端：`/api/fs/resolve`

### 7.1 接口

```
GET /api/fs/resolve?path=<segments joined by /, each url-encoded>
```

**请求**：
- `path` 缺省或空字符串 → 视为 home
- 每段已 URL-decoded 后是真实文件名

**响应 200**：
```json
{
  "node_id": "dir.<base64>.<base64>",
  "kind": "home" | "share_root" | "directory",
  "canonical_segments": ["UMS_TEMP", "2026", "Q2"]
}
```

- home 时 `node_id` 为 `null`，`canonical_segments` 为 `[]`

**错误**：
- 401 Unauthorized：未登录
- 403 Forbidden：IP 禁用 / 该 root 无 browse 权限
- 404 Not Found：root 不存在 / 路径段在磁盘上找不到 / 路径指向文件而非目录

### 7.2 解析逻辑（`ops.rs`）

新增：

```rust
pub fn resolve_path_segments(
    roots: &[ResolvedRoot],
    principal: &ResolvedPrincipal,
    segments: &[String],
) -> Result<ResolvedNode, ResolveError>

pub enum ResolveError { NotFound, Forbidden }

pub struct ResolvedNode {
    pub node_id: Option<String>,           // None 表示 home
    pub kind: ResolveNodeKind,
    pub canonical_segments: Vec<String>,
}

pub enum ResolveNodeKind { Home, ShareRoot, Directory }
```

步骤：

1. `segments.is_empty()` → 返回 `Home`，`node_id=None`，`canonical_segments=[]`
2. 第一段在 `roots` 里按 `id` 或 `alias` 匹配（**大小写敏感**，与现有 `find_root` 一致）。找不到 → `NotFound`
3. 校验该 root 的 browse 权限。无权 → `Forbidden`
4. 后续段：在 root 的 base path 下逐级 `std::fs::read_dir` 查找子目录
   - 段在调用前先排除以下任一情况 → `NotFound`：
     - 段恰好等于 `.` 或 `..`
     - 段为空字符串
     - 段包含 `/`、`\`、`:`、`\0` 中任一字符
   - 只接受 `read_dir` 列出来的真实 entry 名匹配（按 `OsStr` 比较）——从源头杜绝路径穿越
5. canonical 重组：用磁盘读到的真实 entry 名拼出 `canonical_segments`（root 段保留输入字面量，因为 root 段本身就是大小写敏感匹配）
6. 命中目录 → 编码成 `NodeLocator::Directory`（或第一段命中且 segments 长度为 1 → `NodeLocator::ShareRoot`），返回
7. 命中文件 → `NotFound`（resolve 只针对目录）

### 7.3 HTTP 处理（`http.rs`）

- 新增 `ApiResolveQuery { path: Option<String> }`
- 新增 `ApiResolveResponse { node_id: Option<String>, kind: ApiTreeCurrentKind, canonical_segments: Vec<String> }`
  - 复用现有 `ApiTreeCurrentKind`（`home` / `share_root` / `directory`）
- handler 流程：
  1. `require_request_permission(state, headers, ip, FileSharePermission::Browse, false)` 拿 principal —— 401/403 由现有路径产出
  2. 解析 `path` 为 `Vec<String>`：split `/`，逐段 `decodeURIComponent`（用 `percent-encoding` crate；如未引入则手写一个简单 decode，复用现有 `url_encode` 风格）
  3. 调 `resolve_path_segments` → 200 / 404 / 403
- 路由注册：在现有 `Router` 与 `/api/fs/tree` 同级新增 `.route("/api/fs/resolve", get(api_resolve))`

### 7.4 安全

- 只接受磁盘 `read_dir` 实际返回的 entry 名作为路径段——杜绝穿越
- 复用现有 IP 过滤 + session 鉴权中间件链
- 不返回失败具体原因（不区分"段 i 不存在"和"段 i 不是目录"），统一 404，避免目录嗅探

## 8. 前端 API 客户端

`src/share-web/api.ts` 新增：

```ts
async resolvePath(segments: string[]): Promise<{
  node_id: string | null;
  kind: 'home' | 'share_root' | 'directory';
  canonical_segments: string[];
}>
```

- 调用 `GET /api/fs/resolve?path=<segments.map(encodeURIComponent).join('/')>`
- 复用现有 `getErrorMessage` / `isUnauthorized` / `isForbidden` / `isNotFound`

## 9. 错误与回退

| 触发 | 期望行为 |
|---|---|
| Resolve 404 | `pageError = t('app.directoryNotFound')`；`replacePath({ segments: [], q: '', scope: 'global' })`；loadTree(null) |
| Resolve 403 | 同上但提示文案为 `t('app.forbiddenDirectory')`（i18n 新增 key） |
| Resolve 401 | 弹 login，**不动 hash**；登录后用当前 hash 重新 bootstrap |
| Tree 加载 401 / 403 | 现有 `handleMutationError` / `bootstrap` 失败逻辑保持不变 |
| 删除当前目录 | 现有 `submitDelete` 已回退到 home；`refreshCurrentView` 完成后从 breadcrumbs 重新 replacePath |
| 重命名当前目录 / 祖先 | breadcrumbs 自然带新名 → loadTree 后 replacePath 写出新路径 |
| 切换账号导致权限缩小 | 现有 `handleMutationError` 已重新 bootstrap；新路径若不可达走上面的回退 |

## 10. 国际化

`app.directoryNotFound` 与 `app.forbiddenIp` 已存在于 [src/share-web/messages.ts](../../../src/share-web/messages.ts)，复用。

新增（`en` + `zh` 各一份）：

| key | en | zh |
|---|---|---|
| `app.forbiddenDirectory` | You don't have access to this directory. | 无权访问该目录。 |

## 11. 测试

### 11.1 后端（Rust）

新增到 `ops.rs` 的 `mod tests`：

| 测试 | 验证 |
|---|---|
| `resolve_empty_path_returns_home` | 空 segments → kind=Home, node_id=None |
| `resolve_root_only_returns_share_root` | `["UMS_TEMP"]` → kind=ShareRoot, canonical=["UMS_TEMP"] |
| `resolve_nested_directory` | `["UMS_TEMP", "sub", "leaf"]` → kind=Directory，canonical 来自磁盘 |
| `resolve_corrects_subdirectory_case_from_disk` | 输入 `["UMS_TEMP", "SUB"]`，磁盘是 `sub` → canonical=["UMS_TEMP", "sub"] |
| `resolve_unknown_root_returns_not_found` | 不存在 root → NotFound |
| `resolve_missing_segment_returns_not_found` | 中间任一段不存在 → NotFound |
| `resolve_file_segment_returns_not_found` | 路径指向文件 → NotFound |
| `resolve_rejects_path_traversal` | 段含 `..` / `.` / `/` / `\` / 空 / `:` → NotFound |
| `resolve_without_browse_permission_is_forbidden` | principal 无 browse → Forbidden |

HTTP 层加一条 happy path + 一条 NotFound 的 axum 测试，验证状态码与 JSON 结构。

### 11.2 前端（vitest）

引入 `vitest`（devDependency）+ `pnpm test:share-web` script。

新增 `src/share-web/lib/__tests__/url-state.test.ts`：

| 测试 | 验证 |
|---|---|
| `parseHash('')` | → `{ segments: [], q: '', scope: 'global' }` |
| `parseHash('#/')` | 同上 |
| `parseHash('#/UMS_TEMP/sub')` | segments=['UMS_TEMP','sub'] |
| `parseHash('#/UMS_TEMP//sub')` | 跳过空段 |
| `parseHash('#/UMS_TEMP?q=foo&scope=current')` | 含 q + scope |
| `parseHash('#/?scope=invalid')` | scope 非法 → 默认 global |
| `parseHash` 中文 | percent-encoded 段正确 decode |
| `serialize` round-trip | parseHash(serialize(state)) === state |
| `serialize` 中文 | 段正确 percent-encode |
| `subscribe` 去抖 | push 自己写的 hash 不触发 cb |
| `subscribe` 外部 | 模拟 popstate → cb 收到新 state |

vitest 配置：在仓库根加 `vitest.config.ts`，仅扫描 `src/share-web/**/*.test.ts`，复用现有 vite 配置（`@vitejs/plugin-vue` 已装）。jsdom 环境用于触发 `window.popstate` / `location.hash`。

### 11.3 端到端（手测，构建后浏览器验证）

实施完成后必须人工跑一遍：

1. 直接打开 `http://host:8080/` → 看到 home，URL 为 `/`（hash 为空）
2. 点进任一 root → URL 变 `#/<root>`，浏览器后退回到 home
3. 进多级目录 → URL 累加段；前进后退按预期切换
4. 复制深链到无痕窗口（同 IP）→ 直达目标目录
5. 在子目录里搜索 → URL 加 `?q=&scope=current`；刷新仍在目标目录 + 搜索结果
6. 输入不存在路径 `#/不存在` → 回退 home + 顶部错误提示，URL 校正为 `#/`
7. 退出登录 → 再粘贴深链 → 弹 login → 登录后直达目标
8. 路径段带空格 / 中文 / `?` / `#` → percent-encoded 进 URL，回来正确还原
9. 大小写不一致 `#/UMS_TEMP/SUB`（磁盘是 `sub`） → 加载后 URL 校正成 `#/UMS_TEMP/sub`
10. 在某目录下切换搜索关键词多次 → 浏览器历史只增加一条（replace 生效）

## 12. 不在范围

- 主 app 路由（`src/router/index.ts`）—— share-web 是独立 Vite 入口，互不影响
- 与 CHFS URL query 参数互通——只在路径形态参考 CHFS
- 节点 ID 编码方案变更—— `node_id` 内部结构保持现状
- 文件预览状态进 URL（已在澄清阶段排除）

## 13. 验证

按 [CLAUDE.md](../../../CLAUDE.md) 规则，实施完成后：

- `cmd /c pnpm tauri:build:versioned-exe` 必须通过（自动包含 `pnpm build:file-share-web`）
- `pnpm test:share-web` 必须通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 中新增的 ops 测试必须通过
- 第 11.3 节手测清单全部通过
