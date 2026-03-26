use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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
    m.insert(
        ".yaml",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec![],
            multi_end: vec![],
        },
    );
    m.insert(
        ".yml",
        CommentRule {
            single_line: vec!["#"],
            multi_start: vec![],
            multi_end: vec![],
        },
    );
    m
}

fn is_supported_extension(extension: &str) -> bool {
    let rules = get_comment_rules();
    rules.contains_key(extension)
}

fn get_file_extension(filename: &str) -> String {
    Path::new(filename)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

fn normalize_extension(extension: &str) -> String {
    let trimmed = extension
        .trim()
        .trim_start_matches('*')
        .trim()
        .trim_start_matches('.');
    if trimmed.is_empty() {
        return String::new();
    }

    format!(".{}", trimmed.to_lowercase())
}

fn normalize_extensions(extensions: Vec<String>) -> HashSet<String> {
    extensions
        .into_iter()
        .flat_map(|value| {
            value
                .split(|ch: char| {
                    ch == ',' || ch == '，' || ch == ';' || ch == '；' || ch.is_whitespace()
                })
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .map(|value| normalize_extension(&value))
        .filter(|value| !value.is_empty())
        .collect()
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
    pub changed_total: i32,
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
pub struct CodeCountScopeNode {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub children: Vec<CodeCountScopeNode>,
}

// ─── Scanner ─────────────────────────────────────────────────────

struct FileInfo {
    content: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct CodeCountFileFilter {
    include_extensions: HashSet<String>,
    exclude_extensions: HashSet<String>,
}

#[derive(Debug, Default)]
struct CodeCountSelection {
    files: HashSet<String>,
    directories: HashSet<String>,
}

#[derive(Debug, Default)]
struct ScopeTreeNodeDraft {
    key: String,
    label: String,
    kind: String,
    children: BTreeMap<String, ScopeTreeNodeDraft>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LineKind {
    Empty,
    Code,
    Comment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparableLine {
    kind: LineKind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffOp {
    Equal,
    Delete(LineKind),
    Insert(LineKind),
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

fn relative_path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

impl CodeCountSelection {
    fn from_paths(paths: Vec<String>) -> Self {
        let mut selection = Self::default();

        for path in paths
            .into_iter()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
        {
            let path_buf = PathBuf::from(&path);
            let normalized = relative_path_to_string(&path_buf);
            selection.files.insert(normalized);

            let mut parent = path_buf.parent();
            while let Some(current) = parent {
                if current.as_os_str().is_empty() {
                    break;
                }

                selection
                    .directories
                    .insert(relative_path_to_string(current));
                parent = current.parent();
            }
        }

        selection
    }
}

impl CodeCountFileFilter {
    fn from_extensions(
        include_extensions: Option<Vec<String>>,
        exclude_extensions: Option<Vec<String>>,
    ) -> Option<Self> {
        let include_extensions = include_extensions
            .map(normalize_extensions)
            .unwrap_or_default();
        let exclude_extensions = exclude_extensions
            .map(normalize_extensions)
            .unwrap_or_default();

        if include_extensions.is_empty() && exclude_extensions.is_empty() {
            return None;
        }

        Some(Self {
            include_extensions,
            exclude_extensions,
        })
    }

    fn matches_extension(&self, extension: &str) -> bool {
        if !self.exclude_extensions.is_empty() && self.exclude_extensions.contains(extension) {
            return false;
        }

        if self.include_extensions.is_empty() {
            return true;
        }

        self.include_extensions.contains(extension)
    }
}

impl ScopeTreeNodeDraft {
    fn into_node(self) -> CodeCountScopeNode {
        let mut children: Vec<CodeCountScopeNode> = self
            .children
            .into_values()
            .map(ScopeTreeNodeDraft::into_node)
            .collect();

        children.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
            ("directory", "file") => std::cmp::Ordering::Less,
            ("file", "directory") => std::cmp::Ordering::Greater,
            _ => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
        });

        CodeCountScopeNode {
            key: self.key,
            label: self.label,
            kind: self.kind,
            children,
        }
    }
}

fn should_count_file(filename: &str, filter: Option<&CodeCountFileFilter>) -> bool {
    let extension = get_file_extension(filename);
    if extension.is_empty() || !is_supported_extension(&extension) {
        return false;
    }

    filter.map_or(true, |current_filter| {
        current_filter.matches_extension(&extension)
    })
}

fn classify_lines(lines: &[String], file_ext: &str) -> Vec<LineKind> {
    let rules = get_comment_rules();
    let rule = rules.get(file_ext);
    let mut active_multi_end: Option<&'static str> = None;

    lines
        .iter()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return LineKind::Empty;
            }

            if let Some(end_marker) = active_multi_end {
                if trimmed.contains(end_marker) {
                    active_multi_end = None;
                }
                return LineKind::Comment;
            }

            let Some(rule) = rule else {
                return LineKind::Code;
            };

            if rule
                .single_line
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
            {
                return LineKind::Comment;
            }

            for (index, start_marker) in rule.multi_start.iter().enumerate() {
                if !trimmed.starts_with(start_marker) {
                    continue;
                }

                let end_marker = rule
                    .multi_end
                    .get(index)
                    .copied()
                    .or_else(|| rule.multi_end.first().copied());

                if let Some(end_marker) = end_marker {
                    let start_end = start_marker.len();
                    let closes_same_line = trimmed
                        .get(start_end..)
                        .map(|remaining| remaining.contains(end_marker))
                        .unwrap_or(false);

                    if !closes_same_line {
                        active_multi_end = Some(end_marker);
                    }
                }

                return LineKind::Comment;
            }

            LineKind::Code
        })
        .collect()
}

fn should_descend_into_dir(relative_dir: &Path, selection: Option<&CodeCountSelection>) -> bool {
    let Some(selection) = selection else {
        return true;
    };

    selection
        .directories
        .contains(relative_path_to_string(relative_dir).as_str())
}

fn should_include_file(relative_path: &Path, selection: Option<&CodeCountSelection>) -> bool {
    let Some(selection) = selection else {
        return true;
    };

    selection
        .files
        .contains(relative_path_to_string(relative_path).as_str())
}

fn scan_files(
    root_path: &Path,
    selection: Option<&CodeCountSelection>,
    filter: Option<&CodeCountFileFilter>,
) -> Result<HashMap<String, FileInfo>, String> {
    let mut files = HashMap::new();

    fn walk(
        dir: &Path,
        root: &Path,
        files: &mut HashMap<String, FileInfo>,
        selection: Option<&CodeCountSelection>,
        filter: Option<&CodeCountFileFilter>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                let relative_dir = path.strip_prefix(root).map_err(|e| e.to_string())?;
                if should_descend_into_dir(relative_dir, selection) {
                    walk(&path, root, files, selection, filter)?;
                }
            } else {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !should_count_file(&filename, filter) {
                    continue;
                }
                let relative_path = path.strip_prefix(root).map_err(|e| e.to_string())?;
                if !should_include_file(relative_path, selection) {
                    continue;
                }
                let rel_path = relative_path_to_string(relative_path);

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

    walk(root_path, root_path, &mut files, selection, filter)?;
    Ok(files)
}

fn collect_scope_entries(
    root_path: &Path,
    filter: Option<&CodeCountFileFilter>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut directories = Vec::new();
    let mut files = Vec::new();

    fn walk(
        dir: &Path,
        root: &Path,
        directories: &mut Vec<String>,
        files: &mut Vec<String>,
        filter: Option<&CodeCountFileFilter>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.is_dir() {
                let relative_dir = path.strip_prefix(root).map_err(|e| e.to_string())?;
                directories.push(relative_path_to_string(relative_dir));
                walk(&path, root, directories, files, filter)?;
                continue;
            }

            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !should_count_file(&filename, filter) {
                continue;
            }

            let relative_path = path.strip_prefix(root).map_err(|e| e.to_string())?;
            files.push(relative_path_to_string(relative_path));
        }

        Ok(())
    }

    walk(root_path, root_path, &mut directories, &mut files, filter)?;
    Ok((directories, files))
}

fn insert_scope_directory(nodes: &mut BTreeMap<String, ScopeTreeNodeDraft>, relative_path: &str) {
    let path = Path::new(relative_path);
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .filter(|component| !component.is_empty())
        .collect();

    let mut current_nodes = nodes;
    let mut current_path = PathBuf::new();

    for component in components {
        current_path.push(&component);
        let key = relative_path_to_string(&current_path);

        let entry = current_nodes
            .entry(component.clone())
            .or_insert_with(|| ScopeTreeNodeDraft {
                key: key.clone(),
                label: component.clone(),
                kind: "directory".to_string(),
                children: BTreeMap::new(),
            });

        entry.key = key;
        entry.label = component;
        entry.kind = "directory".to_string();
        current_nodes = &mut entry.children;
    }
}

fn insert_scope_file(nodes: &mut BTreeMap<String, ScopeTreeNodeDraft>, relative_path: &str) {
    let path = Path::new(relative_path);
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .filter(|component| !component.is_empty())
        .collect();

    let mut current_nodes = nodes;
    let mut current_path = PathBuf::new();

    for (index, component) in components.iter().enumerate() {
        current_path.push(component);
        let key = relative_path_to_string(&current_path);
        let kind = if index == components.len() - 1 {
            "file"
        } else {
            "directory"
        }
        .to_string();

        let entry = current_nodes
            .entry(component.clone())
            .or_insert_with(|| ScopeTreeNodeDraft {
                key: key.clone(),
                label: component.clone(),
                kind: kind.clone(),
                children: BTreeMap::new(),
            });

        entry.key = key;
        entry.label = component.clone();
        entry.kind = kind;
        current_nodes = &mut entry.children;
    }
}

fn build_scope_tree(
    paths: &[String],
    filter: Option<&CodeCountFileFilter>,
) -> Result<Vec<CodeCountScopeNode>, String> {
    let mut directory_paths = HashSet::new();
    let mut file_paths = HashSet::new();

    for raw_path in paths {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            continue;
        }

        let root_path = Path::new(trimmed);
        if !root_path.is_dir() {
            return Err(format!("Path does not exist: {}", root_path.display()));
        }

        let (directories, files) = collect_scope_entries(root_path, filter)?;

        for directory_path in directories {
            directory_paths.insert(directory_path);
        }

        for file_path in files {
            file_paths.insert(file_path);
        }
    }

    let mut draft_nodes = BTreeMap::new();
    let mut ordered_directories: Vec<String> = directory_paths.into_iter().collect();
    let mut ordered_paths: Vec<String> = file_paths.into_iter().collect();
    ordered_directories.sort();
    ordered_paths.sort();

    for directory_path in ordered_directories {
        insert_scope_directory(&mut draft_nodes, &directory_path);
    }

    for file_path in ordered_paths {
        insert_scope_file(&mut draft_nodes, &file_path);
    }

    let mut nodes: Vec<CodeCountScopeNode> = draft_nodes
        .into_values()
        .map(ScopeTreeNodeDraft::into_node)
        .collect();

    nodes.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
        ("directory", "file") => std::cmp::Ordering::Less,
        ("file", "directory") => std::cmp::Ordering::Greater,
        _ => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
    });

    Ok(nodes)
}

fn preprocess_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.trim_end().to_string()
            }
        })
        .collect()
}

fn build_non_empty_lines(lines: &[String], line_kinds: &[LineKind]) -> Vec<ComparableLine> {
    lines
        .iter()
        .zip(line_kinds.iter())
        .filter_map(|(line, kind)| {
            if line.is_empty() {
                None
            } else {
                Some(ComparableLine {
                    kind: *kind,
                    text: line.clone(),
                })
            }
        })
        .collect()
}

fn diff_sequences(old_lines: &[ComparableLine], new_lines: &[ComparableLine]) -> Vec<DiffOp> {
    let old_len = old_lines.len() as isize;
    let new_len = new_lines.len() as isize;
    if old_len == 0 && new_len == 0 {
        return Vec::new();
    }

    let max = (old_len + new_len) as usize;
    let offset = max as isize;
    let mut v = vec![0isize; max * 2 + 1];
    let mut trace: Vec<Vec<isize>> = Vec::new();

    'outer: for d in 0..=max {
        trace.push(v.clone());

        for k in (-(d as isize)..=(d as isize)).step_by(2) {
            let idx = (k + offset) as usize;
            let mut x = if k == -(d as isize) || (k != d as isize && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;

            while x < old_len && y < new_len && old_lines[x as usize] == new_lines[y as usize] {
                x += 1;
                y += 1;
            }

            v[idx] = x;

            if x >= old_len && y >= new_len {
                break 'outer;
            }
        }
    }

    let mut operations = Vec::new();
    let mut x = old_len;
    let mut y = new_len;

    for d in (0..trace.len()).rev() {
        let snapshot = &trace[d];
        let k = x - y;
        let (prev_x, prev_y) = if d == 0 {
            (0, 0)
        } else {
            let prev_k = if k == -(d as isize)
                || (k != d as isize
                    && snapshot[(k - 1 + offset) as usize] < snapshot[(k + 1 + offset) as usize])
            {
                k + 1
            } else {
                k - 1
            };
            let prev_x = snapshot[(prev_k + offset) as usize];
            let prev_y = prev_x - prev_k;
            (prev_x, prev_y)
        };

        while x > prev_x && y > prev_y {
            operations.push(DiffOp::Equal);
            x -= 1;
            y -= 1;
        }

        if d == 0 {
            break;
        }

        if x == prev_x {
            y -= 1;
            operations.push(DiffOp::Insert(new_lines[y as usize].kind));
        } else {
            x -= 1;
            operations.push(DiffOp::Delete(old_lines[x as usize].kind));
        }
    }

    operations.reverse();
    operations
}

fn flush_edit_block(
    stats: &mut FileStats,
    deleted_code: &mut i32,
    deleted_comment: &mut i32,
    added_code: &mut i32,
    added_comment: &mut i32,
) {
    if *deleted_code == 0 && *deleted_comment == 0 && *added_code == 0 && *added_comment == 0 {
        return;
    }

    let code_modified = (*deleted_code).min(*added_code);
    stats.code_modified += code_modified;
    stats.code_deleted += *deleted_code - code_modified;
    stats.code_added += *added_code - code_modified;

    let comment_modified = (*deleted_comment).min(*added_comment);
    stats.comment_modified += comment_modified;
    stats.comment_deleted += *deleted_comment - comment_modified;
    stats.comment_added += *added_comment - comment_modified;

    *deleted_code = 0;
    *deleted_comment = 0;
    *added_code = 0;
    *added_comment = 0;
}

fn has_changes(stats: &FileStats) -> bool {
    stats.code_added > 0
        || stats.code_deleted > 0
        || stats.code_modified > 0
        || stats.comment_added > 0
        || stats.comment_deleted > 0
        || stats.comment_modified > 0
}

fn calculate_file_stats(
    file_path: &str,
    old_content: &[String],
    new_content: &[String],
) -> FileStats {
    let mut stats = FileStats {
        file_path: file_path.to_string(),
        ..Default::default()
    };

    let file_ext = get_file_extension(file_path);
    let old_lines = preprocess_lines(old_content);
    let new_lines = preprocess_lines(new_content);
    let old_line_kinds = classify_lines(old_content, &file_ext);
    let new_line_kinds = classify_lines(new_content, &file_ext);
    let old_non_empty_lines = build_non_empty_lines(&old_lines, &old_line_kinds);
    let new_non_empty_lines = build_non_empty_lines(&new_lines, &new_line_kinds);
    let operations = diff_sequences(&old_non_empty_lines, &new_non_empty_lines);

    let mut deleted_code = 0;
    let mut deleted_comment = 0;
    let mut added_code = 0;
    let mut added_comment = 0;

    for operation in operations {
        match operation {
            DiffOp::Equal => {
                flush_edit_block(
                    &mut stats,
                    &mut deleted_code,
                    &mut deleted_comment,
                    &mut added_code,
                    &mut added_comment,
                );
            }
            DiffOp::Delete(LineKind::Code) => deleted_code += 1,
            DiffOp::Delete(LineKind::Comment) => deleted_comment += 1,
            DiffOp::Delete(LineKind::Empty) => {}
            DiffOp::Insert(LineKind::Code) => added_code += 1,
            DiffOp::Insert(LineKind::Comment) => added_comment += 1,
            DiffOp::Insert(LineKind::Empty) => {}
        }
    }

    flush_edit_block(
        &mut stats,
        &mut deleted_code,
        &mut deleted_comment,
        &mut added_code,
        &mut added_comment,
    );

    stats
}

fn emit_progress(app_handle: &AppHandle, progress: &CodeCountProgress) {
    let _ = app_handle.emit("code-count-progress", progress);
}

fn compare_directories(
    app_handle: &AppHandle,
    old_path: &str,
    new_path: &str,
    old_selection: Option<&CodeCountSelection>,
    new_selection: Option<&CodeCountSelection>,
    filter: Option<&CodeCountFileFilter>,
) -> Result<CodeCountResult, String> {
    let new_root = Path::new(new_path);
    if !new_root.is_dir() {
        return Err(format!("New path does not exist: {}", new_path));
    }

    // Scan old directory (or skip for new project mode when old_path is empty)
    let old_files = if old_path.is_empty() {
        emit_progress(
            app_handle,
            &CodeCountProgress {
                phase: "scan".to_string(),
                current_file: "New project mode...".to_string(),
                processed_files: 1,
                total_files: 2,
                percent: 25,
            },
        );
        HashMap::new()
    } else {
        let old_root = Path::new(old_path);
        if !old_root.is_dir() {
            return Err(format!("Old path does not exist: {}", old_path));
        }
        emit_progress(
            app_handle,
            &CodeCountProgress {
                phase: "scan".to_string(),
                current_file: "Scanning old directory...".to_string(),
                processed_files: 0,
                total_files: 2,
                percent: 0,
            },
        );
        scan_files(old_root, old_selection, filter)?
    };

    // Phase: scan new directory
    emit_progress(
        app_handle,
        &CodeCountProgress {
            phase: "scan".to_string(),
            current_file: "Scanning new directory...".to_string(),
            processed_files: 1,
            total_files: 2,
            percent: 25,
        },
    );
    let new_files = scan_files(new_root, new_selection, filter)?;

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
            emit_progress(
                app_handle,
                &CodeCountProgress {
                    phase: "diff".to_string(),
                    current_file: file_path.clone(),
                    processed_files: processed,
                    total_files,
                    percent: if total_files > 0 {
                        25 + (processed * 75) / total_files
                    } else {
                        25
                    },
                },
            );
        }

        let old_content = old_files
            .get(file_path)
            .map(|f| &f.content)
            .unwrap_or(&empty_content);
        let new_content = new_files
            .get(file_path)
            .map(|f| &f.content)
            .unwrap_or(&empty_content);

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
    result.operation_summary.deleted_total =
        result.summary.code_deleted + result.summary.comment_deleted;
    result.operation_summary.modified_total =
        result.summary.code_modified + result.summary.comment_modified;
    result.operation_summary.changed_total = result.operation_summary.added_total
        + result.operation_summary.deleted_total
        + result.operation_summary.modified_total;

    emit_progress(
        app_handle,
        &CodeCountProgress {
            phase: "completed".to_string(),
            current_file: "Analysis completed".to_string(),
            processed_files: total_files,
            total_files,
            percent: 100,
        },
    );

    Ok(result)
}

// ─── Tauri Command ───────────────────────────────────────────────

#[tauri::command]
pub async fn code_count_analyze(
    app_handle: AppHandle,
    old_path: String,
    new_path: String,
    included_old_paths: Option<Vec<String>>,
    included_new_paths: Option<Vec<String>>,
    include_extensions: Option<Vec<String>>,
    exclude_extensions: Option<Vec<String>>,
) -> Result<CodeCountResult, String> {
    let old_selection = included_old_paths.map(CodeCountSelection::from_paths);
    let new_selection = included_new_paths.map(CodeCountSelection::from_paths);
    let filter = CodeCountFileFilter::from_extensions(include_extensions, exclude_extensions);

    tauri::async_runtime::spawn_blocking(move || {
        compare_directories(
            &app_handle,
            &old_path,
            &new_path,
            old_selection.as_ref(),
            new_selection.as_ref(),
            filter.as_ref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn code_count_list_scope_tree(
    paths: Vec<String>,
    include_extensions: Option<Vec<String>>,
    exclude_extensions: Option<Vec<String>>,
) -> Result<Vec<CodeCountScopeNode>, String> {
    if paths.iter().all(|path| path.trim().is_empty()) {
        return Ok(Vec::new());
    }

    let filter = CodeCountFileFilter::from_extensions(include_extensions, exclude_extensions);
    build_scope_tree(&paths, filter.as_ref())
}

#[cfg(test)]
mod tests {
    use super::calculate_file_stats;

    #[test]
    fn copied_lines_inserted_elsewhere_count_as_additions() {
        let old_content = vec![
            "usage() {".to_string(),
            "    echo \"line1\"".to_string(),
            "    echo \"line2\"".to_string(),
            "    echo \"line3\"".to_string(),
            "}".to_string(),
        ];
        let new_content = vec![
            "usage() {".to_string(),
            "    echo \"line1\"".to_string(),
            "    echo \"line2\"".to_string(),
            "    echo \"line1\"".to_string(),
            "    echo \"line2\"".to_string(),
            "    echo \"line3\"".to_string(),
            "}".to_string(),
        ];

        let stats = calculate_file_stats("test.sh", &old_content, &new_content);

        assert_eq!(stats.code_added, 2);
        assert_eq!(stats.code_deleted, 0);
        assert_eq!(stats.code_modified, 0);
    }

    #[test]
    fn replaced_line_counts_as_modification() {
        let old_content = vec!["echo \"line1\"".to_string(), "echo \"line2\"".to_string()];
        let new_content = vec![
            "echo \"line1\"".to_string(),
            "echo \"line2 changed\"".to_string(),
        ];

        let stats = calculate_file_stats("test.sh", &old_content, &new_content);

        assert_eq!(stats.code_added, 0);
        assert_eq!(stats.code_deleted, 0);
        assert_eq!(stats.code_modified, 1);
    }
}
