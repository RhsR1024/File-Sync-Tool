# 屏幕共享延迟优化方案审查报告

> 审查对象：`docs/design/screen-share-latency-optimization.md`<br>
> 审查日期：2026-07-28<br>
> 审查方式：结合当前仓库源码、依赖版本、现有测试以及 Media Foundation / WebCodecs / WebRTC 官方规范进行静态审查<br>
> 审查结论：**核心优化方向成立，但方案不能按现稿直接实施，需先修正缓存模型、码控策略、GPU 阶段边界和 WebCodecs 协议。**

---

## 1. 总体结论

原方案准确识别了当前实现中的几个主要性能风险：

- H.264 正常工作时仍在采集线程无条件编码 JPEG。
- BGRA → NV12 使用逐像素标量转换。
- 当前直接实例化 Microsoft H.264 Encoder MFT，没有枚举硬件编码器。
- H.264 输入帧在确认队列是否可接收前就复制整帧。
- 慢观看端发生 `Lagged` 后会重放 GOP 缓存。
- 每个观看端发送 H.264 分段时存在额外复制和 1 ms 人工延迟。
- MSE 播放器使用硬 seek 追赶直播边缘，容易造成可见跳跃。
- WGC 当前每帧执行 GPU → CPU 回读，GPU 零拷贝方向合理。

这些判断与现有代码基本一致，原方案的总体方向应当保留。

但方案中存在以下会影响正确性或实施顺序的问题：

1. “超长 GOP + 个位数缓存”会留下缺少参考帧的无效 GOP 快照。
2. WebCodecs 协议按单个 NAL 发送，缺少 access unit、时间戳和关键帧元数据。
3. P1 硬件编码与 P3 D3D device manager / GPU 纹理管线被人为拆开，实际存在强依赖。
4. 提议的 Media Foundation 码控属性组合并不适用于所有编码器，软件回退尤其不成立。
5. 删除 JPEG 后，现有 FPS、首帧、帧年龄和尺寸指标会停止更新。
6. 对单帧轮询、Tokio broadcast 回收和 MSE 历史缓冲的部分判断不准确。
7. “远程控制流畅度”几乎只从视频链路讨论，缺少输入链路指标和过载策略。

因此，本报告给出的评价是：**方向有条件通过，实施说明需修订后再动工。**

---

## 2. 已确认成立的关键问题

### 2.1 H.264 正常时仍无条件编码 JPEG

采集循环先提交 H.264，然后无条件调用 `encode_jpeg_reuse`：

- `src-tauri/src/screenshare.rs:3083-3097`

JPEG 路径还包含：

- BGRA → RGB 的逐像素 `push`：`src-tauri/src/screenshare.rs:4463-4481`
- `image::codecs::jpeg::JpegEncoder` 编码：`src-tauri/src/screenshare.rs:4483-4495`
- `jpeg_buf.clone()`：`src-tauri/src/screenshare.rs:4497`

该路径与 H.264 并行存在且运行在采集线程，确实可能直接限制采集节奏。将 JPEG 改为惰性 fallback 是当前最值得优先实施的低风险优化之一。

### 2.2 当前 H.264 输入路径存在多次大内存复制

当前路径至少包括：

1. WGC staging texture → `frame_buf`
2. 可选的 DXGI 光标合成 `frame_buf` → `frame_scratch`
3. `try_submit` 中 BGRA slice → `H264InputFrame.pixels`
4. BGRA → NV12 输出缓冲
5. NV12 → Media Foundation input buffer
6. Media Foundation output buffer → Rust `Vec<u8>`

相关代码：

- WGC 回读：`src-tauri/src/screenshare.rs:3981-3996`
- DXGI 光标 scratch：`src-tauri/src/screenshare.rs:2965-2983`
- H.264 输入复制：`src-tauri/src/screenshare_media.rs:274-290`
- NV12 转换：`src-tauri/src/screenshare_media.rs:554-605`
- MFT 输入复制：`src-tauri/src/screenshare_media.rs:950-970`
- MFT 输出复制：`src-tauri/src/screenshare_media.rs:1003-1017`

原方案识别了主要复制，但实际复制次数比文档列出的更多。P1 的循环缓冲只能减少分配抖动，不能消除这些数据搬运；真正的结构性收益仍依赖 P3 GPU 纹理管线。

### 2.3 当前未枚举硬件 H.264 编码器

`WindowsH264Encoder::new` 直接创建 `CLSID_MSH264EncoderMFT`：

- `src-tauri/src/screenshare_media.rs:859-867`

没有看到 `MFTEnumEx`、`IMFActivate`、`MFT_ENUM_FLAG_HARDWARE`、`IMFDXGIDeviceManager` 或异步 MFT 事件处理。因此，枚举硬件 MFT、进行运行时自检并保留软件回退的方向正确。

### 2.4 GOP 快照重放和慢客户端处理确有风险

新连接和 `Lagged` 都会调用 `send_h264_snapshot`：

- 初始连接：`src-tauri/src/screenshare.rs:4928-4939`
- `Lagged` 恢复：`src-tauri/src/screenshare.rs:4994-5005`
- 快照逐段发送：`src-tauri/src/screenshare.rs:5026-5043`

每段发送还会：

- `payload.to_vec()`
- `sleep(H264_STREAM_COOPERATIVE_DELAY)`，当前为 1 ms

这会让已经落后的客户端进一步积压，并增加服务端突发流量。取消完整 GOP 重放、为发送加超时、隔离慢客户端的方向正确。

### 2.5 MSE 硬 seek 可能造成明显跳跃

当前播放器在延迟超过 250 ms 时将 `currentTime` 直接设到直播边缘前 50 ms：

- `src/screen-share-web/lib/mse-player.ts:26-41`
- `src/screen-share-web/lib/mse-player.ts:289-299`

使用播放速率进行小幅漂移校正、只在起播或严重卡顿时 seek，方向正确。但具体阈值应通过真机测量确定。

---

## 3. 必须修正的设计问题

### 3.1 阻塞：长 GOP 与小缓存组合会产生不可解码快照

当前 `H264MediaState::publish_segment` 在关键帧到来时清空缓存，并在缓存超限时保留最前面的关键帧、删除其后的较早分段：

- `src-tauri/src/screenshare_media.rs:187-220`

当 GOP 保持在 2 秒以内且缓存上限为 180 时，这通常能够保留完整 GOP。原方案同时提出：

- 取消周期性 IDR，使用超长或无限 GOP
- 将 `H264_GOP_CACHE_LIMIT` 降至个位数

按当前淘汰算法实施后，缓存会变成：

```text
IDR, P(n-3), P(n-2), P(n-1), P(n)
```

中间作为参考的 P 帧已经被删除。把这样的内容发送给新观看端或掉队观看端，后部 P 帧通常无法正确解码。

#### 建议

不要把“调小缓存常量”作为修复方式，应重构缓存语义：

- 单独保存当前 `init` / codec config。
- 单独保存最近一个完整 IDR access unit。
- 新观看端接入后只发送 init，并等待一个新的或足够新的 IDR。
- 每观看端增加 `waiting_for_keyframe` 状态，在收到 IDR 前丢弃 delta 帧。
- 如果需要缓存 IDR 后的 delta，必须保存连续、完整的依赖链，不能保留任意尾部。
- 强制 IDR 请求需要全局合并、最小间隔和速率限制。

### 3.2 阻塞：WebCodecs 消息不能按单个 NAL 设计

原方案定义：

```text
[u32 长度][AVCC NAL 单元]
```

但 WebCodecs 的 `EncodedVideoChunk` 对 H.264 的要求是完整 access unit，而不是任意单个 NAL。一个 access unit 可能包含：

- AUD
- SPS / PPS
- SEI
- 一个或多个 slice NAL

当前服务端已经在 `parse_annex_b_access_unit` 后获得完整的 `parsed.avcc`，并且保留了 MFT 输出的 sample time 和 duration：

- `src-tauri/src/screenshare_media.rs:713-788`

#### 建议协议

每条媒体消息至少包含：

```text
version
generation
sequence
timestamp_us
duration_us
flags: key / delta / discontinuity
payload_length
payload: 一个完整 AVCC access unit
```

配置消息单独携带：

- codec string
- `AVCDecoderConfigurationRecord`（avcC）
- width / height
- color space（如可用）

观看端据此创建：

```ts
new EncodedVideoChunk({
  type: flags.key ? 'key' : 'delta',
  timestamp: timestampUs,
  duration: durationUs,
  data: accessUnit,
})
```

WebCodecs AVC 注册规范明确要求 `EncodedVideoChunk` 内部数据是 access unit：

- <https://www.w3.org/TR/webcodecs-avc-codec-registration/>

### 3.3 阻塞：P1 硬编与 P3 D3D 管线存在强依赖

原方案把硬件 MFT 枚举放在 P1，把 `IMFDXGIDeviceManager` 和 GPU surface input 放在 P3。这样拆分在部分驱动上可能可用，但不能作为稳定架构假设。

需要提前解决的问题包括：

- 硬件 MFT 是否接受 system-memory NV12。
- 接受 system-memory 时是否发生隐藏 GPU 上传。
- 编码器是否必须在设置 media type 前绑定 D3D manager。
- WGC、VideoProcessor、MFT 是否使用同一个 D3D11 device。
- GPU texture 在 WGC frame 关闭后如何保持生命周期。
- capture thread 与 encoder thread 如何安全共享 D3D11 device/context。

硬件 MFT按规范采用异步处理模型，不能直接复用当前同步 `ProcessInput/ProcessOutput` 循环：

- <https://learn.microsoft.com/en-us/windows/win32/medfound/hardware-mfts>
- <https://learn.microsoft.com/en-us/windows/win32/medfound/asynchronous-mfts>

#### 建议

二选一：

1. 将“硬件 MFT + D3D manager + GPU surface input”作为一个完整阶段实施。
2. 保留 P1 system-memory 硬编实验，但将其明确标记为兼容性探测，不承诺所有目标机型可用；失败时立即回退软件 MFT。

### 3.4 GPU 零拷贝目前只覆盖 WGC，未覆盖 DXGI fallback

当前 `CaptureSource` 对外只暴露 CPU 像素：

- DXGI：`scrap::Frame`
- WGC：`Borrowed { pixels, stride }`

代码位置：

- `src-tauri/src/screenshare.rs:2696-2758`

P3 完成后，如果仍保留当前 `scrap` DXGI 后端，则 DXGI 路径仍然需要：

- GPU → CPU readback
- CPU 光标合成
- CPU BGRA → NV12 或再次上传 GPU

原方案还遗漏了 DXGI 光标处理。WGC 通过 `SetIsCursorCaptureEnabled` 在采集层处理光标，而 DXGI 当前在 CPU 上执行光标渲染和 alpha composite：

- `src-tauri/src/screenshare.rs:2965-2983`
- `src-tauri/src/screenshare.rs:2117-2407`

#### 建议

- P3 第一阶段只承诺 WGC 零拷贝。
- DXGI 明确保留为 CPU/SIMD fallback，或者替换 `scrap`，由自有 Desktop Duplication 后端暴露 `ID3D11Texture2D`。
- 如果要求 DXGI 也零拷贝，需要另行设计 GPU 光标合成。

### 3.5 码率控制参数组合不能硬编码

原方案建议：

```text
LowDelayVBR
AVEncCommonBufferSize = 1–2 帧
AVEncCommonMaxBitRate = 目标 × 1.5
AVEncVideoMaxNumRefFrame = 1
AVLowLatencyMode = true
```

问题在于：

- Microsoft H.264 Encoder 文档没有把 LowDelayVBR 列为其支持模式。
- `CODECAPI_AVEncCommonBufferSize` 对 Microsoft 编码器定义为 CBR buffer size。
- `CODECAPI_AVEncCommonMaxBitRate` 对应 PeakConstrainedVBR。
- `AVEncVideoMaxNumRefFrame` 和 `AVLowLatencyMode` 对部分编码器只是可选属性。
- Intel 不同代际驱动暴露的属性集合可能不同。

官方属性说明：

- <https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-encoder>

#### 建议

为每个 MFT建立运行时能力矩阵：

1. 对每个 `ICodecAPI` 属性调用 `IsSupported` / `IsModifiable`。
2. 优先尝试真正支持的低延迟模式。
3. 不支持时回退到：
   - CBR + 小 VBV buffer + mean bitrate，或
   - PeakConstrainedVBR + mean/max bitrate。
4. 所有属性设置失败都要记录编码器名称、HRESULT 和最终生效模式。
5. 自检结果中上报实际 profile、rate-control mode、B-frame、reference-frame 和 low-latency 状态。

### 3.6 High profile 不会自动启用 CABAC，且必须显式禁止 B 帧

当前代码固定 Baseline：

- `src-tauri/src/screenshare_media.rs:884`

切换 High profile 本身不保证启用 CABAC。Microsoft H.264 Encoder 的 CABAC 默认关闭，需要通过 `CODECAPI_AVEncH264CABACEnable` 设置且编码器支持时才生效。

另一方面，当前 fMP4 构造器没有写 composition time offset：

- `src-tauri/src/screenshare_media.rs:466-518`

这意味着实现默认假设 DTS = PTS，没有为 B 帧重排序建模。硬件编码器在 High profile 下是否默认产生 B 帧不能依赖厂商默认值。

#### 建议

- 显式设置 `CODECAPI_AVEncMPVDefaultBPictureCount = 0`。
- 显式探测并开启 CABAC，而不是只切 profile。
- 若无法确认无 B 帧，则自检必须解析输出 GOP，或者为 fMP4/WebCodecs 协议补齐 DTS/PTS 模型。

### 3.7 删除 JPEG 前必须先拆分指标和最新帧元数据

当前以下状态都在 JPEG 编码成功后更新：

- `media_metrics.record_encoded_frame`
- `interaction.record_frame_with_metadata`
- MJPEG broadcast
- `fps_counter`

代码位置：

- `src-tauri/src/screenshare.rs:3132-3143`

而 `/status` 和宿主页面又从 `InteractionState.latest_frame` 获取：

- 最新帧 ID
- width / height
- captured time
- frame age

代码位置：

- `src-tauri/src/screenshare.rs:1420-1438`
- `src-tauri/src/screenshare.rs:5477-5489`

如果直接删除 JPEG 调用，H.264 正常时这些状态将停止更新。

#### 建议拆分

- `capture_frame_count / capture_fps`
- `h264_encoded_frame_count / encoded_fps / encoded_bytes`
- `mjpeg_encoded_frame_count / jpeg_size`
- `outbound_bytes_total / outbound_bitrate`
- `latest_capture_metadata`，不包含 JPEG bytes
- 可选 `latest_mjpeg_frame`，只在 MJPEG 活跃时存在

---

## 4. 原问题清单中的事实性修正

### 4.1 J3：“N 个观看端 × 频率 = N 倍 JPEG 编码”不成立

观看端的单帧轮询会发起：

```text
/stream?single=1
```

服务端不会为请求重新编码，而是直接返回 `interaction.latest_frame_bytes()` 中缓存的 JPEG：

- `src-tauri/src/screenshare.rs:4713-4741`

因此，单帧轮询的真实成本是：

- N 倍 HTTP 请求
- N 倍缓存 JPEG 传输
- 响应对象和网络开销

而不是 N 倍 JPEG 编码。删除该功能能减少请求和带宽，但不会消除成倍编码。

### 4.2 F3：“慢 receiver 导致 broadcast 队列无法回收”不准确

`socket.send().await` 无超时确实存在问题：

- 对应观看端 task 可能长时间不退出。
- `ViewerGuard` 不能及时 drop，viewer count 和 IP 状态可能滞留。
- task 持有的资源和 WebSocket 状态不能及时释放。

但 Tokio broadcast 是固定容量覆盖式环形缓冲。慢 receiver 不会阻止全局缓冲回收；当旧值被覆盖时，该 receiver 会收到 `Lagged`。

因此应保留 F3 的“发送超时和连接回收”结论，但删除“broadcast 队列无法回收”的因果描述。

### 4.3 V2：保留 20 秒历史缓冲不会直接增加直播延迟

当前 `trimBuffer` 保留较长历史：

- `src/screen-share-web/lib/mse-player.ts:301-315`

它主要影响：

- 浏览器内存
- MSE buffer remove 的频率和开销
- 可 seek 的历史范围

真实直播延迟由 `buffered.end - currentTime` 决定。把历史从 20 秒缩到 2 秒有价值，但不应把它当作直接降低玻璃到玻璃延迟的主要措施。

### 4.4 U3：本地暂停与 MSE 追边缘没有直接实现冲突

本地暂停时会抓取当前 `<video>` / `<img>` 到 canvas，再显示 data URL：

- `src/screen-share-web/App.vue:400-443`

但代码没有停止 `MseH264Player`。隐藏的 H.264 `<video>` 仍在接收、append 和执行 live-edge 同步，恢复时通常已经处于直播边缘。

因此，本地暂停是否删除应被视为产品决策，而不是 P1 播放优化的技术前置条件。

### 4.5 “RTCPeerConnection 规范要求安全上下文”不成立

WebRTC 规范中 `RTCPeerConnection` 的 WebIDL为：

```text
[Exposed=Window]
interface RTCPeerConnection
```

没有 `[SecureContext]`。通常需要 HTTPS 的是 `getUserMedia` 等本地设备采集 API，而本项目观看端不需要采集本地摄像头或麦克风。

规范：

- <https://www.w3.org/TR/webrtc/>

这不代表项目必须选择 WebRTC，但“因为 WebRTC也必须 HTTPS，所以不选”的论据应删除。

---

## 5. P0 功能删除的审查意见

### 5.1 共享冻结

共享冻结确实依赖服务端保存 JPEG bytes：

- `StoredFrame.bytes`：`src-tauri/src/screenshare_interaction.rs:292-300`
- `view.freeze`：`src-tauri/src/screenshare_interaction.rs:758-779`
- `/snapshot/:frame_id`：`src-tauri/src/screenshare.rs:5143-5165`

它还是全局 document 状态，并在冻结时撤销待处理或已授予的远控：

- `src-tauri/src/screenshare_interaction.rs:762-770`

如果产品已经决定删除，共享冻结应随 JPEG bytes 存储和 snapshot endpoint 一并删除。这确实能够简化 JPEG 惰性化。

### 5.2 本地暂停

本地暂停完全可以在客户端通过 `<video>` 截帧实现，不要求服务端持续保存 JPEG。它与服务端 JPEG 路径不是强耦合关系。

如果产品已决定删除，可以删除；但不应把它描述成解除 P1 技术阻塞。

### 5.3 刷新频率选择器

该选择器当前只在 MJPEG 模式实际有意义。删除它不会减少采集线程当前的 JPEG 编码次数，但会移除 MJPEG fallback 下的每客户端带宽限制。

如果保留 MJPEG作为最后回退，建议至少保留一个服务端统一 fallback FPS 上限，例如 5–10 FPS 或按负载自动调整。否则硬件和软件 H.264 都失效时，30 个观看端会全部进入全速 MJPEG，正好落入最差负载场景。

---

## 6. 远程控制链路专项审查

原方案把“远控流畅度”主要归因于视频返回延迟，但当前输入链路也应独立度量。

### 6.1 当前已有的正确设计

前端不会发送每个原始 pointer event，而是每个 `requestAnimationFrame` 只发送最后一个位置：

- `src/screen-share-web/components/RemoteControlOverlay.vue:18-55`

后端输入队列也会合并相邻 pointer move，同时明确禁止跨越 button、wheel、key 边界进行合并：

- `src-tauri/src/screenshare_input.rs:383-400`

队列有固定容量 128，满时撤销控制权，而不是无限积压：

- `src-tauri/src/screenshare_input.rs:412`
- `src-tauri/src/screenshare.rs:5336-5357`

这些设计是合理的，不需要推倒重写。

### 6.2 仍缺少的保护

#### 浏览器发送缓冲没有过载策略

`ScreenShareSessionClient.send` 只检查 WebSocket 是否为 OPEN，没有检查 `socket.bufferedAmount`：

- `src/screen-share-web/lib/session-client.ts:136-151`

当客户端网络或浏览器线程暂时卡顿时，已经进入浏览器 WebSocket buffer 的旧 move 无法再被服务端队列合并。

建议：

- pointer move 在 `bufferedAmount` 超过小阈值时丢弃或只保留最新值。
- button/key/release_all 不能静默丢弃。
- 若关键输入也无法及时发送，应主动退出控制状态，而不是继续累积陈旧输入。

#### 交互 socket 单 task 串行读写

`run_interaction_socket` 在同一个 `tokio::select!` 循环中同时处理：

- outbound interaction broadcast
- inbound remote input
- heartbeat / cancellation

当某次 `send_interaction_message(...).await` 被慢客户端反压时，该 task 不再轮询 inbound input：

- `src-tauri/src/screenshare.rs:5268-5284`

建议将 WebSocket split 为独立 reader / writer：

- reader 优先处理输入授权、解析和 enqueue。
- writer 使用有界队列和发送超时。
- 状态快照可以合并，控制撤销和错误消息不能静默丢弃。

### 6.3 应增加的远控指标

- 浏览器 pointer event → WebSocket send 时间
- `bufferedAmount` P50 / P95 / P99
- 服务端 receive → input queue enqueue
- input queue depth、move coalesced 数量、queue full 次数
- dequeue → `SendInput` 完成时间
- 远端输入发生 → 对应画面变化在观看端显示的 motion-to-photon 延迟

最后一项不能简单通过“视频单向延迟 × 2”计算。

---

## 7. 建议的实施顺序

### M0：度量与基线

在任何结构性改造前完成：

1. 使用 WGC `Direct3D11CaptureFrame.SystemRelativeTime` 记录真实帧时间，不能只在 CPU 回读完成后打时间戳。
2. 增加以下服务端阶段计时：
   - frame wait
   - capture queue age
   - GPU readback
   - black-frame classification
   - JPEG color conversion
   - JPEG encode
   - BGRA → NV12
   - H.264 input queue age
   - MFT encode
   - mux
   - per-client send wait
3. 观看端增加：
   - WebSocket receive
   - append queue duration / bytes
   - SourceBuffer append duration
   - live-edge distance
   - playbackRate / seek 次数
   - dropped / presented frames
4. 单独测量：
   - capture-to-display
   - input-to-SendInput
   - input-to-visible-response

注意：P50 是中位数，不是平均值；各阶段 P99 也不能直接相加当作端到端 P99。

### M1：低风险、高收益修复

1. 解耦捕获元数据、H.264 指标和 JPEG 指标。
2. H.264 ready 且无 MJPEG consumer 时停止 JPEG 编码。
3. 使用 `axum::serve(...).tcp_nodelay(true)`；axum 0.7 已提供该配置。
4. 删除 H.264 发送后的 1 ms sleep。
5. 为媒体和交互 WebSocket send 增加超时。
6. `Lagged` 客户端优先断开并快速重连，不立即为单个慢客户端触发全局 IDR。
7. H.264 cache 重构为 init + 最新 IDR，而不是任意 GOP 尾部。
8. MSE 起播允许一次 seek；稳态使用分档 playbackRate 调节。
9. append queue 改为按预计时长或字节上限控制，避免 180 个 segment 才报错。

发送超时不建议未经测量就设为 3–5 秒。对低延迟媒体，先测试 500–1000 ms 更合理；持续慢客户端应尽快隔离。

### M2：编码器与软件回退

1. 用 `MFTEnumEx` 枚举硬件 encoder MFT。
2. 同时实现同步 MFT和异步 MFT adapter。
3. 为每个 `ICodecAPI` 属性做能力探测。
4. 显式禁止 B 帧。
5. 对 profile、CABAC、rate control、VBV、reference frame 做自检和日志上报。
6. 软件/RDP fallback 使用 SIMD BGRA → NV12，而不是继续使用当前标量循环。
7. 明确实现软件 fallback 的降分辨率/降帧率；原方案虽然写了该回退阶梯，但 P1 当前没有任何 scaler 设计。

### M3：WGC GPU 管线

1. 让 WGC、VideoProcessor、DXGI device manager、硬件 MFT共享同一 D3D11 device。
2. 使用 2–3 块 NV12 texture pool。
3. 明确 texture / frame 生命周期和线程所有权。
4. GPU 完成 BGRA → NV12 和缩放。
5. 黑屏检测使用小尺寸、低频异步回读。
6. DXGI 暂时保留 CPU/SIMD fallback，除非另行重写采集后端。
7. 60 FPS 作为可选或自适应档，而不是默认目标。

60 FPS 可能降低捕获等待和以帧计的浏览器队列延迟，但并非“近似免费”：它会增加 MFT 调度、分段、WebSocket 消息、MSE append 和解码/合成频率，必须在 Broadwell 真机上验证。

### M4：传输层原型对比

P3 完成后再比较：

#### WebCodecs over WSS

优点：

- 可复用当前一份编码、N 份发送的架构。
- 客户端可以只保留最新待渲染帧。
- 可以删除 fMP4/MSE 追边缘逻辑。

风险：

- WebCodecs 是 SecureContext API。
- 自签证书例外存在显著运营成本。
- Canvas 渲染在 Broadwell 上可能产生额外复制或 GPU/CPU 开销。
- TCP 丢包时仍存在队头阻塞。

#### WebRTC receive-only

优点：

- `RTCPeerConnection` 本身不要求 SecureContext。
- 原生提供 RTP、SRTP、NACK、PLI、jitter buffer 和拥塞控制。
- 对临时丢包和未来 Wi-Fi 场景更稳健。

风险：

- SDP / ICE / RTP / RTCP 实现复杂度更高。
- 每观看端仍需独立 SRTP 和发送状态。
- 需要验证一份编码流如何复用到 30 个 PeerConnection，避免重复编码。

本项目已经存在 `ScreenShareMediaTransport::WebRtc` 枚举：

- `src-tauri/src/screenshare.rs:253-279`

因此建议以小型 receive-only LAN 原型获得数据后再决定，不应因为错误的 HTTPS前提提前排除。

---

## 8. 验收与压测修订建议

### 8.1 不应只使用“5 台 + 25 标签页”

该方法可以初步测试：

- 服务端 fan-out task 数量
- WebSocket 分段发送
- 部分广播 Lagged 行为

但不能真实模拟：

- 30 个独立网络栈和接收窗口
- 30 个独立浏览器进程、GPU decoder 和 compositor
- 多台旧 Intel 核显的解码负载
- 真实网络抖动和丢包
- 标签页后台节流差异

F3 应使用一个专门的慢客户端：建立 WebSocket 后停止读取或限制接收窗口，以稳定制造发送反压。

### 8.2 指标需要区分健康客户端和故障注入客户端

“`record_lagged_frames` 恒为 0”只适合健康稳态阶段。加入慢客户端或断网注入后，更合理的验收条件是：

- 健康客户端 lag 为 0。
- 慢客户端在限定时间内被隔离。
- 慢客户端不会触发其他客户端明显的延迟上升。
- viewer count、task 和 IP 状态在限定时间内回收。
- 强制 IDR 有全局频率上限，不会被单个异常客户端形成 keyframe storm。

### 8.3 建议量化而不是使用“平稳”描述

建议增加：

- 100 ms / 1 s 时间窗的 outbound bitrate P50 / P95 / P99
- 单 IDR 最大字节数
- 同一时刻 IDR fan-out 的 socket send P95 / P99
- 健康观看端 live-edge distance P95 / P99
- MSE/WebCodecs dropped frame ratio
- 输入 queue age P95 / P99
- 观看端重连恢复时间 P95 / P99

---

## 9. 对原方案开放问题的答复

### 9.1 自签 HTTPS 后 WebCodecs 是否可用

仍需按原方案建议真机验证。Secure Contexts 规范把 HTTPS origin 视为 potentially trustworthy，但证书点击穿透、浏览器安全状态、例外生命周期和证书轮换行为属于浏览器实现及运营问题：

- <https://www.w3.org/TR/secure-contexts/>

额外需要验证：如果证书每次屏幕共享会话都重新生成，即使 IP 不变，Chrome/Edge 是否会再次弹出拦截页。会话级轮换可能比原文估计的运营摩擦更大。

### 9.2 Broadwell 上 VideoFrame → Canvas 成本

同意原方案：必须先做最小原型。测试时至少记录：

- `decodeQueueSize`
- output callback 频率
- drawImage 或 WebGL upload 时间
- CPU / GPU 占用
- presented / dropped frames
- canvas 到屏幕的实际显示延迟

### 9.3 Intel 老驱动硬件 MFT 自检

建议自检标准：

- 总超时 1–2 秒。
- 输入包含移动或变化图案，不能全黑/全静态。
- 成功取得 SPS、PPS、IDR。
- 输出时间戳单调，duration 合法。
- 输出 access unit 可被一个独立 decoder 成功解码。
- 记录 MFT friendly name、hardware URL、driver/adapter 信息和实际生效参数。

### 9.4 60 FPS 是否能让 MSE 延迟减半

不能作为阶段承诺。浏览器缓冲可能按帧数，也可能受时间阈值、解码器实现和 compositor 策略控制。应保留 30 FPS 基线，并把 60 FPS 做成真机实验或自适应模式。

### 9.5 是否保留 MJPEG

建议当前版本保留，但严格惰性启用。只有在以下条件满足后再考虑彻底删除：

- 所有目标机型的硬件 MFT覆盖率已实测。
- 软件 MFT fallback 已稳定。
- RDP、Microsoft Basic Display Adapter、虚拟机等环境均有明确行为。
- 现场遥测显示 MJPEG fallback 长期没有实际使用价值。

### 9.6 所有耗时均为推算

同意原方案的风险声明。实施优先级必须允许被真机数据重排，尤其不能把 30–45 ms、80–130 ms、150–200 ms 当作版本承诺。

### 9.7 方案 B 的运营成本

同意这是产品决策，但在做决定前应将下列成本演示给产品和现场支持人员：

- 每个共享主机/IP 的证书警告
- 浏览器 profile 或缓存清理后的重复操作
- 证书轮换是否导致重复警告
- DHCP / 多网卡 / VPN 改变访问 IP
- 30 人同时接入时的人工指导成本

---

## 10. 最终建议

建议保留原方案的主要方向，但按以下原则修订：

1. 将“度量与指标解耦”提升为真正的第一阶段。
2. 将产品功能删除与性能优化拆开管理，避免把本地暂停等非阻塞功能包装成技术前置条件。
3. 先完成 JPEG 惰性化、发送超时、缓存重构、MSE 队列和 live-edge 控制。
4. 硬件 MFT按能力协商，不硬编码 CodecAPI 参数组合。
5. 明确 P1 system-memory 硬编和 P3 GPU surface input 的依赖关系。
6. GPU 零拷贝第一阶段只承诺 WGC，DXGI 另行规划。
7. WebCodecs 消息必须以完整 access unit 为单位，并携带时间戳和帧类型。
8. WebRTC应重新进入原型比较，不再以“必须 HTTPS”为排除理由。
9. 将远控输入路径纳入观测、过载保护和 motion-to-photon 验收。
10. 所有延迟目标以 Broadwell / Skylake / 10 代 Intel 真机数据为准。

综合评价：**核心诊断约七成正确，缓存、码控、GPU 阶段边界和 WebCodecs 协议修订后，才适合进入开发。**

---

## 11. 本次静态验证

审查期间执行了现有屏幕共享 Web 测试：

```text
pnpm test:screen-share-web
```

结果：

- 8 个测试文件通过
- 33 个测试通过
- 0 个失败

这些测试覆盖现有 MSE 边缘判断、坐标、远控状态、session client、批注状态等逻辑，但不包含真机延迟、硬件 MFT、GPU 管线或 30 观看端负载测试。
