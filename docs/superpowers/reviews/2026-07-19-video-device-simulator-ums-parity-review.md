# 虚拟设备模拟 UMS 业务复核报告

- 复核日期：2026-07-19
- 复核对象：当前 `D:\WorkSpace\File-Sync-Tool` 与旧项目 `D:\WorkSpace\VirtualTools`
- 复核平台：UMS
- 复核范围：实况、HTTP/LAPI/ONVIF、带图告警、恢复告警、图片回取、RTSP/TCP
- 结论：**部分一致，不能写成完全复刻**

## 1. 结论先行

当前实现已经覆盖旧项目中本次范围的六类设备和 114 条告警定义：

| 设备类型 | 当前数量 |
| --- | ---: |
| 自定义报警相机（普通 IPC） | 2 |
| 智能相机 | 71 |
| 结构化相机 | 4 |
| 人脸门禁相机 | 9 |
| 普通 NVR | 25 |
| 车辆识别 NVR | 3 |

114 条定义数量相同，只能证明告警清单覆盖，**不能证明 114 条主请求、后续请求、恢复请求和请求头都逐字段相同**。当前没有 114 条逐请求的旧报文与新报文基准对照，也没有真实 UMS 验收，因此本地结论只能是“部分一致”。

已经按旧代码修正并有本地测试覆盖的重点包括：

- UMS 设备类型范围；
- 六类设备的主要请求路径和请求方法；
- `Reference` 的设备端口/服务器端口差异；
- 智能相机普通两图和车辆抓拍的 `Data`/URL 交叉顺序；
- 告警类型只改写三个明确 JSON 路径；
- 自定义报警相机的 multipart 边界和图片字段；
- RTSP 的 metadata `SETUP`、旧 SSRC、`Content-Base` 和三路实况入口。

仍然不能宣称完全一致的重点包括：图片 URL 的 `Index` 表达方式、人员库查询、H.264/H.265 与旧抓包的 PT0 差异、部分模板常量、全量逐报文对照以及真实 UMS 结果。

## 2. 旧项目和当前代码证据

旧项目逐行检查的主要文件：

- `D:\WorkSpace\VirtualTools\script\VSITool_UI.py`
- `D:\WorkSpace\VirtualTools\script\HTTPServer.py`
- `D:\WorkSpace\VirtualTools\script\CustomAlarm.py`
- `D:\WorkSpace\VirtualTools\script\SmartAlarm.py`
- `D:\WorkSpace\VirtualTools\script\StructureAlarm.py`
- `D:\WorkSpace\VirtualTools\script\ACSAlarm.py`
- `D:\WorkSpace\VirtualTools\script\NormalAlarm.py`
- `D:\WorkSpace\VirtualTools\script\VehicleAlarm.py`
- `D:\WorkSpace\VirtualTools\script\IPCRtspLib.py`
- `D:\WorkSpace\VirtualTools\script\Vsocket_ip.py`
- `D:\WorkSpace\VirtualTools\script\HTTPMethod.py`
- `D:\WorkSpace\VirtualTools\data\dev_type.yml`
- `D:\WorkSpace\VirtualTools\data\alarms_info.yml`

当前代码的主要检查点：

- `src-tauri/src/device_simulator/alarm_runtime.rs:653-854`：从已批准文件加载告警定义；
- `src-tauri/src/device_simulator/alarm_runtime.rs:1026-1063`：告警类型字段改写；
- `src-tauri/src/device_simulator/alarm_runtime.rs:1221-1307`：图片选择和图片 URL 映射；
- `src-tauri/src/device_simulator/alarm_runtime.rs:1612-1700`：六类设备的 `Reference` 和运行字段；
- `src-tauri/src/device_simulator/alarms/mod.rs:1079-1325`：运行时字段和复合请求；
- `src-tauri/src/device_simulator/rtsp/routes.rs:114-151`、`rtsp/service.rs:18-520`：RTSP 和 metadata；
- `src-tauri/src/device_simulator/protocol_runtime.rs:721-742`：图片回取；
- `src/pages/VideoDeviceSimulatorPage.vue:214-590`、`src/locales/messages.ts:3478-3620`：界面和中文/英文文案。

## 3. 六类设备逐项复核

| 设备类型 | 旧项目请求位置 | 当前主要请求 | 图片和恢复 | 本地判断 |
| --- | --- | --- | --- | --- |
| 自定义报警相机（普通 IPC） | `CustomAlarm.py:89-147` | `POST /LAPI/V1.1/System/Event/Notification` | 带图 1 张；无图不带图；旧项目无恢复 | 主要结构已对齐，URL 表达和真实 UMS 结果仍不同/未验收 |
| 智能相机 | `SmartAlarm.py:73-450` | V1.0 `Structure` 后 `Alarm`；V1.1 multipart | 普通 V1.0 两图、V1.1 单图；部分事件有恢复 | 主要结构已对齐；图片 URL、人员/复合请求仍不能判定完全一致 |
| 结构化相机 | `StructureAlarm.py:62-269` | `POST /LAPI/V1.0/System/Event/Notification/Structure` | 人体 2、人脸 4、机动车 5、非机动车 4 个图片对象；旧项目无恢复 | 主要结构已对齐；没有逐条基准对照 |
| 人脸门禁相机 | `ACSAlarm.py:71-277` | 人员核验 `PersonVerification`；控制告警 `Alarm` | 陌生人 1 张、在库人员 2 张；控制告警按旧配置可恢复 | 人员库查询缺失，不能判定完整一致 |
| 普通 NVR | `NormalAlarm.py:63-118` | `POST /LAPI/V1.0/System/Event/Notification/Alarm` | 禁止带图；旧配置的恢复告警已注册 | 主要结构已对齐；通道删除和所有恢复请求未逐条比对 |
| 车辆识别 NVR | `VehicleAlarm.py:63-225` | 匹配/不匹配 `VehicleEventInfo` 后 `Alarm`；抓拍走 `Structure` | 抓拍有 3 个图片对象；前两个按旧脚本动态覆盖，第三个保留模板内容 | 顺序和路径已对齐，但不能称全部字段动态一致 |

## 4. 已核对的请求细节

### 4.1 `Reference` 端口和路径

旧脚本不是统一端口。当前 `build_context` 已按下面的矩阵处理（`alarm_runtime.rs:1612-1670`）：

| 场景 | 旧项目行为 | 当前行为 |
| --- | --- | --- |
| 自定义报警相机 | 设备 IP 固定 `:81` | 设备 IP 固定 `:81` |
| 智能相机 V1.0 普通 JSON | 服务器 IP 和告警接收端口 | 相同 |
| 智能相机 V1.1 multipart | 设备 IP 固定 `:80` | 相同 |
| 结构化相机 | 设备 IP 固定 `:80` | 相同 |
| 人脸门禁人员核验 | 设备 IP 固定 `:80`、订阅编号 `1000` | 相同 |
| 人脸门禁控制告警 | 服务器 IP 和告警接收端口 | 相同 |
| 普通 NVR | 服务器 IP、硬件编号路径和接收端口 | 相同 |
| 车辆匹配/不匹配 | 服务器 IP、硬件编号路径和接收端口 | 相同 |
| 车辆抓拍 | 设备 IP、硬件编号路径和 `:80` | 相同 |

因此不能再把可配置的虚拟设备 HTTP 端口误写成所有告警正文的端口。

### 4.2 告警类型字段

当前只允许改写三个明确路径（`alarm_runtime.rs:1026-1063`）：

- `/EventInfo/Type`
- `/AlarmInfo/AlarmType`
- `/AlarmType`

旧项目的普通 NVR `ChannelDeleted.json` 根节点 `/Type` 是数值 `3`，不是告警名称；当前对 `nvr-common/channel-deleted` 保留该数值。不能用“递归改所有叫 Type 的字段”代替逐路径检查。

### 4.3 图片顺序和图片 URL

- 智能相机普通 V1.0 两图：旧脚本 `Data=[1,0]`，URL 的 `Index=[0,1]`；当前 `select_pack_images`（`alarm_runtime.rs:1221-1307`）保留这一交叉顺序。
- 人员聚集/发散例外：旧脚本 `Data` 和 URL 都是 `[0,1]`；当前保留。
- 高空抛物单图：旧脚本和当前都使用图片 `[1]`。
- 车辆抓拍：旧脚本 `Data=[0,2]`，URL `Index=[0,1]`；第二个 URL 的 `Size` 仍对应嵌入的第三张图片；当前保留。
- 车辆抓拍正文实际声明 3 个图片对象。旧脚本和当前只动态覆盖前两个，第三个仍是模板内容，不能写成“所有图片对象均动态”。

这里有一个明确的可观察差异：旧项目 URL 的 `Index` 是 Windows 本机图片路径，当前使用 64 位 SHA-256 标识（`alarms/mod.rs:1645`），图片服务只接受该标识（`protocol_runtime.rs:721-742`）。这是请求正文差异，不是内部实现细节。

旧项目图片响应带 `Accept-Ranges: bytes`（`HTTPServer.py:514-521`、`:803-808`）；当前已在 `http.rs:604-656` 对图片响应补上该响应头，并有单元测试，但这不能消除 URL `Index` 的差异。

### 4.4 multipart 和请求头

- 自定义报警相机使用旧边界 `------------------------e7a8348a9833c6f5`、元数据 `text/plain`、字段名 `image` 和 `imageindex=1`。
- 智能相机 V1.0 的第一步保留旧脚本的 `Expect: 100-continue` 条件；V1.1 使用旧脚本的 multipart 分隔符形式。
- 普通 NVR 和车辆 NVR 的 JSON `Content-Type`、`Accept`、连接关闭条件已按旧分支配置。

以上是源码和代表性测试的核对结果，不等于每一条请求头已经与旧程序抓包逐字节相同；当前还没有全量抓包对照。

## 5. 实况和 RTSP 复核

已对齐或补齐的行为：

- IPC 三路入口为 `/media/video1`、`/media/video2`、`/media/video3`；
- NVR 的 HTTP/ONVIF 元数据按通道展开，但 RTSP 仍按旧项目只启动 c1；
- SDP 保留 metadata 轨；`/media/video1/metadata` 的 `SETUP` 现在返回成功，但不启动视频替代轨；
- RTP SSRC 使用旧值 `0x0c8c750a`（`rtsp/service.rs:18`）；
- `Content-Base`、TCP interleaved 和 SETUP 传输格式已有定向测试。

仍有实况差异：

1. 旧 `mediafile/mainstream.pcap` 重组结果包含约 5406 个 PT=105 视频 RTP、395 个 PT=0 RTP、8 个 RTCP，PT=107 metadata RTP 为 0。也就是说旧项目虽然宣告 metadata，但实际没有发送 PT=107；当前同样不发送 PT=107，这一项不是缺口。
2. 当前正式三路素材是 H.264。代码有 H.265 包化测试，但本次正式文件没有 H.265 码流；旧项目能够识别 H.265。因此 H.265 实况尚未在本次正式文件范围内复核。
3. 当前播放的是素材文件，不是旧项目对 `mainstream.pcap` 的逐包重放；序列号、时间戳、PT0 和完整 RTP 字节流不能声称完全相同。

## 6. 人脸门禁和人员库

旧流程不是只写死 `MatchPersonID=1`：

- `VSITool.py:185` 调用 `HTTPMethod.get_person_face()`；
- `HTTPMethod.py:659-698` 查询 UMS 人员信息；
- 查询结果再传给 `ACSAlarm.py`，写入 `MatchPersonID`、姓名、编号等字段。

当前启动请求没有 UMS 用户名/密码或人员查询配置，代码只保留 `MatchPersonID=1` 的失败回退（`alarms/mod.rs:1190-1210`）。因此人脸门禁的模板、图片数量和路径可以本地生成，但“在库人员真实匹配”仍是能力缺口，必须单独验收。

## 7. 界面和用词复核

旧界面 `VSITool_UI.py:528-584` 使用的主要词汇是：服务器配置、设备配置、虚拟设备起始 IP、虚拟设备数量、虚拟设备端口、设备类型、发送图片规格、发送图片间隔、预设发包数、告警类型、数据统计、虚拟设备开启、虚拟设备关闭、发送图片、停止发送、实况。

当前界面已改为“虚拟设备模拟”，并沿用上述词汇；入口标题、侧边栏中文/英文、按钮和页签均已更新。默认界面隐藏网络接口、子网、三路端口、文件版本、设备标识和文件内部编号；“高级设置”才显示这些排查信息。告警类型下拉读取已准备文件中的中文名称，不显示“告警类型 ID”。

另外做了两项易用性修正：

- 初始配置直接显示一行“服务器 IP/服务器端口”，不需要先点击“添加服务器”；
- 发送图片规格恢复为旧界面的“常规、偏小、偏大”三项。

内部路由仍保留 `/tools/video-device-simulator`，这是兼容链接的内部地址，不是用户界面文案。

## 8. 当前本地验证

本次已实际运行：

| 验证 | 结果 |
| --- | --- |
| 正式文件告警复核（设置 `FST_APPROVED_PACK_ROOT` 和 `FST_APPROVED_PACK_VERSION=1.0.3`） | 1 项通过 |
| 告警运行时模块（含正式文件项） | 7 项通过 |
| 告警模块定向测试 | 21 项通过 |
| RTSP 模块定向测试 | 17 项通过 |
| 整个 `device_simulator` Rust 模块 | 197 项通过 |
| HTTP 图片响应头测试 | 1 项通过 |
| `cargo check --lib` | 通过 |
| `pnpm check` | 通过 |
| `VideoDeviceSimulatorPage` 合同测试 | 通过 |
| `deviceSimulator` 合同测试 | 5 项通过 |
| `pnpm lint`、`git diff --check` | 通过 |

正式文件测试依赖本机 `src-tauri/target/approved-packs`。未设置环境变量或文件不存在时测试会明确跳过，不能把“跳过”写成“正式文件已通过”。

尚未完成、不能用本报告代替的验证：

- 114 条请求的逐条旧报文/新报文对照；
- 真实 UMS 发现、上线、订阅、实况录像和图片回取；
- 人员库查询和在库人员匹配；
- H.265 正式文件实况；
- 隔离 Windows 环境下的虚拟 IP、防火墙、崩溃恢复和大量设备压力测试。

旧 EXE、旧哈希和发布 manifest 不是当前工作区源码或当前报文的证据，本次不使用它们证明一致性。

## 9. 外部验收清单

真实 UMS 验收至少应分别保存六类设备的：

1. 发现、添加、在线、KeepAlive、订阅请求和响应；
2. 主码流、辅码流、第三码流拉流、录像、检索、回放和断线重连结果；
3. 所有 114 类告警的主请求、后续请求、恢复请求、请求头、图片 URL 和 UMS 页面结果；
4. 智能相机的 `Structure -> Alarm` 顺序、车辆 NVR 的 `VehicleEventInfo -> Alarm` 顺序及关联字段；
5. NVR 多通道元数据、图片回取响应头和图片显示；
6. 人脸门禁人员库查询、人员编号、姓名和图片匹配结果。

## 10. 最终判定

当前实现是：**六类设备和 114 条告警定义已覆盖，主要请求路径、端口规则、图片顺序和部分 RTSP 握手已按旧项目复核；但请求并非逐字段/逐字节完全一致，真实 UMS 尚未验收。**

因此本任务的最终判定为：**部分一致，不得标记为完全复刻。**
