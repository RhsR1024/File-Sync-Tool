use crate::config::DeployServer;
use serde_json::{json, Value};
use ssh2::Session;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const DEFAULT_USER: &str = "root";
const PRIMARY_PORT: u16 = 23_333;
const PRIMARY_PASSWORD: &str = "admin_123";
const FALLBACK_PORT: u16 = 22;
const FALLBACK_PASSWORD: &str = "123456";
const SSH_API_PORTS: [u16; 2] = [23_006, 9_007];

#[derive(Clone)]
pub struct SshCandidate {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshResolutionSummary {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub ssh_enable_attempted: bool,
    pub ssh_enable_succeeded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttemptFailure {
    label: &'static str,
    message: String,
}

fn candidate(host: &str, port: u16, password: &str) -> SshCandidate {
    SshCandidate {
        host: host.to_string(),
        port,
        username: DEFAULT_USER.to_string(),
        password: password.to_string(),
    }
}

fn connect(candidate: &SshCandidate, timeout: Duration) -> Result<Session, String> {
    let address = format!("{}:{}", candidate.host, candidate.port)
        .to_socket_addrs()
        .map_err(|error| format!("地址解析失败: {error}"))?
        .next()
        .ok_or_else(|| "地址解析未返回可用地址".to_string())?;
    let tcp = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| format!("TCP 连接失败: {error}"))?;
    let mut session = Session::new().map_err(|error| format!("SSH 会话初始化失败: {error}"))?;
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|error| format!("SSH 握手失败: {error}"))?;
    session
        .userauth_password(&candidate.username, &candidate.password)
        .map_err(|error| format!("SSH 用户名或密码认证失败: {error}"))?;
    if !session.authenticated() {
        return Err("SSH 用户名或密码认证失败".to_string());
    }
    Ok(session)
}

fn enable_ssh_at_port(
    client: &reqwest::blocking::Client,
    host: &str,
    port: u16,
) -> Result<(), String> {
    let url = format!("http://{host}:{port}/openAPI/system/v1/network/SSH/set");
    let response = client
        .post(url)
        .json(&json!({ "enable": 1 }))
        .send()
        .map_err(|error| format!("接口请求失败: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("接口响应读取失败: {error}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    if let Ok(value) = serde_json::from_str::<Value>(&body) {
        if value
            .get("code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
        {
            return Err(value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("接口返回失败")
                .to_string());
        }
    }
    Ok(())
}

pub(crate) fn enable_appliance_ssh(host: &str, timeout: Duration) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
        .map_err(|error| format!("HTTP 客户端初始化失败: {error}"))?;
    let mut failures = Vec::new();
    for port in SSH_API_PORTS {
        match enable_ssh_at_port(&client, host, port) {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(format!("{port}: {error}")),
        }
    }
    Err(failures.join("；"))
}

fn apply_all_tcp_whitelist(session: &Session) -> Result<(), String> {
    let command = "iptables -C INPUT -p tcp -s 0.0.0.0/0 -j ACCEPT || iptables -I INPUT 1 -p tcp -s 0.0.0.0/0 -j ACCEPT";
    let mut channel = session
        .channel_session()
        .map_err(|error| format!("SSH 白名单通道初始化失败: {error}"))?;
    channel
        .handle_extended_data(ssh2::ExtendedData::Merge)
        .map_err(|error| format!("SSH 白名单输出合并失败: {error}"))?;
    channel
        .exec(command)
        .map_err(|error| format!("SSH 白名单命令执行失败: {error}"))?;
    channel
        .send_eof()
        .map_err(|error| format!("SSH 白名单命令关闭输入失败: {error}"))?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|error| format!("SSH 白名单命令输出读取失败: {error}"))?;
    channel
        .wait_close()
        .map_err(|error| format!("SSH 白名单命令关闭失败: {error}"))?;
    let exit_code = channel.exit_status().unwrap_or(-1);
    if exit_code != 0 {
        return Err(if output.trim().is_empty() {
            format!("SSH 白名单命令退出码为 {exit_code}")
        } else {
            format!("SSH 白名单命令退出码为 {exit_code}: {}", output.trim())
        });
    }
    Ok(())
}

fn resolve_with<T>(
    host: &str,
    mut probe: impl FnMut(&SshCandidate) -> Result<T, String>,
    mut enable: impl FnMut() -> Result<(), String>,
) -> Result<(T, SshResolutionSummary), String> {
    let primary = candidate(host, PRIMARY_PORT, PRIMARY_PASSWORD);
    let fallback = candidate(host, FALLBACK_PORT, FALLBACK_PASSWORD);
    let mut failures = Vec::new();

    match probe(&primary) {
        Ok(value) => {
            return Ok((
                value,
                SshResolutionSummary {
                    host: host.to_string(),
                    port: primary.port,
                    username: primary.username,
                    ssh_enable_attempted: false,
                    ssh_enable_succeeded: false,
                },
            ));
        }
        Err(message) => failures.push(AttemptFailure {
            label: "首次 23333",
            message,
        }),
    }

    let enable_result = enable();
    let enable_succeeded = enable_result.is_ok();
    if let Err(message) = enable_result {
        failures.push(AttemptFailure {
            label: "开启 SSH 接口",
            message,
        });
    }

    if enable_succeeded {
        match probe(&primary) {
            Ok(value) => {
                return Ok((
                    value,
                    SshResolutionSummary {
                        host: host.to_string(),
                        port: primary.port,
                        username: primary.username,
                        ssh_enable_attempted: true,
                        ssh_enable_succeeded: true,
                    },
                ));
            }
            Err(message) => failures.push(AttemptFailure {
                label: "开启后 23333",
                message,
            }),
        }
    }

    match probe(&fallback) {
        Ok(value) => Ok((
            value,
            SshResolutionSummary {
                host: host.to_string(),
                port: fallback.port,
                username: fallback.username,
                ssh_enable_attempted: true,
                ssh_enable_succeeded: enable_succeeded,
            },
        )),
        Err(message) => {
            failures.push(AttemptFailure {
                label: "22 端口降级",
                message,
            });
            let details = failures
                .iter()
                .map(|failure| format!("{}: {}", failure.label, failure.message))
                .collect::<Vec<_>>()
                .join("；");
            Err(format!(
                "无法连接 {host}。{details}。请检查 SSH 网络、防火墙、端口以及 root 用户名和密码。"
            ))
        }
    }
}

pub fn resolve_and_connect(
    server: &DeployServer,
) -> Result<(Session, SshResolutionSummary), String> {
    let timeout = Duration::from_secs(server.ssh_timeout_secs.max(1));
    resolve_with(
        &server.host,
        |candidate| connect(candidate, timeout),
        || enable_appliance_ssh(&server.host, timeout),
    )
}

/// Enables appliance SSH and applies the same permissive default used by the
/// appliance access-control tool: all TCP ports from every IPv4 source.
pub(crate) fn enable_ssh_and_all_tcp(
    server: &DeployServer,
) -> Result<SshResolutionSummary, String> {
    let (session, summary) = resolve_and_connect(server)?;
    apply_all_tcp_whitelist(&session)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_primary_without_calling_enable_when_first_probe_succeeds() {
        let mut enable_calls = 0;
        let (_, summary) = resolve_with(
            "192.0.2.1",
            |candidate| Ok(candidate.port),
            || {
                enable_calls += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(summary.port, PRIMARY_PORT);
        assert_eq!(enable_calls, 0);
    }

    #[test]
    fn retries_primary_after_enable_then_uses_it() {
        let mut probes = 0;
        let (_, summary) = resolve_with(
            "192.0.2.2",
            |_| {
                probes += 1;
                if probes == 1 {
                    Err("closed".to_string())
                } else {
                    Ok(())
                }
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(summary.port, PRIMARY_PORT);
        assert!(summary.ssh_enable_succeeded);
        assert_eq!(probes, 2);
    }

    #[test]
    fn falls_back_to_22_when_enable_fails() {
        let (_, summary) = resolve_with(
            "192.0.2.3",
            |candidate| {
                if candidate.port == FALLBACK_PORT {
                    Ok(())
                } else {
                    Err("closed".to_string())
                }
            },
            || Err("api unavailable".to_string()),
        )
        .unwrap();
        assert_eq!(summary.port, FALLBACK_PORT);
        assert!(!summary.ssh_enable_succeeded);
    }

    #[test]
    fn falls_back_to_22_when_second_primary_probe_fails() {
        let (_, summary) = resolve_with(
            "192.0.2.4",
            |candidate| {
                if candidate.port == FALLBACK_PORT {
                    Ok(())
                } else {
                    Err("auth failed".to_string())
                }
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(summary.port, FALLBACK_PORT);
        assert!(summary.ssh_enable_succeeded);
    }

    #[test]
    fn aggregates_every_failure_without_passwords() {
        let error = resolve_with::<()>(
            "192.0.2.5",
            |_| Err("unreachable".to_string()),
            || Err("api unavailable".to_string()),
        )
        .unwrap_err();
        assert!(error.contains("首次 23333"));
        assert!(error.contains("开启 SSH 接口"));
        assert!(error.contains("22 端口降级"));
        assert!(!error.contains(PRIMARY_PASSWORD));
        assert!(!error.contains(FALLBACK_PASSWORD));
    }
}
