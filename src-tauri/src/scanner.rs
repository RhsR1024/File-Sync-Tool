use crate::config::{AppConfig, MatchRule};
use crate::history::{add_history_entry, HistoryEntry};
use crate::deploy::deploy_to_remote;
use chrono::{Local, NaiveDateTime, Duration, NaiveTime};
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::fs;
use tauri::{Emitter, Manager};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use std::io::{Read, Write};
use std::fs::OpenOptions;

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
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    name: String,
    version: String,
    datetime: NaiveDateTime,
}

// Helper to emit logs to frontend in real-time
fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit("log-message", LogEvent {
        msg: msg.clone(),
        level: level.to_string(),
    });

    // Also write to log file
    if let Ok(app_dir) = app_handle.path().app_data_dir() {
         let path_buf = app_dir.clone();
         if let Ok(_) = std::fs::create_dir_all(&path_buf) {
             let log_path = path_buf.join("app.log");
             if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
                 let time = Local::now().format("%Y-%m-%d %H:%M:%S");
                 let _ = writeln!(file, "[{}] [{}] {}", time, level.to_uppercase(), msg);
             }
         }
    }
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
    remote_path: &str
) {
    let percentage = if total > 0 {
        (copied as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    
    let _ = app_handle.emit("copy-progress", ProgressEvent {
        folder: folder.to_string(),
        total_bytes: total,
        copied_bytes: copied,
        percentage,
        speed,
        eta_seconds,
        elapsed_seconds,
        local_path: local_path.to_string(),
        remote_path: remote_path.to_string(),
    });
}

// Helper function to copy file with chunking and interruption support
fn copy_file_chunked<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P, 
    to: Q, 
    should_cancel: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(u64) // bytes copied delta
) -> Result<u64, String> {
    let mut file_in = std::fs::File::open(from).map_err(|e| e.to_string())?;
    let mut file_out = std::fs::File::create(to).map_err(|e| e.to_string())?;
    
    let mut buffer = [0u8; 64 * 1024]; // 64KB buffer
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
        
        file_out.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        total_copied += n as u64;
        on_progress(n as u64);
    }
    
    Ok(total_copied)
}

// Extracted copy logic to reuse across different matching rules
async fn perform_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    source_path: PathBuf,
    folder_name: String,
    target_parent_path: &Path,
    config: &AppConfig,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    result: &mut ScanResult
) {
    let target_full_path = target_parent_path.join(&folder_name);
    
    emit_log(app_handle, format!("Target local directory: {}", target_full_path.display()), "info");

    if target_full_path.exists() {
         let is_dir = target_full_path.is_dir();
         let skip_msg = format!("Skipped (Exists): {} -> {} (Is Dir: {})", folder_name, target_full_path.display(), is_dir);
         emit_log(app_handle, skip_msg.clone(), "warn");
         result.errors.push(skip_msg);
         return;
    }

    emit_log(app_handle, format!("Starting copy: {} -> {}", source_path.display(), target_parent_path.display()), "info");
    
    // Ensure parent dir exists
    if let Err(e) = fs::create_dir_all(target_parent_path).await {
        let err_msg = format!("Failed to create local directory {}: {}", target_parent_path.display(), e);
        emit_log(app_handle, err_msg.clone(), "error");
        result.errors.push(err_msg);
        return;
    }

    let app_handle_clone = app_handle.clone();
    let folder_name_clone = folder_name.clone();
    let source_path_clone = source_path.clone();
    let target_full_path_clone = target_full_path.clone();
    
    // Clone config for closure
    let extensions = config.file_extensions.clone();
    let includes = config.filename_includes.clone();
    let config_clone = config.clone();
    let should_cancel_clone = should_cancel.clone();
    let is_paused_clone = is_paused.clone();

    let copy_task = tauri::async_runtime::spawn_blocking(move || {
        let handle = app_handle_clone;
        
        // Log START event to history
        add_history_entry(&handle, HistoryEntry {
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
        });

        let start_time = Instant::now();
        let mut last_emit_time = Instant::now();
        
        // Prepare paths for display
        let local_path_display = target_full_path_clone.to_string_lossy().to_string();
        let remote_path_display = source_path_clone.to_string_lossy().to_string();
        
        // Helper for speed/eta
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
                    &remote_path_display
                );
                last_emit_time = now;
            }
        };
        
        // Just test access to source dir
        if let Err(e) = std::fs::read_dir(&source_path_clone) {
             let e = e.to_string(); 
             emit_log(&handle, format!("Failed to access source dir: {}", e), "error");
             return Err(fs_extra::error::Error::new(fs_extra::error::ErrorKind::Other, &e));
        }
        
        // Collect files with filtering (Iterative)
        let mut filtered_files = Vec::new();
        let mut total_filtered_bytes = 0;
        
        let mut dirs_to_visit = vec![source_path_clone.clone()];
        while let Some(current_dir) = dirs_to_visit.pop() {
             if let Ok(entries) = std::fs::read_dir(&current_dir) {
                 for entry in entries.flatten() {
                     let path = entry.path();
                     if path.is_dir() {
                         dirs_to_visit.push(path);
                     } else {
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
                         
                         if ext_match && inc_match {
                             if let Ok(meta) = entry.metadata() {
                                 filtered_files.push((path, meta.len()));
                                 total_filtered_bytes += meta.len();
                             }
                         }
                     }
                 }
             }
        }
        
        if filtered_files.is_empty() {
            emit_log(&handle, format!("No files found to copy in {}", folder_name_clone), "warn");
            return Ok(0);
        }
        
        emit_log(&handle, format!("Found {} files ({}) to copy.", filtered_files.len(), total_filtered_bytes), "info");
        
        // Create target directory structure and Copy
        let mut copied_bytes_total = 0;
        let mut copied_files_list = Vec::new();
        
        for (src, _size) in filtered_files {
            // Check cancel before starting file
             if should_cancel_clone.load(Ordering::SeqCst) {
                 // Log partial
                 if !copied_files_list.is_empty() {
                     add_history_entry(&handle, HistoryEntry {
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
                     });
                 }
                 return Err(fs_extra::error::Error::new(fs_extra::error::ErrorKind::Interrupted, "Cancelled by user"));
             }
            
             // Calculate relative path
             let rel_path = src.strip_prefix(&source_path_clone).unwrap_or(&src);
             let dst = target_full_path_clone.join(rel_path);
             
             // Create parent dir
             if let Some(parent) = dst.parent() {
                 let _ = std::fs::create_dir_all(parent);
             }
             
             let file_name_display = src.file_name().unwrap_or_default().to_string_lossy().to_string();

             // Copy with chunking
             let copy_res = copy_file_chunked(
                 &src, 
                 &dst, 
                 &should_cancel_clone, 
                 &is_paused_clone,
                 &mut |delta| {
                     copied_bytes_total += delta;
                     update_stats(copied_bytes_total, total_filtered_bytes);
                 }
             );
             
             match copy_res {
                 Ok(_) => {
                     copied_files_list.push(file_name_display);
                 },
                 Err(e) => {
                     if e.contains("Cancelled") {
                         // Save partial
                         if !copied_files_list.is_empty() {
                             add_history_entry(&handle, HistoryEntry {
                                 id: uuid::Uuid::new_v4().to_string(),
                                 timestamp: Local::now().to_rfc3339(),
                                 action_type: "COPY_CANCELLED".to_string(),
                                 description: format!("Cancelled copying {}", folder_name_clone),
                                 folder_name: format!("{} (Cancelled)", folder_name_clone),
                                 source_path: source_path_clone.to_string_lossy().to_string(),
                                 target_path: target_full_path_clone.to_string_lossy().to_string(),
                                 copied_files_count: copied_files_list.len(),
                                 total_size: copied_bytes_total,
                                 files: copied_files_list,
                             });
                         }
                         return Err(fs_extra::error::Error::new(fs_extra::error::ErrorKind::Interrupted, "Cancelled by user"));
                     } else {
                         emit_log(&handle, format!("Failed to copy {}: {}", file_name_display, e), "error");
                     }
                 }
             }
        }

        // Done
         add_history_entry(&handle, HistoryEntry {
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
         });
         
         // Deploy
         if config_clone.deploy_enabled {
              if let Err(e) = deploy_to_remote(
                  &handle, 
                  &config_clone, 
                  &target_full_path_clone, 
                  &folder_name_clone,
                  should_cancel_clone,
                  is_paused_clone
              ) {
                  emit_log(&handle, format!("Deployment failed: {}", e), "error");
              }
         }
        
        Ok(copied_bytes_total)
    });

    match copy_task.await {
        Ok(Ok(_)) => {
            let success_msg = format!("Successfully copied: {}", folder_name);
            emit_log(app_handle, success_msg.clone(), "success");
            result.copied_folders.push(folder_name);
        },
        Ok(Err(e)) => {
            if let fs_extra::error::ErrorKind::Interrupted = e.kind {
                let msg = format!("Copy cancelled: {}", folder_name);
                emit_log(app_handle, msg.clone(), "warn");
            } else {
                let err_msg = format!("Failed to copy {}: {}", folder_name, e);
                emit_log(app_handle, err_msg.clone(), "error");
                result.errors.push(err_msg);
            }
        },
        Err(e) => {
            let err_msg = format!("Copy task panic: {}", e);
            emit_log(app_handle, err_msg.clone(), "error");
            result.errors.push(err_msg);
        }
    }
}

pub async fn scan_and_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>, 
    config: &AppConfig,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>
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
                    NaiveTime::parse_from_str(parts[1], "%H:%M")
                ) {
                    if current_time >= start && current_time <= end {
                        in_range = true;
                        break;
                    }
                }
            }
        }
        
        if !in_range {
             emit_log(app_handle, format!("Current time {} is outside of configured time ranges {:?}. Skipping scan.", current_time.format("%H:%M"), config.time_ranges), "info");
             return result;
        }
    }

    for task in &config.tasks {
        if !task.enabled { continue; }
        
        if should_cancel.load(Ordering::SeqCst) {
            emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
            return result;
        }

        result.scanned_paths += 1;
        emit_log(app_handle, format!("Task [{}]: Scanning {}", task.name, task.remote_path), "info");
        
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
                             if let Ok(parsed) = NaiveDateTime::parse_from_str(date_part.as_str(), "%Y_%m_%d_%H_%M") {
                                 dt = parsed;
                             }
                         }
                    }
                    
                    candidates.push(Candidate {
                        path: entry.path(),
                        name: name_str.clone(),
                        version: if let Some(caps) = re_version.captures(&name_str) {
                            caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default()
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
                     emit_log(app_handle, format!("Directory structure (partial):\n{}", tree_view.join("\n")), "info");
                }
                
                // Filter by version
                let mut version_matches: Vec<&Candidate> = candidates.iter()
                    .filter(|c| c.version == *target_version)
                    .collect();
                
                if version_matches.is_empty() {
                    emit_log(app_handle, format!("No candidates found for version {}", target_version), "info");
                    continue;
                }
                
                version_matches.sort_by(|a, b| b.datetime.cmp(&a.datetime));
                
                if let Some(latest) = version_matches.first() {
                    let folder_date = latest.datetime.date();
                    emit_log(app_handle, format!("Latest candidate for {}: {} ({})", target_version, latest.name, folder_date), "info");

                    if folder_date == today || folder_date == yesterday {
                        result.found_folders.push(latest.name.clone());
                        
                        perform_copy(
                            app_handle,
                            latest.path.clone(),
                            latest.name.clone(),
                            local_parent,
                            config,
                            should_cancel.clone(),
                            is_paused.clone(),
                            &mut result
                        ).await;
                        
                    } else {
                        emit_log(app_handle, format!("Ignored {} because date {} is not Today ({}) or Yesterday ({})", latest.name, folder_date, today, yesterday), "info");
                    }
                }
            },
            MatchRule::DateMatch(format_str) => {
                let fmt = if format_str.is_empty() { "%y%m%d" } else { format_str };
                let target_name = now_local.format(fmt).to_string();
                
                emit_log(app_handle, format!("Checking for date-based folder: {}", target_name), "info");
                
                let target_path = path.join(&target_name);
                
                // Check if exists
                if target_path.exists() && target_path.is_dir() {
                    emit_log(app_handle, format!("Found candidate folder: {}", target_name), "success");
                    
                    // Instead of treating the folder itself as the unit to copy/skip,
                    // we now treat it as a container that may hold multiple build directories.
                    // We need to list its contents and copy them individually if they don't exist locally.
                    
                    let local_target_base = local_parent.join(&target_name);
                    
                    // Scan subdirectories in the remote folder
                    let mut sub_entries = match fs::read_dir(&target_path).await {
                        Ok(e) => e,
                        Err(e) => {
                            let err = format!("Failed to list contents of {}: {}", target_path.display(), e);
                            emit_log(app_handle, err.clone(), "error");
                            result.errors.push(err);
                            continue;
                        }
                    };

                    let mut found_any_new = false;
                    
                    while let Ok(Some(entry)) = sub_entries.next_entry().await {
                         let sub_path = entry.path();
                         if sub_path.is_dir() {
                             let sub_name = entry.file_name().to_string_lossy().to_string();
                             let local_sub_path = local_target_base.join(&sub_name);
                             
                             if !local_sub_path.exists() {
                                 found_any_new = true;
                                 emit_log(app_handle, format!("Found new build directory: {}/{}", target_name, sub_name), "info");
                                 result.found_folders.push(format!("{}/{}", target_name, sub_name));
                                 
                                 perform_copy(
                                     app_handle,
                                     sub_path,
                                     sub_name, // Copy as sub_name
                                     &local_target_base, // Into local/Date/
                                     config,
                                     should_cancel.clone(),
                                     is_paused.clone(),
                                     &mut result
                                 ).await;
                             } else {
                                 // Optional: Check if empty or partial? For now assume existence means done.
                                 // emit_log(app_handle, format!("Skipping existing build: {}/{}", target_name, sub_name), "info");
                             }
                         }
                    }
                    
                    if !found_any_new {
                        emit_log(app_handle, format!("No new build directories found in {}", target_name), "info");
                    }

                } else {
                    emit_log(app_handle, format!("Folder {} does not exist in {}", target_name, task.remote_path), "info");
                }
            }
        }
    }
    result
}
