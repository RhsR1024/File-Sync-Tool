//! UMS 初始密码修改 — 框架 / UMS / CDM 三种密码初始化流程。
//!
//! 三条流程打的是完全不同的服务，端口、账号、哈希算法、签名构造、令牌头大小写、
//! HTTP 方法和成功判据都不一样，因此各自独立实现，只共享 HTTP client、结果结构、
//! 并发调度和日志封装。
//!
//! 安全说明：本模块使用 MD5 摘要与 RSA PKCS#1 v1.5 加密，两者按现代标准都属弱算法。
//! 这里使用它们**仅仅**是为了与既有设备接口保持协议兼容，不作为本项目自身的安全原语。

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use md5::Digest as _;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;

const TOOL_NAME: &str = "UMS初始密码修改";

const FRAMEWORK_PORT: u16 = 21900;
const UMS_PORT: u16 = 80;
const CDM_PORT: u16 = 25011;

const FRAMEWORK_USER: &str = "admin";
const UMS_USER: &str = "loadmin";
const CDM_USER: &str = "admin";

/// 响应体写入日志前的截断长度。UMS 的公钥响应约 400 字符，留足余量即可。
const LOG_BODY_LIMIT: usize = 800;

// ─────────────────────────── 请求 / 结果结构 ───────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordTargets {
    pub framework: bool,
    pub ums: bool,
    pub cdm: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordRequest {
    pub ips: Vec<String>,
    pub targets: UmsInitPasswordTargets,
    /// 统一新密码，对被勾选的三种流程共同生效。
    pub new_password: String,
    pub framework_old_password: String,
    pub ums_old_password: String,
    pub cdm_old_password: String,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UmsInitPasswordKind {
    Framework,
    Ums,
    Cdm,
}

impl UmsInitPasswordKind {
    fn label(self) -> &'static str {
        match self {
            Self::Framework => "框架",
            Self::Ums => "UMS",
            Self::Cdm => "CDM",
        }
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordTargetResult {
    pub kind: UmsInitPasswordKind,
    pub success: bool,
    pub message: String,
    /// 失败阶段：login / publicKey / changePasswd / dictionary。
    pub failed_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UmsInitPasswordResult {
    pub ip: String,
    /// 该 IP 上所有被勾选的流程是否都成功。
    pub success: bool,
    pub targets: Vec<UmsInitPasswordTargetResult>,
}

fn ok_result(kind: UmsInitPasswordKind) -> UmsInitPasswordTargetResult {
    UmsInitPasswordTargetResult {
        kind,
        success: true,
        message: "成功".to_string(),
        failed_at: None,
    }
}

fn fail_result(
    kind: UmsInitPasswordKind,
    message: impl Into<String>,
    failed_at: &str,
) -> UmsInitPasswordTargetResult {
    UmsInitPasswordTargetResult {
        kind,
        success: false,
        message: message.into(),
        failed_at: Some(failed_at.to_string()),
    }
}

// ─────────────────────────── 加密工具 ───────────────────────────

/// MD5 摘要，小写十六进制。
fn md5_hex(plaintext: &str) -> String {
    let mut hasher = md5::Md5::new();
    hasher.update(plaintext.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// UMS 登录签名：`MD5( Base64(UserName) + AccessCode + MD5(password) )`。
fn ums_login_signature(user: &str, access_code: &str, password: &str) -> String {
    let payload = format!(
        "{}{}{}",
        BASE64.encode(user.as_bytes()),
        access_code,
        md5_hex(password)
    );
    md5_hex(&payload)
}

/// CDM 登录签名：`MD5(UserName) + AccessCode + MD5(password)`。
///
/// 注意与 UMS 的区别 —— CDM 是纯拼接，**没有**外层 MD5。
fn cdm_login_signature(user: &str, access_code: &str, password: &str) -> String {
    format!("{}{}{}", md5_hex(user), access_code, md5_hex(password))
}

/// 用 base64 编码的 SPKI DER 公钥做 RSA PKCS#1 v1.5 加密，返回 base64 密文。
///
/// PKCS#1 v1.5 填充带随机数，同一明文两次调用必然得到不同密文 —— UMS 的
/// `newUserPasswd` 与 `NewEncPassword` 正是靠这一点分两次加密同一个新密码。
pub(crate) fn rsa_pkcs1v15_encrypt_base64(
    public_key_b64: &str,
    plaintext: &str,
) -> Result<String, String> {
    let der = BASE64
        .decode(public_key_b64.trim())
        .map_err(|e| format!("公钥 base64 解码失败: {}", e))?;
    let key =
        RsaPublicKey::from_public_key_der(&der).map_err(|e| format!("公钥 DER 解析失败: {}", e))?;
    let ciphertext = key
        .encrypt(
            &mut rand::thread_rng(),
            Pkcs1v15Encrypt,
            plaintext.as_bytes(),
        )
        .map_err(|e| format!("RSA 加密失败: {}", e))?;
    Ok(BASE64.encode(ciphertext))
}

// ─────────────────────────── 本机 IP 探测 ───────────────────────────

/// 排除回环、未指定、链路本地，以及 fake-IP 代理 TUN 常用的 198.18.0.0/15。
/// 后者会应答任意目标的路由查询，直接采纳会把错误的源地址填进 UMS 登录体。
fn is_usable_local_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_link_local()
        && !(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
}

fn same_slash24(a: &Ipv4Addr, b: &Ipv4Addr) -> bool {
    let (x, y) = (a.octets(), b.octets());
    x[0] == y[0] && x[1] == y[1] && x[2] == y[2]
}

/// 返回访问 `target` 时本机会使用的源 IPv4。
///
/// 先用 UDP connect 探针让内核做一次路由查询（不发包），再按网卡枚举兜底。
/// 两条路径都会过滤掉伪 IP 段，全部失败时返回 `None`，由调用方填空串。
fn detect_local_ip_for(target: Ipv4Addr) -> Option<String> {
    if let Ok(socket) = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)) {
        if socket.connect(SocketAddrV4::new(target, UMS_PORT)).is_ok() {
            if let Ok(std::net::SocketAddr::V4(local)) = socket.local_addr() {
                if is_usable_local_ipv4(local.ip()) {
                    return Some(local.ip().to_string());
                }
            }
        }
    }

    let candidates: Vec<Ipv4Addr> = local_ip_address::list_afinet_netifas()
        .ok()?
        .into_iter()
        .filter_map(|(_, addr)| match addr {
            std::net::IpAddr::V4(v4) if is_usable_local_ipv4(&v4) => Some(v4),
            _ => None,
        })
        .collect();

    candidates
        .iter()
        .find(|candidate| same_slash24(candidate, &target))
        .or_else(|| candidates.first())
        .map(|ip| ip.to_string())
}

/// Complete the two-request UMS challenge login and return its AccessToken.
///
/// This intentionally emits no logs: callers must never expose the password,
/// signature, or returned token in the simulator's user-facing run log.
pub(crate) async fn ums_acquire_access_token(
    client: &reqwest::Client,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<String, String> {
    let base = format!("http://{}:{}/sw", host.trim(), port);
    let handshake = client
        .post(format!("{base}/login"))
        .send()
        .await
        .map_err(|error| format!("获取 AccessCode 请求失败: {error}"))?;
    let handshake_status = handshake.status();
    let handshake_text = handshake.text().await.unwrap_or_default();
    if !handshake_status.is_success() {
        return Err(format!(
            "获取 AccessCode 返回 HTTP {}",
            handshake_status.as_u16()
        ));
    }
    let handshake_json: serde_json::Value = serde_json::from_str(&handshake_text)
        .map_err(|error| format!("AccessCode 响应不是有效 JSON: {error}"))?;
    if let Some(detail) = extract_error(&handshake_json) {
        return Err(format!("获取 AccessCode 失败: {detail}"));
    }
    let access_code = json_str(&handshake_json, "AccessCode")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "获取 AccessCode 响应缺少 AccessCode".to_string())?;

    let local_ip = host
        .parse::<Ipv4Addr>()
        .ok()
        .and_then(detect_local_ip_for)
        .unwrap_or_default();
    let login_body = json!({
        "UserName": user,
        "AccessCode": access_code,
        "LoginSignature": ums_login_signature(user, access_code, password),
        "isNewVersion": true,
        "ip": host,
        "languageType": "zh_cn",
        "LoginExtInfo": { "IpAddress": local_ip },
        "ClientIp": "",
    });
    let login = client
        .post(format!("{base}/login"))
        .json(&login_body)
        .send()
        .await
        .map_err(|error| format!("UMS 登录请求失败: {error}"))?;
    let login_status = login.status();
    let login_text = login.text().await.unwrap_or_default();
    if !login_status.is_success() {
        return Err(format!("UMS 登录返回 HTTP {}", login_status.as_u16()));
    }
    let login_json: serde_json::Value = serde_json::from_str(&login_text)
        .map_err(|error| format!("UMS 登录响应不是有效 JSON: {error}"))?;
    if let Some(detail) = extract_error(&login_json) {
        let mut message = format!("登录失败: {detail}");
        if let Some(residue) = login_json
            .get("ResidueDegree")
            .and_then(|value| value.as_i64())
        {
            message.push_str(&format!("（剩余尝试次数 {residue}）"));
        }
        if let Some(remain) = login_json
            .get("RemainMinutes")
            .and_then(|value| value.as_i64())
            .filter(|value| *value > 0)
        {
            message.push_str(&format!("（锁定剩余 {remain} 分钟）"));
        }
        return Err(message);
    }
    json_str(&login_json, "AccessToken")
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "UMS 登录响应缺少 AccessToken".to_string())
}

// ─────────────────────────── 日志 ───────────────────────────

fn log_line(app: &tauri::AppHandle, level: &str, message: &str) {
    crate::scanner::emit_tool_log(app, TOOL_NAME, message, level);
}

fn truncate_for_log(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "<空>".to_string();
    }
    if trimmed.chars().count() <= LOG_BODY_LIMIT {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(LOG_BODY_LIMIT).collect();
    format!("{}…(共 {} 字符)", head, trimmed.chars().count())
}

/// 单个流程的日志上下文，负责统一加上 `[IP][流程]` 前缀。
struct FlowLogger<'a> {
    app: &'a tauri::AppHandle,
    ip: &'a str,
    kind: UmsInitPasswordKind,
}

impl<'a> FlowLogger<'a> {
    fn info(&self, message: &str) {
        log_line(
            self.app,
            "info",
            &format!("[{}][{}] {}", self.ip, self.kind.label(), message),
        );
    }

    fn error(&self, message: &str) {
        log_line(
            self.app,
            "error",
            &format!("[{}][{}] {}", self.ip, self.kind.label(), message),
        );
    }

    fn success(&self, message: &str) {
        log_line(
            self.app,
            "success",
            &format!("[{}][{}] {}", self.ip, self.kind.label(), message),
        );
    }

    /// 发送请求并把 URL、请求体、状态码、响应体全部写进执行日志。
    ///
    /// `body` 为 `None` 时发送真空 body（不带 Content-Type），这是三个挑战握手接口
    /// 的要求 —— 声明了 JSON 类型却给空 body 会被部分网关判为 400。
    async fn send(
        &self,
        client: &reqwest::Client,
        method: reqwest::Method,
        url: &str,
        token: Option<&str>,
        token_header: &str,
        body: Option<&serde_json::Value>,
        step: &str,
    ) -> Result<(reqwest::StatusCode, String), String> {
        let mut request = client.request(method.clone(), url);
        if let Some(token) = token {
            request = request.header(token_header, token);
        }
        match body {
            Some(payload) => {
                self.info(&format!(
                    "{} → {} {}  body={}",
                    step,
                    method,
                    url,
                    truncate_for_log(&payload.to_string())
                ));
                request = request.json(payload);
            }
            None => {
                self.info(&format!("{} → {} {}  body=<真空>", step, method, url));
            }
        }

        let started = std::time::Instant::now();
        let response = request.send().await.map_err(|e| {
            let detail = format!("请求失败: {}", e);
            self.error(&format!("{} ✗ {}", step, detail));
            detail
        })?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        self.info(&format!(
            "{} ← HTTP {} ({} ms)  {}",
            step,
            status.as_u16(),
            started.elapsed().as_millis(),
            truncate_for_log(&text)
        ));
        Ok((status, text))
    }
}

/// 把响应文本解析成 JSON，解析失败时带上原文，便于定位返回 HTML 错误页的情况。
fn parse_json(text: &str, step: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str::<serde_json::Value>(text).map_err(|e| {
        format!(
            "{} 响应解析失败: {} (原文: {})",
            step,
            e,
            truncate_for_log(text)
        )
    })
}

fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// 同时读 camelCase 和 PascalCase 两种键名。
///
/// 这些设备在业务接口的响应里用 `errCode`/`errMsg`，但登录失败响应用的是
/// `ErrCode`/`ErrMsg`。只认一种会把服务端真正的失败原因（账号锁定、密码错误）
/// 埋掉，只剩下「缺少 token」这类毫无价值的兜底信息。
fn json_field<'a>(
    value: &'a serde_json::Value,
    camel: &str,
    pascal: &str,
) -> Option<&'a serde_json::Value> {
    value.get(camel).or_else(|| value.get(pascal))
}

/// 若响应携带非 0 的错误码，返回可直接展示的描述。
fn extract_error(value: &serde_json::Value) -> Option<String> {
    let code = json_field(value, "errCode", "ErrCode")?.as_i64()?;
    if code == 0 {
        return None;
    }
    let msg = json_field(value, "errMsg", "ErrMsg")
        .and_then(|v| v.as_str())
        .unwrap_or("未知错误");
    Some(format!("errCode={} {}", code, msg))
}

/// UMS 侧统一的 `{errCode, errMsg}` 判据。
fn check_err_code(value: &serde_json::Value, step: &str) -> Result<(), String> {
    if let Some(detail) = extract_error(value) {
        return Err(format!("{} 返回 {}", step, detail));
    }
    match json_field(value, "errCode", "ErrCode").and_then(|v| v.as_i64()) {
        Some(_) => Ok(()),
        None => Err(format!(
            "{} 响应缺少 errCode 字段: {}",
            step,
            truncate_for_log(&value.to_string())
        )),
    }
}

// ─────────────────────────── 框架流程 ───────────────────────────

/// 框架：端口 21900，SHA-256 明文哈希，`code == 0` 判成功。
async fn run_framework_flow(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    ip: &str,
    old_password: &str,
    new_password: &str,
) -> UmsInitPasswordTargetResult {
    let kind = UmsInitPasswordKind::Framework;
    let logger = FlowLogger { app, ip, kind };
    let hashed_old = crate::sha256_hex(old_password);
    let hashed_new = crate::sha256_hex(new_password);
    logger.info(&format!(
        "开始，账号={} 旧密码SHA256={} 新密码SHA256={}",
        FRAMEWORK_USER, hashed_old, hashed_new
    ));

    let base = format!("http://{}:{}/openAPI/userMgr/v1", ip, FRAMEWORK_PORT);

    // ① 登录
    let login_body = json!({
        "userName": FRAMEWORK_USER,
        "userPasswd": hashed_old,
        "isUnlockLogin": false,
    });
    let step = "① 登录";
    let (_, text) = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/login", base),
            Some(""),
            "Authorization",
            Some(&login_body),
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "login"),
    };

    let token = match parse_json(&text, step).and_then(|value| {
        if value.get("code").and_then(|v| v.as_i64()) != Some(0) {
            let msg = json_str(&value, "message").unwrap_or("未知错误");
            return Err(format!("登录失败: {}", msg));
        }
        value
            .get("data")
            .and_then(|d| d.get("token"))
            .and_then(|t| t.as_str())
            .map(|t| t.to_string())
            .ok_or_else(|| "登录响应缺少 data.token".to_string())
    }) {
        Ok(token) => token,
        Err(e) => {
            logger.error(&format!("{} ✗ {}", step, e));
            return fail_result(kind, e, "login");
        }
    };
    logger.info(&format!("{} ✓ token={}", step, token));

    // ② 修改密码
    let change_body = json!({
        "userName": FRAMEWORK_USER,
        "oldUserPasswd": hashed_old,
        "newUserPasswd": hashed_new,
    });
    let step = "② 修改密码";
    let (_, text) = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/changePasswd", base),
            Some(&token),
            "Authorization",
            Some(&change_body),
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "changePasswd"),
    };

    if let Err(e) = parse_json(&text, step).and_then(|value| {
        if value.get("code").and_then(|v| v.as_i64()) == Some(0) {
            Ok(())
        } else {
            let msg = json_str(&value, "message").unwrap_or("未知错误");
            Err(format!("修改密码失败: {}", msg))
        }
    }) {
        logger.error(&format!("{} ✗ {}", step, e));
        return fail_result(kind, e, "changePasswd");
    }

    // ③ 登出（尽力而为，失败不影响结果）
    let logout_body = json!({
        "userName": FRAMEWORK_USER,
        "userPasswd": hashed_old,
        "token": token,
    });
    let _ = logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/logout", base),
            Some(&token),
            "Authorization",
            Some(&logout_body),
            "③ 登出",
        )
        .await;

    logger.success("完成");
    ok_result(kind)
}

// ─────────────────────────── UMS 流程 ───────────────────────────

/// UMS：端口 80，挑战握手 + MD5 签名 + RSA PKCS#1 v1.5 加密，最后置 `pwdIsInit` 字典开关。
async fn run_ums_flow(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    ip: &str,
    target: Ipv4Addr,
    old_password: &str,
    new_password: &str,
) -> UmsInitPasswordTargetResult {
    let kind = UmsInitPasswordKind::Ums;
    let logger = FlowLogger { app, ip, kind };
    logger.info(&format!("开始，账号={}", UMS_USER));

    let base = format!("http://{}:{}/sw", ip, UMS_PORT);

    // ① 获取 AccessCode
    let step = "① 获取 AccessCode";
    let (_, text) = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/login", base),
            None,
            "Authorization",
            None,
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "login"),
    };

    let access_code = match parse_json(&text, step).and_then(|value| {
        json_str(&value, "AccessCode")
            .map(|c| c.to_string())
            .ok_or_else(|| "握手响应缺少 AccessCode".to_string())
    }) {
        Ok(code) => code,
        Err(e) => {
            logger.error(&format!("{} ✗ {}", step, e));
            return fail_result(kind, e, "login");
        }
    };

    // ② 登录
    let local_ip = detect_local_ip_for(target).unwrap_or_default();
    if local_ip.is_empty() {
        logger.info("② 登录 本机 IP 探测失败，LoginExtInfo.IpAddress 填空串");
    } else {
        logger.info(&format!("② 登录 探测到本机 IP={}", local_ip));
    }
    let signature = ums_login_signature(UMS_USER, &access_code, old_password);
    logger.info(&format!(
        "② 登录 签名 = MD5(Base64(\"{}\")=\"{}\" + AccessCode + MD5(旧密码)=\"{}\") = {}",
        UMS_USER,
        BASE64.encode(UMS_USER.as_bytes()),
        md5_hex(old_password),
        signature
    ));

    let login_body = json!({
        "UserName": UMS_USER,
        "AccessCode": access_code,
        "LoginSignature": signature,
        "isNewVersion": true,
        "ip": ip,
        "languageType": "zh_cn",
        "LoginExtInfo": { "IpAddress": local_ip },
        "ClientIp": "",
    });
    let step = "② 登录";
    let (_, text) = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/login", base),
            None,
            "Authorization",
            Some(&login_body),
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "login"),
    };

    let token = match parse_json(&text, step).and_then(|value| {
        if let Some(detail) = extract_error(&value) {
            // 锁定倒计时只出现在失败响应里，必须原样带出来 ——
            // 这是决定「还能不能再试一次」的唯一依据。
            let mut message = format!("登录失败: {}", detail);
            if let Some(residue) = value.get("ResidueDegree").and_then(|v| v.as_i64()) {
                message.push_str(&format!("（剩余尝试次数 {}）", residue));
            }
            if let Some(remain) = value
                .get("RemainMinutes")
                .and_then(|v| v.as_i64())
                .filter(|value| *value > 0)
            {
                message.push_str(&format!("（锁定剩余 {} 分钟）", remain));
            }
            return Err(message);
        }
        json_str(&value, "AccessToken")
            .map(|t| t.to_string())
            .ok_or_else(|| {
                format!(
                    "登录响应缺少 AccessToken: {}",
                    truncate_for_log(&value.to_string())
                )
            })
    }) {
        Ok(token) => token,
        Err(e) => {
            logger.error(&format!("{} ✗ {}", step, e));
            if e.contains("lock") || e.contains("锁定") {
                logger
                    .error("账号已进入锁定计数，请先用浏览器确认 loadmin 的真实密码，不要继续重试");
            }
            return fail_result(kind, e, "login");
        }
    };
    logger.info(&format!("{} ✓ AccessToken={}", step, token));

    // 新旧密码相同意味着密码已经是目标值，没有什么可改的 —— 但 pwdIsInit 开关
    // 可能还没置位。这种情况下跳过取公钥和改密，直接走 ⑤ 把初始化标识补上。
    let password_change_skipped = old_password == new_password;
    if password_change_skipped {
        logger.info("③④ 跳过：新旧密码相同，密码无需修改，仅补置 pwdIsInit 开关");
    }

    // ③ 取公钥
    if !password_change_skipped {
        let step = "③ 取公钥";
        let (_, text) = match logger
            .send(
                client,
                reqwest::Method::GET,
                &format!("{}/servers/public/key", base),
                Some(&token),
                "Authorization",
                None,
                step,
            )
            .await
        {
            Ok(value) => value,
            Err(e) => return fail_result(kind, e, "publicKey"),
        };

        let public_key = match parse_json(&text, step).and_then(|value| {
            check_err_code(&value, step)?;
            value
                .get("result")
                .and_then(|r| r.get("publicKey"))
                .and_then(|k| k.as_str())
                .map(|k| k.to_string())
                .ok_or_else(|| "公钥响应缺少 result.publicKey".to_string())
        }) {
            Ok(key) => key,
            Err(e) => {
                logger.error(&format!("{} ✗ {}", step, e));
                return fail_result(kind, e, "publicKey");
            }
        };

        // ④ 修改密码
        //
        // RSA 信封里装的是 `MD5(密码)` 的小写十六进制，不是密码原文 —— 服务端存的就是
        // MD5，与登录签名口径一致。实机验证过：原文会被回成 errCode=94438
        // "Usercode or passwd is invalid"（即解密成功但比对失败，说明 PKCS#1 v1.5
        // 填充本身是对的），换成 MD5 后 errCode=0。不要改回原文。
        let step = "④ 修改密码";
        let encrypted = (|| -> Result<(String, String, String), String> {
            let old_digest = md5_hex(old_password);
            let new_digest = md5_hex(new_password);
            Ok((
                rsa_pkcs1v15_encrypt_base64(&public_key, &new_digest)?,
                rsa_pkcs1v15_encrypt_base64(&public_key, &old_digest)?,
                rsa_pkcs1v15_encrypt_base64(&public_key, &new_digest)?,
            ))
        })();
        let (new_enc, old_enc, new_enc_2) = match encrypted {
            Ok(values) => values,
            Err(e) => {
                logger.error(&format!("{} ✗ {}", step, e));
                return fail_result(kind, e, "changePasswd");
            }
        };
        logger.info(&format!(
            "{} 明文 = MD5(密码)，RSA 密文长度 {}/{}/{}",
            step,
            new_enc.len(),
            old_enc.len(),
            new_enc_2.len()
        ));

        let change_body = json!({
            "userCode": UMS_USER,
            "userName": UMS_USER,
            "newUserPasswd": new_enc,
            "userPasswd": old_enc,
            "NewEncPassword": new_enc_2,
        });
        let (_, text) = match logger
            .send(
                client,
                reqwest::Method::PUT,
                &format!("{}/user/update/passwd", base),
                Some(&token),
                "Authorization",
                Some(&change_body),
                step,
            )
            .await
        {
            Ok(value) => value,
            Err(e) => return fail_result(kind, e, "changePasswd"),
        };

        if let Err(e) = parse_json(&text, step).and_then(|value| check_err_code(&value, step)) {
            logger.error(&format!("{} ✗ {}", step, e));
            return fail_result(kind, e, "changePasswd");
        }
    }

    // ⑤ 置密码初始化开关。
    let step = "⑤ 置 pwdIsInit 开关";
    let now_ms = chrono::Utc::now().timestamp_millis();
    let dictionary_body = json!({
        "createTime": 1_716_258_652_000_i64,
        "description": "loadmin密码初始化开关",
        "key": "pwdIsInit",
        "name": "pwdIsInit",
        "updateTime": now_ms,
        "value": "true",
    });
    let dictionary_outcome = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/switch/value/dictionary/set", base),
            Some(&token),
            "Authorization",
            Some(&dictionary_body),
            step,
        )
        .await
    {
        Ok((_, text)) => parse_json(&text, step).and_then(|value| check_err_code(&value, step)),
        Err(e) => Err(e),
    };

    match dictionary_outcome {
        Ok(()) => {
            logger.success("完成");
            if password_change_skipped {
                UmsInitPasswordTargetResult {
                    kind,
                    success: true,
                    message: "新旧密码相同，已跳过改密，仅置 pwdIsInit 开关".to_string(),
                    failed_at: None,
                }
            } else {
                ok_result(kind)
            }
        }
        Err(e) => {
            if password_change_skipped {
                // 开关是本次唯一的操作，它失败就等于整条流程什么都没做成。
                logger.error(&format!("{} ✗ {}", step, e));
                fail_result(kind, e, "dictionary")
            } else {
                // 密码已经改成功了，开关失败不该把整条流程判成失败，
                // 但必须让使用者看到 —— 因此计入成功并在 message 里标注。
                logger.error(&format!("{} ✗ {}（密码已修改成功）", step, e));
                UmsInitPasswordTargetResult {
                    kind,
                    success: true,
                    message: format!("密码已修改，但 pwdIsInit 开关设置失败: {}", e),
                    failed_at: None,
                }
            }
        }
    }
}

// ─────────────────────────── CDM 流程 ───────────────────────────

/// CDM：端口 25011，挑战握手 + MD5 拼接签名，改密用 PUT，响应体为空靠 HTTP 状态码判成功。
async fn run_cdm_flow(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    ip: &str,
    old_password: &str,
    new_password: &str,
) -> UmsInitPasswordTargetResult {
    let kind = UmsInitPasswordKind::Cdm;
    let logger = FlowLogger { app, ip, kind };
    logger.info(&format!("开始，账号={}", CDM_USER));

    let base = format!("http://{}:{}/cdm/civetweb", ip, CDM_PORT);

    // ① 获取 AccessCode
    let step = "① 获取 AccessCode";
    let (_, text) = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/login_v1", base),
            None,
            "authorization",
            None,
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "login"),
    };

    let access_code = match parse_json(&text, step).and_then(|value| {
        json_str(&value, "AccessCode")
            .map(|c| c.to_string())
            .ok_or_else(|| "握手响应缺少 AccessCode".to_string())
    }) {
        Ok(code) => code,
        Err(e) => {
            logger.error(&format!("{} ✗ {}", step, e));
            return fail_result(kind, e, "login");
        }
    };

    // ② 登录
    let signature = cdm_login_signature(CDM_USER, &access_code, old_password);
    logger.info(&format!(
        "② 登录 签名 = MD5(\"{}\")=\"{}\" + AccessCode + MD5(旧密码)=\"{}\"，共 {} 字符",
        CDM_USER,
        md5_hex(CDM_USER),
        md5_hex(old_password),
        signature.len()
    ));

    let login_body = json!({
        "UserName": CDM_USER,
        "AccessCode": access_code,
        "LoginSignature": signature,
    });
    let step = "② 登录";
    let (status, text) = match logger
        .send(
            client,
            reqwest::Method::POST,
            &format!("{}/login_v2", base),
            None,
            "authorization",
            Some(&login_body),
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "login"),
    };

    let token = match parse_json(&text, step).and_then(|value| {
        // CDM 登录失败走 HTTP 4xx + PascalCase 的 {ErrCode, ErrMsg}，
        // 先判错误码再找令牌，否则真实原因会被「缺少 Authorization」盖掉。
        if let Some(detail) = extract_error(&value) {
            return Err(format!("登录失败: {}", detail));
        }
        json_str(&value, "Authorization")
            .map(|t| t.to_string())
            .ok_or_else(|| {
                format!(
                    "登录失败: HTTP {} {}",
                    status.as_u16(),
                    truncate_for_log(&value.to_string())
                )
            })
    }) {
        Ok(token) => token,
        Err(e) => {
            logger.error(&format!("{} ✗ {}", step, e));
            return fail_result(kind, e, "login");
        }
    };
    logger.info(&format!("{} ✓ Authorization={}", step, token));

    // ③ 修改密码。响应体为空，只能靠 HTTP 状态码判成功。
    let step = "③ 修改密码";
    let change_body = json!({
        "UserName": CDM_USER,
        "OldPassword": md5_hex(old_password),
        "NewPassword": md5_hex(new_password),
    });
    let (status, text) = match logger
        .send(
            client,
            reqwest::Method::PUT,
            &format!("{}/passwd", base),
            Some(&token),
            "authorization",
            Some(&change_body),
            step,
        )
        .await
    {
        Ok(value) => value,
        Err(e) => return fail_result(kind, e, "changePasswd"),
    };

    if !status.is_success() {
        let detail = format!(
            "修改密码失败: HTTP {} {}",
            status.as_u16(),
            truncate_for_log(&text)
        );
        logger.error(&format!("{} ✗ {}", step, detail));
        return fail_result(kind, detail, "changePasswd");
    }

    // ④ 登出（尽力而为）
    let _ = logger
        .send(
            client,
            reqwest::Method::DELETE,
            &format!("{}/logout", base),
            Some(&token),
            "authorization",
            None,
            "④ 登出",
        )
        .await;

    logger.success("完成");
    ok_result(kind)
}

// ─────────────────────────── 单 IP 调度 ───────────────────────────

/// 对单个 IP 依次执行被勾选的三种流程。
///
/// 三条流程互相独立 —— 任意一条失败都不得中断其余流程，因此这里不用 `?`，
/// 每条流程都自行把错误收敛成 `UmsInitPasswordTargetResult`。
async fn run_for_ip(
    app: tauri::AppHandle,
    client: reqwest::Client,
    ip_input: String,
    request: UmsInitPasswordRequest,
) -> Option<UmsInitPasswordResult> {
    let ip = ip_input.trim().to_string();
    if ip.is_empty() {
        return None;
    }

    let selected: Vec<UmsInitPasswordKind> = [
        (UmsInitPasswordKind::Framework, request.targets.framework),
        (UmsInitPasswordKind::Ums, request.targets.ums),
        (UmsInitPasswordKind::Cdm, request.targets.cdm),
    ]
    .into_iter()
    .filter_map(|(kind, picked)| picked.then_some(kind))
    .collect();

    if selected.is_empty() {
        return None;
    }

    let parsed = match ip.parse::<Ipv4Addr>() {
        Ok(addr) if crate::validate_ip(&ip) => addr,
        _ => {
            log_line(&app, "error", &format!("[{}] IP 地址格式非法，跳过", ip));
            let targets = selected
                .into_iter()
                .map(|kind| fail_result(kind, format!("IP 地址格式非法: {}", ip), "login"))
                .collect();
            return Some(UmsInitPasswordResult {
                ip,
                success: false,
                targets,
            });
        }
    };

    log_line(
        &app,
        "info",
        &format!(
            "[{}] 开始处理，勾选流程: {}",
            ip,
            selected
                .iter()
                .map(|kind| kind.label())
                .collect::<Vec<_>>()
                .join(" / ")
        ),
    );

    let mut targets = Vec::with_capacity(selected.len());
    for kind in selected {
        let result = match kind {
            UmsInitPasswordKind::Framework => {
                run_framework_flow(
                    &app,
                    &client,
                    &ip,
                    &request.framework_old_password,
                    &request.new_password,
                )
                .await
            }
            UmsInitPasswordKind::Ums => {
                run_ums_flow(
                    &app,
                    &client,
                    &ip,
                    parsed,
                    &request.ums_old_password,
                    &request.new_password,
                )
                .await
            }
            UmsInitPasswordKind::Cdm => {
                run_cdm_flow(
                    &app,
                    &client,
                    &ip,
                    &request.cdm_old_password,
                    &request.new_password,
                )
                .await
            }
        };
        targets.push(result);
    }

    let success = targets.iter().all(|target| target.success);
    log_line(
        &app,
        if success { "success" } else { "error" },
        &format!(
            "[{}] 处理结束: {}",
            ip,
            targets
                .iter()
                .map(|target| format!(
                    "{}={}",
                    target.kind.label(),
                    if target.success { "成功" } else { "失败" }
                ))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );

    Some(UmsInitPasswordResult {
        ip,
        success,
        targets,
    })
}

// ─────────────────────────── Tauri command ───────────────────────────

#[tauri::command]
pub async fn change_ums_init_password(
    request: UmsInitPasswordRequest,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<UmsInitPasswordResult>, String> {
    if !request.targets.framework && !request.targets.ums && !request.targets.cdm {
        return Err("请至少勾选一种密码修改流程".to_string());
    }
    if request.new_password.is_empty() {
        return Err("新密码不能为空".to_string());
    }

    let api_timeout_secs = state
        .config
        .lock()
        .unwrap()
        .framework_password_api_timeout_secs;

    log_line(
        &app_handle,
        "info",
        &format!(
            "任务开始：{} 个目标，勾选 框架={} UMS={} CDM={}，超时 {} 秒，并发 {}",
            request.ips.len(),
            request.targets.framework,
            request.targets.ums,
            request.targets.cdm,
            api_timeout_secs,
            crate::DEVICE_BATCH_CONCURRENCY_LIMIT
        ),
    );

    let client =
        crate::build_device_http_client_with_timeout(Duration::from_secs(api_timeout_secs))?;

    let results = crate::async_utils::run_ordered_with_limit(
        request.ips.clone(),
        crate::DEVICE_BATCH_CONCURRENCY_LIMIT,
        move |ip| {
            let app_handle = app_handle.clone();
            let client = client.clone();
            let request = request.clone();
            async move { run_for_ip(app_handle, client, ip, request).await }
        },
    )
    .await?;

    Ok(results.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 以下三个断言直接锁定协议样例值，是签名算法唯一可靠的回归锚点。
    #[test]
    fn ums_signature_matches_protocol_sample() {
        assert_eq!(BASE64.encode("loadmin".as_bytes()), "bG9hZG1pbg==");
        assert_eq!(
            ums_login_signature("loadmin", "02630335275641340780", "admin_123"),
            "f1416bd8caf9243c25ac05c9cc121a07"
        );
    }

    #[test]
    fn cdm_signature_is_plain_concatenation_without_outer_hash() {
        let signature = cdm_login_signature("admin", "1234567895201785240293599698403", "admin");
        assert_eq!(
            signature,
            "21232f297a57a5a743894a0e4a801fc3\
             1234567895201785240293599698403\
             21232f297a57a5a743894a0e4a801fc3"
        );
        assert_eq!(signature.len(), 32 + 31 + 32);
    }

    #[test]
    fn cdm_new_password_digest_matches_protocol_sample() {
        assert_eq!(md5_hex("admin_123"), "d6bf4bb9a66419380a7e8b034270d381");
        assert_eq!(md5_hex("admin"), "21232f297a57a5a743894a0e4a801fc3");
    }

    const SAMPLE_PUBLIC_KEY: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEArYLdfzYkT7LfPd3szf2rbVxbWH8zwXu03xsjUYXY+ljORBS5iFqstg66pgYZ+k7MgjDDc5YeiGHEcXwM2pFEpNjeZ5hZv68PQNCeAM6EvZwB6EqxGRJAYwgmiF6X3KurEgS39yXQVKjPPBXBRojFy1BiwkoCQov0N2ztbYV+9VOBNWQYzoUNZgV5L8MNf1aBht0p3G4uXMHZDfQgWLFJnLyPZiX+8Gj6KSqPCX657pv7ciL4n68URx7W8YGWODMasLsw2OI0yezUOh0tRG3G0YRdEiY7mJvodfE2knl7hMrurVwcXSKIYXTFtDZyuDnRWRUzzl2SzeBJgtw3n9NEtQIDAQAB";

    #[test]
    fn rsa_encrypt_produces_2048_bit_ciphertext() {
        let ciphertext = rsa_pkcs1v15_encrypt_base64(SAMPLE_PUBLIC_KEY, "admin_123").unwrap();
        assert_eq!(BASE64.decode(&ciphertext).unwrap().len(), 256);
        // 样例里三个字段都是 344 个 base64 字符。
        assert_eq!(ciphertext.len(), 344);
    }

    #[test]
    fn rsa_padding_is_randomized_so_repeat_encryption_differs() {
        // UMS 的 newUserPasswd 与 NewEncPassword 是同一个新密码的两次独立加密，
        // 密文必须不同，否则说明用错了确定性填充。
        let first = rsa_pkcs1v15_encrypt_base64(SAMPLE_PUBLIC_KEY, "admin_123").unwrap();
        let second = rsa_pkcs1v15_encrypt_base64(SAMPLE_PUBLIC_KEY, "admin_123").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn rsa_encrypt_rejects_malformed_public_key() {
        assert!(rsa_pkcs1v15_encrypt_base64("not-base64!!!", "x").is_err());
        assert!(rsa_pkcs1v15_encrypt_base64("aGVsbG8=", "x").is_err());
    }

    #[test]
    fn fake_ip_proxy_and_reserved_ranges_are_not_usable_local_ips() {
        // 开发机上的 fake-IP 代理 TUN 占用 198.18.0.0/15 并应答任意目标。
        assert!(!is_usable_local_ipv4(&Ipv4Addr::new(198, 18, 0, 1)));
        assert!(!is_usable_local_ipv4(&Ipv4Addr::new(198, 19, 255, 254)));
        assert!(!is_usable_local_ipv4(&Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_usable_local_ipv4(&Ipv4Addr::new(169, 254, 3, 4)));
        assert!(!is_usable_local_ipv4(&Ipv4Addr::UNSPECIFIED));
        assert!(is_usable_local_ipv4(&Ipv4Addr::new(192, 115, 1, 15)));
        assert!(is_usable_local_ipv4(&Ipv4Addr::new(198, 17, 0, 1)));
        assert!(is_usable_local_ipv4(&Ipv4Addr::new(198, 20, 0, 1)));
    }

    #[test]
    fn slash24_matching_prefers_same_subnet() {
        assert!(same_slash24(
            &Ipv4Addr::new(192, 115, 1, 15),
            &Ipv4Addr::new(192, 115, 1, 17)
        ));
        assert!(!same_slash24(
            &Ipv4Addr::new(192, 115, 1, 15),
            &Ipv4Addr::new(192, 115, 2, 38)
        ));
    }

    #[test]
    fn err_code_check_reports_message_from_response() {
        assert!(check_err_code(&json!({ "errCode": 0, "errMsg": "成功" }), "步骤").is_ok());
        let err =
            check_err_code(&json!({ "errCode": 5, "errMsg": "旧密码错误" }), "步骤").unwrap_err();
        assert!(err.contains("errCode=5"));
        assert!(err.contains("旧密码错误"));
        assert!(check_err_code(&json!({ "foo": 1 }), "步骤").is_err());
    }

    #[test]
    fn pascal_case_error_envelopes_are_recognized() {
        // 业务接口返回 camelCase 的 errCode，但登录失败返回 PascalCase 的 ErrCode ——
        // 只认一种会把「账号锁定」「密码错误」埋成「缺少 token」。
        let ums_locked = json!({
            "ErrCode": 94464,
            "ErrMsg": "Usercode or passwd is invalid, locked after multiple times.",
            "AccessCode": "",
            "ResidueDegree": 4,
        });
        let detail = extract_error(&ums_locked).expect("PascalCase 错误必须被识别");
        assert!(detail.contains("94464"));
        assert!(detail.contains("locked after multiple times"));

        let cdm_failed = json!({ "ErrCode": 320038, "ErrMsg": "用户登录失败" });
        let detail = extract_error(&cdm_failed).expect("CDM 错误必须被识别");
        assert!(detail.contains("320038"));
        assert!(detail.contains("用户登录失败"));

        assert!(extract_error(&json!({ "errCode": 0, "errMsg": "成功" })).is_none());
        assert!(extract_error(&json!({ "AccessToken": "abc" })).is_none());
    }

    #[test]
    fn ums_skips_the_change_call_when_old_and_new_passwords_match() {
        // 源码级断言：跳过条件必须是「新旧密码相同」，且 ③④ 被同一个开关包住，
        // ⑤ 始终执行 —— 这正是「密码已是目标值、只补初始化标识」的语义。
        let source = include_str!("ums_init_password.rs");
        assert!(source.contains("let password_change_skipped = old_password == new_password;"));
        assert!(source.contains("if !password_change_skipped {"));
        // 跳过时开关是唯一操作，失败即整体失败；未跳过时开关失败只降级为备注。
        assert!(source.contains("fail_result(kind, e, \"dictionary\")"));
        assert!(source.contains("新旧密码相同，已跳过改密，仅置 pwdIsInit 开关"));
    }

    #[test]
    fn ums_password_change_encrypts_the_md5_digest_not_the_plaintext() {
        // 实机验证：原文被回 errCode=94438，MD5 才是 errCode=0。
        // 这里锁住摘要口径（小写十六进制、32 字符），防止有人改回原文。
        let digest = md5_hex("admin_1234");
        assert_eq!(digest, "defca3e3fee3d112b9275896d086883f");
        assert_eq!(digest.len(), 32);
        assert_eq!(digest, digest.to_lowercase());

        // 摘要进 RSA 信封后仍是标准 2048 位密文。
        let ciphertext = rsa_pkcs1v15_encrypt_base64(SAMPLE_PUBLIC_KEY, &digest).unwrap();
        assert_eq!(BASE64.decode(&ciphertext).unwrap().len(), 256);
    }

    #[test]
    fn long_response_bodies_are_truncated_for_logging() {
        assert_eq!(truncate_for_log("   "), "<空>");
        assert_eq!(truncate_for_log(" ok "), "ok");
        let long = "x".repeat(LOG_BODY_LIMIT + 50);
        let logged = truncate_for_log(&long);
        assert!(logged.contains("共 850 字符"));
        assert!(logged.chars().count() < long.chars().count());
    }
}
