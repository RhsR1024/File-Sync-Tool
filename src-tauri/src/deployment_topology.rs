use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FRAMEWORK_PORT: u16 = 21_900;
const HA_STATUS_PORT: u16 = 9_820;
const SWITCH_PORT: u16 = 80;

#[derive(Debug, Clone, Copy)]
struct TopologyPorts {
    framework: u16,
    ha_status: u16,
    switch: u16,
}

impl Default for TopologyPorts {
    fn default() -> Self {
        Self {
            framework: FRAMEWORK_PORT,
            ha_status: HA_STATUS_PORT,
            switch: SWITCH_PORT,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopologyMode {
    Ha,
    Replica,
    HaAndReplica,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentTopologyRequest {
    pub mode: TopologyMode,
    pub primary_ip: String,
    #[serde(default)]
    pub ha_replica_ip: Option<String>,
    #[serde(default)]
    pub virtual_ip: Option<String>,
    #[serde(default)]
    pub replica_ips: Vec<String>,
    #[serde(default = "default_framework_password")]
    pub framework_password: String,
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_poll_attempts")]
    pub poll_attempts: u32,
}

fn default_framework_password() -> String {
    "admin_123".to_string()
}

fn default_poll_interval_secs() -> u64 {
    30
}

fn default_poll_attempts() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopologyResultStatus {
    Success,
    Failed,
    Unconfirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyServer {
    pub server_ip: String,
    pub server_name: String,
    pub ha_type: i64,
    pub server_status: i64,
    pub is_deployed: i64,
    pub virtual_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentTopologyResult {
    pub status: TopologyResultStatus,
    pub message: String,
    pub expected_server_count: usize,
    pub observed_server_count: usize,
    pub servers: Vec<TopologyServer>,
}

fn compact_hostname(ip: &str) -> String {
    ip.chars().filter(char::is_ascii_digit).collect()
}

fn validate_request(request: &DeploymentTopologyRequest) -> Result<(), String> {
    request
        .primary_ip
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| "主机 IP 无效".to_string())?;
    if matches!(request.mode, TopologyMode::Ha | TopologyMode::HaAndReplica) {
        request
            .ha_replica_ip
            .as_deref()
            .ok_or_else(|| "主备模式必须填写备机 IP".to_string())?
            .parse::<std::net::Ipv4Addr>()
            .map_err(|_| "备机 IP 无效".to_string())?;
        request
            .virtual_ip
            .as_deref()
            .ok_or_else(|| "主备模式必须填写虚拟 IP".to_string())?
            .parse::<std::net::Ipv4Addr>()
            .map_err(|_| "虚拟 IP 无效".to_string())?;
    }
    if matches!(
        request.mode,
        TopologyMode::Replica | TopologyMode::HaAndReplica
    ) && request.replica_ips.is_empty()
    {
        return Err("主从模式必须至少填写一台从机".to_string());
    }
    for ip in &request.replica_ips {
        ip.parse::<std::net::Ipv4Addr>()
            .map_err(|_| format!("从机 IP 无效: {ip}"))?;
    }
    if request.framework_password.is_empty() {
        return Err("框架密码不能为空".to_string());
    }
    if request.poll_attempts == 0 || request.poll_interval_secs == 0 {
        return Err("轮询次数和间隔必须大于 0".to_string());
    }
    Ok(())
}

fn expected_server_count(request: &DeploymentTopologyRequest) -> usize {
    1 + usize::from(matches!(
        request.mode,
        TopologyMode::Ha | TopologyMode::HaAndReplica
    )) + request.replica_ips.len()
}

fn ha_body(request: &DeploymentTopologyRequest) -> Value {
    let password = BASE64.encode(request.framework_password.as_bytes());
    let replica_ip = request.ha_replica_ip.as_deref().unwrap_or_default();
    json!({
        "hAPrimaryHostname": compact_hostname(&request.primary_ip),
        "hAReplicaHostname": "HA",
        "hAPrimaryServerIP": request.primary_ip,
        "hAReplicaServerIP": replica_ip,
        "hAVirtualIP": request.virtual_ip.as_deref().unwrap_or_default(),
        "hAPrimaryServerPasswd": password,
        "hAReplicaServerPasswd": password,
    })
}

fn replica_body(replica_ip: &str, primary_ip: &str) -> Value {
    json!({
        "replicaName": compact_hostname(replica_ip),
        "primaryIp": primary_ip,
    })
}

async fn post_switch(
    client: &reqwest::Client,
    ip: &str,
    port: u16,
    path: &str,
    body: Value,
) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect((ip, port)),
    )
    .await
    .map_err(|_| format!("切换接口连接 {ip}:{port} 超时"))?
    .map_err(|error| format!("切换接口连接 {ip}:{port} 失败: {error}"))?;

    let url = format!("http://{ip}:{port}{path}");
    match client.post(url).json(&body).send().await {
        Ok(response) if response.status().is_success() => {
            let response_body = match response.bytes().await {
                Ok(bytes) => bytes,
                // Headers were received, so the non-idempotent request was dispatched. A body
                // disconnect has the same semantics as EOF/reset during send.
                Err(_) => return Ok(()),
            };
            if response_body.is_empty() {
                return Ok(());
            }
            if let Ok(value) = serde_json::from_slice::<Value>(&response_body) {
                let code = value
                    .get("code")
                    .or_else(|| value.get("errCode"))
                    .or_else(|| value.get("ErrCode"))
                    .and_then(Value::as_i64);
                if let Some(code) = code.filter(|code| *code != 0) {
                    let message = value
                        .get("message")
                        .or_else(|| value.get("msg"))
                        .and_then(Value::as_str)
                        .unwrap_or("未知业务错误");
                    return Err(format!("切换接口业务拒绝（code={}）：{message}", code));
                }
            }
            Ok(())
        }
        Ok(response) => Err(format!("切换接口返回 HTTP {}", response.status().as_u16())),
        // The appliance normally closes the connection immediately and restarts services.
        // Because TCP preflight succeeded, this is an ambiguous dispatched request, not a
        // reason to replay a non-idempotent switch operation.
        Err(error) if error.is_connect() => Err(format!("切换接口请求尚未建立连接: {error}")),
        Err(_) => Ok(()),
    }
}

async fn framework_token(
    client: &reqwest::Client,
    ip: &str,
    port: u16,
    password: &str,
) -> Result<String, String> {
    let response = client
        .post(format!("http://{ip}:{port}/openAPI/userMgr/v1/login"))
        .json(&json!({
            "userName": "admin",
            "userPasswd": crate::sha256_hex(password),
            "isUnlockLogin": false,
        }))
        .send()
        .await
        .map_err(|error| format!("框架登录失败: {error}"))?;
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("框架登录响应无效: {error}"))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("框架登录被拒绝")
            .to_string());
    }
    value
        .pointer("/data/token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "框架登录响应缺少 token".to_string())
}

fn parse_server_list(value: &Value) -> Vec<TopologyServer> {
    value
        .pointer("/data/resTree/orgInfo/serverList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|server| TopologyServer {
            server_ip: server
                .get("serverIP")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            server_name: server
                .get("serverName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            ha_type: server
                .get("haType")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            server_status: server
                .get("serverStatus")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            is_deployed: server
                .get("isDeployed")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            virtual_ip: server
                .get("virtualIP")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

async fn query_servers(
    client: &reqwest::Client,
    primary_ip: &str,
    port: u16,
    token: &str,
) -> Result<Vec<TopologyServer>, String> {
    let response = client
        .post(format!("http://{primary_ip}:{port}/openAPI/res/v1/query"))
        .header("authorization", token)
        .json(&json!({ "resType": 2, "parentOrgCode": 1 }))
        .send()
        .await
        .map_err(|error| format!("服务器列表查询失败: {error}"))?;
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("服务器列表响应无效: {error}"))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("服务器列表查询失败")
            .to_string());
    }
    Ok(parse_server_list(&value))
}

async fn ha_is_ready(
    client: &reqwest::Client,
    virtual_ip: &str,
    port: u16,
    token: &str,
) -> Result<bool, String> {
    let response = client
        .post(format!("http://{virtual_ip}:{port}/api/status"))
        .json(&json!({ "Authorization": token }))
        .send()
        .await
        .map_err(|error| format!("主备状态查询失败: {error}"))?;
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("主备状态响应无效: {error}"))?;
    if value.get("ErrCode").and_then(Value::as_i64) != Some(0) {
        return Ok(false);
    }
    let statuses = value
        .get("STATUS")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let by_name = statuses
        .iter()
        .filter_map(|status| Some((status.get("NAME")?.as_str()?, status)))
        .collect::<HashMap<_, _>>();
    let ready = |name: &str, product_status: &str| {
        by_name.get(name).is_some_and(|status| {
            status.get("HASTATUS").and_then(Value::as_str) == Some("online")
                && status.get("HASTARTSTATUS").and_then(Value::as_str) == Some("YES")
                && status.get("PDTSTATUS").and_then(Value::as_str) == Some(product_status)
        })
    };
    Ok(ready("primary", "running") && ready("replica", "stop"))
}

fn server_list_is_ready(request: &DeploymentTopologyRequest, servers: &[TopologyServer]) -> bool {
    if servers.len() != expected_server_count(request) {
        return false;
    }
    let matches = |ip: &str, ha_type: i64| {
        servers.iter().any(|server| {
            server.server_ip == ip
                && server.ha_type == ha_type
                && server.server_status == 1
                && server.is_deployed == 1
        })
    };
    if !matches(&request.primary_ip, 1) {
        return false;
    }
    if matches!(request.mode, TopologyMode::Ha | TopologyMode::HaAndReplica)
        && !request
            .ha_replica_ip
            .as_deref()
            .is_some_and(|ip| matches(ip, 2))
    {
        return false;
    }
    if matches!(request.mode, TopologyMode::Ha | TopologyMode::HaAndReplica) {
        let expected_virtual_ip = request.virtual_ip.as_deref().unwrap_or_default();
        let ha_ips = [
            request.primary_ip.as_str(),
            request.ha_replica_ip.as_deref().unwrap_or_default(),
        ];
        if servers.iter().any(|server| {
            ha_ips.contains(&server.server_ip.as_str()) && server.virtual_ip != expected_virtual_ip
        }) {
            return false;
        }
    }
    request.replica_ips.iter().all(|ip| matches(ip, 4))
}

async fn query_once(
    client: &reqwest::Client,
    request: &DeploymentTopologyRequest,
    ports: TopologyPorts,
) -> Result<Vec<TopologyServer>, String> {
    let token = framework_token(
        client,
        &request.primary_ip,
        ports.framework,
        &request.framework_password,
    )
    .await?;
    let servers = query_servers(client, &request.primary_ip, ports.framework, &token).await?;
    if !server_list_is_ready(request, &servers) {
        return Err(format!(
            "服务器列表尚未达到预期（当前 {}，预期 {}）",
            servers.len(),
            expected_server_count(request)
        ));
    }
    if matches!(request.mode, TopologyMode::Ha | TopologyMode::HaAndReplica) {
        let virtual_ip = request.virtual_ip.as_deref().unwrap_or_default();
        if !ha_is_ready(client, virtual_ip, ports.ha_status, &token).await? {
            return Err("主备服务状态尚未就绪".to_string());
        }
    }
    Ok(servers)
}

async fn wait_until_ready(
    client: &reqwest::Client,
    request: &DeploymentTopologyRequest,
    ports: TopologyPorts,
    should_cancel: Option<&AtomicBool>,
) -> DeploymentTopologyResult {
    let mut last_error = String::new();
    for attempt in 0..request.poll_attempts {
        if should_cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst)) {
            return DeploymentTopologyResult {
                status: TopologyResultStatus::Failed,
                message: "主备从搭建已取消".to_string(),
                expected_server_count: expected_server_count(request),
                observed_server_count: 0,
                servers: vec![],
            };
        }
        match query_once(client, request, ports).await {
            Ok(servers) => {
                return DeploymentTopologyResult {
                    status: TopologyResultStatus::Success,
                    message: "主备从搭建完成".to_string(),
                    expected_server_count: expected_server_count(request),
                    observed_server_count: servers.len(),
                    servers,
                };
            }
            Err(error) => last_error = error,
        }
        if attempt + 1 < request.poll_attempts {
            let mut remaining = Duration::from_secs(request.poll_interval_secs);
            while !remaining.is_zero() {
                if should_cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst)) {
                    return DeploymentTopologyResult {
                        status: TopologyResultStatus::Failed,
                        message: "主备从搭建已取消".to_string(),
                        expected_server_count: expected_server_count(request),
                        observed_server_count: 0,
                        servers: vec![],
                    };
                }
                let step = remaining.min(Duration::from_millis(250));
                tokio::time::sleep(step).await;
                remaining = remaining.saturating_sub(step);
            }
        }
    }
    DeploymentTopologyResult {
        status: TopologyResultStatus::Unconfirmed,
        message: format!("切换请求已发送，但在等待时限内未确认完成：{last_error}。请手动查询状态后决定是否重试。"),
        expected_server_count: expected_server_count(request),
        observed_server_count: 0,
        servers: vec![],
    }
}

pub(crate) async fn execute_deployment_topology(
    request: DeploymentTopologyRequest,
) -> Result<DeploymentTopologyResult, String> {
    execute_deployment_topology_with_cancel(request, None).await
}

pub(crate) async fn execute_deployment_topology_with_cancel(
    request: DeploymentTopologyRequest,
    should_cancel: Option<Arc<AtomicBool>>,
) -> Result<DeploymentTopologyResult, String> {
    execute_deployment_topology_with_ports(request, should_cancel, TopologyPorts::default()).await
}

async fn execute_deployment_topology_with_ports(
    request: DeploymentTopologyRequest,
    should_cancel: Option<Arc<AtomicBool>>,
    ports: TopologyPorts,
) -> Result<DeploymentTopologyResult, String> {
    validate_request(&request)?;
    let client = reqwest::Client::builder()
        // Appliance management endpoints are always reached directly on the LAN. System proxy
        // settings can otherwise turn a deliberate post-send disconnect into a proxy HTTP 502.
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("HTTP 客户端初始化失败: {error}"))?;

    let ensure_not_cancelled = || {
        if should_cancel
            .as_deref()
            .is_some_and(|cancel| cancel.load(Ordering::SeqCst))
        {
            Err("主备从搭建已取消".to_string())
        } else {
            Ok(())
        }
    };

    if matches!(request.mode, TopologyMode::Ha | TopologyMode::HaAndReplica) {
        ensure_not_cancelled()?;
        post_switch(
            &client,
            &request.primary_ip,
            ports.switch,
            "/openAPI/serverMgr/v1/groupType/switch/singleToHA",
            ha_body(&request),
        )
        .await?;
        if request.mode == TopologyMode::HaAndReplica {
            let mut ha_stage = request.clone();
            ha_stage.mode = TopologyMode::Ha;
            ha_stage.replica_ips.clear();
            let result =
                wait_until_ready(&client, &ha_stage, ports, should_cancel.as_deref()).await;
            if result.status != TopologyResultStatus::Success {
                return Ok(result);
            }
        }
    }

    let replica_primary = if matches!(request.mode, TopologyMode::HaAndReplica) {
        request.virtual_ip.as_deref().unwrap_or(&request.primary_ip)
    } else {
        &request.primary_ip
    };
    let mut processed_replicas = Vec::new();
    let mut last_result = None;
    for replica_ip in &request.replica_ips {
        ensure_not_cancelled()?;
        post_switch(
            &client,
            replica_ip,
            ports.switch,
            "/openAPI/serverMgr/v1/deployType/switch/singleToReplica",
            replica_body(replica_ip, replica_primary),
        )
        .await?;
        processed_replicas.push(replica_ip.clone());
        let mut replica_stage = request.clone();
        replica_stage.replica_ips.clone_from(&processed_replicas);
        last_result =
            Some(wait_until_ready(&client, &replica_stage, ports, should_cancel.as_deref()).await);
        if last_result
            .as_ref()
            .is_some_and(|result| result.status != TopologyResultStatus::Success)
        {
            return Ok(last_result.expect("result was just assigned"));
        }
    }
    Ok(match last_result {
        Some(result) => result,
        None => wait_until_ready(&client, &request, ports, should_cancel.as_deref()).await,
    })
}

#[tauri::command]
pub async fn configure_deployment_topology(
    request: DeploymentTopologyRequest,
) -> Result<DeploymentTopologyResult, String> {
    execute_deployment_topology(request).await
}

#[tauri::command]
pub async fn query_deployment_topology(
    request: DeploymentTopologyRequest,
) -> Result<DeploymentTopologyResult, String> {
    validate_request(&request)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("HTTP 客户端初始化失败: {error}"))?;
    match query_once(&client, &request, TopologyPorts::default()).await {
        Ok(servers) => Ok(DeploymentTopologyResult {
            status: TopologyResultStatus::Success,
            message: "主备从状态正常".to_string(),
            expected_server_count: expected_server_count(&request),
            observed_server_count: servers.len(),
            servers,
        }),
        Err(message) => Ok(DeploymentTopologyResult {
            status: TopologyResultStatus::Failed,
            message,
            expected_server_count: expected_server_count(&request),
            observed_server_count: 0,
            servers: vec![],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn combined_request() -> DeploymentTopologyRequest {
        DeploymentTopologyRequest {
            mode: TopologyMode::HaAndReplica,
            primary_ip: "192.115.1.17".to_string(),
            ha_replica_ip: Some("192.115.1.55".to_string()),
            virtual_ip: Some("192.115.1.128".to_string()),
            replica_ips: vec!["192.115.1.18".to_string()],
            framework_password: "admin_123".to_string(),
            poll_interval_secs: 30,
            poll_attempts: 20,
        }
    }

    #[test]
    fn ha_payload_uses_base64_password_and_compact_hostname() {
        let body = ha_body(&combined_request());
        assert_eq!(body["hAPrimaryHostname"], "192115117");
        assert_eq!(body["hAPrimaryServerPasswd"], "YWRtaW5fMTIz");
        assert_eq!(body["hAReplicaServerPasswd"], "YWRtaW5fMTIz");
    }

    #[test]
    fn combined_server_list_requires_all_three_roles() {
        let request = combined_request();
        let server = |ip: &str, ha_type| TopologyServer {
            server_ip: ip.to_string(),
            server_name: compact_hostname(ip),
            ha_type,
            server_status: 1,
            is_deployed: 1,
            virtual_ip: "192.115.1.128".to_string(),
        };
        let servers = vec![
            server("192.115.1.17", 1),
            server("192.115.1.55", 2),
            server("192.115.1.18", 4),
        ];
        assert!(server_list_is_ready(&request, &servers));
        assert!(!server_list_is_ready(&request, &servers[..2]));
        let mut wrong_virtual_ip = servers.clone();
        wrong_virtual_ip[1].virtual_ip = "192.115.1.200".to_string();
        assert!(!server_list_is_ready(&request, &wrong_virtual_ip));
    }

    #[test]
    fn replica_mode_supports_multiple_replica_ips() {
        let mut request = combined_request();
        request.mode = TopologyMode::Replica;
        request.ha_replica_ip = None;
        request.virtual_ip = None;
        request.replica_ips.push("192.115.1.19".to_string());
        assert_eq!(expected_server_count(&request), 3);
        assert!(validate_request(&request).is_ok());
    }

    fn mock_server_list(roles: &[(i64, &str)], virtual_ip: &str) -> Value {
        let server_list = roles
            .iter()
            .map(|(ha_type, ip)| {
                json!({
                    "serverIP": ip,
                    "serverName": compact_hostname(ip),
                    "haType": ha_type,
                    "serverStatus": 1,
                    "isDeployed": 1,
                    "virtualIP": virtual_ip,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "code": 0,
            "message": "Success.",
            "data": { "resTree": { "orgInfo": { "serverList": server_list } } }
        })
    }

    async fn mount_framework_mocks(server: &MockServer, server_list: Value) {
        Mock::given(method("POST"))
            .and(path("/openAPI/userMgr/v1/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": { "token": "framework-token" }
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/openAPI/res/v1/query"))
            .and(body_json(json!({ "resType": 2, "parentOrgCode": 1 })))
            .respond_with(ResponseTemplate::new(200).set_body_json(server_list))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn ha_switch_payload_and_readiness_are_verified_over_http() {
        let server = MockServer::start().await;
        let port = server.address().port();
        Mock::given(method("POST"))
            .and(path("/openAPI/serverMgr/v1/groupType/switch/singleToHA"))
            .and(body_json(json!({
                "hAPrimaryHostname": "127001",
                "hAReplicaHostname": "HA",
                "hAPrimaryServerIP": "127.0.0.1",
                "hAReplicaServerIP": "127.0.0.1",
                "hAVirtualIP": "127.0.0.1",
                "hAPrimaryServerPasswd": "YWRtaW5fMTIz",
                "hAReplicaServerPasswd": "YWRtaW5fMTIz"
            })))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        mount_framework_mocks(
            &server,
            mock_server_list(&[(1, "127.0.0.1"), (2, "127.0.0.1")], "127.0.0.1"),
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/status"))
            .and(body_json(json!({ "Authorization": "framework-token" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ErrCode": 0,
                "STATUS": [
                    { "NAME": "primary", "HASTATUS": "online", "HASTARTSTATUS": "YES", "PDTSTATUS": "running" },
                    { "NAME": "replica", "HASTATUS": "online", "HASTARTSTATUS": "YES", "PDTSTATUS": "stop" }
                ]
            })))
            .mount(&server)
            .await;

        let request = DeploymentTopologyRequest {
            mode: TopologyMode::Ha,
            primary_ip: "127.0.0.1".to_string(),
            ha_replica_ip: Some("127.0.0.1".to_string()),
            virtual_ip: Some("127.0.0.1".to_string()),
            replica_ips: vec![],
            framework_password: "admin_123".to_string(),
            poll_interval_secs: 1,
            poll_attempts: 1,
        };
        let result = execute_deployment_topology_with_ports(
            request,
            None,
            TopologyPorts {
                framework: port,
                ha_status: port,
                switch: port,
            },
        )
        .await
        .expect("topology execution should complete");
        assert_eq!(result.status, TopologyResultStatus::Success);
        assert_eq!(result.observed_server_count, 2);
    }

    #[tokio::test]
    async fn connection_drop_after_switch_send_is_treated_as_dispatched() {
        let framework = MockServer::start().await;
        mount_framework_mocks(
            &framework,
            mock_server_list(&[(1, "127.0.0.1"), (4, "127.0.0.1")], ""),
        )
        .await;

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let switch_port = listener.local_addr().unwrap().port();
        let disconnect_server = tokio::spawn(async move {
            // The first connection is the TCP preflight; the second carries the POST.
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                drop(stream);
            }
        });
        let request = DeploymentTopologyRequest {
            mode: TopologyMode::Replica,
            primary_ip: "127.0.0.1".to_string(),
            ha_replica_ip: None,
            virtual_ip: None,
            replica_ips: vec!["127.0.0.1".to_string()],
            framework_password: "admin_123".to_string(),
            poll_interval_secs: 1,
            poll_attempts: 1,
        };
        let result = execute_deployment_topology_with_ports(
            request,
            None,
            TopologyPorts {
                framework: framework.address().port(),
                ha_status: framework.address().port(),
                switch: switch_port,
            },
        )
        .await
        .expect("a dropped response after dispatch must not replay the switch");
        disconnect_server.await.unwrap();
        assert_eq!(result.status, TopologyResultStatus::Success);
    }

    #[tokio::test]
    async fn explicit_switch_http_failure_is_not_reported_as_dispatched() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/openAPI/serverMgr/v1/deployType/switch/singleToReplica",
            ))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;
        let request = DeploymentTopologyRequest {
            mode: TopologyMode::Replica,
            primary_ip: "127.0.0.1".to_string(),
            ha_replica_ip: None,
            virtual_ip: None,
            replica_ips: vec!["127.0.0.1".to_string()],
            framework_password: "admin_123".to_string(),
            poll_interval_secs: 1,
            poll_attempts: 1,
        };
        let error = execute_deployment_topology_with_ports(
            request,
            None,
            TopologyPorts {
                framework: server.address().port(),
                ha_status: server.address().port(),
                switch: server.address().port(),
            },
        )
        .await
        .expect_err("HTTP 500 is a definite business failure");
        assert!(error.contains("HTTP 500"));
    }

    #[tokio::test]
    async fn explicit_switch_business_rejection_is_reported() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/openAPI/serverMgr/v1/deployType/switch/singleToReplica",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 72146,
                "message": "switch rejected"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let request = DeploymentTopologyRequest {
            mode: TopologyMode::Replica,
            primary_ip: "127.0.0.1".to_string(),
            ha_replica_ip: None,
            virtual_ip: None,
            replica_ips: vec!["127.0.0.1".to_string()],
            framework_password: "admin_123".to_string(),
            poll_interval_secs: 1,
            poll_attempts: 1,
        };
        let error = execute_deployment_topology_with_ports(
            request,
            None,
            TopologyPorts {
                framework: server.address().port(),
                ha_status: server.address().port(),
                switch: server.address().port(),
            },
        )
        .await
        .expect_err("a non-zero business code is a definite rejection");
        assert!(error.contains("72146"));
        assert!(error.contains("switch rejected"));
    }
}
