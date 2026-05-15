use redis::aio::MultiplexedConnection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

const DISK_SERVER_LIST_PATH: &str = "/openAPI/system/v1/disk/server/list";
const DISK_LIST_PATH: &str = "/openAPI/system/v1/disk/list";
const RAW_DISK_LIST_PATH: &str = "/openAPI/system/v1/raw-disk/list";
const IPSAN_LIST_PATH: &str = "/openAPI/system/v1/IPSAN/list";
const IPSAN_RESOURCE_GROUP_LIST_PATH: &str = "/openAPI/system/v1/IPSAN/resourceGroup/list";
const MAINLINE_STATUS_PATH: &str = "/distapi/status";
const REDIS_PORT: u16 = 6379;
const REDIS_PASSWORD: &str = "ums@redis_service";
const REDIS_OP_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const STORAGE_KEY_PREFIX: &str = "Storage:";
const CACHE_PREVIEW_MAX_CHARS: usize = 240;
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskServerItem {
    #[serde(rename = "serverName")]
    pub server_name: String,
    #[serde(rename = "serverIp")]
    pub server_ip: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub serial: String,
    #[serde(rename = "haType", default)]
    pub ha_type: i32,
    #[serde(rename = "serverCode", default)]
    pub server_code: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Wwn {
    pub wwn: String,
    #[serde(rename = "blockSize", default)]
    pub block_size: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskInfoItem {
    #[serde(rename = "storageId")]
    pub storage_id: String,
    #[serde(rename = "storageType", default)]
    pub storage_type: i32,
    #[serde(default)]
    pub slot: i32,
    #[serde(rename = "enclosureIndex", default)]
    pub enclosure_index: i32,
    #[serde(rename = "storageStatus")]
    pub storage_status: i32,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: i64,
    #[serde(default = "default_usage")]
    pub usage: i32,
    #[serde(rename = "deviceName", default)]
    pub device_name: String,
    #[serde(rename = "worldWideNameList", default)]
    pub world_wide_name_list: Vec<Wwn>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowsPartitionItem {
    #[serde(rename = "partitionSeq")]
    pub partition_seq: i32,
    #[serde(rename = "partitionGUID")]
    pub partition_guid: String,
    #[serde(rename = "partitionOffset", default)]
    pub partition_offset: String,
    #[serde(default)]
    pub capacity: f64,
    #[serde(rename = "partitionStatus", default)]
    pub partition_status: i32,
    #[serde(default = "default_usage")]
    pub usage: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowsDiskItem {
    #[serde(rename = "diskId")]
    pub disk_id: String,
    #[serde(rename = "diskNumber", default)]
    pub disk_number: i32,
    #[serde(rename = "diskName", default)]
    pub disk_name: String,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: f64,
    #[serde(rename = "partitionList", default)]
    pub partition_list: Vec<WindowsPartitionItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpsanItem {
    #[serde(rename = "IPSANId")]
    pub ipsan_id: String,
    #[serde(rename = "IPSANName", default)]
    pub ipsan_name: String,
    #[serde(rename = "IPSANType", default)]
    pub ipsan_type: i32,
    #[serde(rename = "IPSANIp", default)]
    pub ipsan_ip: String,
    #[serde(rename = "IPSANStatus", default)]
    pub ipsan_status: i32,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: f64,
    #[serde(default = "default_usage")]
    pub usage: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpsanResourceGroupMemberItem {
    #[serde(rename = "IPSANId")]
    pub ipsan_id: String,
    #[serde(rename = "IPSANName", default)]
    pub ipsan_name: String,
    #[serde(rename = "IPSANIp", default)]
    pub ipsan_ip: String,
    #[serde(rename = "IPSANStatus", default)]
    pub ipsan_status: i32,
    #[serde(default)]
    pub capacity: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpsanResourceGroupItem {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "groupName", default)]
    pub group_name: String,
    #[serde(rename = "groupStatus", default)]
    pub group_status: i32,
    #[serde(rename = "totalCapacity", default)]
    pub total_capacity: f64,
    #[serde(default = "default_usage")]
    pub usage: i32,
    #[serde(rename = "resourceInfoList", default)]
    pub resource_info_list: Vec<IpsanResourceGroupMemberItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheKeyCheckResult {
    pub present_keys: Vec<String>,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheKeyDeleteResult {
    pub deleted_count: i64,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheKeyContentEntry {
    pub key: String,
    pub value_type: String,
    pub preview: String,
    pub full_value: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheKeyContentResult {
    pub entries: Vec<CacheKeyContentEntry>,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheCheckResult {
    pub present_ids: Vec<String>,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CacheDeleteResult {
    pub deleted_count: i64,
    pub redis_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    code: i32,
    message: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct MainlineStatusEnvelope {
    #[serde(rename = "ErrCode")]
    err_code: i32,
    #[serde(rename = "ErrMsg")]
    err_msg: Option<String>,
    #[serde(rename = "Status", default)]
    status: Vec<MainlineStatusItem>,
}

#[derive(Debug, Deserialize)]
struct MainlineStatusItem {
    #[serde(rename = "HostName", default)]
    host_name: String,
    #[serde(rename = "IP", default)]
    ip: String,
    #[serde(rename = "Role", default)]
    role: String,
    #[serde(rename = "Serial", default)]
    serial: String,
    #[serde(rename = "Status", default)]
    status: i32,
}

#[derive(Debug, Deserialize)]
struct ServerListData {
    #[serde(rename = "serverList", default)]
    server_list: Vec<DiskServerItem>,
}

#[derive(Debug, Deserialize)]
struct DiskListData {
    #[serde(rename = "storageInfoList", default)]
    storage_info_list: Vec<DiskInfoItem>,
}

#[derive(Debug, Deserialize)]
struct WindowsRawDiskListData {
    #[serde(rename = "diskInfoList", default)]
    disk_info_list: Vec<WindowsDiskItem>,
}

#[derive(Debug, Deserialize)]
struct IpsanListData {
    #[serde(rename = "IPSANInfoList", default)]
    ipsan_info_list: Vec<IpsanItem>,
}

#[derive(Debug, Deserialize)]
struct IpsanResourceGroupListData {
    #[serde(rename = "groupInfoList", default)]
    group_info_list: Vec<IpsanResourceGroupItem>,
}

fn default_usage() -> i32 {
    -1
}

fn normalize_host(host: &str) -> Result<String, String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("接入 IP 不能为空".to_string());
    }
    Ok(trimmed.to_string())
}

fn normalize_storage_ids(storage_ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw_id in storage_ids {
        let storage_id = raw_id.trim();
        if storage_id.is_empty() || seen.contains(storage_id) {
            continue;
        }
        seen.insert(storage_id.to_string());
        normalized.push(storage_id.to_string());
    }

    normalized
}

fn normalize_cache_keys(keys: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw_key in keys {
        let key = raw_key.trim();
        if key.is_empty() || seen.contains(key) {
            continue;
        }
        if !key.starts_with(STORAGE_KEY_PREFIX) {
            return Err(format!(
                "Redis key 必须以 {} 开头: {}",
                STORAGE_KEY_PREFIX, key
            ));
        }
        seen.insert(key.to_string());
        normalized.push(key.to_string());
    }

    Ok(normalized)
}

fn legacy_storage_ids_to_cache_keys(storage_ids: Vec<String>) -> Vec<String> {
    normalize_storage_ids(storage_ids)
        .into_iter()
        .map(|storage_id| build_storage_key(&storage_id))
        .collect()
}

fn legacy_present_keys_to_storage_ids(present_keys: Vec<String>) -> Vec<String> {
    present_keys
        .into_iter()
        .filter_map(|key| {
            key.strip_prefix(STORAGE_KEY_PREFIX)
                .map(|id| id.to_string())
        })
        .collect()
}

fn build_disk_cleanup_url(host: &str, path: &str) -> String {
    format!("http://{}:23011{}", host, path)
}

fn build_mainline_url(host: &str, path: &str) -> String {
    format!("http://{}{}", host, path)
}

fn strip_ip_port(ip: &str) -> String {
    let trimmed = ip.trim();
    match trimmed.split_once(':') {
        Some((host, _port)) => host.trim().to_string(),
        None => trimmed.to_string(),
    }
}

fn convert_mainline_status_to_server_item(item: MainlineStatusItem) -> DiskServerItem {
    let server_ip = strip_ip_port(&item.ip);
    DiskServerItem {
        server_name: item.host_name,
        server_ip,
        role: item.role,
        serial: item.serial,
        ha_type: 0,
        server_code: item.status,
    }
}

fn parse_mainline_status_payload(
    status: reqwest::StatusCode,
    response_text: &str,
) -> Result<Vec<DiskServerItem>, String> {
    let trimmed_text = response_text.trim();

    if !status.is_success() {
        return Err(if trimmed_text.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {}", status.as_u16(), trimmed_text)
        });
    }

    if trimmed_text.is_empty() {
        return Err("主线接口返回空响应".to_string());
    }

    let parsed = serde_json::from_str::<MainlineStatusEnvelope>(trimmed_text)
        .map_err(|e| format!("主线接口响应解析失败: {}", e))?;

    if parsed.err_code != 0 {
        return Err(parsed
            .err_msg
            .filter(|msg| !msg.is_empty())
            .unwrap_or_else(|| format!("主线接口返回错误码 {}", parsed.err_code)));
    }

    Ok(parsed
        .status
        .into_iter()
        .map(convert_mainline_status_to_server_item)
        .collect())
}

fn build_storage_key(storage_id: &str) -> String {
    format!("{}{}", STORAGE_KEY_PREFIX, storage_id)
}

fn normalize_cache_value_text(value: &str) -> String {
    value.replace('\0', "\\0")
}

fn summarize_cache_value_preview(value: &str, max_chars: usize) -> String {
    let normalized = normalize_cache_value_text(value);
    let mut chars = normalized.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();

    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        normalized
    }
}

fn serialize_hash_entries(entries: Vec<(String, String)>) -> String {
    entries
        .into_iter()
        .map(|(field, value)| format!("{field}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn serialize_list_entries(entries: Vec<String>) -> String {
    entries.into_iter().collect::<Vec<_>>().join("\n")
}

fn serialize_set_entries(mut entries: Vec<String>) -> String {
    entries.sort();
    serialize_list_entries(entries)
}

fn serialize_zset_entries(entries: Vec<(String, f64)>) -> String {
    entries
        .into_iter()
        .map(|(member, score)| format!("{member} ({score})"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_http_client(timeout_secs: u32) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(Duration::from_secs(timeout_secs.max(1) as u64))
        .build()
        .map_err(|e| format!("创建设备 HTTP 客户端失败: {}", e))
}

fn parse_api_payload<T: DeserializeOwned>(
    status: reqwest::StatusCode,
    response_text: &str,
) -> Result<T, String> {
    let trimmed_text = response_text.trim();

    if !status.is_success() {
        return Err(if trimmed_text.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {}", status.as_u16(), trimmed_text)
        });
    }

    if trimmed_text.is_empty() {
        return Err("接口返回空响应".to_string());
    }

    let parsed = serde_json::from_str::<ApiEnvelope<T>>(trimmed_text)
        .map_err(|e| format!("接口响应解析失败: {}", e))?;

    if parsed.code != 0 {
        return Err(parsed
            .message
            .unwrap_or_else(|| format!("接口返回错误码 {}", parsed.code)));
    }

    parsed.data.ok_or_else(|| "接口返回缺少 data".to_string())
}

async fn post_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: serde_json::Value,
) -> Result<T, String> {
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("读取响应体失败: {}", e))?;
    parse_api_payload(status, &response_text)
}

fn classify_redis_connection_error(message: &str) -> String {
    let lowered = message.to_ascii_lowercase();

    if lowered.contains("auth") || lowered.contains("wrongpass") || lowered.contains("noauth") {
        format!("Redis 认证失败: {}", message)
    } else if lowered.contains("protocol") {
        format!("Redis 协议错误: {}", message)
    } else {
        format!("Redis 连接失败: {}", message)
    }
}

#[cfg(test)]
mod cache_preview_tests {
    use super::{normalize_cache_value_text, summarize_cache_value_preview};

    #[test]
    fn summarize_cache_value_preview_truncates_long_content_and_marks_it() {
        let preview = summarize_cache_value_preview(&"x".repeat(260), 32);

        assert!(preview.starts_with("xxxxxxxx"));
        assert!(preview.ends_with("..."));
        assert!(preview.len() <= 35);
    }

    #[test]
    fn summarize_cache_value_preview_keeps_short_content_intact() {
        assert_eq!(
            summarize_cache_value_preview("{\"slot\":7}", 32),
            "{\"slot\":7}"
        );
    }

    #[test]
    fn normalize_cache_value_text_escapes_null_bytes() {
        assert_eq!(normalize_cache_value_text("ab\0cd"), "ab\\0cd");
    }
}

async fn connect_redis(host: &str) -> Result<MultiplexedConnection, String> {
    let url = format!("redis://:{}@{}:{}/", REDIS_PASSWORD, host, REDIS_PORT);
    let client = redis::Client::open(url).map_err(|e| format!("Redis URL 无效: {}", e))?;
    tokio::time::timeout(REDIS_OP_TIMEOUT, client.get_multiplexed_async_connection())
        .await
        .map_err(|_| "Redis 连接超时".to_string())?
        .map_err(|e| classify_redis_connection_error(&e.to_string()))
}

async fn execute_redis_command<T: redis::FromRedisValue>(
    conn: &mut MultiplexedConnection,
    op_name: &str,
    command: &mut redis::Cmd,
) -> Result<T, String> {
    tokio::time::timeout(REDIS_OP_TIMEOUT, command.query_async::<_, T>(conn))
        .await
        .map_err(|_| format!("Redis {} timed out", op_name))?
        .map_err(|error| format!("Redis {} failed: {}", op_name, error))
}

async fn load_cache_key_content(
    conn: &mut MultiplexedConnection,
    key: &str,
) -> Result<CacheKeyContentEntry, String> {
    let mut type_cmd = redis::cmd("TYPE");
    type_cmd.arg(key);
    let value_type =
        execute_redis_command::<String>(conn, &format!("TYPE {}", key), &mut type_cmd).await?;

    let raw_value = match value_type.as_str() {
        "string" => {
            let mut get_cmd = redis::cmd("GET");
            get_cmd.arg(key);
            execute_redis_command::<Option<String>>(conn, &format!("GET {}", key), &mut get_cmd)
                .await?
                .unwrap_or_default()
        }
        "hash" => {
            let mut hgetall_cmd = redis::cmd("HGETALL");
            hgetall_cmd.arg(key);
            let pairs = execute_redis_command::<Vec<(String, String)>>(
                conn,
                &format!("HGETALL {}", key),
                &mut hgetall_cmd,
            )
            .await?;
            serialize_hash_entries(pairs)
        }
        "list" => {
            let mut lrange_cmd = redis::cmd("LRANGE");
            lrange_cmd.arg(key).arg(0).arg(-1);
            let values = execute_redis_command::<Vec<String>>(
                conn,
                &format!("LRANGE {}", key),
                &mut lrange_cmd,
            )
            .await?;
            serialize_list_entries(values)
        }
        "set" => {
            let mut smembers_cmd = redis::cmd("SMEMBERS");
            smembers_cmd.arg(key);
            let values = execute_redis_command::<Vec<String>>(
                conn,
                &format!("SMEMBERS {}", key),
                &mut smembers_cmd,
            )
            .await?;
            serialize_set_entries(values)
        }
        "zset" => {
            let mut zrange_cmd = redis::cmd("ZRANGE");
            zrange_cmd.arg(key).arg(0).arg(-1).arg("WITHSCORES");
            let values = execute_redis_command::<Vec<(String, f64)>>(
                conn,
                &format!("ZRANGE {}", key),
                &mut zrange_cmd,
            )
            .await?;
            serialize_zset_entries(values)
        }
        "none" => String::new(),
        other => format!("<{} preview unavailable>", other),
    };

    let full_value = normalize_cache_value_text(&raw_value);

    let truncated = full_value.chars().count() > CACHE_PREVIEW_MAX_CHARS;

    Ok(CacheKeyContentEntry {
        key: key.to_string(),
        value_type,
        preview: summarize_cache_value_preview(&raw_value, CACHE_PREVIEW_MAX_CHARS),
        full_value,
        truncated,
    })
}

#[tauri::command]
pub async fn disk_cleanup_list_linux_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, DISK_SERVER_LIST_PATH);
    let data: ServerListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.server_list)
}

#[tauri::command]
pub async fn disk_cleanup_list_mainline_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_mainline_url(&host, MAINLINE_STATUS_PATH);
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| format!("主线接口请求失败: {}", e))?;
    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("读取主线响应体失败: {}", e))?;
    parse_mainline_status_payload(status, &response_text)
}

#[tauri::command]
pub async fn disk_cleanup_list_linux_disks(
    host: String,
    server_ip: String,
    timeout_secs: u32,
) -> Result<Vec<DiskInfoItem>, String> {
    let host = normalize_host(&host)?;
    let server_ip = server_ip.trim().to_string();
    if server_ip.is_empty() {
        return Err("请选择子机 IP".to_string());
    }

    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, DISK_LIST_PATH);
    let data: DiskListData =
        post_json(&client, &url, serde_json::json!({ "serverIp": server_ip })).await?;
    Ok(data.storage_info_list)
}

#[tauri::command]
pub async fn disk_cleanup_list_servers(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<DiskServerItem>, String> {
    disk_cleanup_list_linux_servers(host, timeout_secs).await
}

#[tauri::command]
pub async fn disk_cleanup_list_disks(
    host: String,
    server_ip: String,
    timeout_secs: u32,
) -> Result<Vec<DiskInfoItem>, String> {
    disk_cleanup_list_linux_disks(host, server_ip, timeout_secs).await
}

#[tauri::command]
pub async fn disk_cleanup_list_windows_disks(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<WindowsDiskItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, RAW_DISK_LIST_PATH);
    let data: WindowsRawDiskListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.disk_info_list)
}

#[tauri::command]
pub async fn disk_cleanup_list_ipsans(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<IpsanItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, IPSAN_LIST_PATH);
    let data: IpsanListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.ipsan_info_list)
}

#[tauri::command]
pub async fn disk_cleanup_list_ipsan_resource_groups(
    host: String,
    timeout_secs: u32,
) -> Result<Vec<IpsanResourceGroupItem>, String> {
    let host = normalize_host(&host)?;
    let client = build_http_client(timeout_secs)?;
    let url = build_disk_cleanup_url(&host, IPSAN_RESOURCE_GROUP_LIST_PATH);
    let data: IpsanResourceGroupListData = post_json(&client, &url, serde_json::json!({})).await?;
    Ok(data.group_info_list)
}

#[tauri::command]
pub async fn disk_cleanup_check_cache_keys(host: String, keys: Vec<String>) -> CacheKeyCheckResult {
    let host = match normalize_host(&host) {
        Ok(host) => host,
        Err(error) => {
            return CacheKeyCheckResult {
                present_keys: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let keys = match normalize_cache_keys(keys) {
        Ok(keys) => keys,
        Err(error) => {
            return CacheKeyCheckResult {
                present_keys: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    if keys.is_empty() {
        return CacheKeyCheckResult {
            present_keys: vec![],
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(conn) => conn,
        Err(error) => {
            return CacheKeyCheckResult {
                present_keys: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let mut pipe = redis::pipe();
    for key in &keys {
        pipe.cmd("EXISTS").arg(key);
    }

    let exec =
        tokio::time::timeout(REDIS_OP_TIMEOUT, pipe.query_async::<_, Vec<i64>>(&mut conn)).await;

    match exec {
        Err(_) => CacheKeyCheckResult {
            present_keys: vec![],
            redis_available: false,
            error: Some("Redis 查询超时".to_string()),
        },
        Ok(Err(error)) => CacheKeyCheckResult {
            present_keys: vec![],
            redis_available: false,
            error: Some(format!("Redis EXISTS 失败: {}", error)),
        },
        Ok(Ok(flags)) => {
            let present_keys = keys
                .into_iter()
                .zip(flags)
                .filter_map(|(key, flag)| if flag == 1 { Some(key) } else { None })
                .collect();
            CacheKeyCheckResult {
                present_keys,
                redis_available: true,
                error: None,
            }
        }
    }
}

#[tauri::command]
pub async fn disk_cleanup_get_cache_key_contents(
    host: String,
    keys: Vec<String>,
) -> CacheKeyContentResult {
    let host = match normalize_host(&host) {
        Ok(host) => host,
        Err(error) => {
            return CacheKeyContentResult {
                entries: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let keys = match normalize_cache_keys(keys) {
        Ok(keys) => keys,
        Err(error) => {
            return CacheKeyContentResult {
                entries: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    if keys.is_empty() {
        return CacheKeyContentResult {
            entries: vec![],
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(conn) => conn,
        Err(error) => {
            return CacheKeyContentResult {
                entries: vec![],
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let mut entries = Vec::with_capacity(keys.len());
    for key in keys {
        match load_cache_key_content(&mut conn, &key).await {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                return CacheKeyContentResult {
                    entries: vec![],
                    redis_available: false,
                    error: Some(error),
                };
            }
        }
    }

    CacheKeyContentResult {
        entries,
        redis_available: true,
        error: None,
    }
}

#[tauri::command]
pub async fn disk_cleanup_delete_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyDeleteResult {
    let host = match normalize_host(&host) {
        Ok(host) => host,
        Err(error) => {
            return CacheKeyDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let keys = match normalize_cache_keys(keys) {
        Ok(keys) => keys,
        Err(error) => {
            return CacheKeyDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(error),
            };
        }
    };

    if keys.is_empty() {
        return CacheKeyDeleteResult {
            deleted_count: 0,
            redis_available: true,
            error: None,
        };
    }

    let mut conn = match connect_redis(&host).await {
        Ok(conn) => conn,
        Err(error) => {
            return CacheKeyDeleteResult {
                deleted_count: 0,
                redis_available: false,
                error: Some(error),
            };
        }
    };

    let exec = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        redis::cmd("DEL")
            .arg(&keys)
            .query_async::<_, i64>(&mut conn),
    )
    .await;

    match exec {
        Err(_) => CacheKeyDeleteResult {
            deleted_count: 0,
            redis_available: false,
            error: Some("Redis 删除超时".to_string()),
        },
        Ok(Err(error)) => CacheKeyDeleteResult {
            deleted_count: 0,
            redis_available: false,
            error: Some(format!("Redis DEL 失败: {}", error)),
        },
        Ok(Ok(deleted_count)) => CacheKeyDeleteResult {
            deleted_count,
            redis_available: true,
            error: None,
        },
    }
}

#[tauri::command]
pub async fn disk_cleanup_check_redis(host: String, storage_ids: Vec<String>) -> CacheCheckResult {
    let keys = legacy_storage_ids_to_cache_keys(storage_ids);
    let result = disk_cleanup_check_cache_keys(host, keys).await;

    CacheCheckResult {
        present_ids: legacy_present_keys_to_storage_ids(result.present_keys),
        redis_available: result.redis_available,
        error: result.error,
    }
}

#[tauri::command]
pub async fn disk_cleanup_delete_cache(
    host: String,
    storage_ids: Vec<String>,
) -> CacheDeleteResult {
    let keys = legacy_storage_ids_to_cache_keys(storage_ids);
    let result = disk_cleanup_delete_cache_keys(host, keys).await;

    CacheDeleteResult {
        deleted_count: result.deleted_count,
        redis_available: result.redis_available,
        error: result.error,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_disk_cleanup_url, build_mainline_url, build_storage_key,
        legacy_present_keys_to_storage_ids, legacy_storage_ids_to_cache_keys, normalize_cache_keys,
        normalize_storage_ids, parse_api_payload, parse_mainline_status_payload, strip_ip_port,
        DiskListData, IpsanListData, IpsanResourceGroupListData, WindowsRawDiskListData,
        DISK_LIST_PATH, MAINLINE_STATUS_PATH,
    };
    use reqwest::StatusCode;

    #[test]
    fn build_disk_cleanup_url_uses_expected_port_and_path() {
        assert_eq!(
            build_disk_cleanup_url("10.20.30.40", DISK_LIST_PATH),
            "http://10.20.30.40:23011/openAPI/system/v1/disk/list"
        );
    }

    #[test]
    fn build_storage_key_prefixes_storage_id() {
        assert_eq!(build_storage_key("abc-123"), "Storage:abc-123");
    }

    #[test]
    fn normalize_storage_ids_trims_dedupes_and_drops_empty_items() {
        assert_eq!(
            normalize_storage_ids(vec![
                "  ".to_string(),
                "disk-a".to_string(),
                " disk-b ".to_string(),
                "disk-a".to_string(),
                "".to_string(),
                "disk-b".to_string(),
            ]),
            vec!["disk-a".to_string(), "disk-b".to_string()]
        );
    }

    #[test]
    fn normalize_cache_keys_trims_dedupes_and_keeps_storage_prefix() {
        let keys = normalize_cache_keys(vec![
            " Storage:disk-a ".to_string(),
            "Storage:disk-a".to_string(),
            "Storage:disk-b".to_string(),
        ])
        .unwrap();

        assert_eq!(
            keys,
            vec!["Storage:disk-a".to_string(), "Storage:disk-b".to_string()]
        );
    }

    #[test]
    fn normalize_cache_keys_rejects_non_storage_prefix() {
        let error = normalize_cache_keys(vec!["Partition:{foo}".to_string()]).unwrap_err();
        assert!(error.contains("Storage:"));
    }

    #[test]
    fn legacy_storage_ids_to_cache_keys_trims_dedupes_and_prefixes() {
        assert_eq!(
            legacy_storage_ids_to_cache_keys(vec![
                "  ".to_string(),
                "disk-a".to_string(),
                " disk-b ".to_string(),
                "disk-a".to_string(),
            ]),
            vec!["Storage:disk-a".to_string(), "Storage:disk-b".to_string()]
        );
    }

    #[test]
    fn legacy_present_keys_to_storage_ids_strips_prefix_and_drops_invalid_keys() {
        assert_eq!(
            legacy_present_keys_to_storage_ids(vec![
                "Storage:disk-a".to_string(),
                "Partition:disk-b".to_string(),
                "Storage:disk-c".to_string(),
            ]),
            vec!["disk-a".to_string(), "disk-c".to_string()]
        );
    }

    #[test]
    fn parse_api_payload_returns_data_on_success() {
        let body = r#"{
            "code": 0,
            "message": "ok",
            "data": {
                "storageInfoList": [
                    {
                        "storageId": "disk-a",
                        "storageStatus": 1
                    }
                ]
            }
        }"#;

        let parsed = parse_api_payload::<DiskListData>(StatusCode::OK, body).unwrap();
        assert_eq!(parsed.storage_info_list.len(), 1);
        assert_eq!(parsed.storage_info_list[0].storage_id, "disk-a");
        assert_eq!(parsed.storage_info_list[0].usage, -1);
    }

    #[test]
    fn parse_raw_disk_payload_returns_partition_list() {
        let body = r#"{
            "code": 0,
            "message": "Success",
            "data": {
                "diskInfoList": [
                    {
                        "diskId": "302375165793144832",
                        "diskNumber": 6,
                        "diskName": "ST4000VX000-2AG166",
                        "totalCapacity": 3726.02,
                        "partitionList": [
                            {
                                "partitionSeq": 1,
                                "partitionGUID": "{6042cce1-3fa4-45a4-998d-57d44d6f8da1}",
                                "capacity": 976.56,
                                "partitionStatus": 1,
                                "usage": -1
                            }
                        ]
                    }
                ]
            }
        }"#;

        let parsed = parse_api_payload::<WindowsRawDiskListData>(StatusCode::OK, body).unwrap();
        assert_eq!(
            parsed.disk_info_list[0].partition_list[0].partition_guid,
            "{6042cce1-3fa4-45a4-998d-57d44d6f8da1}"
        );
    }

    #[test]
    fn parse_ipsan_payload_returns_usage_field() {
        let body = r#"{
            "code": 0,
            "message": "Success",
            "data": {
                "IPSANInfoList": [
                    {
                        "IPSANId": "436856425541537792",
                        "IPSANName": "192.115.2.29",
                        "IPSANIp": "192.115.2.29",
                        "IPSANStatus": 1,
                        "totalCapacity": 600,
                        "usage": 5
                    }
                ]
            }
        }"#;

        let parsed = parse_api_payload::<IpsanListData>(StatusCode::OK, body).unwrap();
        assert_eq!(parsed.ipsan_info_list[0].usage, 5);
    }

    #[test]
    fn parse_ipsan_resource_group_payload_returns_members() {
        let body = r#"{
            "code": 0,
            "message": "Success",
            "data": {
                "groupInfoList": [
                    {
                        "groupId": "439245456753561600",
                        "groupName": "192.115.2.26",
                        "groupStatus": 1,
                        "totalCapacity": 1296,
                        "usage": 2,
                        "resourceInfoList": [
                            {
                                "IPSANId": "438596966545362944",
                                "IPSANName": "192.115.2.26",
                                "IPSANIp": "192.115.2.26",
                                "IPSANStatus": 1,
                                "capacity": 648
                            }
                        ]
                    }
                ]
            }
        }"#;

        let parsed =
            parse_api_payload::<IpsanResourceGroupListData>(StatusCode::OK, body).unwrap();
        assert_eq!(parsed.group_info_list.len(), 1);
        assert_eq!(parsed.group_info_list[0].resource_info_list.len(), 1);
        assert_eq!(parsed.group_info_list[0].usage, 2);
        assert_eq!(
            parsed.group_info_list[0].resource_info_list[0].ipsan_id,
            "438596966545362944"
        );
    }

    #[test]
    fn parse_api_payload_rejects_non_zero_code() {
        let body = r#"{
            "code": 5001,
            "message": "device busy",
            "data": null
        }"#;

        let error = parse_api_payload::<DiskListData>(StatusCode::OK, body).unwrap_err();
        assert_eq!(error, "device busy");
    }

    #[test]
    fn parse_api_payload_uses_original_fallback_error_text_when_message_missing() {
        let body = r#"{
            "code": 5001,
            "data": null
        }"#;

        let error = parse_api_payload::<DiskListData>(StatusCode::OK, body).unwrap_err();
        assert_eq!(error, "接口返回错误码 5001");
    }

    #[test]
    fn build_mainline_url_omits_port_and_uses_distapi_path() {
        assert_eq!(
            build_mainline_url("192.115.1.55", MAINLINE_STATUS_PATH),
            "http://192.115.1.55/distapi/status"
        );
    }

    #[test]
    fn strip_ip_port_removes_trailing_port() {
        assert_eq!(strip_ip_port("192.115.1.157:21003"), "192.115.1.157");
        assert_eq!(strip_ip_port("192.115.1.55"), "192.115.1.55");
        assert_eq!(strip_ip_port("  192.115.1.55  "), "192.115.1.55");
    }

    #[test]
    fn parse_mainline_status_payload_maps_to_server_items_and_strips_replica_port() {
        let body = r#"{
            "ErrCode": 0,
            "ErrMsg": "Succeed",
            "Status": [
                {
                    "Status": 1,
                    "HostName": "VMS-U500-H16",
                    "IP": "192.115.1.55",
                    "Role": "primary",
                    "Serial": "210235C8R8324B000006"
                },
                {
                    "Status": 1,
                    "HostName": "VMS-U500-H16-Replica",
                    "IP": "192.115.1.157:21003",
                    "Role": "replica",
                    "Serial": "210235C8X60000000001"
                }
            ]
        }"#;

        let parsed = parse_mainline_status_payload(StatusCode::OK, body).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].server_ip, "192.115.1.55");
        assert_eq!(parsed[0].server_name, "VMS-U500-H16");
        assert_eq!(parsed[0].role, "primary");
        assert_eq!(parsed[0].server_code, 1);
        assert_eq!(parsed[1].server_ip, "192.115.1.157");
        assert_eq!(parsed[1].role, "replica");
        assert_eq!(parsed[1].serial, "210235C8X60000000001");
    }

    #[test]
    fn parse_mainline_status_payload_rejects_non_zero_errcode() {
        let body = r#"{
            "ErrCode": 5001,
            "ErrMsg": "device busy",
            "Status": []
        }"#;

        let error = parse_mainline_status_payload(StatusCode::OK, body).unwrap_err();
        assert_eq!(error, "device busy");
    }

    #[test]
    fn parse_mainline_status_payload_falls_back_when_errmsg_missing() {
        let body = r#"{
            "ErrCode": 5001,
            "Status": []
        }"#;

        let error = parse_mainline_status_payload(StatusCode::OK, body).unwrap_err();
        assert_eq!(error, "主线接口返回错误码 5001");
    }

    #[test]
    fn parse_mainline_status_payload_returns_empty_list_when_status_missing() {
        let body = r#"{
            "ErrCode": 0,
            "ErrMsg": "Succeed"
        }"#;

        let parsed = parse_mainline_status_payload(StatusCode::OK, body).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_api_payload_requires_data() {
        let body = r#"{
            "code": 0,
            "message": "ok"
        }"#;

        let error = parse_api_payload::<DiskListData>(StatusCode::OK, body).unwrap_err();
        assert_eq!(error, "接口返回缺少 data");
    }
}
