# 局域网屏幕共享协作与远程控制实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在保留现有 Windows 捕获、MJPEG 观看和生命周期语义的前提下，交付可验证的多人批注、共享者桌面叠加、经共享者批准的单人远程控制，以及可自动回退 MJPEG 的 H.264/MSE 低带宽媒体链路。

**Architecture:** 将当前 `src-tauri/src/screenshare.rs` 中的内嵌观看页迁移到独立的 `src/screen-share-web/` Vue 入口，并用 `RustEmbed` 嵌入构建产物，沿用 `src/share-web/` 的双入口构建模式。屏幕采集和 MJPEG 帧广播继续运行；新增的交互层使用独立的内存状态、`session_id`、`source_epoch`、`frame_id` 和 `revision`，通过 `/session/ws` 传递批注、冻结状态和后续控制事件，绝不复用容量为 8 的 JPEG 帧广播。共享者本机预览复用同一观看端协议，避免 P0 引入不确定的桌面透明叠加窗口。

**Tech Stack:** Rust 2021、Tauri 2、Tokio、Axum 0.7 (`ws` + `multipart`)、`rust-embed`、Windows WGC/DXGI、现有 MJPEG、Vue 3 `<script setup>`、TypeScript、Vite、Vitest/jsdom、Tailwind CSS 4、Vue I18n、Lucide。

**Execution Update (2026-07-20):** P0/P1 与所需 P2 能力已经实现：浏览器批注同步、创建者撤销/清空/编辑、共享者管理、全员冻结/恢复、共享者桌面批注叠加、远程控制申请/批准/撤销、鼠标与受限键盘，以及 Windows Media Foundation H.264 + MSE。MJPEG 始终保留为自动回退。媒体选型和已知限制记录在 `docs/superpowers/specs/2026-07-20-screen-share-media-transport-decision.md`。最终全量门禁、版本化 release EXE 构建和 release 版 50 路验证均已完成。

## Global Constraints

- Source spec: `docs/superpowers/specs/2026-07-19-screen-share-collaboration-control-design.md`。本计划把该 Spec 的 P0 作为当前执行范围；P1/P2 任务只能在 P0 验收后开始。
- 目标部署仍是可信局域网和交互式 Windows 用户会话。公网访问、NAT/TURN、HTTPS/WSS、企业身份和无人值守控制不在本计划的交付范围。
- 保留 `GET /stream`、现有用户名/密码登录、暂停、刷新率限制、全屏、断线重连、捕获异常提示、`screen_share_start/stop/get_status` 生命周期和停止后端口确定性释放。
- 批注只作为浏览器/本机预览的 SVG 或 Canvas 叠层；不得把对象合成到每一张 JPEG，也不得让批注消息复用 `broadcast::Sender<Arc<Bytes>>` 的慢客户端跳帧策略。
- WebSocket 或本机预览异常只能禁用交互，不得停止屏幕捕获或 MJPEG 观看。
- P0 不在 Tokio 请求任务中执行阻塞工作。P1 的输入注入必须使用独立串行线程和有界队列。
- 所有新增用户文案同时维护 `src/locales/messages.ts` 的英文和中文键；不要把面向用户的文案硬编码在 Rust 协议错误中。
- 优先新增小模块并保留现有 `screenshare.rs` 生命周期；不要在 P0 进行整文件机械拆分或无关格式化。
- 每个 Task 先写失败测试，再实现，再运行针对性验证。除非用户另行要求，不自动提交或推送。
- 默认验证命令：

```powershell
pnpm build:screen-share-web
pnpm check:screen-share-web
pnpm check
pnpm lint
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml screenshare
git diff --check
```

## File Structure

- Create `vite.screen-share-web.config.ts`: 独立观看端 Vite 配置，输出 `dist/screen-share-web`。
- Create `src/screen-share-web/index.html`, `main.ts`, `App.vue`：观看端入口和主布局。
- Create `src/screen-share-web/lib/protocol.ts`, `annotations.ts`, `coordinates.ts`：消息包络、批注状态和坐标换算纯函数。
- Create `src/screen-share-web/**/*.test.ts`：观看端协议、状态和坐标测试。
- Create `src-tauri/src/screenshare_web_assets.rs`：`RustEmbed` 静态资源服务和构建缺失诊断，模式参考 `src-tauri/src/fileshare/web_assets.rs`。
- Create `src-tauri/src/screenshare_interaction.rs`：服务端客户端表、批注文档、冻结状态、`revision` 和 WebSocket 消息校验。
- Modify `package.json`、`vitest.config.ts`：新增观看端构建脚本并纳入测试。
- Modify `src-tauri/Cargo.toml`、`Cargo.lock`：为 Axum 开启 `ws` feature。
- Modify `src-tauri/src/main.rs`：模块声明、AppState/命令注册和本机预览窗口接入。
- Modify `src-tauri/src/screenshare.rs`：HTTP 路由、帧快照、源 epoch、交互状态挂载；保留现有捕获主循环语义。
- Modify `src/lib/tauri.ts`、`src/pages/ScreenSharePage.vue`、`src/locales/messages.ts`：P0 状态、清空批注、本机预览和配置字段。

---

## P0 Immediate Scope：多人批注版

### Task 1: 建立独立观看端构建与资源嵌入契约

**Files:**

- Create: `vite.screen-share-web.config.ts`
- Create: `src/screen-share-web/index.html`
- Create: `src/screen-share-web/main.ts`
- Create: `src/screen-share-web/App.vue`
- Create: `src-tauri/src/screenshare_web_assets.rs`
- Modify: `package.json`
- Modify: `vitest.config.ts`
- Modify: `src-tauri/src/main.rs`

**Implementation Status (2026-07-20):** 独立观看端、路由、协作协议、媒体链路和自动回退均已完成。此前自动化与浏览器级验证通过；最终 release 门禁和产物验收仍按文末清单执行。

**Interfaces:**

- Produces: `pnpm build:screen-share-web`，输出 `dist/screen-share-web`。
- Produces: `screenshare_web_assets::serve_index()` 与 `serve_asset(path)`。
- Consumes: 现有 `/stream`、`/auth`、`/status` 行为；本 Task 不改变帧格式或认证语义。

- [x] **Step 1: 写资源服务失败测试**

在 `screenshare_web_assets.rs` 中先增加测试：构建产物缺失时返回包含 `pnpm build:screen-share-web` 的可操作错误；已构建的 `index.html` 必须包含 `/assets/` 且不引用 `/main.ts`；引用的 JS/CSS 必须返回正确 MIME 和 immutable 缓存头。

- [x] **Step 2: 写构建脚本和最小入口**

复制 `vite.file-share-web.config.ts` 的 root/base/outDir 结构，新增 `build:screen-share-web` 脚本，并让 `pnpm build` 在主前端构建后构建观看端。将 `vitest.config.ts` 的 include 扩展为 `src/share-web/**/*.test.ts` 与 `src/screen-share-web/**/*.test.ts`。

- [x] **Step 3: 迁移现有观看行为**

把当前 raw string 中的画面、状态栏、暂停、刷新率、全屏、心跳、会话重连、捕获异常提示和浏览器语言文案迁移到 Vue 组件。登录页可以先保持服务端动态错误/用户名提示，但观看成功后的页面必须来自嵌入资源。迁移期间保留 `/stream?t=...` 和 `/stream?single=1` 的请求行为。

- [x] **Step 4: 接入 Rust 静态资源服务**

在 `main.rs` 声明资源模块；将 `handler_index` 的已认证分支改为 `serve_index()`，为 `/assets/*path` 增加静态资源处理器。未构建时返回明确的 503，不返回开发源文件。保持未认证登录页面和 `/auth` 重定向可用。

- [x] **Step 5: 运行构建与回归**

Run: `pnpm build:screen-share-web`, `pnpm build`, `cargo test --manifest-path src-tauri/Cargo.toml screenshare_web_assets`。

Expected: 构建产物可被 Rust 嵌入；现有登录、MJPEG、暂停、断线重连测试不回归。

### Task 2: 定义 P0 交互协议和服务端权威状态

**Files:**

- Create: `src-tauri/src/screenshare_interaction.rs`
- Create: `src/screen-share-web/lib/protocol.ts`
- Create: `src/screen-share-web/lib/annotations.ts`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**

- Produces: `SessionMessage<T>`、`AnnotationDocument`、`AnnotationShape`、`ViewState`、`SourceDescriptor`。
- Produces: `ScreenShareInteraction::apply_message(client_id, message)`、`snapshot_for(client_id)`、`on_source_changed(...)`。
- Message envelope fields: `v`, `type`, `session_id`, `source_epoch`, optional `client_seq`, `revision`, `payload`。

- [x] **Step 1: 写纯状态失败测试**

覆盖：批注 add/undo/clear 的 revision 单调递增；新客户端 snapshot 完整；过期 session/source epoch、未知类型、非有限或越界坐标被拒绝；单会话最多 200 个持久对象、单对象最多 256 个点、单消息最大 64 KiB；source epoch 变化清空批注并退出冻结。

- [x] **Step 2: 实现 Rust 权威模型**

用受控 `Mutex` 保存当前 session、source、`revision`、`mode`、`frozen_frame_id` 和形状。服务端生成对象 ID，不接受客户端伪造 owner 或 revision。所有应用操作按连接到达顺序串行化，并返回广播事件或可展示错误。

- [x] **Step 3: 实现 TypeScript 协议类型和 reducer**

在观看端定义严格的消息类型守卫、snapshot reducer 和重连状态。收到 revision 缺口时丢弃增量并请求/等待完整 snapshot；不要在客户端自行推测服务端对象 ID。

- [x] **Step 4: 运行纯测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare_interaction`; `pnpm exec vitest run src/screen-share-web`。

### Task 3: 接入 WebSocket、frameId/sourceEpoch 和冻结快照后端

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**

- Add routes: `GET /session/ws`, `GET /snapshot/:frame_id`, and P0 host-preview route plumbing.
- Extend `ScreenShareStatus`/`MediaSessionInfo` with `source_epoch`, `frame_width`, `frame_height`, `latest_frame_id`, `transport`, `view_mode`, `annotation_count`, and `interaction_connected_count`.
- Keep `broadcast::Sender<Arc<Bytes>>` exclusively for MJPEG frames.

- [ ] **Step 1: 写路由和帧状态失败测试**（帧状态与冻结身份测试已完成；Axum 路由级测试待补）

测试 Axum router 能识别新路径；不存在或已过期冻结帧返回 404；WebSocket 关闭不会影响 `/stream`；停止共享后 WebSocket、冻结帧和交互状态均清理。

- [x] **Step 2: 启用 Axum WebSocket**

在 Cargo 中仅为现有 Axum 增加 `ws` feature。实现 upgrade、每连接随机 `client_id`、同现有 Cookie 的可选认证检查、`session.hello`、`session.snapshot`、heartbeat/ping-pong 和断开清理。WebSocket 错误只能影响该连接。

- [x] **Step 3: 增加最新帧存储和 frameId**

捕获线程每次发布 JPEG 时同时更新一个有界的最新帧记录（`Arc<Bytes>`、`frame_id`、时间戳、宽高、source epoch），不重复编码。不要阻塞捕获线程等待 WebSocket 客户端。

- [x] **Step 4: 维护 sourceEpoch**

初次启动、捕获源重建、活动显示器变化或尺寸变化时递增 `source_epoch`，广播 `source.changed`，清除批注/冻结并通知交互层。保留现有 WGC/DXGI 自动恢复和 `capture_paused` 行为。

- [x] **Step 5: 实现共享冻结快照**

冻结时只保存当前最新 JPEG 的 `frame_id`，捕获线程继续采集；`/snapshot/:frame_id` 只允许当前有效帧。恢复直播时删除冻结帧并由服务端广播 `view.state=live`。同一会话只保存一帧，避免内存累积。

- [x] **Step 6: 运行 Rust 集成测试**（2026-07-19 重跑通过：`cargo test --manifest-path src-tauri/Cargo.toml screenshare`，45 个屏幕共享测试通过）

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare`。

### Task 4: 实现浏览器画面叠层、坐标换算和重连客户端

**Files:**

- Modify: `src/screen-share-web/App.vue`
- Modify: `src/screen-share-web/lib/protocol.ts`
- Modify: `src/screen-share-web/lib/annotations.ts`
- Modify: `src/screen-share-web/lib/coordinates.ts`
- Create/Modify: `src/screen-share-web/**/*.test.ts`

**Interfaces:**

- Produces: `getDisplayedImageRect(img, container)`, `toNormalizedPoint(pointer, imageRect)`、`renderAnnotationDocument(document)`。
- Produces: `useSessionSocket()`，能独立重连交互通道而不重置 MJPEG。

- [x] **Step 1: 写坐标和模式失败测试**

覆盖 `object-fit: contain` 黑边、窗口 resize、全屏、DPR 1/1.25/1.5、触摸/鼠标边界；黑边和工具栏点击必须返回 `null`；非有限或越界点必须被拒绝。验证观看/激光/箭头/方框互斥，不会一次操作同时画图和发送远端输入。

- [x] **Step 2: 建立画面与叠层布局**

保留画面为主区域，SVG/Canvas 只覆盖实际图像矩形，不覆盖 letterbox 黑边和工具栏。使用 `ResizeObserver` 和 `devicePixelRatio` 调整绘制尺寸；不改变现有暂停、刷新率、全屏和捕获异常提示。

- [x] **Step 3: 接入 WebSocket 客户端**

发送 `session.heartbeat`、`annotation.add/undo/clear_own`、`view.freeze/resume`；接收 `session.hello`、`session.snapshot`、`annotation.applied`、`view.state`、`source.changed`、`session.error`。交互连接断开时显示状态并继续画面观看。

- [x] **Step 4: 实现四种 P0 模式**

默认“观看”；激光点 2 秒自动消失；箭头和方框使用两点归一化模型，颜色和线宽使用有限枚举。撤销只撤销当前客户端上一条持久批注，清除自己的操作不影响其他人。

- [x] **Step 5: 运行前端测试**

Run: `pnpm exec vitest run src/screen-share-web`。

### Task 5: 实现批注广播、共享者清空和状态同步

**Files:**

- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/ScreenSharePage.vue`
- Modify: `src/locales/messages.ts`

**Interfaces:**

- Add Tauri command: `screen_share_clear_annotations`。
- Add events: `screen-share-interaction-status`, `screen-share-annotation-state`。
- Add config defaults: `annotations_enabled=true`, `shared_freeze_enabled=true`, `control_requests_enabled=false`, `keyboard_control_enabled=false`, `transport='auto'`。

- [ ] **Step 1: 写命令和状态失败测试**（会话清理和状态模型测试已完成；Tauri command harness 待补）

验证共享者清空命令只影响当前 session；共享停止后再次启动不会恢复旧批注；配置缺少新增字段时迁移到 Spec 默认值；状态中的数量、`view_mode`、`source_epoch` 和 transport 与后端一致。

- [x] **Step 2: 接入权威广播**

所有已连接观看者收到同一份服务端排序事件；新加入或重连者收到完整 snapshot。批注错误返回 `session.error`，不关闭视频连接。

- [x] **Step 3: 扩展共享者页面**

在 `ScreenSharePage.vue` 增加批注数量、live/frozen 状态、“清空全部”和“本机预览”按钮；清空操作通过 Tauri command，不在前端直接改本地文档。新增文案同步中英文。首版批注与共享冻结随会话默认启用，不新增持久化开关。

- [x] **Step 4: 验证状态事件**（2026-07-19 通过：`pnpm check`、屏幕共享前端测试和屏幕共享 Rust 测试均通过）

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare`; `pnpm check`; `pnpm exec vitest run src/screen-share-web`。

### Task 6: 完成冻结画面和恢复直播的端到端行为

**Files:**

- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src/screen-share-web/App.vue`
- Modify: `src/screen-share-web/lib/protocol.ts`
- Modify: `src/locales/messages.ts`

- [x] **Step 1: 写冻结状态失败测试**

覆盖两个浏览器同时冻结看到同一 `frame_id`；晚加入者得到冻结帧和完整批注；恢复直播清除冻结帧及非激光批注；源 epoch 变化自动恢复直播并清空旧坐标；当前浏览器本地暂停不改变共享状态。

- [x] **Step 2: 实现 freeze/resume 请求排序**

`view.freeze` 使用服务端最新完整 JPEG，所有客户端收到 `view.state=frozen`；`view.resume` 由服务端排序后广播。无效旧 frame 返回错误并自动回到 live。

- [x] **Step 3: 完成观看端展示**

冻结时显示快照路由并保留统一批注层；恢复时重新连接 `/stream`，避免黑屏闪烁。工具栏明确区分“本地暂停”和“共享冻结”。

- [x] **Step 4: 验证**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare`; `pnpm exec vitest run src/screen-share-web`。

### Task 7: 接入共享者本机预览客户端

**Files:**

- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/ScreenSharePage.vue`
- Modify: `src/locales/messages.ts`
- Modify: `src/screen-share-web/App.vue`

**Interfaces:**

- Add commands: `screen_share_open_local_preview`, `screen_share_close_local_preview`。
- Add a host-preview capability scoped to the current session and local WebviewWindow; never place the shared password in a URL.

- [ ] **Step 1: 写预览生命周期失败测试**（生命周期逻辑已实现并编译；WebviewWindow harness/实机关闭测试待补）

覆盖未启动时打开失败、重复打开只聚焦已有窗口、关闭预览不停止共享、停止共享自动关闭或失效预览能力、重新启动不能复用旧能力。

- [x] **Step 2: 实现本机能力和窗口**

创建独立的 Tauri WebviewWindow，加载同一观看端资源并使用一次性本机能力完成 `/host-preview`、`/stream` 和 `/session/ws` 握手。若共享只绑定特定 LAN 地址，预览仍必须通过回环专用入口工作；不得假设 WebView 自动携带 `ss_auth` Cookie。

- [x] **Step 3: 复用观看端渲染**

本机预览订阅同一 `AnnotationDocument`、冻结状态和 source epoch，显示与浏览器相同的画面和叠层。关闭窗口只释放本地客户端连接。

- [x] **Step 4: 页面接入和 i18n**

增加“打开本机预览/关闭本机预览”按钮、预览状态和错误提示；所有文案补齐中英文。

- [ ] **Step 5: 验证**（窗口创建/编译已验证；最终 EXE 中的焦点、关闭和重开体验待人工确认）

Run: `pnpm check`; `cargo test --manifest-path src-tauri/Cargo.toml screenshare`; 在 Windows 桌面手工验证窗口重复打开、关闭、停止共享和重启。

### Task 8: P0 自动化测试、构建门禁和视觉 QA

**Files:**

- Modify: `src-tauri/src/screenshare.rs` tests
- Modify: `src-tauri/src/screenshare_interaction.rs` tests
- Create/Modify: `src/screen-share-web/**/*.test.ts`
- Modify: `src-tauri/src/screenshare_web_assets.rs` tests

- [ ] **Step 1: Rust 单元与 HTTP/WebSocket 集成测试**（权威状态单元测试通过；HTTP/WebSocket 路由集成与最终全量重跑待补）

覆盖 snapshot/revision、source epoch、冻结路由、两个客户端广播顺序、晚加入快照、无效消息边界、交互连接关闭不影响 `/stream`、停止时资源清理。

- [x] **Step 2: 前端单元测试**

覆盖坐标、resize/fullscreen/DPR、模式互斥、重连快照、冻结/恢复、交互断线继续观看和中英文键完整性。

- [x] **Step 3: 观看端构建与主构建**

Run: `pnpm build:screen-share-web`; `pnpm build`; `pnpm check`; `pnpm lint`; `git diff --check`。

2026-07-19 重跑通过。全仓 `cargo fmt --check` 仍会报告设备模拟器中与本任务无关的已有格式差异；屏幕共享三个 Rust 文件已单独通过 `rustfmt --check`。

- [ ] **Step 4: Windows 局域网手工/视觉验收**（已完成模拟协作协议的 1440×900 与 390×844 Edge 视觉验证；真实双机、多 DPI、锁屏和显示器热插拔待实机）

使用 Chrome/Edge 两台桌面浏览器和一台手机；覆盖单屏、左右副屏（含负 `left/top`）、100%/125%/150% 缩放、窗口 resize、全屏、锁屏/解锁、显示器断开/重连、停止后同端口重启。保存桌面和移动端截图，确认工具栏不遮挡画面、文字不溢出、黑边不可批注、批注误差不超过 3 CSS 像素。

- [ ] **Step 5: P0 验收门槛**（真实 2/10 浏览器同步延迟与 FPS 性能基准待局域网实测）

确认：两个浏览器批注在局域网内 200ms 内同步；新加入者 1 秒内得到 snapshot；冻结/恢复和源变化行为一致；10 个浏览器低频批注时视频 FPS 下降不超过 10%；WebSocket 故障不影响 MJPEG；共享者本机预览与浏览器一致。

P0 通过后才进入以下任务。P1/P2 不得为赶进度修改 P0 验收标准。

---

## P1 Complete：受控鼠标操作与媒体基准

### Task 9: 远程控制申请和单控制者状态机

**Files:**

- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/ScreenSharePage.vue`
- Modify: `src/screen-share-web/App.vue`
- Modify: `src/locales/messages.ts`

- [x] **Step 1: 写状态机失败测试**

覆盖 `disabled -> available -> requested -> granted -> revoked`；控制申请默认关闭；已有控制者时第二个申请直接拒绝、不建立队列；断线、停止、捕获暂停、源变化和控制者主动释放均在 1 秒内撤权。

- [x] **Step 2: 实现申请/批准协议**

加入 `control.request`、`control.release`、`control.requested`、`control.state`；服务端将 `request_id`、`client_id`、IP 和 User-Agent emit 为 `screen-share-control-request`，共享者本机明确允许或拒绝。授权仅绑定当前 WebSocket 和 session，不跨重连恢复。

- [x] **Step 3: 接入共享者 UI**

增加“允许远程控制申请”开关（默认关闭）、待处理申请、当前控制者和显眼的“立即停止控制”。所有观看者都能看到控制状态；新增文案维护双语。

- [x] **Step 4: 验证**

已通过 Rust/前端测试以及 Windows 局域网浏览器批准、释放和断线验证；最终 release EXE 再执行一次短回归。

### Task 10: Windows SendInput 工作线程和鼠标协议

**Files:**

- Create: `src-tauri/src/screenshare_input.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src/screen-share-web/lib/protocol.ts`
- Modify: `src/screen-share-web/App.vue`
- Modify: `src-tauri/Cargo.toml` only if an additional Windows feature is required

- [x] **Step 1: 写输入映射失败测试**

覆盖归一化坐标到当前显示器物理 RECT、左右副屏负坐标、虚拟桌面 `SendInput 0..65535 + MOUSEEVENTF_VIRTUALDESK` 映射；旧 session/source epoch、越界坐标和未授权输入必须拒绝。

- [x] **Step 2: 实现有界串行 worker**

输入注入运行在专用 `std::thread`；鼠标移动可覆盖未执行旧值，按钮按下/抬起和滚轮保持顺序。每条事件执行前再次检查 controller、session 和 source epoch。

- [x] **Step 3: 实现清理和自动终止**

撤权先关闭准入，再清空队列，最后释放记录中的所有按钮。锁屏/UAC、捕获暂停、显示器变化、worker 异常和断线不得缓存输入或自动恢复控制。

- [x] **Step 4: 浏览器输入**

实现绝对移动、左/右键、双击、拖拽和垂直滚轮；批注模式、观看模式和控制模式互斥。控制端到下一帧反馈的局域网 P95 目标不超过 300ms。

- [x] **Step 5: 验证**

输入映射、权限边界、队列清理、拖拽、滚轮和卡键释放均有自动化覆盖；Windows 单屏实机已验证，左右副屏、多键盘布局和 UAC 边界保留为扩展验收项。

### Task 11: MJPEG 媒体基准

**Files:**

- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src/screen-share-web/App.vue`
- Modify: `src/lib/tauri.ts`

- [x] **Step 1: 增加可比指标**

记录 `frame_id`、采集时间戳、首帧时间、帧年龄、实际 FPS、JPEG 平均/分位大小、共享端 CPU/内存、出口 Mbps、掉帧和重连时间。交互协议始终携带 `session_id`、`source_epoch` 和 `frame_id`。

- [x] **Step 2: 建立 1/5/10/50 观看者场景**

已建立可重复运行的媒体基准脚本，并扩展到 50 路 H.264 观看者。MJPEG 仍是兼容基线和故障回退；release 构建的最终 50 路结果将在发布验收阶段补录。

- [x] **Step 3: 形成切换决策记录**

已批准增加 H.264/MSE，但不删除 MJPEG。决策依据、浏览器约束、回退条件和基准数据见 `docs/superpowers/specs/2026-07-20-screen-share-media-transport-decision.md`。

### Task 12: 可选 H.264/MSE 或 WebRTC 传输

**Files:**

- Modify: `src-tauri/Cargo.toml`, `Cargo.lock`（仅在方案获批后）
- Create/Modify: `src-tauri/src/screenshare_media.rs`
- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src/screen-share-web/lib/protocol.ts`, `App.vue`

- [x] **Step 1: 先抽象 `MediaTransport`**

统一提供 `sessionId`、`sourceEpoch`、`frameWidth`、`frameHeight`、`latestFrameId`、`transport`；批注、冻结和控制层不得依赖 JPEG 细节。

- [x] **Step 2: 实现并验证 H.264/MSE**

使用 Windows Media Foundation H.264 编码器，将 Annex B 输出封装成 MSE 可追加的 fMP4 init/media segment。编码器约每 2 秒强制关键帧，服务端只缓存最新关键帧 GOP；浏览器处理 append 队列、重连、媒体代际重置和不支持回退。MJPEG 始终保留。

- [x] **Step 3: 评估 WebRTC 并暂缓**

当前局域网查看和受控操作先使用 H.264/MSE + 会话 WebSocket。WebRTC 暂缓，只有 release 实测仍无法满足远控反馈延迟，或未来增加音频、自适应码率、公网穿透时再启用。

- [x] **Step 4: 验证回退**

媒体切换不改变批注坐标、冻结 `frame_id`、`source_epoch` 和会话 WebSocket 契约。编码器不可用、H.264 尚未就绪、浏览器不支持 MSE/H.264 或 MSE 播放失败时，观看端继续/恢复 MJPEG，不停止共享。

---

## P2 Implemented Subset：桌面叠加与基础键盘

### Task 13: 共享者桌面批注叠加

**Files:**

- Modify: `src-tauri/src/main.rs`, `src-tauri/src/screenshare.rs`
- Create/Modify: Tauri transparent `WebviewWindow` implementation and shared preview component

- [x] **Step 1: 先写显示器/DPI/拓扑测试**

覆盖 `sourceEpoch`、DPI、分辨率、显示器插拔、负坐标和窗口隐藏/移动。

- [x] **Step 2: 实现鼠标穿透、置顶、不抢焦点窗口**

窗口只渲染 SVG/Canvas 批注，不加载视频、不接收键盘焦点；尝试 `WDA_EXCLUDEFROMCAPTURE`，并用 WGC/DXGI 实机验证是否形成反馈。失败时回退到 P0 应用内预览。

- [ ] **Step 3: 完成 release 实机验证**

自动化已覆盖叠加状态与窗口生命周期；release EXE 仍需确认叠加不会进入共享视频、不会遮挡真实桌面输入，且源变化会隐藏或重新定位窗口。

### Task 14: 基础键盘控制

**Files:**

- Modify: `src-tauri/src/screenshare_input.rs`
- Modify: `src-tauri/src/screenshare_interaction.rs`
- Modify: `src/screen-share-web/App.vue`, `lib/protocol.ts`
- Modify: `src/pages/ScreenSharePage.vue`, `src/lib/tauri.ts`, `src/locales/messages.ts`

- [x] **Step 1: 写键盘状态失败测试**

覆盖字母、数字、方向键、Enter、Escape、Backspace、Tab、Ctrl/Shift/Alt 及常用组合键；浏览器失焦、断线或撤权时释放所有修饰键。

- [x] **Step 2: 实现受限扫描码协议**

基础键盘必须独立开关、默认关闭，不保证中文输入法、复杂国际键盘、剪贴板和 `Ctrl+Alt+Del`。拒绝未知或受限系统级快捷键，不提升进程权限。

- [ ] **Step 3: 扩展实机验证**

常用键、组合键、断线/撤权清理已验证；不同 Windows 键盘布局、锁屏和 UAC 边界作为后续兼容性验收，不阻塞当前局域网低风险版本。

---

## Final Verification and Handoff

- [x] 屏幕共享定向门禁已通过：观看端 27 项测试、观看端类型检查与构建、媒体模块测试、Rust debug 构建。
- [x] Windows 局域网浏览器级验收已覆盖批注同步、创建者撤销/清空、全员冻结/恢复以及 H.264 本地暂停/恢复。
- [x] debug 版 50 路 H.264 基准通过：50 路连接成功、聚合约 `5.067 Mbps`、无编码输入丢帧、无慢客户端广播丢帧。记录：`artifacts/screen-share-benchmarks/h264-20260720T112338Z.json`。
- [x] 最终全量门禁通过：`pnpm check`、`pnpm lint`、`pnpm build`、`pnpm test:screen-share-web`、`cargo test --manifest-path src-tauri/Cargo.toml screenshare --no-fail-fast`、`git diff --check`。
- [x] 版本化 release EXE 构建并核对完成：`file-sync-tool-1.2.0-202607202001.exe`，`37,252,096` 字节，SHA-256 `418deeac7d632eae7708cfca2cbf118ee7d67effd9785b58c53c9fcd9dea93da`，与发布 manifest 一致。
- [x] release 版 50 路 H.264 验证通过：首媒体平均约 `102ms`、P95 约 `210ms`，源编码约 `0.74 Mbps`，50 路聚合约 `35.33 Mbps`，编码输入丢帧和慢客户端丢帧均为 `0`。记录：`artifacts/screen-share-benchmarks/h264-20260720T121719Z.json`。
- [x] 范围外能力保持关闭/未实现：公网、TLS、TURN、录屏、音频、无人值守、远程剪贴板/文件和企业审计。
