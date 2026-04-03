use crate::config::{LocalCommandGroup, LocalScriptBinding, OnFailure};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::Emitter;

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandResult {
    pub command: String,
    pub resolved_command: String,
    pub exit_code: Option<i32>,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub elapsed_seconds: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupResult {
    pub group_id: String,
    pub group_name: String,
    pub success: bool,
    pub command_results: Vec<CommandResult>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalExecResult {
    pub success: bool,
    pub group_results: Vec<GroupResult>,
    pub aborted: bool,
}

pub struct LocalExecContext {
    pub folder_name: String,
    pub local_target: String,
    pub source_path: String,
    pub filename: String,
}

/// Auto-detect interpreter based on file extension of the first token.
pub fn resolve_command(raw_cmd: &str) -> String {
    let trimmed = raw_cmd.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    // Find the first token (potential script path)
    let first_token = trimmed.split_whitespace().next().unwrap_or("");
    let lower = first_token.to_lowercase();

    if lower.ends_with(".py") {
        format!("python {}", trimmed)
    } else if lower.ends_with(".ps1") {
        format!("powershell -ExecutionPolicy Bypass -File {}", trimmed)
    } else if lower.ends_with(".bat") || lower.ends_with(".cmd") {
        format!("cmd /c {}", trimmed)
    } else {
        // Already a full command or unknown — run through cmd /c
        format!("cmd /c {}", trimmed)
    }
}

/// Replace `${variable}` placeholders with values from the context.
pub fn substitute_variables(cmd: &str, ctx: &LocalExecContext) -> String {
    cmd.replace("${folder_name}", &ctx.folder_name)
        .replace("${local_target}", &ctx.local_target)
        .replace("${source_path}", &ctx.source_path)
        .replace("${filename}", &ctx.filename)
}

/// Scan a directory for the first `.tar.gz` file and return its filename.
/// Returns an empty string if none is found.
pub fn find_tar_gz_filename(dir: &Path) -> String {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_lowercase().ends_with(".tar.gz") {
                return name;
            }
        }
    }
    String::new()
}

fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: &str, level: &str) {
    let _ = app_handle.emit(
        "log-message",
        serde_json::json!({ "msg": msg, "level": level }),
    );
}

/// Execute a single command as a subprocess and capture its output.
pub fn run_single_command<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    cmd: &str,
    working_dir: &Path,
    should_cancel: &Arc<AtomicBool>,
) -> CommandResult {
    let start = Instant::now();

    if should_cancel.load(Ordering::SeqCst) {
        return CommandResult {
            command: cmd.to_string(),
            resolved_command: cmd.to_string(),
            exit_code: None,
            stdout_excerpt: String::new(),
            stderr_excerpt: "Cancelled before execution".to_string(),
            elapsed_seconds: 0.0,
        };
    }

    emit_log(app_handle, &format!("[LocalExec] > {}", cmd), "command");

    let output = std::process::Command::new("cmd")
        .args(["/c", cmd])
        .current_dir(working_dir)
        .output();

    let elapsed = start.elapsed().as_secs_f64();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let exit_code = out.status.code();
            let success = out.status.success();

            // Truncate to 2000 chars for excerpts
            let stdout_excerpt = if stdout.len() > 2000 {
                stdout[..2000].to_string()
            } else {
                stdout.clone()
            };
            let stderr_excerpt = if stderr.len() > 2000 {
                stderr[..2000].to_string()
            } else {
                stderr.clone()
            };

            if !stdout.trim().is_empty() {
                for line in stdout.lines().take(50) {
                    emit_log(app_handle, &format!("[LocalExec] {}", line), "info");
                }
            }

            if success {
                emit_log(
                    app_handle,
                    &format!(
                        "[LocalExec] Command completed (exit code: {:?}, {:.1}s)",
                        exit_code, elapsed
                    ),
                    "success",
                );
            } else {
                if !stderr.trim().is_empty() {
                    for line in stderr.lines().take(20) {
                        emit_log(
                            app_handle,
                            &format!("[LocalExec] STDERR: {}", line),
                            "error",
                        );
                    }
                }
                emit_log(
                    app_handle,
                    &format!(
                        "[LocalExec] Command failed (exit code: {:?}, {:.1}s)",
                        exit_code, elapsed
                    ),
                    "error",
                );
            }

            CommandResult {
                command: cmd.to_string(),
                resolved_command: cmd.to_string(),
                exit_code,
                stdout_excerpt,
                stderr_excerpt,
                elapsed_seconds: elapsed,
            }
        }
        Err(e) => {
            let msg = format!("[LocalExec] Failed to execute: {}", e);
            emit_log(app_handle, &msg, "error");
            CommandResult {
                command: cmd.to_string(),
                resolved_command: cmd.to_string(),
                exit_code: None,
                stdout_excerpt: String::new(),
                stderr_excerpt: e.to_string(),
                elapsed_seconds: elapsed,
            }
        }
    }
}

/// Execute all local script groups bound to a task.
/// Groups are executed sequentially; within each group, commands run sequentially.
/// Respects `on_failure` policy and cancellation.
pub fn execute_local_scripts<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    binding: &LocalScriptBinding,
    all_groups: &[LocalCommandGroup],
    ctx: &LocalExecContext,
    should_cancel: Arc<AtomicBool>,
) -> LocalExecResult {
    let mut group_results = Vec::new();
    let mut all_success = true;
    let mut aborted = false;
    let working_dir = Path::new(&ctx.local_target);

    emit_log(
        app_handle,
        &format!(
            "[LocalExec] Starting local script execution ({} groups bound)",
            binding.command_group_ids.len()
        ),
        "info",
    );

    for group_id in &binding.command_group_ids {
        if should_cancel.load(Ordering::SeqCst) {
            aborted = true;
            break;
        }

        let group = match all_groups.iter().find(|g| &g.id == group_id) {
            Some(g) => g,
            None => {
                emit_log(
                    app_handle,
                    &format!("[LocalExec] Group '{}' not found, skipping", group_id),
                    "warn",
                );
                continue;
            }
        };

        emit_log(
            app_handle,
            &format!(
                "[LocalExec] Executing group: {} ({} commands)",
                group.name,
                group.commands.len()
            ),
            "info",
        );

        let mut command_results = Vec::new();
        let mut group_success = true;

        for raw_cmd in &group.commands {
            if should_cancel.load(Ordering::SeqCst) {
                aborted = true;
                break;
            }

            let resolved = resolve_command(raw_cmd);
            let substituted = substitute_variables(&resolved, ctx);

            let result = run_single_command(app_handle, &substituted, working_dir, &should_cancel);
            let cmd_success = result.exit_code == Some(0);
            if !cmd_success {
                group_success = false;
            }
            command_results.push(result);
        }

        if !group_success {
            all_success = false;
            emit_log(
                app_handle,
                &format!("[LocalExec] Group '{}' failed", group.name),
                "error",
            );

            if group.on_failure == OnFailure::Abort {
                emit_log(
                    app_handle,
                    "[LocalExec] Abort policy triggered — stopping remaining groups",
                    "error",
                );
                aborted = true;
            }
        } else {
            emit_log(
                app_handle,
                &format!("[LocalExec] Group '{}' completed successfully", group.name),
                "success",
            );
        }

        group_results.push(GroupResult {
            group_id: group.id.clone(),
            group_name: group.name.clone(),
            success: group_success,
            command_results,
        });

        if aborted {
            break;
        }
    }

    let overall_success = all_success && !aborted;
    let level = if overall_success { "success" } else { "error" };
    emit_log(
        app_handle,
        &format!(
            "[LocalExec] Local script execution {} ({} groups executed)",
            if overall_success {
                "completed"
            } else {
                "finished with errors"
            },
            group_results.len()
        ),
        level,
    );

    LocalExecResult {
        success: overall_success,
        group_results,
        aborted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_py_script() {
        let result = resolve_command("C:\\scripts\\build.py --flag");
        assert_eq!(result, "python C:\\scripts\\build.py --flag");
    }

    #[test]
    fn resolve_ps1_script() {
        let result = resolve_command("deploy.ps1");
        assert!(result.starts_with("powershell -ExecutionPolicy Bypass -File"));
        assert!(result.contains("deploy.ps1"));
    }

    #[test]
    fn resolve_bat_script() {
        let result = resolve_command("run.bat arg1");
        assert_eq!(result, "cmd /c run.bat arg1");
    }

    #[test]
    fn resolve_cmd_script() {
        let result = resolve_command("build.cmd");
        assert_eq!(result, "cmd /c build.cmd");
    }

    #[test]
    fn resolve_passthrough_command() {
        let result = resolve_command("python -m pytest");
        assert_eq!(result, "cmd /c python -m pytest");
    }

    #[test]
    fn resolve_empty_command() {
        let result = resolve_command("");
        assert_eq!(result, "");
    }

    #[test]
    fn substitute_all_variables() {
        let ctx = LocalExecContext {
            folder_name: "Release_01".to_string(),
            local_target: "E:\\Builds\\Release_01".to_string(),
            source_path: "\\\\server\\share\\Release_01".to_string(),
            filename: "pkg-1.0.tar.gz".to_string(),
        };
        let result = substitute_variables("echo ${folder_name} ${local_target} ${filename}", &ctx);
        assert_eq!(
            result,
            "echo Release_01 E:\\Builds\\Release_01 pkg-1.0.tar.gz"
        );
    }

    #[test]
    fn substitute_no_variables() {
        let ctx = LocalExecContext {
            folder_name: "f".to_string(),
            local_target: "l".to_string(),
            source_path: "s".to_string(),
            filename: "n".to_string(),
        };
        let result = substitute_variables("echo hello", &ctx);
        assert_eq!(result, "echo hello");
    }

    #[test]
    fn substitute_source_path() {
        let ctx = LocalExecContext {
            folder_name: "f".to_string(),
            local_target: "l".to_string(),
            source_path: "\\\\net\\src".to_string(),
            filename: "n".to_string(),
        };
        let result = substitute_variables("xcopy ${source_path}", &ctx);
        assert_eq!(result, "xcopy \\\\net\\src");
    }
}
