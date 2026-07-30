//! Registers running virtual devices with configured UMS servers.
//!
//! Passwords and access tokens stay in the main process and are never emitted
//! to the simulator run log or forwarded to the elevated Worker.

use crate::config::DeviceSimulatorSettings;
use crate::ums_init_password::{rsa_pkcs1v15_encrypt_base64, ums_acquire_access_token};
use crate::AppState;
use app_lib::device_simulator::api::DEVICE_SIMULATOR_EVENT_LOG;
use app_lib::device_simulator::errors::SimulatorErrorBody;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

pub const PLATFORM_RSA_PUBLIC_KEY_PATH: &str = "/openAPI/oauth/v1/rsa/publicKey/get";
pub const PLATFORM_ADD_DEVICE_PATH: &str = "/openAPI/deviceManange/v1/encodeDevice/add";
pub const PLATFORM_QUERY_DEVICE_PATH: &str = "/xapi/uap/v1/resource/query";
pub const PLATFORM_DELETE_DEVICE_PATH: &str = "/openAPI/deviceManange/v1/encodeDevice/delete";

const DEVICE_ORG_ID: &str = "2";
const DEVICE_ACCESS_USER: &str = "admin";
const DEVICE_ACCESS_PASSWORD: &str = "Admin_1234";
const DEVICE_TYPE: u8 = 1;
const RESOURCE_QUERY_PAGE_SIZE: u32 = 200;
const PLATFORM_LOG_BODY_LIMIT: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformDeviceEntry {
    pub address: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAddDeviceOutcome {
    pub address: String,
    pub added: bool,
    pub device_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformServerAddResult {
    pub server_id: String,
    pub host: String,
    pub port: u16,
    pub success: bool,
    /// One of `login`, `query`, `delete`, `public_key`, or `add`.
    pub failed_at: Option<String>,
    pub message: Option<String>,
    pub devices: Vec<PlatformAddDeviceOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAddDevicesReport {
    pub servers: Vec<PlatformServerAddResult>,
    /// Registration attempts, so multiple UMS servers multiply this count.
    pub total_devices: u32,
    pub added_devices: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddDeviceRequest {
    device_list: Vec<AddDeviceItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddDeviceItem {
    device_index_code: u32,
    device_name: String,
    org_id: &'static str,
    device_addr: Ipv4Addr,
    device_port: u16,
    user_name: &'static str,
    password: String,
    ms_policy: u8,
    ms_code: &'static str,
    playback_media_policy: u8,
    playback_ms_code: &'static str,
    device_type: u8,
    media_protocol: u8,
    access_type: u8,
    access_protocol: u8,
    access_network: u8,
    channel_name_policy: u8,
    enable_stream_tls: u8,
}

#[derive(Debug, Deserialize)]
struct OpenApiEnvelope<T> {
    #[serde(default, alias = "Code")]
    code: i64,
    #[serde(default, alias = "msg")]
    message: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicKeyData {
    public_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddDeviceData {
    #[serde(default)]
    success_list: Vec<AddDeviceSuccess>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddDeviceSuccess {
    device_index_code: u32,
    device_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryDeviceRequest {
    parent_id: &'static str,
    page_size: u32,
    page_no: u32,
    condition: Vec<QueryCondition>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryCondition {
    query_type: u32,
    logic_flag: u8,
    query_data: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryDeviceData {
    #[serde(default)]
    total: u64,
    #[serde(default)]
    info_list: Vec<QueryDeviceInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryDeviceInfo {
    #[serde(default)]
    res_id: String,
    #[serde(default, rename = "IPAddr")]
    ip_address: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteDeviceRequest {
    device_list: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteDeviceData {
    #[serde(default)]
    success_list: Vec<DeleteDeviceSuccess>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteDeviceSuccess {
    device_id: String,
}

#[tauri::command]
pub async fn device_simulator_add_devices_to_platform(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
    devices: Vec<PlatformDeviceEntry>,
    replace_existing: bool,
) -> Result<PlatformAddDevicesReport, SimulatorErrorBody> {
    if devices.is_empty() {
        return Err(platform_error(
            "device_simulator.platform.devices_missing",
            "deviceSimulator.errors.platformAddFailed",
            "没有可添加的虚拟设备",
        ));
    }
    let (settings, timeout_secs) = {
        let config = app_state.config.lock().unwrap();
        (
            config.device_simulator.clone(),
            config.framework_password_api_timeout_secs,
        )
    };
    validate_registration_settings(&settings)?;
    let client = crate::build_device_http_client_with_timeout(Duration::from_secs(timeout_secs))
        .map_err(|details| {
            platform_error(
                "device_simulator.platform.http_client_failed",
                "deviceSimulator.errors.platformAddFailed",
                details,
            )
        })?;

    let total_devices = devices
        .len()
        .saturating_mul(settings.last_platform_servers.len())
        .try_into()
        .unwrap_or(u32::MAX);
    let mut added_devices = 0_u32;
    let mut servers = Vec::with_capacity(settings.last_platform_servers.len());
    for server in &settings.last_platform_servers {
        emit_log(
            &app_handle,
            "info",
            None,
            format!(
                "开始向 {}:{} 添加 {} 台虚拟设备",
                server.host,
                server.port,
                devices.len()
            ),
        );
        let result = register_server(
            &app_handle,
            &client,
            server,
            &settings.platform_username,
            &settings.platform_password,
            &devices,
            replace_existing,
        )
        .await;
        added_devices = added_devices.saturating_add(
            result
                .devices
                .iter()
                .filter(|outcome| outcome.added)
                .count() as u32,
        );
        let (level, error_code, message) = if result.success {
            (
                "info",
                None,
                format!(
                    "{}:{} 已添加全部 {} 台虚拟设备",
                    server.host,
                    server.port,
                    devices.len()
                ),
            )
        } else {
            let code = match result.failed_at.as_deref() {
                Some("login") => "device_simulator.platform.login_failed",
                Some("public_key") => "device_simulator.platform.public_key_failed",
                _ => "device_simulator.platform.add_failed",
            };
            (
                "error",
                Some(code),
                format!(
                    "{}:{} 添加未全部成功：{}",
                    server.host,
                    server.port,
                    result
                        .message
                        .as_deref()
                        .unwrap_or("部分设备未出现在成功列表")
                ),
            )
        };
        emit_log(&app_handle, level, error_code, message);
        servers.push(result);
    }

    Ok(PlatformAddDevicesReport {
        servers,
        total_devices,
        added_devices,
    })
}

fn validate_registration_settings(
    settings: &DeviceSimulatorSettings,
) -> Result<(), SimulatorErrorBody> {
    if settings.last_platform_servers.is_empty() {
        return Err(platform_error(
            "device_simulator.platform.server_missing",
            "deviceSimulator.errors.platformServerMissing",
            "请先配置至少一台 UMS 服务器",
        ));
    }
    if settings.platform_username.trim().is_empty() || settings.platform_password.is_empty() {
        return Err(platform_error(
            "device_simulator.platform.credentials_missing",
            "deviceSimulator.errors.platformCredentialsMissing",
            "UMS 用户名或密码为空",
        ));
    }
    Ok(())
}

async fn register_server(
    app_handle: &AppHandle,
    client: &reqwest::Client,
    server: &app_lib::device_simulator::api::TargetPlatformServer,
    username: &str,
    password: &str,
    devices: &[PlatformDeviceEntry],
    replace_existing: bool,
) -> PlatformServerAddResult {
    let failure = |failed_at: &str, message: String| PlatformServerAddResult {
        server_id: server.id.clone(),
        host: server.host.clone(),
        port: server.port,
        success: false,
        failed_at: Some(failed_at.to_string()),
        message: Some(message),
        devices: vec![],
    };

    let login_url = format!("http://{}:{}/sw/login", server.host.trim(), server.port);
    emit_log(
        app_handle,
        "info",
        None,
        format!(
            "HTTP request: POST {login_url} | UMS challenge login (2 requests) | credentials and request bodies=<redacted>"
        ),
    );
    let token = match ums_acquire_access_token(
        client,
        &server.host,
        server.port,
        username,
        password,
    )
    .await
    {
        Ok(token) => {
            emit_log(
                    app_handle,
                    "info",
                    None,
                    format!(
                        "HTTP response: POST {login_url} | UMS challenge login succeeded | AccessCode/AccessToken=<redacted>"
                    ),
                );
            token
        }
        Err(message) => {
            emit_log(
                app_handle,
                "error",
                Some("device_simulator.platform.login_failed"),
                format!("HTTP failure: POST {login_url} | {message}"),
            );
            return failure("login", message);
        }
    };

    if replace_existing {
        let existing_device_ids =
            match query_existing_device_ids(app_handle, client, server, &token, devices).await {
                Ok(device_ids) => device_ids,
                Err(message) => return failure("query", message),
            };
        if let Err(message) =
            delete_existing_devices(app_handle, client, server, &token, &existing_device_ids).await
        {
            return failure("delete", message);
        }
    }

    let public_key_url = match server_url(&server.host, server.port, PLATFORM_RSA_PUBLIC_KEY_PATH) {
        Ok(url) => url,
        Err(message) => return failure("public_key", message),
    };
    let public_key = match fetch_public_key(app_handle, client, public_key_url, &token).await {
        Ok(public_key) => public_key,
        Err(message) => return failure("public_key", message),
    };
    let encrypted_password = match encrypt_device_password(&public_key) {
        Ok(password) => password,
        Err(message) => return failure("public_key", message),
    };

    let add_url = match server_url(&server.host, server.port, PLATFORM_ADD_DEVICE_PATH) {
        Ok(url) => url,
        Err(message) => return failure("add", message),
    };
    let request = build_add_request(devices, &encrypted_password);
    let add_url_for_log = add_url.as_str().to_owned();
    emit_log(
        app_handle,
        "info",
        None,
        format!(
            "HTTP request: POST {add_url_for_log} | authorization=<redacted> | Content-Type=application/json | body={}",
            json_for_log(&request)
        ),
    );
    let response = match client
        .post(add_url)
        .header("authorization", &token)
        .json(&request)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return failure("add", format!("添加设备请求失败: {error}")),
    };
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<missing>")
        .to_owned();
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            emit_log(
                app_handle,
                "error",
                Some("device_simulator.platform.add_failed"),
                format!(
                    "HTTP response read failed: POST {add_url_for_log} | status={} | {error}",
                    status.as_u16()
                ),
            );
            return failure(
                "add",
                format!("Failed to read add-device response: {error}"),
            );
        }
    };
    emit_http_response_log(
        app_handle,
        "POST",
        &add_url_for_log,
        status,
        &content_type,
        &text,
    );
    if !status.is_success() {
        return failure("add", format!("添加设备接口返回 HTTP {}", status.as_u16()));
    }
    let response: OpenApiEnvelope<AddDeviceData> = match serde_json::from_str(&text) {
        Ok(response) => response,
        Err(error) => return failure("add", format!("添加设备响应不是有效 JSON: {error}")),
    };
    if response.code != 0 {
        return failure(
            "add",
            format!("添加设备失败：code={} {}", response.code, response.message),
        );
    }
    let success_list = response
        .data
        .map(|data| data.success_list)
        .unwrap_or_default();
    let outcomes = outcomes_from_success(devices, success_list);
    let success = outcomes.iter().all(|outcome| outcome.added);
    PlatformServerAddResult {
        server_id: server.id.clone(),
        host: server.host.clone(),
        port: server.port,
        success,
        failed_at: (!success).then(|| "add".to_string()),
        message: (!success).then(|| "部分设备未出现在 successList".to_string()),
        devices: outcomes,
    }
}

fn build_query_request(page_no: u32) -> QueryDeviceRequest {
    QueryDeviceRequest {
        parent_id: DEVICE_ORG_ID,
        page_size: RESOURCE_QUERY_PAGE_SIZE,
        page_no,
        condition: vec![
            QueryCondition {
                query_type: 910,
                logic_flag: 8,
                query_data: "[[37,1017],[37,1018]]",
            },
            QueryCondition {
                query_type: 910,
                logic_flag: 0,
                query_data: "[[37],[1008],[1009],[1,2019]]",
            },
            QueryCondition {
                query_type: 1000,
                logic_flag: 0,
                query_data: "1",
            },
            QueryCondition {
                query_type: 612,
                logic_flag: 0,
                query_data: "1",
            },
            QueryCondition {
                query_type: 900,
                logic_flag: 8,
                query_data: "3/4",
            },
        ],
    }
}

async fn query_existing_device_ids(
    app_handle: &AppHandle,
    client: &reqwest::Client,
    server: &app_lib::device_simulator::api::TargetPlatformServer,
    token: &str,
    devices: &[PlatformDeviceEntry],
) -> Result<Vec<String>, String> {
    let url = server_url(&server.host, server.port, PLATFORM_QUERY_DEVICE_PATH)?;
    let url_for_log = url.as_str().to_owned();
    let target_addresses = devices
        .iter()
        .map(|device| device.address.to_string())
        .collect::<HashSet<_>>();
    let mut device_ids = Vec::new();
    let mut seen_device_ids = HashSet::new();
    let mut page_no = 1_u32;

    loop {
        let request = build_query_request(page_no);
        emit_log(
            app_handle,
            "info",
            None,
            format!(
                "HTTP request: POST {url_for_log} | authorization=<redacted> | Content-Type=application/json | body={}",
                json_for_log(&request)
            ),
        );
        let response = client
            .post(url.clone())
            .header("authorization", token)
            .json(&request)
            .send()
            .await
            .map_err(|error| format!("查询平台设备请求失败: {error}"))?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing>")
            .to_owned();
        let text = response
            .text()
            .await
            .map_err(|error| format!("读取平台设备查询响应失败: {error}"))?;
        emit_http_response_log(
            app_handle,
            "POST",
            &url_for_log,
            status,
            &content_type,
            &text,
        );
        if !status.is_success() {
            return Err(format!("查询平台设备接口返回 HTTP {}", status.as_u16()));
        }
        let response: OpenApiEnvelope<QueryDeviceData> = serde_json::from_str(&text)
            .map_err(|error| format!("平台设备查询响应不是有效 JSON: {error}"))?;
        if response.code != 0 {
            return Err(format!(
                "查询平台设备失败：code={} {}",
                response.code, response.message
            ));
        }
        let data = response
            .data
            .ok_or_else(|| "平台设备查询响应缺少 data".to_string())?;
        for device in data.info_list {
            if target_addresses.contains(device.ip_address.trim())
                && !device.res_id.trim().is_empty()
                && seen_device_ids.insert(device.res_id.clone())
            {
                device_ids.push(device.res_id);
            }
        }
        if u64::from(page_no).saturating_mul(u64::from(RESOURCE_QUERY_PAGE_SIZE)) >= data.total {
            break;
        }
        page_no = page_no
            .checked_add(1)
            .ok_or_else(|| "平台设备查询页码超出范围".to_string())?;
    }

    emit_log(
        app_handle,
        "info",
        None,
        format!("平台设备查询完成，找到 {} 台同 IP 设备", device_ids.len()),
    );
    Ok(device_ids)
}

async fn delete_existing_devices(
    app_handle: &AppHandle,
    client: &reqwest::Client,
    server: &app_lib::device_simulator::api::TargetPlatformServer,
    token: &str,
    device_ids: &[String],
) -> Result<(), String> {
    if device_ids.is_empty() {
        return Ok(());
    }
    let url = server_url(&server.host, server.port, PLATFORM_DELETE_DEVICE_PATH)?;
    let url_for_log = url.as_str().to_owned();
    let request = DeleteDeviceRequest {
        device_list: device_ids.to_vec(),
    };
    emit_log(
        app_handle,
        "info",
        None,
        format!(
            "HTTP request: POST {url_for_log} | authorization=<redacted> | Content-Type=application/json | body={}",
            json_for_log(&request)
        ),
    );
    let response = client
        .post(url)
        .header("authorization", token)
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("删除平台设备请求失败: {error}"))?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<missing>")
        .to_owned();
    let text = response
        .text()
        .await
        .map_err(|error| format!("读取平台设备删除响应失败: {error}"))?;
    emit_http_response_log(
        app_handle,
        "POST",
        &url_for_log,
        status,
        &content_type,
        &text,
    );
    if !status.is_success() {
        return Err(format!("删除平台设备接口返回 HTTP {}", status.as_u16()));
    }
    let response: OpenApiEnvelope<DeleteDeviceData> = serde_json::from_str(&text)
        .map_err(|error| format!("平台设备删除响应不是有效 JSON: {error}"))?;
    if response.code != 0 {
        return Err(format!(
            "删除平台设备失败：code={} {}",
            response.code, response.message
        ));
    }
    let deleted_ids = response
        .data
        .map(|data| {
            data.success_list
                .into_iter()
                .map(|item| item.device_id)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let missing_ids = device_ids
        .iter()
        .filter(|device_id| !deleted_ids.contains(device_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_ids.is_empty() {
        return Err(format!(
            "平台设备未全部删除，未出现在 successList: {}",
            missing_ids.join(", ")
        ));
    }
    emit_log(
        app_handle,
        "info",
        None,
        format!("已删除 {} 台同 IP 平台设备，准备重新添加", device_ids.len()),
    );
    Ok(())
}

async fn fetch_public_key(
    app_handle: &AppHandle,
    client: &reqwest::Client,
    url: reqwest::Url,
    token: &str,
) -> Result<String, String> {
    let url_for_log = url.as_str().to_owned();
    emit_log(
        app_handle,
        "info",
        None,
        format!("HTTP request: POST {url_for_log} | authorization=<redacted> | body=<empty>"),
    );
    let response = client
        .post(url)
        .header("authorization", token)
        .send()
        .await
        .map_err(|error| format!("获取 RSA 公钥请求失败: {error}"))?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<missing>")
        .to_owned();
    let text = response.text().await.map_err(|error| {
        emit_log(
            app_handle,
            "error",
            Some("device_simulator.platform.public_key_failed"),
            format!(
                "HTTP response read failed: POST {url_for_log} | status={} | {error}",
                status.as_u16()
            ),
        );
        format!("Failed to read RSA public-key response: {error}")
    })?;
    emit_http_response_log(
        app_handle,
        "POST",
        &url_for_log,
        status,
        &content_type,
        &text,
    );
    if !status.is_success() {
        return Err(format!("获取 RSA 公钥接口返回 HTTP {}", status.as_u16()));
    }
    let response: OpenApiEnvelope<PublicKeyData> = serde_json::from_str(&text)
        .map_err(|error| format!("RSA 公钥响应不是有效 JSON: {error}"))?;
    if response.code != 0 {
        return Err(format!(
            "获取 RSA 公钥失败：code={} {}",
            response.code, response.message
        ));
    }
    response
        .data
        .map(|data| data.public_key.trim().to_owned())
        .filter(|public_key| !public_key.is_empty())
        .ok_or_else(|| "RSA 公钥响应缺少 data.publicKey".to_string())
}

fn server_url(host: &str, port: u16, path: &str) -> Result<reqwest::Url, String> {
    let base = reqwest::Url::parse(&format!("http://{}:{}/", host.trim(), port))
        .map_err(|error| format!("UMS 地址无效: {error}"))?;
    base.join(path.trim_start_matches('/'))
        .map_err(|error| format!("UMS 接口地址无效: {error}"))
}

fn build_add_request(
    devices: &[PlatformDeviceEntry],
    encrypted_password: &str,
) -> AddDeviceRequest {
    AddDeviceRequest {
        device_list: devices
            .iter()
            .enumerate()
            .map(|(index, device)| AddDeviceItem {
                device_index_code: (index + 1).try_into().unwrap_or(u32::MAX),
                device_name: device.address.to_string(),
                org_id: DEVICE_ORG_ID,
                device_addr: device.address,
                device_port: device.port,
                user_name: DEVICE_ACCESS_USER,
                password: encrypted_password.to_owned(),
                ms_policy: 1,
                ms_code: "",
                playback_media_policy: 1,
                playback_ms_code: "",
                device_type: DEVICE_TYPE,
                media_protocol: 2,
                access_type: 1,
                access_protocol: 1,
                access_network: 1,
                channel_name_policy: 1,
                enable_stream_tls: 2,
            })
            .collect(),
    }
}

fn encrypt_device_password(public_key: &str) -> Result<String, String> {
    let ciphertext = rsa_pkcs1v15_encrypt_base64(public_key, DEVICE_ACCESS_PASSWORD)?;
    let ciphertext_len = BASE64
        .decode(&ciphertext)
        .map_err(|error| format!("RSA 密文 base64 校验失败: {error}"))?
        .len();
    if ciphertext_len != 128 {
        return Err(format!(
            "openAPI 返回的公钥不是 RSA-1024：密文长度为 {ciphertext_len} 字节"
        ));
    }
    Ok(ciphertext)
}

fn outcomes_from_success(
    devices: &[PlatformDeviceEntry],
    success_list: Vec<AddDeviceSuccess>,
) -> Vec<PlatformAddDeviceOutcome> {
    let success_by_index = success_list
        .into_iter()
        .map(|entry| (entry.device_index_code, entry.device_id))
        .collect::<HashMap<_, _>>();
    devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let device_id = success_by_index.get(&((index + 1) as u32)).cloned();
            PlatformAddDeviceOutcome {
                address: device.address.to_string(),
                added: device_id.is_some(),
                device_id,
                message: (!success_by_index.contains_key(&((index + 1) as u32)))
                    .then(|| "未出现在 successList".to_string()),
            }
        })
        .collect()
}

fn json_for_log(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(mut value) => {
            redact_sensitive_json_fields(&mut value);
            truncate_log_text(&value.to_string())
        }
        Err(error) => format!("<failed to serialize request: {error}>"),
    }
}

fn response_body_for_log(body: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(mut value) => {
            redact_sensitive_json_fields(&mut value);
            truncate_log_text(&value.to_string())
        }
        Err(_) => truncate_log_text(body),
    }
}

fn redact_sensitive_json_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                let normalized = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                if matches!(
                    normalized.as_str(),
                    "authorization" | "password" | "accesstoken" | "accesscode" | "loginsignature"
                ) {
                    *value = serde_json::Value::String("<redacted>".to_string());
                } else {
                    redact_sensitive_json_fields(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_sensitive_json_fields(value);
            }
        }
        _ => {}
    }
}

fn truncate_log_text(value: &str) -> String {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length <= PLATFORM_LOG_BODY_LIMIT {
        return if trimmed.is_empty() {
            "<empty>".to_string()
        } else {
            trimmed.to_string()
        };
    }
    let head = trimmed
        .chars()
        .take(PLATFORM_LOG_BODY_LIMIT)
        .collect::<String>();
    format!("{head}... <truncated; total {length} characters>")
}

fn emit_http_response_log(
    app_handle: &AppHandle,
    method: &str,
    url: &str,
    status: reqwest::StatusCode,
    content_type: &str,
    body: &str,
) {
    emit_log(
        app_handle,
        if status.is_success() { "info" } else { "error" },
        (!status.is_success()).then_some("device_simulator.platform.http_status_failed"),
        format!(
            "HTTP response: {method} {url} | status={} | Content-Type={content_type} | body={}",
            status.as_u16(),
            response_body_for_log(body)
        ),
    );
}

fn emit_log(app_handle: &AppHandle, level: &str, error_code: Option<&str>, message: String) {
    let payload = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "level": level,
        "session_id": null,
        "component": "platform:register",
        "profile_id": null,
        "device_id": null,
        "device_ip": null,
        "channel_id": null,
        "alarm_job_id": null,
        "rtsp_session_id": null,
        "error_code": error_code,
        "message": message,
    });
    let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_LOG, payload);
}

fn platform_error(
    code: impl Into<String>,
    message_key: impl Into<String>,
    details: impl Into<String>,
) -> SimulatorErrorBody {
    SimulatorErrorBody::new(code, message_key).with_public_details(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SAMPLE_PUBLIC_KEY: &str = "MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDuSsGxESaDuynmBqlGj48F/DfGe7k0Pjnq4aaFhpAOzTtJCdUuTq7QWxrfOhsmREOX6GZJ7c6VVpDm/pOqgH+YFU3oBftKJW40VnQmItvlQduGHUYnXiynHHp17ZS8X/wYidmBgzqOnskrIOUMxc6cRp1spSOx3If7RDjGGIbgDQIDAQAB";

    fn devices() -> Vec<PlatformDeviceEntry> {
        vec![
            PlatformDeviceEntry {
                address: "192.115.1.69".parse().unwrap(),
                port: 80,
            },
            PlatformDeviceEntry {
                address: "192.115.1.70".parse().unwrap(),
                port: 80,
            },
        ]
    }

    #[test]
    fn open_api_public_key_produces_a_1024_bit_pkcs1_ciphertext() {
        let ciphertext = encrypt_device_password(SAMPLE_PUBLIC_KEY).unwrap();
        assert_eq!(BASE64.decode(ciphertext).unwrap().len(), 128);
    }

    #[test]
    fn public_key_response_accepts_success_payload_without_code() {
        let response: OpenApiEnvelope<PublicKeyData> = serde_json::from_value(json!({
            "message": "Succeed.",
            "data": { "publicKey": SAMPLE_PUBLIC_KEY }
        }))
        .unwrap();

        assert_eq!(response.code, 0);
        assert_eq!(
            response.data.unwrap().public_key,
            SAMPLE_PUBLIC_KEY.to_string()
        );
    }

    #[test]
    fn diagnostic_json_redacts_credentials_tokens_and_device_passwords() {
        let logged = json_for_log(&json!({
            "authorization": "token-secret",
            "AccessToken": "access-token-secret",
            "nested": {
                "AccessCode": "access-code-secret",
                "LoginSignature": "signature-secret",
                "password": "ciphertext-secret"
            },
            "data": { "publicKey": SAMPLE_PUBLIC_KEY }
        }));

        for secret in [
            "token-secret",
            "access-token-secret",
            "access-code-secret",
            "signature-secret",
            "ciphertext-secret",
        ] {
            assert!(!logged.contains(secret));
        }
        assert!(logged.contains(SAMPLE_PUBLIC_KEY));
        assert!(logged.contains("<redacted>"));
    }

    #[test]
    fn one_request_contains_every_device_with_the_verified_constants() {
        let request = serde_json::to_value(build_add_request(&devices(), "ciphertext")).unwrap();
        assert_eq!(request["deviceList"].as_array().unwrap().len(), 2);
        assert_eq!(request["deviceList"][0]["deviceIndexCode"], 1);
        assert_eq!(request["deviceList"][1]["deviceIndexCode"], 2);
        assert_eq!(request["deviceList"][0]["deviceType"], 1);
        assert_eq!(request["deviceList"][0]["devicePort"], 80);
        assert_eq!(request["deviceList"][0]["orgId"], DEVICE_ORG_ID);
        assert_eq!(request["deviceList"][0]["password"], "ciphertext");
    }

    #[test]
    fn resource_query_request_matches_the_platform_contract_and_requested_page() {
        let request = serde_json::to_value(build_query_request(3)).unwrap();

        assert_eq!(request["parentId"], "2");
        assert_eq!(request["pageSize"], 200);
        assert_eq!(request["pageNo"], 3);
        assert_eq!(
            request["condition"],
            json!([
                {"queryType": 910, "logicFlag": 8, "queryData": "[[37,1017],[37,1018]]"},
                {"queryType": 910, "logicFlag": 0, "queryData": "[[37],[1008],[1009],[1,2019]]"},
                {"queryType": 1000, "logicFlag": 0, "queryData": "1"},
                {"queryType": 612, "logicFlag": 0, "queryData": "1"},
                {"queryType": 900, "logicFlag": 8, "queryData": "3/4"}
            ])
        );
    }

    #[test]
    fn resource_query_response_reads_the_exact_ipaddr_and_res_id_fields() {
        let response: OpenApiEnvelope<QueryDeviceData> = serde_json::from_value(json!({
            "code": 0,
            "message": "Succeed",
            "data": {
                "total": 201,
                "infoList": [{
                    "resId": "630621988062232867",
                    "IPAddr": "192.115.1.220"
                }]
            }
        }))
        .unwrap();
        let data = response.data.unwrap();

        assert_eq!(data.total, 201);
        assert_eq!(data.info_list[0].res_id, "630621988062232867");
        assert_eq!(data.info_list[0].ip_address, "192.115.1.220");
    }

    #[test]
    fn delete_request_uses_the_matched_resource_ids() {
        let request = serde_json::to_value(DeleteDeviceRequest {
            device_list: vec!["630621988062232867".to_string()],
        })
        .unwrap();

        assert_eq!(request, json!({"deviceList": ["630621988062232867"]}));
    }

    #[test]
    fn success_list_is_correlated_by_request_index() {
        let response: OpenApiEnvelope<AddDeviceData> = serde_json::from_value(json!({
            "code": 0,
            "message": "Succeed.",
            "data": { "successList": [{ "deviceIndexCode": 1, "deviceId": "630568641095532835" }] }
        }))
        .unwrap();
        let outcomes = outcomes_from_success(&devices(), response.data.unwrap().success_list);
        assert!(outcomes[0].added);
        assert_eq!(outcomes[0].device_id.as_deref(), Some("630568641095532835"));
        assert!(!outcomes[1].added);
        assert_eq!(outcomes[1].message.as_deref(), Some("未出现在 successList"));
    }
}
