# UMS 初始密码修改 — 设计文档

**日期**: 2026-07-28
**范围**: 将现有「框架密码修改」工具改造为「UMS 初始密码修改」，包含框架密码、UMS 密码、CDM 密码三种可勾选的修改流程。
**实现分支**: `main`
**现状入口**: [FrameworkPasswordPage.vue](src/pages/FrameworkPasswordPage.vue)、[main.rs:3436-3623](src-tauri/src/main.rs#L3436-L3623)

---

## 0. 已确认的产品决策

| 决策点 | 结论 |
| --- | --- |
| 地址模型 | **三种流程共用一份 IP 列表**，沿用现有的服务器勾选 + 手输标签 + 最近使用 |
| 密码模型 | **统一新密码**（一个输入框对三者生效）+ **每类各自可编辑的旧密码**，预填出厂默认值 |
| UMS `LoginExtInfo.IpAddress` | **自动探测本机可达 IP**，不暴露给用户 |
| 默认勾选 | 三者**全部勾选**，可任意取消 |

---

## 1. 现状盘点

### 1.1 现有实现

```
FrameworkPasswordPage.vue
  ├─ selectedIps（服务器勾选） + manualIpTags（手输标签） + recentIps（save_kv 持久化）
  ├─ oldPassword '123456' / newPassword 'admin_123'
  ├─ apiTimeoutSecs → config.framework_password_api_timeout_secs
  └─ invoke change_framework_password(ips, oldPassword, newPassword)
        → Vec<PasswordChangeResult { ip, success, message, failedAt }>

main.rs
  change_framework_password  (command，读 timeout，建 client，run_ordered_with_limit 并发 4)
    └─ change_framework_password_for_ip
         POST http://{ip}:21900/openAPI/userMgr/v1/login        {userName, userPasswd:SHA256(old), isUnlockLogin:false}
         POST http://{ip}:21900/openAPI/userMgr/v1/changePasswd {userName, oldUserPasswd, newUserPasswd}
         POST http://{ip}:21900/openAPI/userMgr/v1/logout       （忽略结果）
       成功判据：JSON `code == 0`
```

### 1.2 需要同步改名的引用点

| 文件 | 内容 |
| --- | --- |
| [src/router/index.ts:75](src/router/index.ts#L75) | 路由 `/tools/framework-password` |
| [src/lib/sidebarNavigation.ts:87-93](src/lib/sidebarNavigation.ts#L87-L93) | 侧边栏项 `framework-password` / `sidebar.frameworkPassword` / `iconKey` |
| [src/lib/sidebarNavigation.ts:6](src/lib/sidebarNavigation.ts#L6) | `SidebarIconKey` 联合类型成员 |
| [src/components/Sidebar.vue:45](src/components/Sidebar.vue#L45) | 图标映射 `frameworkPassword: KeyRound` |
| [src/pages/ToolsHubPage.vue:56-62](src/pages/ToolsHubPage.vue#L56-L62) | 工具卡片 key / titleKey / descriptionKey / path / chipKey |
| [src/locales/messages.ts](src/locales/messages.ts) | 4 处：`sidebar.frameworkPassword`(L14/L3191)、`tools.frameworkPassword`(L636/L3812)、`toolsHub.cards.frameworkPassword`(L1077/L4253) |
| [src/lib/sidebarNavigation.test.mjs](src/lib/sidebarNavigation.test.mjs)、[src/pages/FrameworkPasswordPage.test.mjs](src/pages/FrameworkPasswordPage.test.mjs) | 断言中的 key / 文件名 |
| [README.md](README.md) | 工具清单 |

---

## 2. 三条协议流程（含已验证的签名算法）

### 2.1 框架（保持不变）

端口 `21900`，SHA-256 明文哈希，`code == 0` 判成功。**不改协议，只改被调用的位置。**

### 2.2 UMS（端口 80）

```
① POST http://{ip}/sw/login                      body: 真空（无 body，非 "{}"）
   ← { "AccessCode": "...", "Encryption": "MD5" }

② POST http://{ip}/sw/login
   body: { UserName, AccessCode, LoginSignature, isNewVersion: true,
           ip: <目标IP>, languageType: "zh_cn",
           LoginExtInfo: { IpAddress: <本机IP> }, ClientIp: "" }
   ← { "AccessToken": "...", ... }

③ GET  http://{ip}/sw/servers/public/key         header: Authorization: <AccessToken>
   ← { errCode: 0, result: { publicKey: <base64 SPKI> } }

④ PUT  http://{ip}/sw/user/update/passwd         header: Authorization: <AccessToken>
   body: { userCode, userName,
           newUserPasswd:   RSA(new),
           userPasswd:      RSA(old),
           NewEncPassword:  RSA(new) }
   ← { errCode: 0, errMsg: "成功" }

⑤ POST http://{ip}/sw/switch/value/dictionary/set
   body: { createTime, description: "loadmin密码初始化开关",
           key: "pwdIsInit", name: "pwdIsInit", updateTime, value: "true" }
   ← { errCode: 0, errMsg: "成功" }
```

**签名算法已用示例数据验证通过**：

```
LoginSignature = MD5( Base64(UserName) + AccessCode + MD5(password) )

Base64("loadmin")                              = "bG9hZG1pbg=="
MD5("bG9hZG1pbg==" + "02630335275641340780" + MD5("admin_123"))
                                               = f1416bd8caf9243c25ac05c9cc121a07  ✓ 与示例一致
```

**RSA**：`result.publicKey` 是 base64 的 SPKI DER，RSA-2048；三个密文均为 344 base64 字符 = 256 字节，与 2048 位模长吻合。`newUserPasswd` 与 `NewEncPassword` 密文不同但同为新密码——PKCS#1 v1.5 填充带随机数，同一明文两次加密必然不同，因此**这两个字段是新密码的两次独立加密**（历史字段 + 新字段并存）。

### 2.3 CDM（端口 25011）

```
① POST   http://{ip}:25011/cdm/civetweb/login_v1   body: 真空
   ← { "AccessCode": "...", "MD5": "MD5" }

② POST   http://{ip}:25011/cdm/civetweb/login_v2
   body: { UserName, AccessCode, LoginSignature }
   ← { "Authorization": "SDK@4-..." }

③ PUT    http://{ip}:25011/cdm/civetweb/passwd     header: authorization: <Authorization>
   body: { UserName, OldPassword: MD5(old), NewPassword: MD5(new) }
   ← 空响应

④ DELETE http://{ip}:25011/cdm/civetweb/logout     header: authorization: <Authorization>
   ← 空响应
```

**签名算法与 UMS 不同——是拼接，不再外层哈希**：

```
LoginSignature = MD5(UserName) + AccessCode + MD5(password)     ← 无外层 MD5

示例：32 + 31 + 32 = 95 字符
"21232f297a57a5a743894a0e4a801fc3" + "1234567895201785240293599698403" + "21232f297a57a5a743894a0e4a801fc3"
 = MD5("admin")                      = AccessCode                       = MD5("admin")
```

示例中的 `NewPassword` = `d6bf4bb9a66419380a7e8b034270d381` = **MD5("admin_123")**，与框架的目标新密码一致，印证了「统一新密码」的产品语义。

### 2.4 三者差异汇总（决定实现分层）

| 维度 | 框架 | UMS | CDM |
| --- | --- | --- | --- |
| 端口 | 21900 | 80 | 25011 |
| 账号 | `admin` | `loadmin` | `admin` |
| 出厂旧密码 | `123456` | `admin_123` | `admin` |
| 哈希 | SHA-256 | MD5 + RSA | MD5 |
| 挑战握手 | 无 | 有（AccessCode） | 有（AccessCode） |
| 签名构造 | — | 外层 MD5 | 纯拼接 |
| 令牌头 | `Authorization` | `Authorization` | `authorization` |
| 修改方法 | POST | **PUT** | **PUT** |
| 成功判据 | `code == 0` | `errCode == 0` | **HTTP 状态码**（响应体为空） |
| 收尾步骤 | logout | **字典开关 `pwdIsInit`** | logout（DELETE） |

**关键结论**：三者差异大到不适合抽象成「一个带参数的通用流程」。应各自写成独立的 `async fn`，只共享 HTTP client、结果结构、并发调度和错误封装。

---

## 3. Rust 侧改造

### 3.1 新增依赖（[src-tauri/Cargo.toml](src-tauri/Cargo.toml)）

```toml
md-5 = "0.10"                                    # MD5，digest API 与已有 sha2 一致
rsa  = { version = "0.9", features = ["sha2"] }  # PKCS#1 v1.5 加密 + pkcs8::DecodePublicKey
```

`base64 = "0.22"`、`rand = "0.8"`、`sha2 = "0.10"` 已在依赖中，直接复用。`rsa 0.9` 依赖 `rand_core 0.6`，与 `rand 0.8` 的 `ThreadRng` 兼容，无需再引入 `rand` 版本。

> 提醒：MD5 与 PKCS#1 v1.5 均为弱算法，此处是**对端协议强制要求**，仅用于兼容既有设备接口，不作为本项目的安全基线。建议在模块头注释中写明这一点，避免后续安全扫描误判。

### 3.2 新增模块 `src-tauri/src/ums_init_password.rs`

`main.rs` 已超过 4600 行，不宜继续堆叠。建议抽独立模块，把现有 `change_framework_password_for_ip` 一并搬进去（`sha256_hex`、`password_change_failure`、`validate_ip`、`build_device_http_client_with_timeout`、`DEVICE_BATCH_CONCURRENCY_LIMIT` 改为 `pub(crate)` 复用）。

```rust
// ── 请求 / 结果结构 ──────────────────────────────────────────
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordRequest {
    pub ips: Vec<String>,
    pub targets: UmsInitPasswordTargets,   // 勾选状态
    pub new_password: String,              // 统一新密码
    pub framework_old_password: String,    // 默认 "123456"
    pub ums_old_password: String,          // 默认 "admin_123"
    pub cdm_old_password: String,          // 默认 "admin"
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordTargets {
    pub framework: bool,
    pub ums: bool,
    pub cdm: bool,
}

#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UmsInitPasswordKind { Framework, Ums, Cdm }

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordTargetResult {
    pub kind: UmsInitPasswordKind,
    pub success: bool,
    pub message: String,
    /// 失败阶段：login / publicKey / changePasswd / dictionary / logout
    pub failed_at: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordResult {
    pub ip: String,
    /// 该 IP 上所有被勾选的流程是否都成功
    pub success: bool,
    pub targets: Vec<UmsInitPasswordTargetResult>,
}
```

**为什么不复用 `PasswordChangeResult`**：现在一个 IP 会产生最多 3 条结果。用「一行一 IP、内嵌 targets」而不是「一行一 (IP, target)」，是为了让表格能按 IP 折叠、统计口径保持「N 台机器中 M 台完全成功」，与用户对「初始化一批设备」的心智一致。

### 3.3 命令与调度

```rust
#[tauri::command]
pub async fn change_ums_init_password(
    request: UmsInitPasswordRequest,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<UmsInitPasswordResult>, String> {
    // 1. 读 timeout → build_device_http_client_with_timeout
    // 2. run_ordered_with_limit(ips, DEVICE_BATCH_CONCURRENCY_LIMIT, ...)
    // 3. 每个 IP 内部：框架 → UMS → CDM 顺序执行
}
```

**并发策略**：外层按 IP 并发（沿用 `DEVICE_BATCH_CONCURRENCY_LIMIT = 4`），**单个 IP 内部三种流程串行**。理由：

- 三条流程打的是同一台/同网段设备，串行避免瞬时压力；
- 结果顺序确定，UI 不需要额外排序；
- **一种流程失败绝不能中断另外两种**——每条流程独立 `catch` 成 `UmsInitPasswordTargetResult`，不用 `?` 向上传播。这是本次改造最容易写错的地方。

`validate_ip` 失败时，为所有被勾选的流程各生成一条 `failed_at: "login"` 的失败记录，而不是整体丢弃。

### 3.4 加密工具函数

```rust
fn md5_hex(plaintext: &str) -> String;                       // 与 sha256_hex 同风格

/// UMS：MD5( Base64(user) + AccessCode + MD5(pwd) )
fn ums_login_signature(user: &str, access_code: &str, pwd: &str) -> String;

/// CDM：MD5(user) + AccessCode + MD5(pwd)  —— 拼接，无外层哈希
fn cdm_login_signature(user: &str, access_code: &str, pwd: &str) -> String;

/// base64(SPKI DER) → RsaPublicKey → PKCS#1 v1.5 加密 → base64 密文
fn rsa_pkcs1v15_encrypt_base64(public_key_b64: &str, plaintext: &str) -> Result<String, String>;
```

`rsa_pkcs1v15_encrypt_base64` 内部：`BASE64.decode` → `RsaPublicKey::from_public_key_der` → `encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, plaintext.as_bytes())` → `BASE64.encode`。每次调用独立随机填充，因此 `newUserPasswd` 与 `NewEncPassword` 分两次调用即可自然得到不同密文。

### 3.5 本机 IP 探测（UMS `LoginExtInfo.IpAddress`）

```rust
/// 返回访问 target_ip 时本机会使用的源 IP。
fn detect_local_ip_for(target_ip: &str) -> Option<String>
```

推荐实现顺序：

1. **UDP connect 探针**：`UdpSocket::bind("0.0.0.0:0")` → `connect((target_ip, 80))` → `local_addr().ip()`。不发包，只让内核做一次路由查询，得到的就是真实源地址。
2. **过滤伪 IP 段**：本机存在 fake-IP 代理 TUN（`198.18.0.0/15`）时，探针可能返回 TUN 地址。命中该段、回环、`169.254.0.0/16` 时判定无效，进入下一步。
3. **网卡枚举兜底**：`local_ip_address::list_afinet_netifas()`（依赖已在，[fileshare/mod.rs:1147](src-tauri/src/fileshare/mod.rs#L1147)、[screenshare.rs:7656](src-tauri/src/screenshare.rs#L7656) 有现成用法）过滤同上，优先选与 `target_ip` 同 `/24` 的地址，否则取第一个可用私网地址。
4. 全部失败 → 该字段填 `""`，**不阻断流程**（服务端是否强校验待实机确认，见 §7.2）。

**按目标 IP 逐台探测**，不全局算一次——目标可能分布在不同网段（示例中 UMS 在 `192.115.2.x`、CDM 在 `192.115.1.x`）。

### 3.6 命令注册

[main.rs:4685](src-tauri/src/main.rs#L4685) 的 `invoke_handler` 中把 `change_framework_password` 替换为 `change_ums_init_password`。旧命令**直接删除**（无外部调用方，前端是唯一消费者）。

---

## 4. 配置

`framework_password_api_timeout_secs` 是唯一相关配置项。

**建议：保留字段名不变。** 它是纯粹的 HTTP 超时值，三种流程共用一个超时完全合理，改名要动 7 处（[config.rs](src-tauri/src/config.rs) 结构体 + `AppDomainConfigPatch` + `apply_app_patch` + default fn + 测试、[tauri.ts:180/237](src/lib/tauri.ts#L180)、[configDomains.ts:31](src/lib/configDomains.ts#L31)）并需要 serde 迁移，收益仅是命名整洁。

若坚持改名为 `ums_init_password_api_timeout_secs`，必须加 `#[serde(alias = "framework_password_api_timeout_secs")]` 保证旧配置文件仍可加载，并同步更新 `configDomains.test.mjs` / `configStore.test.mjs` 中的字段。

**顺带修正**：现有页面 [FrameworkPasswordPage.vue:183-185](src/pages/FrameworkPasswordPage.vue#L183-L185) 保存超时用的是整对象 `saveConfig(config.value)`，与 CLAUDE.md 的域级补丁约定不符。新页面应改用 `configStore.saveApp()`。

---

## 5. 前端改造

### 5.1 类型（[src/lib/tauri.ts](src/lib/tauri.ts)）

删除 `FrameworkPasswordResult` / `changeFrameworkPassword`，新增：

```ts
export type UmsInitPasswordKind = 'framework' | 'ums' | 'cdm';

export interface UmsInitPasswordTargets {
  framework: boolean;
  ums: boolean;
  cdm: boolean;
}

export interface UmsInitPasswordTargetResult {
  kind: UmsInitPasswordKind;
  success: boolean;
  message: string;
  failedAt?: 'login' | 'publicKey' | 'changePasswd' | 'dictionary' | 'logout';
}

export interface UmsInitPasswordResult {
  ip: string;
  success: boolean;
  targets: UmsInitPasswordTargetResult[];
}

export interface UmsInitPasswordRequest {
  ips: string[];
  targets: UmsInitPasswordTargets;
  newPassword: string;
  frameworkOldPassword: string;
  umsOldPassword: string;
  cdmOldPassword: string;
}

export async function changeUmsInitPassword(
  request: UmsInitPasswordRequest,
): Promise<UmsInitPasswordResult[]> {
  return await invoke('change_ums_init_password', { request });
}
```

### 5.2 页面 `src/pages/UmsInitialPasswordPage.vue`

由 `FrameworkPasswordPage.vue` 改名而来。**IP 选择区、手输标签、最近使用、超时选择、结果统计卡全部原样保留**（这部分交互已经打磨过，不动）。改动集中在密码卡片和结果表。

```
┌─ 头部：UMS 初始密码修改 ────────────────────────────────┐
└─────────────────────────────────────────────────────────┘
┌─ 说明横幅：三种流程 / 端口 / 账号 ──────────────────────┐
└─────────────────────────────────────────────────────────┘

┌─ 服务器勾选（不变）────────────────────────────────────┐
└─────────────────────────────────────────────────────────┘

┌─ 修改范围 + 密码配置（本次核心新增）───────────────────┐
│  新密码（统一）  [admin_123          ] 👁              │
│  ─────────────────────────────────────────────────────  │
│  [✓] 框架密码修改   :21900  admin                       │
│        旧密码 [123456    ] 👁                           │
│  [✓] UMS 密码修改   :80     loadmin                     │
│        旧密码 [admin_123 ] 👁                           │
│  [✓] CDM 密码修改   :25011  admin                       │
│        旧密码 [admin     ] 👁                           │
└─────────────────────────────────────────────────────────┘

┌─ 手输 IP / 最近使用（不变）────────────────────────────┐
└─────────────────────────────────────────────────────────┘
┌─ 已选 IP / 超时 / 执行按钮（不变）─────────────────────┐
└─────────────────────────────────────────────────────────┘

┌─ 结果表：每 IP 一行，内嵌 3 个状态徽章 ────────────────┐
│  IP              框架    UMS     CDM    详情            │
│  192.115.2.38    ✓成功   ✓成功   —未选  ...            │
│  192.115.1.17    ✗失败   —未选   ✓成功  登录失败:...   │
└─────────────────────────────────────────────────────────┘
```

**校验规则**（沿用 memory 中「校验跟着主操作走，不做单独的检查按钮」的约定）：

- 至少勾选一种流程，否则执行按钮禁用；
- `newPassword` 非空；
- **同名冲突校验按勾选项逐个判断**：现有的 `oldPassword === newPassword` 单一校验要改成——被勾选的流程中，任一其 `旧密码 === 新密码` 即在该行下方标黄提示并禁用执行。注意 UMS 默认旧密码就是 `admin_123`，如果用户把新密码也填 `admin_123`（框架/CDM 的典型目标值），UMS 这一项必然触发冲突。这是**默认配置下就会命中的常见场景**，提示文案要说清是哪一项冲突，不能只给一句笼统的「新旧密码不能相同」。

**图标**：三个流程的行前图标用 `lucide-vue-next` 的 `Layers`（框架）、`Building2`（UMS）、`Database`（CDM），页面主图标沿用 `KeyRound`。不使用 Emoji。

### 5.3 结果表实现要点

保留现有测试锁定的两个类常量语义（[FrameworkPasswordPage.test.mjs](src/pages/FrameworkPasswordPage.test.mjs) 断言 `whitespace-nowrap` 状态列 + `break-all` 消息列 + `table-fixed`）。新表列宽：IP 160px / 三个状态列各 110px / 详情列自适应 `break-all`。测试文件同步改名并更新常量名。

### 5.4 i18n

新增 `tools.umsInitialPassword.*` 命名空间，中英双语同步。现有 `tools.frameworkPassword.*` 的键大部分可平移（IP 输入、最近使用、统计、超时等），新增键约 20 个：

```
title / description / info / infoDetail
scope.legend / scope.framework / scope.ums / scope.cdm
scope.hint（说明账号与端口）
newPassword / newPasswordPlaceholder / newPasswordHint
oldPasswordFor（"{target} 旧密码"）
samePasswordFor（"{target} 的新旧密码不能相同"）
noTargetSelected
column.framework / column.ums / column.cdm / column.detail
targetSkipped（"未选"）
completed（"完成：{success}/{total} 台全部成功"）
```

`sidebar.frameworkPassword` → `sidebar.umsInitialPassword`（中文「UMS 初始密码修改」），`toolsHub.cards.frameworkPassword.chip` → `umsInitialPassword.chip`（「密码」）。

### 5.5 `save_kv` 键名

`frameworkPassword.recentIps` → `umsInitialPassword.recentIps`。**建议在首次加载时做一次迁移**：读新键为空时回落读旧键并写入新键，避免用户丢失已积累的最近 IP 列表。这是 10 行代码换掉一次用户可感知的数据丢失，值得做。

---

## 6. 测试

| 层 | 内容 |
| --- | --- |
| Rust 单测 | `ums_login_signature` / `cdm_login_signature` **直接用本文档的示例数据做断言**——两个算法都已验证能复现示例值，是最可靠的回归锚点 |
| Rust 单测 | `rsa_pkcs1v15_encrypt_base64`：用示例 publicKey 加密后断言 base64 解码长度 == 256；两次加密结果不同（验证随机填充） |
| Rust 单测 | `detect_local_ip_for`：`198.18.x` / `127.x` / `169.254.x` 被过滤 |
| Rust 集成测试 | `wiremock`（已在 dev-dependencies）打三条流程的桩：正常、登录失败、改密失败、CDM 空响应+非 2xx 状态码 |
| Rust 集成测试 | **一种流程失败不影响另外两种**——这是最重要的一条 |
| 前端 | `UmsInitialPasswordPage.test.mjs`：沿用源码断言风格，覆盖表格类常量 + 三列表头 |
| 前端 | `sidebarNavigation.test.mjs` / `configDomains.test.mjs` 更新 |
| 命令 | `pnpm check`、`pnpm lint`、`cargo test`、`git diff --check` |

---

## 7. 待确认事项

协议缺口已补齐（UMS 第 4 步 = `PUT /sw/user/update/passwd`，响应 `{errCode, errMsg}`；挑战握手请求为真空 body）。**当前无阻塞项**，以下为实机验证时需留意的细节。

### 7.1 需实机验证的次要问题

| # | 问题 | 建议默认处理 |
| --- | --- | --- |
| 1 | RSA 加密的明文是密码原文还是 `MD5(密码)`？示例密文无私钥无法反推 | 按原文实现（字段名 `NewEncPassword` 及 2048 位可容纳 245 字节都支持这个判断），实机验证 |
| 2 | `LoginExtInfo.IpAddress` 服务端是否强校验？填错/填空会不会拒登？ | 探测失败时填 `""` 并继续 |
| 3 | 字典开关的 `createTime: 1716258652000` 是固定值还是要先 GET 原记录？ | 先按示例固定值发送；若服务端报错，改为先查询再回填 |
| 4 | 字典开关 `updateTime` 用当前时间戳（毫秒）？ | 用 `chrono::Utc::now().timestamp_millis()` |
| 5 | CDM 改密/登出响应体为空，成功判据是否就是 HTTP 2xx？ | 按 2xx 判成功，非 2xx 时把状态码和响应文本一起写进 message |
| 6 | UMS 的 80 端口、CDM 的 25011 端口是否所有现场都一致？ | 先写成常量；若现场有差异，后续加配置项 |

### 7.2 真空 body 的实现注意

reqwest 中「真空 body」不能用 `.json(&json!({}))`（会发出 `{}`），要用 `.body("")` 或干脆不设 body。同时**不要**带 `Content-Type: application/json`——部分 civetweb/网关实现会因为声明了 JSON 类型但 body 为空而返回 400。UMS 步骤 ① 和 CDM 步骤 ① 都按此处理。

### 7.3 建议的验证路径

`rsa_pkcs1v15_encrypt_base64` 和两个签名函数写完后，先加一个临时的 `#[test] #[ignore]` 打真实环境（192.115.2.38 / 192.115.1.17），确认 §7.1 第 1/2 项，再接入 UI。避免把 UI 全做完才发现协议细节对不上。

---

## 8. 实施顺序

1. Cargo 依赖 + `ums_init_password.rs` 模块骨架 + 三个加密工具函数 + 单测（用示例数据锚定）
2. 搬迁框架流程到新模块，保持行为不变，跑通现有链路
3. 实现 CDM 流程（最简单，无 RSA）+ wiremock 测试
4. 实现 UMS 流程（含本机 IP 探测、RSA、字典开关）+ wiremock 测试
5. 新命令 + 结果结构 + 注册，删除旧命令
6. 前端类型 + 页面改造 + i18n 双语 + `save_kv` 键迁移
7. 路由 / 侧边栏 / ToolsHub / 图标 / README 改名
8. 测试文件更新，跑 `pnpm check`、`pnpm lint`、`cargo test`
9. 实机验证 §7.1 各项，按结果回填实现

第 1–5 步是纯后端且可独立验证，第 6–7 步是纯改名与 UI，两段可以分开提交，减小 review 面。
