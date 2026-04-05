use std::fs;
use std::path::Path;

use serde::Serialize;

use super::ops::{list_directory, ResolvedRoot};

pub const GLOBAL_SEARCH_MAX_RESULTS: usize = 500;

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

pub fn search_current_directory(path: &Path, keyword: &str) -> Result<Vec<SearchResult>, String> {
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
            relative_path: entry.relative_path,
            name: entry.name,
            is_dir: entry.is_dir,
            size: entry.size,
            modified: entry.modified,
        })
        .collect())
}

pub fn search_all_roots(roots: &[ResolvedRoot], keyword: &str) -> Result<Vec<SearchResult>, String> {
    search_all_roots_with_limit(roots, keyword, GLOBAL_SEARCH_MAX_RESULTS)
}

pub fn search_all_roots_with_limit(
    roots: &[ResolvedRoot],
    keyword: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    let needle = keyword.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for root in roots {
        collect_root_matches(root, &root.path, &needle, limit, &mut results)?;
        if results.len() >= limit {
            break;
        }
    }
    Ok(results)
}

fn collect_root_matches(
    root: &ResolvedRoot,
    current: &Path,
    needle: &str,
    limit: usize,
    results: &mut Vec<SearchResult>,
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
            })
            .unwrap_or_default();

        if name.to_lowercase().contains(needle) {
            results.push(SearchResult {
                root_id: root.id.clone(),
                root_alias: root.alias.clone(),
                relative_path: relative_path.clone(),
                name: name.clone(),
                is_dir: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
                modified,
            });
        }

        if metadata.is_dir() {
            collect_root_matches(root, &path, needle, limit, results)?;
        }
    }

    Ok(())
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

        let results = search_current_directory(dir.path(), "readme").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "readme.txt");
    }
}
