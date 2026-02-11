use crate::config::AppConfig;
use chrono::{Local, NaiveDateTime, Duration};
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::fs;
use std::collections::HashMap;

#[derive(Debug, serde::Serialize, Clone)]
pub struct ScanResult {
    pub scanned_paths: usize,
    pub found_folders: Vec<String>,
    pub copied_folders: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    name: String,
    version: String,
    datetime: NaiveDateTime,
}

pub async fn scan_and_copy(config: &AppConfig) -> ScanResult {
    let mut result = ScanResult {
        scanned_paths: 0,
        found_folders: vec![],
        copied_folders: vec![],
        errors: vec![],
    };

    // Regex to match YYYY_MM_DD_HH_MM(Version)
    let re = Regex::new(r"^(\d{4}_\d{2}_\d{2}_\d{2}_\d{2})\((.+)\)$").unwrap();
    let now = Local::now().naive_local();
    let today = now.date();
    let yesterday = today - Duration::days(1);

    for remote_path in &config.remote_paths {
        result.scanned_paths += 1;
        let path = Path::new(remote_path);
        
        let mut entries = match fs::read_dir(path).await {
            Ok(entries) => entries,
            Err(e) => {
                result.errors.push(format!("Failed to read {}: {}", remote_path, e));
                continue;
            }
        };

        // 1. Collect all valid candidates
        let mut candidates: Vec<Candidate> = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy().to_string();
            
            if let Some(caps) = re.captures(&name_str) {
                if let Some(date_part) = caps.get(1) {
                    if let Some(version_part) = caps.get(2) {
                        let version = version_part.as_str().to_string();
                        let date_str = date_part.as_str();
                        
                        // Parse date: YYYY_MM_DD_HH_MM
                        if let Ok(parsed_dt) = NaiveDateTime::parse_from_str(date_str, "%Y_%m_%d_%H_%M") {
                            candidates.push(Candidate {
                                path: entry.path(),
                                name: name_str,
                                version,
                                datetime: parsed_dt,
                            });
                        }
                    }
                }
            }
        }

        // 2. Process each target version
        for target_version in &config.target_versions {
            // Filter candidates for this version
            let mut version_matches: Vec<&Candidate> = candidates.iter()
                .filter(|c| c.version == *target_version)
                .collect();
            
            if version_matches.is_empty() {
                continue;
            }

            // Sort by datetime descending (newest first)
            version_matches.sort_by(|a, b| b.datetime.cmp(&a.datetime));

            // Get the latest one
            if let Some(latest) = version_matches.first() {
                // Check if it's within Today or Yesterday
                let folder_date = latest.datetime.date();
                if folder_date == today || folder_date == yesterday {
                    // This is a valid candidate to copy
                    result.found_folders.push(latest.name.clone());

                    let target_dir = Path::new(&config.local_path).join(&latest.name); // Use full name to avoid version conflict? 
                    // User said: "Copy to E:\UMS_TEMP\XXX(Version)\" 
                    // But if multiple dates have same version, we might overwrite?
                    // The user's prompt says: "copy to E:\UMS_TEMP\XXX(Version)\... shape like 1.3.7.P18... brackets content"
                    // Wait, the user said: "copy... to E:\UMS_TEMP\XXX(Version)\... format like 1.3.7.P18... which is inside brackets"
                    // So destination is ...\1.3.7.P18\
                    
                    let target_dir = Path::new(&config.local_path).join(&latest.name);

                    // Check if target directory ALREADY exists
                    if target_dir.exists() {
                         // Check if it has content? Simple check: if dir exists, assume copied.
                         // User said: "check if local exists... if exists, log and do not copy"
                         // Also user wants the target folder name to be the FULL name e.g. 2026_02_11_03_34(1.3.9.P02)
                         // So target_dir construction above is correct.
                         result.errors.push(format!("Skipped (Exists): {} -> {}", latest.name, target_dir.display()));
                         continue;
                    }

                    // Perform Copy
                    if let Err(e) = copy_dir_recursive(&latest.path, &target_dir).await {
                        result.errors.push(format!("Failed to copy {}: {}", latest.name, e));
                    } else {
                        result.copied_folders.push(latest.name.clone());
                    }
                } else {
                    // Log that we found it but it's too old?
                    // Optional: result.errors.push(format!("Ignored (Old): {}", latest.name));
                }
            }
        }
    }
    result
}

async fn copy_dir_recursive(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target).await?;
    let mut entries = fs::read_dir(source).await?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_type = entry.file_type().await?;
        let target_path = target.join(entry.file_name());
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&entry.path(), &target_path)).await?;
        } else {
            fs::copy(entry.path(), target_path).await?;
        }
    }
    Ok(())
}
