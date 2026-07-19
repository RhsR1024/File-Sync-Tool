# 视频设备模拟器 UMS 业务一致性复核报告

- 日期：2026-07-19
- 应用版本：`1.2.0`（固定）
- 正式素材版本：`1.0.3`
- 目标平台：仅 UMS
- 本地结论：`LOCAL_PARITY_REVIEW_COMPLETE`
- 外部结论：`EXTERNAL_ACCEPTANCE_PENDING`

## 1. 复核结论

当前实现已覆盖用户批准的六类设备：普通 IPC（旧项目“自定义报警相机”）、智能相机、结构化相机、人脸门禁相机、普通 NVR、车辆识别 NVR。设备身份、发现、HTTP/LAPI/ONVIF、RTSP/TCP 三码流、告警调度、模板字段、图片槽位、恢复请求和复合请求均已按旧项目静态源码/模板重写，并在正式 Pack `1.0.3` 下完成本地自动化验证。

模拟器产品代码只保留 `ums` 平台类型。VMS 的类型、UI 选项、序列化兼容别名、运行时分支和素材选择均不再存在。构建素材时对旧源文件中的 VMS 专属 `*-vms` 变体进行排除；测试中出现的 VMS 字样只用于证明旧输入会被正确过滤，不是产品能力。

“本地一致性完成”表示新实现与旧项目可静态确认的业务流程一致，不等于真实 UMS 已验收。没有真实平台请求/响应、抓包和平台侧结果的项目继续保持 `REAL_PLATFORM_REQUIRED`，不会被标记为成功兼容。

## 2. 复核范围与来源

主要旧项目证据：

- `D:\WorkSpace\VirtualTools\data\dev_type.yml`
- `D:\WorkSpace\VirtualTools\data\alarms_info.yml`
- `D:\WorkSpace\VirtualTools\script\HTTPServer.py`
- `D:\WorkSpace\VirtualTools\script\CustomAlarm.py`
- `D:\WorkSpace\VirtualTools\script\SmartAlarm.py`
- `D:\WorkSpace\VirtualTools\script\StructureAlarm.py`
- `D:\WorkSpace\VirtualTools\script\ACSAlarm.py`
- `D:\WorkSpace\VirtualTools\script\NormalAlarm.py`
- `D:\WorkSpace\VirtualTools\script\VehicleAlarm.py`
- 对应 `xml/`、`object/`、`pic/` 与 `mediafile/` 素材

新实现主要证据：

- `src-tauri/src/device_simulator/profiles/`
- `src-tauri/src/device_simulator/protocol_runtime.rs`
- `src-tauri/src/device_simulator/alarm_runtime.rs`
- `src-tauri/src/device_simulator/alarms/`
- `scripts/device-simulator-assets/build-approved-release.mjs`
- `src-tauri/target/approved-packs/*/1.0.3`

## 3. 六类设备逐项结果

| Profile | 旧项目映射 | UMS 告警数 | 主请求行为 | 图片槽位 | 恢复行为 | 本地结论 |
| --- | --- | ---: | --- | --- | --- | --- |
| `ipc-custom` | 自定义报警相机（普通 IPC） | 2 | V1.1 `POST /LAPI/V1.1/System/Event/Notification` | 带图事件 1 图；无图事件 0 图 | 旧项目无恢复，不新增 | 一致 |
| `ipc-smart` | 智能相机 | 71 | V1.0 结构化事件按 `Structure -> Alarm`；V1.1 使用 multipart | V1.0 两图顺序按旧脚本；V1.1 单图 | 仅旧配置声明的人员聚集、入梯等事件生成恢复 | 一致 |
| `ipc-structured` | 结构化相机 | 4 | `POST /LAPI/V1.0/System/Event/Notification/Structure` | 人体 2、人脸 4、机动车 5、非机动车 4 个图像对象 | 旧项目无恢复，不新增 | 一致 |
| `ipc-face-access` | 人脸门禁相机 | 9 | 人员核验走 `PersonVerification`；控制器事件走 `Alarm` | 陌生人 1 图、在库人员 2 图 | 仅 YAML 声明 `alarmTypeOff` 的控制器事件生成恢复 | 一致；人员库查询见第 7 节 |
| `nvr-common` | 普通 NVR | 25 | `POST /LAPI/V1.0/System/Event/Notification/Alarm` | 禁止带图 | 8 类旧配置事件支持恢复 | 一致 |
| `nvr-vehicle` | 车辆识别 NVR | 3 | 匹配/不匹配为 `VehicleEventInfo -> Alarm`；抓拍走 `Structure` | 每类按旧模板使用 2 个嵌入图片槽位 | 旧项目无恢复，不新增 | 一致 |

总注册告警数为 114，正式 Pack 测试按 Profile 精确断言 `2 + 71 + 4 + 9 + 25 + 3`。

## 4. 协议与运行时一致性

### 4.1 身份、发现和订阅

- 六类 Profile 使用旧 `dev_type.yml` 的型号、固件版本、昵称和 IPC/NVR 类型事实。
- IPC、NVR、人脸门禁分别使用旧项目对应发现模板；动态替换设备 IP、MAC、序列号、型号、端口和消息 ID。
- UMS 设备类型、订阅地址和订阅寿命按 Profile 渲染；模板证据仍标记 `reviewed_static`。
- 告警事件 ID、关联 ID、Reference 中的订阅编号不再错误复用同一个占位值；按每次逻辑告警和主请求/后续请求/恢复请求角色生成，车辆两步请求共享 `RelatedID`。

### 4.2 HTTP/LAPI/ONVIF

- 旧公共路由和 Profile 专属路由由签名 Pack 加载，未知路由、模板变量或平台组合 fail closed。
- NVR `System/ChannelDetailInfos`、ONVIF `GetVideoSources` 和 `GetAudioSources` 按配置通道数动态展开；默认 8，产品安全上限 128。
- NVR Smart Capabilities 保留旧项目的 `599 OK` 兼容语义，不擅自改成标准状态码。
- 协议 HTTP 服务只能读取 Pack 已声明图片或 SHA-256 内容寻址的用户图片，运行中的服务可回取任务启动后导入的图片。

### 4.3 RTSP

- IPC 提供 `/media/video1`、`/media/video2`、`/media/video3`，端口默认为 554/555/556。
- 两类 NVR 与旧项目一致：HTTP/ONVIF 元数据按多通道展开，实际 RTSP 只提供 c1 的主、辅、第三码流。
- 仅支持旧实现已确认的 TCP interleaved；不声明 UDP、Digest 或音频。
- c2 到 cN 的独立 RTSP 在旧项目中不存在，因此不属于本次语言重写缺口。

## 5. 告警报文一致性

- 所有告警使用真实 Unix 时间和毫秒/格式化采集时间，不再使用进程启动时长或固定 1970 值。
- 设备 ID、设备 IP、通道、事件 ID、关联 ID、Reference、图片大小/数据/URL 均在强类型模板边界内动态注入。
- UMS V1.1 multipart 图片字段固定为 `image`，包含旧项目使用的 `imageindex=1`；普通 IPC 文件名为 `picture.jpg`。
- 智能相机犬绳/遛狗 V1.1 事件补齐 `EventDetail`；V1.0 两图事件按旧脚本恢复大图/小图顺序。
- 结构化相机的人体、人脸、机动车、非机动车属性按旧脚本范围动态生成；机动车车牌、速度和非机动车方向/类型不再固定。
- 人脸门禁温度、口罩状态和图片 `Name` 动态生成；陌生人空全景槽位保持旧模板语义，在库人员保留两张图。
- 车辆 NVR 匹配/不匹配车牌动态生成；两步请求保持顺序并共享关联 ID，抓拍图片槽位顺序按旧素材断言。
- 普通 NVR 的通道/设备 `AlarmSrcType`、`ChannelDeleted` 特殊正文和恢复事件均按旧配置生成，且禁止附图。

## 6. 有意差异

以下差异是批准的架构或安全改进，不改变所选业务效果：

1. 仅保留 UMS，完全移除模拟器 VMS 类别和运行分支。
2. Python/多进程/WMI 实现改为 Rust/Tokio Worker、会话 journal 和精确资源所有权清理。
3. 旧任意本地文件路径回取改为签名 Pack 或 SHA-256 用户素材，防止路径穿越和任意文件读取。
4. 素材不嵌入 EXE，使用 Ed25519 签名、不可变 Pack、哈希校验、版本 pin 和回滚。
5. 旧脚本“请求已发出即成功”的日志不作为平台成功证据；当前可达响应仍计为 `unverified`，直到真实 UMS 给出成功规则。
6. NVR 最大 128 是产品安全上限，不宣称为厂商协议上限。

## 7. 已知配置能力缺口

旧 `ACSAlarm.py` 在具备平台用户名/密码时可查询 UMS 人员库，并在失败时回退 `MatchPersonID=1`。当前模拟器启动请求没有 UMS 运行时用户名/密码字段，因此未接入真实人员库查询，保持旧项目失败路径的 `MatchPersonID=1`。

该项不影响人脸门禁报文模板、图片槽位和其它动态字段的本地一致性，但真实在库人员匹配必须在后续增加不持久化凭据输入及人员库查询后再验收。当前状态：`CONFIG_CAPABILITY_GAP`，不得标记为真实 UMS 已完成。

## 8. 本地验证证据

| 验证项 | 结果 |
| --- | --- |
| 正式素材构建/解析测试 | 11/11 通过 |
| 告警模块定向测试 | 21/21 通过 |
| 协议运行时定向测试 | 7/7 通过 |
| Rust `app_lib` | 195/195 通过 |
| Rust `app` | 437/437 通过 |
| TypeScript/Vue 类型检查 | 通过 |
| ESLint | 通过 |
| 文件共享 Web 测试 | 33/33 通过 |
| `1.2.0` 裸 EXE 构建 | 已生成 `D:\Rust\target\release\file-sync-tool-1.2.0-202607191326.exe`，35,496,448 bytes |
| EXE SHA-256 | `6b52906146ed88ce0ff1c0270bddc3f4f4d7722193c1a8aa6dbbead819a19d51` |

发布 manifest 已回写为 `latest=1.2.0`、同名文件和上述 SHA-256。EXE 原始字符串扫描未发现 `VirtualTools`、旧 Python 告警脚本、PCAP、图片目录或素材文件名；`alarm-types.json` 和 `1.0.3` 是运行时 Pack 解析代码的预期字符串。扫描到的 `PRIVATE KEY`/`BEGIN RSA` 仅位于 libssh2/OpenSSL 的密钥解析器静态标签附近，没有外部签名私钥文件或 PEM 密钥载荷被打包；签名私钥仍存放在仓库外。

## 9. 外部验证门禁

| 状态 | 必须外部验证的内容 | 关闭条件 |
| --- | --- | --- |
| `EXTERNAL_VM_REQUIRED` | UAC、次要 IP/防火墙真实创建删除、部分失败回滚、主进程/Worker 崩溃、断电恢复、主 IP/DHCP/网关/DNS 不变 | 隔离 Windows VM 中留存前后状态、日志和恢复结果 |
| `REAL_PLATFORM_REQUIRED` | UMS 发现、添加、在线、KeepAlive、订阅响应 | 留存真实 UMS 请求/响应、设备状态和持续在线结果 |
| `REAL_PLATFORM_REQUIRED` | 六类设备 HTTP/LAPI/ONVIF 全流程 | 每个 Profile 留存请求、响应和平台页面结果 |
| `REAL_PLATFORM_REQUIRED` | RTSP 拉流、连续录像、检索、回放、拖动、断流重连 | 留存平台录像和重连结果，不以 VLC 单独替代 |
| `REAL_PLATFORM_REQUIRED` | 六类设备全部 114 类告警、图片 URL 回取、尺寸变体和恢复事件 | 留存平台接收结果、图片显示和恢复状态 |
| `REAL_PLATFORM_REQUIRED` | NVR 多通道元数据行为 | 在真实 UMS 核对通道数量、名称、状态、视频源和音频源 |
| `REAL_PLATFORM_REQUIRED` | 智能相机与车辆 NVR 复合请求顺序 | 确认 UMS 接受 `Structure/VehicleEventInfo -> Alarm` 顺序与关联字段 |
| `REAL_PLATFORM_REQUIRED` | 10/100/500 台及 100 路 2 Mbps、1 小时资源门禁 | 在批准硬件和独立网络记录 CPU、RSS、码率、丢包和稳定性 |

## 10. 最终判定

- 六类 UMS Profile 的可静态确认旧业务逻辑：已实现并完成本地复核。
- VMS 类别：已从模拟器产品范围和代码路径移除。
- 版本：应用固定 `1.2.0`，正式素材 `1.0.3`。
- 真实 UMS 兼容性：尚未验收，继续门控。
- 隔离 Windows 资源生命周期：尚未验收，继续门控。
- UMS 人员库查询：当前缺少运行时凭据配置，继续使用旧失败回退并单列能力缺口。
