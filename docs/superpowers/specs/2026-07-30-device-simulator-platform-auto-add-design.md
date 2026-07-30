# 虚拟设备模拟 — 平台凭据配置与启动后自动添加设备 · 设计文档

**日期**: 2026-07-30
**范围**: 在「视频设备模拟」的服务器配置区新增一组平台登录凭据（默认 `loadmin` / `admin_123`，可改、可明文查看），并新增「开启虚拟设备后直接添加到平台」勾选项。勾选后在会话进入 `Running` 之后调用 UMS openAPI 批量添加编码设备。
**实现分支**: `main`
**相关入口**: [VideoDeviceSimulatorPage.vue:869-938](src/pages/VideoDeviceSimulatorPage.vue#L869-L938)、[device_simulator_commands.rs:487-673](src-tauri/src/device_simulator_commands.rs#L487-L673)、[ums_init_password.rs:492-615](src-tauri/src/ums_init_password.rs#L492-L615)

---

## 0. 概念先厘清：两组「用户名密码」不是一回事

需求描述里的「服务器用户名密码 `loadmin`/`admin_123`」与示例请求体里的 `userName: "admin"` / `password: "<密文>"` 是**两组完全不同的凭据**，实现时混用会直接导致 401 或添加后设备离线。

| | 平台登录凭据 | 设备接入凭据 |
| --- | --- | --- |
| 取值 | `loadmin` / `admin_123` | `admin` / 将明文 `Admin_1234` 用 openAPI RSA 公钥加密后的 Base64 密文 |
| 用途 | 登录 UMS 换 `AccessToken`，作为 `Authorization` 头 | 写进 `deviceList[]`，供平台**反向登录被添加的设备** |
| 出现位置 | 请求头 | 请求体 |
| 本次要不要暴露给用户 | **要**（本需求的新增配置项） | **不要**（见 §2.4） |
| 现有代码 | [ums_init_password.rs:26](src-tauri/src/ums_init_password.rs#L26) `UMS_USER = "loadmin"` | 无 |

设备接入凭据之所以可以写死：模拟出来的虚拟设备**没有任何协议层凭据**——见 [api.rs:68-74](src-tauri/src/device_simulator/api.rs#L68-L74) 的 `PlatformAccessMode` 文档注释（"ordinary listeners with no protocol-level credentials"），准入靠 IP 白名单和防火墙，不靠账号。平台拿这对凭据去登录虚拟设备时，虚拟设备根本不校验。因此本功能固定使用实测通过的明文 `Admin_1234`，但传给平台前仍必须按 §2.3.2 使用平台刚返回的 RSA 公钥加密；设备接入凭据不暴露为 UI 配置项。

---

## 1. 已确认的产品决策与待定项

### 1.1 已确认

| 决策点 | 结论 |
| --- | --- |
| 凭据默认值 | `loadmin` / `admin_123`，与 [ums_init_password.rs:26](src-tauri/src/ums_init_password.rs#L26)、[messages.ts](src/locales/messages.ts) 中 UMS 出厂默认一致 |
| 凭据可见性 | **明文可查看**（眼睛图标切换 `type="password"` / `type="text"`），可编辑 |
| 自动添加 | 勾选项，默认**开启**；用户勾选或取消后均随模拟器设置持久化，下次打开保持上次选择 |
| 触发时机 | 虚拟设备**启动成功之后**（`SessionState::Running`） |
| 密码公钥接口 | `POST http://{host}:{port}/openAPI/oauth/v1/rsa/publicKey/get`，使用本次 UMS 登录得到的 `AccessToken` 鉴权，请求体为空 |
| 密码加密 | RSA-1024 + PKCS#1 v1.5，结果使用 Base64 编码；Node.js `crypto.publicEncrypt` + `RSA_PKCS1_PADDING` 已验证成功 |
| 添加接口 | `POST http://{host}:{port}/openAPI/deviceManange/v1/encodeDevice/add`（注意官方路径把 `Manage` 拼成了 `Manange`，**必须照抄**） |
| 多设备添加 | 同一台 UMS 下本次模拟出的所有设备放入同一个 `deviceList`，只调用一次添加接口 |

### 1.2 需要拍板的三项（不阻塞后端开发，影响 UI 与默认值）

| # | 问题 | 本文档采用的默认方案 |
| --- | --- | --- |
| 1 | 服务器列表可以配多台（[VideoDeviceSimulatorPage.vue:882](src/pages/VideoDeviceSimulatorPage.vue#L882) 是 `v-for`），凭据是**一份共享**还是**每台一份**？ | **一份共享**，挂在平台配置层。理由：需求原文是单数「一个可选项」；现场同一批 UMS 的 loadmin 密码通常一致 |
| 2 | 配了多台服务器时，添加到**哪几台**？ | **全部已配置服务器都添加**，逐台出结果。理由：`servers` 的既有语义就是「这些平台都该看到这批设备」（`configured_servers_only` 的白名单就是从这里推出来的，见 [VideoDeviceSimulatorPage.vue:410-415](src/pages/VideoDeviceSimulatorPage.vue#L410-L415)） |
| 3 | `orgId` 固定 `"2"`？ | **固定常量 `"2"`**，取自示例。不做组织树选择器（需求没提，且要多调一个组织列表接口）。若现场根组织不是 `2`，再提到高级区 |

---

## 2. 协议

### 2.1 鉴权：复用 UMS 登录的前两步

[ums_init_password.rs:492-615](src-tauri/src/ums_init_password.rs#L492-L615) 的 UMS 流程已经把这条链路跑通过（2026-07-29 在 192.115.1.17 实机验证，见 [2026-07-28-ums-initial-password-design.md §7.1.1](docs/superpowers/specs/2026-07-28-ums-initial-password-design.md)）。本功能先复用它的 ① ② 获取 `AccessToken`，但不调用该流程后面的改密 / 字典开关接口。添加设备仍需另行调用 §2.3.1 的 openAPI RSA 公钥接口；它与改密使用的 `/sw/servers/public/key` 不是同一个端点：

```
① POST http://{host}:{port}/sw/login            body: 真空（不是 "{}"，且不带 Content-Type）
   ← { "AccessCode": "...", "Encryption": "MD5" }

② POST http://{host}:{port}/sw/login
   body: { UserName, AccessCode, LoginSignature, isNewVersion: true,
           ip: {host}, languageType: "zh_cn",
           LoginExtInfo: { IpAddress: <本机可达 IP> }, ClientIp: "" }
   ← { "AccessToken": "..." }

LoginSignature = MD5( Base64(UserName) + AccessCode + MD5(password) )
```

`AccessToken` 原样放进后续 openAPI 请求的 `authorization` 头。HTTP 头名称大小写不敏感；本文按实测请求统一写成小写 `authorization`，不要把 token 记录到日志。

**必须复用而不是重写的两个函数**：

- [ums_init_password.rs:123](src-tauri/src/ums_init_password.rs#L123) `ums_login_signature`
- [ums_init_password.rs:181](src-tauri/src/ums_init_password.rs#L181) `detect_local_ip_for` —— 内含 `198.18.0.0/15` fake-IP 代理 TUN 的过滤，开发机上不过滤会把 TUN 地址填进登录体

### 2.2 ⚠ 登录失败会累计锁定次数

[ums_init_password.rs:579-614](src-tauri/src/ums_init_password.rs#L579-L614) 已经证实 UMS 登录失败会返回 `ResidueDegree`（剩余尝试次数）和 `RemainMinutes`（锁定剩余分钟），多次失败会**锁定 loadmin 账号**。

由此产生两条硬约束：

1. **登录失败绝不重试。** 一次失败即终止该服务器的添加流程，把 `ResidueDegree` / `RemainMinutes` 原样带进错误信息——这是使用者判断「还能不能再试」的唯一依据。
2. **不做「测试连接」按钮。** 每次点击都是一次真实登录尝试，等于给用户一个消耗锁定配额的按钮。凭据只在真正执行添加时被使用一次，符合 memory 中「校验跟着主操作走，不做单独的检查按钮」的约定。

### 2.3 添加编码设备的完整 openAPI 调用链

本节的 `{host}:{port}` 是虚拟设备模拟界面中配置的 UMS IP 与端口；以下实测示例使用 `192.115.1.17:80`。进入本节前，先按 §2.1 获取本次调用使用的 `AccessToken`。

#### 2.3.1 获取 RSA 公钥

```http
POST http://192.115.1.17/openAPI/oauth/v1/rsa/publicKey/get
authorization: 02630561954418655523
```

响应：

```json
{
  "code": 0,
  "message": "Succeed.",
  "data": {
    "publicKey": "MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDuSsGxESaDuynmBqlGj48F/DfGe7k0Pjnq4aaFhpAOzTtJCdUuTq7QWxrfOhsmREOX6GZJ7c6VVpDm/pOqgH+YFU3oBftKJW40VnQmItvlQduGHUYnXiynHHp17ZS8X/wYidmBgzqOnskrIOUMxc6cRp1spSOx3If7RDjGGIbgDQIDAQAB"
  }
}
```

成功时要求 HTTP 为 2xx、`data.publicKey` 非空；若响应包含 `code`，还要求 `code == 0`。部分现场版本的成功响应不返回 `code`，不能仅因该字段缺失拒绝有效公钥。该值是没有 PEM 头尾的 SubjectPublicKeyInfo Base64 内容，对应 RSA-1024 公钥。它与 UMS 改密使用的 `GET /sw/servers/public/key`（RSA-2048）不是同一个接口，也不能混用。

#### 2.3.2 用公钥加密设备密码

- 算法：RSA-1024。
- 填充：PKCS#1 v1.5。
- 输入：设备接入明文密码，当前固定为 `Admin_1234`。
- 输出：Base64 编码的密文字符串。

伪代码：

```text
pem = "-----BEGIN PUBLIC KEY-----\n" + data.publicKey + "\n-----END PUBLIC KEY-----"
encryptedPassword = RSA_ENCRYPT(
  pem,
  "Admin_1234",
  padding=PKCS1_v1_5
).toBase64()
```

PKCS#1 v1.5 含随机填充，因此同一公钥、同一明文每次得到的密文都可能不同；不能用密文字符串相等作为测试断言。服务端用私钥解密后都应得到相同明文。项目验证脚本已使用 Node.js `crypto.publicEncrypt` + `RSA_PKCS1_PADDING` 跑通该流程。

#### 2.3.3 一次性添加本次模拟出的全部设备

```http
POST http://192.115.1.17/openAPI/deviceManange/v1/encodeDevice/add
authorization: 02630561954418655523
Content-Type: application/json
```

单设备请求示例：

```json
{
  "deviceList": [
    {
      "deviceIndexCode": 1,
      "deviceName": "192.115.1.69",
      "orgId": "2",
      "deviceAddr": "192.115.1.69",
      "devicePort": 80,
      "userName": "admin",
      "password": "<第二步产出的加密密文>",
      "msPolicy": 1,
      "msCode": "",
      "playbackMediaPolicy": 1,
      "playbackMSCode": "",
      "deviceType": 1,
      "mediaProtocol": 2,
      "accessType": 1,
      "accessProtocol": 1,
      "accessNetwork": 1,
      "channelNamePolicy": 1,
      "enableStreamTls": 2
    }
  ]
}
```

响应：

```json
{
  "code": 0,
  "message": "Succeed.",
  "data": {
    "successList": [
      {
        "deviceIndexCode": 1,
        "deviceId": "630568641095532835"
      }
    ]
  }
}
```

完整顺序为：

```text
POST /openAPI/oauth/v1/rsa/publicKey/get
  → data.publicKey
  → RSA-1024 / PKCS#1 v1.5 加密 Admin_1234
  → Base64 encryptedPassword
  → POST /openAPI/deviceManange/v1/encodeDevice/add
  → data.successList[].deviceId
```

流程图：

```text
┌─────────────────────────────────────────────┐
│  1. POST /openAPI/oauth/v1/rsa/publicKey/get│
│     使用 authorization 获取 RSA 公钥        │
└──────────────────────┬──────────────────────┘
                       │ data.publicKey
                       ▼
┌─────────────────────────────────────────────┐
│  2. RSA-1024 / PKCS#1 v1.5                 │
│     公钥加密明文密码 Admin_1234              │
│     → Base64 密文字符串                      │
└──────────────────────┬──────────────────────┘
                       │ encryptedPassword
                       ▼
┌─────────────────────────────────────────────┐
│  3. POST /encodeDevice/add                  │
│     全部设备放入同一个 deviceList            │
│     password 填入加密密文 → 返回 deviceId    │
└─────────────────────────────────────────────┘
```

### 2.4 请求字段与成功判定

成功判据：`code == 0`。**但 `code == 0` 不代表每台都成功**——`successList` 只列出成功的项，请求里有而 `successList` 里没有的 `deviceIndexCode` 就是失败项。必须按 `deviceIndexCode` 做差集，否则「10 台报成功、实际只进了 3 台」会被静默吞掉。响应中是否还有 `failList` / `errorList` 待实机确认（§7 第 2 项）。

单项字段（`deviceIndexCode` 为请求内 1-based 序号，仅用于与 `successList` 关联）：

| 字段 | 取值来源 | 说明 |
| --- | --- | --- |
| `deviceIndexCode` | 请求内序号，从 1 开始 | 关联键，非设备编号 |
| `deviceName` | 设备 IP 字符串 | 与 `deviceAddr` 相同 |
| `orgId` | 常量 `"2"` | 字符串，不是数字 |
| `deviceAddr` | `DeviceIdentityPreviewDto.ip` | [api.rs:254](src-tauri/src/device_simulator/api.rs#L254) |
| `devicePort` | `SimulatorStartRequest.device_http_port` | 使用虚拟设备实际监听的 HTTP 端口；实测示例为 `80` |
| `userName` | 常量 `"admin"` | 设备接入凭据，见 §0 |
| `password` | §2.3.2 产出的 Base64 密文 | 每次请求前用本次取得的 openAPI 公钥加密 `Admin_1234` |
| `msPolicy` | `1` | 以下均为实测示例中的固定值，含义为推测，**照抄不改** |
| `msCode` | `""` | |
| `playbackMediaPolicy` | `1` | |
| `playbackMSCode` | `""` | |
| `deviceType` | `1` | 编码设备类型；以实测成功请求为准 |
| `mediaProtocol` | `2` | 推测：RTSP |
| `accessType` | `1` | |
| `accessProtocol` | `1` | |
| `accessNetwork` | `1` | |
| `channelNamePolicy` | `1` | |
| `enableStreamTls` | `2` | 推测：2 = 关闭 |

这些魔法数字全部写成一个 `const` 块并在注释里标明「取自 2026-07-30 实测成功请求，含义未经确认」，不要散落在构造函数里。

### 2.5 多设备批量语义

若本次模拟出多台设备，必须把所有设备一次性放进同一个 `deviceList`，并对每台 UMS **只调用一次**添加设备接口；不要按设备逐次调用，也不要预设 100 台分片。`deviceIndexCode` 在这个完整请求内从 1 连续递增，响应仍按该字段与设备关联。

---

## 3. Rust 侧改造

### 3.1 配置字段：放哪里，为什么

新增三个字段到 [config.rs:21-63](src-tauri/src/config.rs#L21-L63) 的 `DeviceSimulatorSettings`：

```rust
pub struct DeviceSimulatorSettings {
    // …既有字段…
    /// UMS 平台登录账号。默认 "loadmin"。
    pub platform_username: String,
    /// UMS 平台登录密码。明文持久化，与 DeployServer.password 一致（见下方说明）。
    pub platform_password: String,
    /// 启动成功后自动把虚拟设备添加到已配置的平台服务器。
    pub platform_auto_add_devices: bool,
}
```

默认值 `"loadmin"` / `"admin_123"` / `true`。首次使用及旧配置缺少该字段时默认勾选；保存过的选择（包括明确取消勾选）按原值恢复。

**该结构体已有 `#[serde(default)]`（[config.rs:20](src-tauri/src/config.rs#L20)），旧配置文件缺这三个键时按 Default 补齐，其中 `platform_auto_add_devices` 默认为 `true`，不会触发解析失败。用户配置中已显式保存的 `true` 或 `false` 均原样保留。** 这一点很关键——memory 记录过「配置解析失败会静默重置整个 AppConfig」，所以任何新增持久化字段都必须确认它落在 `serde(default)` 覆盖范围内，且**不能**给它加 `deny_unknown_fields`。

需要同步修掉的一处注释矛盾：[deviceSimulator.ts:70](src/lib/deviceSimulator.ts#L70) 现在写着 "Runtime credentials never belong here"。本次要么改注释、要么换存储位置。**建议改注释**，并写清边界：

> 平台登录凭据按 [DeployServer.password](src-tauri/src/config.rs#L173) 的既有口径明文持久化并明文回传前端；`portal_login` 那套 DPAPI 加密 + 回传脱敏（[config.rs:393-407](src-tauri/src/config.rs#L393-L407)）不适用于此，因为需求明确要求明文可查看，脱敏后就没法查看了。

这是一个**有意识的降级**，应当在注释里留痕，而不是悄悄放进去。

### 3.2 凭据**不进** `SimulatorStartRequest`

不要把凭据加到 [api.rs:87](src-tauri/src/device_simulator/api.rs#L87) 的 `TargetPlatformConfig`。三条理由：

1. `TargetPlatformConfig` 带 `deny_unknown_fields`，加字段要连带处理旧会话日志的兼容（虽然 `#[serde(default)]` 能解决，但没必要引入这个风险面）。
2. 该结构会跨 Worker IPC 边界传给提权子进程（[device_simulator_commands.rs:572-583](src-tauri/src/device_simulator_commands.rs#L572-L583) 的 `InitializeSessionPayload`）。密码没有任何理由进入提权进程。
3. [errors.rs:14-17](src-tauri/src/device_simulator/errors.rs#L14-L17) 明确写了「Passwords, access tokens … must never be stored here」，跨边界结构对密钥的态度是一贯保守的，不要开这个口子。

**添加设备是一次纯 HTTP 调用，不需要提权，就在主进程做。** 凭据从 `AppState.config` 现读现用。

会话日志本身不受影响：它只存 `DeviceRequestSummary { profile_ids, total_devices }`（[session_journal.rs:34-38](src-tauri/src/device_simulator/session_journal.rs#L34-L38)），从不存完整请求。

### 3.3 新模块 `src-tauri/src/device_simulator/platform_registration.rs`

放在 `device_simulator/` 下而不是复用 `ums_init_password.rs`，因为它属于模拟器领域；但**鉴权部分反向依赖** `ums_init_password.rs`，需要把三个私有项提升为 `pub(crate)`：

```rust
// ums_init_password.rs 中改为 pub(crate)
pub(crate) fn ums_login_signature(user: &str, access_code: &str, password: &str) -> String;
pub(crate) fn detect_local_ip_for(target: Ipv4Addr) -> Option<String>;

// 新抽出的公共函数：把 UMS 流程 ① ② 抽成可复用的登录
/// 完成挑战握手 + 登录，返回 AccessToken。失败信息含 ResidueDegree / RemainMinutes。
pub(crate) async fn ums_acquire_access_token(
    client: &reqwest::Client,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<String, String>;
```

抽 `ums_acquire_access_token` 时要小心：现有 UMS 流程的登录段与 `FlowLogger`（[ums_init_password.rs:227-310](src-tauri/src/ums_init_password.rs#L227-L310)）耦合，日志前缀是 `[IP][流程]`。抽公共函数时**日志改成通过回调注入**（`&dyn Fn(&str, &str)` 或一个小 trait），不要让模拟器的日志被打成「UMS 初始密码修改」工具的日志——那会出现在错误的日志页里。

模块主体：

```rust
pub const PLATFORM_ADD_DEVICE_PATH: &str = "/openAPI/deviceManange/v1/encodeDevice/add";
pub const PLATFORM_RSA_PUBLIC_KEY_PATH: &str = "/openAPI/oauth/v1/rsa/publicKey/get";
const DEVICE_ORG_ID: &str = "2";
const DEVICE_ACCESS_USER: &str = "admin";
const DEVICE_ACCESS_PASSWORD: &str = "Admin_1234";
const DEVICE_TYPE: u8 = 1;

/// 一台待添加设备的最小描述。由前端从 preview 派生后传入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformDeviceEntry {
    pub address: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAddDeviceOutcome {
    pub address: String,
    pub added: bool,
    /// 平台分配的 deviceId，成功时存在。
    pub device_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformServerAddResult {
    pub server_id: String,
    pub host: String,
    pub port: u16,
    /// 该服务器上是否全部成功。
    pub success: bool,
    /// 失败阶段：login / public_key / add。登录或取公钥失败时 devices 为空。
    pub failed_at: Option<String>,
    pub message: Option<String>,
    pub devices: Vec<PlatformAddDeviceOutcome>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAddDevicesReport {
    pub servers: Vec<PlatformServerAddResult>,
    pub total_devices: u32,
    pub added_devices: u32,
}
```

模块在每台服务器上严格按「获取 `AccessToken` → 获取 openAPI RSA 公钥 → 加密设备密码 → 单次 POST 全部设备」执行。可复用现有 `rsa_pkcs1v15_encrypt_base64` 的 PKCS#1 v1.5 加密实现，但入参必须是 `PLATFORM_RSA_PUBLIC_KEY_PATH` 本次返回的 RSA-1024 公钥，不能复用 `/sw/servers/public/key` 的 RSA-2048 公钥。结果结构照 `UmsInitPasswordResult` 的分层思路（[ums_init_password.rs:82-89](src-tauri/src/ums_init_password.rs#L82-L89)）：外层一行一服务器、内嵌逐设备结果，让 UI 能按服务器折叠。

### 3.4 新 Tauri command

```rust
#[tauri::command]
pub async fn device_simulator_add_devices_to_platform(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
    devices: Vec<PlatformDeviceEntry>,
) -> Result<PlatformAddDevicesReport, SimulatorErrorBody>
```

- 服务器列表、账号、密码全部从 `app_state.config.lock()` 的 `device_simulator` 读取——启动流程里 [useDeviceSimulator.ts:581](src/composables/useDeviceSimulator.ts#L581) 已经在 `start()` 之前调过 `saveSettings`，配置一定是最新的。
- **`devices` 由前端传入**：前端此刻手上有 `preview.devices`（[api.rs:263-267](src-tauri/src/device_simulator/api.rs#L263-L267)），后端重新推导等于把 `preview_devices` 的整套派生逻辑再走一遍，且要额外持有会话状态。传入的只是 IP 和端口，不含任何机密，权衡下更划算。
- 服务器之间**串行**（每台都要单独登录，并发登录多台没有意义还容易撞上锁定计数）；每台服务器内部只获取一次公钥、只加密一次固定设备密码，并把本次所有设备放入一个 `deviceList` 后只调用一次添加接口。
- HTTP client 复用 [`crate::build_device_http_client_with_timeout`](src-tauri/src/ums_init_password.rs#L1092)，超时沿用 `framework_password_api_timeout_secs`（不新增配置项，它就是个 HTTP 超时）。
- 日志走 `DEVICE_SIMULATOR_EVENT_LOG`（[api.rs:22](src-tauri/src/device_simulator/api.rs#L22)），`component` 用 `"platform:register"`，与 `alarm:dispatch` 的既有风格一致（[useDeviceSimulator.ts:621](src/composables/useDeviceSimulator.ts#L621)）。每个 openAPI 请求都记录方法、URL、HTTP 状态、Content-Type、脱敏后的请求体和响应体，长正文最多保留 16384 字符并标注截断；运行日志界面必须支持 JSON 自动换行及日志导出。**密码、设备密码密文、AccessCode、LoginSignature、Authorization 和 AccessToken 永不入日志**——现有 UMS 流程把 token 打进了日志（[ums_init_password.rs:615](src-tauri/src/ums_init_password.rs#L615)），那是排障工具的取舍，模拟器日志页面对普通使用者，不要照搬。

注册到 `invoke_handler`，与其余 `device_simulator_*` 命令并列。

### 3.5 为什么不做进 `device_simulator_start`

`device_simulator_start`（[device_simulator_commands.rs:487-673](src-tauri/src/device_simulator_commands.rs#L487-L673)）的返回类型是 `SimulatorStatusSnapshot`，塞不进添加结果；要塞就得改 `SessionState` 或加事件，成本明显更高。而且语义上更重要的一点：

**添加失败绝不能回滚已经起好的会话。** 虚拟设备已经在跑、IP 别名已经加好、防火墙规则已经建好，此时因为平台登录失败就把整个会话拆掉，是把一个可重试的小失败升级成大破坏。做成独立命令后，失败就只是一张红卡片 + 一个「重试添加」按钮。

---

## 4. 前端改造

### 4.1 类型（[src/lib/deviceSimulator.ts](src/lib/deviceSimulator.ts)）

`DeviceSimulatorSettings`（L71-89）加三个字段，与 Rust 侧同名同序：

```ts
platform_username: string;
platform_password: string;
platform_auto_add_devices: boolean;
```

新增命令常量 `addDevicesToPlatform: 'device_simulator_add_devices_to_platform'`，`DeviceSimulatorApi` 加一条，`createDeviceSimulatorApi`（L471-493）加一行：

```ts
addDevicesToPlatform: (devices) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.addDevicesToPlatform, { devices }),
```

新增 `PlatformDeviceEntry` / `PlatformAddDeviceOutcome` / `PlatformServerAddResult` / `PlatformAddDevicesReport` 四个 interface。

**注意 `settingsFromRequest()`（[useDeviceSimulator.ts:227-245](src/composables/useDeviceSimulator.ts#L227-L245)）**：它以 `...settings.value` 开头再覆盖若干键。新增的三个字段**不**在 `request` 里（§3.2），所以会被展开自动带过去，无需改动——但如果把它们做成 `reactive` 的独立 ref，就必须在这里显式合并，否则保存时会被旧值覆盖。**建议直接编辑 `settings.value` 上的字段**（`asset_server_url_override` 就是这么做的，见 [VideoDeviceSimulatorPage.vue:933](src/pages/VideoDeviceSimulatorPage.vue#L933)），最省事且天然一致。

### 4.2 UI：服务器配置区

插在服务器列表与「准入模式」之间（[VideoDeviceSimulatorPage.vue:888-889](src/pages/VideoDeviceSimulatorPage.vue#L888-L889) 之间）：

```
┌─ 平台（既有卡片）───────────────────────────────────────────┐
│  平台 [UMS]           报警接收端口 [22815]                   │
│  ─ 服务器列表（既有）──────────────────────────────────────  │
│  服务器地址 [192.115.1.17 ]  端口 [80  ]  🗑                 │
│  ＋ 添加服务器                                               │
│  ─ 平台凭据（新增）───────────────────────────────────────   │
│  用户名 [loadmin      ]   密码 [admin_123    ] 👁            │
│  ⓘ 添加设备到平台时用于登录 UMS；登录失败会累计账号锁定次数   │
│  ─ 自动添加（新增）───────────────────────────────────────   │
│  [ ] 开启虚拟设备后直接添加到平台                             │
│      ⓘ 启动成功后自动把 N 台设备添加到上面 M 台服务器         │
│  ─ 准入模式（既有）───────────────────────────────────────   │
└──────────────────────────────────────────────────────────────┘
```

要点：

- 明文切换用 `lucide-vue-next` 的 `Eye` / `EyeOff`，照 [UmsInitialPasswordPage.vue](src/pages/UmsInitialPasswordPage.vue) 的现成写法。**不用 Emoji**（CLAUDE.md）。
- 勾选项用 `KeyRound`（凭据）+ `CloudUpload`（自动添加）作节标题图标。
- 整块在 `<fieldset :disabled="simulator.topologyLocked.value">` 内（L867），运行期自动禁用编辑，不需要额外处理。
- 提示文案里的 N/M 用 `preview.total_devices` 和 `servers.length` 实时插值，让「勾了会发生什么」当场可见。

### 4.3 校验：跟着主操作走

沿用 memory 中「检查自己会跑，不做检查按钮，在主操作上校验、在顶部报告」的约定，以及页面已有的 `platformAccessNeedsServer` 模式（[VideoDeviceSimulatorPage.vue:410-415](src/pages/VideoDeviceSimulatorPage.vue#L410-L415)）：

```ts
/** 勾了自动添加却没有可用服务器 / 凭据，启动前就得说清楚。 */
const platformAutoAddNeedsConfig = computed(() => {
  if (!simulator.settings.value.platform_auto_add_devices) return false;
  const hasServer = simulator.request.platform.servers
    .some((s) => s.host.trim() !== '' && s.port > 0);
  const hasCreds = simulator.settings.value.platform_username.trim() !== ''
    && simulator.settings.value.platform_password !== '';
  return !hasServer || !hasCreds;
});
```

命中时在勾选项下方出琥珀色提示（复用 L896 的 `AlertTriangle` 样式）并**禁用启动按钮**。

**判定「配置不全」时不要阻断，只在勾选了自动添加时才阻断**——没勾的人不该被平台凭据挡住启动。

### 4.4 启动后串接

改 [useDeviceSimulator.ts:575-586](src/composables/useDeviceSimulator.ts#L575-L586) 的 `start()`：

```ts
async function start() {
  const result = await run('start', async () => { /* …原样不变… */ });
  if (!result) return;
  applyStatus(result);
  if (!settings.value.platform_auto_add_devices) return;
  await addDevicesToPlatform();          // 独立 run()，独立 busy 键
}

async function addDevicesToPlatform() {
  const devices = (preview.value?.devices ?? [])
    .map((d) => ({ address: d.ip, port: request.device_http_port }));
  if (devices.length === 0) return;
  const report = await run('add-to-platform', () => deviceSimulatorApi.addDevicesToPlatform(devices));
  if (report) platformAddReport.value = report;
}
```

三个必须做对的地方：

1. **`applyStatus(result)` 要在添加之前调用。** 会话已经在跑，状态必须先落地，否则添加期间 UI 显示的还是「启动中」。
2. **添加用独立的 `run()` 键**（`'add-to-platform'`），不要复用 `'start'`。否则添加失败会被渲染成「启动失败」，而设备其实起来了——这是最容易造成误判的一种错报。
3. `addDevicesToPlatform` 同时**导出为可手动调用的方法**，供失败后的「重试添加」按钮使用。

新增 `platformAddReport = ref<PlatformAddDevicesReport | null>(null)`，`stop()` 里清空。

### 4.5 结果展示

在「运行时」标签页加一张卡片（仅 `platformAddReport` 非空时出现）：

```
┌─ 平台添加结果 ──────────────────────────────────────┐
│  ✓ 已添加 8/8 台                                     │
│  192.115.1.17:80        ✓ 8 台成功                   │
│  192.115.2.38:80        ✗ 登录失败：errCode=94464    │
│                            （剩余尝试次数 4）          │
│                            [重试添加]                 │
└──────────────────────────────────────────────────────┘
```

失败明细逐设备列出，消息列用 `break-all`。`ResidueDegree` / `RemainMinutes` 必须原样显示（§2.2）。

### 4.6 i18n

新增键，中英双语同步维护 [src/locales/messages.ts](src/locales/messages.ts)：

```
deviceSimulator.fields.platformUsername / platformPassword / platformPasswordToggle
deviceSimulator.fields.platformCredentialsHint      // 含账号锁定警告
deviceSimulator.fields.autoAddDevices
deviceSimulator.fields.autoAddDevicesHint           // 带 {devices} {servers} 插值
deviceSimulator.fields.autoAddNeedsConfig
deviceSimulator.platformAdd.title / summary         // {added}/{total}
deviceSimulator.platformAdd.serverSuccess           // {count}
deviceSimulator.platformAdd.retry
deviceSimulator.platformAdd.loginFailed / addFailed
deviceSimulator.errors.platformLoginFailed
deviceSimulator.errors.platformPublicKeyFailed
deviceSimulator.errors.platformAddFailed
deviceSimulator.errors.platformServerMissing
```

---

## 5. 错误码

新增，与既有 `device_simulator.*` 命名保持一致：

| code | 含义 | message_key |
| --- | --- | --- |
| `device_simulator.platform.server_missing` | 勾了自动添加但没有可用服务器 | `deviceSimulator.errors.platformServerMissing` |
| `device_simulator.platform.credentials_missing` | 账号或密码为空 | `deviceSimulator.errors.platformServerMissing` |
| `device_simulator.platform.login_failed` | UMS 登录失败（含锁定） | `deviceSimulator.errors.platformLoginFailed` |
| `device_simulator.platform.public_key_failed` | openAPI 公钥接口 HTTP 非 2xx、`code != 0`、公钥为空或无法解析 | `deviceSimulator.errors.platformPublicKeyFailed` |
| `device_simulator.platform.add_failed` | 添加接口 `code != 0` 或 HTTP 非 2xx | `deviceSimulator.errors.platformAddFailed` |
| `device_simulator.platform.add_partial` | `code == 0` 但部分设备不在 `successList` | `deviceSimulator.errors.platformAddFailed` |

`login_failed` 的 `details` 里带 `errCode` + `errMsg` + 锁定信息；**不带密码、不带 token**（[errors.rs:14-17](src-tauri/src/device_simulator/errors.rs#L14-L17)）。

---

## 6. 测试

| 层 | 内容 |
| --- | --- |
| Rust 单测 | 公钥响应解析：校验 `data.publicKey` 非空；兼容成功响应缺少 `code`，存在 `code` 时要求为 `0`，并能包装为 PEM 公钥 |
| Rust 单测 | RSA 加密：使用测试私钥解密后得到 `Admin_1234`；密文 Base64 解码后为 128 字节，不断言两次随机密文相等 |
| Rust 单测 | 请求体构造：给定 2 台设备 + 端口 80，断言 JSON 与 §2.3.3 示例逐字段一致（含 `orgId` 是字符串 `"2"`、`deviceType == 1`、路径含 `deviceManange` 拼写） |
| Rust 单测 | `deviceIndexCode` 在完整 `deviceList` 中从 1 连续递增 |
| Rust 单测 | `successList` 差集：请求 3 台、返回 2 台，断言第 3 台被标为失败且整体判 `add_partial` |
| Rust 单测 | 批量：多台设备只构造一个添加请求，请求体的单个 `deviceList` 包含全部设备 |
| Rust 集成（wiremock，已在 dev-dependencies） | 登录成功→获取公钥→加密→添加成功；登录失败（带 `ResidueDegree`）→**不重试**、不取公钥、不发添加请求；公钥接口失败/空公钥；添加 HTTP 500；`code != 0`；部分成功 |
| Rust 集成 | 多服务器：第一台登录失败**不影响**第二台，且结果各自独立 |
| Rust 单测 | 旧配置文件（无三个新键）反序列化后取到 `loadmin` / `admin_123` / `true`，且**其余字段未被重置**（防 memory 记录的整体重置回归）；自动添加开关的 `true` / `false` 序列化往返后保持不变 |
| 前端 | [VideoDeviceSimulatorPage.test.mjs](src/pages/VideoDeviceSimulatorPage.test.mjs)：凭据输入 + 明文切换 + 勾选项存在；`platformAutoAddNeedsConfig` 的四种组合 |
| 前端 | [deviceSimulator.contract.test.ts](src/lib/deviceSimulator.contract.test.ts)：新命令名与参数形状；三个新 settings 字段 |
| 前端 | `start()` 未勾选时**不调用** `addDevicesToPlatform`；添加失败时 `busyAction` 不是 `'start'` |
| 命令 | `pnpm check`、`pnpm lint`、`cargo test`、`git diff --check` |

---

## 7. 待确认事项

| # | 问题 | 处理 |
| --- | --- | --- |
| 1 | openAPI 是否与 UMS 登录共用 80 端口 | 实测 URL 无端口即 80，与 `UMS_PORT` 一致（[ums_init_password.rs:22](src-tauri/src/ums_init_password.rs#L22)）。实现仍使用服务器行配置的 `port`，需在非 80 端口现场确认 |
| 2 | 响应里有没有 `failList` / `errorList`，失败原因怎么给 | 先按 `successList` 差集判定，只能给出「未出现在成功列表」。抓到失败样例后回填 |
| 3 | `orgId "2"` 是否所有现场都是根组织 | 先固定常量。现场有差异则提到高级区（§1.2 第 3 项） |
| 4 | `AccessToken` 需不需要登出释放 | UMS 流程现在就没登出（[ums_init_password.rs:719-778](src-tauri/src/ums_init_password.rs#L719-L778) 之后直接结束），沿用；若现场出现会话数耗尽再补 |
| 5 | 重复添加同一 IP 的行为（重启会话后再添加） | 待实机确认：报错、覆盖、还是产生重复设备。**这是最可能被用户反复触发的路径**（改配置→重启→再添加），务必验证并在 UI 提示 |

---

## 8. 实施顺序

0. 以 §2.3 已跑通的 Node.js `crypto.publicEncrypt` + `RSA_PKCS1_PADDING` 调用链作为协议锚点，保留一份脱敏后的请求/响应样例用于回归。
1. 配置字段（三个）+ 默认值 + 旧配置兼容单测。
2. 从 `ums_init_password.rs` 抽出不记录敏感数据的 `ums_acquire_access_token`；模拟器调用方只记录脱敏后的登录阶段摘要，保持原 UMS 流程行为与现有测试全绿。
3. `platform_registration.rs`：公钥获取 + RSA-1024 PKCS#1 v1.5 加密 + 单次批量请求体构造 + `successList` 差集 + 单测（用 §2.3 示例做锚点）。
4. wiremock 集成测试，重点是「登录失败不重试/不继续取公钥」「多设备只发一次添加请求」和「多服务器互不影响」。
5. Tauri command + 注册 + 日志事件。
6. 前端类型 + api 封装 + contract 测试。
7. UI：凭据卡 + 勾选项 + 校验 + 结果卡 + 重试按钮。
8. i18n 双语。
9. `pnpm check`、`pnpm lint`、`cargo test`、`git diff --check`。
10. 实机跑通并回填 §7 各项。

第 1–5 步纯后端、可独立验证；第 6–8 步纯前端。两段可分开提交。

> **注意当前工作区有大量未提交改动**（`src-tauri/src/device_simulator/` 下 20 余个文件、`src/` 下 8 个文件）。开工前先确认这些改动的归属，不要把它们卷进本功能的提交。
