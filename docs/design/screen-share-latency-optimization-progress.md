# 屏幕共享低延迟优化进度与证据矩阵

> 对应 `screen-share-latency-optimization.md`，更新日期：2026-07-28。
>
> 本文区分“代码与自动化验证完成”和“目标硬件/现场性能验收完成”。没有真机报告时，不得宣称已经达到某个毫秒指标。

## 1. 状态定义

| 状态 | 含义 |
|---|---|
| `code-done` | 代码与可在当前工作区执行的自动化验证已经具备；仍可能受外部验收门禁约束 |
| `external-hardware-blocked` | 需要目标 CPU/GPU/驱动、真实浏览器/证书/网段或独立设备，当前工作区不能代替 |
| `decision-pending` | 原型已具备，但必须等同条件数据后才能选型或调整默认值 |
| `not-applicable` | 经审查后不是问题、已被产品删除或明确不在本轮范围 |

“源码存在”本身不是 `code-done` 证据。每项必须至少有类型/编译检查或针对性测试；性能结论还必须引用原始测量产物。

## 2. 当前总体结论

| 阶段 | 代码状态 | 规格完成状态 | 尚缺证据 |
|---|---|---|---|
| 产品清理 | `code-done` | 完成 | 刷新频率、共享冻结、本地暂停及 snapshot/single-frame 路径已删除；共享者本机预览于 2026-07-30 删除，见 §2.1 |
| 共享者批注操作栏 | `code-done` | 完成 | 有批注时在共享屏幕右下角出现，可清除上一条或全部，见 §2.2 |
| M0 度量 | `code-done` | `external-hardware-blocked` | 三代 Intel、真实浏览器呈现、受控远控响应和 20–30 独立设备基线；断线重连恢复与共享端资源采样已补齐产出方 |
| M1 低风险修复 | `code-done` | `external-hardware-blocked` | 30 客户端 30 分钟、慢客户端、锁屏/多屏/控制完整回归 |
| M2 编码器 | `code-done` | `external-hardware-blocked` | Broadwell、Skylake、10 代 Intel 的 capability/self-test/长稳报告；2026-07-29 已由 Intel 目标机实测发现并修复两个降级链缺陷，见 §5.1 |
| M3 WGC GPU | `code-done` | `external-hardware-blocked` | Intel/NVIDIA/AMD、1080p/4K、锁屏/重配置/故障注入与资源曲线 |
| M4 传输原型 | `code-done` | `decision-pending` | 受管 Edge/策略/证书、真实媒体与丢包抖动、三代 Intel 和 30 独立客户端同条件比较 |
| 30/60 FPS 可选档 | `code-done` | 完成 | 界面 60 FPS 实验开关、1–60 FPS 服务端范围与微秒节拍已落地 |
| 30/60 FPS 默认值 | 默认保持 ≤30 FPS | `decision-pending` | Broadwell 及最终传输方案下的捕获等待、资源和丢帧数据 |
| 现场证据门禁 | `code-done` | 完成 | 九个门禁全部结构化校验；六类现场报告不能只凭 `status` 关闭 |

因此，本轮可以确认“规格要求的实现骨架与自动化门禁已落地”，不能确认“整个性能规格已验收”或“某传输已经胜出”。

### 2.1 共享者本机预览已删除（2026-07-30）

`2026-07-19-screen-share-collaboration-control-design.md` 把“本机预览客户端”定为 P0，理由是共享者能看到与观看者一致的画面。实机使用后确认该方案对整屏捕获无解：预览窗口渲染的就是它自己所在的那块显示器，必然形成无限嵌套，与共享者自己用浏览器打开观看地址没有区别。

唯一的技术出路是把预览窗口排除出捕获，而这条路已在 `screen_share_desktop_overlay_ready` 的注释中记录为不可用——被排除的窗口会让 WGC 和 DXGI 在受支持的 Windows 版本上返回黑帧。既然桌面批注叠加层已经把批注直接画在共享者桌面上，预览窗口的剩余价值不足以支撑一条独立的鉴权路径。

已删除：`screen_share_open_local_preview`、`screen_share_close_local_preview`、`ScreenShareHandle::preview_token`、`HttpServerState::preview_token`、`/?host_preview=<token>` 一次性能力、`ss_preview` HttpOnly Cookie 及其在六个鉴权点上的旁路，以及界面按钮与中英文文案。共享地址的鉴权现在只有一条路径（`check_auth_cookie`）。回归门禁见 `src/pages/ScreenSharePage.test.mjs` 的 `screen share no longer offers the host local preview`。

### 2.2 共享者批注操作栏（2026-07-30）

桌面批注叠加层是鼠标穿透的，只能画不能操作，因此观看者批注后共享者必须切回工具的屏幕共享界面才能清除。现在共享监视器工作区右下角会出现一个可交互的小操作栏，提供“清除上一条”和“清除全部”，以及一个“隐藏到下次有新批注”的按钮。

- 窗口标签 `screen-share-annotation-bar-<session_id>`，页面 `src/pages/ScreenShareAnnotationBarPage.vue`，能力声明 `src-tauri/capabilities/screen-share-annotation-bar.json`。
- 与叠加层一同在会话启动时创建（`annotations_enabled` 为真时），初始隐藏；显示与隐藏由 Rust 的 `screen_share_set_annotation_bar_visible` 执行并使用 `SW_SHOWNOACTIVATE`，因此操作栏出现时不会从共享者正在演示的窗口抢走焦点。
- 判定规则（是否显示、撤销目标、dismiss 的进位与失效）抽到 `src/screen-share-web/lib/annotation-bar.ts`，由 `annotation-bar.test.ts` 的 10 项测试锁定；页面不允许另写一份。激光点会自行过期，因此不计入。
- 定位使用 `GetMonitorInfoW` 的 `rcWork`（新增 `work_rect_for_monitor`），让操作栏落在任务栏上方而不是压住时钟；`annotation_bar_placement` 的右下锚定、DPI 缩放、副屏原点和窄工作区夹取由 5 项 Rust 测试覆盖。捕获源映射仍然使用 `rcMonitor`，未受影响。
- 操作栏与叠加层一样无法排除出捕获（同 §2.1），因此观看者也会看到它。

## 3. M0：度量与基线

### 3.1 已落地

- `screenshare.rs` 的 `ScreenShareMediaMetricsState` 分离 capture、JPEG、H.264、GPU readback/preprocess、black-frame、per-client send 和 100 ms/1 s outbound 窗口；状态不再依赖 JPEG 成功。
- WGC 使用 `SystemRelativeTime` 计算 capture queue age，并生成 capture sequence 与近似 wall-clock capture time；DXGI 明确只能使用观察时刻。
- `screenshare_media.rs` 为每个 AU 保留 generation、media sequence、capture sequence、capture time、输入序列和成功 `SendInput` 时间；MSE/WebCodecs 发送 `media.trace` sidecar。
- `/time` 与浏览器最小 RTT 时钟估计用于跨端关联；时钟源使用 `performance.timeOrigin + performance.now()`，MSE/WebCodecs 每 30 秒复校，offset 突变会清空旧 epoch 并累计 discontinuity。MSE 优先使用 `requestVideoFrameCallback.expectedDisplayTime`；WebCodecs 只在下一次 animation paint 计呈现，paint 前覆盖帧计入 dropped，并明确标记为 pre-paint proxy。
- `screenshare_input.rs` 只在 Windows 注入成功后推进 `latest_applied_input`；capture boundary 会拒绝 `applied time > capture time` 的输入，避免把未来输入挂到旧帧。
- `session-client.ts`、`latency-trace.ts` 分别记录 pointer event→send、queue ACK、successful SendInput、capture→display 和 input→visible 分布。
- `scripts/screen-share-benchmark.mjs` 支持 MSE H.264、WebCodecs H.264 和 MJPEG 的健康客户端、停止读取慢客户端、状态前后差异和原始 JSON 输出；它只验证 wire fan-out/反压，不冒充浏览器解码与呈现；模板见 `screen-share-latency-baseline-template.md`。
- `window.__SCREEN_SHARE_DIAGNOSTICS__.snapshot()` 返回当前 transport、服务端指标，以及 MSE/WebCodecs/WebRTC 各自的 `{ state, metrics }`。
- 服务端与 WebRTC 分布均区分累计样本数、滚动保留数、容量和测量范围；viewer、IP 引用和 media task 三个计数独立暴露，便于证明断线回收。

### 3.1.1 逐条核对补齐的两项（2026-07-28 复核）

对 §3.1/§3.2/§8.2 做逐条核对后，发现两项"必须报告"的结果没有任何产出方，现已补齐：

- **断线重连恢复**：§8.2 要求报告该分布，但播放器只有重连逻辑（状态、退避、次数），没有恢复计时。现在 MSE 与 WebCodecs 均输出 `reconnectRecoveryMs` 与 `unexpectedDisconnectCount`。MSE 记录断线时刻的缓冲末端，只有 `mediaTime` 超过它的呈现帧才结算——否则断线前已缓冲的画面会把恢复时间压成接近 0；generation 变化会重建时间线，此时边界失效，改为任何呈现帧都算恢复。WebCodecs 没有播放缓冲，重连后第一帧绘制即结算。主动 `stop()` 与终止性失败不产生样本。WebRTC 原型不自动重连，该项对它不适用。
- **共享端资源**：§8.2 要求共享端 CPU/GPU/内存，此前基准脚本明确把它标为"需要目标主机工具"，而仓库里没有这样的工具。新增 `pnpm benchmark:screen-share:resource-sample`，按固定节拍采样目标进程 CPU（按逻辑核归一化）、工作集、私有字节和句柄数，GPU 取该进程 GPU engine 计数器之和并使用独立的较慢节拍（枚举实例在本机约 2.3 秒，与 CPU 同频会把 12 秒窗口压到 3 个样本）。计数器不可用时写入 `evidence_gaps` 并返回 2，不用 0 冒充测量值。观看端资源仍必须在观看设备上测量。

其余 §0.2、§2.1–§2.7、§3.1、§4–§7 条款在本次核对中均已找到对应实现与测试（例如 IDR 合并/限速为 200 ms / 500 ms、交互 writer 的 critical/bulk 优先级、`/snapshot`、`/stream?single=1` 的移除门禁）。

### 3.2 指标解释边界

- `capture-to-display` 只能从实际呈现回调计算，接收、append 或 decode 完成不能冒充呈现。
- `input-to-visible-response` 当前是受控场景代理：关联首个 `capture time >= successful SendInput time` 且被呈现的帧。它排除了时间倒挂，但不检查像素，任意场景不能据此声称严格视觉因果。
- WebRTC 已能在 SDP 实际协商后为 AU 发送实验性 Absolute Capture Time，并分别报告协商和发送样本；客户端也会报告协商/浏览器样本状态。但目标浏览器样本尚未与同一服务端 capture sequence 交叉验证，因此 `endToEndLatency.available=false`；`captureTime`/`receiveTime` 仍只报告为 browser timing proxy，`getStats()` 的 RTT/jitter/frames 不能冒充端到端延迟。该扩展不是 RFC 8872。
- 各阶段 P99 不可相加为端到端 P99。

### 3.3 外部门禁

- Broadwell、Skylake、10 代 Intel；720p30、1080p30；静态、动态、视频、快速滚动。
- WGC、DXGI、RDP、Basic Display Adapter/无硬编。
- 1、5、20、30 健康客户端；至少最终 20–30 台独立设备。
- 停止读取媒体客户端、交互 writer 反压、网络损伤、锁屏/解锁、多屏切换。
- 受控点击变色/计数器场景的 input-to-visible，以及外部光学基准交叉校验。

## 4. M1：低风险修复与扇出加固

| 规格项 | 状态 | 代码证据 |
|---|---|---|
| JPEG 惰性化与指标解耦 | `code-done` | 只有 MJPEG consumer 才触发 JPEG/readback；fallback 上限 10 FPS；最新 capture 元数据独立维护 |
| 删除旧产品路径 | `code-done` | `/snapshot/:frame_id`、`/stream?single=1`、刷新/冻结/本地暂停已移除；`/stream` 仅为 MJPEG |
| TCP 与发送超时 | `code-done` | `tcp_nodelay(true)`；媒体/交互发送均有限时；H.264 固定 1 ms sleep 已删除 |
| 观看端上限与状态回收 | `code-done` | 全局 40 media viewer soft ceiling、原子 reserve、429、IP 引用计数和 RAII guard |
| GOP/IDR 恢复 | `code-done` | cache 只暴露当前 generation 的连续 IDR 起始链；新接入、gap、lag 共用全局合并/限速 gate |
| MSE 稳态 | `code-done` | 约 120 ms 目标边缘、最高 1.05 漂移追赶、约 1 s 才 hard seek；queue 同时按预计时长和 bytes 限制 |
| 远控反压 | `code-done` | reader/writer 分离、有界优先级队列；只可淘汰 pointer move；关键输入失败会安全退出并 release-all |
| 输入 ACK/指标 | `code-done` | client sequence、queued ACK、successful SendInput ACK、queue age/depth/coalesce/full 分布 |

M1 的 30 客户端 30 分钟、2 秒慢客户端隔离、3 秒状态回收、健康端 P99 劣化和完整功能回归仍是 `external-hardware-blocked`，不能由单元测试替代。

## 5. M2：编码器能力协商与 CPU 回退

| 规格项 | 状态 | 代码证据 |
|---|---|---|
| 硬件 MFT 枚举 | `code-done` | `MFTEnumEx` 候选及名称、async、hardware URL/driver 诊断；`MFT_ENUM_ADAPTER_LUID` 按 8 字节 blob 解析并与输入 D3D11 adapter 精确匹配，缺失/不匹配在激活前拒绝 |
| 异步 adapter | `code-done` | 单一挂起的 `BeginGetEvent/EndGetEvent` 回调通过有界 credit 驱动 `NeedInput/HaveOutput`，无 1 ms 忙轮询；销毁时显式 `IMFShutdown` |
| CodecAPI 矩阵 | `code-done` | 每属性记录 `IsSupported`、`IsModifiable`、请求/回读值与 HRESULT |
| 码控与低延迟属性 | `code-done` | LowDelayVBR→CBR→PeakConstrainedVBR，协商 buffer/max/reference/CABAC/B=0/dynamic bitrate/force keyframe；B=0 不可写时要求 Baseline 输出类型 + 无 B-slice 自检双重兜底 |
| 启动自检 | `code-done` | 动态 NV12 图案、SPS/PPS/IDR、Baseline profile、B-slice/时间线检查和独立 decoder，整体期限 1.8 秒 |
| GPU surface 自检 | `code-done` | 同 device、同生产纹理描述的动态 DXGI surface；buffer 设置有效 `CurrentLength`，初始化纹理 flush 后经 MFT 与独立解码门禁 |
| 软件回退 | `code-done` | SSSE3 BGRA→NV12 与标量一致性测试；软件 scaler 最高降级到 1080p30 |
| 失败诊断 | `code-done` | 候选创建、运行时编码、无输出均关闭 active；状态保留最多 16 条结构化候选报告并另报总尝试数，全部报告同时写日志 |

当前开发机的 system-memory Media Foundation 编码→独立解码自检已确认使用同一个 `NVIDIA H.264 Encoder MFT` 并通过。真实 `BGRA -> VideoProcessor NV12 -> DXGI buffer -> MFT` 集成门禁在 GTX 1660、1280×720、15 FPS 下未通过：该 NVIDIA activation 未暴露有效 `MFT_ENUM_ADAPTER_LUID` blob，现于 `adapter_match` 阶段快速拒绝并回退 CPU/SIMD。修复前强制设置 device manager 的 A/B 证据为 `ProcessInput` 成功后约 1.8 秒没有后续 `NeedInput/HaveOutput`；动态纹理改用 `D3D11_BIND_VIDEO_ENCODER` 也没有改变结果。由此只能判定当前驱动组合不可安全直连，不能计作 NVIDIA 零拷贝通过。三代 Intel、其他 NVIDIA 驱动和 AMD 矩阵与长稳仍为 `external-hardware-blocked`。驱动内核调用永久挂死无法由 Rust 用户态线程安全抢占，必须依靠目标机故障注入和进程级看护验证。

### 5.1 真机测试发现并修复的两个编码器缺陷（2026-07-29，Intel 目标机）

在一台 Intel 核显目标机上实测时，`/status` 的 `h264_media.error` 显示 §5.2 的降级链**四级全部失败**，只剩 MJPEG，导致观看端无论选哪种传输拿到的都是 MJPEG（1080p/10 FPS、55–60 Mbps），四种传输的对比数据因此完全无效。逐级死因：

1. Intel QSV（GPU DXGI 直连）：未暴露有效 adapter LUID，按 §6.1 拒绝——设计行为，正确。
2. Intel QSV（系统内存）：自检 24 ms 后 `ProcessOutput` 返回 `0xC00D6D61`。
3. `H264 Encoder MFT`（软件）：`MFT_MESSAGE_COMMAND_FLUSH` 返回 `E_FAIL (0x80004005)`。
4. `Microsoft software encoder`：同上。

**缺陷一**：`0xC00D6D61` 是 `MF_E_TRANSFORM_STREAM_CHANGE`，属于 MFT 的正常协议事件（要求重新协商输出类型后继续产出），硬件编码器在起始若干帧后常触发。原实现只处理 `MF_E_TRANSFORM_NEED_MORE_INPUT`，其余一律当硬失败，把可用编码器整体否掉；同一文件的独立解码器路径此前已正确处理该事件。现按协议 `GetOutputAvailableType` → `SetOutputType` 重新协商并重试一次，仍变化才判失败。重新协商后复用 `b_frame_configuration_confirmed` 重新校验 §2.2 的时间线前提，不满足仍然拒绝，避免协商到带 B 帧的 profile 破坏 DTS=PTS 假设。

**缺陷二**：初始化时在 `MFT_MESSAGE_NOTIFY_BEGIN_STREAMING` 之前发送了 `MFT_MESSAGE_COMMAND_FLUSH`。刚激活并完成类型协商的 MFT 没有可丢弃的缓冲数据，该调用本身多余，而 Microsoft 软件 H.264 MFT 对未开始流传输的 FLUSH 返回 `E_FAIL`，把最后一级保底也打掉。已删除该调用。

新增真机门禁 `windows_software_h264_encoder_passes_startup_self_test`：以 `hardware_allowed=false` 强制走软件 MFT，在任何 Windows 上可执行。在本机验证过它确实能捕获缺陷二——临时恢复那行 FLUSH 后，该测试以与目标机逐字相同的 `Encoder failed MFT_MESSAGE_COMMAND_FLUSH message (0x80004005)` 失败；删除后自检通过（8 个访问单元、SPS/PPS/IDR、时间线单调、Baseline profile 确认、0 B-slice、独立解码 8 帧）。

缺陷一只能在带 Intel 核显的目标机复验：本开发机为 i5-10400F + GTX 1660，没有触发 `STREAM_CHANGE` 的硬件路径，因此修复的有效性尚待该机型确认。

## 6. M3：WGC GPU 管线

| 规格项 | 状态 | 代码证据 |
|---|---|---|
| 同一 D3D11 device/context | `code-done` | WGC、VideoProcessor、DXGI device manager 与 MFT surface input 共享设备 |
| NV12 texture pool | `code-done` | 三槽池与显式 acquire/release；对应 `ProcessOutput` 后才回收，flush/error 路径释放 |
| GPU BGRA→NV12/缩放 | `code-done` | `VideoProcessorBlt` 在 capture frame close 前执行 |
| 色彩一致性 | `code-done` | VideoProcessor 与 MFT 类型显式使用 full RGB、BT.601、16–235 NV12，匹配 CPU scalar/SIMD fallback |
| 避免全帧 readback | `code-done` | WGC + H.264 且无 MJPEG 时不做 staging/map/CPU BGRA→NV12 |
| 黑帧看门狗 | `code-done` | 64×36、2 FPS 异步 query/readback，不重新引入每帧全量读取 |
| 可靠回退 | `code-done` | VIDEO_SUPPORT/device/preprocess/MFT 失败均回 CPU/SIMD/MJPEG并暴露 reason/count |
| MJPEG 动态接入竞态 | `code-done` | mid-frame consumer 接入时不再 `expect` panic；本帧跳过并标记下帧 readback |
| GPU 指标 | `code-done` | preprocess P50/P95/P99/max、backpressure drops、fallback count/reason、active |
| 真实 GPU→MFT 闭环 | `external-hardware-blocked` | 本机 GTX 1660/NVIDIA MFT 因缺失有效 adapter LUID 在 `adapter_match` 阶段拒绝并安全回退；仍需带可匹配 LUID 的 Intel/NVIDIA/AMD 目标驱动各自通过 |
| DXGI 零拷贝 | `not-applicable` | 本轮明确不重写 Desktop Duplication；继续 CPU/SIMD fallback |

Intel/NVIDIA/AMD、1080p/4K、真实颜色与缩放、锁屏/恢复、分辨率切换、三槽背压和注入失败仍为 `external-hardware-blocked`。

### 6.1 30/60 FPS 实验档

§6.2 要求"最终提供 30/60 FPS 可选"，此前服务端把 FPS 硬限制在 1–30，界面滑块也只有 5–30，导致 §3.3 的 60 FPS 实验组和 §6.3 的双档对比数据无法采集。现在：

- `screen_share_start` 通过 `validate_capture_fps` 接受 1–60 FPS，错误文案同步为 `FPS must be 1-60`。
- `capture_frame_interval` 改用微秒：30 FPS 为 33_333 µs、60 FPS 为 16_666 µs，避免此前 `1000/fps` 毫秒整数除法把两档实际抬到 30.3/62.5 FPS，从而污染对比数据；越界与 0 值仍产生有限节拍。
- 界面新增独立的"60 FPS（实验）"复选框，常规滑块保持 5–30 FPS；开启后滑块禁用、显示生效帧率，取值随共享设置持久化。中英文文案均说明它会提高编码/传输/解码负载且软件回退仍限制 1080p30。
- 默认值没有改变，也没有引入自适应策略。默认档位仍是 `decision-pending`，必须由现场 `fps_default_decision` 证据决定。

## 7. M4：传输原型与选型

### 7.1 WebCodecs/WSS

- 40-byte `FSTW` v1 header，包含 generation、sequence、timestamp、duration、key/delta/discontinuity 和 payload length；payload 是一个完整 AVCC access unit。
- hello 携带 codec、width/height 和准确的 `AVCDecoderConfigurationRecord`；generation/sequence/duration/payload 做严格校验。
- 客户端 generation 变化时 reset/reconfigure；IDR 前、gap 后和 decode queue 过载时丢 delta 并请求全局合并/限速 IDR。
- 服务端遇到 broadcast `Lagged` 时从权威媒体状态重读最新 descriptor/generation，不清空可能含唯一 Reset 的保留事件，避免永久等待旧 generation。
- `VideoFrame` draw 后立即 `close()`；单 pending rAF 只把 paint 前最后一帧计为 presented，其余计 `droppedBeforePresentation`。
- `/media/webcodecs/ws` 复用鉴权、全局 viewer lease、媒体 trace 与 H.264 AU，不二次编码。
- 明文 LAN 页不具备 WebCodecs 安全上下文；当前内置服务没有自动 TLS 终止。无法使用 WSS/`VideoDecoder` 时 UI 明确诊断并回退 MJPEG。证书部署不是已解决事项。

### 7.2 receive-only WebRTC

- 可选 feature：`screen-share-webrtc-prototype`；默认构建未开启时，选择该传输会在启动阶段明确失败。
- 当前正式裸 EXE 为控制约 7–8 MiB 的依赖增量而默认不编译该 feature，桌面界面通过后端编译能力自动隐藏 WebRTC；代码、测试和显式 feature 均保留，待 H.264/MSE 60 FPS 实测收益不足时可重新开启。
- 不配置 STUN/TURN，仅 host candidate；复用单份 H.264 AU，转换为 Annex-B/RTP，不重复编码。
- 支持 NACK/PLI/FIR 并接入全局 IDR gate；显式注册 sender TWCC interceptor 并统计反馈；offer 复用 Cookie/preview 鉴权与全局 40 viewer/IP lease。
- capture sequence 缺口通过空 sample 只推进 packetizer timestamp、不发空 RTP 包；依赖行为已有单测锁定，避免丢帧后时间轴被压缩。
- 注册 WebRTC 实验性 Absolute Capture Time；只在最终 SDP answer 含协商 extmap 时为真实 AU 写入 64-bit UQ32.32 NTP capture time，并暴露注册、协商 offer 和成功交付样本计数。
- 未挂载的无鉴权备用 Axum router 已删除，信令只保留主服务器的鉴权、body limit 与 viewer lease 入口。
- 首次连接 15 秒、重连断开 5 秒生命周期限制；peer 结束释放 lease，screen-share stop 会 shutdown peers。
- 客户端连续采集呈现回调与扩展 `getStats()` 指标；`expectedDisplayTime` 优先，并从 answer SDP 与回调样本报告 Absolute Capture Time 验证状态。目标浏览器与 capture sequence 尚未交叉验证前，`captureTime`/`receiveTime` 只作为浏览器代理，精确 capture-to-display/input-to-visible 仍标为 unavailable。

### 7.3 选型仍未完成

WebRTC 在开发机 Chrome 的普通页面以及 Chrome/Edge 150 的独立 headless profile 中，均已在物理 LAN 明文 HTTP 地址完成“可构造且 WebCodecs 不可用”的窄验证；这只能确定原型优先级，不能决定产品默认。必须在 Broadwell/Skylake/10 代 Intel、受管 Edge/现场策略、真实媒体连接和 30 独立设备比较端到端延迟、资源、bitrate、网络损伤恢复、加入/离开 IDR 尖峰及运维成本。只有明显优于 MSE 且可运营才替换；保留 MSE 是合法结论。

## 8. 现场证据门禁

完整规格审计有九个门禁。`startup_matrix`、`m0_latency_input_visible`、`wgc_stability_recovery` 消费各自工具生成的输出；其余六项是现场人员编写的报告。此前审计对这六项只检查 `scope` 与 `status`，意味着六个两字段 JSON 就能把 `spec_completion` 置为 `passed`。这与"不得通过降低验收定义关闭门禁"冲突，因此已改为结构化校验：

| 门禁 | 结构化要求（对应规格条款） |
|---|---|
| `performance_matrix` | §8.2 全部十项分布 + 帧计数 + 输入队列 + 双端资源；§3.3 三代 Intel、四种捕获后端、四类场景、720p30/1080p30、1/5/20/30 客户端；§6.3 的 30/60 FPS 双档 |
| `independent_viewing_devices` | §3.3 至少 20 台去重且 `independent_hardware=true` 的设备、标签页替代必须显式否认；§4.6 的 30 分钟、healthy lag=0、3 秒状态回收 |
| `managed_browser_external_media` | §7.3 受管浏览器与 policy scope、真实媒体呈现帧、独立外部 peer；§7.1 证书信任/轮换/profile 清理/DHCP 变更；同浏览器 loopback 明确不算 |
| `network_impairment_recovery` | §7.4 至少 loss 与 jitter 注入、恢复分布与画面连续性；阈值以审计 manifest 为准，报告不能放宽自己的阈值 |
| `transport_selection` | §7.4 三种传输同条件对比、30 客户端、三代 Intel；替换 MSE 必须声明显著优势与运维可接受；§6.2/§6.3 的 `fps_default_decision` |
| `feature_regression` | §8.3 十二项功能/后端回归与中英文文案同步 |

所有百分位统一按 §8.2 校验：p50 ≤ p95 ≤ p99、累计/保留/容量计数非负、`retained_sample_count` 不得超过容量或累计数、`measurement_scope` 非空、呈现类 run 必须带 `presentationTraceSource`。

`pnpm benchmark:screen-share:field-evidence` 可在汇总前单独预检一份报告，完整审计会重复同一套校验，因此预检不能替代审计。该门禁校验结构、覆盖与阈值，不能判断数字是否真的来自目标硬件；`scripts/screen-share-field-evidence.fixtures.mjs` 只是测试样例，不得作为测量结果提交。字段清单见 `screen-share-field-evidence-gates.md`。

## 9. 自动化证据

本轮已取得的定向证据：

- `pnpm test:screen-share-web`：85/85 通过（MSE、session、时钟跳变/周期校准、WebCodecs、WebRTC、diagnostics、UI 状态）；新增三项锁定断线重连恢复：MSE 跳过断线前缓冲帧、只在超过边界的呈现帧结算且稳态不重复计数，主动停止不计断线，WebCodecs 在重连后第一帧绘制结算。
- `pnpm check:screen-share-web`：通过。
- `cargo check --bin app`：通过。
- `cargo check --features screen-share-webrtc-prototype --bin app`：通过。
- Rust `cargo test --bin app` 全量：611 passed、3 ignored、0 failed（三个 ignored 均为需目标硬件显式执行的真机门禁：system-memory MF、软件 MFT、GPU DXGI surface）。
- Rust `cargo test --bin app screenshare`：139 passed、2 ignored；两个显式真机门禁分别覆盖 system-memory Media Foundation 编码→独立解码，以及 `BGRA -> VideoProcessor NV12 -> DXGI surface -> MFT`。前者已在当前开发机 1/1 通过，后者在 GTX 1660/NVIDIA MFT 上按预期记录为未通过并触发 CPU/SIMD 回退；普通测试覆盖 LUID blob 解析、精确匹配、畸形长度拒绝，以及新增的 1–60 FPS 范围与微秒采集节拍。
- Rust WebCodecs wire/hello 测试包含在上述套件；WebRTC feature 定向测试：13/13 通过，包括 TWCC/RTP 时间轴、Absolute Capture Time 编码/解析和真实 MediaEngine offer/answer 协商门禁。
- `cargo test --bin app --features screen-share-webrtc-prototype screenshare` 合并门禁：152 passed、2 ignored；两个 ignored 项均为上述必须按目标 GPU/驱动矩阵显式执行的真机门禁。
- `screenshare_media`：27 passed、2 ignored；system-memory Windows Media Foundation 自检另行 1/1 通过，真实 DXGI surface 门禁在当前 GTX 1660 上于 `adapter_match` 阶段失败并完成候选拒绝和 CPU/SIMD 安全回退验证；候选报告和 B=0/Baseline 双门禁已覆盖。
- `screenshare_gpu`：11/11 通过。
- `pnpm test:screen-share-benchmark`：20/20 通过；fan-out 子集与完整矩阵范围不会混淆，浏览器 probe 的地址/版本/参数解析也有门禁。
- `pnpm test:screen-share-field-evidence`：11/11 通过；覆盖"两字段报告不能通过任何门禁"、完整结构 fixture 全部通过、分布计数/保留量非法、矩阵覆盖缺失、标签页冒充独立设备、证书运营未验证、manifest 阈值收紧优先于报告自带阈值、替换 MSE 需显著优势与运维接受、回归项与中英文文案，以及 CLI 退出码与 `--collect-only` 不改写结果。
- `pnpm test:screen-share-spec-evidence-audit`：9/9 通过；新增"仅声明 status 不能关闭现场门禁"和"manifest 阈值可以判定报告自称通过的门禁为 failed"两项。
- `node --test src/pages/ScreenSharePage.test.mjs`：7/7 通过；新增两项锁定 60 FPS 实验开关、滑块仍为 5–30、启动与持久化使用生效帧率。
- `pnpm benchmark:screen-share:browser -- --browser all --host-ip 192.168.0.111 --output artifacts/screen-share-benchmarks/browser-capabilities-codex-2026-07-28.json`：本机 Chrome/Edge 150 的明文物理 LAN capability 产物已生成；只计构造能力证据。
- `pnpm benchmark:screen-share:environment -- -Output artifacts/screen-share-benchmarks/environment-codex-2026-07-28.json`：环境证据已生成；该文件不代表目标硬件矩阵。
- `pnpm benchmark:screen-share:qualification`：已提供目标编码主机的一键资格验证入口，固定执行环境采集、Web 产物构建、system-memory MF、GPU DXGI surface 和浏览器 probe；各步骤独立日志并受进程级超时保护。schema v3 保留旧 candidate reports，并增加输入 DXGI adapter identity、规范化 activation LUID/`luid_match`、candidate parse 计数、artifact hashes 和仅在集成自检观察到三槽全部回收后才为 true 的 `pool_recycled` 断言。当前开发机实跑得到 `qualification_status=failed`：同一个 `NVIDIA H.264 Encoder MFT` 的 system-memory report 为 admitted，GPU report 在 `adapter_match` 因 LUID 缺失被拒绝，其余步骤通过；严格模式返回 1，`-CollectOnly` 返回 0 但不篡改 JSON，主动跳过步骤的验证返回 2。
- `pnpm benchmark:screen-share:resource-sample`：本机以 explorer 进程实跑 12 秒验证，得到 12 个 CPU/内存样本、2 个 GPU 样本、`status=passed`、退出码 0；目标进程不存在时报错并返回 1。该产物只描述被采样进程，不代表任何目标硬件矩阵项通过。
- `pnpm test:share-web`：33/33 通过。
- 新增真机门禁 `windows_software_h264_encoder_passes_startup_self_test`（软件 MFT，任何 Windows 可执行）：本机 1/1 通过；临时恢复缺陷二的 FLUSH 调用后可稳定复现目标机的失败信息，证明该门禁确实能捕获它。
- 全仓 ESLint 与 `git diff --check`：通过；后者只有 CRLF 工作树提示。`cargo fmt --check` 在本轮改动的文件上无差异，但工作树中未涉及本任务的 `device_simulator/preflight.rs` 与 `ums_init_password.rs` 仍有待格式化的差异，属于其他改动的范围。

合并后的 `pnpm check`、`pnpm check:screen-share-web`、`pnpm lint`、`pnpm build`、`cargo check --bin app` 与 WebRTC feature check 均已通过。涉及目标硬件和真实浏览器的门禁仍必须按 §10 单独执行。

构建顺序有一个项目级约束：`screenshare_web_assets.rs` 通过 `rust-embed` 在 Rust 编译期读取 `dist/screen-share-web`。因此 CI/本地合并门禁必须先完成 `pnpm build:screen-share-web`，再启动 Cargo 编译；不能让 Vite 清理/重建该目录与 Cargo derive 并发，否则会产生资源接口未生成的瞬时编译失败。

当前环境产物显示该机为 i5-10400F + NVIDIA GTX 1660，并同时安装虚拟显示适配器；10400F 没有 Intel 核显，因此这次真实 MF 自检只能证明本机可用候选与软件回退门禁工作，不能计入“10 代 Intel iGPU 编码器”验收。

## 10. 外部验收清单

以下任一缺失时，规格目标不能标记为“全部完成”：

1. Broadwell、Skylake、10 代 Intel 的 MFT capability、自检、B=0/时间线、回退和 30 分钟报告。
2. Intel/NVIDIA/AMD 的 WGC GPU 管线、DXGI surface admission、1080p/4K、锁屏/重配置、黑帧探针和故障注入报告；本机 GTX 1660 当前为明确失败样本，不能以 CPU fallback 通过替代。
3. 受管 Chrome/Edge/现场策略下 HTTP WebRTC 与 HTTPS/WSS WebCodecs 的真实媒体、证书、DHCP/IP、profile 清理和现场网段报告；本机 headless constructor probe 不能替代。
4. 1/5/20/30 健康客户端、停止读取客户端、20–30 独立设备和网络损伤报告。
5. capture-to-display、input-to-SendInput、受控 input-to-visible、live-edge、资源和 bitrate 的原始 JSON 与 P50/P95/P99。
6. 批注、申请/授权/撤销、键鼠 release-all、光标、多屏、WGC/DXGI/RDP、隐私/黑屏恢复回归。
7. 基于同条件数据完成 MSE/WebCodecs/WebRTC 选型和 30/60 FPS 决策；60 FPS 实验档已可在界面开启，缺的是双档现场数据。

这些门禁是环境与产品决策依赖，不应通过降低验收定义、把浏览器统计冒充端到端指标或使用单机标签页冒充独立设备来关闭。

第 1–7 项现在都有对应的结构化报告位置：按 §8 的字段要求产出报告，再由 `pnpm benchmark:screen-share:spec-evidence-audit` 汇总。只有九个门禁全部 `passed` 时审计才会输出 `spec_completion=passed`；在此之前审计固定输出 `not_evaluated`，任何“已完成整份规格”的说法都缺少依据。
