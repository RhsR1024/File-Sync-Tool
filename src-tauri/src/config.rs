use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeployServer {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub remote_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub remote_paths: Vec<String>,
    pub target_versions: Vec<String>,
    pub local_path: String,
    pub interval_minutes: u64,
    pub time_ranges: Vec<String>, // "HH:mm-HH:mm"
    // New fields for filtering
    pub file_extensions: Vec<String>, // e.g. ["exe", "tar.gz"]
    pub filename_includes: Vec<String>, // e.g. ["UMS", "VMS"] - OR logic
    
    // Deploy Config
    pub deploy_enabled: bool,
    #[serde(default)]
    pub servers: Vec<DeployServer>, // New: Multiple servers
    
    // Legacy single server config (kept for migration/fallback)
    #[serde(default)]
    pub ssh_host: String,
    #[serde(default)]
    pub ssh_port: u16,
    #[serde(default)]
    pub ssh_user: String,
    #[serde(default)]
    pub ssh_password: String,
    #[serde(default)]
    pub remote_linux_path: String,
    
    pub post_commands: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            remote_paths: vec![],
            target_versions: vec![],
            local_path: "E:\\UMS_TEMP".to_string(),
            interval_minutes: 10,
            time_ranges: vec![],
            file_extensions: vec![],
            filename_includes: vec![],
            deploy_enabled: false,
            servers: vec![],
            ssh_host: "".to_string(),
            ssh_port: 22,
            ssh_user: "".to_string(),
            ssh_password: "".to_string(),
            remote_linux_path: "/tmp/upload".to_string(),
            post_commands: vec![],
        }
    }
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let config_path = get_config_path(app_handle);
    if config_path.exists() {
        if let Ok(content) = fs::read_to_string(&config_path) {
            if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                // Migration: If servers empty but legacy host exists, migrate it
                if config.servers.is_empty() && !config.ssh_host.is_empty() {
                    config.servers.push(DeployServer {
                        id: uuid::Uuid::new_v4().to_string(),
                        enabled: true,
                        name: "Default Server".to_string(),
                        host: config.ssh_host.clone(),
                        port: config.ssh_port,
                        user: config.ssh_user.clone(),
                        password: config.ssh_password.clone(),
                        remote_path: config.remote_linux_path.clone(),
                    });
                }
                return config;
            }
        }
    }
    AppConfig::default()
}

pub fn save_config(app_handle: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let config_path = get_config_path(app_handle);
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(config_path, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn get_config_path(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle.path().app_config_dir().unwrap().join("config.json")
}
