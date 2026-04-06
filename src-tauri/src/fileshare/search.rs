use std::fs;
use std::path::Path;

use serde::Serialize;

use super::ops::{join_relative_path, list_directory, ResolvedRoot};

pub const GLOBAL_SEARCH_MAX_RESULTS: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchNodeKind {
    ShareRoot,
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchNodeMatch {
    pub root_id: String,
    pub root_alias: String,
    pub relative_path: String,
    pub name: String,
    pub kind: SearchNodeKind,
    pub size: Option<u64>,
    pub modified: Option<String>,
    pub display_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchResult {
    pub root_id: String,
    pub root_alias: String,
    pub relative_path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

pub fn search_current_directory(
    path: &Path,
    current_relative_path: &str,
    keyword: &str,
) -> Result<Vec<SearchResult>, String> {
    let needle = keyword.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let entries = list_directory(path)?;
    Ok(entries
        .into_iter()
        .filter(|entry| entry.name.to_lowercase().contains(&needle))
        .map(|entry| SearchResult {
            root_id: String::new(),
            root_alias: String::new(),
            relative_path: join_relative_path(current_relative_path, &entry.relative_path),
            name: entry.name,
            is_dir: entry.is_dir,
            size: entry.size,
            modified: entry.modified,
        })
        .collect())
}

pub fn search_tree_globally(
    roots: &[ResolvedRoot],
    keyword: &str,
) -> Result<Vec<SearchNodeMatch>, String> {
    search_tree_globally_with_limit(roots, keyword, GLOBAL_SEARCH_MAX_RESULTS)
}

pub fn search_tree_globally_with_limit(
    roots: &[ResolvedRoot],
    keyword: &str,
    limit: usize,
) -> Result<Vec<SearchNodeMatch>, String> {
    let needle = keyword.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for root in roots {
        collect_tree_scope_matches(root, None, &needle, limit, &mut results)?;
        if results.len() >= limit {
            break;
        }
    }
    Ok(results)
}

pub fn search_tree_subtree(
    root: &ResolvedRoot,
    relative_path: Option<&str>,
    keyword: &str,
    limit: usize,
) -> Result<Vec<SearchNodeMatch>, String> {
    let needle = keyword.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    collect_tree_scope_matches(root, relative_path, &needle, limit, &mut results)?;
    Ok(results)
}

fn collect_tree_scope_matches(
    root: &ResolvedRoot,
    relative_path: Option<&str>,
    needle: &str,
    limit: usize,
    results: &mut Vec<SearchNodeMatch>,
) -> Result<(), String> {
    if results.len() >= limit {
        return Ok(());
    }

    if let Some(relative_path) = relative_path {
        let normalized = relative_path.trim().trim_matches('/');
        if normalized.is_empty() {
            if root.alias.to_lowercase().contains(needle) {
                results.push(SearchNodeMatch {
                    root_id: root.id.clone(),
                    root_alias: root.alias.clone(),
                    relative_path: String::new(),
                    name: root.alias.clone(),
                    kind: SearchNodeKind::ShareRoot,
                    size: None,
                    modified: None,
                    display_path: root.alias.clone(),
                });
            }
            collect_tree_path_matches(root, &root.path, needle, limit, results)?;
            return Ok(());
        }

        let target = super::ops::resolve_relative_path(root, normalized)?;
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(normalized)
            .to_string();
        if name.to_lowercase().contains(needle) {
            results.push(SearchNodeMatch {
                root_id: root.id.clone(),
                root_alias: root.alias.clone(),
                relative_path: normalized.to_string(),
                name,
                kind: SearchNodeKind::Directory,
                size: None,
                modified: None,
                display_path: display_path(&root.alias, normalized),
            });
        }
        collect_tree_path_matches(root, &target, needle, limit, results)?;
        return Ok(());
    }

    if root.alias.to_lowercase().contains(needle) {
        results.push(SearchNodeMatch {
            root_id: root.id.clone(),
            root_alias: root.alias.clone(),
            relative_path: String::new(),
            name: root.alias.clone(),
            kind: SearchNodeKind::ShareRoot,
            size: None,
            modified: None,
            display_path: root.alias.clone(),
        });
    }

    collect_tree_path_matches(root, &root.path, needle, limit, results)
}

fn collect_tree_path_matches(
    root: &ResolvedRoot,
    current: &Path,
    needle: &str,
    limit: usize,
    results: &mut Vec<SearchNodeMatch>,
) -> Result<(), String> {
    if results.len() >= limit {
        return Ok(());
    }

    let read_dir = fs::read_dir(current)
        .map_err(|e| format!("Failed to read directory {}: {}", current.display(), e))?;

    for entry in read_dir.flatten() {
        if results.len() >= limit {
            break;
        }

        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let relative_path = path
            .strip_prefix(&root.path)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.clone());
        let modified = metadata
            .modified()
            .ok()
            .map(|value| {
                let date_time: chrono::DateTime<chrono::Local> = value.into();
                date_time.format("%Y-%m-%d %H:%M").to_string()
            });

        if name.to_lowercase().contains(needle) {
            results.push(SearchNodeMatch {
                root_id: root.id.clone(),
                root_alias: root.alias.clone(),
                relative_path: relative_path.clone(),
                name: name.clone(),
                kind: if metadata.is_dir() {
                    SearchNodeKind::Directory
                } else {
                    SearchNodeKind::File
                },
                size: if metadata.is_dir() {
                    None
                } else {
                    Some(metadata.len())
                },
                modified,
                display_path: display_path(&root.alias, &relative_path),
            });
        }

        if metadata.is_dir() {
            collect_tree_path_matches(root, &path, needle, limit, results)?;
        }
    }

    Ok(())
}

fn display_path(root_alias: &str, relative_path: &str) -> String {
    if relative_path.trim().is_empty() {
        root_alias.to_string()
    } else {
        format!("{root_alias}/{}", relative_path.trim_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use uuid::Uuid;

    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fst-fileshare-search-{}-{}",
                label,
                Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("test temp dir should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn current_page_search_filters_only_current_directory() {
        let dir = TestDir::new("current");
        fs::create_dir_all(dir.path().join("nested")).expect("nested dir should exist");
        fs::write(dir.path().join("readme.txt"), b"ok").expect("top-level file should exist");
        fs::write(dir.path().join("nested").join("readme.txt"), b"ok")
            .expect("nested file should exist");

        let results = search_current_directory(dir.path(), "", "readme").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "readme.txt");
    }

    #[test]
    fn current_page_search_returns_root_relative_paths() {
        let dir = TestDir::new("current-relative");
        let nested = dir.path().join("实用工具");
        fs::create_dir_all(&nested).expect("nested dir should exist");
        fs::write(
            nested.join("流程图绘制工具Drawio Desktop v13.9.9.txt"),
            b"ok",
        )
        .expect("nested file should exist");

        let results = search_current_directory(
            &nested,
            "实用工具",
            "流程图绘制工具Drawio Desktop",
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].relative_path,
            "实用工具/流程图绘制工具Drawio Desktop v13.9.9.txt"
        );
    }
}
