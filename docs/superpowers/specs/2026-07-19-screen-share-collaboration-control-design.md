# 局域网屏幕共享协作与远程控制扩展 — Design Spec

- **Date**: 2026-07-19
- **Status**: Draft / awaiting user review
- **Scope**: Windows 共享端、可信局域网、浏览器零安装观看；扩展多人批注、共享冻结帧和经共享者批准的单人远程操作
- **Base Spec**: `docs/superpowers/specs/2026-04-03-screen-share-tool-design.md`
- **Source of Truth**: 当前源码；旧 Spec 中与当前实现不一致的捕获后端、状态字段和观看页行为不再作为实现依据

---

## 1. 背景

当前屏幕共享已经具备 Windows 屏幕采集、MJPEG 浏览器观看、多显示器选择、访问密码、二维码、观看人数、连接 IP、捕获异常恢复等能力，但浏览器只能被动观看。

本扩展希望在不重做视频链路的前提下增加两类局域网交互：

1. 观看者在浏览器上画箭头、方框或激光点，其他观看者同步看到。
2. 观看者在浏览器申请操作共享者电脑，经共享者本机同意后进行鼠标操作；基础键盘控制作为后续可选能力。

本 Spec 是原屏幕共享设计的增量扩展。旧 Spec 中“远程控制不支持”由本 Spec 的分阶段范围取代；音频、公网、录屏和跨平台等非目标继续有效。

---

## 2. 场景假设

本设计只面向以下环境：

- 共享者和观看者位于同一可信局域网。
- 共享者运行的是当前 Windows 桌面应用和交互式用户会话。
- 观看者通过现代 Chromium、Edge、Chrome 或同等级浏览器访问 `http://ip:port`。
- 使用场景是临时会议、演示、排障和协作，不是无人值守运维。
- 网络内的观看者被视为低风险参与者，不建设企业级身份、证书或审计体系。
- 一个共享会话可以有多个观看者和批注者，但同一时刻最多一个远程控制者。

即使处于可信局域网，远程输入仍必须经过共享者本机确认。这是防误操作要求，不属于高安全等级建设。

---

## 3. 决策摘要

| 决策项 | 本 Spec 选择 | 理由 |
|---|---|---|
| 视频传输 | 保留现有 MJPEG | 当前链路已可用，批注和控制不要求重做视频 |
| 实时交互 | 新增同源 WebSocket `/session/ws` | 同时承载批注、会话状态和控制事件 |
| 批注呈现 | 浏览器透明 SVG/Canvas 叠层 | 不增加 JPEG 编码负担，便于撤销和同步 |
| 批注状态 | 服务端内存权威状态 + revision | 新观看者和重连者可以恢复一致状态 |
| 截图式批注 | 增加全员共享冻结帧 | 避免各观看者在不同画面上批注 |
| 远程控制 | 浏览器申请、共享者本机批准、单控制者 | 满足局域网临时协作且避免误操作 |
| 输入注入 | Rust 专用工作线程 + Windows `SendInput` | 正确处理多屏、按键状态和队列清理 |
| 认证 | 复用现有可选用户名/密码和 Cookie | 本期不建设新账号或令牌体系 |
| 加密 | 继续 HTTP + `ws://` | 仅面向可信局域网；不做 TLS/WSS |
| WebRTC | 不做 | 无公网、音频和弱网要求，MJPEG 足够 |
| 观看页实现 | 从 Rust raw string 拆为独立可构建资源 | 工具栏、画布和协议状态已超过内嵌脚本的可维护范围 |

---

## 4. 范围分级

### 4.1 P0：首期必须完成 — 多人批注

- 独立的屏幕共享 Web 观看端资源和构建入口。
- WebSocket 连接、重连、心跳和服务端快照。
- 观看、激光点、箭头、方框四种模式。
- 固定颜色色板和有限线宽选择。
- 撤销自己的上一条批注。
- 清除自己的全部批注。
- 共享者从桌面应用清除全部批注。
- 所有连接者实时收到批注。
- 新加入和断线重连的观看者收到当前完整快照。
- 批注坐标随浏览器窗口、全屏和设备像素比正确缩放。
- 共享冻结帧和恢复直播。
- 显示器、分辨率或实际捕获源变化时清空旧批注并退出冻结状态。

### 4.2 P1：第二期必须完成 — 受控鼠标操作

- 共享者页面提供“允许远程控制申请”开关，默认关闭。
- 观看者页面在开关开启时显示“申请控制”。
- 共享者本机收到申请并可以允许或拒绝。
- 同一时刻只允许一个控制者。
- 支持鼠标移动、左键、右键、双击、拖拽和滚轮。
- 控制中所有观看者可见“某观看者正在控制”状态。
- 共享者页面始终提供“立即停止控制”。
- 控制者主动释放、连接断开、停止共享、捕获暂停或显示器变化时自动终止控制。
- 终止时清空待执行输入，并释放所有仍按下的鼠标按钮。

### 4.3 P2：可选增强 — 基础键盘

- 字母、数字、方向键、Enter、Escape、Backspace、Tab。
- Ctrl、Shift、Alt 及常用组合键。
- 浏览器失焦、断线或撤权时释放所有修饰键。
- 键盘控制可由共享者单独关闭，不与鼠标权限强绑定。

P2 不作为 P0/P1 验收阻塞项。先验证鼠标控制的延迟、坐标和异常清理，再决定是否进入键盘实现。

### 4.4 明确暂不做

- 公网访问、NAT 穿透、中继服务器、TURN/STUN。
- HTTPS、WSS、证书签发、设备身份、一次性配对码。
- WebRTC、H.264/AV1、音频、麦克风和自适应码率。
- 企业账号、角色目录、细粒度 ACL、审批流和合规审计。
- 无人值守控制、自动批准、Windows 服务、开机前控制。
- 锁屏、登录界面、UAC 安全桌面、切换用户和 `Ctrl+Alt+Del` 控制。
- 保证控制管理员权限高于本应用的窗口。
- 多人同时控制、控制权排队和自动移交。
- 远程剪贴板、文件传输、拖放文件、聊天和语音。
- 录屏、回放、会话历史、批注持久化和批注导出。
- 文本框、贴纸、图片粘贴、复杂图形编辑和批注对象缩放旋转。
- macOS/Linux 共享端和浏览器以外的观看客户端。
- 移动端软键盘、手势缩放与远控手势的完整适配；手机继续保证观看和基础批注。

---

## 5. 当前实现基线与兼容约束

以下能力必须保留：

- `auto / WGC / DXGI` 捕获后端和现有自动恢复逻辑。
- 屏幕、网卡、端口、用户名、密码、画质、FPS 和光标设置。
- `GET /stream` 的 MJPEG 输出和慢观看者跳帧策略。
- 浏览器暂停、刷新率限制、全屏、断线重连和捕获异常提示。
- 共享者页面的访问地址、二维码、观看人数、连接数、IP、FPS、码率和时长。
- `screen_share_start`、`screen_share_stop`、`screen_share_get_status` 的现有生命周期语义。
- 停止后端口确定性释放和同端口立即重启能力。

本扩展不得：

- 把批注对象合成到每一张 JPEG。
- 让批注消息复用容量为 8、允许 `Lagged` 跳过的帧广播通道。
- 在 Tokio 请求任务中直接执行阻塞输入注入。
- 因协作通道异常而停止屏幕捕获或 MJPEG 观看。
- 破坏只观看浏览器的现有访问方式。

---

## 6. 观看者体验

### 6.1 工具栏

观看页底部工具栏在现有状态、刷新率、暂停和全屏控制基础上增加：

```text
[观看] [激光点] [箭头] [方框] [颜色] [线宽] [撤销] [清除我的]
[冻结画面/恢复直播] [申请控制/释放控制]
```

行为约定：

- 默认模式为“观看”，浏览器点击不产生批注或远程输入。
- 选择批注工具后，拖动在屏幕实际画面区域内创建对象。
- 黑边和工具栏区域不接受批注坐标。
- 激光点为短时对象，建议 2 秒自动消失；箭头和方框保留到撤销、清除、恢复直播或源变化。
- 暂停是当前浏览器的本地行为；冻结画面是整个会话的共享行为，两者必须使用不同文案和状态。
- 共享冻结期间所有观看者看到同一个 `frame_id` 对应的 JPEG 和同一批注层。
- 任何观看者可请求冻结或恢复直播；事件由服务端排序并广播。若后续发现干扰较大，再增加共享者开关，不在首期引入角色系统。

### 6.2 批注可见范围

首期批注只覆盖在本项目浏览器观看页上：

- 所有通过观看页连接的浏览器都能看到。
- 共享者物理桌面不会显示批注。
- 直接请求 `/stream` 的第三方 MJPEG 客户端不会看到批注。
- 不增加 Windows 透明顶层批注窗口。

### 6.3 控制状态

控制状态固定为：

```text
disabled -> available -> requested -> granted -> revoked
```

- `disabled`：共享者未开启控制申请。
- `available`：观看者可以申请。
- `requested`：申请已送达共享者，尚未决定。
- `granted`：该客户端可以发送输入。
- `revoked`：拒绝、释放或异常终止，随后回到 `available` 或 `disabled`。

第二个观看者在已有控制者时申请，服务端直接返回“当前已有控制者”，不建立等待队列。

---

## 7. 批注和坐标契约

### 7.1 服务端权威模型

```typescript
interface AnnotationDocument {
  sessionId: number;
  sourceEpoch: number;
  revision: number;
  mode: 'live' | 'frozen';
  frozenFrameId: number | null;
  shapes: AnnotationShape[];
}

interface AnnotationShape {
  id: string;
  ownerClientId: string;
  kind: 'laser' | 'arrow' | 'rect';
  points: Array<{ x: number; y: number }>;
  color: string;
  width: number;
  expiresAtMs: number | null;
}
```

服务端负责：

- 校验 `sessionId` 和 `sourceEpoch`。
- 生成对象 ID 和单调递增 `revision`。
- 按连接顺序应用操作并广播结果。
- 向新连接发送完整 `snapshot`。
- 客户端消息落后或发生 revision 缺口时重新发送 `snapshot`，而不是继续应用不完整增量。
- 共享停止时丢弃全部内存状态，不落盘。

### 7.2 坐标

- 所有点使用原始捕获画面的归一化坐标 `[0, 1]`。
- 浏览器通过 `<img>.naturalWidth/naturalHeight`、容器尺寸和 `object-fit: contain` 计算实际显示矩形。
- SVG/Canvas 只覆盖实际显示矩形，不覆盖 letterbox 黑边。
- 使用 `ResizeObserver` 处理窗口变化，并按 `devicePixelRatio` 调整绘制清晰度。
- 服务端拒绝非有限值和超出 `[0, 1]` 的坐标。
- 捕获源尺寸、显示器或旋转变化时增加 `sourceEpoch`，清空批注并通知客户端重建画布。

### 7.3 冻结帧

- 捕获线程继续采集和推流，冻结只影响浏览器展示，不暂停共享者桌面。
- 服务端始终保留最近一张完整 JPEG，冻结时分配 `frameId` 并保存该帧。
- 新增 `GET /snapshot/:frame_id`，只返回当前冻结帧；旧 ID 返回 `404`。
- 恢复直播时删除冻结帧和所有非激光批注，所有客户端重新连接或恢复 `/stream`。
- 同一会话最多保存一张冻结帧，避免内存累积。

### 7.4 稳定性边界

即使不做高安全建设，也必须保留以下资源边界：

- 单条 WebSocket JSON 最大 64 KiB。
- 单会话最多 200 个持久批注对象。
- 单对象最多 256 个点；首期箭头和方框实际只使用 2 个点。
- 鼠标移动按最新值覆盖，输入队列不得无界增长。
- 无效类型、过期 session/source epoch 和无法解析的消息返回错误但不关闭视频共享。

这些限制用于避免误操作和内存失控，不视为安全体系建设。

---

## 8. 远程输入契约

### 8.1 申请和批准

1. 观看者发送 `control.request`。
2. Rust 生成 `requestId`，记录 `clientId`、IP 和 User-Agent，并向 Tauri 页面 emit `screen-share-control-request`。
3. 共享者页面显示非阻塞确认框：“IP x.x.x.x 请求控制当前屏幕”。
4. 共享者选择允许或拒绝。
5. 允许后服务端广播 `control.state = granted`；拒绝后回到 `available`。
6. 授权仅对当前 WebSocket 连接和当前共享会话有效，不跨重连恢复。

本期不设计密码之外的二次验证码、租约 Token 或审批记录。

### 8.2 输入范围

P1 支持：

- 绝对鼠标移动。
- 左键、右键按下和抬起。
- 双击由浏览器发送两组完整按下/抬起事件，不新增专用 Windows 动作。
- 拖拽由按下、移动、抬起序列组成。
- 垂直滚轮；水平滚轮暂不做。

P2 可选支持基本键盘。中文输入法、组合文本事件、剪贴板粘贴和复杂国际键盘布局不在本期保证范围内。

### 8.3 坐标映射

浏览器发送：

```json
{
  "type": "input.pointer_move",
  "session_id": 42,
  "source_epoch": 7,
  "seq": 128,
  "x": 0.42,
  "y": 0.73
}
```

Rust 必须使用当前实际捕获源，而不是启动时选择值：

```text
归一化画面坐标
  -> 当前显示器物理 RECT(left, top, width, height)
  -> Windows 虚拟桌面坐标
  -> SendInput 0..65535 + MOUSEEVENTF_VIRTUALDESK
```

显示器描述必须统一包含稳定标识、`left/top/width/height`、主屏标志和当前 `sourceEpoch`。捕获、光标叠加和输入映射必须引用同一个活动描述，避免恢复过程中“看到 B 屏却点击 A 屏”。

### 8.4 输入工作线程

- 输入注入运行在单独的串行 `std::thread`，不占用 Tokio worker。
- 使用有界命令队列。
- `pointer_move` 可以覆盖尚未执行的旧移动。
- `button_down/up` 和键盘边沿必须保持顺序。
- 执行每条输入前再次确认当前 controller、session 和 source epoch。
- 撤权时先禁止新输入，再清空队列，最后释放所有已记录为按下的按钮和按键。

### 8.5 自动终止条件

以下任一条件发生时立即撤销控制：

- 控制者 WebSocket 断开或心跳超时。
- 控制者主动释放。
- 共享者点击“停止控制”或关闭控制申请。
- 屏幕共享停止或重启。
- 捕获进入 `capture_paused`。
- 活动显示器、分辨率或 `sourceEpoch` 变化。
- 输入工作线程异常退出。

锁屏或 UAC 安全桌面期间不缓存输入，恢复后必须重新申请。

---

## 9. WebSocket 协议

### 9.1 连接

- 新增 `GET /session/ws`。
- Axum 增加 `ws` feature。
- 若当前共享启用了用户名/密码，握手复用现有 `ss_auth` Cookie 检查。
- 若未启用密码，则与当前只读观看一样允许局域网直接连接。
- 服务端为每个 WebSocket 连接生成随机 `clientId`，不能以 IP 作为唯一标识。
- WebSocket 失败只禁用交互功能，MJPEG 观看继续工作并显示“交互连接已断开”。

### 9.2 消息包络

```typescript
interface SessionMessage<T = unknown> {
  v: 1;
  type: string;
  session_id: number;
  source_epoch: number;
  client_seq?: number;
  revision?: number;
  payload?: T;
}
```

### 9.3 客户端到服务端

| Type | 说明 | 阶段 |
|---|---|---|
| `session.heartbeat` | 连接保活 | P0 |
| `annotation.add` | 提交激光点、箭头或方框 | P0 |
| `annotation.undo` | 撤销自己的上一条持久批注 | P0 |
| `annotation.clear_own` | 清除自己的批注 | P0 |
| `view.freeze` | 请求共享冻结当前帧 | P0 |
| `view.resume` | 恢复实时画面 | P0 |
| `control.request` | 申请控制 | P1 |
| `control.release` | 主动释放控制 | P1 |
| `input.pointer_move` | 鼠标绝对移动 | P1 |
| `input.pointer_button` | 鼠标按钮按下/抬起 | P1 |
| `input.wheel` | 垂直滚轮 | P1 |
| `input.key` | 键盘按下/抬起 | P2 |

### 9.4 服务端到客户端

| Type | 说明 | 阶段 |
|---|---|---|
| `session.hello` | clientId、会话、源和功能开关 | P0 |
| `session.snapshot` | 批注文档、冻结和控制状态快照 | P0 |
| `annotation.applied` | 服务端已排序的批注操作 | P0 |
| `view.state` | live/frozen 和 frameId | P0 |
| `source.changed` | 源尺寸或 epoch 变化 | P0 |
| `control.requested` | 申请已送达 | P1 |
| `control.state` | available/requested/granted/revoked | P1 |
| `session.error` | 可展示的协议错误 | P0 |

---

## 10. 后端数据与接口扩展

### 10.1 `ScreenShareConfig`

新增：

```typescript
interface ScreenShareInteractionConfig {
  annotations_enabled: boolean;          // 默认 true
  shared_freeze_enabled: boolean;        // 默认 true
  control_requests_enabled: boolean;     // 默认 false
  keyboard_control_enabled: boolean;     // 默认 false，P2
}
```

字段同步进入 Rust、`src/lib/tauri.ts`、页面保存设置和默认值。旧设置缺字段时必须使用上述默认值。

### 10.2 `ScreenShareStatus`

新增：

```typescript
interface ScreenShareInteractionStatus {
  interaction_connected_count: number;
  annotation_count: number;
  view_mode: 'live' | 'frozen';
  control_state: 'disabled' | 'available' | 'requested' | 'granted';
  controller_ip: string | null;
  source_epoch: number;
}
```

### 10.3 Tauri commands

新增：

| Command | 参数 | 返回 | 说明 |
|---|---|---|---|
| `screen_share_respond_control_request` | `requestId, allow` | `Result<void, String>` | 允许或拒绝当前申请 |
| `screen_share_revoke_control` | 无 | `Result<void, String>` | 立即撤销当前控制 |
| `screen_share_clear_annotations` | 无 | `Result<void, String>` | 共享者清空所有批注 |

### 10.4 Tauri events

新增：

| Event | Payload | 说明 |
|---|---|---|
| `screen-share-control-request` | `{ request_id, client_id, ip, user_agent }` | 共享者本机显示控制申请 |
| `screen-share-interaction-status` | 交互状态快照 | 控制、冻结或批注数量变化时更新页面 |

### 10.5 HTTP routes

保留：`/`、`/stream`、`/auth`、`/status`。

新增：

| Method | Path | 说明 |
|---|---|---|
| `GET` | `/session/ws` | 批注和控制 WebSocket |
| `GET` | `/snapshot/:frame_id` | 当前共享冻结帧 |

### 10.6 Rust 模块边界

`screenshare.rs` 已承担捕获、编码、恢复、HTTP 和内嵌页面。新增功能不应继续全部堆入该文件：

```text
src-tauri/src/screenshare/
├── mod.rs           # command、会话生命周期和现有导出
├── capture.rs       # 现有 WGC/DXGI 捕获与恢复（后续机械迁移，不阻塞 P0）
├── http.rs          # Router、MJPEG、snapshot、WebSocket upgrade
├── interaction.rs   # client、批注文档、revision、冻结和控制状态机
├── input.rs         # Windows SendInput worker 和按键/按钮状态
└── protocol.rs      # WebSocket serde 消息与校验
```

为控制改造一次性拆分整个 `screenshare.rs` 风险较高。实施时允许先新增 `screenshare_interaction.rs` / `screenshare_input.rs` 并保持现有文件，待行为稳定后再做机械拆分。

---

## 11. 前端扩展

### 11.1 共享者页面

配置区新增：

- “允许观看者批注”，默认开启。
- “允许共享冻结画面”，默认开启。
- “允许远程控制申请”，默认关闭。
- “允许键盘控制”，默认关闭且仅在远控开启时可用，P2。

运行状态区新增：

- 当前批注数量和“清空全部”命令。
- 当前 live/frozen 状态。
- 待处理控制申请确认框。
- 当前控制者 IP 和显眼的“停止控制”按钮。

所有新增文案同步维护中英文 Vue I18n。

### 11.2 浏览器观看端

建议新增独立入口 `src/screen-share-web/`，使用 Vue 3 + TypeScript 构建并由 Rust 嵌入静态产物。该入口与 `src/share-web/` 独立，避免把文件共享和屏幕共享协议耦合。

观看端必须保留：

- 首屏直接进入观看，不增加介绍页。
- 当前画面为主区域，工具栏不遮挡画面。
- 桌面和手机浏览器文本、图标不溢出。
- 暂停、刷新率、全屏、状态、观看人数和捕获异常提示。
- WebSocket 断开时继续观看，并允许单独重连交互通道。

---

## 12. 异常处理

| 场景 | 期望行为 |
|---|---|
| WebSocket 建立失败 | MJPEG 继续播放，批注/控制按钮禁用并提示交互连接断开 |
| WebSocket 重连 | 服务端分配新 clientId，下发完整 snapshot；旧控制权不恢复 |
| 客户端 revision 缺口 | 丢弃本地增量，申请或接收完整 snapshot |
| 冻结帧失效 | 自动恢复 live，清空批注并广播状态 |
| 捕获暂停/锁屏/UAC | 保留观看连接和最后画面，撤销控制，不缓存输入 |
| 捕获源切换/尺寸变化 | sourceEpoch +1，恢复 live，清空批注，撤销控制 |
| 控制者断线 | 立即撤权，清队列，释放按钮/按键 |
| 输入队列满 | 移动事件覆盖旧值；按钮/按键事件失败时撤权，避免状态不一致 |
| 共享停止 | 关闭 WebSocket、清理批注/冻结/控制状态、停止输入线程 |
| 旧浏览器或 JS 异常 | 仍可直接使用 `/stream` 观看；交互能力不保证 |

---

## 13. 测试要求

### 13.1 Rust 单元测试

- 批注 add/undo/clear 和 revision 单调递增。
- 新客户端 snapshot 与当前权威状态一致。
- source epoch 变化会清空批注、退出冻结并撤销控制。
- 第二个控制申请在已有控制者时被拒绝。
- 断线/撤权释放所有按钮和按键。
- 归一化坐标到负坐标显示器和虚拟桌面的映射。
- 旧 session、旧 source epoch、越界坐标和未知消息被拒绝。
- 输入队列满时移动合并、边沿事件不静默丢弃。

### 13.2 HTTP/WebSocket 集成测试

- 未启用密码时 WebSocket 可连接；启用密码时复用现有认证结果。
- 两个客户端之间批注广播顺序一致。
- 晚加入客户端收到完整批注和冻结快照。
- `/snapshot/:frame_id` 只返回当前有效冻结帧。
- WebSocket 关闭不影响 `/stream`。

### 13.3 前端测试

- `object-fit: contain` 下画面矩形和归一化坐标计算。
- resize、全屏和 DPR 变化后批注仍对齐。
- 观看/批注/控制模式互斥，避免一次操作同时画图和点击远端。
- 交互断线、控制申请中、控制中和撤权状态文案。
- 所有新增中英文文案键完整。

### 13.4 手工验收环境

- Windows 单屏 100% / 125% / 150% 缩放。
- 主屏左右各有副屏，覆盖负 `left/top` 坐标。
- 两台桌面浏览器和一台手机同时观看。
- Chrome 和 Edge 最新稳定版。
- 锁屏、解锁、显示器断开/重连、停止后同端口重启。

---

## 14. 验收标准

### 14.1 P0 批注

1. 两个浏览器同时观看时，在任一浏览器画箭头或方框，另一浏览器在局域网内 200ms 内看到。
2. 不同窗口比例和全屏状态下，批注指向相同的屏幕内容，边缘误差不超过 3 个 CSS 像素。
3. 新加入浏览器在连接后 1 秒内获得当前批注快照。
4. 冻结后所有浏览器显示同一帧；恢复后全部回到直播并清空冻结批注。
5. 捕获源变化后不保留旧坐标批注。
6. WebSocket 故障不影响 MJPEG 继续观看。
7. 10 个浏览器同时连接并进行低频批注时，共享端无明显卡顿，视频 FPS 不因静止批注下降超过 10%。

### 14.2 P1 鼠标控制

1. 未开启控制申请时，观看端不能发送有效输入。
2. 观看者申请后，必须由共享者本机允许才可控制。
3. 鼠标移动、点击、右键、拖拽和滚轮在单屏及左右副屏上位置正确。
4. 同一时刻仅一个控制者，其他观看者继续观看和批注。
5. 断开控制者网络后 1 秒内撤权，且不出现鼠标按钮卡住。
6. 锁屏、捕获暂停或源变化立即终止控制，恢复后必须重新申请。
7. 从浏览器操作到下一帧看到结果的局域网 P95 目标不超过 300ms；不为此引入 WebRTC。

### 14.3 工程验证

- `cargo test --manifest-path src-tauri/Cargo.toml screenshare`
- 屏幕共享 Web 前端的单元和构建测试
- `pnpm check`
- `pnpm lint`
- `pnpm build`
- `git diff --check`
- Windows 局域网实机冒烟；不要求 Tauri release 构建，除非进入发布阶段

---

## 15. 分阶段实施建议

### Phase 0：控制面基础

- 拆出可构建的屏幕共享观看端。
- Axum 启用 WebSocket。
- 建立 session/client/source epoch、消息包络、重连和 snapshot。
- 保证 WebSocket 故障不影响 MJPEG。

### Phase 1：批注

- 激光点、箭头、方框、颜色、线宽。
- 服务端权威文档、revision、撤销和清除。
- 多窗口尺寸与手机观看适配。

### Phase 2：共享冻结帧

- 保存最新 JPEG、`frameId` 和 snapshot 路由。
- 全员 freeze/live 状态同步。
- 源变化自动退出冻结。

Phase 0-2 组成建议优先交付的“多人批注版”。

### Phase 3：受控鼠标操作

- 共享者开关、申请确认、单控制者状态机。
- 显示器统一描述和坐标映射。
- SendInput worker、按钮状态和异常清理。

### Phase 4：基础键盘（可选）

- 基本键位、修饰键和常用组合键。
- 浏览器焦点和强制释放处理。
- 根据 Phase 3 实机反馈决定是否实施。

---

## 16. 用户评审项

请重点确认以下范围选择：

1. **批注是否只在浏览器显示**：本 Spec 选择“是”，不在共享者物理桌面显示。
2. **是否需要共享冻结帧**：本 Spec 选择“需要”，否则截图式批注语义不一致。
3. **谁可以批注/冻结**：本 Spec 选择“所有成功进入观看页的人”，不增加逐人授权。
4. **远控首期范围**：本 Spec 选择“鼠标优先”，基础键盘后置为可选。
5. **远控批准方式**：本 Spec 选择“共享者每次本机批准”，不做自动批准和无人值守。
6. **网络安全范围**：本 Spec 选择继续使用 HTTP、现有可选密码和同源 Cookie，不做 TLS、账号体系和审计。
7. **视频技术**：本 Spec 选择继续使用 MJPEG，不做 WebRTC 和音频。

上述 7 项确认后，再编写逐任务实施计划；在 Spec 评审完成前不进入代码实现。
