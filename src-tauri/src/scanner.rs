use crate::config::AppConfig;
use crate::history::{add_history_entry, HistoryEntry};
use crate::deploy::deploy_to_remote;
use chrono::{Local, NaiveDateTime, Duration, NaiveTime};
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::fs;
use tauri::{Emitter, Manager}; // Import Manager for path access

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
    level: String, // "info", "success", "error"
}

use std::time::Instant;

#[derive(Debug, serde::Serialize, Clone)]
struct ProgressEvent {
    folder: String,
    total_bytes: u64,
    copied_bytes: u64,
    percentage: f64,
    speed: u64, // bytes per second
    eta_seconds: u64,
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    name: String,
    version: String,
    datetime: NaiveDateTime,
}

use std::io::{Read, Write}; // Import traits

use std::fs::OpenOptions;

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
    eta_seconds: u64
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
    });
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

// Modify signature to accept app_handle
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

    let re = Regex::new(r"^(\d{4}_\d{2}_\d{2}_\d{2}_\d{2})\((.+)\)$").unwrap();
    let now_local = Local::now();
    let now = now_local.naive_local();
    let today = now.date();
    let yesterday = today - Duration::days(1);
    
    // Check Time Ranges
    // Format "HH:mm-HH:mm" e.g. "05:00-09:00"
    // If ranges are configured, we ONLY run if current time is within ONE of them.
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

    for remote_path in &config.remote_paths {
        if should_cancel.load(Ordering::SeqCst) {
            emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
            return result;
        }

        result.scanned_paths += 1;
        emit_log(app_handle, format!("Scanning remote path: {}", remote_path), "info");
        
        let path = Path::new(remote_path);
        
        let mut entries = match fs::read_dir(path).await {
            Ok(entries) => entries,
            Err(e) => {
                let err_msg = format!("Failed to read {}: {}", remote_path, e);
                emit_log(app_handle, err_msg.clone(), "error");
                result.errors.push(err_msg);
                continue;
            }
        };

        // 1. Collect all valid candidates
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut raw_files_count = 0;

        // Collect names for "tree" visualization
        let mut tree_view: Vec<String> = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            if should_cancel.load(Ordering::SeqCst) {
                emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                return result;
            }
            raw_files_count += 1;
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy().to_string();
            
            // Only add to tree view if it looks like a version folder (optional, or list all?)
            // Listing ALL might be too much if there are thousands. Let's list matching ones or first 50.
            // User requested: "Sort by latest date, output top 20"
            // But we are iterating via ReadDir, which is not sorted.
            // We need to collect ALL first to sort them?
            // If directory is huge, collecting all might be slow. But usually < 1000 items is fine.
            // Let's collect names and sort them.
            
            // Just collect everything first
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy().to_string();
            
            // Try to parse date for sorting?
            let mut dt = NaiveDateTime::MIN;
            if let Some(caps) = re.captures(&name_str) {
                 if let Some(date_part) = caps.get(1) {
                     if let Ok(parsed) = NaiveDateTime::parse_from_str(date_part.as_str(), "%Y_%m_%d_%H_%M") {
                         dt = parsed;
                     }
                 }
            }
            
            candidates.push(Candidate {
                path: entry.path(),
                name: name_str.clone(),
                version: if let Some(caps) = re.captures(&name_str) {
                    caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default()
                } else {
                    String::new()
                },
                datetime: dt,
            });
        }
        
        // Now sort candidates by datetime desc
        candidates.sort_by(|a, b| b.datetime.cmp(&a.datetime));

        // Generate tree view for top 20
        for (i, cand) in candidates.iter().take(20).enumerate() {
             tree_view.push(format!("├─ {}", cand.name));
        }
        if candidates.len() > 20 {
             tree_view.push(format!("└─ ... ({} more files)", candidates.len() - 20));
        }
        
        // Print tree view
        if !tree_view.is_empty() {
             let tree_msg = format!("Directory structure (partial):\n{}", tree_view.join("\n"));
             emit_log(app_handle, tree_msg, "info");
        }
        
        emit_log(app_handle, format!("Found {} candidates matching pattern.", candidates.len()), "info");

        // 2. Process each target version
        for target_version in &config.target_versions {
            if should_cancel.load(Ordering::SeqCst) {
                emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                return result;
            }
            emit_log(app_handle, format!("Checking for target version: {}", target_version), "info");
            
            let mut version_matches: Vec<&Candidate> = candidates.iter()
                .filter(|c| c.version == *target_version)
                .collect();
            
            if version_matches.is_empty() {
                emit_log(app_handle, format!("No candidates found for version {}", target_version), "info");
                continue;
            }

            // Sort by datetime descending (newest first)
            version_matches.sort_by(|a, b| b.datetime.cmp(&a.datetime));

            if let Some(latest) = version_matches.first() {
                let folder_date = latest.datetime.date();
                
                // Log the latest candidate found
                emit_log(app_handle, format!("Latest candidate for {}: {} ({})", target_version, latest.name, folder_date), "info");

                if folder_date == today || folder_date == yesterday {
                    result.found_folders.push(latest.name.clone());

                    // Target local path should be the PARENT directory, because fs_extra::copy copies the FOLDER ITSELF into the target
                    // If we want E:\UMS_TEMP\2026..., and source is \\...\2026...
                    // fs_extra copy(source, target) -> target\source_folder_name
                    // So if we want result at E:\UMS_TEMP\2026..., we should copy to E:\UMS_TEMP
                    
                    let target_parent = Path::new(&config.local_path);
                    let target_full_path = target_parent.join(&latest.name);

                    emit_log(app_handle, format!("Target local directory: {}", target_full_path.display()), "info");

                    if target_full_path.exists() {
                         let is_dir = target_full_path.is_dir();
                         let skip_msg = format!("Skipped (Exists): {} -> {} (Is Dir: {})", latest.name, target_full_path.display(), is_dir);
                         emit_log(app_handle, skip_msg.clone(), "warn");
                         result.errors.push(skip_msg);
                         continue;
                    }

                    emit_log(app_handle, format!("Starting copy: {} -> {}", latest.path.display(), target_parent.display()), "info");
                    
                    // Ensure parent dir exists
                    if let Err(e) = fs::create_dir_all(target_parent).await {
                        let err_msg = format!("Failed to create local directory {}: {}", target_parent.display(), e);
                        emit_log(app_handle, err_msg.clone(), "error");
                        result.errors.push(err_msg);
                        continue;
                    }

                    let source_path = latest.path.clone();
                    // NOTE: If we are filtering files, we CANNOT use fs_extra::dir::copy_with_progress for the whole folder directly.
                    // We must manually copy selected files if filters are active.
                    // If filters are empty, we copy whole folder.
                    
                    let use_filter = !config.file_extensions.is_empty() || !config.filename_includes.is_empty();
                    
                    let app_handle_clone = app_handle.clone();
                    let folder_name = latest.name.clone();
                    let target_full_path_clone = target_full_path.clone();
                    
                    // Clone config for closure
                    let extensions = config.file_extensions.clone();
                    let includes = config.filename_includes.clone();
                    let config_clone = config.clone(); // Clone full config for deploy
                    let should_cancel_clone = should_cancel.clone();
                    let is_paused_clone = is_paused.clone();

                    let copy_task = tauri::async_runtime::spawn_blocking(move || {
                        let handle = app_handle_clone;
                        
                        // Log START event to history
                        add_history_entry(&handle, HistoryEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            timestamp: Local::now().to_rfc3339(),
                            action_type: "COPY_STARTED".to_string(),
                            description: format!("Started copying {}", folder_name),
                            folder_name: folder_name.clone(),
                            source_path: source_path.to_string_lossy().to_string(),
                            target_path: target_full_path_clone.to_string_lossy().to_string(),
                            copied_files_count: 0,
                            total_size: 0,
                            files: vec![],
                        });

                        let start_time = Instant::now();
                        let mut last_emit_time = Instant::now();
                        let mut last_copied_bytes = 0;
                        
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
                                
                                emit_progress(&handle, &folder_name, copied, total, speed, eta);
                                last_emit_time = now;
                                last_copied_bytes = copied;
                            }
                        };
                        
                        // Recursive scan for all cases, applying filters if needed.
                        
                        // Just test access to source dir
                        if let Ok(entries) = std::fs::read_dir(&source_path) {
                            // Valid
                        } else {
                             let e = std::io::Error::last_os_error();
                             emit_log(&handle, format!("Failed to access source dir: {}", e), "error");
                             return Err(fs_extra::error::Error::new(fs_extra::error::ErrorKind::Other, &e.to_string()));
                        }
                        
                        // Collect files with filtering (Iterative)
                        let mut filtered_files = Vec::new();
                        let mut total_filtered_bytes = 0;
                        
                        let mut dirs_to_visit = vec![source_path.clone()];
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
                                             if let Some(ext) = path.extension() {
                                                 // The extension() returns "gz" for "tar.gz" usually, or just last part.
                                                 // If user configured "tar.gz", we need to check full name ends with it.
                                                 // Standard logic: if any extension in list is contained at end of filename.
                                                 
                                                 let name_lower = file_name.to_lowercase();
                                                 let mut any_match = false;
                                                 for configured_ext in &extensions {
                                                     let conf_lower = configured_ext.to_lowercase();
                                                     // If configured is "tar.gz", and file ends with ".tar.gz", it's a match.
                                                     // We should check if file_name ends with "." + ext OR if it ends with ext (if user typed .tar.gz)
                                                     
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
                                             } else {
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
                            emit_log(&handle, format!("No files found to copy in {}", folder_name), "warn");
                            return Ok(0);
                        }
                        
                        emit_log(&handle, format!("Found {} files ({}) to copy.", filtered_files.len(), total_filtered_bytes), "info");
                        
                        // Create target directory structure and Copy
                        let mut copied_bytes_total = 0;
                        let mut copied_files_list = Vec::new();
                        
                        for (src, size) in filtered_files {
                            // Check cancel before starting file
                             if should_cancel_clone.load(Ordering::SeqCst) {
                                 // Log partial
                                 if !copied_files_list.is_empty() {
                                     add_history_entry(&handle, HistoryEntry {
                                         id: uuid::Uuid::new_v4().to_string(),
                                         timestamp: Local::now().to_rfc3339(),
                                         action_type: "COPY_CANCELLED".to_string(),
                                         description: format!("Cancelled copying {}", folder_name),
                                         folder_name: format!("{} (Cancelled)", folder_name),
                                         source_path: source_path.to_string_lossy().to_string(),
                                         target_path: target_full_path_clone.to_string_lossy().to_string(),
                                         copied_files_count: copied_files_list.len(),
                                         total_size: copied_bytes_total,
                                         files: copied_files_list.clone(),
                                     });
                                 }
                                 return Err(fs_extra::error::Error::new(fs_extra::error::ErrorKind::Interrupted, "Cancelled by user"));
                             }
                            
                             // Calculate relative path
                             let rel_path = src.strip_prefix(&source_path).unwrap_or(&src);
                             let dst = target_full_path_clone.join(rel_path);
                             
                             // Create parent dir
                             if let Some(parent) = dst.parent() {
                                 let _ = std::fs::create_dir_all(parent);
                             }
                             
                             let file_name_display = src.file_name().unwrap_or_default().to_string_lossy().to_string();

                             // Copy with chunking
                             let mut current_file_copied = 0;
                             let copy_res = copy_file_chunked(
                                 &src, 
                                 &dst, 
                                 &should_cancel_clone, 
                                 &is_paused_clone,
                                 &mut |delta| {
                                     copied_bytes_total += delta;
                                     current_file_copied += delta;
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
                                                 description: format!("Cancelled copying {}", folder_name),
                                                 folder_name: format!("{} (Cancelled)", folder_name),
                                                 source_path: source_path.to_string_lossy().to_string(),
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
                             description: format!("Successfully copied {}", folder_name),
                             folder_name: folder_name.clone(),
                             source_path: source_path.to_string_lossy().to_string(),
                             target_path: target_full_path_clone.to_string_lossy().to_string(),
                             copied_files_count: copied_files_list.len(),
                             total_size: copied_bytes_total,
                             files: copied_files_list.clone(),
                         });
                         
                         // Deploy
                         if config_clone.deploy_enabled {
                              if let Err(e) = deploy_to_remote(&handle, &config_clone, &target_full_path_clone, &folder_name) {
                                  emit_log(&handle, format!("Deployment failed: {}", e), "error");
                              }
                         }
                        
                        Ok(copied_bytes_total)
                    });

                    match copy_task.await {
                        Ok(Ok(_)) => {
                            let success_msg = format!("Successfully copied: {}", latest.name);
                            emit_log(app_handle, success_msg.clone(), "success");
                            result.copied_folders.push(latest.name.clone());
                        },
                        Ok(Err(e)) => {
                            if let fs_extra::error::ErrorKind::Interrupted = e.kind {
                                let msg = format!("Copy cancelled: {}", latest.name);
                                emit_log(app_handle, msg.clone(), "warn");
                                // Do not push to errors if it's just a cancel, or maybe user wants it in error list?
                                // User said "print Warn即可, 不用打印Error".
                                // If we push to errors, it might show up as red in summary.
                                // Let's NOT push to errors if we want to avoid "Error" perception.
                                // result.errors.push(msg); 
                            } else {
                                let err_msg = format!("Failed to copy {}: {}", latest.name, e);
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

                } else {
                    emit_log(app_handle, format!("Ignored {} because date {} is not Today ({}) or Yesterday ({})", latest.name, folder_date, today, yesterday), "info");
                }
            }
        }
    }
    result
}
