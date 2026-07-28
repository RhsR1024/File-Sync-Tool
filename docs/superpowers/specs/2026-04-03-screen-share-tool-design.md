# 局域网屏幕共享工具设计（修订版 v2）

> **修订说明**：基于实现后的问题审查（浏览器无法访问），修订技术细节、修正已知 Bug 清单，并补充 QR Code 渲染方案与防火墙可靠处理方案。

---

## 背景

团队在局域网内进行远程会议时，需要屏幕共享功能。共享者在本工具中一键启动，观看者在浏览器输入 `ip:port` 即可观看，零安装、零配置。

---

## 目标

1. 共享者在"其他工具"中启动屏幕共享，支持多屏幕选择
2. 观看者在浏览器访问 `http://ip:port` 即可观看，无需安装客户端
3. 稳定支持 30-50 人同时观看，帧率 ≥15 FPS，延迟 ≤300ms
4. 支持密码保护，防止未授权访问
5. 共享者可实时查看当前观看人数、实际帧率、码率、时长
6. QR Code 展示访问地址，供手机扫码直接访问

## 非目标

1. 不支持音频共享
2. 不支持远程控制
3. 不支持跨广域网（NAT 穿透）
4. 不支持录屏/回放
5. 不支持 macOS/Linux（仅 Windows）

---

## 已知 Bug（v1 实现）与修复方案

### Bug 1：Windows 防火墙阻断入站连接（Critical）

**现象**：点击"开始共享"后，手机和局域网内其他 PC 均无法通过浏览器访问。  
**根因**：`netsh advfirewall firewall add rule` 需要管理员权限（UAC 提升）。普通用户身份运行时该命令静默失败（退出码非 0 但没有日志）。Windows 防火墙继续阻断所有入站 TCP 连接。`0.0.0.0` 监听本身正确，localhost 可以访问，但局域网访问被墙。

**修复方案**：

```rust
// 检查 netsh 执行结果，失败时向前端发送警告日志
fn add_firewall_rule(port: u16, app_handle: &AppHandle) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let result = std::process::Command::new("netsh")
            .args([
                "advfirewall", "firewall", "add", "rule",
                &format!("name=FileSyncTool_SS_{}", port),
                "dir=in", "action=allow", "protocol=TCP",
                &format!("localport={}", port),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        
        let success = result.map(|o| o.status.success()).unwrap_or(false);
        if !success {
            // 发送明确的用户提示
            let _ = app_handle.emit("screen-share-log", serde_json::json!({
                "level": "warn",
                "message": format!(
                    "防火墙规则添加失败（需要管理员权限）。请手动运行：\
                    netsh advfirewall firewall add rule name=FileSyncTool dir=in action=allow protocol=TCP localport={}",
                    port
                )
            }));
        }
    }
}
```

前端在收到 `level: "warn"` 的防火墙日志时，弹出显眼的提示 banner：
```
⚠ 防火墙规则未能自动添加。如果其他设备无法访问，请以管理员身份运行以下命令：
netsh advfirewall firewall add rule name=FileSyncTool dir=in action=allow protocol=TCP localport=9870
[复制命令]
```

### Bug 2：MJPEG 流首次连接挂死（High）

**现象**：浏览器访问后 `<img>` 一直转圈，不显示画面，不报错。  
**根因**：`scrap` DXGI 捕获器初始化后，前数秒内 `frame()` 持续返回 `WouldBlock`，广播 channel 无帧发出。Viewer 的 `rx.recv().await` 阻塞，流响应体无内容，浏览器等待数据超时（Chrome 约 30s，Safari 约 60s）。由于流是"连接中"状态，`<img>` 不触发 `onerror`，自动重连逻辑无法启动。

**修复方案**：在 viewer 订阅后，先发送一个占位 JPEG（灰色单色帧，或上次已知的帧），确保流立即有数据推出。捕获线程在实际帧到来前发送 "waiting" 帧：

```rust
// 在 screen_share_start 中，先等最多 2s 有一帧到达再返回给前端
// 捕获线程：首次 WouldBlock 时立即发送一个灰色 fallback 帧
fn make_placeholder_frame(width: usize, height: usize) -> Vec<u8> {
    // 生成一个 1x1 的灰色 JPEG，供浏览器立即显示
    let gray = vec![0x80u8; 3]; // RGB gray pixel
    encode_jpeg_rgb(&gray, 1, 1, 80)
}
```

另外修改 `viewer_html()` 中的重连逻辑：

```javascript
// 增加 loadstart 超时检测：5s 内没有任何数据视为连接失败
let loadTimer = setTimeout(() => {
  if (!alive) return;
  img.src = '/stream?t=' + Date.now();
}, 5000);
img.onload = function() { clearTimeout(loadTimer); ... };
```

### Bug 3：IP 地址检测不准（High）

**现象**：显示的访问 URL IP 与实际 LAN IP 不符（如显示 VPN IP、Docker IP 等）。  
**根因**：`local_ip_address::local_ip()` 返回默认路由对应的 IP，在多网卡场景下可能不是 LAN IP。

**修复方案**：枚举所有非回环 IPv4 地址，优先选择 192.168.x.x / 10.x.x.x 段，并在前端展示所有可用地址供用户选择：

```rust
pub fn get_lan_ips() -> Vec<String> {
    use std::net::IpAddr;
    local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, ip)| match ip {
            IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() => Some(v4.to_string()),
            _ => None,
        })
        .collect()
}
```

`screen_share_start` 返回主 IP，同时通过 `screen-share-status` 携带 `all_urls: Vec<String>` 字段。

### Bug 4：QR Code 未渲染（Medium）

**现象**：点击 QR Code 按钮无任何反应（无二维码显示）。  
**根因**：`ScreenSharePage.vue` 中有 `showQr` 状态和按钮，但模板中没有任何 QR Code 组件或渲染逻辑。`qrcode.vue` 库未安装。

**修复方案**：

安装 `qrcode` 库（纯 JS，无框架依赖）：
```bash
pnpm add qrcode
pnpm add -D @types/qrcode
```

在模板中使用 `<canvas>` 渲染：
```vue
<canvas v-if="showQr" ref="qrCanvas" class="rounded-lg" width="128" height="128" />
```

```typescript
import QRCode from 'qrcode';
const qrCanvas = ref<HTMLCanvasElement | null>(null);

watch([showQr, serverUrl], async ([show, url]) => {
  if (show && url && qrCanvas.value) {
    await QRCode.toCanvas(qrCanvas.value, url, { width: 128, margin: 1 });
  }
});
```

### Bug 5：密码错误提示不可靠（Low）

**现象**：密码输错后，错误提示依赖客户端 JS 检测 `?error=1` 参数，如果 JS 执行有问题则不显示。  
**根因**：`handler_index` 中 `let has_error = false` 硬编码，没有读取 query param。

**修复方案**：在 axum 路由中添加 query param 解析：

```rust
#[derive(Deserialize)]
struct IndexQuery { error: Option<u8> }

async fn handler_index(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(q): Query<IndexQuery>,
    headers: HeaderMap,
) -> Response {
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return Html(login_html(q.error.unwrap_or(0) == 1)).into_response();
        }
    }
    Html(viewer_html()).into_response()
}
```

---

## 技术选型（维持不变）

| 组件 | 方案 | 原因 |
|------|------|------|
| 屏幕捕获 | `scrap` (DXGI GPU) | 1-3ms/帧，60+ FPS |
| 图像编码 | `image` crate JPEG | turbojpeg 交叉编译难，image 已是依赖 |
| 传输协议 | MJPEG over HTTP | 浏览器原生支持，零 JS |
| HTTP 服务器 | `axum` (tokio) | 与项目已有依赖一致 |
| 并发广播 | `tokio::sync::broadcast` | 零拷贝，慢客户端自动跳帧 |
| QR Code | `qrcode` npm 包 | 纯 JS，无 CDN |

---

## 数据模型

### Rust 侧（screenshare.rs）

#### ScreenShareConfig

| 字段 | 类型 | 说明 |
|------|------|------|
| `port` | `u16` | HTTP 端口，默认 9870 |
| `password` | `Option<String>` | 访问密码，None 表示无密码 |
| `monitor_index` | `usize` | 屏幕索引 |
| `quality` | `u8` | JPEG 质量 10-100，默认 70 |
| `fps` | `u8` | 目标帧率 1-60，默认 15；30 以上属于 `screen-share-latency-optimization.md` §6.2 的实验档 |
| `show_cursor` | `bool` | 是否显示鼠标光标（暂未实现，预留） |

#### ScreenShareStatus

| 字段 | 类型 | 说明 |
|------|------|------|
| `is_active` | `bool` | 是否正在共享 |
| `viewer_count` | `u32` | 当前观看人数 |
| `fps_actual` | `f32` | 实际帧率 |
| `bitrate_kbps` | `u32` | 总出站码率 |
| `uptime_secs` | `u64` | 已共享时长（秒） |
| `server_url` | `String` | 主访问地址 |
| `all_urls` | `Vec<String>` | **新增**：所有可用访问地址（多网卡支持） |
| `firewall_ok` | `bool` | **新增**：防火墙规则是否添加成功 |

---

## Tauri Commands

| Command | 参数 | 返回 | 说明 |
|---------|------|------|------|
| `screen_share_list_monitors` | 无 | `Vec<MonitorInfo>` | 枚举所有屏幕 |
| `screen_share_start` | `ScreenShareConfig` | `Result<String, String>` | 启动共享 |
| `screen_share_stop` | 无 | `Result<(), String>` | 停止共享 |
| `screen_share_get_status` | 无 | `ScreenShareStatus` | 状态快照 |

### Tauri 事件

| 事件名 | Payload | 方向 | 说明 |
|--------|---------|------|------|
| `screen-share-status` | `ScreenShareStatus` | Rust→Vue | 每秒推送 |
| `screen-share-log` | `{ level, message }` | Rust→Vue | 日志 |

---

## 内部架构

### 管道模型（不变）

```
Capture Thread (std::thread)
  → DXGI frame → JPEG encode → broadcast_tx.send(Arc<Bytes>)
      ↓                           ↓
  fps_counter++           ┌── Viewer 1 (axum async)
                          ├── Viewer 2
                          └── Viewer N
                               → MJPEG response → Browser <img>
```

### 修复后的捕获线程启动流程

```
1. Capturer::new(display)
2. 发送 placeholder 帧（1×1 灰色 JPEG）到 broadcast
3. 进入 frame() 循环
4. WouldBlock → sleep(1ms) → continue（不发 placeholder）
5. 成功帧 → encode_jpeg → broadcast_tx.send
```

---

## UI 设计

### 共享者页面（ScreenSharePage.vue）

#### 关键 UI 修复

1. **防火墙警告 Banner**：收到 `level: "warn"` 且消息含防火墙关键词时，在状态面板顶部显示醒目橙色 banner，包含可复制的 netsh 命令。

2. **QR Code 实际渲染**：安装 `qrcode` 包，使用 `<canvas>` 元素渲染，`watch(showQr)` 触发绘制。

3. **多 IP 地址显示**：当 `all_urls` 有多个时，在访问地址区域增加下拉/展开，显示"备用地址"。

4. **复制成功 Toast**：复制 URL 后在按钮旁显示 "已复制 ✓"（1.5s 后消失），而非仅写日志。

#### 布局结构（维持原有，补充细节）

```
┌─ Header: [MonitorUp] 屏幕共享 ─────────────────────────────┐
└────────────────────────────────────────────────────────────┘

┌─ 白色圆角卡片 ──────────────────────────────────────────────┐
│  ┌─ 左：配置区域(3/5) ──┐  ┌─ 右：状态面板(2/5) ──────────┐ │
│  │ 屏幕选择下拉框        │  │ ● 未启动 / ● 共享中          │ │
│  │ 端口号 | 密码开关      │  │                              │ │
│  │ 画质滑块             │  │ [防火墙警告 Banner - 橙色]    │ │
│  │ 帧率滑块             │  │                              │ │
│  │ 显示鼠标光标 checkbox │  │ 访问地址                     │ │
│  │                      │  │ http://192.168.x.x:9870      │ │
│  │ [错误信息区域]        │  │ [复制✓] [QR]                 │ │
│  │                      │  │                              │ │
│  │ [开始共享▶] / [停止■] │  │ [QR Canvas 128×128]          │ │
│  └──────────────────────┘  │                              │ │
│                             │ 观看人数 / 帧率 / 码率 / 时长 │ │
│                             └──────────────────────────────┘ │
│  ─────────── 日志折叠区域 ─────────────────────────────────  │
└────────────────────────────────────────────────────────────┘
```

#### 防火墙警告 Banner 设计

```
┌─────────────────────────────────────────────────────────┐
│ ⚠ 防火墙规则未能自动添加。                               │
│   若其他设备无法连接，请以管理员运行：                    │
│   [netsh advfirewall ... localport=9870]  [复制命令]     │
└─────────────────────────────────────────────────────────┘
```

样式：`bg-amber-50 border-amber-200 text-amber-800`，图标 `AlertTriangle`。

### 观看者页面（axum 内嵌 HTML）

维持原有设计，增加以下修复：

1. **5s 无数据自动重连**：`loadstart` 后 5s 内无 `load` 事件，主动刷新 `img.src`
2. **首次加载提示**：未收到画面前显示 "正在连接..." 占位文字

---

## 新增依赖

### Cargo.toml（Rust）

```toml
# 当前已有的（保持不变）
scrap = "0.5"
axum = { version = "0.8", features = ["ws"] }
bytes = "1"
sha2 = "0.10"
local-ip-address = "0.6"
image = { version = "0.25", features = ["jpeg"] }
async-stream = "0.3"
```

### package.json（前端）

```json
{
  "qrcode": "^1.5",
  "@types/qrcode": "^1.5"
}
```

---

## 验收标准

1. 共享者点击"开始共享"后，**局域网内其他机器**浏览器输入地址可看到实时屏幕画面（需防火墙规则生效，或用户手动开放端口）
2. 浏览器连接后 **3s 内**出现画面（修复 MJPEG 挂死问题）
3. 点击 QR Code 按钮后，**实际显示二维码图像**（修复未渲染问题）
4. 防火墙添加失败时，页面显示 **明确的警告 banner** 和可复制命令
5. 多网卡场景下，状态面板显示**所有可用 IP 地址**
6. 手机浏览器可正常观看，画面自适应
7. `pnpm tauri build` 构建通过

---

## 实现分阶段（修订）

### Phase 1：Bug 修复（优先）

1. `add_firewall_rule` 检查退出码，失败时 emit warn 日志
2. 前端收到防火墙警告时显示 banner + 复制命令功能
3. 修复 MJPEG 首次挂死：添加 placeholder 帧 + 浏览器端 5s 超时重连
4. 修复 `handler_index` 读取 `?error=1` query param
5. 枚举所有 LAN IP，`ScreenShareStatus` 新增 `all_urls` / `firewall_ok` 字段

### Phase 2：QR Code 渲染

1. `pnpm add qrcode @types/qrcode`
2. `ScreenSharePage.vue` 添加 `<canvas ref="qrCanvas">` 和 `watch` 逻辑
3. 复制成功改为短暂显示 "已复制 ✓" 文字而非仅写日志

### Phase 3：多 IP 支持

1. 状态面板"访问地址"区域，当 `all_urls.length > 1` 时显示备用地址折叠列表

### Phase 4：验证

1. 构建 + 功能测试（局域网实机验证）
