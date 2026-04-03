# 局域网共享工具修复计划（屏幕共享 + 文件共享）

> **目标**：解决浏览器无法访问的 Critical/High Bug，补全 QR Code 渲染，完善防火墙提示 UX  
> **参考 Spec**：
> - `docs/superpowers/specs/2026-04-03-screen-share-tool-design.md`（v2）
> - `docs/superpowers/specs/2026-04-03-file-share-tool-design.md`（v1）

---

## 问题优先级总览

| ID | 问题 | 影响 | 优先级 | 涉及文件 |
|----|------|------|--------|----------|
| F1 | 防火墙规则失败静默，外部设备无法访问 | 功能完全不可用 | P0 | screenshare.rs, fileshare.rs |
| F2 | MJPEG 首次连接挂死（WouldBlock 期间无帧） | 屏幕共享浏览器卡死 | P0 | screenshare.rs, viewer_html |
| F3 | IP 地址检测不准（多网卡/VPN） | 显示地址错误 | P1 | screenshare.rs, fileshare.rs |
| F4 | QR Code 按钮无实际渲染 | 功能缺失 | P1 | ScreenSharePage.vue |
| F5 | 密码错误提示硬编码 false | 小 UX 缺陷 | P2 | screenshare.rs |
| F6 | 连接数统计仅计下载，不计浏览 | 数据误导 | P2 | fileshare.rs, FileSharePage.vue |

---

## Phase 1：P0 Bug 修复（必须先做）

### 任务 1.1：防火墙规则失败检测 + 用户提示（双模块）

**目标**：让用户知道防火墙没有生效，并提供可操作的修复命令。

#### Rust 后端修改

**文件**：`src-tauri/src/screenshare.rs`

修改 `add_firewall_rule` 函数签名，接收 `app_handle` 并检查退出码：

```rust
fn add_firewall_rule(port: u16, app_handle: &AppHandle) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let ok = std::process::Command::new("netsh")
            .args([
                "advfirewall", "firewall", "add", "rule",
                &format!("name=FileSyncTool_SS_{}", port),
                "dir=in", "action=allow", "protocol=TCP",
                &format!("localport={}", port),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            let _ = app_handle.emit("screen-share-log", serde_json::json!({
                "level": "warn",
                "message": format!("__FIREWALL_FAILED__:{}", port)
            }));
        }
        return ok;
    }
    #[allow(unreachable_code)]
    true
}
```

> 注：使用 `__FIREWALL_FAILED__:{port}` 作为魔法标记，前端检测此前缀来展示 banner（避免字符串 i18n 的耦合）。

同样修改 `src-tauri/src/fileshare.rs` 中的 `add_firewall_rule`，使用 `__FILESHARE_FIREWALL_FAILED__:{port}` 标记。

**在 `screen_share_start` 中调用**：
```rust
let firewall_ok = add_firewall_rule(config.port, &app_handle);
// 将 firewall_ok 存入 handle 供 status 读取
*handle.firewall_ok.lock().unwrap() = firewall_ok;
```

**`ScreenShareHandle` 新增字段**：
```rust
pub struct ScreenShareHandle {
    // ... 已有字段 ...
    firewall_ok: Mutex<bool>,
}
```

**`ScreenShareStatus` 新增字段**：
```rust
pub struct ScreenShareStatus {
    // ... 已有字段 ...
    pub firewall_ok: bool,
}
```

相同改动同步到 `fileshare.rs` / `FileShareStatus` / `FileShareHandle`。

#### 前端修改

**文件**：`src/pages/ScreenSharePage.vue`

```typescript
// 新增 state
const firewallWarning = ref<string | null>(null);

// 监听日志中的防火墙失败标记
unlistenLog = await listen<{ level: string; message: string }>('screen-share-log', (event) => {
  if (event.payload.message.startsWith('__FIREWALL_FAILED__:')) {
    const port = event.payload.message.split(':')[1];
    firewallWarning.value = port;
    return; // 不写入日志
  }
  addLog(event.payload.level, event.payload.message);
});
```

模板中在状态面板顶部添加 banner（仅共享中时显示）：

```html
<!-- 防火墙警告 Banner -->
<div v-if="isActive && firewallWarning"
     class="mb-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5">
  <div class="flex items-start gap-2">
    <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
    <div class="min-w-0 flex-1">
      <p class="text-xs font-semibold text-amber-800">{{ t('tools.screenShare.firewallWarningTitle') }}</p>
      <code class="mt-1 block truncate rounded bg-amber-100 px-2 py-1 text-xs text-amber-700">
        netsh advfirewall firewall add rule name=FileSyncTool dir=in action=allow protocol=TCP localport={{ firewallWarning }}
      </code>
      <button @click="copyFirewallCmd" class="mt-1 text-xs text-amber-600 underline hover:text-amber-800">
        {{ t('tools.screenShare.firewallCopyCmd') }}
      </button>
    </div>
  </div>
</div>
```

同样改动应用到 `FileSharePage.vue`（替换事件名和 i18n key）。

**i18n 新增 key（messages.ts）**：
```typescript
// 屏幕共享
firewallWarningTitle: 'Firewall rule not added (needs admin). Other devices may not connect.',
firewallCopyCmd: 'Copy command',

// 文件共享
firewallWarningTitle: 'Firewall rule not added (needs admin). Other devices may not connect.',
firewallCopyCmd: 'Copy command',
```

---

### 任务 1.2：修复 MJPEG 首次连接挂死

**目标**：浏览器连接后 3s 内出现画面，不卡死。

#### 方案 A：Placeholder 帧（推荐）

`screenshare.rs` 中的 `capture_loop` 在第一帧成功前，每 500ms 向广播发送一个 1×1 灰色 JPEG：

```rust
fn capture_loop(...) {
    let mut capturer = match create_capturer(monitor_index) { ... };
    let width = capturer.width();
    let height = capturer.height();
    let frame_interval = Duration::from_millis(1000 / fps.max(1) as u64);
    
    let mut first_frame_sent = false;

    // 在进入主循环之前，先向广播发一个 placeholder
    {
        let placeholder = make_placeholder_jpeg();
        let _ = tx.send(Arc::new(Bytes::from(placeholder)));
    }

    loop {
        // ... 现有捕获逻辑 ...
        match capturer.frame() {
            Ok(frame) => {
                // ... encode + broadcast ...
                first_frame_sent = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 还没就绪，每 500ms 发一个 placeholder 让浏览器不超时
                if !first_frame_sent {
                    let placeholder = make_placeholder_jpeg();
                    let _ = tx.send(Arc::new(Bytes::from(placeholder)));
                    std::thread::sleep(Duration::from_millis(500));
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
                continue;
            }
            // ... 其他错误处理 ...
        }
    }
}

fn make_placeholder_jpeg() -> Vec<u8> {
    // 生成 1×1 深蓝色 JPEG（与 viewer 背景色 #0f172a 接近）
    let rgb = [15u8, 23u8, 42u8]; // #0f172a
    encode_jpeg(&[15, 23, 42, 255], 1, 1, 1, 30) // BGRA: B=42,G=23,R=15,A=255
}
```

#### 方案 B：浏览器端超时重连（配合方案 A）

修改 `viewer_html()` 中的 JS：

```javascript
let reconnectTimer;
function scheduleReconnect(delayMs) {
  clearTimeout(reconnectTimer);
  reconnectTimer = setTimeout(() => {
    img.src = '/stream?t=' + Date.now();
  }, delayMs);
}

img.onerror = function() {
  alive = false;
  dot.className = 'dot dot-off';
  st.textContent = 'Disconnected';
  scheduleReconnect(3000);
};

// 5秒内没有 load 事件（图像从未成功显示）→ 强制重连
let initialTimer = setTimeout(() => {
  if (!alive) return;
  img.src = '/stream?t=' + Date.now();
}, 5000);

img.onload = function() {
  clearTimeout(initialTimer);
  if (!alive) { alive = true; dot.className = 'dot dot-on'; st.textContent = 'Connected'; }
};
```

---

## Phase 2：P1 Bug 修复

### 任务 2.1：多网卡 IP 枚举

**目标**：`all_urls` 展示所有可访问地址，不依赖单个 `local_ip` 返回值。

#### Rust 后端

在 `screenshare.rs` 和 `fileshare.rs` 中添加共用函数（可提取到 `src-tauri/src/network.rs` 或各自 rs 文件内）：

```rust
fn get_lan_ips() -> Vec<String> {
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

`screen_share_start` / `file_share_start` 修改：

```rust
let all_ips = get_lan_ips();
let primary_ip = all_ips.first()
    .cloned()
    .unwrap_or_else(|| "127.0.0.1".to_string());
let server_url = format!("http://{}:{}", primary_ip, config.port);
let all_urls: Vec<String> = all_ips.iter()
    .map(|ip| format!("http://{}:{}", ip, config.port))
    .collect();
```

#### 前端 tauri.ts 类型更新

```typescript
export interface ScreenShareStatus {
  // ...
  all_urls: string[];
  firewall_ok: boolean;
}

export interface FileShareStatus {
  // ...
  all_urls: string[];
  firewall_ok: boolean;
  download_count: number;    // 重命名自 connection_count
}
```

#### 前端 UI 更新

在状态面板"访问地址"区域，当 `all_urls.length > 1` 时，展示"备用地址"可展开列表：

```html
<!-- 备用地址（仅多 IP 时显示） -->
<div v-if="status.all_urls.length > 1" class="mt-2">
  <button @click="showAltUrls = !showAltUrls"
          class="text-xs text-slate-400 underline hover:text-slate-600">
    {{ showAltUrls ? t('tools.screenShare.hideAltUrls') : t('tools.screenShare.showAltUrls', { n: status.all_urls.length - 1 }) }}
  </button>
  <div v-if="showAltUrls" class="mt-1 space-y-1">
    <div v-for="url in status.all_urls.slice(1)" :key="url"
         class="flex items-center gap-1 text-xs text-slate-500">
      <code class="flex-1 truncate">{{ url }}</code>
      <button @click="copyText(url)" class="text-slate-400 hover:text-slate-600">
        <Copy class="h-3 w-3" />
      </button>
    </div>
  </div>
</div>
```

---

### 任务 2.2：QR Code 渲染（ScreenSharePage）

**目标**：点击 QR Code 按钮实际显示二维码。

#### 安装依赖

```bash
pnpm add qrcode
pnpm add -D @types/qrcode
```

#### 代码修改（ScreenSharePage.vue）

```typescript
import QRCode from 'qrcode';

const qrCanvas = ref<HTMLCanvasElement | null>(null);

// watch showQr 和 serverUrl，在 canvas 上渲染
watch([showQr, serverUrl], async ([show, url]) => {
  if (!show || !url) return;
  await nextTick(); // 等 canvas 渲染到 DOM
  if (qrCanvas.value) {
    await QRCode.toCanvas(qrCanvas.value, url, {
      width: 128,
      margin: 1,
      color: { dark: '#1e293b', light: '#ffffff' },
    });
  }
});
```

模板中在 URL 区域下方条件渲染（替换原本空白的 showQr 逻辑）：

```html
<!-- QR Code Canvas -->
<div v-if="showQr" class="mt-3 flex justify-center">
  <div class="rounded-lg border border-slate-200 bg-white p-2">
    <canvas ref="qrCanvas" width="128" height="128" />
    <p class="mt-1 text-center text-xs text-slate-400">{{ t('tools.screenShare.qrCodeHint') }}</p>
  </div>
</div>
```

---

## Phase 3：P2 改进

### 任务 3.1：屏幕共享密码错误提示服务端化

**文件**：`src-tauri/src/screenshare.rs`

```rust
#[derive(Deserialize)]
struct IndexQuery { error: Option<u8> }

async fn handler_index(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(q): Query<IndexQuery>,    // 新增 query param 读取
    headers: HeaderMap,
) -> Response {
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            let has_error = q.error.unwrap_or(0) == 1;  // 服务端读取
            return Html(login_html(has_error)).into_response();
        }
    }
    Html(viewer_html()).into_response()
}
```

同时简化 `login_html` 中的 JS（去掉客户端检测 `?error=1` 的 JS，因为服务端已处理）。

### 任务 3.2：文件共享连接数语义修正

**目标**：避免用户误解"连接数 0"的含义。

**Rust**：`FileShareStatus` 字段重命名 `connection_count` → `download_count`（serde rename 或直接重命名）。

**TypeScript**：`tauri.ts` 中 `FileShareStatus.connection_count` → `download_count`。

**Vue**：`FileSharePage.vue` 中对应的 template 字段名和 i18n key：
- `t('tools.fileShare.connections')` → `t('tools.fileShare.downloadCount')`

**messages.ts** 新增：
```typescript
downloadCount: 'Active Downloads',   // en
downloadCount: '下载中',              // zh
```

---

## 变更文件汇总

| 文件 | 变更内容 |
|------|----------|
| `src-tauri/src/screenshare.rs` | 防火墙检测、placeholder 帧、多 IP、query param 密码错误、ScreenShareStatus 新字段 |
| `src-tauri/src/fileshare.rs` | 防火墙检测、多 IP、FileShareStatus 新字段（download_count, all_urls, firewall_ok） |
| `src-tauri/src/main.rs` | AppState 中 ScreenShareHandle/FileShareHandle 新增 firewall_ok 字段（如果需要） |
| `src/lib/tauri.ts` | ScreenShareStatus、FileShareStatus 类型新增字段 |
| `src/pages/ScreenSharePage.vue` | 防火墙 banner、QR Code canvas、多 IP 展示、复制提示 |
| `src/pages/FileSharePage.vue` | 防火墙 banner、多 IP 展示、download_count 重命名 |
| `src/locales/messages.ts` | 新增 firewallWarningTitle、firewallCopyCmd、showAltUrls、hideAltUrls、downloadCount |
| `package.json` / `pnpm-lock.yaml` | 新增 `qrcode`、`@types/qrcode` |

---

## 测试清单

### 功能测试

- [ ] 以**普通用户**（非管理员）启动屏幕共享，验证防火墙 banner 出现
- [ ] 复制防火墙命令，以管理员运行后，从其他机器浏览器访问成功
- [ ] 屏幕共享启动后，浏览器 **3s 内**出现画面（而非卡死）
- [ ] 点击 QR Code 按钮，**二维码实际显示**
- [ ] 手机扫描二维码，浏览器正常打开屏幕共享
- [ ] 多网卡环境下，状态面板显示多个访问地址
- [ ] 文件共享启动后，局域网内其他设备可浏览目录、下载文件
- [ ] 文件共享有密码时：未登录跳转登录页，错误密码显示提示
- [ ] 文件共享 ZIP 下载功能正常
- [ ] 停止共享后再次启动，状态正常重置

### 构建验证

```bash
pnpm tauri build
```

---

## 执行顺序

```
Phase 1.1 (防火墙检测)
    → Rust 后端先改（screenshare.rs + fileshare.rs）
    → 前端 banner + i18n key
Phase 1.2 (MJPEG 挂死)
    → screenshare.rs capture_loop + viewer_html
Phase 2.1 (多 IP)
    → Rust get_lan_ips + Status 新字段
    → tauri.ts 类型 + Vue 展示
Phase 2.2 (QR Code)
    → pnpm add qrcode
    → ScreenSharePage.vue watch + canvas
Phase 3.1 (密码错误)
    → screenshare.rs handler_index
Phase 3.2 (连接数重命名)
    → fileshare.rs + tauri.ts + FileSharePage.vue + messages.ts
构建验证
```
