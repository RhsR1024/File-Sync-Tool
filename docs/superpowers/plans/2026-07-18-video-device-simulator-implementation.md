# 视频设备模拟器实施计划

> 依据：`docs/superpowers/specs/2026-07-18-video-device-simulator-design.md`
>
> 状态：应用 `1.2.1` 与正式签名素材 Pack `1.0.2` 的本地可实现、可自动验证范围已完成；仅保留隔离 Windows VM 和真实 VMS/UMS 验收项。

## 目标

在 File Sync Tool 中新增“视频设备模拟器”，使用 Vue 3 + Tauri 2 + Rust/Tokio 实现；主进程负责 UI、配置、素材和 Worker 生命周期，提权 Worker 负责虚拟 IP、防火墙、设备发现、HTTP/LAPI/ONVIF、RTSP/RTP 和告警。协议模板、图片和媒体素材通过独立、版本化素材仓库下载，不进入应用 EXE。

## 全局约束

- 协议事实优先级和禁止猜测规则以设计规格第 2 节为准。
- Phase 1 未完成、审查问题未获批准前，不实现依赖未知协议事实的 handler、URL、模板或身份格式。
- 不执行旧项目，不修改测试网卡，不发送真实告警；旧项目只做静态审计。
- 不复制旧配置中的凭据，不把旧项目完整素材加入仓库或应用 EXE。
- 所有 Tauri 调用集中在 `src/lib/tauri.ts`，页面不直接调用裸 `invoke()`。
- 配置属于应用域；Rust/TypeScript 类型、默认值、规范化、迁移、配置域补丁和测试必须同步。
- 新文案必须同步维护中英文，界面图标统一使用 `lucide-vue-next`。
- 当前工作树已有大量用户改动；实施只做增量修改，不回退、不覆盖无关文件。默认不自动提交或推送；2026-07-19 用户已明确授权本轮验证完成后提交并推送当前项目全部安全文件。
- 每个阶段先写可自动化测试；Windows 网络、UAC 和真实平台行为在隔离 VM/专用测试机验证。

## 初始仓库接入审计（历史基线）

- 当前仓库尚无 `device_simulator`、`device-simulator` 或“视频设备模拟器”实现，需要从零建立新模块。
- 共享集成热点 `src-tauri/src/main.rs`、`src-tauri/src/config.rs`、`src/App.vue`、`src/components/Sidebar.vue`、`src/lib/sidebarNavigation.ts`、`src/lib/tauri.ts`、`src/locales/messages.ts`、`src/pages/ToolsHubPage.vue`、`src/router/index.ts` 均已有用户未提交修改。并行 Agent 只负责新模块/新测试；这些共享文件由单一集成 owner 串行合并。
- `src-tauri/src/main.rs` 的 Worker 参数分支必须位于 WebView2 bootstrap 和单实例初始化之前，不能复用普通 Tauri 启动路径。
- 当前 `confirm_quit`、窗口退出和托盘退出存在立即/延时强制退出路径，与清理协议冲突；退出编排是最高风险集成点，必须在 Worker journal/recovery 稳定后改造。
- 关闭到托盘的现有“仅隐藏窗口”语义可保留，模拟会话继续运行；真正退出必须等待 Worker 清理，失败时显式报告残留风险。
- 前端全局状态采用“先注册事件监听，再主动读取 `get_status` 快照”的方式恢复，避免页面重载或监听竞态丢失真实状态。

## 阶段依赖

```text
Phase 1 协议事实审计 / 审查门
  -> Phase 2 素材契约与 AssetStore
  -> Phase 3 Worker / 恢复 / Windows 资源
  -> Phase 4 身份 / 发现 / HTTP
  -> Phase 5 RTSP / RTP
  -> Phase 6 告警
  -> Phase 7 Vue / Tauri / 退出编排
  -> Phase 8 兼容、规模与发布验收
```

Phase 2 和 Phase 3 的纯基础设施可在审查结论明确后部分并行；Phase 4～6 必须消费已批准的 profile 证据，不得以示例值代替事实。

## 当前收敛状态（2026-07-19）

| 状态 | 范围 | 当前处理 |
| --- | --- | --- |
| `LOCAL_COMPLETE` | Schema、正式签名素材发布工具与 `1.0.2` 六个不可变 Pack、AssetStore、Worker 协议与 Manager、会话 journal/recovery、稳定网卡标识、原生 IP/防火墙后端、身份预览、发现/HTTP/RTSP/RTP/告警运行时、内容寻址用户图片、Tauri/配置/UI/退出保护 | 已实现并由本地自动化与正式素材运行时门禁覆盖；不以此宣称厂商平台兼容 |
| `EXTERNAL_VM_REQUIRED` | UAC、真实次要 IP 与防火墙创建/删除、部分失败回滚、主进程/Worker 崩溃、断电恢复、主 IP/DHCP/网关/DNS 不变 | 只能在隔离 Windows VM 执行，当前主机只做原生 API 编译与只读枚举验证 |
| `REAL_PLATFORM_REQUIRED` | VMS/UMS 发现、添加/在线、keepalive、HTTP/LAPI/ONVIF、RTSP 录像/检索/回放、告警/图片/恢复及 10/100/500 规模 | 必须由真实平台反馈关闭；所有未测组合继续显示“未验证” |
| `EVIDENCE_GATED` | NVR 多通道 RTSP URL、车辆/智能复合多请求语义、各平台精确 Content-Type/boundary 与成功判定 | 当前静态证据可运行部分已实现；冲突或未获真实平台证据的候选继续 fail closed 或标记“未验证” |

---

## Phase 1：协议事实审计与审查门

### Task 1：建立四类 Profile 来源矩阵

**产物**

- `docs/superpowers/specs/2026-07-18-video-device-simulator-evidence-matrix.md`
- `ipc-custom`、`ipc-smart`、`nvr-common`、`nvr-vehicle` 的源码、配置、模板、图片和媒体证据索引。

**步骤**

- [x] 读取设计规格并提取禁止猜测规则、范围、状态机、接口和验收要求。
- [x] 静态审计 `D:\WorkSpace\VirtualTools` 的入口、设备类型、告警配置、协议路由、RTSP、模板和素材。
- [x] 对每个 profile 记录平台、型号/版本、端口、发现、HTTP、RTSP、告警、图片、鉴权和模板来源。
- [x] 对相互冲突或无证据的事实标记“待业务确认”，列出影响，不自行选择。
- [x] 确认旧素材/模板/PCAP/图片可用于测试、学习、复制和打包，但禁止商用。

**完成标准**

- 每个准备实现的 profile/handler 均可追溯到具体文件和符号/配置项。
- 设计规格第 34 节与证据矩阵的 16 项审查问题均有“已批准 / 后续范围 / 待确认”状态。
- 未验证的平台组合明确标记“未验证”。

### Task 2：冻结首版范围与兼容矩阵

**依赖**：Task 1

- [x] 确认首版目标平台和四类 profile 范围。
- [x] 确认 NVR 默认/最大通道数、鉴权、RTSP transport/码流/音频范围。
- [x] 确认 catalog 信任模型、防火墙自动管理、素材服务器认证、自定义图片范围。
- [x] 记录首版性能验收硬件和 RTSP 并发目标。
- [x] 将审查结论回写设计规格和证据矩阵。

**门禁**：未完成本 Task，不进入含厂商协议行为的 Phase 4～6。

---

## Phase 2：素材仓库与客户端 AssetStore

### Task 3：定义 Catalog、Pack 与 Profile Schema

**建议文件**

- `src-tauri/src/device_simulator/assets/catalog.rs`
- `src-tauri/src/device_simulator/assets/validation.rs`
- `src-tauri/src/device_simulator/profiles/schema.rs`
- `scripts/device-simulator-assets/`（仅构建/校验工具，不进入运行时）

- [x] 定义 `catalog-v1.json`、`pack.json` 和 profile schema 的强类型模型。
- [x] 校验 schema version、engine API、版本、依赖闭包、URL、大小和 SHA-256。
- [x] 拒绝绝对路径、盘符、UNC、`..`、未列出文件和可执行扩展名；ZIP entry 符号链接元数据由后续归档层继续拒绝。
- [x] 设置压缩大小、解压大小、文件数量和单文件大小上限。
- [x] 为 schema、依赖环、路径逃逸、Windows 保留路径、可执行内容和引用闭包写 Rust 单元测试。

### Task 4：实现素材发布生成与校验工具

**依赖**：Task 1、Task 3

- [x] 从经批准素材生成不可变 ZIP 和 catalog，不从未知数据猜填媒体参数。
- [x] 生成/复算每个文件和 ZIP 的 SHA-256、大小、解压大小与依赖。
- [x] 实现“先上传 ZIP，验证可访问，最后原子替换 catalog”的发布流程。
- [x] 扩展 `scripts/release-server/README.md`，明确开发服务器与生产静态服务器边界。
- [x] 增加生成结果的可重复性和篡改失败测试。
- [x] 生成并本地发布 `ipc-custom`、`ipc-smart`、`nvr-common`、`nvr-vehicle`、`protocol-core`、`media-h264-live` 六个 `1.0.2` 不可变 Pack；catalog 使用 `device-assets-static-review-2026` 离线 Ed25519 签名，`min_app_version=1.2.1`。

### Task 5：实现 AssetStore 下载、安装、缓存与回滚

**建议文件**

- `src-tauri/src/device_simulator/assets/{resolver,download,archive,cache}.rs`
- 复用/扩展 `src-tauri/src/download_verify.rs`

- [x] 通过 Tauri/custom data dir 解析缓存目录，不硬编码 `%APPDATA%`。
- [x] 实现签名 catalog 获取/离线成对缓存、依赖闭包、磁盘空间预检和本地有效缓存识别。
- [x] 实现 `.part`、HTTP Range 续传、取消、重试、流式 SHA-256 和聚合进度。
- [x] 安全校验并解压到独占 staging：拒绝符号链接/特殊文件、Zip Slip、未声明文件、重复项、大小与哈希不一致，失败时清理半成品。
- [x] 将验证完成的 staging 原子安装到版本目录并以可恢复替换更新 `active.json`。
- [x] 会话 pin 素材版本；更新仅影响下一会话；保留上一套有效版本和上一份签名 catalog 并可回滚。
- [x] 验证离线缓存启动、损坏缓存拒绝，以及清理时保护 active、previous 和活动会话 pin 的 pack。

---

## Phase 3：Worker、会话恢复与 Windows 资源

### Task 6：建立模块骨架、错误模型与 Worker 协议

**建议文件**

- `src-tauri/src/device_simulator/{mod,models,errors,events,worker_entry,worker_protocol}.rs`
- `src-tauri/src/main.rs`

- [x] 在 Tauri/WebView2/单实例初始化前识别 `--simulator-worker`。
- [x] Worker 模式不启动窗口、托盘、同步器、剪贴板或其他工具。
- [x] 定义长度前缀 JSON、`request_id`、事件序号、协议版本和握手模型。
- [x] 实现可测试的帧编解码、版本不兼容、错误响应、心跳与 EOF 行为。
- [x] 密码和临时凭据不得进入命令行、日志或持久化配置。

### Task 7：实现 Manager、提权启动和命名管道

**建议文件**

- `src-tauri/src/device_simulator/manager.rs`
- `src-tauri/src/device_simulator/windows/{elevation,named_pipe}.rs`

- [x] 强制单活动会话和完整状态机转换。
- [x] 创建随机管道/会话 ID，设置当前用户与 Administrators ACL。
- [x] 通过 `runas` 启动同一 EXE Worker，并校验 session/version/PID/elevated。
- [x] 实现启动超时、UAC 取消、管道断开、Worker panic 和有限停止超时。
- [x] 禁止未确认系统状态下无限自动重启。

### Task 8：实现会话日志与精确恢复

**建议文件**

- `src-tauri/src/device_simulator/{session,journal}.rs`

- [x] 使用临时文件 + 原子替换记录资源所有权、pack 版本和清理阶段。
- [x] 启动/进入页面时识别非终态会话，不仅依据可能复用的 PID。
- [x] 只清理会话实际拥有且系统仍存在的 IP/防火墙规则。
- [x] 清理失败进入 `recovery_required`，保留日志和明确错误。
- [x] 覆盖日志截断、原子恢复、PID 复用、部分清理和幂等恢复测试。

### Task 9：实现网卡、虚拟 IP、冲突预检和防火墙

**建议文件**

- `src-tauri/src/device_simulator/windows/{interfaces,ip_alias,firewall}.rs`
- `src-tauri/src/device_simulator/preflight.rs`

- [x] 使用稳定接口标识枚举网卡和地址。
- [x] 按真实 CIDR 校验网络/广播/容量/跨界/本机重复。
- [x] 结合本机地址表和所选网卡的 Windows 邻居/ARP 表给出冲突结论；未发送主动网络探测且无占用证据时明确保留风险 warning，真实冲突场景验收留待 `EXTERNAL_VM_REQUIRED`。
- [x] 使用 Windows 原生 API 添加/删除次要 IP，不修改 DHCP、主 IP、网关和 DNS（写操作验收留待隔离 VM）。
- [x] 创建带产品前缀和 Session ID 的最小范围防火墙规则，只删除会话创建项（写操作验收留待隔离 VM）。
- [ ] 在隔离 Windows VM 验证 UAC、回滚、崩溃恢复和主配置不变。

---

## Phase 4：Profile、设备身份、发现与 HTTP

### Task 10：实现 Profile Registry、身份生成和设备预览

**依赖**：Task 2、Task 3、Task 5、Task 9

- [x] 从已验证 pack 加载 profile，拒绝未知 handler/变量/平台组合。
- [x] 按 profile 证据生成唯一 IP、MAC、序列号和硬件 ID。
- [x] 预览与实际启动共享同一 Rust 生成算法和确定性输入。
- [x] 生成 HTTP、RTSP 地址摘要和 NVR 通道信息。
- [x] 覆盖非 `/24`、大数量、重复身份和 profile 格式测试。

### Task 11：实现发现服务和 HTTP/LAPI/ONVIF 路由

**依赖**：Task 10；必须使用 Task 1 的已批准证据

- [x] 从旧实现/抓包静态确认组播地址、端口、Probe 识别和响应字段；真实平台请求严格度继续保留 `REAL_PLATFORM_REQUIRED`。
- [x] 每个虚拟 `IP:HTTP端口` 独立异步监听，不默认绑定 `0.0.0.0`。
- [x] 使用公共路由 + profile 路由表 + 少量强类型 handler。
- [x] 模板变量白名单、编码、路径和大小在会话启动前完成校验/编译。
- [x] 为四个迁移 profile 建立来自旧模板/源码的正式静态 fixtures，并通过签名 Pack 加载与 loopback HTTP/RTSP 集成门禁。
- [x] 记录解析/发送失败指标并限频日志，不使用宽泛静默异常。

---

## Phase 5：媒体转换与 RTSP/RTP

### Task 12：实现媒体 Pack 构建和加载

**依赖**：Task 1、Task 3、Task 4

- [x] 从批准的三路 PCAP 提取 H.264 codec、clock、payload、参数集、帧边界、关键帧和建议码率，生成正式 `media-h264-live@1.0.2`。
- [x] 在媒体 manifest 中记录 PCAP/SDP/control-path 差异和已批准选择；运行时拒绝未解决差异，不自行修正。
- [x] 运行时加载一次并共享不可变帧/NAL 缓冲。
- [x] 拒绝无关键参数、越界索引、异常码率和过大素材。

### Task 13：实现 RTSP 状态机、RTP packetizer 与共享调度

**依赖**：Task 2、Task 10、Task 12

- [x] 按旧源码和批准 fixtures 实现所需 RTSP 方法、状态转换和 TCP interleaved 会话；真实平台方法序列继续保留 `REAL_PLATFORM_REQUIRED`。
- [x] 支持已批准的 IPC 与设备级主、辅、第三码流 URL；NVR 多通道 URL/control 映射仍为 `EVIDENCE_GATED` / `REAL_PLATFORM_REQUIRED`。
- [x] 首版以已确认的 RTSP/TCP interleaved 为基线；UDP/Digest/音频不得无证据扩展。
- [x] 每客户端独立 SSRC/sequence/timestamp，共享媒体时钟和只读帧。
- [x] 循环点保持时间戳/序列连续；慢客户端使用有界队列隔离。
- [x] 验证断开、重连、取消、端口释放、wrap 和多客户端资源回收。

---

## Phase 6：告警

### Task 14：实现 Handler Registry、图片缓存和报文构造

**依赖**：Task 2、Task 3、Task 10；必须使用 Task 1 的已批准证据

- [x] 四类 profile 的正式 `alarm-types.json` 已绑定强类型 handler、模板、图片、动态字段、传输和恢复定义，并由运行时 registry 门禁加载。
- [x] 静态核实并实现 method、URL、源 IP、Content-Type、boundary 和时间戳；真实平台成功判定继续只计 `unverified`，保留 `REAL_PLATFORM_REQUIRED`。
- [x] 图片在任务开始时校验并共享，不在每次发送时读盘。
- [x] 普通 NVR 不增加旧业务不存在的带图告警。
- [x] 使用旧模板/源码和正式 Pack fixtures 校验固定身份、时间、图片、尺寸变体与 multipart/raw 请求；车辆/智能复合多请求语义仍需真实平台验收。

### Task 15：实现告警调度与统计

- [x] 支持指定、随机、顺序、单次、固定次数和持续发送。
- [x] `send_count = None` 仅表示持续发送，避免跨层魔法值。
- [x] 全局/目标服务器限流、有界队列、每设备独立节奏和计数。
- [x] 停止时取消未发送项，并在上限内等待在途请求。
- [x] 统计成功、失败、未验证、总数、在途和耗时；成功判定仍为 `REAL_PLATFORM_REQUIRED`，当前不会把可达响应冒充平台成功。

---

## Phase 7：Tauri、配置、Vue 与退出编排

### Task 16：接入应用域配置、Commands、事件和运行状态

**主要文件**

- `src-tauri/src/config.rs`
- `src-tauri/src/lib.rs` / `src-tauri/src/main.rs`
- `src/lib/tauri.ts`
- `src/lib/configDomains.ts`
- `src/lib/store.ts`

- [x] 新增 `DeviceSimulatorSettings`，同步默认值、规范化、迁移和应用域补丁。
- [x] 目标平台密码/Token、Worker/PID/会话统计不持久化。
- [x] Commands 集中在后端边界并通过 Manager/强类型服务编排；页面重载通过 `get_status` 恢复快照，外部证据未满足时 fail closed。
- [x] 批量发送状态/设备/RTSP/告警事件，避免每设备高频事件。
- [x] 更新配置域、configStore、TypeScript 契约和命令测试。

### Task 17：实现视频设备模拟器页面和导航

**主要文件**

- `src/pages/VideoDeviceSimulatorPage.vue`
- `src/components/device-simulator/`
- `src/composables/useDeviceSimulator.ts`
- `src/lib/deviceSimulatorTypes.ts`
- `src/router/index.ts`
- `src/lib/sidebarNavigation.ts`
- `src/components/Sidebar.vue`
- `src/pages/ToolsHubPage.vue`
- `src/locales/messages.ts`

- [x] 素材 Banner：检查、下载、取消、重试、错误分类和空间信息；正式 `1.0.2` 签名 catalog/Pack 已生成并通过本地下载、验证、缓存和离线门禁。
- [x] 配置：平台/服务器、网卡/IP/端口、可增删设备组。
- [x] 结构化预检和设备身份预览；错误与可忽略 warning 明确区分。
- [x] 运行状态、阶段、在线数、RTSP 客户端/码率、流地址复制/导出。
- [x] 告警控制、统计和结构化日志过滤/导出。
- [x] 官方 `small/normal/big` 告警图片变体与 JPEG/PNG 自定义图片导入；用户图片按 SHA-256 内容寻址独立保存，任务开始时校验并缓存，不混入官方 Pack。
- [x] 运行中锁定拓扑配置；残留会话优先展示恢复卡片。
- [x] 导航、工具卡片和侧栏活动点与文件共享/屏幕共享语义一致。
- [x] 确保可见焦点、表单 label、44px 关键操作目标、无颜色单一提示、减少动画偏好和窄屏无横向溢出。
- [x] 覆盖路由、侧栏、工具卡、i18n key、下载、预检、锁定、刷新恢复、清理、告警统计和自定义图片命令契约；真实 UAC/资源清理路径由 `EXTERNAL_VM_REQUIRED` 单独验收。

### Task 18：改造托盘与实际退出编排

**依赖**：Task 7、Task 8、Task 16、Task 17

- [x] 关闭到托盘时保持模拟会话。
- [ ] 实际退出先保存必要状态，再 shutdown Worker 并等待精确清理（退出钩子和有界等待已接入；活动 Worker 路径等待 `EXTERNAL_VM_REQUIRED`）。
- [x] 超时/失败保留恢复状态并显示残留风险；未确认清理时阻止退出。
- [x] 不恢复旧的静默强杀或“假装已停止”。

---

## Phase 8：兼容、规模和工程质量验收

### Task 19：自动化与隔离 Windows 验证

- [x] Rust 单元、静态 golden 与集成测试全部通过：正式 `1.0.2` Pack 门禁启用时 `cargo test` 为 app_lib 190 项、app 437 项，0 失败。
- [ ] Windows VM 验证 UAC、IP、防火墙、主进程/Worker 崩溃和断电恢复。
- [x] 验证停止后 HTTP/RTSP 端口可立即复用（本地 loopback 自动化测试）。
- [x] 前端相关测试、`pnpm check`、`pnpm lint`、`git diff --check` 通过。

### Task 20：真实平台与规模验收

- [ ] 按 profile/平台分别记录发现、添加、在线、信息、流、告警、图片和恢复。
- [ ] 平台连续录像、检索、回放、拖动和断流重连通过；VLC 仅作为辅助测试。
- [ ] 在记录硬件/网卡/码率/并发条件下完成 10/100/500 台档位。
- [x] 未实测组合显示“未验证”，不宣称支持。
- [x] 确认 EXE 不含旧项目素材、旧 Python 工具或私钥（`file-sync-tool-1.2.1-202607190838.exe`，SHA-256 `041b52b537a2d19ffda68438e787d4b842b02f46640c44e9396bc0d710f0bf6a`；369 个正式素材样本、3 种私钥表示和 26 个旧工具/路径标记均无命中；bundle 资源仅含既有 `restore-win-v.ps1`）。

## 每阶段交付检查

每个 Task 完成时至少记录：

1. 修改文件与公开接口。
2. 对应设计规格章节和证据矩阵条目。
3. 自动化测试命令与结果。
4. 需要 Windows VM/真实平台执行的手工验证。
5. 新增风险、遗留问题和回滚方式。

默认验证命令：

```powershell
pnpm check
pnpm lint
pnpm test:share-web
cargo test --manifest-path src-tauri/Cargo.toml
git diff --check
```

仅在用户要求发布或改动进入生产构建验收时运行耗时的版本化 Tauri release 构建。
