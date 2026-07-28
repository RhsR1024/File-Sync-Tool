use crate::network_probe::{
    arp_cache_neighbors, arp_resolve, format_mac, icmp_echo, is_on_link, local_prefixes,
    IcmpOutcome, LocalPrefix,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, State};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

type ProbeFuture = Pin<Box<dyn Future<Output = Option<ProbeHit>> + Send>>;

/// Addresses probed at once. Far above the old limit because probes no longer
/// spawn a process each.
const HOST_CONCURRENCY: usize = 128;
/// Ceiling on IP Helper calls in flight. Each one occupies a blocking thread
/// for its full duration, and `SendARP` ignores our timeout entirely.
const BLOCKING_PROBE_LIMIT: usize = 128;
/// Attempts per address during the rescan pass. A single echo is lost often
/// enough on a busy link to be the main source of missed hosts.
const RESCAN_ATTEMPTS: usize = 2;
const TCP_PROBE_PORTS: &[u16] = &[80, 443, 22, 53, 135, 139, 445, 3389];

async fn first_successful_probe<T>(probes: Vec<Pin<Box<dyn Future<Output = Option<T>> + Send>>>) -> Option<T>
where
    T: Send + 'static,
{
    if probes.is_empty() {
        return None;
    }

    let mut tasks = JoinSet::new();
    for probe in probes {
        tasks.spawn(probe);
    }

    while let Some(joined) = tasks.join_next().await {
        if let Ok(Some(hit)) = joined {
            tasks.abort_all();
            return Some(hit);
        }
    }

    None
}

fn round_ms(elapsed: Duration) -> f64 {
    (elapsed.as_secs_f64() * 10_000.0).round() / 10.0
}

/// How an address was found to be occupied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeMethod {
    Arp,
    Icmp,
    Tcp,
    ArpCache,
}

#[derive(Debug, Clone, Copy)]
struct ProbeHit {
    method: ProbeMethod,
    latency_ms: Option<f64>,
    mac: Option<[u8; 6]>,
}

struct ProbeContext {
    prefixes: Vec<LocalPrefix>,
    blocking: Arc<Semaphore>,
}

/// Runs a blocking IP Helper call under a permit. Taking the permit before the
/// thread starts means a probe that loses its race while still queued costs
/// nothing; once running it cannot be cancelled, so the permit is what actually
/// bounds thread usage.
async fn run_blocking_probe<T, F>(limiter: Arc<Semaphore>, probe: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let permit = limiter.acquire_owned().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        probe()
    })
    .await
    .ok()
}

fn arp_probe(ip: Ipv4Addr, limiter: Arc<Semaphore>) -> ProbeFuture {
    Box::pin(async move {
        let (mac, elapsed) = run_blocking_probe(limiter, move || arp_resolve(ip)).await?;
        Some(ProbeHit {
            method: ProbeMethod::Arp,
            latency_ms: Some(round_ms(elapsed)),
            mac: Some(mac?),
        })
    })
}

fn icmp_probe(ip: Ipv4Addr, timeout_ms: u64, limiter: Arc<Semaphore>) -> ProbeFuture {
    Box::pin(async move {
        let timeout_ms = timeout_ms.clamp(1, u32::MAX as u64) as u32;
        let (outcome, elapsed) =
            run_blocking_probe(limiter, move || icmp_echo(ip, timeout_ms)).await?;

        match outcome {
            IcmpOutcome::Reply { round_trip_ms } => Some(ProbeHit {
                method: ProbeMethod::Icmp,
                // The API reports whole milliseconds and reads 0 on a fast LAN,
                // so fall back to the measured call duration.
                latency_ms: Some(if round_trip_ms > 0 {
                    round_trip_ms as f64
                } else {
                    round_ms(elapsed)
                }),
                mac: None,
            }),
            IcmpOutcome::Unreachable | IcmpOutcome::NoReply => None,
        }
    })
}

fn tcp_probe(ip: Ipv4Addr, port: u16, timeout: Duration) -> ProbeFuture {
    Box::pin(async move {
        let addr = SocketAddr::from((ip, port));
        let started = Instant::now();

        let reachable = match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr))
            .await
        {
            Ok(Ok(_stream)) => true,
            // A refusal still proves a TCP stack is answering at that address.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => true,
            _ => false,
        };

        reachable.then(|| ProbeHit {
            method: ProbeMethod::Tcp,
            latency_ms: Some(round_ms(started.elapsed())),
            mac: None,
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbePass {
    /// Sweeps every address once.
    Initial,
    /// Revisits addresses that looked dead, with a longer timeout and retries.
    Rescan,
}

/// Probes one address.
async fn probe_host(
    ip: Ipv4Addr,
    timeout_ms: u64,
    ctx: &ProbeContext,
    pass: ProbePass,
) -> Option<ProbeHit> {
    let on_link = is_on_link(ip, &ctx.prefixes);

    if pass == ProbePass::Initial && on_link {
        // ARP is definitive on the local link: the stack cannot deliver ICMP or
        // TCP to a neighbour it cannot resolve, so a failed ARP means nothing
        // else would have answered either. Probing ARP alone keeps the first
        // pass cheap on a subnet that is mostly empty.
        let hit = arp_probe(ip, ctx.blocking.clone()).await?;
        // The host is up, so borrow a real round-trip figure from ICMP.
        let latency = icmp_probe(ip, timeout_ms, ctx.blocking.clone()).await;
        return Some(ProbeHit {
            latency_ms: latency.and_then(|probe| probe.latency_ms).or(hit.latency_ms),
            ..hit
        });
    }

    let timeout = Duration::from_millis(timeout_ms);
    let mut probes: Vec<ProbeFuture> = Vec::with_capacity(TCP_PROBE_PORTS.len() + 2);
    if on_link {
        // Worth asking again even though the first pass already did. `SendARP`
        // normally retries internally for seconds, but when the neighbour cache
        // holds a negative entry for the address it fails immediately instead —
        // and the cache sweep discards that same entry as unreachable. Without
        // a second attempt, a host that merely lost one ARP exchange is
        // reported free while it answers a manual ping fine.
        probes.push(arp_probe(ip, ctx.blocking.clone()));
    }
    probes.push(icmp_probe(ip, timeout_ms, ctx.blocking.clone()));
    probes.extend(
        TCP_PROBE_PORTS
            .iter()
            .map(|port| tcp_probe(ip, *port, timeout)),
    );

    first_successful_probe(probes).await
}

// ─── Ping Scan ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub ip: String,
    pub alive: bool,
    pub latency_ms: Option<f64>,
    pub mac: Option<String>,
    pub method: Option<ProbeMethod>,
}

impl PingResult {
    fn new(ip: Ipv4Addr, hit: Option<&ProbeHit>) -> Self {
        match hit {
            Some(hit) => Self {
                ip: ip.to_string(),
                alive: true,
                latency_ms: hit.latency_ms,
                mac: hit.mac.as_ref().map(format_mac),
                method: Some(hit.method),
            },
            None => Self {
                ip: ip.to_string(),
                alive: false,
                latency_ms: None,
                mac: None,
                method: None,
            },
        }
    }
}

/// Which pass the scan is currently in, so the UI can explain the work that
/// happens after every address has a first answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingScanPhase {
    pub phase: &'static str,
    pub remaining: usize,
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
    pub port_cancel: Arc<AtomicBool>,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            ping_cancel: Arc::new(AtomicBool::new(false)),
            port_cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn parse_prefix(prefix: &str) -> Result<[u8; 3], String> {
    let octets: Vec<&str> = prefix.split('.').collect();
    if octets.len() != 3 {
        return Err(format!(
            "Invalid prefix '{}': expected 3 octets (e.g. 192.168.1)",
            prefix
        ));
    }

    let mut parsed = [0u8; 3];
    for (i, octet) in octets.iter().enumerate() {
        match octet.parse::<u16>() {
            Ok(v) if v <= 255 => parsed[i] = v as u8,
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
    Ok(parsed)
}

fn emit_phase(app_handle: &tauri::AppHandle, phase: &'static str, remaining: usize) {
    let _ = app_handle.emit("ping-scan-phase", PingScanPhase { phase, remaining });
}

/// Probes every target once. `deep` also enables retries, for the pass that
/// only revisits addresses which looked dead.
async fn run_probe_round(
    app_handle: &tauri::AppHandle,
    cancel: &Arc<AtomicBool>,
    ctx: &Arc<ProbeContext>,
    targets: &[Ipv4Addr],
    timeout_ms: u64,
    pass: ProbePass,
    emit_misses: bool,
) -> Vec<(Ipv4Addr, Option<ProbeHit>)> {
    let host_limit = Arc::new(Semaphore::new(HOST_CONCURRENCY));
    let mut tasks = JoinSet::new();

    for ip in targets.iter().copied() {
        let host_limit = host_limit.clone();
        let cancel = cancel.clone();
        let ctx = ctx.clone();

        tasks.spawn(async move {
            let _permit = host_limit.acquire_owned().await;
            if cancel.load(Ordering::SeqCst) {
                return (ip, None);
            }

            if pass == ProbePass::Initial {
                return (ip, probe_host(ip, timeout_ms, &ctx, pass).await);
            }

            let mut hit = None;
            for _ in 0..RESCAN_ATTEMPTS {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                hit = probe_host(ip, timeout_ms, &ctx, pass).await;
                if hit.is_some() {
                    break;
                }
            }
            (ip, hit)
        });
    }

    let mut outcomes = Vec::with_capacity(targets.len());
    while let Some(joined) = tasks.join_next().await {
        let Ok((ip, hit)) = joined else { continue };

        // A cancelled task reports no hit because it never ran, not because the
        // address is free — never publish that as an offline result.
        if !cancel.load(Ordering::SeqCst) && (emit_misses || hit.is_some()) {
            let _ = app_handle.emit("ping-result", &PingResult::new(ip, hit.as_ref()));
        }
        outcomes.push((ip, hit));
    }

    outcomes
}

#[tauri::command]
pub async fn ping_scan(
    app_handle: tauri::AppHandle,
    state: State<'_, NetworkState>,
    request: PingScanRequest,
) -> Result<(), String> {
    let prefix = parse_prefix(&request.prefix)?;

    if request.start > request.end {
        return Err(format!(
            "Invalid range: start ({}) > end ({})",
            request.start, request.end
        ));
    }

    let timeout_ms = if request.timeout_ms == 0 {
        1000
    } else {
        request.timeout_ms.clamp(100, 30_000)
    };

    // Reset cancel flag
    state.ping_cancel.store(false, Ordering::SeqCst);
    let cancel = state.ping_cancel.clone();

    let targets: Vec<Ipv4Addr> = (request.start..=request.end)
        .map(|host| Ipv4Addr::new(prefix[0], prefix[1], prefix[2], host))
        .collect();

    let ctx = Arc::new(ProbeContext {
        prefixes: tokio::task::spawn_blocking(local_prefixes)
            .await
            .unwrap_or_default(),
        blocking: Arc::new(Semaphore::new(BLOCKING_PROBE_LIMIT)),
    });

    let mut hits: HashMap<Ipv4Addr, ProbeHit> = HashMap::new();
    let mut missed: Vec<Ipv4Addr> = Vec::new();

    emit_phase(&app_handle, "scanning", targets.len());
    for (ip, hit) in run_probe_round(
        &app_handle,
        &cancel,
        &ctx,
        &targets,
        timeout_ms,
        ProbePass::Initial,
        true,
    )
    .await
    {
        match hit {
            Some(hit) => {
                hits.insert(ip, hit);
            }
            None => missed.push(ip),
        }
    }

    // Second pass over everything that looked dead, with a longer timeout and
    // retries. A single lost echo is the usual reason a live host is missed,
    // and running the full probe set here also covers hosts that filter ARP.
    if !missed.is_empty() && !cancel.load(Ordering::SeqCst) {
        emit_phase(&app_handle, "rescanning", missed.len());
        let rescan_timeout = timeout_ms.saturating_mul(2).clamp(1_000, 5_000);
        for (ip, hit) in run_probe_round(
            &app_handle,
            &cancel,
            &ctx,
            &missed,
            rescan_timeout,
            ProbePass::Rescan,
            false,
        )
        .await
        {
            if let Some(hit) = hit {
                hits.insert(ip, hit);
            }
        }
    }

    // Finally consult the neighbour table. Anything that answered ARP at any
    // point during the scan is in there, including hosts whose reply arrived
    // after their own probe had already given up.
    if !cancel.load(Ordering::SeqCst) {
        emit_phase(&app_handle, "arpSweep", 0);
        let in_range: HashSet<Ipv4Addr> = targets.iter().copied().collect();
        let neighbors = tokio::task::spawn_blocking(arp_cache_neighbors)
            .await
            .unwrap_or_default();

        for (ip, mac) in neighbors {
            if !in_range.contains(&ip) {
                continue;
            }
            match hits.get_mut(&ip) {
                // Already known to be up; just fill in the hardware address.
                Some(hit) if hit.mac.is_none() => hit.mac = Some(mac),
                Some(_) => continue,
                None => {
                    hits.insert(
                        ip,
                        ProbeHit {
                            method: ProbeMethod::ArpCache,
                            latency_ms: None,
                            mac: Some(mac),
                        },
                    );
                }
            }
            let _ = app_handle.emit("ping-result", &PingResult::new(ip, hits.get(&ip)));
        }
    }

    let _ = app_handle.emit("ping-scan-complete", ());
    Ok(())
}

#[tauri::command]
pub fn cancel_ping_scan(state: State<'_, NetworkState>) {
    state.ping_cancel.store(true, Ordering::SeqCst);
}

// ─── Manual ping console ─────────────────────────────────────────────────────

/// Echo count for the console shown on demand. Enough packets to expose a host
/// that answers intermittently, which is the case a one-shot scan gets wrong.
const PING_CONSOLE_COUNT: &str = "10";

/// Rejects anything that is not a bare IPv4 address. Re-rendering the parsed
/// address guarantees the string handed to the shell holds digits and dots
/// only, so it cannot carry command syntax.
fn validated_ping_target(ip: &str) -> Result<String, String> {
    ip.trim()
        .parse::<Ipv4Addr>()
        .map(|parsed| parsed.to_string())
        .map_err(|_| format!("Invalid IPv4 address: '{}'", ip))
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn open_ping_console(ip: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    /// Gives the child its own visible console instead of borrowing ours.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let target = validated_ping_target(&ip)?;

    // `/k` keeps the window open once ping finishes so the output stays
    // readable and the address can be re-tested by hand.
    std::process::Command::new("cmd")
        .args(["/k", "ping", "-n", PING_CONSOLE_COUNT, target.as_str()])
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map_err(|e| format!("Failed to open ping console for {}: {}", target, e))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn open_ping_console(ip: String) -> Result<(), String> {
    let target = validated_ping_target(&ip)?;
    Err(format!(
        "Opening a ping console for {} is only supported on Windows",
        target
    ))
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

const PORT_TEST_CONCURRENCY: usize = 200;

fn normalize_requested_ports(mut ports: Vec<u16>) -> Result<Vec<u16>, String> {
    if ports.is_empty() {
        return Err("No ports specified".to_string());
    }

    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

async fn probe_port(host: String, port: u16, timeout: std::time::Duration) -> SinglePortResult {
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
}

#[tauri::command]
pub async fn test_ports(
    app_handle: tauri::AppHandle,
    state: State<'_, NetworkState>,
    request: PortTestRequest,
) -> Result<PortTestResult, String> {
    let ports = normalize_requested_ports(request.ports)?;
    let host = request.host.trim().to_string();
    if host.is_empty() {
        return Err("Host is required".to_string());
    }

    let timeout_ms = if request.timeout_ms == 0 {
        500
    } else {
        request.timeout_ms.clamp(100, 30_000)
    };
    let timeout = std::time::Duration::from_millis(timeout_ms);

    // DNS resolution
    let resolved_ip = match tokio::net::lookup_host(format!("{}:0", host)).await {
        Ok(mut addrs) => addrs.next().map(|a| a.ip().to_string()),
        Err(_) => None,
    };

    state.port_cancel.store(false, Ordering::SeqCst);
    let cancel = state.port_cancel.clone();

    let mut results = Vec::new();
    let mut tasks = JoinSet::new();
    let mut next_index = 0usize;
    let mut active = 0usize;

    while next_index < ports.len() || active > 0 {
        while active < PORT_TEST_CONCURRENCY && next_index < ports.len() {
            if cancel.load(Ordering::SeqCst) {
                break;
            }

            let host = host.clone();
            let port = ports[next_index];
            tasks.spawn(probe_port(host, port, timeout));
            next_index += 1;
            active += 1;
        }

        if active == 0 {
            break;
        }

        if let Some(joined) = tasks.join_next().await {
            active = active.saturating_sub(1);
            if cancel.load(Ordering::SeqCst) {
                tasks.abort_all();
                break;
            }
            if let Ok(result) = joined {
                let _ = app_handle.emit("port-test-result", &result);
                results.push(result);
            }
        }
    }

    results.sort_by_key(|r| r.port);
    let _ = app_handle.emit("port-test-complete", ());

    Ok(PortTestResult {
        host,
        resolved_ip,
        results,
    })
}

#[tauri::command]
pub fn cancel_port_test(state: State<'_, NetworkState>) {
    state.port_cancel.store(true, Ordering::SeqCst);
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
    use super::{
        first_successful_probe, normalize_requested_ports, parse_prefix, top_port_counts,
        validated_ping_target, PortCount, PingResult, ProbeHit, ProbeMethod,
    };
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
    fn parse_prefix_rejects_malformed_input_and_keeps_octets() {
        assert_eq!(parse_prefix("192.168.1"), Ok([192, 168, 1]));
        // Leading zeros are accepted here but would fail a string-based
        // Ipv4Addr parse, which is why the scan builds addresses from octets.
        assert_eq!(parse_prefix("192.168.001"), Ok([192, 168, 1]));
        assert!(parse_prefix("192.168.1.5").is_err());
        assert!(parse_prefix("192.168").is_err());
        assert!(parse_prefix("192.168.256").is_err());
    }

    #[test]
    fn ping_console_target_accepts_only_bare_ipv4() {
        assert_eq!(
            validated_ping_target(" 192.168.0.108 "),
            Ok("192.168.0.108".to_string())
        );

        // Nothing that could reach the shell as syntax may survive validation.
        for hostile in [
            "192.168.0.1 && calc",
            "192.168.0.1 | calc",
            "192.168.0.1&calc",
            "192.168.0.1\"&calc",
            "192.168.0.1; calc",
            "$(calc)",
            "%COMSPEC%",
            "localhost",
            "::1",
            "",
        ] {
            assert!(
                validated_ping_target(hostile).is_err(),
                "should have rejected {:?}",
                hostile
            );
        }
    }

    #[test]
    fn ping_result_reports_method_and_mac_for_a_hit() {
        let hit = ProbeHit {
            method: ProbeMethod::Arp,
            latency_ms: Some(1.5),
            mac: Some([0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]),
        };

        let result = PingResult::new("192.168.1.7".parse().unwrap(), Some(&hit));

        assert!(result.alive);
        assert_eq!(result.latency_ms, Some(1.5));
        assert_eq!(result.mac.as_deref(), Some("00:1A:2B:3C:4D:5E"));
        assert_eq!(result.method, Some(ProbeMethod::Arp));
    }

    #[test]
    fn ping_result_reports_a_miss_as_offline() {
        let result = PingResult::new("192.168.1.7".parse().unwrap(), None);

        assert!(!result.alive);
        assert_eq!(result.latency_ms, None);
        assert_eq!(result.mac, None);
        assert_eq!(result.method, None);
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

    #[test]
    fn normalize_requested_ports_accepts_full_tcp_range() {
        let ports: Vec<u16> = (1..=u16::MAX).collect();

        let normalized = normalize_requested_ports(ports).expect("full TCP range should be valid");

        assert_eq!(normalized.len(), 65_535);
        assert_eq!(normalized.first(), Some(&1));
        assert_eq!(normalized.last(), Some(&65_535));
    }

    #[test]
    fn normalize_requested_ports_sorts_and_deduplicates_without_a_thousand_port_limit() {
        let ports = (1..=1001).chain([22, 80, 80]).collect();

        let normalized = normalize_requested_ports(ports).expect("1001+ ports should be valid");

        assert_eq!(normalized.len(), 1001);
        assert_eq!(normalized[0], 1);
        assert_eq!(normalized[21], 22);
        assert_eq!(normalized[79], 80);
        assert_eq!(normalized.last(), Some(&1001));
    }
}
