# 网络工具模块设计文档

## 概述

在"其他工具"侧边栏下新增"网络工具"入口，页面内通过 Tab 切换 5 个网络工具功能。

## 功能清单

| Tab | 功能 | 后端实现 | 前端特性 |
|-----|------|----------|----------|
| Ping 扫描 | 扫描网段内 IP 在线状态 | Rust ICMP/TCP ping，并发执行，Tauri 事件实时推送 | 16列紧凑网格 + 表格切换，实时动画 |
| TCP 连接统计 | 查看本机 TCP 连接数及分布 | Rust 解析系统 netstat 或调用 Windows API | 状态卡片 + 双面板聚合(按IP/按端口) |
| 端口连通性测试 | 测试指定 IP:Port 是否可连 | Rust TCP connect 探测 | 预设端口组(可自定义) + 结果表格 |
| Wake-on-LAN | 发送魔术包唤醒局域网设备 | Rust UDP 广播发送 WOL 包 | 已保存设备列表 + 一键唤醒 |
| 子网计算器 | 计算子网信息 | 纯前端计算 | CIDR 快捷按钮 + 6格结果卡片 + 二进制表示 |

## 架构设计

### 导航结构

- 侧边栏"其他工具"下新增一个"网络工具"子项
- 路由: `/tools/network`
- 页面内 5 个 Tab 切换

### 前端文件

```
src/pages/NetworkToolsPage.vue       # 主页面（Tab 容器）
src/components/network/
  PingScanTab.vue                    # Ping 扫描
  TcpConnectionsTab.vue             # TCP 连接统计
  PortTestTab.vue                   # 端口连通性测试
  WakeOnLanTab.vue                  # Wake-on-LAN
  SubnetCalcTab.vue                 # 子网计算器
```

### 后端文件

```
src-tauri/src/network.rs             # 所有网络工具的 Tauri commands
```

### Tauri Commands

| Command | 参数 | 返回 | 事件 |
|---------|------|------|------|
| `ping_scan` | `{ prefix: string, start: u8, end: u8, timeout_ms: u32 }` | `()` | `ping-result` 逐个推送 |
| `cancel_ping_scan` | — | `()` | — |
| `get_tcp_connections` | — | `TcpConnectionStats` | — |
| `test_port` | `{ host: string, ports: Vec<u16>, timeout_ms: u32 }` | `Vec<PortTestResult>` | — |
| `send_wol` | `{ mac: string, broadcast: string, port: u16 }` | `WolResult` | — |

子网计算器纯前端实现，无需 Tauri command。

### 数据类型

```typescript
// Ping 扫描
interface PingResult {
  ip: string;
  alive: boolean;
  latency_ms: number | null;
  hostname: string | null;
}

// TCP 连接统计
interface TcpConnectionStats {
  total: number;
  by_state: Record<string, number>;        // ESTABLISHED: 312, TIME_WAIT: 428 ...
  by_remote_ip: { ip: string; count: number }[];   // Top N
  by_port: { port: number; name: string; count: number }[];  // Top N
}

// 端口测试
interface PortTestResult {
  port: number;
  service: string;
  open: boolean;
  latency_ms: number | null;
  error: string | null;
}

// WOL
interface WolResult {
  success: boolean;
  message: string;
}

// 端口预设组（持久化到 config）
interface PortPreset {
  name: string;
  ports: string;  // "80,443" 或 "8000-8100"
}
```

### 配置扩展

在 `AppConfig` 中新增字段:

```rust
pub network_tools: Option<NetworkToolsConfig>,
```

```rust
struct NetworkToolsConfig {
    port_presets: Vec<PortPreset>,    // 自定义端口预设组
    wol_devices: Vec<WolDevice>,     // 已保存的 WOL 设备
}

struct PortPreset {
    name: String,
    ports: String,
}

struct WolDevice {
    name: String,
    mac: String,
    broadcast: String,
    port: u16,
}
```

### Ping 扫描实现细节

- **方式**: Windows 下使用 `winapi` 的 `IcmpSendEcho` 或 fallback 到 TCP connect 80/443 端口
- **并发**: `tokio::spawn` + `Semaphore` 控制并发数（默认 50）
- **事件推送**: 每个 IP 完成后通过 `app_handle.emit("ping-result", PingResult)` 推送
- **取消**: `AtomicBool` 标志位，与现有 `should_cancel` 模式一致

### 输入校验规则

**Ping 扫描**:
- 网段前缀: 必须 3 段，每段 0-255
- 起始/结束: 1-254，起始 ≤ 结束
- 超时: 100-10000ms

**端口测试**:
- 目标地址: IP 或域名格式校验
- 端口: 1-65535，支持逗号分隔和范围格式
- 总端口数不超过 1000

**WOL**:
- MAC 地址: `XX:XX:XX:XX:XX:XX` 或 `XX-XX-XX-XX-XX-XX` 格式
- 广播地址: 合法 IP

**子网计算器**:
- IP 地址: 合法 IPv4
- CIDR: 0-32

### 端口预设自定义

- 内置默认预设: Web(80,443), SSH(22), 数据库(3306,5432,6379), 常用全量
- 用户可新增/编辑/删除自定义预设
- 预设持久化到 `AppConfig.network_tools.port_presets`
- 编辑通过弹窗实现（输入名称 + 端口列表）

### i18n

所有 UI 文本在 `src/locales/messages.ts` 中添加中英翻译，key 前缀为 `networkTools.*`。
