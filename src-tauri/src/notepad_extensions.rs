use chrono::Utc;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;
use zip::ZipArchive;

const CATALOG_PATH: &str = "notepad-plugins/catalog-v1.json";
const MAX_PLUGIN_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PLUGIN_ARCHIVE_FILES: usize = 2_048;
const ENHANCE_PLUGIN_NAME: &str = "EnhanceAnyLexer";
const ENHANCE_CONFIG_NAME: &str = "EnhanceAnyLexerConfig.ini";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotepadArchitecture {
    X86,
    X64,
    Arm64,
    Unknown,
}

impl NotepadArchitecture {
    fn catalog_key(&self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X64 => "x64",
            Self::Arm64 => "arm64",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotepadPluginStatus {
    pub installed: bool,
    pub dll_path: String,
    pub config_path: String,
    pub config_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledNotepadPlugin {
    pub name: String,
    pub dll_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotepadInstance {
    pub exe_path: String,
    pub install_dir: String,
    pub settings_dir: String,
    pub architecture: NotepadArchitecture,
    pub architecture_key: String,
    pub source: String,
    pub portable: bool,
    pub running: bool,
    pub requires_elevation: bool,
    pub installed_plugins: Vec<InstalledNotepadPlugin>,
    pub enhance_any_lexer: NotepadPluginStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCatalog {
    pub schema_version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub plugins: Vec<PluginCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCatalogEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub description_zh: String,
    #[serde(default)]
    pub description_en: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub adapter: String,
    #[serde(default)]
    pub releases: Vec<PluginCatalogRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCatalogRelease {
    pub version: String,
    #[serde(default)]
    pub notepad_compatible: String,
    #[serde(default)]
    pub packages: std::collections::HashMap<String, PluginCatalogPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCatalogPackage {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
    pub install_dir: String,
    pub entry_dll: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInstallProgress {
    pub plugin_id: String,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInstallResult {
    pub target_path: String,
    pub restart_required: bool,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhanceAnyLexerGlobal {
    pub indicator_id: i32,
    pub offset: i32,
    pub regex_error_style_id: i32,
    pub regex_error_color: String,
}

impl Default for EnhanceAnyLexerGlobal {
    fn default() -> Self {
        Self {
            indicator_id: 0,
            offset: 0,
            regex_error_style_id: 30,
            regex_error_color: "#E06C75".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnhanceMatcherKind {
    Words,
    Line,
    Between,
    Preset,
    #[default]
    Regex,
}

/// 预设匹配器。编译产物写入 `EnhanceAnyLexerRule::pattern`，正则始终是唯一真相来源。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnhanceMatcher {
    #[serde(default)]
    pub kind: EnhanceMatcherKind,
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub open: String,
    #[serde(default)]
    pub close: String,
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub line_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhanceAnyLexerRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub color: String,
    pub pattern: String,
    #[serde(default)]
    pub whitelist_styles: Vec<i32>,
    #[serde(default)]
    pub matcher: EnhanceMatcher,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhanceAnyLexerSection {
    pub lexer: String,
    #[serde(default)]
    pub excluded_styles: Vec<i32>,
    #[serde(default)]
    pub rules: Vec<EnhanceAnyLexerRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhanceAnyLexerConfig {
    #[serde(default)]
    pub global: EnhanceAnyLexerGlobal,
    #[serde(default)]
    pub sections: Vec<EnhanceAnyLexerSection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnhanceSaveResult {
    pub config_path: String,
    pub backup_path: Option<String>,
    pub restart_required: bool,
}

fn normalize_windows_path_text(value: &str) -> String {
    let normalized = value.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = normalized.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        normalized
    }
}

fn path_text(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        normalize_windows_path_text(&path.to_string_lossy())
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string_lossy().into_owned()
    }
}

fn parse_pe_architecture(path: &Path) -> Result<NotepadArchitecture, String> {
    let mut file = fs::File::open(path).map_err(|error| format!("open_executable: {error}"))?;
    let mut dos = [0_u8; 64];
    file.read_exact(&mut dos)
        .map_err(|error| format!("read_executable: {error}"))?;
    if &dos[0..2] != b"MZ" {
        return Err("not_windows_executable".into());
    }
    let pe_offset = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(pe_offset))
        .map_err(|error| format!("seek_executable: {error}"))?;
    let mut header = [0_u8; 6];
    file.read_exact(&mut header)
        .map_err(|error| format!("read_pe_header: {error}"))?;
    if &header[0..4] != b"PE\0\0" {
        return Err("invalid_pe_signature".into());
    }
    Ok(match u16::from_le_bytes([header[4], header[5]]) {
        0x014c => NotepadArchitecture::X86,
        0x8664 => NotepadArchitecture::X64,
        0xaa64 => NotepadArchitecture::Arm64,
        _ => NotepadArchitecture::Unknown,
    })
}

fn settings_dir_for(install_dir: &Path) -> PathBuf {
    if install_dir.join("doLocalConf.xml").is_file() {
        return install_dir.to_path_buf();
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| install_dir.to_path_buf())
        .join("Notepad++")
}

fn enhance_status(install_dir: &Path, settings_dir: &Path) -> NotepadPluginStatus {
    let dll_path = install_dir
        .join("plugins")
        .join(ENHANCE_PLUGIN_NAME)
        .join(format!("{ENHANCE_PLUGIN_NAME}.dll"));
    let config_path = settings_dir
        .join("plugins")
        .join("Config")
        .join(ENHANCE_PLUGIN_NAME)
        .join(ENHANCE_CONFIG_NAME);
    NotepadPluginStatus {
        installed: dll_path.is_file(),
        dll_path: path_text(&dll_path),
        config_exists: config_path.is_file(),
        config_path: path_text(&config_path),
    }
}

fn installed_plugins(install_dir: &Path) -> Vec<InstalledNotepadPlugin> {
    let root = install_dir.join("plugins");
    let mut plugins = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return plugins;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let dll = entry.path().join(format!("{name}.dll"));
        if dll.is_file() {
            plugins.push(InstalledNotepadPlugin {
                name,
                dll_path: path_text(&dll),
            });
        }
    }
    plugins.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    plugins
}

fn is_under_program_files(path: &Path) -> bool {
    [
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
    ]
    .into_iter()
    .flatten()
    .map(PathBuf::from)
    .any(|root| path.starts_with(root))
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    PathBuf::from(path_text(&resolved))
}

fn normalized_path_key(path: &Path) -> String {
    let mut value = path_text(&normalize_existing_path(path));
    while value.len() > 3 && (value.ends_with('\\') || value.ends_with('/')) {
        value.pop();
    }
    value.to_lowercase()
}

fn validate_instance_path(path: &Path, source: &str) -> Result<NotepadInstance, String> {
    if !path.is_file() {
        return Err("notepad_executable_not_found".into());
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !file_name.eq_ignore_ascii_case("notepad++.exe") {
        return Err("not_notepad_executable".into());
    }
    let path = normalize_existing_path(path);
    let install_dir = path
        .parent()
        .ok_or_else(|| "notepad_install_dir_missing".to_string())?
        .to_path_buf();
    if !install_dir.join("langs.model.xml").is_file()
        && !install_dir.join("stylers.model.xml").is_file()
    {
        return Err("notepad_marker_files_missing".into());
    }
    let architecture = parse_pe_architecture(&path)?;
    if architecture == NotepadArchitecture::Unknown {
        return Err("notepad_architecture_unsupported".into());
    }
    let settings_dir = settings_dir_for(&install_dir);
    let instance_key = normalized_path_key(&path);
    let running = running_notepad_paths()
        .iter()
        .any(|candidate| normalized_path_key(Path::new(candidate)) == instance_key);
    let portable = install_dir.join("doLocalConf.xml").is_file();
    Ok(NotepadInstance {
        exe_path: path_text(&path),
        install_dir: path_text(&install_dir),
        settings_dir: path_text(&settings_dir),
        architecture_key: architecture.catalog_key().into(),
        architecture,
        source: source.into(),
        portable,
        running,
        requires_elevation: is_under_program_files(&install_dir),
        installed_plugins: installed_plugins(&install_dir),
        enhance_any_lexer: enhance_status(&install_dir, &settings_dir),
    })
}

#[cfg(target_os = "windows")]
fn registry_candidates() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut paths = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for key in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\notepad++.exe",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\notepad++.exe",
        ] {
            if let Ok(app_path) = root.open_subkey_with_flags(key, KEY_READ) {
                if let Ok(value) = app_path.get_value::<String, _>("") {
                    paths.push(PathBuf::from(value.trim_matches('"')));
                }
            }
        }
    }
    paths
}

#[cfg(not(target_os = "windows"))]
fn registry_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn running_notepad_paths() -> Vec<String> {
    use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut result = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return result;
        };
        if snapshot == INVALID_HANDLE_VALUE {
            return result;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut has_entry = Process32FirstW(snapshot, &mut entry).is_ok();
        while has_entry {
            let nul = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..nul]);
            if name.eq_ignore_ascii_case("notepad++.exe") {
                if let Ok(process) = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    entry.th32ProcessID,
                ) {
                    let mut buffer = vec![0_u16; 32_768];
                    let mut length = buffer.len() as u32;
                    if QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buffer.as_mut_ptr()),
                        &mut length,
                    )
                    .is_ok()
                    {
                        result.push(String::from_utf16_lossy(&buffer[..length as usize]));
                    }
                    let _ = CloseHandle(process);
                }
            }
            has_entry = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn running_notepad_paths() -> Vec<String> {
    Vec::new()
}

fn automatic_candidates() -> Vec<(PathBuf, String)> {
    let mut candidates = Vec::new();
    for path in running_notepad_paths() {
        candidates.push((PathBuf::from(path), "running".into()));
    }
    for path in registry_candidates() {
        candidates.push((path, "registry".into()));
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push((
                PathBuf::from(root).join("Notepad++").join("notepad++.exe"),
                "standard".into(),
            ));
        }
    }
    candidates
}

#[tauri::command]
pub async fn notepad_extensions_detect_instances() -> Result<Vec<NotepadInstance>, String> {
    tokio::task::spawn_blocking(|| {
        let mut seen = HashSet::new();
        let mut instances = Vec::new();
        for (path, source) in automatic_candidates() {
            let key = normalized_path_key(&path);
            if !seen.insert(key) {
                continue;
            }
            if let Ok(instance) = validate_instance_path(&path, &source) {
                instances.push(instance);
            }
        }
        Ok(instances)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn notepad_extensions_validate_instance(
    exe_path: String,
) -> Result<NotepadInstance, String> {
    tokio::task::spawn_blocking(move || validate_instance_path(Path::new(&exe_path), "manual"))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn notepad_extensions_pick_executable() -> Result<Option<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .add_filter("Notepad++", &["exe"])
        .pick_file()
        .await
        .map(|file| path_text(file.path())))
}

fn catalog_url(server_url: &str) -> Result<Url, String> {
    let base = server_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("plugin_server_not_configured".into());
    }
    Url::parse(&format!("{base}/{CATALOG_PATH}"))
        .map_err(|error| format!("plugin_catalog_url_invalid: {error}"))
}

#[tauri::command]
pub async fn notepad_extensions_fetch_catalog(server_url: String) -> Result<PluginCatalog, String> {
    let url = catalog_url(&server_url)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| format!("plugin_catalog_network: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "plugin_catalog_http_{}",
            response.status().as_u16()
        ));
    }
    let catalog: PluginCatalog = response
        .json()
        .await
        .map_err(|error| format!("plugin_catalog_invalid: {error}"))?;
    if catalog.schema_version != 1 {
        return Err(format!(
            "plugin_catalog_schema_unsupported:{}",
            catalog.schema_version
        ));
    }
    Ok(catalog)
}

fn safe_plugin_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn resolve_package_url(server_url: &str, value: &str) -> Result<Url, String> {
    if let Ok(url) = Url::parse(value) {
        if matches!(url.scheme(), "http" | "https") {
            return Ok(url);
        }
        return Err("plugin_package_scheme_invalid".into());
    }
    let root = Url::parse(&format!(
        "{}/{}/",
        server_url.trim().trim_end_matches('/'),
        "notepad-plugins"
    ))
    .map_err(|error| format!("plugin_server_url_invalid: {error}"))?;
    root.join(value)
        .map_err(|error| format!("plugin_package_url_invalid: {error}"))
}

fn emit_install_phase(app: &AppHandle, plugin_id: &str, phase: &str) {
    let _ = app.emit(
        "notepad-plugin-install-progress",
        PluginInstallProgress {
            plugin_id: plugin_id.into(),
            phase: phase.into(),
        },
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn app_backup_root(plugin_id: &str) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.filesync.tool")
        .join("notepad-extensions")
        .join("backups")
        .join(plugin_id)
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| format!("create_install_dir: {error}"))?;
    for entry in fs::read_dir(source).map_err(|error| format!("read_package_dir: {error}"))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|error| format!("copy_plugin_file: {error}"))?;
        }
    }
    Ok(())
}

fn find_named_file(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named_file(&path, name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(name))
        {
            return Some(path);
        }
    }
    None
}

fn extract_plugin_package(
    bytes: Vec<u8>,
    package: &PluginCatalogPackage,
) -> Result<PathBuf, String> {
    let temp_root = std::env::temp_dir().join(format!("fst-npp-plugin-{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_root).map_err(|error| format!("create_plugin_temp: {error}"))?;
    let result = (|| {
        let mut archive = ZipArchive::new(Cursor::new(bytes))
            .map_err(|error| format!("plugin_archive_invalid: {error}"))?;
        if archive.len() > MAX_PLUGIN_ARCHIVE_FILES {
            return Err("plugin_archive_too_many_files".into());
        }
        let mut total_size = 0_u64;
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|error| format!("plugin_archive_entry_invalid: {error}"))?;
            total_size = total_size.saturating_add(file.size());
            if total_size > MAX_PLUGIN_PACKAGE_BYTES {
                return Err("plugin_archive_too_large".into());
            }
            if file
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("plugin_archive_symlink_rejected".into());
            }
            let relative = file
                .enclosed_name()
                .ok_or_else(|| "plugin_archive_path_rejected".to_string())?;
            if relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err("plugin_archive_path_rejected".into());
            }
            let output = temp_root.join(relative);
            if file.is_dir() {
                fs::create_dir_all(&output).map_err(|error| error.to_string())?;
                continue;
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut target = fs::File::create(&output).map_err(|error| error.to_string())?;
            std::io::copy(&mut file, &mut target).map_err(|error| error.to_string())?;
        }
        let entry = find_named_file(&temp_root, &package.entry_dll)
            .ok_or_else(|| "plugin_entry_dll_missing".to_string())?;
        Ok(entry
            .parent()
            .ok_or_else(|| "plugin_package_root_missing".to_string())?
            .to_path_buf())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp_root);
    }
    result
}

fn install_extracted_plugin(
    instance: &NotepadInstance,
    plugin_id: &str,
    package: &PluginCatalogPackage,
    extracted_root: &Path,
) -> Result<PluginInstallResult, String> {
    if !safe_plugin_component(&package.install_dir)
        || !safe_plugin_component(&package.entry_dll)
        || !package.entry_dll.to_ascii_lowercase().ends_with(".dll")
    {
        return Err("plugin_install_layout_invalid".into());
    }
    let install_root = PathBuf::from(&instance.install_dir).join("plugins");
    let target = install_root.join(&package.install_dir);
    if !target.starts_with(&install_root) {
        return Err("plugin_install_path_rejected".into());
    }
    let entry = extracted_root.join(&package.entry_dll);
    if !entry.is_file() {
        return Err("plugin_entry_dll_missing".into());
    }
    let plugin_arch = parse_pe_architecture(&entry)?;
    if plugin_arch != instance.architecture {
        return Err("plugin_architecture_mismatch".into());
    }
    fs::create_dir_all(&install_root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            "plugin_install_permission_denied".into()
        } else {
            format!("plugin_install_dir_failed: {error}")
        }
    })?;
    let staging = install_root.join(format!(
        ".{}.fst-install-{}",
        package.install_dir,
        Uuid::new_v4()
    ));
    copy_tree(extracted_root, &staging).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        if error.to_ascii_lowercase().contains("access")
            || error.to_ascii_lowercase().contains("permission")
        {
            "plugin_install_permission_denied".into()
        } else {
            error
        }
    })?;

    let mut backup_path = None;
    if target.exists() {
        let backup =
            app_backup_root(plugin_id).join(Utc::now().format("%Y%m%d-%H%M%S").to_string());
        copy_tree(&target, &backup)?;
        backup_path = Some(path_text(&backup));
        fs::remove_dir_all(&target).map_err(|error| {
            let _ = fs::remove_dir_all(&staging);
            if instance.running {
                "plugin_update_requires_notepad_exit".into()
            } else if error.kind() == std::io::ErrorKind::PermissionDenied {
                "plugin_install_permission_denied".into()
            } else {
                format!("plugin_remove_old_failed: {error}")
            }
        })?;
    }
    if let Err(error) = fs::rename(&staging, &target) {
        let _ = fs::remove_dir_all(&staging);
        if let Some(backup) = backup_path.as_ref() {
            let _ = copy_tree(Path::new(backup), &target);
        }
        return Err(if error.kind() == std::io::ErrorKind::PermissionDenied {
            "plugin_install_permission_denied".into()
        } else {
            format!("plugin_install_finalize_failed: {error}")
        });
    }
    Ok(PluginInstallResult {
        target_path: path_text(&target),
        restart_required: instance.running,
        backup_path,
    })
}

#[tauri::command]
pub async fn notepad_extensions_install_plugin(
    app: AppHandle,
    server_url: String,
    exe_path: String,
    plugin_id: String,
    package: PluginCatalogPackage,
) -> Result<PluginInstallResult, String> {
    if !safe_plugin_component(&plugin_id) {
        return Err("plugin_id_invalid".into());
    }
    let instance = validate_instance_path(Path::new(&exe_path), "manual")?;
    let url = resolve_package_url(&server_url, &package.url)?;
    emit_install_phase(&app, &plugin_id, "downloading");
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| format!("plugin_download_failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "plugin_download_http_{}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_PLUGIN_PACKAGE_BYTES)
    {
        return Err("plugin_package_too_large".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("plugin_download_read_failed: {error}"))?;
    if bytes.len() as u64 > MAX_PLUGIN_PACKAGE_BYTES {
        return Err("plugin_package_too_large".into());
    }
    emit_install_phase(&app, &plugin_id, "verifying");
    let expected = package.sha256.trim().to_ascii_lowercase();
    if expected.len() != 64
        || !expected
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("plugin_sha256_invalid".into());
    }
    if sha256_hex(&bytes) != expected {
        return Err("plugin_sha256_mismatch".into());
    }
    emit_install_phase(&app, &plugin_id, "extracting");
    let package_for_extract = package.clone();
    let extracted = tokio::task::spawn_blocking(move || {
        extract_plugin_package(bytes.to_vec(), &package_for_extract)
    })
    .await
    .map_err(|error| error.to_string())??;
    emit_install_phase(&app, &plugin_id, "installing");
    let install_plugin_id = plugin_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let result = install_extracted_plugin(&instance, &install_plugin_id, &package, &extracted);
        let temp_root = extracted
            .ancestors()
            .find(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with("fst-npp-plugin-"))
            })
            .map(Path::to_path_buf);
        if let Some(root) = temp_root {
            let _ = fs::remove_dir_all(root);
        }
        result
    })
    .await
    .map_err(|error| error.to_string())??;
    emit_install_phase(&app, &plugin_id, "complete");
    Ok(result)
}

/// 内置语义预设。EnhanceAnyLexer 使用 Boost.Regex（Perl 语法），可放心使用 `\d`、`{n,m}` 和非捕获组。
const ENHANCE_PRESETS: &[(&str, &str)] = &[
    ("ipv4", r"\b(?:\d{1,3}\.){3}\d{1,3}\b"),
    ("number", r"\b\d+(?:\.\d+)?\b"),
    ("hex", r"\b0[xX][0-9a-fA-F]+\b"),
    ("version", r"\bv?\d+(?:\.\d+){1,3}\b"),
    ("url", r#"\bhttps?://[^\s"'<>]+"#),
    ("win_path", r#"\b[A-Za-z]:\\[^\s"'<>|]*"#),
    ("timestamp", r"\b\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}\b"),
    (
        "guid",
        r"\b[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}\b",
    ),
    ("mac", r"\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b"),
];

fn escape_regex_literal(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}'
        ) {
            output.push('\\');
        }
        output.push(character);
    }
    output
}

fn matcher_core(matcher: &EnhanceMatcher) -> Result<String, String> {
    let terms: Vec<String> = matcher
        .terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .map(escape_regex_literal)
        .collect();
    if terms.is_empty() {
        return Err("enhance_matcher_terms_empty".into());
    }
    let core = if terms.len() == 1 {
        terms[0].clone()
    } else {
        format!("(?:{})", terms.join("|"))
    };
    Ok(if matcher.whole_word {
        format!(r"\b{core}\b")
    } else {
        core
    })
}

fn preset_pattern(id: &str) -> Result<&'static str, String> {
    ENHANCE_PRESETS
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, pattern)| *pattern)
        .ok_or_else(|| format!("enhance_preset_unknown:{id}"))
}

/// 把预设匹配器编译成正则。`Regex` 模式返回 `None`，此时 `pattern` 自身就是权威值。
pub fn compile_matcher(matcher: &EnhanceMatcher) -> Result<Option<String>, String> {
    let body = match matcher.kind {
        EnhanceMatcherKind::Regex => return Ok(None),
        EnhanceMatcherKind::Words => {
            let core = matcher_core(matcher)?;
            if matcher.line_start {
                format!("^{core}")
            } else {
                core
            }
        }
        EnhanceMatcherKind::Line => format!("^.*{}.*$", matcher_core(matcher)?),
        EnhanceMatcherKind::Between => {
            let open = matcher.open.trim();
            let close = matcher.close.trim();
            if open.is_empty() || close.is_empty() {
                return Err("enhance_matcher_delimiter_empty".into());
            }
            let open_pattern = escape_regex_literal(open);
            let close_pattern = escape_regex_literal(close);
            // 结束符是单字符时用否定字符类，保证相邻两段各自独立着色而不会连成一片。
            if close.chars().count() == 1 {
                format!("{open_pattern}[^{close_pattern}]*{close_pattern}")
            } else {
                format!("{open_pattern}.*?{close_pattern}")
            }
        }
        EnhanceMatcherKind::Preset => {
            let pattern = preset_pattern(matcher.preset.trim())?;
            if matcher.line_start {
                format!("^{pattern}")
            } else {
                pattern.to_string()
            }
        }
    };
    // 引擎默认忽略大小写，区分大小写要靠反向内联标志，且必须位于最外层最前面。
    Ok(Some(if matcher.case_sensitive {
        format!("(?-i){body}")
    } else {
        body
    }))
}

/// 注释里的匹配器必须能重新编译出文件中的同一条正则，否则说明用户手工改过，降级为原始正则模式。
fn resolve_matcher(candidate: Option<EnhanceMatcher>, pattern: &str) -> EnhanceMatcher {
    let Some(matcher) = candidate else {
        return EnhanceMatcher::default();
    };
    match compile_matcher(&matcher) {
        Ok(Some(compiled)) if compiled == pattern => matcher,
        _ => EnhanceMatcher::default(),
    }
}

fn rgb_from_plugin_color(value: &str, rgb_format: bool) -> String {
    let normalized = value
        .trim()
        .trim_start_matches('#')
        .trim_start_matches("0x");
    let parsed = u32::from_str_radix(normalized, 16).unwrap_or(0) & 0x00ff_ffff;
    let rgb = if rgb_format {
        parsed
    } else {
        let red = parsed & 0xff;
        let green = parsed & 0xff00;
        let blue = (parsed >> 16) & 0xff;
        (red << 16) | green | blue
    };
    format!("#{rgb:06X}")
}

fn parse_style_list(value: &str) -> Vec<i32> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse::<i32>().ok())
        .collect()
}

fn parse_rule_line(
    line: &str,
    rgb_format: bool,
    enabled: bool,
    pending_name: Option<String>,
    pending_matcher: Option<EnhanceMatcher>,
    index: usize,
) -> Option<EnhanceAnyLexerRule> {
    let split = line.find('=')?;
    let left = line[..split].trim();
    let pattern = line[split + 1..].trim();
    if pattern.is_empty() {
        return None;
    }
    let whitelist = left
        .find('[')
        .and_then(|start| left.find(']').map(|end| (start, end)))
        .map(|(start, end)| parse_style_list(&left[start + 1..end]))
        .unwrap_or_default();
    let color = left.split('[').next().unwrap_or(left).trim();
    Some(EnhanceAnyLexerRule {
        id: Uuid::new_v4().to_string(),
        name: pending_name.unwrap_or_else(|| format!("Rule {}", index + 1)),
        enabled,
        color: rgb_from_plugin_color(color, rgb_format),
        pattern: pattern.into(),
        whitelist_styles: whitelist,
        matcher: resolve_matcher(pending_matcher, pattern),
    })
}

fn parse_enhance_config(text: &str) -> EnhanceAnyLexerConfig {
    let rgb_format = text.lines().any(|line| {
        line.trim()
            .strip_prefix("use_rgb_format")
            .and_then(|value| value.split_once('='))
            .is_some_and(|(_, value)| value.trim() == "1")
    });
    let mut config = EnhanceAnyLexerConfig {
        global: EnhanceAnyLexerGlobal::default(),
        sections: Vec::new(),
    };
    let mut current: Option<EnhanceAnyLexerSection> = None;
    let mut pending_name: Option<String> = None;
    let mut pending_matcher: Option<EnhanceMatcher> = None;
    for source_line in text.lines() {
        let line = source_line.trim();
        if line.starts_with("; FST-NAME ") {
            pending_name = Some(line.trim_start_matches("; FST-NAME ").trim().into());
            continue;
        }
        if let Some(payload) = line.strip_prefix("; FST-MATCH ") {
            pending_matcher = serde_json::from_str::<EnhanceMatcher>(payload.trim()).ok();
            continue;
        }
        if let Some(disabled) = line.strip_prefix("; FST-DISABLED ") {
            if let Some(section) = current.as_mut() {
                if let Some(rule) = parse_rule_line(
                    disabled,
                    rgb_format,
                    false,
                    pending_name.take(),
                    pending_matcher.take(),
                    section.rules.len(),
                ) {
                    section.rules.push(rule);
                }
            }
            continue;
        }
        if line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(section) = current.take() {
                if !section.lexer.eq_ignore_ascii_case("global") {
                    config.sections.push(section);
                }
            }
            current = Some(EnhanceAnyLexerSection {
                lexer: line[1..line.len() - 1].trim().into(),
                excluded_styles: Vec::new(),
                rules: Vec::new(),
            });
            continue;
        }
        let Some(section) = current.as_mut() else {
            continue;
        };
        if section.lexer.eq_ignore_ascii_case("global") {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim();
                match key.trim() {
                    "indicator_id" => config.global.indicator_id = value.parse().unwrap_or(0),
                    "offset" => config.global.offset = value.parse().unwrap_or(0),
                    "regex_error_style_id" => {
                        config.global.regex_error_style_id = value.parse().unwrap_or(30)
                    }
                    "regex_error_color" => {
                        config.global.regex_error_color = rgb_from_plugin_color(value, rgb_format)
                    }
                    _ => {}
                }
            }
        } else if let Some(value) = line.strip_prefix("excluded_styles") {
            if let Some((_, list)) = value.split_once('=') {
                section.excluded_styles = parse_style_list(list);
            }
        } else if let Some(rule) = parse_rule_line(
            line,
            rgb_format,
            true,
            pending_name.take(),
            pending_matcher.take(),
            section.rules.len(),
        ) {
            section.rules.push(rule);
        }
    }
    if let Some(section) = current {
        if !section.lexer.eq_ignore_ascii_case("global") {
            config.sections.push(section);
        }
    }
    config
}

fn validate_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn validate_enhance_config(config: &EnhanceAnyLexerConfig) -> Result<(), String> {
    if !validate_hex_color(&config.global.regex_error_color) {
        return Err("enhance_regex_error_color_invalid".into());
    }
    let mut lexers = HashSet::new();
    for section in &config.sections {
        let lexer = section.lexer.trim();
        if lexer.is_empty() || lexer.contains(['\r', '\n', '[', ']']) {
            return Err("enhance_lexer_invalid".into());
        }
        if !lexers.insert(lexer.to_ascii_lowercase()) {
            return Err(format!("enhance_lexer_duplicate:{lexer}"));
        }
        for rule in &section.rules {
            if !validate_hex_color(&rule.color) {
                return Err(format!("enhance_rule_color_invalid:{}", rule.name));
            }
            if rule.pattern.trim().is_empty() || rule.pattern.contains(['\r', '\n']) {
                return Err(format!("enhance_rule_pattern_invalid:{}", rule.name));
            }
            if rule.name.contains(['\r', '\n']) {
                return Err("enhance_rule_name_invalid".into());
            }
        }
    }
    Ok(())
}

fn render_enhance_config(config: &EnhanceAnyLexerConfig) -> String {
    let mut output = String::new();
    output.push_str(
        "; Managed by File Sync Tool. Manual edits are supported and can be re-imported.\n",
    );
    output.push_str("; Only foreground colors are supported by EnhanceAnyLexer.\n\n");
    output.push_str("[global]\n");
    output.push_str(&format!("indicator_id={}\n", config.global.indicator_id));
    output.push_str(&format!("offset={}\n", config.global.offset));
    output.push_str(&format!(
        "regex_error_style_id={}\n",
        config.global.regex_error_style_id
    ));
    output.push_str(&format!(
        "regex_error_color={}\n",
        config.global.regex_error_color
    ));
    output.push_str("use_rgb_format=1\n");
    for section in &config.sections {
        output.push_str(&format!("\n[{}]\n", section.lexer.trim()));
        if !section.excluded_styles.is_empty() {
            output.push_str(&format!(
                "excluded_styles = {}\n",
                section
                    .excluded_styles
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        for rule in &section.rules {
            output.push_str(&format!("; FST-NAME {}\n", rule.name.trim()));
            if rule.matcher.kind != EnhanceMatcherKind::Regex {
                if let Ok(payload) = serde_json::to_string(&rule.matcher) {
                    if !payload.contains(['\r', '\n']) {
                        output.push_str(&format!("; FST-MATCH {payload}\n"));
                    }
                }
            }
            let whitelist = if rule.whitelist_styles.is_empty() {
                String::new()
            } else {
                format!(
                    "[{}]",
                    rule.whitelist_styles
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            let prefix = if rule.enabled { "" } else { "; FST-DISABLED " };
            output.push_str(&format!(
                "{}{}{} = {}\n",
                prefix,
                rule.color.to_ascii_uppercase(),
                whitelist,
                rule.pattern.trim()
            ));
        }
    }
    output
}

/// 首次进入时的示例规则，直接用关键字列表匹配器，让用户看到的就是预设形态而不是裸正则。
fn seeded_words_rule(name: &str, color: &str, terms: &[&str]) -> EnhanceAnyLexerRule {
    let matcher = EnhanceMatcher {
        kind: EnhanceMatcherKind::Words,
        terms: terms.iter().map(|term| (*term).to_string()).collect(),
        whole_word: true,
        ..EnhanceMatcher::default()
    };
    let pattern = compile_matcher(&matcher)
        .ok()
        .flatten()
        .unwrap_or_else(|| terms.join("|"));
    EnhanceAnyLexerRule {
        id: Uuid::new_v4().to_string(),
        name: name.into(),
        enabled: true,
        color: color.into(),
        pattern,
        whitelist_styles: Vec::new(),
        matcher,
    }
}

/// 保存前把预设匹配器编译进 `pattern`，保证落盘的正则与匹配器永远自洽。
fn normalize_enhance_config(config: &mut EnhanceAnyLexerConfig) -> Result<(), String> {
    for section in &mut config.sections {
        for rule in &mut section.rules {
            if let Some(pattern) = compile_matcher(&rule.matcher)? {
                rule.pattern = pattern;
            }
        }
    }
    Ok(())
}

fn enhance_config_path(exe_path: &str) -> Result<(NotepadInstance, PathBuf), String> {
    let instance = validate_instance_path(Path::new(exe_path), "manual")?;
    let path = PathBuf::from(&instance.enhance_any_lexer.config_path);
    Ok((instance, path))
}

#[tauri::command]
pub async fn notepad_extensions_read_enhance_config(
    exe_path: String,
) -> Result<EnhanceAnyLexerConfig, String> {
    tokio::task::spawn_blocking(move || {
        let (_, path) = enhance_config_path(&exe_path)?;
        if !path.is_file() {
            return Ok(EnhanceAnyLexerConfig {
                global: EnhanceAnyLexerGlobal::default(),
                sections: vec![EnhanceAnyLexerSection {
                    lexer: "normal text".into(),
                    excluded_styles: Vec::new(),
                    rules: vec![
                        seeded_words_rule("ERROR", "#EF4444", &["ERROR"]),
                        seeded_words_rule("WARN", "#F59E0B", &["WARN", "WARNING"]),
                        seeded_words_rule("INFO", "#22C55E", &["INFO"]),
                    ],
                }],
            });
        }
        let text =
            fs::read_to_string(&path).map_err(|error| format!("read_enhance_config: {error}"))?;
        Ok(parse_enhance_config(&text))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 供规则卡片实时显示「生成的正则」。编译逻辑只在 Rust 侧存在一份，避免前后端实现漂移。
#[tauri::command]
pub async fn notepad_extensions_compile_matcher(matcher: EnhanceMatcher) -> Result<String, String> {
    Ok(compile_matcher(&matcher)?.unwrap_or_default())
}

#[tauri::command]
pub async fn notepad_extensions_save_enhance_config(
    exe_path: String,
    config: EnhanceAnyLexerConfig,
) -> Result<EnhanceSaveResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut config = config;
        normalize_enhance_config(&mut config)?;
        validate_enhance_config(&config)?;
        let (instance, path) = enhance_config_path(&exe_path)?;
        let parent = path
            .parent()
            .ok_or_else(|| "enhance_config_parent_missing".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create_enhance_config_dir: {error}"))?;
        let backup_path = if path.is_file() {
            let backup = parent.join(format!(
                "{}.bak-{}",
                ENHANCE_CONFIG_NAME,
                Utc::now().format("%Y%m%d-%H%M%S")
            ));
            fs::copy(&path, &backup).map_err(|error| format!("backup_enhance_config: {error}"))?;
            Some(path_text(&backup))
        } else {
            None
        };
        let temp = parent.join(format!(".{ENHANCE_CONFIG_NAME}.{}.tmp", Uuid::new_v4()));
        let content = render_enhance_config(&config);
        let mut file =
            fs::File::create(&temp).map_err(|error| format!("write_enhance_config: {error}"))?;
        file.write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("write_enhance_config: {error}"))?;
        let replaced = parent.join(format!(
            ".{ENHANCE_CONFIG_NAME}.{}.replaced",
            Uuid::new_v4()
        ));
        if path.exists() {
            fs::rename(&path, &replaced)
                .map_err(|error| format!("replace_enhance_config: {error}"))?;
        }
        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            if replaced.exists() {
                let _ = fs::rename(&replaced, &path);
            }
            return Err(format!("replace_enhance_config: {error}"));
        }
        if replaced.exists() {
            let _ = fs::remove_file(&replaced);
        }
        Ok(EnhanceSaveResult {
            config_path: path_text(&path),
            backup_path,
            restart_required: instance.running,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_windows_extended_path_prefixes() {
        assert_eq!(
            normalize_windows_path_text(r"\\?\C:\Program Files\Notepad++\notepad++.exe"),
            r"C:\Program Files\Notepad++\notepad++.exe"
        );
        assert_eq!(
            normalize_windows_path_text(r"\\?\UNC\server\share\notepad++.exe"),
            r"\\server\share\notepad++.exe"
        );
    }

    #[test]
    fn parses_bgr_and_disabled_rules() {
        let config = parse_enhance_config(
            "[global]\nuse_rgb_format=0\nregex_error_color=0x756ce0\n\n[normal text]\n; FST-NAME Error\n0x0000FF = ERROR\n; FST-NAME Debug\n; FST-DISABLED #999999 = DEBUG\n",
        );
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].rules[0].color, "#FF0000");
        assert!(!config.sections[0].rules[1].enabled);
    }

    #[test]
    fn managed_config_round_trips() {
        let source = EnhanceAnyLexerConfig {
            global: EnhanceAnyLexerGlobal::default(),
            sections: vec![EnhanceAnyLexerSection {
                lexer: "python".into(),
                excluded_styles: vec![1, 3, 4],
                rules: vec![EnhanceAnyLexerRule {
                    id: "rule-1".into(),
                    name: "Self".into(),
                    enabled: true,
                    color: "#A855F7".into(),
                    pattern: r"\bself\b".into(),
                    whitelist_styles: vec![2],
                    matcher: EnhanceMatcher::default(),
                }],
            }],
        };
        let rendered = render_enhance_config(&source);
        let parsed = parse_enhance_config(&rendered);
        assert_eq!(parsed.sections[0].lexer, "python");
        assert_eq!(parsed.sections[0].excluded_styles, vec![1, 3, 4]);
        assert_eq!(parsed.sections[0].rules[0].name, "Self");
        assert_eq!(parsed.sections[0].rules[0].whitelist_styles, vec![2]);
    }

    #[test]
    fn rejects_duplicate_lexer_names() {
        let config = EnhanceAnyLexerConfig {
            global: EnhanceAnyLexerGlobal::default(),
            sections: vec![
                EnhanceAnyLexerSection {
                    lexer: "Python".into(),
                    excluded_styles: vec![],
                    rules: vec![],
                },
                EnhanceAnyLexerSection {
                    lexer: "python".into(),
                    excluded_styles: vec![],
                    rules: vec![],
                },
            ],
        };
        assert!(validate_enhance_config(&config).is_err());
    }

    fn words_matcher(terms: &[&str]) -> EnhanceMatcher {
        EnhanceMatcher {
            kind: EnhanceMatcherKind::Words,
            terms: terms.iter().map(|term| (*term).to_string()).collect(),
            whole_word: true,
            ..EnhanceMatcher::default()
        }
    }

    #[test]
    fn compiles_matchers_for_boost_regex() {
        assert_eq!(
            compile_matcher(&words_matcher(&["ERROR", "FATAL"])).unwrap(),
            Some(r"\b(?:ERROR|FATAL)\b".into())
        );
        // 引擎默认忽略大小写，区分大小写必须显式加反向内联标志。
        let mut sensitive = words_matcher(&["Error"]);
        sensitive.case_sensitive = true;
        assert_eq!(
            compile_matcher(&sensitive).unwrap(),
            Some(r"(?-i)\bError\b".into())
        );
        let line = EnhanceMatcher {
            kind: EnhanceMatcherKind::Line,
            ..words_matcher(&["ERROR"])
        };
        assert_eq!(
            compile_matcher(&line).unwrap(),
            Some(r"^.*\bERROR\b.*$".into())
        );
        let between = EnhanceMatcher {
            kind: EnhanceMatcherKind::Between,
            open: "\"".into(),
            close: "\"".into(),
            ..EnhanceMatcher::default()
        };
        assert_eq!(
            compile_matcher(&between).unwrap(),
            Some("\"[^\"]*\"".into())
        );
        // 原始正则模式不编译，pattern 自身就是权威值。
        assert_eq!(compile_matcher(&EnhanceMatcher::default()).unwrap(), None);
    }

    #[test]
    fn escapes_metacharacters_in_terms() {
        let matcher = EnhanceMatcher {
            whole_word: false,
            ..words_matcher(&["a.b(c)"])
        };
        assert_eq!(
            compile_matcher(&matcher).unwrap(),
            Some(r"a\.b\(c\)".into())
        );
    }

    #[test]
    fn rejects_incomplete_matchers() {
        assert!(compile_matcher(&words_matcher(&[])).is_err());
        let between = EnhanceMatcher {
            kind: EnhanceMatcherKind::Between,
            open: "\"".into(),
            ..EnhanceMatcher::default()
        };
        assert!(compile_matcher(&between).is_err());
        let preset = EnhanceMatcher {
            kind: EnhanceMatcherKind::Preset,
            preset: "nope".into(),
            ..EnhanceMatcher::default()
        };
        assert!(compile_matcher(&preset).is_err());
    }

    #[test]
    fn matcher_survives_round_trip() {
        let mut config = EnhanceAnyLexerConfig {
            global: EnhanceAnyLexerGlobal::default(),
            sections: vec![EnhanceAnyLexerSection {
                lexer: "normal text".into(),
                excluded_styles: vec![],
                rules: vec![EnhanceAnyLexerRule {
                    id: "rule-1".into(),
                    name: "Levels".into(),
                    enabled: true,
                    pattern: String::new(),
                    color: "#EF4444".into(),
                    whitelist_styles: vec![],
                    matcher: words_matcher(&["ERROR", "FATAL"]),
                }],
            }],
        };
        normalize_enhance_config(&mut config).unwrap();
        assert_eq!(config.sections[0].rules[0].pattern, r"\b(?:ERROR|FATAL)\b");

        let parsed = parse_enhance_config(&render_enhance_config(&config));
        let rule = &parsed.sections[0].rules[0];
        assert_eq!(rule.matcher.kind, EnhanceMatcherKind::Words);
        assert_eq!(rule.matcher.terms, vec!["ERROR", "FATAL"]);
    }

    #[test]
    fn hand_edited_pattern_downgrades_to_regex() {
        // 用户在 Notepad++ 里直接改了正则，注释已经对不上，必须以文件中的正则为准。
        let text = concat!(
            "[normal text]\n",
            "; FST-NAME Levels\n",
            r#"; FST-MATCH {"kind":"words","terms":["ERROR"],"whole_word":true}"#,
            "\n",
            r"#EF4444 = \bERROR|FATAL\b",
            "\n"
        );
        let parsed = parse_enhance_config(text);
        let rule = &parsed.sections[0].rules[0];
        assert_eq!(rule.matcher.kind, EnhanceMatcherKind::Regex);
        assert_eq!(rule.pattern, r"\bERROR|FATAL\b");
    }
}
