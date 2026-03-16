use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path};
use tauri::{AppHandle, Emitter};

// ─── Comment Rules ───────────────────────────────────────────────

#[allow(dead_code)]
struct CommentRule {
    single_line: Vec<&'static str>,
    multi_start: Vec<&'static str>,
    multi_end: Vec<&'static str>,
}

fn get_comment_rules() -> HashMap<&'static str, CommentRule> {
    let mut m = HashMap::new();
    let c_style = || CommentRule {
        single_line: vec!["//"],
        multi_start: vec!["/*"],
        multi_end: vec!["*/"],
    };
    m.insert(".go", c_style());
    m.insert(".java", c_style());
    m.insert(".c", c_style());
    m.insert(".cpp", c_style());
    m.insert(".cc", c_style());
    m.insert(".js", c_style());
    m.insert(".ts", c_style());
    m.insert(".tsx", c_style());
    m.insert(".jsx", c_style());
    m.insert(".vue", c_style());
    m.insert(".swift", c_style());
    m.insert(".kt", c_style());
    m.insert(".dart", c_style());
    m.insert(".rs", c_style());
    m.insert(".scala", c_style());
    m.insert(".scss", c_style());
    m.insert(".less", c_style());
    m.insert(
        ".py",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec![],
            multi_end: vec![],
        },
    );
    m.insert(
        ".sh",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec![],
            multi_end: vec![],
        },
    );
    m.insert(
        ".r",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec![],
            multi_end: vec![],
        },
    );
    m.insert(
        ".pl",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec![],
            multi_end: vec![],
        },
    );
    m.insert(
        ".sql",
        CommentRule {
            single_line: vec!["--"],
            multi_start: vec!["/*"],
            multi_end: vec!["*/"],
        },
    );
    m.insert(
        ".xml",
        CommentRule {
            single_line: vec![],
            multi_start: vec!["<!--"],
            multi_end: vec!["-->"],
        },
    );
    m.insert(
        ".html",
        CommentRule {
            single_line: vec![],
            multi_start: vec!["<!--"],
            multi_end: vec!["-->"],
        },
    );
    m.insert(
        ".css",
        CommentRule {
            single_line: vec![],
            multi_start: vec!["/*"],
            multi_end: vec!["*/"],
        },
    );
    m.insert(
        ".php",
        CommentRule {
            single_line: vec!["//", "#"],
            multi_start: vec!["/*"],
            multi_end: vec!["*/"],
        },
    );
    m.insert(
        ".rb",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec!["=begin"],
            multi_end: vec!["=end"],
        },
    );
    m.insert(
        ".lua",
        CommentRule {
            single_line: vec!["--"],
            multi_start: vec!["--[["],
            multi_end: vec!["]]"],
        },
    );
    m
}

fn is_supported_file(filename: &str) -> bool {
    let ext = get_file_extension(filename);
    let rules = get_comment_rules();
    rules.contains_key(ext.as_str())
}

fn get_file_extension(filename: &str) -> String {
    Path::new(filename)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

fn is_comment(line: &str, file_ext: &str) -> bool {
    let rules = get_comment_rules();
    let rule = match rules.get(file_ext) {
        Some(r) => r,
        None => return false,
    };

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    for prefix in &rule.single_line {
        if trimmed.starts_with(prefix) {
            return true;
        }
    }

    for prefix in &rule.multi_start {
        if trimmed.starts_with(prefix) {
            return true;
        }
    }

    false
}

// ─── Data Models ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileStats {
    pub file_path: String,
    pub code_added: i32,
    pub code_deleted: i32,
    pub code_modified: i32,
    pub comment_added: i32,
    pub comment_deleted: i32,
    pub comment_modified: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub code_added: i32,
    pub code_deleted: i32,
    pub code_modified: i32,
    pub comment_added: i32,
    pub comment_deleted: i32,
    pub comment_modified: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OperationSummary {
    pub added_total: i32,
    pub deleted_total: i32,
    pub modified_total: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodeCountResult {
    pub files: Vec<FileStats>,
    pub summary: Summary,
    pub operation_summary: OperationSummary,
    pub file_type_summary: HashMap<String, Summary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCountProgress {
    pub phase: String,
    pub current_file: String,
    pub processed_files: i32,
    pub total_files: i32,
    pub percent: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCountScopeOption {
    pub key: String,
    pub label: String,
    pub kind: String,
}

// ─── Scanner ─────────────────────────────────────────────────────

struct FileInfo {
    content: Vec<String>,
}

fn read_file_lines(path: &Path) -> Result<Vec<String>, std::io::Error> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for line in reader.lines() {
        lines.push(line?);
    }
    Ok(lines)
}

fn get_top_level_scope_key(relative_path: &Path) -> String {
    let mut components = relative_path.components();
    let first = match components.next() {
        Some(component) => component,
        None => return ".".to_string(),
    };

    if components.next().is_none() {
        ".".to_string()
    } else {
        match first {
            Component::Normal(name) => name.to_string_lossy().to_string(),
            _ => ".".to_string(),
        }
    }
}

fn should_descend_into_dir(relative_dir: &Path, included_roots: Option<&HashSet<String>>) -> bool {
    let Some(included_roots) = included_roots else {
        return true;
    };

    let mut components = relative_dir.components();
    let Some(first) = components.next() else {
        return true;
    };

    if components.next().is_some() {
        return true;
    }

    match first {
        Component::Normal(name) => included_roots.contains(name.to_string_lossy().as_ref()),
        _ => true,
    }
}

fn should_include_file(relative_path: &Path, included_roots: Option<&HashSet<String>>) -> bool {
    let Some(included_roots) = included_roots else {
        return true;
    };

    included_roots.contains(get_top_level_scope_key(relative_path).as_str())
}

fn scan_files(
    root_path: &Path,
    included_roots: Option<&HashSet<String>>,
) -> Result<HashMap<String, FileInfo>, String> {
    let mut files = HashMap::new();

    fn walk(
        dir: &Path,
        root: &Path,
        files: &mut HashMap<String, FileInfo>,
        included_roots: Option<&HashSet<String>>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                let relative_dir = path.strip_prefix(root).map_err(|e| e.to_string())?;
                if should_descend_into_dir(relative_dir, included_roots) {
                    walk(&path, root, files, included_roots)?;
                }
            } else {
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                if !is_supported_file(&filename) {
                    continue;
                }
                let relative_path = path.strip_prefix(root).map_err(|e| e.to_string())?;
                if !should_include_file(relative_path, included_roots) {
                    continue;
                }
                let rel_path = relative_path.to_string_lossy().to_string();

                match read_file_lines(&path) {
                    Ok(content) => {
                        files.insert(rel_path, FileInfo { content });
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to read file {}: {}", path.display(), e);
                    }
                }
            }
        }
        Ok(())
    }

    walk(root_path, root_path, &mut files, included_roots)?;
    Ok(files)
}

fn list_scope_options(root_path: &Path) -> Result<Vec<CodeCountScopeOption>, String> {
    if !root_path.is_dir() {
        return Err(format!("Path does not exist: {}", root_path.display()));
    }

    let mut has_root_files = false;
    let mut directories: Vec<String> = Vec::new();

    let entries = fs::read_dir(root_path)
        .map_err(|e| format!("Failed to read directory {}: {}", root_path.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            directories.push(name);
            continue;
        }

        if path.is_file() && is_supported_file(&name) {
            has_root_files = true;
        }
    }

    directories.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    let mut options = Vec::new();
    if has_root_files {
        options.push(CodeCountScopeOption {
            key: ".".to_string(),
            label: "根目录文件".to_string(),
            kind: "root".to_string(),
        });
    }

    for directory in directories {
        options.push(CodeCountScopeOption {
            key: directory.clone(),
            label: directory,
            kind: "directory".to_string(),
        });
    }

    Ok(options)
}

fn preprocess_lines(lines: &[String]) -> Vec<String> {
    lines.iter().map(|l| l.trim().to_string()).collect()
}

fn has_changes(stats: &FileStats) -> bool {
    stats.code_added > 0
        || stats.code_deleted > 0
        || stats.code_modified > 0
        || stats.comment_added > 0
        || stats.comment_deleted > 0
        || stats.comment_modified > 0
}

fn calculate_file_stats(file_path: &str, old_content: &[String], new_content: &[String]) -> FileStats {
    let mut stats = FileStats {
        file_path: file_path.to_string(),
        ..Default::default()
    };

    let file_ext = get_file_extension(file_path);
    let old_lines = preprocess_lines(old_content);
    let new_lines = preprocess_lines(new_content);

    // Build line-content -> indices maps
    let mut old_line_map: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut new_line_map: HashMap<&str, Vec<usize>> = HashMap::new();

    for (i, line) in old_lines.iter().enumerate() {
        if !line.is_empty() {
            old_line_map.entry(line.as_str()).or_default().push(i);
        }
    }
    for (i, line) in new_lines.iter().enumerate() {
        if !line.is_empty() {
            new_line_map.entry(line.as_str()).or_default().push(i);
        }
    }

    let mut old_matched = vec![false; old_lines.len()];
    let mut new_matched = vec![false; new_lines.len()];

    // Match identical lines
    for (content, old_indices) in &old_line_map {
        if let Some(new_indices) = new_line_map.get(content) {
            let min_len = old_indices.len().min(new_indices.len());
            for i in 0..min_len {
                old_matched[old_indices[i]] = true;
                new_matched[new_indices[i]] = true;
            }
        }
    }

    // Count unmatched old lines as deletions
    for (i, line) in old_lines.iter().enumerate() {
        if !old_matched[i] && !line.is_empty() {
            if is_comment(&old_content[i], &file_ext) {
                stats.comment_deleted += 1;
            } else {
                stats.code_deleted += 1;
            }
        }
    }

    // Count unmatched new lines as additions
    for (i, line) in new_lines.iter().enumerate() {
        if !new_matched[i] && !line.is_empty() {
            if is_comment(&new_content[i], &file_ext) {
                stats.comment_added += 1;
            } else {
                stats.code_added += 1;
            }
        }
    }

    stats
}

fn emit_progress(app_handle: &AppHandle, progress: &CodeCountProgress) {
    let _ = app_handle.emit("code-count-progress", progress);
}

pub fn compare_directories(
    app_handle: &AppHandle,
    old_path: &str,
    new_path: &str,
    included_roots: Option<&HashSet<String>>,
) -> Result<CodeCountResult, String> {
    let new_root = Path::new(new_path);
    if !new_root.is_dir() {
        return Err(format!("New path does not exist: {}", new_path));
    }

    // Scan old directory (or skip for new project mode when old_path is empty)
    let old_files = if old_path.is_empty() {
        emit_progress(app_handle, &CodeCountProgress {
            phase: "scan".to_string(),
            current_file: "New project mode...".to_string(),
            processed_files: 1,
            total_files: 2,
            percent: 25,
        });
        HashMap::new()
    } else {
        let old_root = Path::new(old_path);
        if !old_root.is_dir() {
            return Err(format!("Old path does not exist: {}", old_path));
        }
        emit_progress(app_handle, &CodeCountProgress {
            phase: "scan".to_string(),
            current_file: "Scanning old directory...".to_string(),
            processed_files: 0,
            total_files: 2,
            percent: 0,
        });
        scan_files(old_root, included_roots)?
    };

    // Phase: scan new directory
    emit_progress(app_handle, &CodeCountProgress {
        phase: "scan".to_string(),
        current_file: "Scanning new directory...".to_string(),
        processed_files: 1,
        total_files: 2,
        percent: 25,
    });
    let new_files = scan_files(new_root, included_roots)?;

    // Build union of all file paths
    let mut all_files: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for key in old_files.keys().chain(new_files.keys()) {
        if seen.insert(key.clone()) {
            all_files.push(key.clone());
        }
    }
    all_files.sort();

    let total_files = all_files.len() as i32;
    let mut result = CodeCountResult::default();
    let empty_content: Vec<String> = Vec::new();

    for (idx, file_path) in all_files.iter().enumerate() {
        let processed = idx as i32;
        if processed % 50 == 0 || processed == total_files - 1 {
            emit_progress(app_handle, &CodeCountProgress {
                phase: "diff".to_string(),
                current_file: file_path.clone(),
                processed_files: processed,
                total_files,
                percent: if total_files > 0 { 25 + (processed * 75) / total_files } else { 25 },
            });
        }

        let old_content = old_files.get(file_path).map(|f| &f.content).unwrap_or(&empty_content);
        let new_content = new_files.get(file_path).map(|f| &f.content).unwrap_or(&empty_content);

        let stats = calculate_file_stats(file_path, old_content, new_content);

        if has_changes(&stats) {
            result.summary.code_added += stats.code_added;
            result.summary.code_deleted += stats.code_deleted;
            result.summary.code_modified += stats.code_modified;
            result.summary.comment_added += stats.comment_added;
            result.summary.comment_deleted += stats.comment_deleted;
            result.summary.comment_modified += stats.comment_modified;

            let ext = get_file_extension(file_path);
            let ext_summary = result.file_type_summary.entry(ext).or_default();
            ext_summary.code_added += stats.code_added;
            ext_summary.code_deleted += stats.code_deleted;
            ext_summary.code_modified += stats.code_modified;
            ext_summary.comment_added += stats.comment_added;
            ext_summary.comment_deleted += stats.comment_deleted;
            ext_summary.comment_modified += stats.comment_modified;

            result.files.push(stats);
        }
    }

    result.operation_summary.added_total = result.summary.code_added + result.summary.comment_added;
    result.operation_summary.deleted_total = result.summary.code_deleted + result.summary.comment_deleted;
    result.operation_summary.modified_total = result.summary.code_modified + result.summary.comment_modified;

    emit_progress(app_handle, &CodeCountProgress {
        phase: "completed".to_string(),
        current_file: "Analysis completed".to_string(),
        processed_files: total_files,
        total_files,
        percent: 100,
    });

    Ok(result)
}

// ─── Tauri Command ───────────────────────────────────────────────

#[tauri::command]
pub async fn code_count_analyze(
    app_handle: AppHandle,
    old_path: String,
    new_path: String,
    included_roots: Option<Vec<String>>,
) -> Result<CodeCountResult, String> {
    let included_roots = included_roots.map(|roots| {
        roots
            .into_iter()
            .map(|root| root.trim().to_string())
            .filter(|root| !root.is_empty())
            .collect::<HashSet<_>>()
    });

    tauri::async_runtime::spawn_blocking(move || {
        compare_directories(&app_handle, &old_path, &new_path, included_roots.as_ref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn code_count_list_scope_options(path: String) -> Result<Vec<CodeCountScopeOption>, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    list_scope_options(Path::new(trimmed))
}
