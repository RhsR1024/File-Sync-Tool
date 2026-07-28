use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::config::PortalLoginSettings;

const TOOL_NAME: &str = "portal-auto-login";

#[derive(Debug, Clone, Serialize)]
pub struct PortalLoginStep {
    pub code: String,
    pub level: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortalLoginResult {
    pub outcome: String,
    pub attempts: u32,
    pub account: Option<String>,
    pub detail: Option<String>,
    pub checked_at: String,
    pub steps: Vec<PortalLoginStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortalLoginCheckResult {
    pub logged_in: bool,
    pub account: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortalLoginRuntimeStatus {
    pub running: bool,
    pub last_result: Option<PortalLoginResult>,
}

#[derive(Clone, Default)]
pub struct PortalLoginRuntime {
    running: Arc<AtomicBool>,
    last_result: Arc<Mutex<Option<PortalLoginResult>>>,
}

impl PortalLoginRuntime {
    fn status(&self) -> PortalLoginRuntimeStatus {
        PortalLoginRuntimeStatus {
            running: self.running.load(Ordering::SeqCst),
            last_result: self.last_result.lock().unwrap().clone(),
        }
    }
}

struct RunningGuard<'a>(&'a AtomicBool);

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct StepLog {
    steps: Vec<PortalLoginStep>,
}

impl StepLog {
    fn push(
        &mut self,
        app_handle: &tauri::AppHandle,
        code: &str,
        level: &str,
        detail: impl Into<Option<String>>,
    ) {
        let detail = detail.into();
        let suffix = detail
            .as_deref()
            .map(|value| format!(": {value}"))
            .unwrap_or_default();
        crate::scanner::emit_tool_log(app_handle, TOOL_NAME, &format!("{code}{suffix}"), level);
        self.steps.push(PortalLoginStep {
            code: code.to_string(),
            level: level.to_string(),
            detail,
        });
    }
}

#[derive(Debug, Deserialize)]
struct PortalResponse {
    #[serde(default)]
    success: bool,
    location: Option<String>,
    msg: Option<String>,
    data: Option<Value>,
}

pub fn start_if_enabled(
    app_handle: tauri::AppHandle,
    settings: PortalLoginSettings,
    runtime: PortalLoginRuntime,
) {
    if !settings.enabled {
        return;
    }

    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_with_runtime(app_handle.clone(), settings, runtime).await {
            crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &error, "error");
        }
    });
}

#[tauri::command]
pub fn portal_login_get_runtime_status(
    runtime: State<'_, PortalLoginRuntime>,
) -> PortalLoginRuntimeStatus {
    runtime.status()
}

#[tauri::command]
pub async fn portal_login_check_status(
    state: State<'_, crate::AppState>,
) -> Result<PortalLoginCheckResult, String> {
    let settings = state.config.lock().unwrap().portal_login.clone();
    validate_credentials(&settings, false)?;
    let client = build_client(settings.request_timeout_secs)?;
    let base_url = parse_base_url(&settings.host)?;
    let status = query_login_status(&client, &base_url).await;
    Ok(PortalLoginCheckResult {
        logged_in: status.logged_in,
        account: status.account,
        checked_at: now_rfc3339(),
    })
}

#[tauri::command]
pub async fn portal_login_run(
    app_handle: tauri::AppHandle,
    state: State<'_, crate::AppState>,
    runtime: State<'_, PortalLoginRuntime>,
) -> Result<PortalLoginResult, String> {
    let settings = state.config.lock().unwrap().portal_login.clone();
    run_with_runtime(app_handle, settings, runtime.inner().clone()).await
}

async fn run_with_runtime(
    app_handle: tauri::AppHandle,
    settings: PortalLoginSettings,
    runtime: PortalLoginRuntime,
) -> Result<PortalLoginResult, String> {
    validate_credentials(&settings, true)?;
    if runtime
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("portal_login.already_running".to_string());
    }
    let _guard = RunningGuard(&runtime.running);
    let result = execute_login(&app_handle, &settings).await;
    *runtime.last_result.lock().unwrap() = Some(result.clone());
    Ok(result)
}

fn validate_credentials(
    settings: &PortalLoginSettings,
    require_credentials: bool,
) -> Result<(), String> {
    crate::config::validate_portal_login_settings(settings)?;
    if require_credentials && settings.username.trim().is_empty() {
        return Err("portal_login.username_required".to_string());
    }
    if require_credentials && settings.password.is_empty() {
        return Err("portal_login.password_required".to_string());
    }
    Ok(())
}

fn build_client(timeout_secs: u64) -> Result<Client, String> {
    Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|error| format!("portal_login.client_failed: {error}"))
}

fn parse_base_url(value: &str) -> Result<Url, String> {
    let normalized = format!("{}/", value.trim().trim_end_matches('/'));
    let url = Url::parse(&normalized).map_err(|_| "portal_login.invalid_host".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("portal_login.invalid_host".to_string());
    }
    Ok(url)
}

fn resolve_url(base: &Url, value: &str) -> Result<Url, String> {
    if let Ok(url) = Url::parse(value.trim()) {
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            return Ok(url);
        }
        return Err("portal_login.invalid_url".to_string());
    }
    base.join(value.trim().trim_start_matches('/'))
        .map_err(|_| "portal_login.invalid_url".to_string())
}

struct LoginStatus {
    logged_in: bool,
    account: Option<String>,
}

async fn query_login_status(client: &Client, base_url: &Url) -> LoginStatus {
    if let Ok(home_url) = base_url.join("homepage/index.html") {
        if let Ok(response) = client.get(home_url).send().await {
            if response.status().is_success() {
                if let Ok(content) = response.text().await {
                    if !content.contains("ac_portal") {
                        return LoginStatus {
                            logged_in: true,
                            account: None,
                        };
                    }
                }
            }
        }
    }

    let Ok(info_url) = base_url.join("homepage/info.php") else {
        return LoginStatus {
            logged_in: false,
            account: None,
        };
    };
    let response = client.post(info_url).form(&[("opr", "list")]).send().await;
    if let Ok(response) = response {
        if let Ok(payload) = response.json::<PortalResponse>().await {
            if payload.success {
                return LoginStatus {
                    logged_in: true,
                    account: account_name(&payload),
                };
            }
        }
    }
    LoginStatus {
        logged_in: false,
        account: None,
    }
}

async fn wait_for_network(
    app_handle: &tauri::AppHandle,
    client: &Client,
    base_url: &Url,
    wait_secs: u64,
    steps: &mut StepLog,
) {
    if wait_secs == 0 {
        return;
    }
    steps.push(
        app_handle,
        "waiting_network",
        "info",
        Some(wait_secs.to_string()),
    );
    let started = tokio::time::Instant::now();
    loop {
        let probe =
            tokio::time::timeout(Duration::from_secs(3), client.get(base_url.clone()).send()).await;
        if matches!(probe, Ok(Ok(_))) {
            steps.push(app_handle, "network_ready", "success", None);
            return;
        }
        if started.elapsed() >= Duration::from_secs(wait_secs) {
            steps.push(app_handle, "network_timeout", "warn", None);
            return;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn execute_login(
    app_handle: &tauri::AppHandle,
    settings: &PortalLoginSettings,
) -> PortalLoginResult {
    let mut steps = StepLog::default();
    let client = match build_client(settings.request_timeout_secs) {
        Ok(client) => client,
        Err(error) => {
            steps.push(app_handle, "request_failed", "error", Some(error.clone()));
            return result("failed", 0, None, Some(error), steps);
        }
    };
    let base_url = match parse_base_url(&settings.host) {
        Ok(url) => url,
        Err(error) => {
            steps.push(app_handle, "request_failed", "error", Some(error.clone()));
            return result("failed", 0, None, Some(error), steps);
        }
    };

    wait_for_network(
        app_handle,
        &client,
        &base_url,
        settings.network_wait_secs,
        &mut steps,
    )
    .await;

    steps.push(app_handle, "checking_status", "info", None);
    let status = query_login_status(&client, &base_url).await;
    if status.logged_in {
        steps.push(
            app_handle,
            "already_logged_in",
            "success",
            status.account.clone(),
        );
        return result("already_logged_in", 0, status.account, None, steps);
    }

    let login_url = match resolve_url(&base_url, &settings.login_url) {
        Ok(url) => url,
        Err(error) => return result("failed", 0, None, Some(error), steps),
    };
    let portal_url = match resolve_url(&base_url, &settings.portal_url) {
        Ok(url) => url,
        Err(error) => return result("failed", 0, None, Some(error), steps),
    };

    for attempt in 1..=settings.retry_count {
        steps.push(
            app_handle,
            "attempt_started",
            "info",
            Some(format!("{attempt}/{}", settings.retry_count)),
        );
        match login_once(
            app_handle,
            settings,
            &client,
            &base_url,
            &portal_url,
            &login_url,
            &mut steps,
        )
        .await
        {
            Ok((account, detail)) => {
                steps.push(app_handle, "completed", "success", account.clone());
                return result("success", attempt, account, detail, steps);
            }
            Err(error) => {
                if attempt == settings.retry_count {
                    steps.push(app_handle, "login_failed", "error", Some(error.clone()));
                    return result("failed", attempt, None, Some(error), steps);
                }
                steps.push(
                    app_handle,
                    "retry_scheduled",
                    "warn",
                    Some(settings.retry_interval_secs.to_string()),
                );
                tokio::time::sleep(Duration::from_secs(settings.retry_interval_secs)).await;
            }
        }
    }

    result("failed", settings.retry_count, None, None, steps)
}

async fn login_once(
    app_handle: &tauri::AppHandle,
    settings: &PortalLoginSettings,
    client: &Client,
    base_url: &Url,
    portal_url: &Url,
    login_url: &Url,
    steps: &mut StepLog,
) -> Result<(Option<String>, Option<String>), String> {
    steps.push(app_handle, "fetching_cookie", "info", None);
    match client.get(portal_url.clone()).send().await {
        Ok(_) => steps.push(app_handle, "cookie_ready", "success", None),
        Err(error) => steps.push(app_handle, "cookie_failed", "warn", Some(error.to_string())),
    }

    let key = unix_timestamp_millis().to_string();
    let encrypted_password = rc4_hex_utf16(&settings.password, &key);
    steps.push(app_handle, "encrypting_password", "info", None);
    steps.push(app_handle, "sending_request", "info", None);

    let remember_pwd = if settings.remember_pwd { "1" } else { "0" };
    let response = client
        .post(login_url.clone())
        .form(&[
            ("opr", "pwdLogin"),
            ("userName", settings.username.trim()),
            ("pwd", encrypted_password.as_str()),
            ("auth_tag", key.as_str()),
            ("rememberPwd", remember_pwd),
        ])
        .send()
        .await
        .map_err(|error| format!("portal_login.request_failed: {error}"))?;
    let status = response.status();
    let payload = response
        .json::<PortalResponse>()
        .await
        .map_err(|error| format!("portal_login.invalid_response ({status}): {error}"))?;
    if !payload.success {
        return Err(payload
            .msg
            .unwrap_or_else(|| "portal_login.rejected".to_string()));
    }
    steps.push(
        app_handle,
        "server_accepted",
        "success",
        payload.msg.clone(),
    );

    if let Some(location) = payload
        .location
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        steps.push(app_handle, "visiting_location", "info", None);
        match resolve_url(base_url, location) {
            Ok(url) => {
                if let Err(error) = client.get(url).send().await {
                    steps.push(
                        app_handle,
                        "location_failed",
                        "warn",
                        Some(error.to_string()),
                    );
                }
            }
            Err(error) => steps.push(app_handle, "location_failed", "warn", Some(error)),
        }
    }

    steps.push(app_handle, "verifying", "info", None);
    let status = query_login_status(client, base_url).await;
    if status.logged_in {
        steps.push(app_handle, "verified", "success", status.account.clone());
        Ok((status.account, payload.msg))
    } else {
        steps.push(app_handle, "verification_failed", "warn", None);
        // Preserve the script's behavior: a successful login response is accepted even
        // when the optional info.php verification endpoint is unavailable.
        Ok((account_name(&payload), payload.msg))
    }
}

fn result(
    outcome: &str,
    attempts: u32,
    account: Option<String>,
    detail: Option<String>,
    steps: StepLog,
) -> PortalLoginResult {
    PortalLoginResult {
        outcome: outcome.to_string(),
        attempts,
        account,
        detail,
        checked_at: now_rfc3339(),
        steps: steps.steps,
    }
}

fn account_name(payload: &PortalResponse) -> Option<String> {
    payload
        .data
        .as_ref()
        .and_then(|data| data.get("basic"))
        .and_then(|basic| basic.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn rc4_hex_utf16(plain_text: &str, key: &str) -> String {
    let source: Vec<u16> = plain_text.encode_utf16().collect();
    let password: Vec<u16> = key.encode_utf16().collect();
    if password.is_empty() {
        return String::new();
    }

    let mut sbox: Vec<u16> = (0..=255).collect();
    let mut j = 0usize;
    for i in 0..256usize {
        j = (j + usize::from(sbox[i]) + usize::from(password[i % password.len()])) % 256;
        sbox.swap(i, j);
    }

    let mut a = 0usize;
    let mut b = 0usize;
    let mut output = String::with_capacity(source.len() * 2);
    for value in source {
        a = (a + 1) % 256;
        b = (b + usize::from(sbox[a])) % 256;
        sbox.swap(a, b);
        let c = (usize::from(sbox[a]) + usize::from(sbox[b])) % 256;
        output.push_str(&format!("{:02x}", value ^ sbox[c]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_matches_the_standard_ascii_vector() {
        assert_eq!(rc4_hex_utf16("Plaintext", "Key"), "bbf316e8d940af0ad3");
    }

    #[test]
    fn relative_portal_paths_resolve_against_the_host_root() {
        let base = parse_base_url("http://1.1.1.3").unwrap();
        assert_eq!(
            resolve_url(&base, "/ac_portal/login.php").unwrap().as_str(),
            "http://1.1.1.3/ac_portal/login.php"
        );
    }
}
