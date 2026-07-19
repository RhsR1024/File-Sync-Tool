# 视频设备模拟器协议事实与素材证据矩阵

> 状态：Phase 1 范围、非商用素材授权和正式签名素材 Pack `1.0.2` 已冻结；仅隔离 Windows VM 与真实 VMS/UMS 实测仍为外部门禁
>
> 事实来源规则：设计规格第 2 节。本文只记录静态审计得到的证据；“未验证”不等于“不支持”，“待确认”不得由实现者自行补默认值。

## 1. 审计安全边界

- 仅静态读取 `D:\WorkSpace\VirtualTools`，不运行旧 EXE/Python 工具。
- 不添加虚拟 IP、不修改网卡/防火墙、不向真实平台发送发现或告警。
- 不记录或复制旧配置中的明文凭据。
- 用户已确认旧模板、图片和 PCAP 可用于测试、学习、复制和打包，但禁止商用；后续生成物必须保留该非商用边界。
- Golden fixtures 必须来自旧实现或真实平台抓包，不能由新实现反向生成期望值。

## 2. Profile 总表

| 新 Profile | 旧项目设备类型 | 设计目标 | 源码证据 | 平台实测 | 当前状态 |
| --- | --- | --- | --- | --- | --- |
| `ipc-custom` | 自定义报警相机 | IPC 上线、RTSP、带图自定义告警 | 静态源码已定位，`1.0.2` Pack/fixture 已发布并通过运行时门禁 | 未验证 | 本地静态实现完成/平台待验 |
| `ipc-smart` | 智能相机 | IPC 上线、RTSP、VMS/UMS 智能告警 | 静态源码已定位，`1.0.2` Pack/fixture 已发布并通过运行时门禁 | 未验证 | 本地静态实现完成/平台待验 |
| `nvr-common` | 普通NVR | 多通道、常规通道/设备告警 | 静态源码已定位，`1.0.2` Pack/fixture 已发布并通过运行时门禁 | 未验证 | 本地静态实现完成/NVR 多通道与平台待验 |
| `nvr-vehicle` | 车辆识别NVR | 多通道、带图车辆告警 | 静态源码已定位，`1.0.2` Pack/fixture 已发布并通过运行时门禁 | 未验证 | 本地静态实现完成/复合告警与平台待验 |

## 3. 逐 Profile 证据

证据状态含义：

- `源码确认`：旧项目当前源码/模板/配置组合有直接证据。
- `文档声称`：仅 README/版本记录存在陈述，尚未由源码或平台确认。
- `平台验证`：已有目标平台请求、响应、抓包或验收记录。
- `冲突`：来源之间不一致，需要业务/平台审查。
- `待确认`：证据不足，禁止实现该细节。

### 3.1 `ipc-custom`

| 主题 | 结论 | 证据位置 | 状态 | 实现影响 |
| --- | --- | --- | --- | --- |
| 旧设备类型映射 | `自定义报警相机`；model `IPC-E244-WH@PAEK-Z-VF`，version `QIPC-B2202.2.0.230111`，nick `CUSTOM`，handler `CustomAlarm.pic_send`，`dev_typeenum=0` | `data/dev_type.yml:74-80` | 源码确认 | 作为 profile 元数据候选；仍需授权和平台验收 |
| 支持平台 | 配置声明 `EZStation/VMS系列/UMS`，未声明 EZAccess；未找到真实平台验收 | `data/dev_type.yml:74-80`、`data/alarms_info.yml:166-168`、`script/YamlOperator.py:41-56,133-148` | 文档声称/未验证 | 三个平台均不得显示“已支持” |
| 鉴权配置 | 旧 UI 不为该设备启用平台用户名、密码或平台端口输入；HTTP 层未见 Basic/Digest/WS-Security 校验 | `script/VSITool.py:426-478`、`script/HTTPServer.py:83-420` | 源码确认/平台未验证 | 无证据时不得强制添加账号字段 |
| HTTP/RTSP 端口 | HTTP 可配置，旧默认 `81`；RTSP 主/辅/第三端口 `554/555/556`，三个 listener 绑定 `0.0.0.0` | `config/VSIConfig.ini:12`、`script/Vsocket_ip.py:219-230` | 源码确认 | 新架构按虚拟 IP 绑定；端口 profile 例外仍需核实 |
| 发现协议 | 使用 `xml/Common/search.xml`；监听 `239.255.255.252:3702`，含 `Probe` 即响应，并发往源 IP 的 `3705/3706/3707` | `script/Vsocket_ip.py:108-175`、`xml/Common/search.xml:2` | 源码确认/平台未验证 | Probe 严格度、Scope 和三个目标端口需抓包确认 |
| HTTP/LAPI/ONVIF | SOAPAction/GET 片段动态映射 `xml/Common`；Custom 有订阅与订阅能力两个专属路由 | `script/HTTPServer.py:83-420`、`xml/Custom/` | 源码确认/存在冲突 | 动态公共模板闭包需真实请求确定 |
| Custom 订阅正文 | 两个 `.xml` 文件实际为 JSON，但旧 POST 默认响应 `application/soap+xml` | `xml/Custom/{Event-Subscription,Subscription-Capabilities}.xml`、`script/HTTPServer.py:318-341,419-420,520-536` | 冲突 | Content-Type 必须平台验证，不自行修正 |
| 身份格式 | IP 连续递增且只跳过末段 `.255`；MAC 为 12 位小写 hex；`hardware_id` 含固定前缀、随机数字/大写字母、起始 IP 和计数 | `script/VSITool.py:636-675` | 源码确认/语义待确认 | 新地址分配必须按 CIDR；身份是否跨重启稳定需审查 |
| 序列号/硬件 ID 语义 | 发现/HTTP/告警使用生成值，但 `GetDeviceInformation.xml` 的 `HardwareId` 占位符最终变成型号，`SerialNumber` 才变成生成 `hardware_id` | `xml/Common/GetDeviceInformation.xml:2`、`script/HTTPServer.py:187-219` | 冲突 | 不得把两字段直接视为同义 |
| RTSP URL | 主/辅/第三码流为 `/media/video1`、`/media/video2`、`/media/video3` | `script/Vsocket_ip.py:246-251`、`script/HTTPServer.py:134-143` | 源码确认/平台未验证 | Profile URL 候选 |
| RTSP SDP/RTP | 默认 PCAP 路径使用 H.264；视频 PT=105，ONVIF metadata PT=107；SETUP 固定 TCP interleaved；无 UDP/Digest 证据 | `script/IPCRtspLib.py:32-74,205-340,1439-1463`、辅/第三 RTSP 库对应 SDP | 源码确认/存在冲突 | Public 声明不能当完整方法支持；辅/第三 SDP control 仍指 video1 |
| 告警类型 | `NewObjectIsRecognized`（4 图）与 `NewWaterGaugeDetection`（无图），声明 EZStation/VMS/UMS | `data/alarms_info.yml:166-168` | 源码确认/存在冲突 | 水位事件未被订阅能力声明，非 UMS 构造会失败 |
| 告警报文 | POST LAPI V1.1 multipart；UMS URL 无尾斜杠且仅附 1 图，VMS/EZStation URL 有尾斜杠且附 4 图；固定 boundary | `script/CustomAlarm.py:77-145` | 源码确认/存在冲突 | 需按平台拆 handler，图片数量必须实测 |
| 告警图片 | `pic/CUSTOM/{big,normal,small}/custom/` 共 12 张；读取依赖未排序 `os.walk()` | `script/FileOperation.py:36-59`、`pic/CUSTOM/` | 源码确认/非商用授权已批准/`ipc-custom@1.0.2` 已发布 | manifest 显式排序并声明三种尺寸；运行时按声明加载，不继承旧遍历顺序 |
| 告警恢复/成功 | 无恢复请求，`NotificationType=0`；request 返回即计成功，不读取 HTTP status/body | `script/CustomAlarm.py:44-154`、`object/CustomStruct/*.json` | 源码确认/不可信成功语义 | 恢复默认不得实现；成功判定需平台证据 |

### 3.2 `ipc-smart`

| 主题 | 结论 | 证据位置 | 状态 | 实现影响 |
| --- | --- | --- | --- | --- |
| 旧设备类型映射 | `智能相机`；model `IPC3615SB-ADF28KM-I0`，version `GIPC-B6202.SMD-20220629.220629`，nick `SMART`，handler `SmartAlarm.pic_send`，`dev_typeenum=0` | `data/dev_type.yml:50-56` | 源码确认 | 作为独立 profile，不与 `ipc-custom` 合并 |
| 首版平台与告警 | VMS/UMS 共同声明 10 类 V1.0 告警；UMS 另有“人员发散”和 60 类 V1.1 告警 | `data/alarms_info.yml:86-156` | 源码确认/平台未验证 | VMS 10 类、UMS 71 类均进入首版源码迁移范围，但验收前不得宣称平台兼容 |
| HTTP/发现/RTSP | 使用公共发现和 HTTP 路由；HTTP 旧默认 81；RTSP 为 554/555/556、TCP interleaved 三码流 | `script/Vsocket_ip.py:108-230,237-253`、`script/HTTPServer.py:239-420` | 源码确认/平台未验证 | 与 IPC 公共能力复用，禁止复制旧全局绑定方式 |
| 智能能力 | 专属 `/LAPI/V1.0/Channels/0/Smart/Capabilities`；能力模板声明入梯不可配置，但 YAML 允许入梯告警 | `xml/Smart/Smart-Capabilities.xml:76-164`、`data/alarms_info.yml:96` | 冲突 | 保留“可上报告警但不可配置”的证据，不宣称平台已验证 |
| 恢复行为 | 只有人员聚集、入梯检测自动恢复；智能运动检测结束是独立事件 | `script/SmartAlarm.py:146-169,243-266,342-365` | 源码确认 | 调度器不得为其他告警自动构造恢复 |
| 订阅/Reference | 订阅 ID 固定 1000，告警 Reference 随机 1..1000；V1.0/V1.1 的主机和端口语义冲突 | `xml/Common/Event-Subscription.xml:3-15`、`script/SmartAlarm.py:90-458` | 冲突 | 未有抓包前只保留为兼容证据，不自行修正 |
| 图片与安全 | 57 张智能图片；旧图片 URL 可携带本地绝对路径并由 HTTP 直接打开 | `pic/SMART/`、`script/SmartAlarm.py:99-401`、`script/HTTPServer.py:429-516` | 源码确认/非商用授权已批准/`ipc-smart@1.0.2` 已发布 | 新实现只接受签名 Pack 或 SHA-256 user-asset 句柄，禁止任意路径读取 |

### 3.3 `nvr-common`

| 主题 | 结论 | 证据位置 | 状态 | 实现影响 |
| --- | --- | --- | --- | --- |
| 旧设备类型映射 | `普通NVR`；model `NVR302-09E2-IQ`，version `NVR-B3113.37.20.230625`，nick `COMMONNVR`，handler `NormalAlarm.pic_send`，`dev_typeenum=1` | `data/dev_type.yml:122-128` | 源码确认 | 作为 profile 元数据候选；仍需授权和平台验收 |
| 支持平台 | 配置声明 `VMS系列/UMS`；运行时按 `serverSupport` 过滤；未发现成功日志/抓包 | `data/dev_type.yml:122-128`、`script/YamlOperator.py:133-149` | 文档声称/未验证 | UI 不得显示“已支持” |
| 通道数 | 旧配置示例 `chlnum=8`、`alarmnum=16`；HTTP/ONVIF 模板按 `chl_num` 复制通道/视频源/音频源；未发现最大值约束 | `config/VSIConfig.ini:16-17`、`script/HTTPServer.py:227-231,729-757,1019-1042` | 源码确认/待确认 | 可配置多通道有证据；默认值和最大值不能据此冻结 |
| HTTP/RTSP 端口 | HTTP 可配置，旧示例为 `81`；RTSP 三个 listener 固定 `554/555/556` | `config/VSIConfig.ini:10-17`、`script/Vsocket_ip.py:206-230` | 源码确认 | HTTP 默认仍需审查；RTSP profile 例外需核实 |
| 发现协议 | WS-Discovery `239.255.255.252:3702`，响应端口 `3705/3706/3707`，NVR 使用 `xml/Common/search-aibox.xml` | `script/Vsocket_ip.py:108-175` | 源码确认/平台未验证 | 作为 golden fixture 候选，不等于真实平台兼容 |
| HTTP/LAPI/ONVIF | NVR 分派 `HTTPServer.handle_client_aibox`，默认模板目录 `xml/AIBOX/` | `script/HTTPServer.py:63-80,578-718` | 源码确认 | 需继续按实际路由最小化素材依赖 |
| RTSP URL/通道/码流 | 广告主/辅/第三码流为 `rtsp://<ip>:554/unicast/c1/s0/live`、`:555/.../s1/live`、`:556/.../s2/live`；旧服务绑定 `0.0.0.0` 且只起三条全局 listener | `script/Vsocket_ip.py:206-250` | 源码确认/存在冲突 | 新实现不能照搬全局绑定；多通道 URL 映射仍未闭合 |
| RTSP 状态/SDP | 可见 handler 为 `OPTIONS/SETUP/PLAY/GET_PARAMETER`，其他请求进入 SDP；SETUP 固定 TCP interleaved；SDP 为 H264 PT=105 + ONVIF metadata PT=107 | `script/IPCRtspLib.py:210-326,1439-1463` | 源码确认/存在冲突 | 方法集合、SDP control 与广告 URL 需真实平台确认 |
| 常规告警 | 旧清单含通道/设备类 V1.0 JSON 告警；POST `/LAPI/V1.0/System/Event/Notification/Alarm`，`application/json; charset=utf-8`；存在 `alarmTypeOff` 时延迟恢复 | `data/alarms_info.yml:189-214`、`script/NormalAlarm.py:40-123` | 源码确认/平台未验证 | 逐告警类型和 `serverSupport` 建 handler；成功判定仍需平台证据 |
| 订阅端口 | 普通 NVR 分支直接读取订阅 JSON `Port`，静态代码未见 55000～55999 校验 | `script/HTTPServer.py:589-616` | 源码确认/待确认 | 是否保留此差异需平台证据 |
| 图片告警 | 未发现 `pic/COMMONNVR`；模板仅 `object/NormalStruct/NormalAlarm.json`、`ChannelDeleted.json` | `object/NormalStruct/`、`data/alarms_info.yml:189-214` | 源码确认 | 首版禁止为普通 NVR 增加带图告警 |

### 3.4 `nvr-vehicle`

| 主题 | 结论 | 证据位置 | 状态 | 实现影响 |
| --- | --- | --- | --- | --- |
| 旧设备类型映射 | `车辆识别NVR`；model `NVR302-09E2-IQ`，version `NVR-B3113.37.20.230625`，nick `VEHICLE`，handler `VehicleAlarm.pic_send`，`dev_typeenum=1` | `data/dev_type.yml:114-120` | 源码确认 | 作为 profile 元数据候选；仍需授权和平台验收 |
| 支持平台 | 配置声明 `VMS系列/UMS`；未发现成功日志/抓包 | `data/dev_type.yml:114-120`、`script/YamlOperator.py:133-149` | 文档声称/未验证 | UI 不得显示“已支持” |
| 通道数 | 与普通 NVR 共用动态通道模板逻辑；旧配置仅提供示例值，最大值未知 | `config/VSIConfig.ini:16-17`、`script/HTTPServer.py:227-231,729-757,1019-1042` | 源码确认/待确认 | 默认/最大通道数阻塞配置冻结 |
| 发现和 HTTP | 共用 `search-aibox.xml` 与 `handle_client_aibox`；三个能力接口覆盖到 `xml/Vehicle/` | `script/Vsocket_ip.py:108-175`、`script/HTTPServer.py:63-80,578-718,652-657` | 源码确认/平台未验证 | 建立公共依赖 + profile 覆盖，不整目录复制 |
| RTSP URL/通道/码流 | 与普通 NVR 共用三端口/三 URL 静态逻辑；类型名分支和 SDP control 存在不一致 | `script/Vsocket_ip.py:188-250`、`script/IPCRtspLib.py:1439-1463` | 冲突 | GetStreamUri 和 RTSP control 阻塞实现 |
| 车辆告警 | 三类：匹配、不匹配、抓拍；匹配/不匹配先发 `VehicleEventInfo`，再发关联 `Alarm`；抓拍发 `Structure` | `data/alarms_info.yml:184-187`、`script/VehicleAlarm.py:40-227` | 源码确认/平台未验证 | 需三个强类型 handler/流程，并确认成功判定 |
| 图片映射和上传 | JSON 中嵌 PlateImage/PanoImage Data 和 Picture URL；使用 5 个 `object/VehicleStruct` 模板及 `pic/VEHICLE` 21 张图片 | `script/VehicleAlarm.py:71-226`、`object/VehicleStruct/`、`pic/VEHICLE/` | 源码确认/非商用授权已批准/`nvr-vehicle@1.0.2` 已发布 | 图片共享缓存与静态 fixture 已落地；关联多请求平台语义仍需真实验收 |
| 订阅端口 | 车辆 NVR 走非普通分支，静态代码要求 `55000 <= port < 55999` | `script/HTTPServer.py:602-616` | 源码确认/待确认 | 与普通 NVR 的差异需平台验证后决定是否保留 |

## 3.5 已发现的跨 Profile 冲突

1. `script/Vsocket_ip.py:188-196` 判断名称为 `车牌识别NVR`，但 `data/dev_type.yml:114-120` 的类型名是 `车辆识别NVR`；随后 `script/HTTPServer.py:103-112` 对真实类型索引 NVR URL 键。无法确认这是旧 bug、别名还是版本偏差，车辆 NVR 的 GetStreamUri 暂停实现。
2. NVR 广告 URL 为 `/unicast/c1/s*/live`，而 `script/IPCRtspLib.py:1445-1452` 的 SDP control 为 `/media/video1/video|metadata`。需真实平台请求/响应或抓包决定兼容行为，不自行统一。
3. 两类 NVR 仅有 `serverSupport` 配置声明，未找到成功日志、抓包或验收记录；VMS/UMS 均标记“未验证”。
4. 普通 NVR 与车辆 NVR 的订阅端口校验不同，是否为必要兼容行为未知。
5. `xml/AIBOX/` 包含设计明确不迁移的录像/回放模板；素材依赖必须按实际路由最小化，不能整目录打包。
6. 旧实现只启动三条绑定 `0.0.0.0` 的全局 RTSP listener，多 NVR 通道 URL/通道映射及最大通道数证据未闭合。

## 3.6 `ipc-custom` 专项冲突

1. YAML 包含 `NewWaterGaugeDetection`，但 `xml/Custom/Subscription-Capabilities.xml:11` 和 `Event-Subscription.xml:16-23` 只声明/订阅 `NewObjectIsRecognized`。
2. `CustomAlarm_NoPic.json` 没有 `ImageList`，非 UMS 分支却在 `script/CustomAlarm.py:116-137` 无条件访问四张图片；VMS/EZStation 水位告警按静态源码会失败。
3. `CustomAlarm_Pic.json:32-70` 声明 `ImageNum=4`，UMS 分支只发送一张图片，VMS/EZStation 发送四张。
4. 12 张 JPEG 的实际尺寸与模板声明尺寸不一致，不能由实现者自动“修正”。
5. 订阅响应固定 CreatedID/ID `261881`，告警 Reference 却使用随机 `1..1000`；Reference 还硬编码 HTTP `:81`，与可配置 HTTP 端口冲突。
6. 旧 `CustomAlarm` 没有恢复行为，且不读取 HTTP status/body；不能把旧“发送成功”日志当平台接收证据。
7. RTSP Public 声明多种方法，但只有少数显式实现；未知方法回落 SDP，辅/第三码流 SDP control 仍为 video1。

## 4. 素材来源与拟拆包清单

下表同时记录审计来源与已生成的正式静态素材；所有发布物继续受非商用限制。

| Pack 候选 | 内容 | 旧项目首要来源 | 当前状态 |
| --- | --- | --- | --- |
| `protocol-core@1.0.2` | 公共 schema、经批准的公共模板/变量/handler 描述 | `xml/`、`object/`、HTTP/发现源码 | 已生成、签名、发布并通过运行时加载门禁 |
| `ipc-custom@1.0.2` | IPC profile、订阅模板、自定义告警和 12 张三尺寸图片 | `data/dev_type.yml`、`data/alarms_info.yml`、最小 `xml/Common/` 闭包、`xml/Custom/`、`object/CustomStruct/`、`pic/CUSTOM/` | 已生成、签名、发布并通过 profile/alarm fixture 门禁 |
| `ipc-smart@1.0.2` | 智能相机 profile、智能能力、V1.0/V1.1 告警和 57 张图片 | `data/dev_type.yml`、`data/alarms_info.yml`、`xml/Smart/`、`object/SmartStruct/`、`pic/SMART/` | 已生成、签名、发布并通过 profile/alarm fixture 门禁；复合语义平台待验 |
| `nvr-common@1.0.2` | 普通 NVR profile、动态通道模板、常规 JSON 告警 | `data/dev_type.yml`、`data/alarms_info.yml`、`xml/Common/search-aibox.xml`、精确 `xml/AIBOX/` 路由闭包、`object/NormalStruct/` | 已生成、签名、发布并通过 profile/alarm fixture 门禁；多通道平台待验 |
| `nvr-vehicle@1.0.2` | 车辆识别 NVR profile、能力覆盖、车辆告警和 21 张图片 | NVR 公共依赖、`xml/Vehicle/`、`object/VehicleStruct/`、`pic/VEHICLE/` | 已生成、签名、发布并通过 profile/alarm fixture 门禁；关联多请求平台待验 |
| `media-h264-live@1.0.2` | 三路经提取的 H.264 帧、SPS/PPS、关键帧索引与码率 | `mediafile/{mainstream,substream,thirdstream}.pcap`、RTSP 源码、旧 SDP | 已生成、签名、发布；差异记录为 `reviewed_static`，三路 runtime/loopback 门禁通过 |
| `media-h265-live` | H.265 帧、VPS/SPS/PPS 和媒体索引 | `mediafile/`、RTSP 源码、旧 SDP | 不在首版已批准媒体范围；运行时仅保留通用校验能力，不发布 Pack |

禁止直接把 PCAP 作为运行时重放数据。媒体发布工具必须从真实素材提取 codec、clock rate、payload type、参数集、帧边界和关键帧索引，并记录 PCAP/SDP 差异。

## 5. Golden Fixture 来源索引

| 契约 | 输入证据 | 输出证据 | 状态 |
| --- | --- | --- | --- |
| 设备发现 | 旧发现源码与模板；真实平台请求待验 | 旧模板 + 受限动态替换规则 | 正式静态 fixture 与路由测试完成/平台请求严格度待验 |
| 设备信息/能力 | 旧 HTTP/LAPI/ONVIF 路由源码与模板 | 正式 Pack 模板 + 强类型 handler | 四 profile 静态 fixture/运行时加载完成/平台待验 |
| GetStreamUri | 旧替换规则和广告 URL | IPC 三码流与 NVR 设备级候选 | IPC/设备级 fixture 完成；NVR 多通道映射继续 `EVIDENCE_GATED` |
| RTSP 会话 | 旧 RTSP 源码、PCAP 与 SDP | OPTIONS/SETUP/PLAY/GET_PARAMETER、TCP interleaved 响应与 RTP | 三路正式媒体 loopback 集成通过/真实平台序列待验 |
| 普通 NVR 告警 | 固定设备身份/时间 | `NormalAlarm.py` + `object/NormalStruct/*.json` | 正式静态 fixture/registry/请求构造门禁完成/成功语义待验 |
| 自定义 IPC 带图告警 | 固定设备身份/时间/图片 | `CustomAlarm.py` + `object/CustomStruct/*.json` + `pic/CUSTOM/` | VMS/UMS 静态 handler 与图片尺寸门禁完成；图片数量和成功语义平台待验 |
| 智能相机告警 | 固定设备身份/时间/图片 | `SmartAlarm.py` + `object/SmartStruct/*` + `pic/SMART/` | VMS/UMS 静态 handler/registry 门禁完成；复合多请求语义平台待验 |
| 车辆 NVR 带图告警 | 固定设备身份/时间/图片 | `VehicleAlarm.py` + `object/VehicleStruct/*.json` + `pic/VEHICLE/` | 三类静态 handler/registry 门禁完成；关联多请求语义平台待验 |

## 6. 审查决策表

状态只能使用：`已批准`、`延后`、`待确认`。

| # | 决策 | 状态 | 当前证据/建议 | 阻塞范围 |
| --- | --- | --- | --- | --- |
| 1 | 首版是否限定 VMS/UMS，是否支持 EZStation | 已批准 | 首版仅 VMS/UMS；EZStation 延后 | Profile 路由、兼容声明 |
| 2 | `ipc-custom` 是否为首版 IPC，是否增加智能相机 | 已批准 | 同时交付独立 `ipc-custom`、`ipc-smart` | Profile 与素材拆包 |
| 3 | 普通 NVR、车辆识别 NVR 是否均首版交付 | 已批准 | 两类 NVR 均为首版范围 | Phase 1/4/6 工作量 |
| 4 | NVR 默认和最大通道数 | 已批准 | 默认 8；现有固定 GetProfiles 素材覆盖 1..128，首版以 128 为产品安全上限，不表示厂商协议上限 | 配置、预览、RTSP |
| 5 | 各 profile 的用户名/密码和鉴权 | 已批准 | 旧模拟设备无入站鉴权证据；首版兼容默认 `none`，不迁移旧平台登录凭据 | 配置、HTTP、RTSP |
| 6 | 是否需要 RTSP Digest Authentication | 已批准 | 首版关闭；未来作为 profile 可选新行为 | RTSP |
| 7 | 是否只需 RTSP/TCP，是否支持 UDP transport | 已批准 | 首版仅 TCP interleaved；UDP 请求明确返回不支持 | RTSP/RTP |
| 8 | 主、辅、第三码流是否同时启动 | 已批准 | 三条 URL 同时可用；内部按客户端惰性调度媒体 | RTSP、性能 |
| 9 | 是否需要音频轨道 | 已批准 | 首版四类 profile 均不声明实况音频 | 媒体 pack、SDP/RTP |
| 10 | Catalog 是否需要数字签名 | 已批准 | 首版强制 Ed25519 离线签名，签名失败关闭 | 素材安全模型 |
| 11 | 是否允许自动创建/删除防火墙规则 | 已批准 | 默认自动精确管理；启动前风险确认；只删除 journal 证明归本会话所有的规则 | Worker、预检、退出 |
| 12 | 性能验收硬件和 RTSP 并发目标 | 已批准 | 8C/16T、32 GiB、1 GbE；500 台在线、100 路 2 Mbps H.264 并发为首版门禁 | 压测与容量声明 |
| 13 | 素材服务器是否认证 | 已批准 | 首版 `none`，不保存/发送应用层凭据；依赖签名、HTTPS 与内网 ACL | 下载和凭据 |
| 14 | 自定义用户图片是否独立存储 | 已批准 | 内容寻址独立 user-assets，配置只保存 image_id，不混入官方 pack | 告警 UI/缓存 |
| 15 | 旧模板、图片、PCAP 和代码的内部复用授权 | 已批准 | 允许测试、学习、复制和打包，禁止商用；素材元数据和发布工具必须保留该限制 | 素材发布、fixtures |
| 16 | 实际 `min_app_version` 和首版 engine API | 已批准 | 正式功能版本提升为 1.2.1；schema=1、engine API=1、正式 Pack `1.0.2` 的 `min_app_version=1.2.1` | Catalog 发布 |

## 7. 审计完成门禁

- [x] 四个 profile 的拟实现范围均已有具体源码/模板证据，冲突已明确标注。
- [x] 平台、端口、URL、RTSP/SDP/PT、鉴权、告警和成功判定均有静态结论或明确的 `EVIDENCE_GATED` / `REAL_PLATFORM_REQUIRED` 标记，无未标注猜测。
- [x] 首版所需 XML/JSON/图片/媒体清单完整，并以 `1.0.2` 六个 Pack 区分公共与 profile 专属资产。
- [x] 冲突证据和未知项已进入审查决策表。
- [x] 素材/代码已获非商用测试、学习、复制和打包授权；商业使用仍被禁止。
- [x] 当前实现使用的静态 golden fixture 来源已确认并来自旧源码/模板/PCAP；真实平台请求与成功语义作为外部验收单独保留。
- [x] 首版兼容矩阵已经业务批准；未实测组合标记“未验证”。

静态审计与素材门禁已完成；隔离 VM 和真实平台门禁未关闭前，仍不允许宣称 profile 或平台兼容。

## 8. 当前实施与外部验证状态（2026-07-19）

| 状态 | 验收范围 | 证据/行为 |
| --- | --- | --- |
| `LOCAL_COMPLETE` | 正式素材安全链、Worker/Manager、journal/recovery、Windows 原生后端、身份/模板/HTTP/RTSP/RTP/告警运行时、自定义图片、Tauri/配置/UI/退出保护 | 本地编译与自动化覆盖；正式 Pack 环境下 `cargo test` 为 app_lib 190 项、app 437 项，0 失败；只读网卡、邻居表与防火墙枚举已在当前 Windows 主机通过 |
| `LOCAL_SIGNED_ASSETS_READY` | Ed25519 公钥集、签名 catalog、四类 profile Pack、`protocol-core`、三路 H.264 媒体与静态 fixtures | `1.0.2` 六个不可变 ZIP 已生成并发布到本地素材服务目录；Key ID `device-assets-static-review-2026`，运行时 profile/alarm/HTTP/RTSP 门禁通过；发布产物和私钥不进入 EXE/项目提交 |
| `LOCAL_RELEASE_READY` | 1.2.1 版本化裸 EXE | `D:\Rust\target\release\file-sync-tool-1.2.1-202607190838.exe`，SHA-256 `041b52b537a2d19ffda68438e787d4b842b02f46640c44e9396bc0d710f0bf6a`；manifest 哈希一致，369 个正式素材样本、3 种私钥表示和 26 个旧工具/路径标记均无命中 |
| `EXTERNAL_VM_REQUIRED` | UAC、IP alias/防火墙真实写入与精确回滚、崩溃/断电恢复、主网络配置不变 | 只允许在隔离 Windows VM 执行；当前主机未进行网络/防火墙写操作 |
| `REAL_PLATFORM_REQUIRED` | VMS/UMS 发现、在线/保活、HTTP/LAPI/ONVIF、RTSP 录像/检索/回放/重连、告警/图片/恢复 | 必须记录真实平台请求、响应和验收结果后才能把组合从“未验证”升级 |
| `REAL_PLATFORM_REQUIRED` | 10/100/500 台规模与 100 路 2 Mbps H.264、1 小时资源门禁 | 必须在批准硬件与独立网络中记录 CPU、RSS、码率、丢包和稳定性 |
| `EVIDENCE_GATED` | NVR 多通道 URL/control、车辆/智能复合多请求、精确平台 Content-Type/boundary 与成功判定 | 静态可运行部分已实现；冲突候选保留拒绝/未验证状态，不从旧代码冲突中自行选择 |
