use async_tftp::packet;
use async_tftp::server::{Handler, TftpServerBuilder};
use chrono::{Local, SecondsFormat};
use futures_lite::{AsyncRead, AsyncWrite};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tauri::{State, WebviewWindow};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const MAX_EVENTS: usize = 200;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SEND_RETRIES: u32 = 20;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TftpServerConfig {
    pub root_dir: String,
    pub bind_address: String,
    pub port: u16,
    pub allow_upload: bool,
    pub allow_overwrite: bool,
    pub block_size_limit: u16,
    pub window_size_limit: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct TftpTransfer {
    pub id: u64,
    pub direction: String,
    pub client: String,
    pub file_name: String,
    pub bytes: u64,
    pub expected_bytes: Option<u64>,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TftpEvent {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub action: String,
    pub client: Option<String>,
    pub file_name: Option<String>,
    pub bytes: u64,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TftpStats {
    pub completed_downloads: u64,
    pub completed_uploads: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TftpSharedFile {
    pub relative_path: String,
    pub size: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TftpPickedFile {
    pub path: String,
    pub root_dir: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TftpServerStatus {
    pub is_active: bool,
    pub root_dir: String,
    pub bind_address: String,
    pub port: u16,
    pub allow_upload: bool,
    pub allow_overwrite: bool,
    pub block_size_limit: u16,
    pub window_size_limit: u16,
    pub uptime_secs: u64,
    pub active_transfers: Vec<TftpTransfer>,
    pub events: Vec<TftpEvent>,
    pub stats: TftpStats,
    pub last_error: Option<String>,
}

impl Default for TftpServerStatus {
    fn default() -> Self {
        Self {
            is_active: false,
            root_dir: String::new(),
            bind_address: "0.0.0.0".to_string(),
            port: 69,
            allow_upload: false,
            allow_overwrite: false,
            block_size_limit: 8192,
            window_size_limit: 8,
            uptime_secs: 0,
            active_transfers: Vec::new(),
            events: Vec::new(),
            stats: TftpStats::default(),
            last_error: None,
        }
    }
}

#[derive(Default)]
struct SharedState {
    next_id: u64,
    active: HashMap<u64, TftpTransfer>,
    events: VecDeque<TftpEvent>,
    stats: TftpStats,
    last_error: Option<String>,
}

impl SharedState {
    fn next_id(&mut self) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }

    fn push_event(&mut self, event: TftpEvent) {
        self.events.push_back(event);
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    fn server_event(&mut self, action: &str, level: &str, message: String) {
        let id = self.next_id();
        self.push_event(TftpEvent {
            id,
            timestamp: timestamp(),
            level: level.to_string(),
            action: action.to_string(),
            client: None,
            file_name: None,
            bytes: 0,
            message,
        });
    }

    fn transfer_started(
        &mut self,
        direction: &str,
        client: &SocketAddr,
        file_name: &str,
        expected_bytes: Option<u64>,
    ) -> u64 {
        let id = self.next_id();
        self.active.insert(
            id,
            TftpTransfer {
                id,
                direction: direction.to_string(),
                client: client.to_string(),
                file_name: file_name.to_string(),
                bytes: 0,
                expected_bytes,
                started_at: timestamp(),
            },
        );
        self.push_event(TftpEvent {
            id,
            timestamp: timestamp(),
            level: "info".to_string(),
            action: format!("{direction}_started"),
            client: Some(client.to_string()),
            file_name: Some(file_name.to_string()),
            bytes: 0,
            message: String::new(),
        });
        id
    }

    fn transfer_bytes(&mut self, id: u64, bytes: u64) {
        if let Some(transfer) = self.active.get_mut(&id) {
            transfer.bytes = transfer.bytes.saturating_add(bytes);
        }
    }

    fn transfer_finished(&mut self, id: u64, completed: bool, message: String) {
        let Some(transfer) = self.active.remove(&id) else {
            return;
        };
        if completed {
            match transfer.direction.as_str() {
                "download" => {
                    self.stats.completed_downloads =
                        self.stats.completed_downloads.saturating_add(1);
                    self.stats.bytes_sent = self.stats.bytes_sent.saturating_add(transfer.bytes);
                }
                "upload" => {
                    self.stats.completed_uploads = self.stats.completed_uploads.saturating_add(1);
                    self.stats.bytes_received =
                        self.stats.bytes_received.saturating_add(transfer.bytes);
                }
                _ => {}
            }
        }
        self.push_event(TftpEvent {
            id,
            timestamp: timestamp(),
            level: if completed { "success" } else { "error" }.to_string(),
            action: format!(
                "{}_{}",
                transfer.direction,
                if completed { "completed" } else { "failed" }
            ),
            client: Some(transfer.client),
            file_name: Some(transfer.file_name),
            bytes: transfer.bytes,
            message,
        });
    }

    fn request_failed(
        &mut self,
        direction: &str,
        client: &SocketAddr,
        file_name: &str,
        message: String,
    ) {
        let id = self.next_id();
        self.push_event(TftpEvent {
            id,
            timestamp: timestamp(),
            level: "error".to_string(),
            action: format!("{direction}_failed"),
            client: Some(client.to_string()),
            file_name: Some(file_name.to_string()),
            bytes: 0,
            message,
        });
    }
}

fn timestamp() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

struct Runtime {
    task: Option<JoinHandle<()>>,
    started_at: Option<Instant>,
    config: Option<TftpServerConfig>,
    bound_port: u16,
    shared: Arc<StdMutex<SharedState>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            task: None,
            started_at: None,
            config: None,
            bound_port: 69,
            shared: Arc::new(StdMutex::new(SharedState::default())),
        }
    }
}

#[derive(Default)]
pub struct TftpServerState {
    runtime: Mutex<Runtime>,
}

impl TftpServerState {
    async fn start(&self, config: TftpServerConfig) -> Result<TftpServerStatus, String> {
        validate_config(&config)?;

        let root = tokio::fs::canonicalize(config.root_dir.trim())
            .await
            .map_err(|error| format!("无法访问共享目录：{error}"))?;
        let metadata = tokio::fs::metadata(&root)
            .await
            .map_err(|error| format!("无法读取共享目录：{error}"))?;
        if !metadata.is_dir() {
            return Err("共享路径必须是目录".to_string());
        }

        let bind_ip: IpAddr = config
            .bind_address
            .trim()
            .parse()
            .map_err(|_| "监听地址格式无效".to_string())?;
        let bind_addr = SocketAddr::new(bind_ip, config.port);

        let socket = tokio::task::spawn_blocking(move || UdpSocket::bind(bind_addr))
            .await
            .map_err(|error| format!("创建 TFTP 监听任务失败：{error}"))?
            .map_err(|error| format!("无法监听 UDP {}：{error}", bind_addr))?;
        let bound_port = socket
            .local_addr()
            .map_err(|error| format!("无法读取 TFTP 监听地址：{error}"))?
            .port();

        let mut runtime = self.runtime.lock().await;
        if runtime
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Err("TFTP 服务已在运行".to_string());
        }
        if let Some(task) = runtime.task.take() {
            task.abort();
        }

        let normalized_config = TftpServerConfig {
            root_dir: root.to_string_lossy().to_string(),
            bind_address: bind_ip.to_string(),
            port: config.port,
            ..config
        };
        let shared = Arc::clone(&runtime.shared);
        let handler = TftpHandler {
            root,
            allow_upload: normalized_config.allow_upload,
            allow_overwrite: normalized_config.allow_overwrite,
            shared: Arc::clone(&shared),
        };
        let server = TftpServerBuilder::with_handler(handler)
            .std_socket(socket)
            .map_err(|error| format!("初始化 TFTP 套接字失败：{error}"))?
            .timeout(DEFAULT_TIMEOUT)
            .max_send_retries(MAX_SEND_RETRIES)
            .block_size_limit(normalized_config.block_size_limit)
            .window_size_limit(normalized_config.window_size_limit)
            .build()
            .await
            .map_err(|error| format!("启动 TFTP 服务失败：{error}"))?;

        {
            let mut state = shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.last_error = None;
            state.server_event(
                "server_started",
                "success",
                format!(
                    "{}:{} · {}",
                    normalized_config.bind_address, bound_port, normalized_config.root_dir
                ),
            );
        }

        runtime.bound_port = bound_port;
        runtime.started_at = Some(Instant::now());
        runtime.config = Some(normalized_config);
        runtime.task = Some(tokio::spawn(async move {
            if let Err(error) = server.serve().await {
                let mut state = shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let message = error.to_string();
                state.last_error = Some(message.clone());
                state.server_event("server_failed", "error", message);
            }
        }));
        drop(runtime);
        Ok(self.status().await)
    }

    async fn stop(&self) -> Result<TftpServerStatus, String> {
        let (task, shared) = {
            let mut runtime = self.runtime.lock().await;
            let task = runtime.task.take();
            runtime.started_at = None;
            (task, Arc::clone(&runtime.shared))
        };
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
            let mut state = shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.server_event("server_stopped", "info", String::new());
        }
        Ok(self.status().await)
    }

    async fn status(&self) -> TftpServerStatus {
        let runtime = self.runtime.lock().await;
        let is_active = runtime
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        let mut status = TftpServerStatus::default();
        if let Some(config) = &runtime.config {
            status.root_dir = config.root_dir.clone();
            status.bind_address = config.bind_address.clone();
            status.allow_upload = config.allow_upload;
            status.allow_overwrite = config.allow_overwrite;
            status.block_size_limit = config.block_size_limit;
            status.window_size_limit = config.window_size_limit;
        }
        status.port = runtime.bound_port;
        status.is_active = is_active;
        status.uptime_secs = runtime
            .started_at
            .filter(|_| is_active)
            .map(|started| started.elapsed().as_secs())
            .unwrap_or(0);

        let shared = runtime
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.active_transfers = shared.active.values().cloned().collect();
        status.active_transfers.sort_by_key(|transfer| transfer.id);
        status.events = shared.events.iter().cloned().collect();
        status.stats = shared.stats.clone();
        status.last_error = shared.last_error.clone();
        status
    }
}

fn validate_config(config: &TftpServerConfig) -> Result<(), String> {
    if config.root_dir.trim().is_empty() {
        return Err("请选择共享目录".to_string());
    }
    if !(512..=65464).contains(&config.block_size_limit) {
        return Err("最大块大小必须在 512 到 65464 字节之间".to_string());
    }
    if !(1..=64).contains(&config.window_size_limit) {
        return Err("窗口大小必须在 1 到 64 之间".to_string());
    }
    if config.allow_overwrite && !config.allow_upload {
        return Err("允许覆盖前必须先允许设备上传".to_string());
    }
    Ok(())
}

struct TftpHandler {
    root: PathBuf,
    allow_upload: bool,
    allow_overwrite: bool,
    shared: Arc<StdMutex<SharedState>>,
}

impl Handler for TftpHandler {
    type Reader = TrackedReader;
    type Writer = TrackedWriter;

    async fn read_req_open(
        &mut self,
        client: &SocketAddr,
        requested: &Path,
    ) -> Result<(Self::Reader, Option<u64>), packet::Error> {
        let file_name = requested.to_string_lossy().to_string();
        let path = match resolve_read_path(&self.root, requested).await {
            Ok(path) => path,
            Err((error, message)) => {
                self.shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .request_failed("download", client, &file_name, message);
                return Err(error);
            }
        };
        let file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) => {
                let packet_error = packet::Error::from(error);
                self.shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .request_failed("download", client, &file_name, "无法打开文件".to_string());
                return Err(packet_error);
            }
        };
        let size = file.metadata().await.ok().map(|metadata| metadata.len());
        let id = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .transfer_started("download", client, &file_name, size);
        Ok((
            TrackedReader::new(file.compat(), id, Arc::clone(&self.shared)),
            size,
        ))
    }

    async fn write_req_open(
        &mut self,
        client: &SocketAddr,
        requested: &Path,
        size: Option<u64>,
    ) -> Result<Self::Writer, packet::Error> {
        let file_name = requested.to_string_lossy().to_string();
        if !self.allow_upload {
            self.shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .request_failed("upload", client, &file_name, "设备上传未启用".to_string());
            return Err(packet::Error::PermissionDenied);
        }
        let path = match resolve_write_path(&self.root, requested, self.allow_overwrite).await {
            Ok(path) => path,
            Err((error, message)) => {
                self.shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .request_failed("upload", client, &file_name, message);
                return Err(error);
            }
        };
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(!self.allow_overwrite);
        if self.allow_overwrite {
            options.create(true).truncate(true);
        }
        let file = match options.open(&path).await {
            Ok(file) => file,
            Err(error) => {
                let packet_error = packet::Error::from(error);
                self.shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .request_failed("upload", client, &file_name, "无法创建文件".to_string());
                return Err(packet_error);
            }
        };
        if let Some(size) = size {
            if let Err(error) = file.set_len(size).await {
                return Err(packet::Error::from(error));
            }
        }
        let id = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .transfer_started("upload", client, &file_name, size);
        Ok(TrackedWriter::new(
            file.compat_write(),
            id,
            Arc::clone(&self.shared),
        ))
    }
}

fn relative_request_path(requested: &Path) -> Result<PathBuf, packet::Error> {
    let mut relative = PathBuf::new();
    for component in requested.components() {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => return Err(packet::Error::PermissionDenied),
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(packet::Error::FileNotFound);
    }
    Ok(relative)
}

async fn resolve_read_path(
    root: &Path,
    requested: &Path,
) -> Result<PathBuf, (packet::Error, String)> {
    let relative = relative_request_path(requested)
        .map_err(|error| (error, "文件路径超出共享目录".to_string()))?;
    let path = tokio::fs::canonicalize(root.join(relative))
        .await
        .map_err(|error| {
            (
                packet::Error::from(error),
                "文件不存在或不可访问".to_string(),
            )
        })?;
    if !path.starts_with(root) {
        return Err((
            packet::Error::PermissionDenied,
            "文件路径超出共享目录".to_string(),
        ));
    }
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| (packet::Error::from(error), "文件不可访问".to_string()))?;
    if !metadata.is_file() {
        return Err((packet::Error::FileNotFound, "请求目标不是文件".to_string()));
    }
    Ok(path)
}

async fn resolve_write_path(
    root: &Path,
    requested: &Path,
    allow_overwrite: bool,
) -> Result<PathBuf, (packet::Error, String)> {
    let relative = relative_request_path(requested)
        .map_err(|error| (error, "文件路径超出共享目录".to_string()))?;
    let candidate = root.join(relative);
    if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
        if !allow_overwrite {
            return Err((
                packet::Error::FileAlreadyExists,
                "同名文件已存在，未允许覆盖".to_string(),
            ));
        }
        let canonical = tokio::fs::canonicalize(&candidate)
            .await
            .map_err(|error| (packet::Error::from(error), "文件不可访问".to_string()))?;
        if !canonical.starts_with(root) {
            return Err((
                packet::Error::PermissionDenied,
                "文件路径超出共享目录".to_string(),
            ));
        }
        return Ok(canonical);
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| (packet::Error::PermissionDenied, "文件路径无效".to_string()))?;
    let canonical_parent = tokio::fs::canonicalize(parent)
        .await
        .map_err(|error| (packet::Error::from(error), "目标目录不存在".to_string()))?;
    if !canonical_parent.starts_with(root) {
        return Err((
            packet::Error::PermissionDenied,
            "文件路径超出共享目录".to_string(),
        ));
    }
    let file_name = candidate
        .file_name()
        .ok_or_else(|| (packet::Error::PermissionDenied, "文件路径无效".to_string()))?;
    Ok(canonical_parent.join(file_name))
}

struct TransferTracker {
    id: u64,
    shared: Arc<StdMutex<SharedState>>,
    completed: bool,
}

impl TransferTracker {
    fn new(id: u64, shared: Arc<StdMutex<SharedState>>) -> Self {
        Self {
            id,
            shared,
            completed: false,
        }
    }

    fn add_bytes(&self, bytes: usize) {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .transfer_bytes(self.id, bytes as u64);
    }

    fn complete(&mut self) {
        if self.completed {
            return;
        }
        self.completed = true;
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .transfer_finished(self.id, true, String::new());
    }
}

impl Drop for TransferTracker {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .transfer_finished(self.id, false, "传输中断".to_string());
    }
}

pub struct TrackedReader {
    inner: Compat<tokio::fs::File>,
    tracker: TransferTracker,
}

impl TrackedReader {
    fn new(inner: Compat<tokio::fs::File>, id: u64, shared: Arc<StdMutex<SharedState>>) -> Self {
        Self {
            inner,
            tracker: TransferTracker::new(id, shared),
        }
    }
}

impl AsyncRead for TrackedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_read(context, buffer) {
            Poll::Ready(Ok(0)) => {
                self.tracker.complete();
                Poll::Ready(Ok(0))
            }
            Poll::Ready(Ok(bytes)) => {
                self.tracker.add_bytes(bytes);
                Poll::Ready(Ok(bytes))
            }
            result => result,
        }
    }
}

pub struct TrackedWriter {
    inner: Compat<tokio::fs::File>,
    tracker: TransferTracker,
}

impl TrackedWriter {
    fn new(inner: Compat<tokio::fs::File>, id: u64, shared: Arc<StdMutex<SharedState>>) -> Self {
        Self {
            inner,
            tracker: TransferTracker::new(id, shared),
        }
    }
}

impl AsyncWrite for TrackedWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(context, buffer) {
            Poll::Ready(Ok(bytes)) => {
                self.tracker.add_bytes(bytes);
                Poll::Ready(Ok(bytes))
            }
            result => result,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_close(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.inner).poll_close(context) {
            Poll::Ready(Ok(())) => {
                self.tracker.complete();
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}

#[tauri::command]
pub async fn tftp_server_pick_directory() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        Ok(rfd::FileDialog::new()
            .pick_folder()
            .map(|path| path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|error| format!("打开目录选择器失败：{error}"))?
}

#[tauri::command]
pub async fn tftp_server_pick_file(
    window: WebviewWindow,
) -> Result<Option<TftpPickedFile>, String> {
    let picked = crate::run_dialog_task_on_main_thread(&window, || {
        Ok(rfd::FileDialog::new()
            .set_title("选择要上传到设备的文件 / Select File for Device")
            .pick_file())
    })
    .await?;

    let Some(path) = picked else {
        return Ok(None);
    };
    let root_dir = path
        .parent()
        .ok_or_else(|| "无法读取所选文件所在目录".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "无法读取所选文件名".to_string())?;
    Ok(Some(TftpPickedFile {
        path: path.to_string_lossy().to_string(),
        root_dir: root_dir.to_string_lossy().to_string(),
        file_name: file_name.to_string_lossy().to_string(),
    }))
}

#[tauri::command]
pub async fn tftp_server_list_files(root_dir: String) -> Result<Vec<TftpSharedFile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::fs::canonicalize(root_dir.trim())
            .map_err(|error| format!("无法访问共享目录：{error}"))?;
        if !root.is_dir() {
            return Err("共享路径必须是目录".to_string());
        }
        let mut files = Vec::new();
        collect_shared_files(&root, &root, 0, &mut files)?;
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(files)
    })
    .await
    .map_err(|error| format!("读取共享目录任务失败：{error}"))?
}

fn collect_shared_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<TftpSharedFile>,
) -> Result<(), String> {
    if depth > 8 || files.len() >= 500 {
        return Ok(());
    }
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("无法读取目录 {}：{error}", directory.display()))?;
    for entry in entries {
        if files.len() >= 500 {
            break;
        }
        let entry = entry.map_err(|error| format!("无法读取目录项：{error}"))?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("无法读取文件信息：{error}"))?;
        if entry
            .file_type()
            .map(|kind| kind.is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        let path = entry.path();
        if metadata.is_dir() {
            collect_shared_files(root, &path, depth + 1, files)?;
        } else if metadata.is_file() {
            let relative_path = path
                .strip_prefix(root)
                .map_err(|_| "文件路径超出共享目录".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let modified_at = metadata.modified().ok().map(|value| {
                chrono::DateTime::<Local>::from(value).to_rfc3339_opts(SecondsFormat::Secs, false)
            });
            files.push(TftpSharedFile {
                relative_path,
                size: metadata.len(),
                modified_at,
            });
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn tftp_server_start(
    config: TftpServerConfig,
    state: State<'_, TftpServerState>,
) -> Result<TftpServerStatus, String> {
    state.start(config).await
}

#[tauri::command]
pub async fn tftp_server_stop(
    state: State<'_, TftpServerState>,
) -> Result<TftpServerStatus, String> {
    state.stop().await
}

#[tauri::command]
pub async fn tftp_server_get_status(
    state: State<'_, TftpServerState>,
) -> Result<TftpServerStatus, String> {
    Ok(state.status().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::time::{sleep, timeout};

    fn test_config(root: &Path) -> TftpServerConfig {
        TftpServerConfig {
            root_dir: root.to_string_lossy().to_string(),
            bind_address: "127.0.0.1".to_string(),
            port: 0,
            allow_upload: false,
            allow_overwrite: false,
            block_size_limit: 8192,
            window_size_limit: 8,
        }
    }

    #[tokio::test]
    async fn rejects_paths_outside_the_shared_root() {
        let root = tempdir().unwrap();
        let root = tokio::fs::canonicalize(root.path()).await.unwrap();
        let result = resolve_read_path(&root, Path::new("../secret.bin")).await;
        assert!(matches!(result, Err((packet::Error::PermissionDenied, _))));
    }

    #[tokio::test]
    async fn serves_a_tftp_read_request_and_records_stats() {
        let root = tempdir().unwrap();
        tokio::fs::write(root.path().join("firmware.bin"), b"firmware-data")
            .await
            .unwrap();
        let state = TftpServerState::default();
        let started = state.start(test_config(root.path())).await.unwrap();
        assert!(started.is_active);
        assert_ne!(started.port, 0);

        let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut request = vec![0, 1];
        request.extend_from_slice(b"firmware.bin\0octet\0");
        client
            .send_to(&request, ("127.0.0.1", started.port))
            .await
            .unwrap();

        let mut response = [0_u8; 1024];
        let (length, transfer_addr) =
            timeout(Duration::from_secs(3), client.recv_from(&mut response))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(&response[..4], &[0, 3, 0, 1]);
        assert_eq!(&response[4..length], b"firmware-data");
        client.send_to(&[0, 4, 0, 1], transfer_addr).await.unwrap();

        for _ in 0..20 {
            if state.status().await.stats.completed_downloads == 1 {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
        let status = state.status().await;
        assert_eq!(status.stats.completed_downloads, 1);
        assert_eq!(status.stats.bytes_sent, 13);
        state.stop().await.unwrap();
    }
}
