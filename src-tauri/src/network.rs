use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

type ProbeFuture = Pin<Box<dyn Future<Output = Option<f64>> + Send>>;

async fn first_successful_probe(probes: Vec<ProbeFuture>) -> Option<f64> {
    if probes.is_empty() {
        return None;
    }

    let mut tasks = JoinSet::new();
    for probe in probes {
        tasks.spawn(probe);
    }

    while let Some(joined) = tasks.join_next().await {
        if let Ok(Some(latency_ms)) = joined {
            tasks.abort_all();
            return Some(latency_ms);
        }
    }

    None
}

async fn probe_tcp_port(ip: String, port: u16, timeout: std::time::Duration) -> Option<f64> {
    let addr = format!("{}:{}", ip, port);
    let start = std::time::Instant::now();

    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => Some(start.elapsed().as_secs_f64() * 1000.0),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            Some(start.elapsed().as_secs_f64() * 1000.0)
        }
        _ => None,
    }
}

async fn tcp_ping_ports(ip: &str, ports: &[u16], timeout_ms: u64) -> (bool, Option<f64>) {
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let probes = ports
        .iter()
        .copied()
        .map(|port| {
            let ip = ip.to_string();
            Box::pin(async move { probe_tcp_port(ip, port, timeout).await }) as ProbeFuture
        })
        .collect();

    match first_successful_probe(probes).await {
        Some(latency_ms) => (true, Some(latency_ms)),
        None => (false, None),
    }
}

// ─── Ping Scan ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub ip: String,
    pub alive: bool,
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingScanRequest {
    pub prefix: String,
    pub start: u8,
    pub end: u8,
    pub timeout_ms: u64,
}

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

/// Try a TCP connect to common service ports. If we get a connection OR a
/// "connection refused" the host is alive (there is a TCP stack answering).
async fn tcp_ping(ip: &str, timeout_ms: u64) -> (bool, Option<f64>) {
    let ports: &[u16] = &[80, 443, 22, 135, 445];
    tcp_ping_ports(ip, ports, timeout_ms).await
}

fn validate_prefix(prefix: &str) -> Result<(), String> {
    let octets: Vec<&str> = prefix.split('.').collect();
    if octets.len() != 3 {
        return Err(format!(
            "Invalid prefix '{}': expected 3 octets (e.g. 192.168.1)",
            prefix
        ));
    }
    for (i, octet) in octets.iter().enumerate() {
        match octet.parse::<u16>() {
            Ok(v) if v <= 255 => {}
            _ => {
                return Err(format!(
                    "Invalid prefix '{}': octet {} ('{}') is not 0-255",
                    prefix,
                    i + 1,
                    octet
                ));
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn ping_scan(
    app_handle: tauri::AppHandle,
    state: State<'_, NetworkState>,
    request: PingScanRequest,
) -> Result<(), String> {
    validate_prefix(&request.prefix)?;

    if request.start > request.end {
        return Err(format!(
            "Invalid range: start ({}) > end ({})",
            request.start, request.end
        ));
    }

    let timeout_ms = if request.timeout_ms == 0 {
        1500
    } else {
        request.timeout_ms
    };

    // Reset cancel flag
    state.ping_cancel.store(false, Ordering::SeqCst);
    let cancel = state.ping_cancel.clone();

    let semaphore = Arc::new(Semaphore::new(50));
    let mut handles = Vec::new();

    for i in request.start..=request.end {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        let ip = format!("{}.{}", request.prefix, i);
        let app = app_handle.clone();
        let cancel = cancel.clone();
        let sem = semaphore.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await;

            if cancel.load(Ordering::SeqCst) {
                return;
            }

            let (alive, latency_ms) = tcp_ping(&ip, timeout_ms).await;

            if cancel.load(Ordering::SeqCst) {
                return;
            }

            let result = PingResult {
                ip,
                alive,
                latency_ms,
            };
            let _ = app.emit("ping-result", &result);
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
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

// ─── TCP Connections (netstat) ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatCount {
    pub state: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpCount {
    pub ip: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortCount {
    pub port: u16,
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpConnectionStats {
    pub total: usize,
    pub by_state: Vec<StatCount>,
    pub by_remote_ip: Vec<IpCount>,
    pub by_remote_port: Vec<PortCount>,
}

fn well_known_port_name(port: u16) -> &'static str {
    match port {
        20 => "FTP-Data",
        21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 => "SMTP",
        53 => "DNS",
        80 => "HTTP",
        110 => "POP3",
        119 => "NNTP",
        135 => "RPC",
        139 => "NetBIOS",
        143 => "IMAP",
        161 => "SNMP",
        389 => "LDAP",
        443 => "HTTPS",
        445 => "SMB",
        465 => "SMTPS",
        587 => "SMTP-Sub",
        636 => "LDAPS",
        993 => "IMAPS",
        995 => "POP3S",
        1433 => "MSSQL",
        1521 => "Oracle",
        3306 => "MySQL",
        3389 => "RDP",
        5432 => "PostgreSQL",
        5900 => "VNC",
        6379 => "Redis",
        8080 => "HTTP-Alt",
        8443 => "HTTPS-Alt",
        27017 => "MongoDB",
        _ => "",
    }
}

fn top_port_counts(mut ports: Vec<PortCount>, limit: usize) -> Vec<PortCount> {
    ports.sort_by_key(|port| std::cmp::Reverse(port.count));
    ports.truncate(limit);
    ports
}

#[tauri::command]
pub async fn get_tcp_connections() -> Result<TcpConnectionStats, String> {
    let output = tokio::process::Command::new("netstat")
        .args(["-n", "-p", "TCP"])
        .output()
        .await
        .map_err(|e| format!("Failed to run netstat: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("netstat failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut state_map: HashMap<String, usize> = HashMap::new();
    let mut ip_map: HashMap<String, usize> = HashMap::new();
    let mut port_map: HashMap<u16, usize> = HashMap::new();
    let mut total: usize = 0;

    for line in stdout.lines() {
        let line = line.trim();
        // Typical netstat -n -p TCP line:
        //   TCP    192.168.1.5:52341    93.184.216.34:443    ESTABLISHED
        if !line.starts_with("TCP") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        // parts: [TCP, local_addr, remote_addr, state]
        if parts.len() < 4 {
            continue;
        }

        let remote_addr = parts[2];
        let state = parts[3];

        total += 1;
        *state_map.entry(state.to_string()).or_insert(0) += 1;

        // Parse remote address — handle IPv4 "ip:port"
        if let Some(colon_pos) = remote_addr.rfind(':') {
            let ip_part = &remote_addr[..colon_pos];
            let port_part = &remote_addr[colon_pos + 1..];

            if !ip_part.is_empty() {
                *ip_map.entry(ip_part.to_string()).or_insert(0) += 1;
            }
            if let Ok(port) = port_part.parse::<u16>() {
                *port_map.entry(port).or_insert(0) += 1;
            }
        }
    }

    // Sort by_state descending
    let mut by_state: Vec<StatCount> = state_map
        .into_iter()
        .map(|(state, count)| StatCount { state, count })
        .collect();
    by_state.sort_by_key(|state| std::cmp::Reverse(state.count));

    // Sort by_remote_ip descending, top 20
    let mut by_remote_ip: Vec<IpCount> = ip_map
        .into_iter()
        .map(|(ip, count)| IpCount { ip, count })
        .collect();
    by_remote_ip.sort_by_key(|entry| std::cmp::Reverse(entry.count));
    by_remote_ip.truncate(20);

    // Sort by_remote_port descending, top 20
    let mut by_remote_port: Vec<PortCount> = port_map
        .into_iter()
        .map(|(port, count)| PortCount {
            port,
            name: well_known_port_name(port).to_string(),
            count,
        })
        .collect();
    by_remote_port = top_port_counts(by_remote_port, 20);

    Ok(TcpConnectionStats {
        total,
        by_state,
        by_remote_ip,
        by_remote_port,
    })
}

// ─── Port Test ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortTestRequest {
    pub host: String,
    pub ports: Vec<u16>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortTestResult {
    pub host: String,
    pub resolved_ip: Option<String>,
    pub results: Vec<SinglePortResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SinglePortResult {
    pub port: u16,
    pub open: bool,
    pub latency_ms: Option<f64>,
    pub name: String,
}

#[tauri::command]
pub async fn test_ports(request: PortTestRequest) -> Result<PortTestResult, String> {
    if request.ports.is_empty() {
        return Err("No ports specified".to_string());
    }
    if request.ports.len() > 1000 {
        return Err("Too many ports (max 1000)".to_string());
    }

    let host = request.host.trim().to_string();
    if host.is_empty() {
        return Err("Host is required".to_string());
    }

    let timeout_ms = if request.timeout_ms == 0 {
        2000
    } else {
        request.timeout_ms
    };

    // DNS resolution
    let resolved_ip = match tokio::net::lookup_host(format!("{}:0", host)).await {
        Ok(mut addrs) => addrs.next().map(|a| a.ip().to_string()),
        Err(_) => None,
    };

    let semaphore = Arc::new(Semaphore::new(50));
    let mut handles = Vec::new();

    for &port in &request.ports {
        let host = host.clone();
        let sem = semaphore.clone();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await;
            let addr = format!("{}:{}", host, port);
            let start = std::time::Instant::now();

            let open = matches!(
                tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr)).await,
                Ok(Ok(_))
            );

            let latency_ms = if open {
                Some(start.elapsed().as_secs_f64() * 1000.0)
            } else {
                None
            };

            SinglePortResult {
                port,
                open,
                latency_ms,
                name: well_known_port_name(port).to_string(),
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

    // Sort by port number
    results.sort_by_key(|r| r.port);

    Ok(PortTestResult {
        host,
        resolved_ip,
        results,
    })
}

// ─── Wake-on-LAN ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WolRequest {
    pub mac: String,
    pub broadcast_ip: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WolResult {
    pub mac: String,
    pub success: bool,
    pub message: String,
}

fn parse_mac(mac: &str) -> Result<[u8; 6], String> {
    let cleaned = mac.trim();
    let parts: Vec<&str> = if cleaned.contains(':') {
        cleaned.split(':').collect()
    } else if cleaned.contains('-') {
        cleaned.split('-').collect()
    } else if cleaned.len() == 12 {
        // No separators, split every 2 chars
        (0..6).map(|i| &cleaned[i * 2..i * 2 + 2]).collect()
    } else {
        return Err(format!("Invalid MAC format: '{}'", mac));
    };

    if parts.len() != 6 {
        return Err(format!(
            "Invalid MAC address '{}': expected 6 octets, got {}",
            mac,
            parts.len()
        ));
    }

    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16)
            .map_err(|_| format!("Invalid MAC octet '{}' in '{}'", part, mac))?;
    }

    Ok(bytes)
}

#[tauri::command]
pub async fn send_wol(request: WolRequest) -> Result<WolResult, String> {
    let mac_bytes = match parse_mac(&request.mac) {
        Ok(b) => b,
        Err(e) => {
            return Ok(WolResult {
                mac: request.mac,
                success: false,
                message: e,
            });
        }
    };

    // Build magic packet: 6 bytes of 0xFF followed by 16 repetitions of MAC
    let mut packet = Vec::with_capacity(6 + 6 * 16);
    packet.extend_from_slice(&[0xFF; 6]);
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }

    let broadcast_ip = request.broadcast_ip.as_deref().unwrap_or("255.255.255.255");
    let port = request.port.unwrap_or(9);
    let dest = format!("{}:{}", broadcast_ip, port);

    let socket = std::net::UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind UDP socket: {}", e))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("Failed to enable broadcast: {}", e))?;

    match socket.send_to(&packet, &dest) {
        Ok(_) => Ok(WolResult {
            mac: request.mac,
            success: true,
            message: format!("Magic packet sent to {}", dest),
        }),
        Err(e) => Ok(WolResult {
            mac: request.mac,
            success: false,
            message: format!("Failed to send magic packet: {}", e),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{first_successful_probe, top_port_counts, PortCount};
    use std::pin::Pin;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn first_successful_probe_returns_without_waiting_for_slow_failures() {
        let started = Instant::now();
        let latency = first_successful_probe(vec![
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(120)).await;
                None
            }) as Pin<Box<dyn std::future::Future<Output = Option<f64>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Some(7.5)
            }) as Pin<Box<dyn std::future::Future<Output = Option<f64>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(120)).await;
                None
            }) as Pin<Box<dyn std::future::Future<Output = Option<f64>> + Send>>,
        ])
        .await;

        assert_eq!(latency, Some(7.5));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn top_port_counts_sorts_before_truncating() {
        let ports = vec![
            PortCount {
                port: 1000,
                name: String::new(),
                count: 1,
            },
            PortCount {
                port: 1001,
                name: String::new(),
                count: 8,
            },
            PortCount {
                port: 1002,
                name: String::new(),
                count: 3,
            },
            PortCount {
                port: 1003,
                name: String::new(),
                count: 9,
            },
        ];

        let top = top_port_counts(ports, 2);
        let ports_only: Vec<u16> = top.into_iter().map(|entry| entry.port).collect();

        assert_eq!(ports_only, vec![1003, 1001]);
    }
}
