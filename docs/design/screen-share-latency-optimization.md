# 屏幕共享低延迟与远程控制流畅度优化实施方案

> 状态：**已评审 · 实施中**
>
> 适用仓库：File Sync Tool（Tauri 2 + Vue 3 + Rust）
>
> 修订日期：2026-07-28
> 依据：当前源码、`screen-share-latency-optimization-review.md` 及原方案作者对审查意见的复核

---

## 0. 文档定位

本文是屏幕共享优化的实施规格，而不是最终性能结论。原方案中所有绝对耗时均来自静态推算，尚无目标机型 profiler 或端到端真机测量，因此本版不再把“30–45 ms”“80–130 ms”等数字当作版本承诺。

本文使用以下标记：

- **[代码事实]**：已在 2026-07-28 的仓库源码中核实；行号可能随后续实施发生漂移，以符号名和 Git 历史为准。
- **[设计约束]**：实施时必须保持的正确性或兼容性要求。
- **[候选阈值]**：用于启动实验的初值，必须由 M0 数据校准。
- **[待实测]**：不能仅凭代码或规范得出结论。

优化目标分为三个相互独立但最终汇合的指标：

1. `capture-to-display`：共享端捕获时间到观看端实际呈现时间。
2. `input-to-SendInput`：观看端产生输入到共享端完成 Windows 输入注入。
3. `input-to-visible-response`：观看端产生输入到该输入造成的画面变化重新显示在观看端。

第三项不是“单向视频延迟 × 2”，必须单独测量。

### 0.1 本次评审已经纠正的事实

以下结论已经由双方复核，不得在后续实现或说明中恢复为原表述：

| ID | 原判断 | 纠正后的结论 | 代码或规范依据 |
|---|---|---|---|
| J3 | N 个单帧轮询观看端会触发 N 倍 JPEG 编码 | `/stream?single=1` 返回 `latest_frame_bytes()` 中的缓存 JPEG；真实成本是 N 倍 HTTP 请求、响应和带宽，不是 N 倍编码 | `src-tauri/src/screenshare.rs:4713-4741` |
| F3 | 慢 receiver 阻止 Tokio broadcast 队列回收 | broadcast 是固定容量覆盖式环形缓冲；慢 receiver 会收到 `Lagged`。无超时 send 的真实问题是客户端 task、`ViewerGuard`、连接和状态不能及时释放 | `src-tauri/src/screenshare.rs:4966` 附近 |
| V2 | MSE 保留 20 秒历史直接造成 20 秒直播延迟 | 历史缓冲主要增加内存、`remove` 开销和可 seek 范围；直播延迟由 `buffered.end - currentTime` 决定 | `src/screen-share-web/lib/mse-player.ts:301-315` |
| U3 | 本地暂停会阻断 MSE 追直播边缘 | 本地暂停只截取并覆盖显示画面，没有停止隐藏的 `MseH264Player`；删除它是产品决策，不是播放优化技术前置 | `src/screen-share-web/App.vue:400-443` |
| WEBRTC-SEC | `RTCPeerConnection` 规范要求 SecureContext | WebRTC WebIDL 的 `RTCPeerConnection` 没有 `[SecureContext]`；通常要求 HTTPS 的是 `getUserMedia` 等本地采集 API。本项目观看端不采集本地设备。2026-07-28 已在本机 Chrome 普通页面及 Chrome/Edge headless profile 的 LAN 明文 HTTP 页面验证可创建；受管策略和真实媒体仍待现场复验 | [WebRTC 规范](https://www.w3.org/TR/webrtc/)、§7.3 实测 |

### 0.2 产品功能与性能改造解耦

产品已经决定并完成删除：

- 刷新频率选择器；
- 共享冻结；
- 本地暂停。

这些删除作为独立产品工作流实施，不再命名为性能阶段 P0，也不阻塞 M0–M4：

- 共享冻结依赖服务端 JPEG bytes 和 `/snapshot/:frame_id`，删除后会简化 JPEG 惰性化。
- 刷新频率选择器删除后，单帧轮询请求和带宽随之消失，但不会消除所谓“N 倍 JPEG 编码”，因为该编码本来就不存在。
- 本地暂停可独立删除；它与 MSE 追边缘没有实现冲突。
- 删除范围已经由路由与交互测试固化；不得从兼容代码重新引入旧 snapshot/single-frame/freeze/pause 行为。

MJPEG 保留为惰性最后回退。服务端统一 fallback 上限已经设为 10 FPS，避免 H.264 硬件和软件路径同时失败时观看端进入全速 MJPEG；最终值仍可由 M0 数据校准。

### 0.3 2026-07-28 实施快照

本节只描述代码和自动化验证状态，不代表目标硬件已经达到延迟指标；逐项证据见 `screen-share-latency-optimization-progress.md`。

- **M0（代码完成，现场基线待采集）**：服务端已记录捕获、GPU/JPEG/H.264、扇出、IDR 和输入队列分布；所有滚动分布同时报告累计样本数、保留样本数、容量和测量范围。`/time`、`media.trace`、浏览器单调时钟映射、30 秒周期校准、时钟跳变换代、呈现帧关联及基准脚本已经实现。`input-to-visible-response` 是“成功 `SendInput` 后、捕获时间不早于注入时间的首个实际呈现帧”代理，必须在受控画面变化场景使用，不能证明任意场景的像素因果关系。
- **M1（代码完成，30 客户端真机门禁待执行）**：JPEG 已按 MJPEG consumer 惰性启停并限制为 10 FPS；旧 snapshot/single-frame 端点及刷新/冻结/本地暂停 UI 已删除；TCP、媒体与交互发送有超时和连接上限；GOP cache、IDR 合并/限速、MSE 稳态、交互 reader/writer 分离及关键输入保护已经落地。
- **M2（代码完成，三代 Intel 矩阵待执行）**：硬件 MFT 枚举、基于 `BeginGetEvent/EndGetEvent` 的异步事件 adapter、`ICodecAPI` 能力协商、B=0/时间线/独立解码自检、动态 GPU surface 自检、软件 SIMD 与 1080p30 降级已经实现。状态中保留最多 16 条结构化候选报告并另报总尝试数，失败候选不再只剩摘要。本机 system-memory Media Foundation 自检通过不等于 Broadwell、Skylake、10 代 Intel 驱动均已通过。
- **M3（代码完成，跨 GPU/锁屏/长稳待执行）**：WGC 已使用同一 D3D11 device/context 完成 VideoProcessor BGRA→NV12、三槽 surface 池、DXGI manager 和硬件 MFT surface input；VideoProcessor 与 MFT 媒体类型显式统一为全范围 RGB 输入、BT.601 矩阵、16–235 NV12 输出，匹配 CPU fallback。正常 H.264 路径避免每帧全量 CPU readback，并保留小尺寸低频黑帧探针及 CPU/MJPEG 回退。`MFT_ENUM_ADAPTER_LUID` 现按规范的 8 字节 blob 解析，只有候选 LUID 与输入 D3D11 adapter 精确一致时才设置 DXGI device manager；缺失/不匹配直接拒绝而不把 surface 交给错误设备。2026-07-28 本机 GTX 1660 的 NVIDIA H.264 Encoder MFT 未暴露有效 adapter LUID，因而在 `adapter_match` 阶段被拒绝并按设计回到 CPU/SIMD；此前强制直连时首个 `ProcessInput` 后没有 `NeedInput/HaveOutput`，验证了该保护的必要性，但不能计作 NVIDIA 零拷贝通过。DXGI Desktop Duplication 仍按设计走 CPU/SIMD 路径。
- **M4（原型完成，尚未选型）**：WebCodecs/WSS 已定义严格的完整 AVCC AU 协议并实现客户端解码/最新帧呈现；receive-only WebRTC 以可选 Cargo feature 实现 host candidate、单份 H.264 复用、NACK/PLI/FIR、发送端 TWCC feedback 接收、Absolute Capture Time 和跨丢帧 RTP 时间轴推进。两者均复用全局 viewer/IDR 保护；WebRTC 浏览器 `captureTime`/`receiveTime` 在目标浏览器与 capture sequence 完成关联验证前仍只作为浏览器代理指标。仍须完成受管浏览器/策略、证书运营、真实媒体网络损伤及 30 独立设备同条件对比，现阶段不替换默认 MSE。

没有把 append queue 简单从 180 降到固定小帧数；当前实现同时按预计时长和字节数约束。

60 FPS 已经按 §6.2 作为**实验档**提供：界面新增独立的“60 FPS（实验）”开关，常规滑块仍是 5–30 FPS，服务端接受 1–60 FPS，采集节拍改用微秒避免整数除法把 30/60 FPS 抬成 30.3/62.5 FPS。默认值没有改变，也没有自适应策略；该开关的唯一用途是让 §3.3 的 60 FPS 实验组和 §6.3 的双档对比数据可以被采集。软件编码回退仍最高降级到 1080p30。

现场证据侧新增结构化门禁：`performance_matrix`、`independent_viewing_devices`、`managed_browser_external_media`、`network_impairment_recovery`、`transport_selection` 和 `feature_regression` 六类报告不再只检查 `status`，而是按 §3.3/§4.6/§7.4/§8.2/§8.3 校验字段、覆盖范围和阈值，详见 `screen-share-field-evidence-gates.md`。

---

## 1. 评审基线、当前实现与已确认瓶颈

### 1.1 评审时基线数据流

以下是本轮改造前用于定位瓶颈的数据流，不是当前代码状态：

```text
WGC（主）/ DXGI Desktop Duplication（回退）
  -> CPU BGRA frame
  -> H264InputFrame（复制）
  -> 标量 BGRA -> NV12
  -> Microsoft 软件 H.264 MFT
  -> Annex-B 解析 -> fMP4 fragment
  -> H264MediaState cache + Tokio broadcast
  -> 每观看端 /media/ws task
  -> MSE SourceBuffer -> <video>

同一捕获帧还会：
  -> BGRA -> RGB -> JPEG
  -> MJPEG broadcast / latest_frame_bytes / snapshot 兼容路径

远控：
  pointer/key event -> /session/ws -> input queue -> SendInput
  -> 捕获到画面变化 -> 上述视频返回路径
```

### 1.2 评审时基线代码证据

下表保留审查结论及历史依据。其目的在于解释为什么实施对应改造；“当前”字样均指评审基线，不能用来判断最新工作树：

| 领域 | 已确认事实 | 当前源码位置 |
|---|---|---|
| JPEG | H.264 提交后仍无条件执行 JPEG；路径包含逐像素 BGRA→RGB、JPEG 编码和 `jpeg_buf.clone()` | `screenshare.rs:3083-3143`、`:4463-4497` |
| 指标耦合 | FPS、`record_encoded_frame`、最新帧元数据当前在 JPEG 成功后更新；直接删除 JPEG 会让状态停更 | `screenshare.rs:3132-3143`、`:1420-1438`、`:5477-5489` |
| 捕获回读 | WGC 每帧 `CopyResource` 到 staging 后 `Map(READ)` 并复制到 CPU | `screenshare.rs:3981-3996` |
| DXGI 光标 | DXGI fallback 通过 CPU scratch 和 alpha composite 合成光标 | `screenshare.rs:2965-2983` |
| 编码输入 | `try_submit` 在确认队列接受前执行 `to_vec()`，1080p BGRA 每帧约 8 MiB | `screenshare_media.rs:274-290` |
| 色彩转换 | `bgra_to_nv12` 是标量逐像素循环 | `screenshare_media.rs:554-605` |
| 编码器 | 当前直接实例化 `CLSID_MSH264EncoderMFT`，未枚举硬件 MFT | `screenshare_media.rs:859-867` |
| fMP4 时间线 | `trun` 没有 sample composition time offset，当前封装假设 DTS = PTS | `screenshare_media.rs:466-518` |
| GOP cache | 超限时保留首个 IDR、删除其后的早期 P 帧；长 GOP 配小缓存会留下断裂参考链 | `screenshare_media.rs:187-220` |
| 新接入/落后 | 初始连接和 `Lagged` 都调用 `send_h264_snapshot` 重放缓存 | `screenshare.rs:4928-5005`、`:5026-5043` |
| 每客户端发送 | 每个分段执行 `to_vec()`，之后人为 sleep 1 ms | `screenshare.rs:4964-4971` |
| MSE 追边缘 | live-edge 距离超过 250 ms 时直接硬 seek 到边缘前 50 ms | `mse-player.ts:26-41`、`:289-299` |
| 远控前端 | pointer move 已按 `requestAnimationFrame` 合并，但 `send` 不检查 `WebSocket.bufferedAmount` | `RemoteControlOverlay.vue:18-55`、`session-client.ts:136-151` |
| 远控后端 | 相邻 move 已合并且不跨 button/wheel/key 边界；输入队列容量 128，满时撤销控制 | `screenshare_input.rs:383-412`、`screenshare.rs:5336-5357` |
| 交互 socket | 一个 `tokio::select!` task 串行负责 inbound 和 outbound，慢 send 会暂停 input receive | `screenshare.rs:5268-5284` |

### 1.3 当前实现增量

```text
WGC -> 同一 D3D11 device/context -> VideoProcessor -> 三槽 NV12 surface
    -> IMFDXGIDeviceManager -> 经动态 surface 自检的异步硬件 H.264 MFT
    -> 完整 AU + fMP4（默认 MSE）
                    + AVCC AU（WebCodecs 原型）
                    + Annex-B/RTP（可选 WebRTC 原型）

DXGI Desktop Duplication -> CPU BGRA -> SIMD NV12 -> 软件/可用硬件编码回退
MJPEG consumer > 0      -> CPU readback/JPEG，统一限制 10 FPS
无 MJPEG consumer       -> WGC 正常 H.264 路径不做全帧 CPU readback/JPEG

远控：browser priority queue -> split /session/ws reader+writer
    -> bounded input queue -> SendInput 成功 ACK
    -> 下一幅 capture-time >= applied-time 的呈现帧关联（受控场景代理）
```

当前增量的主要源码入口为 `screenshare_gpu.rs`、`screenshare_media.rs`、`screenshare.rs`、`screenshare_input.rs`、`screenshare_interaction.rs`、`mse-player.ts`、`webcodecs-player.ts` 和 `webrtc-player.ts`。精确行号随实施持续变化，以符号和 Git diff 为准。

### 1.4 原方案中仍然成立的主要方向

- JPEG 正常路径惰性化；
- 删除 H.264 每分段的 1 ms 人工 sleep；
- 用 `MFTEnumEx` 发现硬件编码器，并保留软件/MJPEG 回退；
- 为软件路径增加 SIMD BGRA→NV12；
- 为观看端发送增加超时并隔离慢客户端；
- MSE 起播和严重失步才 hard seek，稳态使用小幅 `playbackRate` 漂移校正；
- WGC 方向采用共享 D3D11 device、GPU BGRA→NV12 和 surface input；
- 在传输层比较 WebCodecs/WSS 与 receive-only WebRTC，而不是预先锁定其中一个。

---

## 2. 跨阶段正确性约束

以下约束优先级高于单项性能收益。

### 2.1 缓存、关键帧和恢复语义

禁止实施“超长/无限 GOP + 个位数任意尾缓存”。当前淘汰算法会保留 IDR，却删除 IDR 后最早的参考 P 帧，形成不可解码快照。

缓存必须采用以下两种合法模型之一：

1. **受控新 IDR 模型（M1 首选）**
   - 单独保存当前 codec generation 的 init/config。
   - 新观看端先订阅 live stream，再进入 `waiting_for_keyframe`。
   - 通过全局 IDR gate 请求新 IDR；收到该 IDR 前丢弃 delta。
   - IDR 到达后，从该 AU 开始连续转发。
   - `Lagged` 初版直接断开并快速重连，避免慢客户端触发全局恢复流量。

2. **连续依赖链模型（仅在起播数据证明必要时）**
   - 原子保存 `init + [IDR, delta...]`，序列必须从 IDR 开始且无缺口。
   - 淘汰不能删除链中间的参考帧；容量不足时整条链失效并等待下一 IDR。
   - snapshot 的结束 sequence 与随后 live subscription 必须去重且无间隙。

共同约束：

- codec/profile/resolution 改变必须递增 `generation`，旧 generation 的 init、IDR、delta 不得混发。
- IDR 请求需全局合并和限速。候选值：200 ms 合并窗口、500 ms 最小间隔；最终值由 M0/M1 压测确定。
- 新接入合并和异常客户端限速是互补关系：前者减少并发接入的重复 IDR，后者防止 keyframe storm。
- 不承诺无限 GOP。最大恢复间隔、场景切换 IDR 和驱动行为需纳入自检与实测。

### 2.2 编码时间线约束

当前 fMP4 没有 composition time offset，因此所有继续使用现有 muxer 的编码路径必须满足：

- 显式尝试设置 `CODECAPI_AVEncMPVDefaultBPictureCount = 0`，并记录 `IsSupported`、`IsModifiable`、设置和回读结果；
- 若驱动不允许修改该属性，只能在 `GetOutputCurrentType` 明确回读为 H.264 Baseline profile，且启动自检确认没有 B-slice 或输出重排时继续；“属性不可写”本身既不能静默忽略，也不能直接淘汰所有 Baseline 编码器；
- 若 B=0 属性已确认，也仍须由自检确认没有 B 帧或输出重排；
- DTS 与 PTS 单调且相等。

如果既不能由 B=0 属性回读确认，也不能由已协商的 Baseline profile 与无 B-slice 自检共同保证，必须拒绝该编码器，或先为 mux/protocol 实现完整 DTS/PTS 与 composition offset；不得带病启用 High profile。

### 2.3 码率控制能力协商

不能硬编码 `LowDelayVBR + BufferSize + MaxBitRate` 并假设所有 MFT 支持。每个候选 MFT 必须通过 `ICodecAPI::IsSupported` / `IsModifiable` 建立能力矩阵：

1. 探测 rate-control modes；
2. 根据实际支持选择低延迟模式、CBR + 小 VBV，或 PeakConstrainedVBR + mean/max bitrate；
3. 单独探测 low-latency、reference frames、B-frame count、CABAC；
4. High profile 仅在兼容性和自检通过后启用；CABAC 必须显式探测并设置，不能假定 High 自动开启；
5. 记录 MFT 名称、HRESULT、请求值和最终生效值，失败属性不能静默忽略。

### 2.4 M2 硬编与 M3 GPU 管线边界

硬件 MFT 与 GPU surface input 存在强依赖，阶段划分遵循：

- M2 可以完成枚举、异步 MFT adapter、能力协商和 system-memory NV12 原型。
- 只有目标驱动明确接受 system-memory NV12 且自检/真机稳定时，M2 才可临时启用该硬编路径；要记录其可能发生隐藏 GPU 上传。
- 如果 MFT 要求 D3D manager、system-memory 路径不稳定或收益不成立，则生产启用推迟到 M3。
- M3 中 WGC、VideoProcessor、`IMFDXGIDeviceManager` 和硬件 MFT 必须共享同一个 D3D11 device，并明确 context 线程模型与 texture 生命周期。

### 2.5 WGC 与 DXGI 范围

- M3 第一阶段只承诺 **WGC 零拷贝**。
- 当前 `scrap::Frame` 暴露的是 CPU 像素，DXGI fallback 继续走 CPU/SIMD 路径。
- 若以后要求 DXGI 零拷贝，需要自有 Desktop Duplication 后端暴露 `ID3D11Texture2D`，并重新设计 GPU 光标合成；不包含在本轮 M3。
- 黑屏看门狗必须保留。WGC GPU 路径采用低频、小尺寸异步回读，不得为检测重新引入每帧全量 readback。

### 2.6 指标与 JPEG 解耦

停止 JPEG 前必须先拆分：

- `capture_frame_count / capture_fps / latest_capture_metadata`；
- `h264_encoded_frame_count / encoded_fps / encoded_bytes`；
- `mjpeg_encoded_frame_count / jpeg_bytes`；
- `outbound_bytes_total / outbound_bitrate`；
- 可选的 `latest_mjpeg_frame`，只在 MJPEG 活跃时存在。

`latest_capture_metadata` 只保存 frame id、width、height、capture timestamp，不依赖 JPEG bytes。产品删除已完成：`/snapshot/:frame_id` 和 `/stream?single=1` 均已移除，`/stream` 只提供惰性 MJPEG fallback；未知 query 会被拒绝，不能再依赖旧单帧兼容行为。

### 2.7 远控输入链路约束

- 保留现有 RAF move 合并和服务端“只合并相邻 move、不跨关键输入边界”的设计。
- pointer move 在浏览器发送缓冲超过阈值时可以丢弃旧值、只保留最新值。
- button、wheel、key、`release_all` 不得静默丢弃；关键输入无法及时发送时应主动结束控制状态，避免粘键。
- `/session/ws` reader 与 writer 分离：reader 持续处理输入，writer 使用有界队列和发送超时。
- 状态快照可以合并；授权撤销、`release_all` 和错误消息不可合并丢失。

---

## 3. M0：度量与基线

M0 是所有结构性优化的门禁。没有基线数据，不得以推算数字宣称某项优化已降低端到端延迟。

### 3.1 服务端埋点

使用 WGC `Direct3D11CaptureFrame.SystemRelativeTime` 记录真实捕获时间；CPU 回读结束后的 `Instant::now()` 只能代表处理时间，不能冒充 capture timestamp。

需要记录 count、bytes、P50、P95、P99 和 max：

- frame wait、capture queue age；
- WGC/DXGI GPU readback；
- black-frame classification；
- JPEG BGRA→RGB、JPEG encode；
- H.264 input queue age、BGRA→NV12、MFT encode、mux；
- IDR size、IDR request/coalesce/rate-limit 次数；
- per-client send wait、timeout、disconnect、`Lagged`；
- outbound bytes，按 100 ms 和 1 s 窗口汇总；
- input receive→enqueue、queue depth、move coalesced、queue full、dequeue→`SendInput`。

### 3.2 观看端埋点

- media WebSocket receive timestamp；
- 非人为断线到重连后画面重新呈现的恢复耗时，以及非人为断线次数；
- append queue 的预计时长和字节数；
- `SourceBuffer.appendBuffer` 耗时；
- `buffered.end - currentTime`；
- playbackRate、hard seek 次数；
- presented/dropped frames；
- `WebSocket.bufferedAmount` P50/P95/P99；
- pointer event→send，以及输入序列号的服务端确认时间。

浏览器时间必须使用 `performance.timeOrigin + performance.now()` 的单调 Unix 映射，不能在呈现回调中直接混用可跳变的 `Date.now()`。MSE/WebCodecs 至少每 30 秒重新采样 `/time`；offset 突变超过 `max(250 ms, RTT)` 时清空旧校准 epoch，并报告 offset range、末次 offset 和 discontinuity count。MSE/WebRTC 优先使用 `requestVideoFrameCallback.expectedDisplayTime`，回调执行时刻只能标记为 fallback；WebCodecs Canvas 路径必须明确标记为 `animation-frame-pre-paint-proxy`。

所有 P50/P95/P99 分布必须同时报告累计 `sample_count`、实际 `retained_sample_count`、窗口容量和 `measurement_scope`。滚动窗口的分位数不得让读者误以为覆盖了全部累计样本。

### 3.3 基线矩阵

至少覆盖：

- Broadwell、Skylake、10 代 Intel 各一台；
- 720p30、1080p30；60 FPS 仅作为实验组；
- 1、5、20、30 个健康观看端；
- 动态桌面、静态桌面、视频播放、快速滚动；
- WGC 主路径、DXGI fallback、RDP、Microsoft Basic Display Adapter/无可用硬编；
- 单个停止读取的媒体慢客户端和单个交互 writer 反压客户端；
- 批注、远控、光标、多显示器切换、隐私/黑屏恢复。

5 台真机 + 25 标签页可用于服务端扇出初测，但不能替代 20–30 台独立设备的最终验收。故障注入必须使用可稳定停止读取或限制接收窗口的专用慢客户端。

每台编码主机先运行 `pnpm benchmark:screen-share:qualification`，以同一流程保存环境、system-memory MF、GPU DXGI surface 和浏览器 capability 证据。该工具的 GPU 失败必须保留为失败矩阵项；`-CollectOnly` 只用于继续收集其余日志，不得改变 JSON 的 `qualification_status`。Vite 的 `dist/screen-share-web` 重建和 Cargo 的 `rust-embed` 编译不得并发，资格验证固定先构建 Web 产物再执行 Rust 门禁。

### 3.4 M0 完成条件

- 能分别报告三项端到端指标：capture-to-display、input-to-SendInput、input-to-visible-response。
- 能从 trace 关联同一 capture/input sequence 的客户端与服务端事件。
- 不把各阶段 P99 简单相加当作端到端 P99。
- 形成基线报告并据此校准 M1–M4 的候选阈值。

---

## 4. M1：低风险路径修复与扇出加固

### 4.1 指标解耦与 JPEG 惰性化

1. 先实现 §2.6 的指标和元数据拆分。
2. H.264 ready 且没有 MJPEG consumer 时停止 JPEG 编码。
3. MJPEG 仅在协商选择它或 H.264 全部失败时启动，并应用服务端统一 FPS 上限。
4. JPEG 启停、consumer 数和 fallback 原因进入状态日志。

### 4.2 媒体发送和连接回收

1. 使用 `axum::serve(...).tcp_nodelay(true)`；当前 axum 0.7 支持该设置。
2. 删除 `H264_STREAM_COOPERATIVE_DELAY` 的每分段 1 ms sleep。
3. 每次媒体/交互 WebSocket send 增加超时。初始测试窗口 500–1000 ms；最终值以 M0 数据为准，不默认等待 3–5 秒。
4. 超时后关闭单个客户端，确保 viewer count、IP 状态、task 和 socket 在限定时间内回收。
5. 增加观看端软上限，默认候选 40；超限返回明确原因。

当前 axum 0.7 的 `Message::Binary` 持有 `Vec<u8>`，因此本阶段不承诺“删除 `.to_vec()` 即实现 `Bytes` 零拷贝”。先测量复制占比；若值得优化，再独立评估升级 axum/tungstenite 或下沉 WebSocket 层。

### 4.3 GOP cache 与恢复

1. 按 §2.1 实现受控新 IDR 模型。
2. 新接入先订阅，再等待属于当前 generation 的新 IDR。
3. `Lagged` 初版断开并触发客户端快速重连，不重放 GOP，不因单个异常客户端立即强制全局 IDR。
4. 并发新接入共享合并后的 IDR；异常重连受最小间隔限制。
5. 单元测试覆盖：generation 切换、IDR 前 delta 丢弃、并发接入合并、连续链不可截断、snapshot/live 去重。

### 4.4 MSE 稳态控制

1. 起播允许一次定位到 live edge。
2. 稳态小漂移采用分档 `playbackRate`，候选范围 1.00–1.05。
3. 只有严重失步或 append discontinuity 才 hard seek，候选阈值 1 秒。
4. append queue 按预计时长和字节限制，不再按固定 180 个小 fragment 才判断过载。
5. 缩短历史缓冲只作为内存和 `remove` 开销优化，不宣称直接降低 live-edge distance。

### 4.5 远控输入保护

1. session client 采集 `bufferedAmount`；pointer move 超阈值时只保留最新待发位置。
2. 关键输入发送失败或超过最大陈旧时间时退出控制状态，并尽力发送 `release_all`。
3. 服务端 split 交互 WebSocket reader/writer；writer 使用有界优先级队列和发送超时。
4. 保持 reader 对 inbound input 的轮询不受慢 outbound send 阻塞。

### 4.6 M1 量化验收

在 M0 基线矩阵上至少满足：

| 指标 | 目标 |
|---|---|
| 健康 30 客户端、30 分钟 | 健康客户端 `Lagged = 0`；无持续增长的 task/连接状态 |
| 慢媒体客户端 | 2 秒内隔离；其他健康客户端 live-edge P99 相对基线劣化不超过 20% |
| 状态回收 | 断开后 3 秒内 viewer count、IP 和 task 状态一致 |
| IDR storm | 全局 IDR 请求满足合并窗口和最小间隔；单异常客户端不能绕过限速 |
| MSE 稳态 | 正常 30 分钟 hard seek 为 0；仅起播/故障恢复允许发生 |
| 远控注入 | 健康 LAN 下 receive→`SendInput` P95 ≤ 20 ms、P99 ≤ 50 ms；queue full 为 0 |
| 功能 | 批注、控制授权/撤销、粘键释放、光标、多显示器、黑屏恢复无回归 |

若 M0 显示候选阈值与设备能力冲突，应记录理由后调整，不得为了通过验收隐藏或丢弃指标。

---

## 5. M2：编码器能力协商与 CPU 回退

### 5.1 硬件 MFT 发现和 adapter

1. 使用 `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER)` 枚举硬件编码器。
2. 保留当前同步软件 MFT adapter，并为硬件 MFT 实现异步事件模型：`MF_TRANSFORM_ASYNC_UNLOCK`、单一挂起的 `BeginGetEvent/EndGetEvent` 回调、`METransformNeedInput`、`METransformHaveOutput`；不得使用 1 ms 忙轮询模拟事件循环。
3. 按 §2.3 探测并协商 CodecAPI 能力。
4. 显式 B=0；High/CABAC 只有探测和自检通过才启用。
5. 对每次 admission attempt 记录结构化报告：候选名称、硬件标记、adapter LUID、hardware URL、driver、输入尺寸/FPS、system-memory/GPU surface、激活/配置阶段、完整 capability、自检与失败原因。状态最多保留 16 条并另报总数；超出部分仍写入日志，不能只留下最终编码器。

### 5.2 编码器自检

每个候选编码器启动自检总超时 1–2 秒：

- 输入包含移动或变化图案，不使用全黑静帧；
- 成功得到 SPS、PPS、IDR；
- 时间戳单调，duration 合法；
- 无 B 帧/重排序；
- `BPictureCount=0` 回读成功，或当前输出类型确认为 Baseline profile；两者都不成立时，即使短样本暂未观察到 B-slice 也必须拒绝；
- 输出 access unit 能被独立 decoder 解码至少一帧；
- GPU surface 模式在 `MFCreateDXGISurfaceBuffer` 后设置有效 `CurrentLength`，初始化纹理在必要的设备同步后送入 MFT；异步 MFT 在失败/销毁时执行 `IMFShutdown`；
- 记录 adapter、driver、MFT friendly name、hardware URL 和最终生效参数。

任一关键条件失败即降级，顺序为：

```text
硬件 MFT（仅自检通过且输入模式稳定）
  -> Microsoft 软件 H.264 MFT + SIMD 转换 + 降分辨率/帧率
  -> 惰性 MJPEG + 服务端 fallback FPS 上限
```

### 5.3 软件/DXGI 回退

- 使用经过正确性测试的 SIMD BGRA→NV12 实现替代标量主路径，并保留标量参考实现做测试 oracle。
- 明确软件 fallback 的分辨率/帧率策略；不能只在文档中写“自动降级”而没有 scaler 和触发条件。
- DXGI、RDP、虚拟机和 Basic Display Adapter 必须能走该路径正常出图。

### 5.4 M2 完成条件

- 三代目标 Intel 设备均记录完整 capability/self-test 报告。
- 所有启用的编码器通过属性回读或 Baseline profile + 无 B-slice 自检确认 B=0，且时间线合法并能独立解码。
- 不支持的 CodecAPI 属性有明确日志且不会阻止回退。
- 软件/SIMD 与标量输出在容差内一致；不同 stride、奇偶尺寸边界有测试。
- M2 未完成 M3 时，不以 system-memory 硬编推断最终 GPU 零拷贝收益。

---

## 6. M3：WGC GPU 管线

### 6.1 实施范围

1. WGC、`ID3D11VideoProcessor`、`IMFDXGIDeviceManager` 和硬件 MFT 共享同一 D3D11 device。
2. 建立 2–3 块 NV12 texture pool，定义 acquire、GPU 使用完成和 recycle 状态。
3. 在 GPU 完成 BGRA→NV12 和可选缩放。
4. 用 `MFCreateDXGISurfaceBuffer` 将 surface 交给 MFT，并以 `GetMaxLength -> SetCurrentLength` 建立有效 DXGI media buffer；`MFT_ENUM_ADAPTER_LUID` 必须按 blob 读取且与输入 D3D11 adapter 的 `LUID` 精确匹配，缺失/不匹配时不得设置 device manager；纹理 bind flags、设备同步与 adapter 必须纳入候选自检。
5. 明确 WGC frame 关闭前的 texture 引用、capture/encoder 线程所有权及 D3D context 使用规则。
6. 黑屏检测改为 1–2 FPS、小尺寸、异步 CPU 回读。
7. DXGI 保持 CPU/SIMD fallback；本阶段不声称其零拷贝。
8. VideoProcessor stream/output color space 与 MFT 输入/输出媒体类型必须显式声明，并与 CPU fallback 使用同一矩阵和 nominal range；GPU→CPU 切换不得出现可见亮度/色偏跳变。

所有格式需先经 `CheckVideoProcessorFormat` 和 MFT input type 能力探测；不支持时可靠回退，不因优化破坏出图。

### 6.2 60 FPS 决策

60 FPS 不作为 M3 默认值或“近似免费”收益。它会增加 MFT 调度、分段、WebSocket 消息、MSE append、解码和合成频率。

在 WebCodecs/WebRTC 只保留最新帧的原型中，60 FPS 将最大采集等待从约 33 ms 降到约 16 ms，这一潜在收益明确；但系统能否稳定承受仍需 Broadwell 真机验证。最终提供 30/60 FPS 可选或自适应策略，由 M3/M4 数据决定默认值。

30/60 可选部分已经实现：界面提供独立的 60 FPS 实验开关（常规滑块保持 5–30 FPS），Rust 侧 `screen_share_start` 接受 1–60 FPS，`capture_frame_interval` 用微秒计算节拍，使两档对比数据不被整数除法污染。默认档位、是否自适应仍属 `decision-pending`，必须由 §6.3 的双档报告和 §7.4 的 `fps_default_decision` 证据决定，不得因为开关已存在就改默认值。

### 6.3 M3 完成条件

- WGC 正常编码路径没有每帧全分辨率 GPU→CPU 回读。
- texture pool 长时间运行无泄漏、复用前写入或跨 generation 污染。
- 黑屏/隐私模式检测和恢复行为与基线一致。
- WGC GPU 路径失败时自动回到 CPU/SIMD 路径并持续出图。
- Intel、NVIDIA、AMD 至少各一台目标设备的 GPU surface 自检和 30 分钟 WGC 长稳通过；单机候选被安全拒绝只证明回退正确，不能计作该厂商零拷贝验收通过。
- 30 FPS 与 60 FPS 分别报告 capture-to-display、CPU/GPU、掉帧和 30 客户端扇出数据，再决定默认档位。

---

## 7. M4：传输层原型对比与选型

M4 不预先承诺 WebCodecs 或 WebRTC。两条路线复用同一份 H.264 编码流，使用相同场景和指标对比。

### 7.1 WebCodecs over WSS 原型

#### 协议要求

WebCodecs 消息单位必须是**完整 H.264 access unit**，不是单个 NAL。当前 `parse_annex_b_access_unit` 已产生完整 `parsed.avcc`，并保留 sample time/duration（`screenshare_media.rs:713-788`）。

配置消息至少包含：

```text
version
generation
codec string
AVCDecoderConfigurationRecord (avcC)
width / height
color space（可用时）
```

每条媒体消息至少包含：

```text
version
generation
sequence
timestamp_us
duration_us
flags: key | delta | discontinuity
payload_length
payload: 一个完整 AVCC access unit
```

观看端构造：

```ts
new EncodedVideoChunk({
  type: flags.key ? 'key' : 'delta',
  timestamp: timestampUs,
  duration: durationUs,
  data: accessUnit,
})
```

观看端必须：

- generation 变化时 reset/reconfigure decoder；
- key AU 前不解码 delta；
- 基于 `decodeQueueSize` 和 sequence 丢弃陈旧 delta，只保留最新可恢复路径；
- output 后立即渲染并 `VideoFrame.close()`；
- 实测 Canvas、WebGL 或其他呈现路径在 Broadwell 上的复制、CPU/GPU 和延迟。

当前原型已经在 `/media/webcodecs/ws` 落地 40-byte `FSTW` v1 header、严格元数据校验、准确 avcC 配置、完整 AU、generation/gap/overload 恢复和客户端主动 IDR 请求。呈现端使用单 pending `requestAnimationFrame`：同一 paint 前被后帧覆盖的解码帧记为 dropped，只有最后一帧进入 presented/端到端 trace；所有 `VideoFrame` 均立即关闭。broadcast `Lagged` 恢复必须从 `H264MediaState::descriptor()` 重读权威 generation，不能盲目清空可能包含唯一 Reset 的保留事件。

规范依据：[WebCodecs AVC Codec Registration](https://www.w3.org/TR/webcodecs-avc-codec-registration/)。

#### 安全上下文与运营验证

WebCodecs `VideoDecoder` 要求安全上下文，WSS/证书方案必须验证证书点击穿透、证书轮换、DHCP/IP 变化和浏览器 profile 清理后的行为。不得把自签证书摩擦当作纯技术细节。

当前内置屏幕共享服务器不终止 TLS，因此该原型只有部署可信 HTTPS/WSS 终止层后才能真正启用；明文页面会显示具体不可用原因并回退 MJPEG。这是尚未完成的运营门禁，不是代码中已经解决的证书能力。

### 7.2 WebRTC receive-only 原型

目标是 LAN 内无本地媒体采集的 receive-only peer：

- 不配置 STUN/TURN，先验证 host candidates；
- 复用一份编码输出，避免每 `PeerConnection` 重复编码；
- 支持 NACK、PLI、jitter buffer 和拥塞反馈；
- 服务端作为 RTP sender 必须注册 TWCC sender interceptor，接收并统计 transport-wide feedback；仅注册默认 receiver interceptor 不会给发送端提供拥塞反馈；
- capture sequence 出现缺口时，不发送空 RTP payload，但 packetizer 的 90 kHz timestamp 必须推进缺失 duration，避免接收端时间轴被压缩；
- 注册 WebRTC 实验性 Absolute Capture Time header extension；只有最终 SDP answer 实际协商该 URI 时，才为每个真实 AU 写入服务端 capture wall-clock 的 64-bit UQ32.32 NTP mandatory field，并分别统计协商 offer 和发送样本；该格式以 [WebRTC `AbsoluteCaptureTime` 定义](https://webrtc.googlesource.com/src/+/refs/heads/main/api/rtp_headers.h) 和实现为准，不错误归因于 RFC 8872（后者讨论 RTP 多流复用）；
- 对多个 PLI 做全局 IDR 合并和限速；
- 测量每观看端 SRTP/发送状态成本和 30 人扇出。

项目现已实现可选 Cargo feature `screen-share-webrtc-prototype`：服务端提供 receive-only peer、host ICE、共享 H.264 AU、RTCP NACK/PLI/FIR/TWCC、实验性 Absolute Capture Time、跨丢帧 RTP 时间轴与生命周期/观看端上限保护；前端提供显式 `web_rtc` 传输、失败回退、连续 `requestVideoFrameCallback` 和扩展 `getStats()` 指标。默认构建未启用该 feature 时，选择 WebRTC 会返回明确错误。

Absolute Capture Time 不能仅凭代码注册就计为有效：服务端报告 `absolute_capture_time_offers_negotiated` / `absolute_capture_time_samples_sent`，客户端从最终 SDP answer 判断 URI 是否协商，并区分“未协商”“等待 `captureTime` 样本”“已有样本但待目标浏览器关联验证”。在目标 Chrome/Edge 尚未把该样本与同一服务端 capture sequence 交叉验证前，`endToEndLatency.available` 必须保持 `false`；`captureTime`/`receiveTime` 分布仍单列为 browser timing proxy。它仍是待同条件实测的原型，不能把“代码存在”视为已经完成传输选型。

### 7.3 目标浏览器三项验证

在目标 Chrome/Edge 的 `http://192.168.x.x` 页面实测：

```js
window.isSecureContext
new RTCPeerConnection()
typeof VideoDecoder
```

2026-07-28 已在本项目开发机的 Chrome、物理局域网地址 `http://192.168.0.111:8765` 上通过页面脚本实测：

| 项目 | 结果 |
|---|---|
| `window.isSecureContext` | `false` |
| `new RTCPeerConnection()` | 成功，不抛异常 |
| `typeof VideoDecoder` | `"undefined"` |
| `typeof MediaSource`（附加对照） | `"function"` |

同日又使用 `scripts/screen-share-browser-probe.mjs` 让本机 Chrome 150 与 Edge 150 的独立 headless profile 访问同一物理 LAN IP 的临时明文 HTTP 页面，两者均得到 `isSecureContext=false`、`RTCPeerConnection` 构造成功、`VideoDecoder=undefined`、`MediaSource=function`；原始产物为 `artifacts/screen-share-benchmarks/browser-capabilities-codex-2026-07-28.json`。该自动 probe 只证明构造能力，不证明 ICE/DTLS/RTP、媒体呈现或受管策略。

因此，当前本机 Chrome 与 Edge 已确认 receive-only WebRTC 可以在无证书 LAN 页面原型化，而 WebCodecs 路线需要 HTTPS/WSS 才能使用 `VideoDecoder`。这足以把 **WebRTC 调整为 M4 优先原型**，但不是直接选型：仍须在受管 Edge、现场策略/网段和真实媒体连接复验，并完成 §7.4 的同条件性能比较。

### 7.4 M4 选型门禁

在 Broadwell、Skylake、10 代 Intel 和 30 客户端场景比较：

- capture-to-display P50/P95/P99；
- input-to-visible-response P50/P95/P99；
- 丢包/抖动注入后的恢复时间和画面连续性；
- CPU/GPU、outbound bitrate、每客户端内存；
- 30 客户端加入/离开造成的 IDR 数和延迟尖峰；
- 浏览器支持、证书/网络策略和现场运维成本。

只有数据明显优于 MSE 且运维可接受时才替换现有传输。若两者收益不足，保留 MSE 是合法结论。

---

## 8. 全局验收和发布门禁

### 8.1 健康与故障客户端分开验收

“`record_lagged_frames` 恒为 0”只适用于健康稳态。故障注入阶段的目标是隔离，而不是假装没有 lag：

- 健康客户端 lag 为 0；
- 慢客户端在限定时间内隔离；
- 慢客户端不会显著推高其他客户端 live-edge P99；
- viewer count、task、IP 和控制状态及时回收；
- IDR 请求受全局合并和限速保护。

### 8.2 每阶段必须报告的量化结果

- capture-to-display P50/P95/P99；
- input-to-SendInput P50/P95/P99；
- input-to-visible-response P50/P95/P99；
- live-edge distance P50/P95/P99；
- 100 ms / 1 s outbound bitrate P50/P95/P99；
- 单 IDR 大小及 fan-out send P95/P99；
- dropped/presented frame ratio；
- input queue age、depth、coalesced 和 full 次数；
- 断线重连恢复 P50/P95/P99；
- 共享端和观看端 CPU/GPU/内存。

每个分布必须附带累计样本数、保留样本数、窗口容量和测量范围；每个呈现延迟必须附带 `presentationTraceSource`。WebRTC browser capture/receive proxy 与权威服务端 capture trace 分栏，不得混合汇总。

断线重连恢复由播放器的 `reconnectRecoveryMs` / `unexpectedDisconnectCount` 提供：MSE 只把 `mediaTime` 超过断线时刻缓冲末端的呈现帧算作恢复，避免断线前已缓冲的画面把恢复时间压成 0；WebCodecs 无播放缓冲，重连后第一帧绘制即结算；主动停止与终止性失败不产生样本。共享端进程的 CPU/GPU/内存由 `pnpm benchmark:screen-share:resource-sample` 采样，观看端资源仍需在观看设备上测量。

上述结构由 `pnpm benchmark:screen-share:field-evidence` 与完整审计共同强制校验：缺少样本计数、容量、`measurement_scope` 或 `presentationTraceSource` 的百分位不算证据，`retained_sample_count` 超过容量或累计样本数会直接判为不完整。矩阵覆盖（三代 Intel、四种捕获后端、四类场景、720p30/1080p30、30/60 FPS、1/5/20/30 客户端）同样是门禁条件，字段清单见 `screen-share-field-evidence-gates.md`。

### 8.3 回归门禁

- 批注、控制申请/授权/撤销、键鼠释放、光标、多显示器切换、隐私/黑屏恢复行为不回归；
- WGC、DXGI、RDP、软件编码和 MJPEG fallback 均有明确测试结果；
- 中英文用户文案同步；
- `pnpm check`、`pnpm lint`、`pnpm test:screen-share-web` 通过；
- Rust 定向测试和 `cargo check` 通过；
- `git diff --check` 通过；
- 上述回归项以 `feature-regression` 报告的形式进入完整审计，每项需 `tested`/`passed` 显式为真，中英文文案同步以 `localization.zh_cn`/`localization.en_us` 声明；
- 目标 Intel 真机数据完成前，不发布“已达到某毫秒延迟”的结论。

---

## 9. 实施顺序与依赖

```text
产品功能删除 ───────────────────────────────┐
                                             ├─ 已完成旧协议/UI 清理
M0 埋点代码 ─> M1 修复 ─> M2 编码器 ─> M3 WGC GPU ─> M4 双原型（代码均已落地）
    │            │                                  │
    └─ 真机基线  └─ 远控输入保护                    └─ 同条件外部数据后才能选型
```

- M0 仍是性能结论的证据门禁；为并行开发而提前落地后续代码，不得被解读为跳过基线即可宣称收益。
- 产品功能删除已经完成；M1 的 JPEG 惰性化不再承担 snapshot/single-frame 兼容责任，仅保留 `/stream` MJPEG fallback。
- M2 和 M3 可以分别开发 adapter 与 GPU 管线，但生产硬编启用必须遵守 §2.4。
- M4 在 M3 后做最终对比，避免把捕获/编码瓶颈误归因于传输层。

---

## 10. 本轮明确不做

- 不做 HEVC / AV1：目标机型覆盖不足。
- 不做浏览器不可接收的二层多播。
- 不在没有数据时承诺完整 ABR；但保留未来使用 WebRTC 拥塞反馈或分档码率的接口空间。
- 不在 M3 第一阶段重写 DXGI Desktop Duplication 后端。
- 不把功能删除包装成延迟优化的技术前提。
- 不因 WebRTC 复杂而提前排除，也不因 WebCodecs 缓冲可控而提前选定。

本方案的核心原则是：**先建立真实时间线，再修复已证实的 CPU/缓存/反压问题；编码器按能力协商，GPU 管线按真实设备边界设计，最终以同条件原型决定传输层。**
