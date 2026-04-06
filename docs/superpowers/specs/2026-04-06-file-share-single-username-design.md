# 文件共享单用户名配置设计

- 日期：2026-04-06
- 状态：设计已确认，待用户 review 文档后进入实现计划
- 适用范围：桌面端文件共享设置页、share-web 登录流程、Rust 文件共享持久化与鉴权
- 设计结论：彻底移除“显示名称 + 登录 ID”双字段，统一为单一“用户名”模型；不做旧账号配置迁移

## 1. 背景

当前文件共享账号配置同时暴露了“显示名称”和“登录 ID”两个字段：

1. 设置页里用户需要分别理解“给人看的名字”和“真正用于登录的名字”。
2. share-web 登录时实际使用的是 `id`，导致“改了显示名称但登录名没变”的认知落差。
3. 前端、后端和持久化层都同时维护 `id` 与 `name`，模型复杂度高，但并没有带来明确业务价值。

本次需求的目标非常直接：文件共享工具只保留一个用户名称字段，用户在设置页里填写什么，网页登录时就输入什么，系统内部也只认这一套名字。

## 2. 目标

### 2.1 产品目标

1. 文件共享设置页只显示一个“用户名”字段，不再显示“显示名称”和“登录 ID”。
2. share-web 登录弹窗只要求输入“用户名”和“密码”。
3. 顶部当前身份显示使用同一个用户名字段，不再区分显示名和登录名。

### 2.2 技术目标

1. Rust 持久化模型、运行时模型、HTTP 接口和 TypeScript 类型全部切换到单用户名结构。
2. 文件共享鉴权流程不再依赖隐藏的 `account_id`。
3. 访客身份保留，但通过显式的“访客账号”结构和 `is_guest` 标记区分，而不是通过固定字符串 ID。

## 3. 非目标

1. 不做旧账号配置迁移。
2. 不兼容旧的“双字段账号”持久化格式。
3. 不修改文件共享权限模型本身，只修改账号身份字段与对应接口。
4. 不改变共享目录、端口、缩略图、图片预览等与本需求无关的功能语义。

## 4. 方案结论

采用“彻底单用户名化”的方案：

1. 自定义账号只保留 `username`，不再保留 `id` 或 `name`。
2. 访客账号从普通账号列表中拆出，作为独立的 `guest_account` 保存。
3. 登录请求、会话响应和会话存储全部以 `username` 为核心字段。
4. 为了明确切断旧结构，持久化文件升级为新的独立文件名 `file_share_v3.json`；旧文件 `file_share_v2.json` 不参与读取，也不做转换。

这样可以避免“界面只删字段、内部却继续保留旧登录 ID”的半重构状态。

## 5. 数据模型

### 5.1 Rust 持久化模型

账号结构统一为：

```rust
pub struct PersistedFileShareUser {
    pub username: String,
    pub enabled: bool,
    pub preset: PermissionPreset,
    pub permissions: FileSharePermissionSet,
    pub password_hash: Option<String>,
}
```

文件共享配置结构调整为：

```rust
pub struct PersistedFileShareConfig {
    pub version: u32,
    pub port: u16,
    pub roots: Vec<FileShareRoot>,
    pub guest_access_enabled: bool,
    pub guest_account: PersistedFileShareUser,
    pub accounts: Vec<PersistedFileShareUser>,
    pub session_ttl_minutes: u32,
    pub ip_filter_mode: IpFilterMode,
    pub ip_rules: Vec<String>,
    pub image_preview_enabled: bool,
    pub thumbnail_enabled: bool,
    pub delete_mode: DeleteMode,
    pub remember_settings: bool,
    pub auto_start_on_page_open: bool,
    pub auto_start_with_windows: bool,
}
```

关键点：

1. `guest_account` 独立存在，不再混入 `accounts`。
2. `accounts` 中每个元素只保留 `username`。
3. 配置文件名改为 `file_share_v3.json`，表示本次结构为破坏式切换。

### 5.2 前后端视图模型

设置页接口同样使用单用户名结构：

```ts
interface FileShareUserView {
  username: string;
  enabled: boolean;
  preset: FileSharePermissionPreset;
  permissions: FileSharePermissionSet;
  password_set: boolean;
}

interface FileShareSettingsView {
  guest_access_enabled: boolean;
  guest_account: FileShareUserView;
  accounts: FileShareUserView[];
  // 其余字段保持不变
}
```

保存请求结构：

```ts
interface FileShareUserSaveRequest {
  username: string;
  enabled: boolean;
  preset: FileSharePermissionPreset;
  permissions: FileSharePermissionSet;
  new_password?: string | null;
  clear_password: boolean;
}
```

## 6. 设置页设计

### 6.1 访客区域

访客区域保留：

1. 用户名
2. 密码
3. 权限预设/自定义权限

移除：

1. 显示名称
2. 登录 ID

访客区域的数据来源改为 `guest_account`，不再通过 `accounts.find(...)` 查找固定 ID。

### 6.2 自定义账号区域

自定义账号卡片每项只保留一个“用户名”输入框，不再区分两列。

新增账号时：

1. 仍创建一个可编辑的新账号草稿。
2. 默认填入本地化的默认用户名文案。
3. 不再自动生成 `account`, `account-2` 这类内部登录 ID。

### 6.3 前端草稿结构

因为用户名现在是可编辑主键，Vue 列表不能再用 `username` 作为稳定 key。前端草稿层需要增加仅用于本地渲染的临时字段，例如：

```ts
type DraftUser = FileShareUserView & {
  draft_key: string;
  new_password: string;
  clear_password: boolean;
};
```

该字段仅用于：

1. `v-for` 稳定 key
2. 删除、更新本地草稿项

该字段不参与保存请求。

## 7. share-web 接口与文案

### 7.1 登录请求

share-web 登录请求从：

```json
{ "account_id": "...", "password": "..." }
```

改为：

```json
{ "username": "...", "password": "..." }
```

### 7.2 会话响应

会话响应从：

```json
{
  "account_id": "...",
  "account_name": "...",
  "is_guest": false
}
```

改为：

```json
{
  "username": "...",
  "is_guest": false
}
```

`permissions` 与 `features` 保持不变。

### 7.3 文案调整

share-web 文案同步改为：

1. `Account ID` -> `Username`
2. `账号 ID` -> `用户名`
3. 登录说明统一描述为“输入用户名和密码”

桌面端设置页文案同步改为：

1. 移除 `displayName`
2. 移除 `loginId`
3. 新增或复用 `username`

## 8. 鉴权与会话设计

### 8.1 用户名匹配规则

用户名统一做 `trim` 后参与匹配和校验。

登录时：

1. 先匹配 `guest_account.username`
2. 再匹配 `accounts[].username`
3. 匹配成功后校验密码

因为访客账号也可能设置密码，所以访客用户名同样是一个可登录的显式用户名。

### 8.2 访客自动访问规则

访客自动访问延续现有语义：

1. `guest_access_enabled = true`
2. `guest_account.enabled = true`
3. `guest_account.password_hash = None`

同时满足时，未登录请求自动以访客身份访问。

否则返回未授权，share-web 弹出用户名/密码登录框。

### 8.3 会话存储

会话记录不再保存隐藏账号 ID，而是保存显式身份信息，例如：

```rust
enum SessionSubject {
    Guest,
    Account { username: String },
}
```

这样可以：

1. 明确区分访客与普通账号
2. 不引入第二套隐藏命名字段
3. 让 `/api/session` 中的 `is_guest` 来源清晰

## 9. 校验规则

### 9.1 设置页保存校验

1. 访客用户名不能为空。
2. 自定义账号用户名不能为空。
3. 自定义账号用户名必须唯一。
4. 访客用户名不能与任何自定义账号用户名重复。

### 9.2 默认值

1. 默认访客账号始终存在。
2. 默认访客用户名使用现有本地化默认值。
3. 当没有自定义账号时，`accounts` 为空数组即可，不再需要插入伪访客项。

## 10. 与旧配置的关系

本次改动明确不做迁移。

实施规则如下：

1. 新结构使用 `file_share_v3.json`。
2. 旧结构 `file_share_v2.json` 不读取、不转换、不合并。
3. 升级到新版本后，文件共享账号配置视为重新开始配置。

这是一个有意为之的破坏式切换，目标是换取模型彻底简化。

## 11. 受影响实现面

预计至少涉及以下模块：

1. `src/pages/FileSharePage.vue`
2. `src/locales/messages.ts`
3. `src/share-web/components/LoginDialog.vue`
4. `src/share-web/App.vue`
5. `src/share-web/api.ts`
6. `src/share-web/messages.ts`
7. `src/share-web/types.ts`
8. `src/lib/tauri.ts`
9. `src-tauri/src/fileshare/model.rs`
10. `src-tauri/src/fileshare/persist.rs`
11. `src-tauri/src/fileshare/auth.rs`
12. `src-tauri/src/fileshare/mod.rs`
13. `src-tauri/src/fileshare/http.rs`

## 12. 测试策略

### 12.1 Rust 持久化测试

覆盖以下场景：

1. 新配置文件默认包含独立访客账号。
2. 保存请求会正确写入 `guest_account` 与 `accounts`。
3. 空用户名会被拒绝。
4. 访客用户名与普通账号用户名重复时会被拒绝。
5. 普通账号用户名重复时会被拒绝。
6. 密码哈希仍然只以哈希形式持久化。

### 12.2 Rust HTTP / 鉴权测试

覆盖以下场景：

1. 使用普通账号用户名和密码登录成功。
2. 使用访客用户名和密码登录成功。
3. 访客开启且无密码时，请求自动获得访客会话。
4. 用户名错误或密码错误时返回 `401`。
5. `/api/session` 只返回 `username` 与 `is_guest`，不再返回旧的 `account_id` / `account_name`。

### 12.3 前端验证

覆盖以下场景：

1. 设置页中不再显示“显示名称”和“登录 ID”。
2. 自定义账号编辑、删除、新增在用户名可编辑时仍保持稳定。
3. share-web 登录框按“用户名 + 密码”工作。
4. 顶部身份展示只使用单一用户名字段。

### 12.4 手工验证

1. 新建访客用户名并设置密码，保存并重启后可用该用户名登录。
2. 新建普通账号并登录，权限行为与旧逻辑一致。
3. 访客无密码时可直接访问。
4. 同名账号保存时报错。

## 13. 风险与约束

1. 这是破坏式配置切换，升级后旧账号配置不会自动保留。
2. 用户名现在是唯一身份字段，前端草稿层如果没有独立临时 key，容易出现编辑态错位。
3. 访客与普通账号都可能参与显式登录，因此唯一性校验必须覆盖两类账号。
4. 任一接口若遗漏旧字段清理，都会导致前后端类型不一致。

## 14. 设计结论

本次设计不采用“界面只删一个输入框”的折中方案，而是把文件共享账号体系完整改造成单用户名模型：

1. 用户看到一个用户名。
2. 登录使用同一个用户名。
3. 后端内部也只保存这一套用户名。
4. 访客通过独立结构与 `is_guest` 标记区分，而不是隐藏 ID。

这能从根本上消除“显示名称”和“登录 ID”并存带来的理解成本与实现复杂度。
