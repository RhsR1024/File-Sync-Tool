# 屏幕共享延迟与扇出基线报告模板

> 对应 `screen-share-latency-optimization.md` 的 M0/M1。本文档是填写模板，不包含未经实测的性能结论。

## 1. 运行信息

| 字段 | 结果 |
|---|---|
| 日期 / 构建版本 / Git commit | 待填写 |
| 共享端 CPU / GPU / 驱动 / Windows | 待填写 |
| 捕获后端（WGC / DXGI） | 待填写 |
| 传输（mse-h264 / mjpeg / web-codecs / web-rtc） | 待填写 |
| 分辨率 / FPS / 质量档位 | 待填写 |
| 60 FPS 实验开关是否启用（§6.2/§6.3 双档对比） | 待填写 |
| 场景（静态 / 动态 / 视频 / 快速滚动） | 待填写 |
| 网络拓扑 / RTT / 丢包 / 抖动 | 待填写 |
| 健康客户端 / 独立设备数量 | 待填写 |
| 停止读取客户端数量 | 待填写 |
| 浏览器 / 是否受管 / secure context | 待填写 |
| 原始 JSON 路径 | 待填写 |
| 环境 JSON / 浏览器 diagnostics 路径 | 待填写 |

先在共享端采集硬件、驱动、Windows、浏览器版本和受管 Edge policy scope/策略名称（不导出可能含内部 URL 的策略值）；该产物只描述环境，不证明 GPU 管线已启用：

```powershell
pnpm benchmark:screen-share:environment -- -Output artifacts/screen-share-benchmarks/environment-host-a.json
```

在每台目标浏览器主机上保存明文 LAN capability 产物（必须使用该机真实非 loopback IPv4）：

```powershell
pnpm benchmark:screen-share:browser -- --browser all --host-ip 192.168.1.20 --output artifacts/screen-share-benchmarks/browser-capabilities-host-a.json
```

该 probe 使用独立 headless profile。除 secure-context 与构造/API 可见性外，`webrtc_loopback_media` 还以 canvas 合成视频完成同一浏览器内的 offer/answer、ICE/DTLS/RTP、远端 video frame 与 inbound RTP stats。它仅是本地 synthetic media loopback 的补充证据；`managed_browser_external_acceptance` 固定为 `false`，受管 policy、真实用户 profile、证书、LAN 外部 peer、真实屏幕媒体和独立设备仍要另测。

编码主机可以用统一资格验证入口按固定顺序完成环境采集、Web 产物构建、system-memory MF 自检、GPU DXGI surface 自检和浏览器 capability probe：

```powershell
pnpm benchmark:screen-share:qualification -- -HostIp 192.168.1.20 -OutputDirectory artifacts/screen-share-benchmarks/qualification-host-a
```

跨主机的目标主机启动资格可以用 schema v1 manifest 汇总；report 路径相对于 manifest 所在目录解析。该聚合器只评估 `target-host-startup-qualification`，输出固定为 `spec_completion=not_evaluated`，不把启动自检或 browser constructor probe 解释为整份屏幕共享规格通过：

```json
{
  "schema_version": 1,
  "expected_git_commit": "optional-commit",
  "runs": [{
    "id": "intel-broadwell-a",
    "report": "intel-broadwell-a/qualification.json",
    "required": true,
    "roles": ["m2:intel-broadwell"],
    "expected": { "gpu_vendor": "intel" }
  }]
}
```

```powershell
pnpm benchmark:screen-share:qualification:aggregate -- --manifest artifacts/screen-share-benchmarks/qualification-matrix.json --output artifacts/screen-share-benchmarks/qualification-matrix-result.json --markdown artifacts/screen-share-benchmarks/qualification-matrix-result.md --require-clean
```

完整规格的只读 evidence audit 使用独立 manifest。它固定检查 startup matrix、M0 latency/input-to-visible、WGC 长稳与恢复、性能矩阵、20-30 独立观看设备、受管浏览器外部媒体、网络损伤恢复、传输选型和功能回归；每项引用报告必须使用对应固定 `scope` 并声明 `passed`。资格矩阵和 `webrtc_loopback_media` 可作为引用记录，但不会满足完整规格门禁：

```powershell
pnpm benchmark:screen-share:spec-evidence-audit -- --manifest artifacts/screen-share-benchmarks/full-spec-evidence.json --output artifacts/screen-share-benchmarks/full-spec-evidence-result.json --markdown artifacts/screen-share-benchmarks/full-spec-evidence-result.md
```

M0 使用单独的 gate manifest，分别引用 `m0-latency-samples` 百分位报告和 `input-to-visible-causal-evidence` 报告。只有 capture-to-display、input-to-SendInput、input-to-visible-response 的 P50/P95/P99 均不超过各自阈值，且因果报告明确使用 `pixel`、`optical` 或 `explicit_causal` 并设置 `causal_link=true` 时，M0 gate 才会通过；普通 fan-out/first-media 数字不能替代这些指标或因果证据：

```powershell
pnpm benchmark:screen-share:m0-gate -- --manifest artifacts/screen-share-benchmarks/m0-gate-manifest.json --output artifacts/screen-share-benchmarks/m0-gate.json --markdown artifacts/screen-share-benchmarks/m0-gate.md
```

WGC stability and recovery uses a separate structured gate. It requires a 30-minute capture, resource growth, lock-screen and display-reconfiguration recovery, multi-monitor coverage, black-frame thresholds, recovery-event accounting, frame continuity, and hashed external artifact references. See `screen-share-wgc-stability-gate.md` and run:

```powershell
pnpm benchmark:screen-share:wgc-stability-gate -- --manifest artifacts/screen-share-benchmarks/wgc-stability-host-a/wgc-gate-manifest.json --output artifacts/screen-share-benchmarks/wgc-stability-host-a/wgc-gate.json --markdown artifacts/screen-share-benchmarks/wgc-stability-host-a/wgc-gate.md
```

将生成的 `m0-gate.json` 作为 full-spec manifest 的 `m0_latency_input_visible` gate 引用。`--collect-only` 仅使进程返回 0，不能改写 JSON 或 Markdown 的 recommended exit code。

其余六个门禁是现场报告，由结构化校验器按 §3.3/§4.6/§7.4/§8.2/§8.3 检查字段、覆盖范围和阈值；只写 `scope` 和 `status` 不能关闭任何一项。字段清单见 `screen-share-field-evidence-gates.md`，可在汇总前逐份预检：

```powershell
pnpm benchmark:screen-share:field-evidence -- --gate performance_matrix --report artifacts/screen-share-benchmarks/performance-matrix.json --output artifacts/screen-share-benchmarks/performance-matrix-gate.json
```

`network_impairment_recovery` 的阈值以 full-spec manifest 的 gate 条目为准，报告自带的 `thresholds` 不能放宽它。

```json
{
  "schema_version": 1,
  "gates": [
    { "id": "startup_matrix", "report": "qualification-matrix-result.json" },
    { "id": "managed_browser_external_media", "report": "managed-browser-media.json" }
  ],
  "supplementary": [
    { "kind": "browser-loopback", "report": "browser-capabilities-host-a.json" }
  ]
}
```

它要求六类角色（Broadwell、Skylake、10th Gen 的 M2 system-memory，以及 Intel、NVIDIA、AMD 的 M3 DXGI surface）各自具有结构化候选与自检证据。`--collect-only` 只让命令进程返回 0，不会改写 JSON 的 matrix status 或推荐退出码。M0 延迟/像素因果、30 分钟 WGC 稳定性、1080p/4K/FPS、20-30 独立设备、受管 Edge 的真实 WebRTC/证书/策略、网络损伤恢复、传输选择及完整回归仍需要独立外部实测。

资格生产端当前写出 schema v3。除兼容保留的 v2 字段外，它包含 `run_id`、`source`、`host`、相对 artifact SHA-256、candidate `total/parsed/malformed` 计数以及 `structured_evidence`。GPU self-test 从实际 `IDXGIDevice -> IDXGIAdapter::GetDesc` 导出输入 adapter 的 name/vendor/device/LUID；每条候选还导出规范化 activation LUID 与 `luid_match`。集成自检只有在 encoder flush/drop 后观察到全部 NV12 pool slot 回到 `Free` 时才输出 `pool_recycled=true`。这些事实未被观察到时会写入 `null` 和 `evidence_gaps`，聚合器将报告判为 `incomplete`，而不是从 MFT 名称、环境 GPU 或失败文本中推断通过。

该命令默认是严格门禁：全部步骤通过才返回 0，存在失败返回 1，主动跳过步骤返回 2。需要在已知失败机器上完整收集日志而不让命令失败时增加 `-CollectOnly`；JSON 里的 `qualification_status` 仍会保留 `failed`/`incomplete`，不得因为进程退出码为 0 改写结果。每个子步骤有进程级超时和独立日志，避免驱动调用永久挂死整个采集流程。schema v2 还会把 system-memory 与 GPU DXGI 的结构化 candidate reports 分别写入 `media_foundation`，跨机器汇总时无需从文本日志猜测候选和失败阶段。

再从基准客户端运行 fan-out 子集：

```powershell
pnpm benchmark:screen-share -- --base-url http://192.0.2.10:9870/ --transport mse-h264 --healthy-clients 30 --slow-clients 1 --duration-seconds 1800 --scenario fast-scroll --output artifacts/screen-share-benchmarks/fast-scroll-h264.json
```

WebCodecs wire fan-out 使用相同工具和 `/media/webcodecs/ws`，但该 Node 客户端不解码、不绘制，不能证明浏览器安全上下文、解码或呈现性能：

```powershell
pnpm benchmark:screen-share -- --base-url https://192.0.2.10:9870/ --transport webcodecs-h264 --healthy-clients 30 --duration-seconds 1800 --scenario fast-scroll --output artifacts/screen-share-benchmarks/fast-scroll-webcodecs-wire.json
```

需要脚本以退出码执行子集门禁时增加 `--require-gates`。JSON 中的 `acceptance.scope` 固定为 `fanout_subset`；即使 `fanout_subset_overall=pass`，也不表示 M1 整体通过。浏览器 live-edge/hard seek、IDR storm、远控输入、资源曲线、网络损伤和独立设备矩阵仍需分别给出证据。

工具仅支持未配置用户名/密码的受控测试共享。媒体端点返回 HTTP 401 时，工具会明确报告鉴权限制；不要把密码或共享 Cookie 写入命令行、日志或报告。

## 2. 媒体客户端与扇出

从 JSON 的 `healthy_clients`、`slow_clients` 和 `outbound_receive_windows` 填写：

| 指标 | P50 | P95 | P99 | Max / Count |
|---|---:|---:|---:|---:|
| 健康客户端连接时间（ms） |  |  |  |  |
| 健康客户端首帧时间（ms） |  |  |  |  |
| 媒体帧到达间隔（ms） |  |  |  |  |
| 100 ms 接收窗口 bitrate（bps） |  |  |  |  |
| 1 s 接收窗口 bitrate（bps） |  |  |  |  |
| 健康客户端意外断线 | — | — | — |  |
| 慢客户端握手后停止读取 | — | — | — |  |
| 慢客户端断线观测时间（ms） |  |  |  |  |
| 断开后 viewer / IP / task 回收（ms） |  |  |  |  |

对照 `status_before` / `status_after` 记录服务端 `viewer_count`、`viewer_ip_reference_count`、`active_media_task_count`、`media_metrics`、`h264_media` 的计数差值，特别检查健康客户端 lag、send timeout、disconnect 和 IDR 次数。停止读取客户端必须使用脚本的 raw TCP/TLS 模式；浏览器后台标签页不能替代该故障注入。

接收窗口统计的是 MJPEG HTTP body bytes 或 H.264 WebSocket message payload bytes，不包含 TCP/IP、TLS 和 WebSocket framing 开销；若验收需要线速率，另配抓包或网卡计数器。

## 3. 端到端时间线（需浏览器与服务端 trace 补充）

该脚本不解码、不显示画面，因此以下结果不能从接收窗口推算，必须使用同一 capture/input sequence 的客户端和服务端事件关联：

| 指标 | P50 | P95 | P99 | 累计 / 保留 / 容量 | scope / source |
|---|---:|---:|---:|---:|---|
| capture-to-display（ms） |  |  |  |  |  |
| input-to-SendInput（ms） |  |  |  |  |  |
| input-to-visible-response（ms） |  |  |  |  |  |
| live-edge distance（ms） |  |  |  |  |  |
| input queue age（ms） |  |  |  |  |  |
| `WebSocket.bufferedAmount`（bytes） |  |  |  |  |  |
| 断线重连恢复（ms） |  |  |  |  |  |

断线重连恢复来自播放器指标 `reconnectRecoveryMs` 与 `unexpectedDisconnectCount`：计时从非人为断线开始，到重连后的画面重新呈现为止。MSE 会跳过断线前已缓冲、断线后继续呈现的帧，只有 `mediaTime` 超过断线时刻缓冲末端的帧才结算；WebCodecs 没有播放缓冲，重连后第一帧绘制即结算。主动停止和终止性失败不产生样本。WebRTC 原型不自动重连，该行留空并注明。

时钟和呈现来源必须单独记录：

| clock / presentation 字段 | 结果 |
|---|---|
| clock sample count / RTT / offset | 待填写 |
| offset range / last offset / discontinuity count | 待填写 |
| MSE `presentationTraceSource` | 待填写（`expected-display-time` / `callback-time`） |
| WebCodecs `presentationTraceSource` | 待填写（应为 `animation-frame-pre-paint-proxy`） |
| WebRTC `presentationTraceSource` | 待填写 |
| WebRTC Absolute Capture Time registered / negotiated offers / sent samples | 待填写；来自服务端 `webrtc` 指标 |
| WebRTC Absolute Capture Time 客户端 validation | 待填写（`not-negotiated` / `awaiting-browser-sample` / `pending-target-browser-correlation`） |
| WebRTC browser capture→display proxy | 待填写；不得填入权威 capture-to-display 行 |
| WebRTC browser receive→display proxy | 待填写 |

不要把各阶段 P99 相加当作端到端 P99。

`input-to-visible-response` 当前是受控场景代理：客户端输入序列只允许关联到 `capture timestamp >= successful SendInput timestamp` 的首个实际呈现帧。该规则能排除“输入发生在捕获之后却被倒挂到旧帧”的假低延迟，但不能仅凭序列号证明任意画面的像素变化由该输入造成。验收时应使用点击后唯一变色、固定位置计数器等可判定场景；否则该项必须标为不可用。

## 4. 资源与功能回归

共享端进程的 CPU、GPU、工作集和句柄可以直接采样，输出 `scope: "host-resource-usage"` 的百分位产物；观看端资源仍需在观看设备上另行测量：

```powershell
pnpm benchmark:screen-share:resource-sample -- -ProcessName app -DurationSeconds 1800 -Label perf-matrix-run-a -Output artifacts/screen-share-benchmarks/resource-usage-host-a.json
```

CPU 按逻辑核归一化；GPU 取该进程 GPU engine 计数器之和，默认 5 秒采一次（枚举实例较慢，不与 CPU/内存同频）。计数器不可用时会写入 `evidence_gaps` 并返回 2，不会用 0 冒充测量值。

| 项目 | 基线 | 优化后 | 结论 |
|---|---:|---:|---|
| 共享端 CPU / GPU / 内存 |  |  |  |
| 观看端 CPU / GPU / 内存 |  |  |  |
| outbound bitrate |  |  |  |
| dropped / presented frame ratio |  |  |  |
| 30 分钟 hard seek |  |  |  |
| IDR 数量 / 单 IDR 大小 |  |  |  |
| 编码器候选报告总数 / 保留数 |  |  |  |
| 最终编码器 B=0 属性或 Baseline profile 证据 |  |  |  |
| 批注、授权/撤销、粘键释放 |  |  |  |
| 光标、多显示器、黑屏恢复 |  |  |  |

## 5. 覆盖矩阵与未覆盖项

逐项列出 Broadwell、Skylake、10 代 Intel，WGC、DXGI、RDP、Basic Display Adapter/无硬编，以及 20–30 台独立设备最终验收的完成情况。每台编码主机需保存 `h264_media.encoder_candidate_reports`、`encoder_candidate_report_total_count` 和应用日志；若总数大于保留容量，日志是超出部分的权威证据。Node 单机客户端只能用于扇出与反压初测，不能代替真机、真实浏览器解码/呈现、远控可见响应或网络损伤测试。

Windows GPU surface 门禁按目标主机显式执行并保存完整输出；通过条件是测试本身通过，不能把“候选拒绝后 CPU fallback 可用”填写为 GPU 零拷贝通过：

```powershell
cd src-tauri
$env:RUST_LOG='info'
cargo test --bin app --features screen-share-webrtc-prototype screenshare_media::tests::windows_gpu_preprocess_and_mf_dxgi_encoder_passes_integration_self_test -- --ignored --exact --nocapture
```

报告必须保留输入 D3D11 adapter 与 MFT activation 的 LUID。`MFT_ENUM_ADAPTER_LUID` 是 blob，不是字符串；缺失、畸形或与输入 adapter 不匹配时，门禁应在 `adapter_match` 阶段失败，不得强行设置 DXGI device manager 后继续。

| GPU / driver / adapter | 尺寸 / FPS | DXGI surface admission | 独立解码 | pool 回收 | 失败阶段与回退 |
|---|---:|---|---|---|---|
| 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 |

## 6. 阈值校准与阶段结论

- 采用或调整的 M1 阈值及原始证据：待填写。
- 慢媒体客户端是否在 2 秒内隔离，健康客户端 P99 劣化是否不超过 20%：待填写。
- 健康 30 客户端、30 分钟是否 `Lagged = 0` 且状态无持续增长：待填写。
- 30 FPS 与 60 FPS 实验档的 capture-to-display、CPU/GPU、掉帧和 30 客户端扇出对比结论，以及据此选择的默认档位：待填写。该结论需同时写入 `transport_selection` 报告的 `fps_default_decision`，并引用性能矩阵的 run id。
- 是否满足进入 M2/M3/M4 的数据门禁；若否，缺失证据：待填写。

## 7. 浏览器端诊断快照

在观看页面的开发者工具 Console 中执行以下命令，复制结果到本地基线附件：

```js
window.__SCREEN_SHARE_DIAGNOSTICS__.snapshot()
```

快照只读取当前传输类型、服务端计数器和浏览器播放器指标，不包含画面、账号、Cookie、连接 IP 或按键内容，也不会自动上传或持久化。WebCodecs 结果应同时保留安全上下文/WSS 可用性、播放器 `state`、解码队列、paint 前覆盖丢帧及端到端 trace；MSE 与 WebRTC 也必须同时保留各自的 `state` 和 `metrics`。WebRTC 原型还应记录浏览器连接状态和服务端 peer/RTCP 指标。若全局对象不存在，先记录页面版本与构建 commit，不能用肉眼流畅度代替指标。

WebRTC 原型已在 SDP 实际协商后发送 WebRTC 实验性 Absolute Capture Time，并报告服务端发送计数与客户端 `captureTime` 样本状态；但当前仍没有把目标浏览器样本与同一 H.264 capture sequence 交叉验证，因此诊断会把 `endToEndLatency.available` 明确标为 `false`。`requestVideoFrameCallback` 的 `captureTime` / `receiveTime` 只填 browser proxy 行；`getStats()` 的 jitter、RTT、丢包或帧数也不能冒充 capture-to-display / input-to-visible。权威行必须通过目标浏览器 capture-sequence 关联或外部光学/高速相机基准后填写。
