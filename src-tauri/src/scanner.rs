use crate::config::{AppConfig, MatchRule, TaskServerBinding};
use crate::deploy::deploy_to_remote;
use crate::history::{add_history_entry, HistoryEntry};
use chrono::{Duration, Local, NaiveDateTime, NaiveTime, Timelike};
use flate2::write::GzEncoder;
use flate2::Compression;
use regex::Regex;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant, SystemTime};
use tauri::{Emitter, Manager};
use tokio::fs;

#[derive(Debug, serde::Serialize, Clone)]
pub struct ScanResult {
    pub scanned_paths: usize,
    pub found_folders: Vec<String>,
    pub copied_folders: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, serde::Serialize, Clone)]
struct LogEvent {
    msg: String,
    level: String,
}

#[derive(Debug, serde::Serialize, Clone)]
struct ProgressEvent {
    folder: String,
    total_bytes: u64,
    copied_bytes: u64,
    percentage: f64,
    speed: u64, // bytes per second
    eta_seconds: u64,
    elapsed_seconds: u64,
    local_path: String,
    remote_path: String,
    source: String, // "manual" or "scheduled"
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    name: String,
    version: String,
    datetime: NaiveDateTime,
}

// Global mutex to serialize log rotation + write, preventing concurrent rotation races
static LOG_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
fn get_log_mutex() -> &'static Mutex<()> {
    LOG_MUTEX.get_or_init(|| Mutex::new(()))
}

/// Compress `src` into a gzip file at `dst`.
fn compress_log(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(src)?;
    let output = std::fs::File::create(dst)?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    std::io::copy(&mut input, &mut encoder)?;
    encoder.finish()?;
    Ok(())
}

/// Rotate log files if `log_path` has reached 5 MB.
/// Keeps up to MAX_ROTATED compressed files; deletes the oldest when exceeded.
fn rotate_log_if_needed(log_path: &Path) {
    const MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB
    const MAX_ROTATED: u32 = 5;

    let size = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
    if size < MAX_SIZE {
        return;
    }

    let dir = match log_path.parent() {
        Some(d) => d,
        None => return,
    };
    let base = log_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Delete oldest rotated file if it exists
    let oldest = dir.join(format!("{}.{}.gz", base, MAX_ROTATED));
    let _ = std::fs::remove_file(&oldest);

    // Shift: app.log.4.gz -> app.log.5.gz, ..., app.log.1.gz -> app.log.2.gz
    for n in (1..MAX_ROTATED).rev() {
        let from = dir.join(format!("{}.{}.gz", base, n));
        let to = dir.join(format!("{}.{}.gz", base, n + 1));
        if from.exists() {
            let _ = std::fs::rename(&from, &to);
        }
    }

    // Compress current log -> app.log.1.gz
    let gz_path = dir.join(format!("{}.1.gz", base));
    if compress_log(log_path, &gz_path).is_ok() {
        // Remove the original log so a fresh one is created on next write
        let _ = std::fs::remove_file(log_path);
    }
}

/// Write a log entry to the app log file. Thread-safe. Used by both scanner and deploy modules.
pub fn write_log_to_file<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    msg: &str,
    level: &str,
) {
    if let Ok(app_dir) = app_handle.path().app_data_dir() {
        if std::fs::create_dir_all(&app_dir).is_ok() {
            let log_path = app_dir.join("app.log");
            let _guard = get_log_mutex().lock().unwrap();
            rotate_log_if_needed(&log_path);
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
                let time = Local::now().format("%Y-%m-%d %H:%M:%S");
                let _ = writeln!(file, "[{}] [{}] {}", time, level.to_uppercase(), msg);
            }
        }
    }
}

// Helper to emit logs to frontend in real-time
fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit(
        "log-message",
        LogEvent {
            msg: msg.clone(),
            level: level.to_string(),
        },
    );
    write_log_to_file(app_handle, &msg, level);
}

fn emit_progress<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    folder: &str,
    copied: u64,
    total: u64,
    speed: u64,
    eta_seconds: u64,
    elapsed_seconds: u64,
    local_path: &str,
    remote_path: &str,
    source: &str,
) {
    let percentage = if total > 0 {
        (copied as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let _ = app_handle.emit(
        "copy-progress",
        ProgressEvent {
            folder: folder.to_string(),
            total_bytes: total,
            copied_bytes: copied,
            percentage,
            speed,
            eta_seconds,
            elapsed_seconds,
            local_path: local_path.to_string(),
            remote_path: remote_path.to_string(),
            source: source.to_string(),
        },
    );
}

// Helper function to copy file with chunking and interruption support
fn copy_file_chunked<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    should_cancel: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64), // bytes copied delta
) -> Result<u64, String> {
    let mut file_in = std::fs::File::open(from).map_err(|e| e.to_string())?;
    let mut file_out = std::fs::File::create(to).map_err(|e| e.to_string())?;

    let mut buffer = vec![0u8; buffer_size];
    let mut total_copied = 0;

    loop {
        // Check cancel
        if should_cancel.load(Ordering::SeqCst) {
            return Err("Cancelled by user".to_string());
        }

        // Check pause
        while is_paused.load(Ordering::SeqCst) {
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Cancelled by user".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let n = file_in.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break; // EOF
        }

        file_out
            .write_all(&buffer[..n])
            .map_err(|e| e.to_string())?;
        total_copied += n as u64;
        on_progress(n as u64);
    }

    Ok(total_copied)
}

fn build_temp_copy_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("copy-target");

    target.with_file_name(format!(".{}.{}.part", file_name, uuid::Uuid::new_v4()))
}

fn copy_file_with_overwrite_mode<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    overwrite_existing: bool,
    should_cancel: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64),
) -> Result<u64, String> {
    let target = to.as_ref();

    if overwrite_existing && target.exists() {
        if !target.is_file() {
            return Err(format!(
                "Target path is not a file and cannot be overwritten: {}",
                target.display()
            ));
        }

        let temp_target = build_temp_copy_path(target);
        let copy_result = copy_file_chunked(
            from,
            &temp_target,
            should_cancel,
            is_paused,
            buffer_size,
            on_progress,
        );

        match copy_result {
            Ok(bytes_copied) => {
                if let Err(error) = std::fs::remove_file(target) {
                    let _ = std::fs::remove_file(&temp_target);
                    return Err(error.to_string());
                }
                if let Err(error) = std::fs::rename(&temp_target, target) {
                    let _ = std::fs::remove_file(&temp_target);
                    return Err(error.to_string());
                }
                Ok(bytes_copied)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&temp_target);
                Err(error)
            }
        }
    } else {
        copy_file_chunked(
            from,
            target,
            should_cancel,
            is_paused,
            buffer_size,
            on_progress,
        )
    }
}

// Extracted copy logic to reuse across different matching rules
async fn perform_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    source_path: PathBuf,
    folder_name: String,
    target_parent_path: &Path,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    overwrite_existing: bool,
    result: &mut ScanResult,
    task_id: Option<String>,
    allow_deploy: bool,
    source: &str,
    filter_extensions: &[String],
    filter_includes: &[String],
) {
    let target_full_path = target_parent_path.join(&folder_name);

    if target_full_path.exists() && !target_full_path.is_dir() {
        let err_msg = format!(
            "Target local path already exists as a file and cannot be used as a directory: {}",
            target_full_path.display()
        );
        emit_log(app_handle, err_msg.clone(), "error");
        result.errors.push(err_msg);
        return;
    }

    emit_log(
        app_handle,
        format!("Target local directory: {}", target_full_path.display()),
        "info",
    );

    // Check if target directory exists, but don't skip entire copy - check for new files
    if target_full_path.exists() {
        emit_log(
            app_handle,
            if overwrite_existing {
                format!(
                    "Target directory {} exists. Matching files may be overwritten and missing files will still be copied.",
                    target_full_path.display()
                )
            } else {
                format!(
                    "Target directory {} exists. Checking for new files...",
                    target_full_path.display()
                )
            },
            "info",
        );
    } else {
        emit_log(
            app_handle,
            format!(
                "Starting copy: {} -> {}",
                source_path.display(),
                target_parent_path.display()
            ),
            "info",
        );
    }

    // Ensure parent dir exists
    if let Err(e) = fs::create_dir_all(target_parent_path).await {
        let err_msg = format!(
            "Failed to create local directory {}: {}",
            target_parent_path.display(),
            e
        );
        emit_log(app_handle, err_msg.clone(), "error");
        result.errors.push(err_msg);
        return;
    }

    let app_handle_clone = app_handle.clone();
    let folder_name_clone = folder_name.clone();
    let source_path_clone = source_path.clone();
    let target_full_path_clone = target_full_path.clone();

    // Clone filter parameters for closure
    let extensions = filter_extensions.to_vec();
    let includes = filter_includes.to_vec();
    let stability_check_secs = config.stability_check_secs;
    let recent_file_guard_mins = config.recent_file_guard_mins;
    let copy_buffer_size = (config.copy_buffer_size_kb as usize).max(64) * 1024;
    let should_cancel_clone = should_cancel.clone();
    let is_paused_clone = is_paused.clone();
    let live_config_clone = live_config.clone();
    let task_id_clone = task_id.clone();
    let source_clone = source.to_string();

    let copy_task = tauri::async_runtime::spawn_blocking(move || {
        let handle = app_handle_clone;

        // Prepare paths for display (needed later for progress events)
        let local_path_display = target_full_path_clone.to_string_lossy().to_string();
        let remote_path_display = source_path_clone.to_string_lossy().to_string();

        // Just test access to source dir
        if let Err(e) = std::fs::read_dir(&source_path_clone) {
            let e = e.to_string();
            emit_log(
                &handle,
                format!("Failed to access source dir: {}", e),
                "error",
            );
            return Err(fs_extra::error::Error::new(
                fs_extra::error::ErrorKind::Other,
                &e,
            ));
        }

        // Log active filter rules
        let has_ext_filter = !extensions.is_empty();
        let has_inc_filter = !includes.is_empty();
        if has_ext_filter || has_inc_filter {
            let mut parts = Vec::new();
            if has_ext_filter {
                parts.push(format!("extensions=[{}]", extensions.join(", ")));
            }
            if has_inc_filter {
                parts.push(format!("keywords=[{}]", includes.join(", ")));
            }
            emit_log(
                &handle,
                format!("Active filter rules for '{}': {}", folder_name_clone, parts.join("; ")),
                "info",
            );
        } else {
            emit_log(
                &handle,
                format!("No filter rules active for '{}' — all files will be considered.", folder_name_clone),
                "info",
            );
        }

        // Collect files with filtering (Iterative)
        let mut filtered_files: Vec<(PathBuf, u64, bool)> = Vec::new();
        let mut total_files_scanned: u64 = 0;
        let mut skipped_by_ext: Vec<String> = Vec::new();
        let mut skipped_by_keyword: Vec<String> = Vec::new();
        let mut skipped_existing: u64 = 0;
        let recent_file_guard_secs = recent_file_guard_mins * 60;
        let now_system = SystemTime::now();

        let mut dirs_to_visit = vec![source_path_clone.clone()];
        while let Some(current_dir) = dirs_to_visit.pop() {
            if let Ok(entries) = std::fs::read_dir(&current_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        dirs_to_visit.push(path);
                    } else {
                        total_files_scanned += 1;
                        // File Check
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        let mut ext_match = true;
                        if !extensions.is_empty() {
                            let name_lower = file_name.to_lowercase();
                            let mut any_match = false;
                            for configured_ext in &extensions {
                                let conf_lower = configured_ext.to_lowercase();
                                let suffix = if conf_lower.starts_with('.') {
                                    conf_lower.clone()
                                } else {
                                    format!(".{}", conf_lower)
                                };

                                if name_lower.ends_with(&suffix) {
                                    any_match = true;
                                    break;
                                }
                            }

                            if !any_match {
                                ext_match = false;
                            }
                        }

                        let mut inc_match = true;
                        if !includes.is_empty() {
                            inc_match = false;
                            for inc in &includes {
                                if file_name.contains(inc) {
                                    inc_match = true;
                                    break;
                                }
                            }
                        }

                        if !ext_match {
                            if skipped_by_ext.len() < 20 {
                                skipped_by_ext.push(file_name.clone());
                            }
                            continue;
                        }
                        if !inc_match {
                            if skipped_by_keyword.len() < 20 {
                                skipped_by_keyword.push(file_name.clone());
                            }
                            continue;
                        }

                        let rel_path = path.strip_prefix(&source_path_clone).unwrap_or(&path);
                        let dst = target_full_path_clone.join(rel_path);

                        if dst.exists() && !dst.is_file() {
                            emit_log(
                                &handle,
                                format!(
                                    "Skipping '{}' because target path exists as a directory: {}",
                                    file_name,
                                    dst.display()
                                ),
                                "warn",
                            );
                            continue;
                        }

                        if overwrite_existing || !dst.exists() {
                            if let Ok(meta) = entry.metadata() {
                                let file_size = meta.len();
                                let is_recent = meta
                                    .modified()
                                    .ok()
                                    .and_then(|modified| {
                                        now_system.duration_since(modified).ok()
                                    })
                                    .map(|age| {
                                        age < StdDuration::from_secs(recent_file_guard_secs)
                                    })
                                    .unwrap_or(true);

                                filtered_files.push((path, file_size, is_recent));
                            }
                        } else {
                            skipped_existing += 1;
                        }
                    }
                }
            }
        }

        // Log filtering summary
        let matched_count = filtered_files.len() as u64;
        let ext_skipped = skipped_by_ext.len() as u64;
        let kw_skipped = skipped_by_keyword.len() as u64;
        emit_log(
            &handle,
            format!(
                "Scan summary for '{}': {} file(s) found, {} matched filters, {} skipped by extension, {} skipped by keyword, {} already exist locally.",
                folder_name_clone, total_files_scanned, matched_count, ext_skipped, kw_skipped, skipped_existing
            ),
            "info",
        );
        if !skipped_by_ext.is_empty() {
            emit_log(
                &handle,
                format!("Skipped by extension filter: {}", skipped_by_ext.join(", ")),
                "info",
            );
        }
        if !skipped_by_keyword.is_empty() {
            emit_log(
                &handle,
                format!("Skipped by keyword filter: {}", skipped_by_keyword.join(", ")),
                "info",
            );
        }

        if filtered_files.is_empty() {
            emit_log(
                &handle,
                format!(
                    "'{}' is up to date — no new files to copy.",
                    folder_name_clone
                ),
                "info",
            );
            return Ok(0u64);
        }

        // --- Stability check ---
        // Only files modified within the configured recent-file window enter the waiting flow.
        // Older files are copied directly; recent files wait `stability_check_secs` then re-check size.
        let mut files_ready_now: Vec<(PathBuf, u64)> = filtered_files
            .iter()
            .filter(|(_, _, is_recent)| !*is_recent)
            .map(|(path, size, _)| (path.clone(), *size))
            .collect();
        let recent_files: Vec<(PathBuf, u64)> = filtered_files
            .into_iter()
            .filter(|(_, _, is_recent)| *is_recent)
            .map(|(path, size, _)| (path, size))
            .collect();

        if !files_ready_now.is_empty() {
            emit_log(
                &handle,
                format!(
                    "{} file(s) were last modified over {} minute(s) ago and will be copied immediately.",
                    files_ready_now.len(),
                    recent_file_guard_mins
                ),
                "info",
            );
        }

        if stability_check_secs > 0 && !recent_files.is_empty() {
            emit_log(
                &handle,
                format!(
                    "Waiting {}s to verify {} recently modified file(s) are fully written...",
                    stability_check_secs,
                    recent_files.len()
                ),
                "info",
            );
            let intervals = stability_check_secs * 5;
            for _ in 0..intervals {
                if should_cancel_clone.load(Ordering::SeqCst) {
                    return Err(fs_extra::error::Error::new(
                        fs_extra::error::ErrorKind::Interrupted,
                        "Cancelled by user",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }

            for (path, original_size) in recent_files {
                match std::fs::metadata(&path) {
                    Ok(meta) => {
                        let current_size = meta.len();
                        if current_size == original_size {
                            files_ready_now.push((path, original_size));
                        } else {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            emit_log(
                                &handle,
                                format!(
                                    "Skipping '{}' — size changed ({} -> {} bytes), will retry next scan",
                                    name, original_size, current_size
                                ),
                                "warn",
                            );
                        }
                    }
                    Err(e) => {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        emit_log(&handle, format!("Cannot stat '{}': {}", name, e), "warn");
                    }
                }
            }
        } else if !recent_files.is_empty() {
            files_ready_now.extend(recent_files);
        }

        let filtered_files: Vec<(PathBuf, u64)> = files_ready_now;
        let total_filtered_bytes: u64 = filtered_files.iter().map(|(_, s)| *s).sum();

        if filtered_files.is_empty() {
            emit_log(
                &handle,
                "All recent candidate files are still being written. Will retry next scan."
                    .to_string(),
                "warn",
            );
            return Ok(0u64);
        }
        emit_log(
            &handle,
            format!(
                "{} file(s) confirmed ready, proceeding with copy.",
                filtered_files.len()
            ),
            "info",
        );

        // ---- Confirmed: we have files to copy ----
        // Record COPY_STARTED only now, so history is clean when nothing needs copying.
        add_history_entry(
            &handle,
            HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Local::now().to_rfc3339(),
                action_type: "COPY_STARTED".to_string(),
                description: format!("Started copying {}", folder_name_clone),
                folder_name: folder_name_clone.clone(),
                source_path: source_path_clone.to_string_lossy().to_string(),
                target_path: target_full_path_clone.to_string_lossy().to_string(),
                copied_files_count: 0,
                total_size: 0,
                files: vec![],
            },
        );

        let start_time = Instant::now();
        let mut last_emit_time = Instant::now();

        // Helper for speed/eta progress events
        let mut update_stats = |copied: u64, total: u64| {
            let now = Instant::now();
            if now.duration_since(last_emit_time).as_millis() > 500 || copied == total {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    (copied as f64 / elapsed) as u64
                } else {
                    0
                };
                let eta = if speed > 0 && total > copied {
                    (total - copied) / speed
                } else {
                    0
                };
                emit_progress(
                    &handle,
                    &folder_name_clone,
                    copied,
                    total,
                    speed,
                    eta,
                    elapsed as u64,
                    &local_path_display,
                    &remote_path_display,
                    &source_clone,
                );
                last_emit_time = now;
            }
        };

        emit_log(
            &handle,
            format!(
                "Copying {} file(s) ({} bytes) from '{}'...",
                filtered_files.len(),
                total_filtered_bytes,
                folder_name_clone
            ),
            "info",
        );

        // Create target directory structure and Copy
        let mut copied_bytes_total = 0;
        let mut copied_files_list = Vec::new();

        for (src, _size) in filtered_files {
            // Check cancel before starting file
            if should_cancel_clone.load(Ordering::SeqCst) {
                // Log partial
                if !copied_files_list.is_empty() {
                    add_history_entry(
                        &handle,
                        HistoryEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            timestamp: Local::now().to_rfc3339(),
                            action_type: "COPY_CANCELLED".to_string(),
                            description: format!("Cancelled copying {}", folder_name_clone),
                            folder_name: format!("{} (Cancelled)", folder_name_clone),
                            source_path: source_path_clone.to_string_lossy().to_string(),
                            target_path: target_full_path_clone.to_string_lossy().to_string(),
                            copied_files_count: copied_files_list.len(),
                            total_size: copied_bytes_total,
                            files: copied_files_list.clone(),
                        },
                    );
                }
                return Err(fs_extra::error::Error::new(
                    fs_extra::error::ErrorKind::Interrupted,
                    "Cancelled by user",
                ));
            }

            // Calculate relative path
            let rel_path = src.strip_prefix(&source_path_clone).unwrap_or(&src);
            let dst = target_full_path_clone.join(rel_path);

            // Create parent dir
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let file_name_display = src
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // Copy with chunking
            let copy_res = copy_file_with_overwrite_mode(
                &src,
                &dst,
                overwrite_existing,
                &should_cancel_clone,
                &is_paused_clone,
                copy_buffer_size,
                &mut |delta| {
                    copied_bytes_total += delta;
                    update_stats(copied_bytes_total, total_filtered_bytes);
                },
            );

            match copy_res {
                Ok(_) => {
                    copied_files_list.push(file_name_display);
                }
                Err(e) => {
                    if e.contains("Cancelled") {
                        // Save partial
                        if !copied_files_list.is_empty() {
                            add_history_entry(
                                &handle,
                                HistoryEntry {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    timestamp: Local::now().to_rfc3339(),
                                    action_type: "COPY_CANCELLED".to_string(),
                                    description: format!("Cancelled copying {}", folder_name_clone),
                                    folder_name: format!("{} (Cancelled)", folder_name_clone),
                                    source_path: source_path_clone.to_string_lossy().to_string(),
                                    target_path: target_full_path_clone
                                        .to_string_lossy()
                                        .to_string(),
                                    copied_files_count: copied_files_list.len(),
                                    total_size: copied_bytes_total,
                                    files: copied_files_list,
                                },
                            );
                        }
                        return Err(fs_extra::error::Error::new(
                            fs_extra::error::ErrorKind::Interrupted,
                            "Cancelled by user",
                        ));
                    } else {
                        emit_log(
                            &handle,
                            format!("Failed to copy {}: {}", file_name_display, e),
                            "error",
                        );
                    }
                }
            }
        }

        // Done
        add_history_entry(
            &handle,
            HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Local::now().to_rfc3339(),
                action_type: "COPY_COMPLETED".to_string(),
                description: format!("Successfully copied {}", folder_name_clone),
                folder_name: folder_name_clone.clone(),
                source_path: source_path_clone.to_string_lossy().to_string(),
                target_path: target_full_path_clone.to_string_lossy().to_string(),
                copied_files_count: copied_files_list.len(),
                total_size: copied_bytes_total,
                files: copied_files_list.clone(),
            },
        );

        // Deploy: Re-read the latest config so that enabling deploy after scheduler
        // start is detected without needing to restart the scheduler.
        // server_bindings are also read from live_config here so that edits made
        // during a long copy are picked up at deploy time.
        let current_config = live_config_clone.lock().unwrap().clone();
        if allow_deploy && current_config.deploy_enabled {
            let live_server_bindings: Vec<TaskServerBinding> = if let Some(ref tid) = task_id_clone
            {
                current_config
                    .tasks
                    .iter()
                    .find(|t| &t.id == tid)
                    .map(|t| t.server_bindings.clone())
                    .unwrap_or_default()
            } else {
                vec![]
            };

            if live_server_bindings.is_empty() {
                emit_log(
                    &handle,
                    format!(
                        "No deploy servers selected for task '{}', skipping deployment.",
                        folder_name_clone
                    ),
                    "info",
                );
                return Ok(copied_bytes_total);
            }

            if let Err(e) = deploy_to_remote(
                &handle,
                &live_server_bindings,
                &current_config.servers,
                &current_config.command_groups,
                &target_full_path_clone,
                &folder_name_clone,
                should_cancel_clone,
                is_paused_clone,
            ) {
                emit_log(&handle, format!("Deployment failed: {}", e), "error");
            }
        }

        Ok(copied_bytes_total)
    });

    match copy_task.await {
        Ok(Ok(0)) => {
            // Nothing was copied (all files already up to date) — do not count as "copied"
        }
        Ok(Ok(_)) => {
            emit_log(
                app_handle,
                format!("Successfully copied: {}", folder_name),
                "success",
            );
            result.copied_folders.push(folder_name);
        }
        Ok(Err(e)) => {
            if let fs_extra::error::ErrorKind::Interrupted = e.kind {
                let msg = format!("Copy cancelled: {}", folder_name);
                emit_log(app_handle, msg.clone(), "warn");
            } else {
                let err_msg = format!("Failed to copy {}: {}", folder_name, e);
                emit_log(app_handle, err_msg.clone(), "error");
                result.errors.push(err_msg);
            }
        }
        Err(e) => {
            let err_msg = format!("Copy task panic: {}", e);
            emit_log(app_handle, err_msg.clone(), "error");
            result.errors.push(err_msg);
        }
    }
}

pub async fn temporary_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    source_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
) -> Result<(), String> {
    let source_path = PathBuf::from(source_path.trim());
    let target_root_path = PathBuf::from(target_root_path.trim());

    if source_path.as_os_str().is_empty() {
        return Err("Source path is required".to_string());
    }
    if target_root_path.as_os_str().is_empty() {
        return Err("Target root path is required".to_string());
    }
    if !source_path.exists() {
        return Err(format!(
            "Source path does not exist: {}",
            source_path.display()
        ));
    }
    if !source_path.is_dir() && !source_path.is_file() {
        return Err(format!(
            "Source path must be a file or directory: {}",
            source_path.display()
        ));
    }
    if !target_root_path.exists() {
        return Err(format!(
            "Target root directory does not exist: {}",
            target_root_path.display()
        ));
    }
    if !target_root_path.is_dir() {
        return Err(format!(
            "Target root path is not a directory: {}",
            target_root_path.display()
        ));
    }

    // Handle single file copy
    if source_path.is_file() {
        return temporary_copy_file(
            app_handle,
            config,
            source_path,
            target_root_path,
            overwrite_existing,
            should_cancel,
            is_paused,
        )
        .await;
    }

    // Directory copy (original logic)
    let folder_name = source_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "temporary-copy".to_string());

    let target_full_path = target_root_path.join(&folder_name);
    if target_full_path == source_path || target_full_path.starts_with(&source_path) {
        return Err(format!(
            "Target path would be created inside source path, which may cause recursive copying: {}",
            target_full_path.display()
        ));
    }

    emit_log(
        app_handle,
        format!(
            "Starting temporary copy: {} -> {}",
            source_path.display(),
            target_root_path.display()
        ),
        "info",
    );

    let mut result = ScanResult {
        scanned_paths: 1,
        found_folders: vec![folder_name.clone()],
        copied_folders: vec![],
        errors: vec![],
    };

    perform_copy(
        app_handle,
        source_path,
        folder_name,
        &target_root_path,
        config,
        live_config,
        should_cancel,
        is_paused,
        overwrite_existing,
        &mut result,
        None,
        false,
        "manual",
        &file_extensions,
        &filename_includes,
    )
    .await;

    if result.errors.is_empty() {
        Ok(())
    } else {
        Err(result.errors.join("; "))
    }
}

/// Copy a single file to the target directory.
async fn temporary_copy_file<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    source_path: PathBuf,
    target_root_path: PathBuf,
    overwrite_existing: bool,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> Result<(), String> {
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "Cannot extract file name from source path".to_string())?;

    let target_file = target_root_path.join(&file_name);

    emit_log(
        app_handle,
        format!(
            "Starting single-file copy: {} -> {}",
            source_path.display(),
            target_root_path.display()
        ),
        "info",
    );

    // Ensure target directory exists
    if let Err(e) = fs::create_dir_all(&target_root_path).await {
        return Err(format!(
            "Failed to create target directory {}: {}",
            target_root_path.display(),
            e
        ));
    }

    // Check if target file already exists
    if target_file.exists() {
        if !target_file.is_file() {
            return Err(format!(
                "Target path already exists as a directory and cannot be used as a file: {}",
                target_file.display()
            ));
        }
        if overwrite_existing {
            emit_log(
                app_handle,
                format!(
                    "File already exists at target and will be overwritten: {}",
                    target_file.display()
                ),
                "warn",
            );
        } else {
            emit_log(
                app_handle,
                format!(
                    "File already exists at target, skipping because overwrite is disabled: {}",
                    target_file.display()
                ),
                "info",
            );
            return Ok(());
        }
    }

    let target_existed_before = target_file.exists();

    if target_existed_before && overwrite_existing {
        emit_log(
            app_handle,
            format!(
                "Preparing overwrite copy for existing target file: {}",
                target_file.display()
            ),
            "warn",
        );
    }

    // Get file metadata
    let meta =
        std::fs::metadata(&source_path).map_err(|e| format!("Cannot read file metadata: {}", e))?;
    let file_size = meta.len();

    // Stability check for recently modified files
    let recent_file_guard_secs = config.recent_file_guard_mins * 60;
    let is_recent = meta
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age < StdDuration::from_secs(recent_file_guard_secs))
        .unwrap_or(true);

    if is_recent && config.stability_check_secs > 0 {
        emit_log(
            app_handle,
            format!(
                "File was recently modified, waiting {}s for stability check...",
                config.stability_check_secs
            ),
            "info",
        );
        let intervals = config.stability_check_secs * 5;
        for _ in 0..intervals {
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Cancelled by user".to_string());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        // Re-check file size
        let new_meta = std::fs::metadata(&source_path)
            .map_err(|e| format!("Cannot re-check file metadata: {}", e))?;
        if new_meta.len() != file_size {
            return Err(format!(
                "File size changed during stability check ({} -> {} bytes), aborting",
                file_size,
                new_meta.len()
            ));
        }
    }

    // Record history: COPY_STARTED
    add_history_entry(
        app_handle,
        HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Local::now().to_rfc3339(),
            action_type: "COPY_STARTED".to_string(),
            description: format!("Started copying file {}", file_name),
            folder_name: file_name.clone(),
            source_path: source_path.to_string_lossy().to_string(),
            target_path: target_root_path.to_string_lossy().to_string(),
            copied_files_count: 0,
            total_size: 0,
            files: vec![],
        },
    );

    // Copy with progress
    let app_handle_clone = app_handle.clone();
    let file_name_clone = file_name.clone();
    let source_clone = source_path.clone();
    let target_file_clone = target_file.clone();
    let target_file_display = target_file.to_string_lossy().to_string();
    let source_display = source_path.to_string_lossy().to_string();
    let copy_buffer_size = (config.copy_buffer_size_kb as usize).max(64) * 1024;

    let copy_result = tauri::async_runtime::spawn_blocking(move || {
        let start_time = Instant::now();
        let mut last_emit_time = Instant::now();
        let mut copied_so_far: u64 = 0;

        let mut on_progress = |delta: u64| {
            copied_so_far += delta;
            let now = Instant::now();
            if now.duration_since(last_emit_time).as_millis() > 500 || copied_so_far == file_size {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    (copied_so_far as f64 / elapsed) as u64
                } else {
                    0
                };
                let eta = if speed > 0 && file_size > copied_so_far {
                    (file_size - copied_so_far) / speed
                } else {
                    0
                };
                emit_progress(
                    &app_handle_clone,
                    &file_name_clone,
                    copied_so_far,
                    file_size,
                    speed,
                    eta,
                    elapsed as u64,
                    &target_file_display,
                    &source_display,
                    "manual",
                );
                last_emit_time = now;
            }
        };

        copy_file_with_overwrite_mode(
            &source_clone,
            &target_file_clone,
            overwrite_existing,
            &should_cancel,
            &is_paused,
            copy_buffer_size,
            &mut on_progress,
        )
    })
    .await
    .map_err(|e| format!("Copy task panic: {}", e))?;

    match copy_result {
        Ok(bytes_copied) => {
            emit_log(
                app_handle,
                format!(
                    "File copied successfully: {} ({} bytes)",
                    file_name, bytes_copied
                ),
                "success",
            );

            // Record history: COPY_COMPLETED
            add_history_entry(
                app_handle,
                HistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: Local::now().to_rfc3339(),
                    action_type: "COPY_COMPLETED".to_string(),
                    description: format!("Completed copying file {}", file_name),
                    folder_name: file_name.clone(),
                    source_path: source_path.to_string_lossy().to_string(),
                    target_path: target_root_path.to_string_lossy().to_string(),
                    copied_files_count: 1,
                    total_size: bytes_copied,
                    files: vec![file_name],
                },
            );

            Ok(())
        }
        Err(e) => {
            // Clean up partial file on failure when this was a brand-new target.
            if !target_existed_before {
                let _ = std::fs::remove_file(&target_file);
            }
            Err(format!("Failed to copy file: {}", e))
        }
    }
}

pub async fn scan_and_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> ScanResult {
    let mut result = ScanResult {
        scanned_paths: 0,
        found_folders: vec![],
        copied_folders: vec![],
        errors: vec![],
    };

    let re_version = Regex::new(r"^(\d{4}_\d{2}_\d{2}_\d{2}_\d{2})\((.+)\)$").unwrap();
    let now_local = Local::now();
    let now = now_local.naive_local();
    let today = now.date();
    let yesterday = today - Duration::days(1);

    // Check Time Ranges
    if !config.time_ranges.is_empty() {
        let current_time = now_local.time();
        let mut in_range = false;
        for range in &config.time_ranges {
            let parts: Vec<&str> = range.split('-').collect();
            if parts.len() == 2 {
                if let (Ok(start), Ok(end)) = (
                    NaiveTime::parse_from_str(parts[0], "%H:%M"),
                    NaiveTime::parse_from_str(parts[1], "%H:%M"),
                ) {
                    if current_time >= start && current_time <= end {
                        in_range = true;
                        break;
                    }
                }
            }
        }

        if !in_range {
            emit_log(
                app_handle,
                format!(
                    "Current time {} is outside of configured time ranges {:?}. Skipping scan.",
                    current_time.format("%H:%M"),
                    config.time_ranges
                ),
                "info",
            );
            return result;
        }
    }

    for task in &config.tasks {
        if !task.enabled {
            continue;
        }

        if should_cancel.load(Ordering::SeqCst) {
            emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
            return result;
        }

        result.scanned_paths += 1;
        emit_log(
            app_handle,
            format!("Task [{}]: Scanning {}", task.name, task.remote_path),
            "info",
        );

        let path = Path::new(&task.remote_path);
        let local_parent = if let Some(custom_local) = &task.local_path {
            Path::new(custom_local)
        } else {
            Path::new(&config.local_path)
        };

        match &task.rule {
            MatchRule::VersionMatch(target_version) => {
                let mut entries = match fs::read_dir(path).await {
                    Ok(entries) => entries,
                    Err(e) => {
                        let err_msg = format!("Failed to read {}: {}", task.remote_path, e);
                        emit_log(app_handle, err_msg.clone(), "error");
                        result.errors.push(err_msg);
                        continue;
                    }
                };

                // Collect candidates
                let mut candidates: Vec<Candidate> = Vec::new();
                let mut tree_view: Vec<String> = Vec::new();

                while let Ok(Some(entry)) = entries.next_entry().await {
                    if should_cancel.load(Ordering::SeqCst) {
                        emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                        return result;
                    }

                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy().to_string();

                    let mut dt = NaiveDateTime::MIN;
                    if let Some(caps) = re_version.captures(&name_str) {
                        if let Some(date_part) = caps.get(1) {
                            if let Ok(parsed) =
                                NaiveDateTime::parse_from_str(date_part.as_str(), "%Y_%m_%d_%H_%M")
                            {
                                dt = parsed;
                            }
                        }
                    }

                    candidates.push(Candidate {
                        path: entry.path(),
                        name: name_str.clone(),
                        version: if let Some(caps) = re_version.captures(&name_str) {
                            caps.get(2)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default()
                        } else {
                            String::new()
                        },
                        datetime: dt,
                    });
                }

                // Sort
                candidates.sort_by(|a, b| b.datetime.cmp(&a.datetime));

                // Tree view
                for cand in candidates.iter().take(20) {
                    tree_view.push(format!("├─ {}", cand.name));
                }
                if candidates.len() > 20 {
                    tree_view.push(format!("└─ ... ({} more files)", candidates.len() - 20));
                }
                if !tree_view.is_empty() {
                    emit_log(
                        app_handle,
                        format!("Directory structure (partial):\n{}", tree_view.join("\n")),
                        "info",
                    );
                }

                // Filter by version
                let mut version_matches: Vec<&Candidate> = candidates
                    .iter()
                    .filter(|c| c.version == *target_version)
                    .collect();

                if version_matches.is_empty() {
                    emit_log(
                        app_handle,
                        format!("No candidates found for version {}", target_version),
                        "info",
                    );
                    continue;
                }

                version_matches.sort_by(|a, b| b.datetime.cmp(&a.datetime));

                if let Some(latest) = version_matches.first() {
                    let folder_date = latest.datetime.date();
                    emit_log(
                        app_handle,
                        format!(
                            "Latest candidate for {}: {} ({})",
                            target_version, latest.name, folder_date
                        ),
                        "info",
                    );

                    if folder_date == today || folder_date == yesterday {
                        result.found_folders.push(latest.name.clone());

                        perform_copy(
                            app_handle,
                            latest.path.clone(),
                            latest.name.clone(),
                            local_parent,
                            config,
                            live_config.clone(),
                            should_cancel.clone(),
                            is_paused.clone(),
                            false,
                            &mut result,
                            Some(task.id.clone()),
                            true,
                            "scheduled",
                            &config.file_extensions,
                            &config.filename_includes,
                        )
                        .await;
                    } else {
                        emit_log(
                            app_handle,
                            format!(
                                "Ignored {} because date {} is not Today ({}) or Yesterday ({})",
                                latest.name, folder_date, today, yesterday
                            ),
                            "info",
                        );
                    }
                }
            }
            MatchRule::DateMatch(format_str) => {
                let fmt = if format_str.trim().is_empty() {
                    "%y%m%d"
                } else {
                    format_str.trim()
                };
                let today_name = now_local.format(fmt).to_string();
                let yesterday_name = (now_local - Duration::days(1)).format(fmt).to_string();

                // Only check yesterday during the first hour of a new day (00:00–01:00),
                // to catch files that were generated near midnight but missed by the last
                // scan of the previous day. perform_copy is incremental so re-scanning
                // yesterday costs nothing once all files have already been copied.
                let is_first_hour = now_local.hour() == 0;
                let dirs_to_check: Vec<String> = if is_first_hour && yesterday_name != today_name {
                    vec![today_name.clone(), yesterday_name]
                } else {
                    vec![today_name.clone()]
                };

                emit_log(
                    app_handle,
                    format!(
                        "Checking date-based folder(s): {}",
                        dirs_to_check.join(", ")
                    ),
                    "info",
                );

                for target_name in dirs_to_check {
                    if should_cancel.load(Ordering::SeqCst) {
                        emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                        return result;
                    }

                    let target_path = path.join(&target_name);

                    if !target_path.exists() || !target_path.is_dir() {
                        emit_log(
                            app_handle,
                            format!(
                                "Folder {} does not exist in {}",
                                target_name, task.remote_path
                            ),
                            "info",
                        );
                        continue;
                    }

                    emit_log(
                        app_handle,
                        format!("Found candidate folder: {}", target_name),
                        "success",
                    );

                    let local_target_base = local_parent.join(&target_name);

                    let mut sub_entries = match fs::read_dir(&target_path).await {
                        Ok(e) => e,
                        Err(e) => {
                            let err = format!(
                                "Failed to list contents of {}: {}",
                                target_path.display(),
                                e
                            );
                            emit_log(app_handle, err.clone(), "error");
                            result.errors.push(err);
                            continue;
                        }
                    };

                    let mut found_any = false;

                    while let Ok(Some(entry)) = sub_entries.next_entry().await {
                        let sub_path = entry.path();
                        if sub_path.is_dir() {
                            let sub_name = entry.file_name().to_string_lossy().to_string();
                            found_any = true;
                            result
                                .found_folders
                                .push(format!("{}/{}", target_name, sub_name));

                            perform_copy(
                                app_handle,
                                sub_path,
                                sub_name,
                                &local_target_base,
                                config,
                                live_config.clone(),
                                should_cancel.clone(),
                                is_paused.clone(),
                                false,
                                &mut result,
                                Some(task.id.clone()),
                                true,
                                "scheduled",
                                &config.file_extensions,
                                &config.filename_includes,
                            )
                            .await;
                        }
                    }

                    if !found_any {
                        emit_log(
                            app_handle,
                            format!("No build directories found in {}", target_name),
                            "info",
                        );
                    }
                }
            }
        }
    }
    result
}
