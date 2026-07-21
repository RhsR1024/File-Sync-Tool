# 屏幕共享媒体传输决策：Media Foundation H.264 + MSE

**状态：** 已接受并实现（2026-07-20）

**适用范围：** Windows 共享端、可信局域网、浏览器观看端；公网、音频和无人值守不在本决策范围。

## 背景

原有 MJPEG 链路兼容性高、故障定位简单，但每一帧都是独立 JPEG，出口带宽会随分辨率、帧率和观看者数量快速增长。多人批注、冻结和远程控制已经通过独立的 `/session/ws` 协议与视频解耦，因此媒体链路可以替换，而不改变 `session_id`、`source_epoch`、`frame_id`、批注坐标或控制授权语义。

局域网观看地址使用普通 HTTP，例如 `http://192.168.0.111:9870`。WebCodecs 在浏览器中依赖安全上下文，不能作为这类局域网 HTTP 地址的稳定默认能力。WebRTC 可以进一步降低延迟并提供自适应码率，但需要维护每个 peer 的 RTP/RTCP、拥塞控制和连接生命周期，当前单向查看场景没有足够收益支撑这项复杂度。

## 决策

1. Windows 共享端增加单次编码、多观看者复用的 H.264 链路。
2. 编码使用 Windows Media Foundation H.264 MFT，输入为捕获线程提供的 BGRA 帧，经 NV12 转换后编码。
3. 编码输出从 Annex B 解析 SPS/PPS 和访问单元，再封装为 fragmented MP4：连接先发送 `media.hello` 和 init segment，随后发送可直接追加到 MSE `SourceBuffer` 的 media segment。
4. 浏览器通过同源 `/media/ws` 接收媒体，通过 Media Source Extensions 播放；批注、冻结和远程控制继续使用 `/session/ws`。
5. `auto` 和 `mse_h264` 会尝试启动 H.264；编码器就绪后运行时传输标记切换为 `mse_h264`。显式 `mjpeg` 不启动 H.264；`webrtc` 当前保留枚举但按 MJPEG 路径运行。
6. MJPEG `/stream` 永久保留。编码器启动/运行失败、H.264 尚未就绪、浏览器 `MediaSource.isTypeSupported` 返回 false、媒体 WebSocket/MSE append 失败时，观看端继续或恢复 MJPEG，不中止屏幕共享。
7. WebCodecs 暂不采用；WebRTC 暂缓。若 release 实测无法满足远控反馈延迟，或未来需要音频、自适应码率、公网穿透，再单独评估 WebRTC。

## 关键实现约束

- H.264 输入队列有界，捕获线程不得等待编码器；编码跟不上时只丢弃编码输入，不阻塞 MJPEG、批注或控制。
- 编码器以约 2 秒为周期请求关键帧，并设置对应 GOP 上限。
- 服务端只缓存从最新关键帧开始的 GOP。新观看者先收到最新 init segment 和可独立解码的 GOP，避免从非关键帧开始。
- 媒体代际在分辨率或 SPS/PPS 变化时重建；浏览器销毁旧 `MediaSource`/`SourceBuffer` 后重新初始化。
- MSE 客户端使用串行 append 队列、有限队列长度和指数退避重连；队列持续落后时主动失败并回退 MJPEG。
- H.264 WebSocket 每发送一个分片后让出约 1ms 调度时间，并保留 Ping/Pong/Close 读取分支，避免大量观看者发送时饿死控制面和连接清理。
- 观看者日志文件写入放入阻塞线程池，不能占用 Tokio 异步执行线程。
- 指标至少包括编码帧/字节、关键帧数、缓存分片/字节、编码输入丢帧、慢客户端丢帧、首媒体时间、实际 FPS、码率和观看者数量。

## 验证结果

- Chrome/Edge 在 Windows 局域网 HTTP 地址上通过 MSE 播放 `1920×1080` H.264，Chrome 使用 `D3D11VideoDecoder`。
- 观看端 27 项测试通过；观看端类型检查和生产构建通过。
- `screenshare_media` 5 项 Rust 测试通过；debug 应用构建通过。
- H.264 下本地暂停/恢复通过浏览器级验证；批注、冻结和控制协议不受媒体切换影响。
- debug 版 50 路结果：50 路连接成功，10 秒观测窗口聚合约 `5.067 Mbps`，每观看者平均约 `0.101 Mbps`，编码输入丢帧 `0`，慢客户端广播丢帧 `0`。
- debug 版首个可解码媒体分片 P95 约 `7.96s`；release 复测证明该结果主要受 debug 构建和并发调度开销影响。
- release 版 50 路结果：50 路连接成功，首媒体平均约 `102ms`、P95 约 `210ms`，源编码约 `0.74 Mbps`，50 路聚合约 `35.33 Mbps`，编码输入丢帧 `0`，慢客户端广播丢帧 `0`，连接在测试结束时保持 `50/50`。
- 基准原始记录：debug 为 `artifacts/screen-share-benchmarks/h264-20260720T112338Z.json`，release 为 `artifacts/screen-share-benchmarks/h264-20260720T121719Z.json`。

## 暂不实现

- WebCodecs：局域网普通 HTTP 不是稳定安全上下文。
- WebRTC、STUN/TURN、WSS 和公网穿透。
- 音频、录屏、自适应码率、AV1/HEVC 和多档转码。
- 为每个观看者独立编码；当前保持单次编码、多连接广播。

## 后续触发条件

满足任一条件时重新评估媒体方案：

- release 版首个可解码分片 P95 仍明显超过 2.5 秒。
- 局域网远控点击到下一帧反馈 P95 超过 300ms，且无法通过捕获/编码参数解决。
- 需要音频、自适应码率、移动网络或公网穿透。
- 目标浏览器不支持当前 H.264 MSE MIME，且 MJPEG 带宽不可接受。
