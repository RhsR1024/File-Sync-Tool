use std::fs;
use std::path::{Path, PathBuf};

use git2::{build::RepoBuilder, Cred, FetchOptions, RemoteCallbacks};
use uuid::Uuid;

pub const GITLAB_BASE_URL: &str = "http://igcode.uniview.com";
pub const GITLAB_PROJECT_PATH: &str = "RD-UNIVIEW/public/pubResList/errorcode";
pub const GITLAB_BRANCH: &str = "main";
pub const GITLAB_FALLBACK_BRANCH: &str = "master";
pub const GITLAB_USERNAME: &str = "cmo_ipc";
pub const GITLAB_PASSWORD: &str = "*Ab64799254";

#[derive(Debug)]
pub enum SyncError {
    Network(String),
    Auth,
    Http(u16),
    Archive(String),
    Io(String),
}

impl SyncError {
    pub fn toast_key(&self) -> &'static str {
        match self {
            SyncError::Network(_) => "errorCodeLookup.toast.networkFail",
            SyncError::Auth => "errorCodeLookup.toast.authFail",
            SyncError::Http(_) => "errorCodeLookup.toast.httpError",
            SyncError::Archive(_) | SyncError::Io(_) => "errorCodeLookup.toast.archiveError",
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Network(message) => write!(f, "network: {message}"),
            SyncError::Auth => write!(f, "auth_failed"),
            SyncError::Http(status) => write!(f, "http_{status}"),
            SyncError::Archive(message) => write!(f, "archive: {message}"),
            SyncError::Io(message) => write!(f, "io: {message}"),
        }
    }
}

impl std::error::Error for SyncError {}

pub fn build_archive_url() -> String {
    build_repo_url_for(GITLAB_PROJECT_PATH)
}

pub fn build_archive_urls() -> Vec<String> {
    vec![build_archive_url()]
}

#[allow(dead_code)]
pub async fn fetch_archive() -> Result<Vec<(String, Vec<u8>)>, SyncError> {
    let mut emit_log = |level: &str, message: String| match level {
        "error" => log::error!("[error_code] {message}"),
        "warn" => log::warn!("[error_code] {message}"),
        _ => log::info!("[error_code] {message}"),
    };
    let (files, _) = fetch_archive_with_logger(&mut emit_log).await?;
    Ok(files)
}

pub async fn fetch_archive_with_logger<F>(
    emit_log: &mut F,
) -> Result<(Vec<(String, Vec<u8>)>, String), SyncError>
where
    F: FnMut(&str, String),
{
    let repo_url = build_archive_urls()
        .into_iter()
        .next()
        .ok_or_else(|| SyncError::Archive("repository_url_missing".to_string()))?;

    fetch_files_from_repo_candidates(&repo_url, &build_branch_candidates(), emit_log)
}

fn fetch_files_from_repo_candidates<F>(
    repo_url: &str,
    branches: &[&str],
    emit_log: &mut F,
) -> Result<(Vec<(String, Vec<u8>)>, String), SyncError>
where
    F: FnMut(&str, String),
{
    let temp_root = TempCheckout::new("fst-error-code")?;
    let mut saw_missing_branch = false;
    let mut last_error: Option<SyncError> = None;

    for branch in branches {
        let checkout_dir = temp_root.path().join(branch);
        emit_log(
            "info",
            format!("准备克隆错误码仓库：branch={} <- {}", branch, repo_url),
        );

        match clone_branch_into(repo_url, branch, &checkout_dir) {
            Ok(()) => {
                emit_log(
                    "info",
                    format!("错误码仓库克隆成功：branch={} <- {}", branch, repo_url),
                );

                let files = collect_csv_files_from_checkout(&checkout_dir)?;
                emit_log(
                    "info",
                    format!(
                        "已收集 {} 个 CSV 文件：branch={} <- {}",
                        files.len(),
                        branch,
                        repo_url
                    ),
                );

                return Ok((files, format!("{repo_url}#{branch}")));
            }
            Err(error) => {
                let detail = error.to_string();
                if is_auth_error_message(&detail) {
                    emit_log(
                        "error",
                        format!(
                            "错误码仓库认证失败：branch={} <- {}，详情={}",
                            branch, repo_url, detail
                        ),
                    );
                    return Err(SyncError::Auth);
                }

                if is_missing_branch_message(&detail) {
                    saw_missing_branch = true;
                    emit_log(
                        "warn",
                        format!(
                            "错误码仓库分支不存在，继续尝试下一个分支：branch={} <- {}，详情={}",
                            branch, repo_url, detail
                        ),
                    );
                    last_error = Some(SyncError::Http(404));
                    continue;
                }

                emit_log(
                    "error",
                    format!(
                        "错误码仓库克隆失败：branch={} <- {}，详情={}",
                        branch, repo_url, detail
                    ),
                );
                return Err(SyncError::Network(detail));
            }
        }
    }

    if saw_missing_branch {
        emit_log("error", "所有错误码仓库候选分支都不存在".to_string());
    }

    Err(last_error.unwrap_or_else(|| SyncError::Http(404)))
}

fn clone_branch_into(repo_url: &str, branch: &str, destination: &Path) -> Result<(), git2::Error> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, allowed| {
        if allowed.is_user_pass_plaintext() {
            return Cred::userpass_plaintext(GITLAB_USERNAME, GITLAB_PASSWORD);
        }
        if allowed.is_username() {
            return Cred::username(username_from_url.unwrap_or(GITLAB_USERNAME));
        }

        Err(git2::Error::from_str("unsupported_credentials"))
    });

    let mut fetch_options = FetchOptions::new();
    if !is_local_repository_url(repo_url) {
        fetch_options.depth(1);
    }
    fetch_options.remote_callbacks(callbacks);

    let mut builder = RepoBuilder::new();
    builder.branch(branch);
    builder.fetch_options(fetch_options);
    builder.clone(repo_url, destination)?;
    Ok(())
}

fn is_local_repository_url(repo_url: &str) -> bool {
    repo_url.starts_with("file://") || Path::new(repo_url).exists()
}

fn collect_csv_files_from_checkout(root: &Path) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir)
            .map_err(|error| SyncError::Io(format!("read_dir({}): {error}", dir.display())))?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                SyncError::Io(format!("read_dir_entry({}): {error}", dir.display()))
            })?;
            let path = entry.path();

            if path.is_dir() {
                pending.push(path);
                continue;
            }

            let Some(name) = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
            else {
                continue;
            };

            if !name.to_ascii_lowercase().ends_with(".csv") {
                continue;
            }

            let bytes = fs::read(&path).map_err(|error| {
                SyncError::Io(format!("read_file({}): {error}", path.display()))
            })?;
            files.push((name, bytes));
        }
    }

    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn build_branch_candidates() -> Vec<&'static str> {
    let mut branches = Vec::new();

    for branch in [GITLAB_BRANCH, GITLAB_FALLBACK_BRANCH] {
        if !branches.contains(&branch) {
            branches.push(branch);
        }
    }

    branches
}

fn build_repo_url_for(project_path: &str) -> String {
    format!(
        "{}/{}.git",
        GITLAB_BASE_URL,
        project_path.trim_end_matches(".git")
    )
}

fn is_auth_error_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("auth_failed")
        || message.contains("authentication")
        || message.contains("credentials")
        || message.contains("401")
        || message.contains("403")
}

fn is_missing_branch_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    (message.contains("remote branch") && message.contains("not found"))
        || (message.contains("reference") && message.contains("not found"))
        || (message.contains("invalid refspec"))
}

struct TempCheckout {
    path: PathBuf,
}

impl TempCheckout {
    fn new(prefix: &str) -> Result<Self, SyncError> {
        let path = std::env::temp_dir().join(format!("{}-{}", prefix, Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|error| {
            SyncError::Io(format!("create_temp_dir({}): {error}", path.display()))
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCheckout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, RepositoryInitOptions};
    use tempfile::TempDir;

    #[test]
    fn archive_url_matches_git_repository_clone_url() {
        let url = build_archive_url();
        assert_eq!(
            url,
            "http://igcode.uniview.com/RD-UNIVIEW/public/pubResList/errorcode.git"
        );
    }

    #[test]
    fn archive_url_candidates_are_plain_git_urls() {
        let urls = build_archive_urls();

        assert!(!urls.is_empty());
        assert!(urls.iter().all(
            |url| url == "http://igcode.uniview.com/RD-UNIVIEW/public/pubResList/errorcode.git"
        ));
    }

    #[test]
    fn collect_csv_files_from_checkout_ignores_non_csv_entries() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("nested")).unwrap();
        fs::write(
            root.path().join("10w.csv"),
            b"code,cn,en,solution,module,remark\n0,A,A,,,",
        )
        .unwrap();
        fs::write(
            root.path().join("nested").join("20w.csv"),
            b"code,cn,en,solution,module,remark\n1,B,B,,,",
        )
        .unwrap();
        fs::write(root.path().join("README.md"), b"# ignore me").unwrap();

        let files = collect_csv_files_from_checkout(root.path()).unwrap();
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();

        assert_eq!(names, vec!["10w.csv", "20w.csv"]);
    }

    #[test]
    fn fetch_archive_with_local_repo_falls_back_to_master() {
        let repo_dir = init_test_repo(
            "master",
            &[("20w.csv", b"code,cn,en,solution,module,remark\n200,B,B,,,")],
        );
        let repo_url = repo_dir.path().to_string_lossy().to_string();
        let mut logs = Vec::<(String, String)>::new();

        let (files, source) = fetch_files_from_repo_candidates(
            &repo_url,
            &["main", "master"],
            &mut |level: &str, message: String| logs.push((level.to_string(), message)),
        )
        .unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "20w.csv");
        assert_eq!(source, format!("{repo_url}#master"));
        assert!(logs
            .iter()
            .any(|(_, message)| message.contains("branch=main")));
        assert!(logs
            .iter()
            .any(|(_, message)| message.contains("branch=master")));
    }

    fn init_test_repo(branch: &str, files: &[(&str, &[u8])]) -> TempDir {
        let dir = TempDir::new().unwrap();
        let mut options = RepositoryInitOptions::new();
        options.initial_head(branch);
        let repo = Repository::init_opts(dir.path(), &options).unwrap();

        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "codex").unwrap();
            config.set_str("user.email", "codex@example.com").unwrap();
        }

        for (relative_path, content) in files {
            let full_path = dir.path().join(relative_path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full_path, content).unwrap();
        }

        let mut index = repo.index().unwrap();
        for (relative_path, _) in files {
            index.add_path(Path::new(relative_path)).unwrap();
        }

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = repo.signature().unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .unwrap();

        dir
    }
}
