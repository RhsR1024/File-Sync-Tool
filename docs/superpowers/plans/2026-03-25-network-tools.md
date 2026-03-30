# Network Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "网络工具" (Network Tools) page under "Other Tools" with 5 tabs: Ping Scan, TCP Connections, Port Test, Wake-on-LAN, and Subnet Calculator.

**Architecture:** Single Vue page (`NetworkToolsPage.vue`) with tab switching between 5 child components. Rust backend in `network.rs` provides Tauri commands for Ping, TCP stats, port testing, and WOL. Subnet calculator is pure frontend. Network tools config (port presets, WOL devices) stored separately from main `AppConfig` in a dedicated JSON file to avoid bloating the existing config.

**Tech Stack:** Vue 3 + TypeScript + Tailwind CSS 4 (frontend), Rust + Tokio (backend), Tauri 2.x events for real-time ping results.

---

## File Structure

### New Files
| File | Responsibility |
|------|---------------|
| `src-tauri/src/network.rs` | All network tool Tauri commands (ping_scan, cancel_ping_scan, get_tcp_connections, test_ports, send_wol) + data types |
| `src/pages/NetworkToolsPage.vue` | Tab container page with 5 tabs |
| `src/components/network/PingScanTab.vue` | Ping scan UI: input form, grid view, table view |
| `src/components/network/TcpConnectionsTab.vue` | TCP connection statistics UI |
| `src/components/network/PortTestTab.vue` | Port connectivity test UI with custom presets |
| `src/components/network/WakeOnLanTab.vue` | WOL UI with saved devices |
| `src/components/network/SubnetCalcTab.vue` | Pure frontend subnet calculator |

### Modified Files
| File | Changes |
|------|---------|
| `src-tauri/src/main.rs` | Add `mod network;`, register new commands in `invoke_handler` |
| `src-tauri/Cargo.toml` | No new deps needed (`tokio`, `serde`, `serde_json` already present) |
| `src/router/index.ts` | Add `/tools/network` route |
| `src/components/Sidebar.vue` | Add "网络工具" child under tools menu |
| `src/lib/tauri.ts` | Add network tool types + invoke wrappers |
| `src/locales/messages.ts` | Add `networkTools.*` translations (en + zh) |

---

### Task 1: Rust Backend — network.rs (Ping + Cancel)

**Files:**
- Create: `src-tauri/src/network.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Create `network.rs` with ping scan types and command**

```rust
// src-tauri/src/network.rs
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, State};
use tokio::sync::Semaphore;

// ── Types ──

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub ip: String,
    pub alive: bool,
    pub latency_ms: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingScanRequest {
    pub prefix: String,    // "192.168.1"
    pub start: u8,
    pub end: u8,
    pub timeout_ms: u32,
}

// ── State ──

pub struct NetworkState {
    pub ping_cancel: Arc<AtomicBool>,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            ping_cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

// ── Ping via TCP connect (ports 80, 443, 22 fallback) ──

fn tcp_ping(ip: Ipv4Addr, timeout: Duration) -> (bool, Option<f64>) {
    let ports = [80, 443, 22, 135, 445];
    for port in ports {
        let addr = SocketAddr::new(IpAddr::V4(ip), port);
        let start = Instant::now();
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(_) => return (true, Some(start.elapsed().as_secs_f64() * 1000.0)),
            Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                // Port refused = host is alive
                return (true, Some(start.elapsed().as_secs_f64() * 1000.0));
            }
            Err(_) => continue,
        }
    }
    (false, None)
}

// ── Commands ──

#[tauri::command]
pub async fn ping_scan(
    app_handle: tauri::AppHandle,
    state: State<'_, NetworkState>,
    request: PingScanRequest,
) -> Result<(), String> {
    // Validate prefix
    let parts: Vec<&str> = request.prefix.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.parse::<u8>().is_err()) {
        return Err("Invalid prefix: must be 3 octets like 192.168.1".to_string());
    }
    if request.start > request.end {
        return Err("Start must be <= end".to_string());
    }
    if request.start == 0 && request.end == 0 {
        return Err("Range cannot be 0-0".to_string());
    }

    state.ping_cancel.store(false, Ordering::SeqCst);
    let cancel = state.ping_cancel.clone();
    let timeout = Duration::from_millis(request.timeout_ms.max(100).min(10000) as u64);
    let concurrency = 50usize;
    let semaphore = Arc::new(Semaphore::new(concurrency));

    let mut handles = Vec::new();

    for i in request.start..=request.end {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        let ip_str = format!("{}.{}", request.prefix, i);
        let ip: Ipv4Addr = ip_str.parse().map_err(|e| format!("Invalid IP: {}", e))?;
        let app = app_handle.clone();
        let cancel_flag = cancel.clone();
        let sem = semaphore.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            if cancel_flag.load(Ordering::SeqCst) {
                return;
            }
            let (alive, latency_ms) =
                tokio::task::spawn_blocking(move || tcp_ping(ip, timeout))
                    .await
                    .unwrap_or((false, None));

            let result = PingResult {
                ip: ip_str,
                alive,
                latency_ms: latency_ms.map(|ms| (ms * 100.0).round() / 100.0),
            };
            let _ = app.emit("ping-result", &result);
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    let _ = app_handle.emit("ping-scan-complete", ());
    Ok(())
}

#[tauri::command]
pub fn cancel_ping_scan(state: State<'_, NetworkState>) {
    state.ping_cancel.store(true, Ordering::SeqCst);
}
```

- [ ] **Step 2: Register module and commands in main.rs**

In `main.rs`:
- Add `mod network;` after the existing module declarations
- In `.setup()`, add: `app.manage(network::NetworkState::default());`
- In `.invoke_handler()`, add: `network::ping_scan, network::cancel_ping_scan,`

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: Compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/network.rs src-tauri/src/main.rs
git commit -m "feat: 添加网络工具 Rust 后端 - Ping 扫描命令"
```

---

### Task 2: Rust Backend — TCP Connections, Port Test, WOL

**Files:**
- Modify: `src-tauri/src/network.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Add TCP connection stats command to network.rs**

```rust
// ── TCP Connection Stats ──

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TcpConnectionStats {
    pub total: usize,
    pub by_state: Vec<StatCount>,
    pub by_remote_ip: Vec<IpCount>,
    pub by_port: Vec<PortCount>,
}

#[derive(Serialize, Clone)]
pub struct StatCount {
    pub state: String,
    pub count: usize,
}

#[derive(Serialize, Clone)]
pub struct IpCount {
    pub ip: String,
    pub count: usize,
}

#[derive(Serialize, Clone)]
pub struct PortCount {
    pub port: u16,
    pub name: String,
    pub count: usize,
}

fn well_known_port_name(port: u16) -> &'static str {
    match port {
        22 => "SSH",
        80 => "HTTP",
        443 => "HTTPS",
        3306 => "MySQL",
        5432 => "PostgreSQL",
        6379 => "Redis",
        8080 => "HTTP-Alt",
        8443 => "HTTPS-Alt",
        3389 => "RDP",
        21 => "FTP",
        25 => "SMTP",
        53 => "DNS",
        110 => "POP3",
        143 => "IMAP",
        27017 => "MongoDB",
        _ => "",
    }
}

#[tauri::command]
pub fn get_tcp_connections() -> Result<TcpConnectionStats, String> {
    let output = std::process::Command::new("netstat")
        .args(["-n", "-p", "TCP"])
        .output()
        .map_err(|e| format!("Failed to run netstat: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut state_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut ip_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut port_map: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    let mut total = 0usize;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Windows netstat -n: Proto  Local Address  Foreign Address  State
        if parts.len() >= 4 && parts[0].eq_ignore_ascii_case("TCP") {
            total += 1;
            let state = parts[3].to_string();
            *state_map.entry(state).or_insert(0) += 1;

            let foreign = parts[2];
            // Parse remote IP (handle IPv4 ip:port format)
            if let Some(colon_pos) = foreign.rfind(':') {
                let ip = &foreign[..colon_pos];
                if ip != "0.0.0.0" && ip != "*" {
                    *ip_map.entry(ip.to_string()).or_insert(0) += 1;
                }
                if let Ok(port) = foreign[colon_pos + 1..].parse::<u16>() {
                    if port > 0 {
                        *port_map.entry(port).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut by_state: Vec<StatCount> = state_map.into_iter().map(|(state, count)| StatCount { state, count }).collect();
    by_state.sort_by(|a, b| b.count.cmp(&a.count));

    let mut by_remote_ip: Vec<IpCount> = ip_map.into_iter().map(|(ip, count)| IpCount { ip, count }).collect();
    by_remote_ip.sort_by(|a, b| b.count.cmp(&a.count));
    by_remote_ip.truncate(20);

    let mut by_port: Vec<PortCount> = port_map.into_iter().map(|(port, count)| PortCount {
        port,
        name: well_known_port_name(port).to_string(),
        count,
    }).collect();
    by_port.sort_by(|a, b| b.count.cmp(&a.count));
    by_port.truncate(20);

    Ok(TcpConnectionStats { total, by_state, by_remote_ip, by_port })
}
```

- [ ] **Step 2: Add port test command**

```rust
// ── Port Connectivity Test ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortTestRequest {
    pub host: String,
    pub ports: Vec<u16>,
    pub timeout_ms: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PortTestResult {
    pub port: u16,
    pub service: String,
    pub open: bool,
    pub latency_ms: Option<f64>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn test_ports(request: PortTestRequest) -> Result<Vec<PortTestResult>, String> {
    if request.ports.len() > 1000 {
        return Err("Too many ports (max 1000)".to_string());
    }

    let timeout = Duration::from_millis(request.timeout_ms.max(100).min(10000) as u64);
    let host = request.host.clone();

    // Resolve host once
    let addr: IpAddr = if let Ok(ip) = host.parse::<IpAddr>() {
        ip
    } else {
        // DNS resolve
        let host_port = format!("{}:0", host);
        match tokio::net::lookup_host(&host_port).await {
            Ok(mut addrs) => match addrs.next() {
                Some(a) => a.ip(),
                None => return Err(format!("Cannot resolve host: {}", host)),
            },
            Err(e) => return Err(format!("DNS lookup failed: {}", e)),
        }
    };

    let sem = Arc::new(Semaphore::new(20));
    let mut handles = Vec::new();

    for port in request.ports {
        let sem = sem.clone();
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let socket_addr = SocketAddr::new(addr, port);
            let start = Instant::now();

            let (open, latency, error) =
                match tokio::task::spawn_blocking(move || {
                    TcpStream::connect_timeout(&socket_addr, timeout)
                })
                .await
                .unwrap()
                {
                    Ok(_) => (true, Some(start.elapsed().as_secs_f64() * 1000.0), None),
                    Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                        (false, None, Some("Connection refused".to_string()))
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        (false, None, Some("Timeout".to_string()))
                    }
                    Err(e) => (false, None, Some(e.to_string())),
                };

            PortTestResult {
                port,
                service: well_known_port_name(port).to_string(),
                open,
                latency_ms: latency.map(|ms| (ms * 100.0).round() / 100.0),
                error,
            }
        });
        handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }
    results.sort_by_key(|r| r.port);
    Ok(results)
}
```

- [ ] **Step 3: Add WOL command**

```rust
// ── Wake-on-LAN ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WolRequest {
    pub mac: String,
    pub broadcast: String,
    pub port: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WolResult {
    pub success: bool,
    pub message: String,
}

fn parse_mac(mac: &str) -> Result<[u8; 6], String> {
    let clean: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() != 12 {
        return Err("Invalid MAC address format".to_string());
    }
    let mut bytes = [0u8; 6];
    for i in 0..6 {
        bytes[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16)
            .map_err(|_| "Invalid MAC address".to_string())?;
    }
    Ok(bytes)
}

#[tauri::command]
pub fn send_wol(request: WolRequest) -> WolResult {
    let mac_bytes = match parse_mac(&request.mac) {
        Ok(b) => b,
        Err(e) => return WolResult { success: false, message: e },
    };

    // Build magic packet: 6 x 0xFF + 16 x MAC
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }

    let bind_result = std::net::UdpSocket::bind("0.0.0.0:0");
    let socket = match bind_result {
        Ok(s) => s,
        Err(e) => return WolResult { success: false, message: format!("Failed to bind socket: {}", e) },
    };

    if let Err(e) = socket.set_broadcast(true) {
        return WolResult { success: false, message: format!("Failed to enable broadcast: {}", e) };
    }

    let target = format!("{}:{}", request.broadcast, request.port);
    match socket.send_to(&packet, &target) {
        Ok(_) => WolResult {
            success: true,
            message: format!("Magic packet sent to {} (broadcast: {}:{})", request.mac, request.broadcast, request.port),
        },
        Err(e) => WolResult {
            success: false,
            message: format!("Failed to send packet: {}", e),
        },
    }
}
```

- [ ] **Step 4: Register new commands in main.rs**

Add to `invoke_handler`: `network::get_tcp_connections, network::test_ports, network::send_wol,`

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/network.rs src-tauri/src/main.rs
git commit -m "feat: 添加 TCP 连接统计、端口测试、WOL 命令"
```

---

### Task 3: Frontend Types + Tauri Invoke Wrappers

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add network tool types and invoke functions to tauri.ts**

Append to the end of `src/lib/tauri.ts`:

```typescript
// ─── Network Tools ─────────────────────────────────────

export interface PingResult {
  ip: string;
  alive: boolean;
  latencyMs: number | null;
}

export interface PingScanRequest {
  prefix: string;
  start: number;
  end: number;
  timeoutMs: number;
}

export interface TcpConnectionStats {
  total: number;
  byState: { state: string; count: number }[];
  byRemoteIp: { ip: string; count: number }[];
  byPort: { port: number; name: string; count: number }[];
}

export interface PortTestRequest {
  host: string;
  ports: number[];
  timeoutMs: number;
}

export interface PortTestResult {
  port: number;
  service: string;
  open: boolean;
  latencyMs: number | null;
  error: string | null;
}

export interface WolRequest {
  mac: string;
  broadcast: string;
  port: number;
}

export interface WolResult {
  success: boolean;
  message: string;
}

// Port preset & WOL device (persisted in localStorage)
export interface PortPreset {
  name: string;
  ports: string;
}

export interface WolDevice {
  name: string;
  mac: string;
  broadcast: string;
  port: number;
}

export async function pingScan(request: PingScanRequest): Promise<void> {
  await invoke('ping_scan', { request });
}

export async function cancelPingScan(): Promise<void> {
  await invoke('cancel_ping_scan');
}

export async function getTcpConnections(): Promise<TcpConnectionStats> {
  return await invoke('get_tcp_connections');
}

export async function testPorts(request: PortTestRequest): Promise<PortTestResult[]> {
  return await invoke('test_ports', { request });
}

export async function sendWol(request: WolRequest): Promise<WolResult> {
  return await invoke('send_wol', { request });
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat: 添加网络工具前端类型定义与 invoke 封装"
```

---

### Task 4: i18n Translations

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Add sidebar entry + networkTools translations**

In `sidebar` section (both `en` and `zh`):
```
networkTools: 'Network Tools',   // en
networkTools: '网络工具',          // zh
```

Add `networkTools` object in both `en` and `zh` with keys for all 5 tabs. Key structure:

```typescript
networkTools: {
  title: 'Network Tools',
  tabs: {
    pingScan: 'Ping Scan',
    tcpConnections: 'TCP Connections',
    portTest: 'Port Test',
    wol: 'Wake-on-LAN',
    subnetCalc: 'Subnet Calculator',
  },
  ping: {
    prefix: 'Subnet Prefix',
    prefixPlaceholder: '192.168.1',
    start: 'Start',
    end: 'End',
    timeoutMs: 'Timeout(ms)',
    startScan: 'Start Scan',
    stopScan: 'Stop Scan',
    scanning: 'Scanning',
    complete: 'Complete',
    online: 'Online',
    offline: 'Offline',
    waiting: 'Waiting',
    gridView: 'Grid',
    tableView: 'Table',
    ipAddress: 'IP Address',
    status: 'Status',
    latency: 'Latency',
    filterAll: 'All',
    filterOnline: 'Online Only',
    filterOffline: 'Offline Only',
    exportCsv: 'Export CSV',
    prefixError: 'Please enter a valid 3-octet prefix, e.g. 192.168.1',
    rangeError: 'Start must be ≤ End, range 1-254',
  },
  tcp: {
    refresh: 'Refresh',
    autoRefresh: 'Auto Refresh (5s)',
    lastUpdate: 'Last Update',
    total: 'Total',
    byRemoteIp: 'By Remote IP (Top 20)',
    byPort: 'By Port (Top 20)',
  },
  port: {
    targetHost: 'Target Host',
    targetPlaceholder: 'IP or hostname',
    ports: 'Ports',
    portsPlaceholder: '80,443,22 or 8000-8100',
    timeoutMs: 'Timeout(ms)',
    startTest: 'Start Test',
    testing: 'Testing...',
    presets: 'Presets',
    customPresets: 'Custom Presets',
    addPreset: 'Add Preset',
    editPreset: 'Edit',
    deletePreset: 'Delete',
    presetName: 'Preset Name',
    presetPorts: 'Ports',
    save: 'Save',
    cancel: 'Cancel',
    port: 'Port',
    service: 'Service',
    status: 'Status',
    latency: 'Latency',
    open: 'Open',
    closed: 'Closed',
    hostError: 'Please enter a valid IP or hostname',
    portsError: 'Please enter valid ports',
  },
  wol: {
    macAddress: 'MAC Address',
    macPlaceholder: 'AA:BB:CC:DD:EE:FF',
    broadcast: 'Broadcast',
    broadcastPlaceholder: '255.255.255.255',
    port: 'Port',
    sendWol: 'Send WOL',
    savedDevices: 'Saved Devices',
    saveCurrent: '+ Save Current',
    wake: 'Wake',
    delete: 'Delete',
    deviceName: 'Device Name',
    macError: 'Invalid MAC address format',
    noDevices: 'No saved devices',
  },
  subnet: {
    ipAddress: 'IP Address',
    ipPlaceholder: '192.168.1.0',
    cidr: 'CIDR Prefix',
    calculate: 'Calculate',
    networkAddr: 'Network Address',
    broadcastAddr: 'Broadcast Address',
    subnetMask: 'Subnet Mask',
    wildcardMask: 'Wildcard Mask',
    ipRange: 'Usable IP Range',
    hostCount: 'Usable Hosts',
    binary: 'Binary Representation',
    ipLabel: 'IP',
    maskLabel: 'Mask',
    commonCidr: 'Common',
  },
},
```

Chinese translations follow the same structure with Chinese values.

- [ ] **Step 2: Commit**

```bash
git add src/locales/messages.ts
git commit -m "feat: 添加网络工具 i18n 翻译 (中英文)"
```

---

### Task 5: Router + Sidebar

**Files:**
- Modify: `src/router/index.ts`
- Modify: `src/components/Sidebar.vue`

- [ ] **Step 1: Add route**

In `src/router/index.ts`, add to routes array:

```typescript
{
  path: '/tools/network',
  component: () => import('../pages/NetworkToolsPage.vue'),
},
```

- [ ] **Step 2: Add sidebar entry**

In `src/components/Sidebar.vue`, add to the tools children array:

```typescript
{ name: t('sidebar.networkTools'), path: '/tools/network' },
```

- [ ] **Step 3: Commit**

```bash
git add src/router/index.ts src/components/Sidebar.vue
git commit -m "feat: 添加网络工具路由和侧边栏入口"
```

---

### Task 6: NetworkToolsPage.vue (Tab Container)

**Files:**
- Create: `src/pages/NetworkToolsPage.vue`

- [ ] **Step 1: Create the tab container page**

A page with:
- Header with icon + title
- Tab bar with 5 tabs
- Dynamic component switching via `<component :is="...">`
- Active tab stored in `ref<string>('ping')`
- Tab definitions: `{ id: string, label: string, component: Component }`

Follow the existing page patterns (use `useI18n`, Tailwind CSS, lucide icons).

Use `Network` or `Globe` icon from lucide-vue-next for the header.

- [ ] **Step 2: Commit**

```bash
git add src/pages/NetworkToolsPage.vue
git commit -m "feat: 添加网络工具页面 Tab 容器"
```

---

### Task 7: PingScanTab.vue

**Files:**
- Create: `src/components/network/PingScanTab.vue`

- [ ] **Step 1: Create Ping scan component**

Input area:
- Prefix input field (3 octets, e.g. "192.168.1")
- Start/End number inputs (default 1/254)
- Timeout input (default 1000)
- Start/Stop scan button

Validation:
- Prefix: regex `/^\d{1,3}\.\d{1,3}\.\d{1,3}$/`, each octet 0-255
- Start/End: 1-254, start ≤ end
- Show inline error messages with red border

Results:
- Stats bar: scanned count / total, online count, offline count
- Grid/Table toggle button
- Progress bar
- Grid view: 16-column CSS grid, each cell shows `.N` suffix
  - Colors: green (#10b981) = online, gray (#e2e8f0) = offline, yellow (#fbbf24) = scanning, dark (#334155) = waiting
  - Tooltip on hover showing full IP + latency
- Table view: sortable table with IP, status badge, latency
  - Filter radio buttons: All / Online Only / Offline Only
  - Export CSV button

Event handling:
- Listen to `ping-result` event via `listen()` from `@tauri-apps/api/event`
- Listen to `ping-scan-complete` event
- Each result updates a `Map<string, PingResult>` reactively
- Call `pingScan()` to start, `cancelPingScan()` to stop

- [ ] **Step 2: Commit**

```bash
git add src/components/network/PingScanTab.vue
git commit -m "feat: 添加 Ping 网段扫描 Tab 组件"
```

---

### Task 8: TcpConnectionsTab.vue

**Files:**
- Create: `src/components/network/TcpConnectionsTab.vue`

- [ ] **Step 1: Create TCP connections component**

Controls:
- Refresh button
- Auto-refresh checkbox (5s interval via `setInterval`, clear on unmount)
- Last update timestamp

Summary cards (5 cards in a row):
- Total connections (blue)
- ESTABLISHED count (green)
- TIME_WAIT count (yellow)
- CLOSE_WAIT count (pink)
- LISTENING count (gray)
- Dynamically map from `byState` data

Two panels side by side:
- Left: "By Remote IP (Top 20)" - table with IP, count, bar chart
- Right: "By Port (Top 20)" - table with port:name, count, bar chart
- Bar width calculated as percentage of max count

Call `getTcpConnections()` on mount and on refresh.

- [ ] **Step 2: Commit**

```bash
git add src/components/network/TcpConnectionsTab.vue
git commit -m "feat: 添加 TCP 连接统计 Tab 组件"
```

---

### Task 9: PortTestTab.vue

**Files:**
- Create: `src/components/network/PortTestTab.vue`

- [ ] **Step 1: Create port test component**

Input area:
- Host input (IP or hostname)
- Ports input (comma-separated or range like 8000-8100)
- Timeout input (default 3000)
- Start Test button

Port presets:
- Built-in presets: Web(80,443), SSH(22), Database(3306,5432,6379), Common(22,80,443,3306,5432,6379,8080,8443)
- Custom presets stored in `localStorage` key `networkTools.portPresets`
- Clicking a preset fills the ports input
- "Add Preset" opens inline form (name + ports) to save
- Edit/Delete on custom presets

Parsing ports input:
- Split by comma, each item is either a number or a range "start-end"
- Expand ranges, deduplicate, sort
- Validate: 1-65535, max 1000 total

Results table:
- Port, Service, Status (Open/Closed badge), Latency, Error
- Call `testPorts()` on submit

- [ ] **Step 2: Commit**

```bash
git add src/components/network/PortTestTab.vue
git commit -m "feat: 添加端口连通性测试 Tab 组件（支持自定义预设）"
```

---

### Task 10: WakeOnLanTab.vue

**Files:**
- Create: `src/components/network/WakeOnLanTab.vue`

- [ ] **Step 1: Create WOL component**

Input area:
- MAC address input (validate format: `XX:XX:XX:XX:XX:XX` or `XX-XX-XX-XX-XX-XX`)
- Broadcast address input (default 255.255.255.255)
- Port input (default 9)
- Send WOL button

Saved devices:
- Stored in `localStorage` key `networkTools.wolDevices`
- Table: device name, MAC, broadcast, "Wake" button, "Delete" button
- "Save Current" button to save current input as a new device (opens name input)

Result display:
- Success: green banner with message
- Error: red banner with message

Call `sendWol()` on submit.

- [ ] **Step 2: Commit**

```bash
git add src/components/network/WakeOnLanTab.vue
git commit -m "feat: 添加 Wake-on-LAN Tab 组件"
```

---

### Task 11: SubnetCalcTab.vue

**Files:**
- Create: `src/components/network/SubnetCalcTab.vue`

- [ ] **Step 1: Create subnet calculator component (pure frontend)**

Input area:
- IP address input
- CIDR prefix input with `/` prefix label (0-32)
- Calculate button
- Quick CIDR buttons: /8, /16, /24, /25, /26, /27, /28, /30, /32

Calculation logic (all in TypeScript, no Rust needed):
- Parse IP to 32-bit number
- Apply CIDR mask
- Calculate: network address, broadcast address, subnet mask, wildcard mask, first/last usable IP, host count
- Binary representation with network/host bits color-coded

Results: 6-card grid layout matching the mockup design.

- [ ] **Step 2: Commit**

```bash
git add src/components/network/SubnetCalcTab.vue
git commit -m "feat: 添加子网计算器 Tab 组件"
```

---

### Task 12: Integration Test + Build Verification

**Files:** None (verification only)

- [ ] **Step 1: Run full build**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: Build succeeds, produces executable

- [ ] **Step 2: Manual verification checklist**

- Navigate to "Other Tools > Network Tools" in sidebar
- All 5 tabs render correctly
- Ping scan: enter prefix, scan, see grid populate in real-time, stop works
- TCP connections: stats load, auto-refresh works
- Port test: presets work, custom presets persist, results show
- WOL: send packet, save device, wake from saved list
- Subnet calculator: calculate works, CIDR quick buttons work

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "feat: 网络工具模块完成 - Ping扫描/TCP统计/端口测试/WOL/子网计算"
```
