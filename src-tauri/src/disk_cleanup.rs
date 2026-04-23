use redis::aio::MultiplexedConnection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

const DISK_SERVER_LIST_PATH: &str = "/openAPI/system/v1/disk/server/list";
const DISK_LIST_PATH: &str = "/openAPI/system/v1/disk/list";
const RAW_DISK_LIST_PATH: &str = "/openAPI/system/v1/raw-disk/list";
const IPSAN_LIST_PATH: &str = "/openAPI/system/v1/IPSAN/list";
const REDIS_PORT: u16 = 6379;
const REDIS_PASSWORD: &str = "ums@redis_service";
const REDIS_OP_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const STORAGE_KEY_PREFIX: &str = "Storage:";

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

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    code: i32,
    message: Option<String>,
    data: Option<T>,
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

fn build_disk_cleanup_url(host: &str, path: &str) -> String {
    format!("http://{}:23011{}", host, path)
}

fn build_storage_key(storage_id: &str) -> String {
    format!("{}{}", STORAGE_KEY_PREFIX, storage_id)
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
        return Err(
            parsed
                .message
                .unwrap_or_else(|| format!("接口返回错误码 {}", parsed.code)),
        );
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

async fn connect_redis(host: &str) -> Result<MultiplexedConnection, String> {
    let url = format!("redis://:{}@{}:{}/", REDIS_PASSWORD, host, REDIS_PORT);
    let client = redis::Client::open(url).map_err(|e| format!("Redis URL 无效: {}", e))?;
    tokio::time::timeout(REDIS_OP_TIMEOUT, client.get_multiplexed_async_connection())
        .await
        .map_err(|_| "Redis 连接超时".to_string())?
        .map_err(|e| classify_redis_connection_error(&e.to_string()))
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
    let data: DiskListData = post_json(
        &client,
        &url,
        serde_json::json!({ "serverIp": server_ip }),
    )
    .await?;
    Ok(data.storage_info_list)
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
pub async fn disk_cleanup_check_cache_keys(
    host: String,
    keys: Vec<String>,
) -> CacheKeyCheckResult {
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

    let exec = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        pipe.query_async::<_, Vec<i64>>(&mut conn),
    )
    .await;

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
        redis::cmd("DEL").arg(&keys).query_async::<_, i64>(&mut conn),
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

#[cfg(test)]
mod tests {
    use super::{
        build_disk_cleanup_url, build_storage_key, normalize_cache_keys, normalize_storage_ids,
        parse_api_payload, DiskListData, IpsanListData, WindowsRawDiskListData, DISK_LIST_PATH,
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
    fn parse_api_payload_requires_data() {
        let body = r#"{
            "code": 0,
            "message": "ok"
        }"#;

        let error = parse_api_payload::<DiskListData>(StatusCode::OK, body).unwrap_err();
        assert_eq!(error, "接口返回缺少 data");
    }
}
