# WebView2 Runtime Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让裸 `.exe` 分发的 File Sync Tool 在 WebView2 Runtime 缺失时,于 Tauri 启动前自动完成"检测 → 原生确认 → 从内部更新服务器下载 → SHA-256 校验 → 静默安装 → 重启"闭环。

**Architecture:** 在 `main()` 中、`install_panic_log_hook()` 之后且 **`single_instance_guard::ensure_single_instance()` 之前**插入纯 Win32 自举模块 `webview2_bootstrap`。下载复用从 `updater/download.rs` 抽取的通用校验下载器 `download_verify.rs`(自建 tokio runtime `block_on`)。原生进度窗 + MessageBox 降级,失败即关(fail closed)。

**Tech Stack:** Rust(std + tokio 1.49 + reqwest 0.11 `no_proxy` + sha2)、windows crate 0.58(Registry / WindowsAndMessaging / Controls)、wiremock + tempfile 测试。

**规格来源:**
- 设计 spec:`docs/superpowers/specs/2026-07-09-webview2-bootstrap-design.md`
- 契约 spec:`.trellis/spec/backend/webview2-bootstrap.md`
- 审查修订(本计划 Task 1 落盘):单实例互斥体交接竞态、`FST_WEBVIEW2_BOOTSTRAP_RESTARTED` 读后即删、确认框用 MB_YESNO、自建 tokio runtime。

## Global Constraints

- 仅 Windows 生效;非 Windows 构建 `ensure_webview2_runtime()` 直接返回 `Continue`。
- **不新增 crate 依赖**;windows crate 仅追加 features:`Win32_UI_Controls`、`Win32_System_LibraryLoader`、`Win32_UI_Input_KeyboardAndMouse`(`Win32_System_Registry` 已存在)。
- 服务器契约(逐字):`${update_server_url}/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe` 与同名 `.exe.sha256`。
- 环境变量(逐字):`FST_SKIP_WEBVIEW2_BOOTSTRAP`、`FST_WEBVIEW2_BOOTSTRAP_RESTARTED`;后者**读取后立即 `remove_var`**,防止透传给后代进程(updater.bat 重启链)。
- 下载临时目录(逐字):`%TEMP%\file-sync-tool-webview2\`,先写 `.part` 再改名。
- 安装参数(逐字):`/silent /install`;不强制提权;零退出码后轮询检测最多 60 秒(间隔 2 秒)。
- 默认更新服务器 URL(逐字):`http://192.115.1.3:8080`(复用 `config.rs` 的 `default_update_server_url`)。
- 注册表检测(逐字):`HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` 与 `HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` 的 `pv`;空/缺失/`0.0.0.0` 视为缺失。
- **调用位置**:`install_panic_log_hook()` 之后、`single_instance_guard::ensure_single_instance()` **之前**(守卫互斥体句柄存活到进程退出,若自举父进程先占守卫再重启,子进程会误判"已有实例"而静默退出)。
- 原生对话框文本中英双语(此阶段无 vue-i18n)。
- 提交信息用中文;**不要对 `src-tauri/src/main.rs` 跑 rustfmt**(会递归格式化全 crate,见项目记忆);新文件可单独 `rustfmt`;不要以 clippy 全绿为门槛(存量 deny error 必然 exit 101)。
- 每个任务以 `cargo test --manifest-path src-tauri/Cargo.toml -p app <filter>` 验证;完整 `cmd /c pnpm tauri:build:versioned-exe` 在 Task 12 统一执行。

---

## File Structure

| 文件 | 职责 | 动作 |
| --- | --- | --- |
| `src-tauri/src/download_verify.rs` | 通用"流式下载 + SHA-256 校验 + 取消 + 进度"下载器(中立错误类型) | 新建(自 updater 抽取) |
| `src-tauri/src/updater/download.rs` | 变为薄包装:委托 download_verify,错误映射回 `UpdaterError` | 修改 |
| `src-tauri/src/webview2_bootstrap/mod.rs` | 编排:preflight 决策、自举互斥体、工作线程管线、`ensure_webview2_runtime()` | 新建 |
| `src-tauri/src/webview2_bootstrap/detect.rs` | 注册表 `pv` 检测 + 版本判定纯函数 | 新建 |
| `src-tauri/src/webview2_bootstrap/startup_config.rs` | 无 AppHandle 解析生效的 `update_server_url`(pivot.json 支持) | 新建 |
| `src-tauri/src/webview2_bootstrap/server.rs` | 资产 URL 拼接 | 新建 |
| `src-tauri/src/webview2_bootstrap/sha256_file.rs` | `.sha256` 文本解析 | 新建 |
| `src-tauri/src/webview2_bootstrap/download.rs` | 安装包下载编排(自建 runtime,`.part`→改名) | 新建 |
| `src-tauri/src/webview2_bootstrap/install.rs` | 静默安装 + 装后轮询 | 新建 |
| `src-tauri/src/webview2_bootstrap/restart.rs` | 带防环 env 的自重启 | 新建 |
| `src-tauri/src/webview2_bootstrap/native_ui.rs` | MessageBox 对话框 + Win32 进度窗 + 降级 | 新建 |
| `src-tauri/src/main.rs` | 声明模块、main() 接线 | 修改 |
| `src-tauri/src/config.rs` | `default_update_server_url`/`normalize_update_server_url`/`validate_update_server_url` 改 `pub(crate)` | 修改 |
| `src-tauri/Cargo.toml` | windows features 追加 | 修改 |
| 两份 spec 文档 | 补审查修订 | 修改 |
| `scripts/release-server/README.md`、`UPDATE_DEPLOYMENT_GUIDE.md` | webview2 资产部署说明 | 修改 |

---

### Task 1: 修订 spec 文档(审查发现的缺口)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-09-webview2-bootstrap-design.md`
- Modify: `.trellis/spec/backend/webview2-bootstrap.md`

**Interfaces:**
- Produces: 后续所有任务遵循的四条修订契约(单实例顺序、env 消费、MB_YESNO、自建 runtime)。

- [ ] **Step 1: 设计 spec 增加 §2.6 并修订 §3/§7.1/§13**

在 `docs/superpowers/specs/2026-07-09-webview2-bootstrap-design.md` 的 `### 2.5 User Experience` 小节之后追加:

```markdown
### 2.6 Single-Instance Interaction (review amendment)

`main()` already runs `single_instance_guard::ensure_single_instance()` whose
guard mutex handle is intentionally leaked until process exit. The bootstrap
therefore runs BEFORE the single-instance guard:

- If bootstrap ran after the guard, the restarted child would race the dying
  parent's guard mutex, take the `notify_primary_and_exit` path, fail to find
  the plugin's hidden window (the bootstrap parent never created Tauri
  windows), and silently exit — the app would never come back after install.
- Bootstrap protects itself against double-launch with its own named mutex
  `com.filesync.tool-wv2-bootstrap` (created only when the Runtime is
  missing). A second instance during install shows an info dialog and exits.
- `FST_WEBVIEW2_BOOTSTRAP_RESTARTED` is read once at bootstrap entry and
  immediately removed from the process environment so it never propagates to
  descendants (e.g. the updater.bat restart chain).
- The confirmation dialog uses `MessageBoxW` `MB_YESNO` (Yes = install). The
  custom button labels from §7.1 would require TaskDialog/comctl32 v6 and are
  intentionally not used at this fragile pre-UI stage.
- The async download helper requires a Tokio runtime; bootstrap builds its own
  current-thread runtime and `block_on`s it (Tauri's runtime does not exist yet).
```

- [ ] **Step 2: 契约 spec 同步修订**

在 `.trellis/spec/backend/webview2-bootstrap.md` 的 `- Placement rules:` 列表末尾追加两行:

```markdown
  - Bootstrap must run BEFORE `single_instance_guard::ensure_single_instance` (the guard mutex lives until process exit and would make the restarted child silently exit).
  - Bootstrap prevents concurrent installs with its own named mutex `com.filesync.tool-wv2-bootstrap`; a losing instance shows an info dialog and exits.
```

在 `- Restart rules:` 列表末尾追加一行:

```markdown
  - `FST_WEBVIEW2_BOOTSTRAP_RESTARTED` is consumed (read then `remove_var`) at bootstrap entry so it does not propagate to descendant processes.
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-09-webview2-bootstrap-design.md .trellis/spec/backend/webview2-bootstrap.md
git commit -m "docs(webview2): spec 补充单实例交接竞态、env 消费与确认框修订"
```

---

### Task 2: 抽取通用校验下载器 `download_verify.rs`

**Files:**
- Create: `src-tauri/src/download_verify.rs`
- Modify: `src-tauri/src/updater/download.rs`(变薄包装,原测试原地保留)
- Modify: `src-tauri/src/main.rs`(`mod disk_cleanup;` 之后插入 `mod download_verify;`)

**Interfaces:**
- Produces:
  - `pub enum DownloadError { Network(String), Http(u16), Io(String), VerifyFailed, Cancelled }`(impl `Display`)
  - `pub async fn download_to_file<F>(url: &str, dest: &Path, expected_sha256_hex: &str, cancel: watch::Receiver<bool>, on_progress: F) -> Result<(), DownloadError> where F: FnMut(u64, Option<u64>) + Send + 'static`
  - `pub fn sha256_hex(bytes: &[u8]) -> String`、`pub fn verify_bytes(bytes: &[u8], expected: &str) -> bool`
- Consumes: 现有 `src-tauri/src/updater/download.rs` 的实现(整体搬移改名)。

- [ ] **Step 1: 新建 `src-tauri/src/download_verify.rs`**

将现有 `updater/download.rs` 的全部实现(不含 `#[cfg(test)] mod tests`)搬入,做以下机械替换:
- `use crate::updater::UpdaterError;` → 删除;
- 所有 `UpdaterError::X` → `DownloadError::X`;
- 文件头注释改为 `//! Shared streaming download + SHA-256 verification, usable by the updater and the WebView2 bootstrap.`;
- 文件顶部新增错误类型:

```rust
#[derive(Debug)]
pub enum DownloadError {
    Network(String),
    Http(u16),
    Io(String),
    VerifyFailed,
    Cancelled,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Network(message) => write!(f, "network: {message}"),
            DownloadError::Http(status) => write!(f, "http_{status}"),
            DownloadError::Io(message) => write!(f, "io: {message}"),
            DownloadError::VerifyFailed => write!(f, "verify_failed"),
            DownloadError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for DownloadError {}
```

`sha256_hex` / `verify_bytes` / `hex_encode` / `download_to_file` 函数体逐字保留(仅错误类型替换)。

- [ ] **Step 2: `updater/download.rs` 改为薄包装**

整文件替换为:

```rust
//! Updater-flavored wrapper over the shared verified downloader
//! (`crate::download_verify`). Kept so existing call sites and tests are
//! unchanged; the WebView2 bootstrap uses `download_verify` directly.

use crate::download_verify::{self, DownloadError};
use crate::updater::UpdaterError;
use std::path::Path;
use tokio::sync::watch;

pub use crate::download_verify::{sha256_hex, verify_bytes};

impl From<DownloadError> for UpdaterError {
    fn from(error: DownloadError) -> Self {
        match error {
            DownloadError::Network(message) => UpdaterError::Network(message),
            DownloadError::Http(status) => UpdaterError::Http(status),
            DownloadError::Io(message) => UpdaterError::Io(message),
            DownloadError::VerifyFailed => UpdaterError::VerifyFailed,
            DownloadError::Cancelled => UpdaterError::Cancelled,
        }
    }
}

pub async fn download_to_file<F>(
    url: &str,
    dest: &Path,
    expected_sha256_hex: &str,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<(), UpdaterError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    download_verify::download_to_file(url, dest, expected_sha256_hex, cancel, on_progress)
        .await
        .map_err(UpdaterError::from)
}
```

然后把原文件底部的 `#[cfg(test)] mod tests { ... }` **原样追加回该文件末尾**(测试通过薄包装继续验证同样行为,回归保护)。

- [ ] **Step 3: main.rs 声明模块**

在 `src-tauri/src/main.rs:9` 的 `mod disk_cleanup;` 之后插入一行:

```rust
mod download_verify;
```

- [ ] **Step 4: 运行更新器下载回归测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::download`
Expected: PASS(原 5 个测试:sha256_hex、verify_bytes、成功、取消清理、校验失败清理)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/download_verify.rs src-tauri/src/updater/download.rs src-tauri/src/main.rs
git commit -m "refactor(updater): 抽取通用校验下载器 download_verify 供 webview2 自举复用"
```

---

### Task 3: 模块骨架 + `detect.rs` 注册表检测

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/mod.rs`(骨架)
- Create: `src-tauri/src/webview2_bootstrap/detect.rs`
- Modify: `src-tauri/src/main.rs`(`mod updater;` 之后插入 `mod webview2_bootstrap;`)

**Interfaces:**
- Produces:
  - `detect::version_indicates_present(pv: Option<&str>) -> bool`(纯函数)
  - `detect::detect_webview2_runtime() -> Option<String>`(Windows 读注册表;非 Windows 恒 `None`)

- [ ] **Step 1: 写失败测试(先建骨架让其可编译)**

`src-tauri/src/webview2_bootstrap/mod.rs`:

```rust
//! Pre-Tauri WebView2 Runtime bootstrap. Runs before `tauri::Builder` and
//! before the single-instance guard; must not touch AppHandle/plugins.
//! Spec: docs/superpowers/specs/2026-07-09-webview2-bootstrap-design.md

pub mod detect;
```

`src-tauri/src/main.rs:25` 的 `mod updater;` 之后插入:

```rust
mod webview2_bootstrap;
```

`src-tauri/src/webview2_bootstrap/detect.rs` 先只放测试:

```rust
//! Registry-based WebView2 Runtime detection (Microsoft distribution docs:
//! check the Evergreen Runtime client `pv` value).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_empty_versions_are_absent() {
        assert!(!version_indicates_present(None));
        assert!(!version_indicates_present(Some("")));
        assert!(!version_indicates_present(Some("   ")));
    }

    #[test]
    fn zero_version_is_absent() {
        assert!(!version_indicates_present(Some("0.0.0.0")));
    }

    #[test]
    fn real_version_is_present() {
        assert!(version_indicates_present(Some("109.0.1518.78")));
    }

    #[test]
    fn garbage_version_is_absent() {
        assert!(!version_indicates_present(Some("abc")));
        assert!(!version_indicates_present(Some("1.2.x.4")));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::detect`
Expected: FAIL(`version_indicates_present` 未定义,编译错误)

- [ ] **Step 3: 实现**

在 `detect.rs` 测试模块之前补齐实现:

```rust
/// WebView2 Evergreen Runtime 的客户端 GUID(Microsoft 分发文档固定值)。
const CLIENT_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// `pv` 值语义:非空、可全数字解析且大于 0.0.0.0 才算已安装。
pub fn version_indicates_present(pv: Option<&str>) -> bool {
    let Some(pv) = pv else { return false };
    let trimmed = pv.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut any_nonzero = false;
    for part in trimmed.split('.') {
        match part.parse::<u64>() {
            Ok(value) => any_nonzero |= value != 0,
            Err(_) => return false,
        }
    }
    any_nonzero
}

/// 依次检查 HKLM(WOW6432Node,per-machine)与 HKCU(per-user)。
/// 任一 `pv` 通过判定即返回该版本号;全部缺失返回 None。
#[cfg(target_os = "windows")]
pub fn detect_webview2_runtime() -> Option<String> {
    let hklm_subkey =
        format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT_GUID}");
    let hkcu_subkey = format!(r"Software\Microsoft\EdgeUpdate\Clients\{CLIENT_GUID}");
    [
        read_pv(windows::Win32::System::Registry::HKEY_LOCAL_MACHINE, &hklm_subkey),
        read_pv(windows::Win32::System::Registry::HKEY_CURRENT_USER, &hkcu_subkey),
    ]
    .into_iter()
    .flatten()
    .find(|pv| version_indicates_present(Some(pv)))
}

#[cfg(not(target_os = "windows"))]
pub fn detect_webview2_runtime() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn read_pv(
    root: windows::Win32::System::Registry::HKEY,
    subkey: &str,
) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let value_w: Vec<u16> = "pv".encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = [0u16; 64];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR(value_w.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if result != ERROR_SUCCESS {
        return None;
    }
    // size 是含 NUL 的字节数;去掉 NUL 后转 UTF-16。
    let chars = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..chars.min(buffer.len())]))
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::detect`
Expected: PASS(4 个测试)

- [ ] **Step 5: 格式化新文件并提交**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/detect.rs
git add src-tauri/src/webview2_bootstrap/ src-tauri/src/main.rs
git commit -m "feat(webview2): 注册表 pv 检测与版本判定"
```

---

### Task 4: `sha256_file.rs` 解析 `.sha256`

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/sha256_file.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod sha256_file;`)

**Interfaces:**
- Produces: `pub fn parse_sha256_file(content: &str) -> Result<String, String>`(返回小写 64 位 hex)。

- [ ] **Step 1: 写失败测试**

```rust
//! Parse `.sha256` sidecar files: bare `<64hex>` or `<64hex>  <filename>`.

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    #[test]
    fn parses_bare_hash_with_whitespace() {
        assert_eq!(parse_sha256_file(&format!("{HASH}\r\n")).unwrap(), HASH);
    }

    #[test]
    fn parses_hash_with_filename() {
        let content = format!("{HASH}  MicrosoftEdgeWebView2RuntimeInstallerX64.exe\n");
        assert_eq!(parse_sha256_file(&content).unwrap(), HASH);
    }

    #[test]
    fn lowercases_uppercase_hash() {
        assert_eq!(
            parse_sha256_file(&HASH.to_ascii_uppercase()).unwrap(),
            HASH
        );
    }

    #[test]
    fn rejects_wrong_length_and_non_hex() {
        assert!(parse_sha256_file(&HASH[..63]).is_err());
        assert!(parse_sha256_file(&format!("g{}", &HASH[..63])).is_err());
        assert!(parse_sha256_file("").is_err());
        assert!(parse_sha256_file("   \n").is_err());
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::sha256_file`
Expected: FAIL(`parse_sha256_file` 未定义)

- [ ] **Step 3: 实现**

```rust
/// 取首个空白分隔 token(容忍 UTF-8 BOM),要求恰为 64 位 hex,返回小写。
pub fn parse_sha256_file(content: &str) -> Result<String, String> {
    let token = content
        .trim_start_matches('\u{feff}')
        .split_whitespace()
        .next()
        .ok_or_else(|| "empty .sha256 file".to_string())?;
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid sha256 content: {token:.80}"));
    }
    Ok(token.to_ascii_lowercase())
}
```

`mod.rs` 追加 `pub mod sha256_file;`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::sha256_file`
Expected: PASS(4 个测试)

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/sha256_file.rs
git add src-tauri/src/webview2_bootstrap/
git commit -m "feat(webview2): .sha256 边车文件解析"
```

---

### Task 5: `server.rs` 资产 URL 拼接

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/server.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod server;`)

**Interfaces:**
- Produces:
  - `pub const INSTALLER_FILENAME: &str = "MicrosoftEdgeWebView2RuntimeInstallerX64.exe";`
  - `pub fn installer_url(base: &str) -> String`
  - `pub fn sha256_url(base: &str) -> String`

- [ ] **Step 1: 写失败测试**

```rust
//! Build WebView2 asset URLs under `${update_server_url}/webview2/`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_without_trailing_slash() {
        assert_eq!(
            installer_url("http://192.115.1.3:8080"),
            "http://192.115.1.3:8080/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
        );
    }

    #[test]
    fn tolerates_trailing_slash() {
        assert_eq!(
            installer_url("http://192.115.1.3:8080/"),
            "http://192.115.1.3:8080/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
        );
    }

    #[test]
    fn sha256_url_appends_suffix() {
        assert_eq!(
            sha256_url("http://192.115.1.3:8080"),
            "http://192.115.1.3:8080/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256"
        );
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::server`
Expected: FAIL(编译错误)

- [ ] **Step 3: 实现**

```rust
pub const INSTALLER_FILENAME: &str = "MicrosoftEdgeWebView2RuntimeInstallerX64.exe";

pub fn installer_url(base: &str) -> String {
    format!("{}/webview2/{INSTALLER_FILENAME}", base.trim_end_matches('/'))
}

pub fn sha256_url(base: &str) -> String {
    format!("{}.sha256", installer_url(base))
}
```

`mod.rs` 追加 `pub mod server;`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::server`
Expected: PASS(3 个测试)

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/server.rs
git add src-tauri/src/webview2_bootstrap/
git commit -m "feat(webview2): webview2 资产 URL 构造"
```

---

### Task 6: `startup_config.rs` 无 AppHandle 解析更新服务器 URL

**Files:**
- Modify: `src-tauri/src/config.rs`(三个函数改 `pub(crate)`)
- Create: `src-tauri/src/webview2_bootstrap/startup_config.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod startup_config;`)

**Interfaces:**
- Consumes: `crate::config::{default_update_server_url, normalize_update_server_url, validate_update_server_url}`(本任务改为 `pub(crate)`)、`crate::default_app_data_dir()`(main.rs 已有,crate 根私有项对子模块可见)。
- Produces:
  - `pub fn resolve_update_server_url() -> Result<String, String>`(Err 为面向用户的原因)
  - 内部 `fn resolve_from_root(root: &Path) -> Result<String, String>`(测试注入根目录)

- [ ] **Step 1: config.rs 可见性调整**

`src-tauri/src/config.rs:226/230/239` 三处 `fn` 前加 `pub(crate) `:

```rust
pub(crate) fn default_update_server_url() -> String {
pub(crate) fn normalize_update_server_url(value: &str) -> String {
pub(crate) fn validate_update_server_url(value: &str) -> Result<(), String> {
```

- [ ] **Step 2: 写失败测试**

`startup_config.rs`:

```rust
//! Resolve the effective `update_server_url` with pure filesystem logic,
//! mirroring `config::get_config_path` (pivot.json custom_data_dir aware)
//! because no Tauri AppHandle exists during bootstrap.

use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn missing_config_falls_back_to_default_url() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_from_root(root.path()).unwrap(),
            "http://192.115.1.3:8080"
        );
    }

    #[test]
    fn reads_url_from_default_config_and_normalizes() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": " http://10.0.0.9:9000/ "}"#,
        );
        assert_eq!(resolve_from_root(root.path()).unwrap(), "http://10.0.0.9:9000");
    }

    #[test]
    fn pivot_custom_data_dir_overrides_default_config() {
        let root = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": "http://default.example"}"#,
        );
        write(
            &custom.path().join("config.json"),
            r#"{"update_server_url": "http://custom.example"}"#,
        );
        let pivot = format!(
            r#"{{"custom_data_dir": {}}}"#,
            serde_json::to_string(custom.path().to_str().unwrap()).unwrap()
        );
        write(&root.path().join("pivot.json"), &pivot);
        assert_eq!(resolve_from_root(root.path()).unwrap(), "http://custom.example");
    }

    #[test]
    fn pivot_to_missing_dir_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("pivot.json"),
            r#"{"custom_data_dir": "C:\\does\\not\\exist\\anywhere"}"#,
        );
        assert_eq!(
            resolve_from_root(root.path()).unwrap(),
            "http://192.115.1.3:8080"
        );
    }

    #[test]
    fn empty_url_is_error() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("config.json"), r#"{"update_server_url": "  "}"#);
        assert!(resolve_from_root(root.path()).is_err());
    }

    #[test]
    fn non_http_url_is_error() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": "ftp://192.115.1.3"}"#,
        );
        assert!(resolve_from_root(root.path()).is_err());
    }
}
```

- [ ] **Step 3: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::startup_config`
Expected: FAIL(`resolve_from_root` 未定义)

- [ ] **Step 4: 实现**

在测试模块之前补齐:

```rust
/// 入口:根目录取 `%APPDATA%\com.filesync.tool`(与 startup_log 同源)。
pub fn resolve_update_server_url() -> Result<String, String> {
    let root = crate::default_app_data_dir()
        .ok_or_else(|| "无法解析 %APPDATA% / cannot resolve %APPDATA%".to_string())?;
    resolve_from_root(&root)
}

fn resolve_from_root(root: &Path) -> Result<String, String> {
    let raw = read_update_server_url(&effective_config_path(root))
        .unwrap_or_else(crate::config::default_update_server_url);
    let normalized = crate::config::normalize_update_server_url(&raw);
    if normalized.is_empty() {
        return Err(
            "更新服务器地址未配置,请联系管理员 / update server URL is not configured".to_string(),
        );
    }
    crate::config::validate_update_server_url(&normalized)?;
    Ok(normalized)
}

/// 与 `config::get_config_path` 语义一致:pivot.custom_data_dir 存在且为目录
/// 时用 `<custom>\config.json`,否则默认根下 `config.json`。
fn effective_config_path(default_root: &Path) -> PathBuf {
    let pivot_path = default_root.join("pivot.json");
    if let Ok(content) = std::fs::read_to_string(&pivot_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(dir) = value.get("custom_data_dir").and_then(|v| v.as_str()) {
                let dir = PathBuf::from(dir);
                if dir.is_dir() {
                    return dir.join("config.json");
                }
            }
        }
    }
    default_root.join("config.json")
}

/// 用 serde_json::Value 宽松读取,配置文件缺失/损坏/字段缺失都返回 None。
fn read_update_server_url(config_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(config_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    value
        .get("update_server_url")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}
```

`mod.rs` 追加 `pub mod startup_config;`。

- [ ] **Step 5: 运行确认通过(含 config 回归)**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::startup_config`
Expected: PASS(6 个测试)
Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app config::`
Expected: PASS(可见性调整无行为变化)

- [ ] **Step 6: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/startup_config.rs
git add src-tauri/src/config.rs src-tauri/src/webview2_bootstrap/
git commit -m "feat(webview2): 启动期无 AppHandle 解析 update_server_url（支持 pivot 自定义目录）"
```

---

### Task 7: `download.rs` 安装包下载编排

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/download.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod download;`)

**Interfaces:**
- Consumes: `crate::download_verify::{download_to_file, DownloadError}`、`super::{server, sha256_file}`。
- Produces:
  - `pub enum InstallerDownloadError { Cancelled, Failed(String) }`
  - `pub fn default_download_dir() -> PathBuf`(`%TEMP%\file-sync-tool-webview2`)
  - `pub fn download_installer_blocking<F>(base_url: &str, dir: &Path, cancel: watch::Receiver<bool>, on_progress: F) -> Result<PathBuf, InstallerDownloadError>`(自建 current-thread tokio runtime)

- [ ] **Step 1: 写失败测试**

```rust
//! Orchestrate the installer download: fetch `.sha256`, stream the installer
//! to `<dir>\<name>.part` with verification, then rename to the final path.

use crate::download_verify::{self, DownloadError};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

use super::{server, sha256_file};

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const EXE_PATH: &str = "/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe";
    const SHA_PATH: &str = "/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256";

    async fn mock_server(payload: &[u8], sha_body: String) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(SHA_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string(sha_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(EXE_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn success_finalizes_installer_and_removes_part() {
        let payload = vec![7u8; 20_000];
        let sha = download_verify::sha256_hex(&payload);
        let server = mock_server(&payload, format!("{sha}  installer.exe")).await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;

        let result =
            download_installer(&server.uri(), dir.path(), cancel, |_, _| {}).await;

        let final_path = result.expect("download should succeed");
        assert_eq!(std::fs::read(&final_path).unwrap(), payload);
        assert!(!dir
            .path()
            .join(format!("{}.part", server::INSTALLER_FILENAME))
            .exists());
    }

    #[tokio::test]
    async fn hash_mismatch_deletes_files_and_fails() {
        let payload = vec![7u8; 1_000];
        let wrong = download_verify::sha256_hex(b"something else");
        let server = mock_server(&payload, wrong).await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;

        let result =
            download_installer(&server.uri(), dir.path(), cancel, |_, _| {}).await;

        assert!(matches!(result, Err(InstallerDownloadError::Failed(_))));
        assert!(!dir.path().join(server::INSTALLER_FILENAME).exists());
        assert!(!dir
            .path()
            .join(format!("{}.part", server::INSTALLER_FILENAME))
            .exists());
    }

    #[tokio::test]
    async fn missing_sha256_fails_before_installer_download() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;

        let result =
            download_installer(&server.uri(), dir.path(), cancel, |_, _| {}).await;

        assert!(matches!(result, Err(InstallerDownloadError::Failed(_))));
    }

    #[test]
    fn blocking_wrapper_runs_without_ambient_runtime() {
        // 自建 runtime:在无 tokio 环境的普通线程里可直接调用。
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;
        let result = download_installer_blocking(
            "http://127.0.0.1:1", // 无服务,应快速失败而非 panic
            dir.path(),
            cancel,
            |_, _| {},
        );
        assert!(matches!(result, Err(InstallerDownloadError::Failed(_))));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::download`
Expected: FAIL(`download_installer` / `InstallerDownloadError` 未定义)

- [ ] **Step 3: 实现**

```rust
#[derive(Debug)]
pub enum InstallerDownloadError {
    Cancelled,
    Failed(String),
}

pub fn default_download_dir() -> PathBuf {
    std::env::temp_dir().join("file-sync-tool-webview2")
}

/// 自举阶段没有 Tauri/tokio runtime,自建 current-thread runtime 阻塞执行。
pub fn download_installer_blocking<F>(
    base_url: &str,
    dir: &Path,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<PathBuf, InstallerDownloadError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| InstallerDownloadError::Failed(format!("tokio runtime: {e}")))?;
    runtime.block_on(download_installer(base_url, dir, cancel, on_progress))
}

async fn download_installer<F>(
    base_url: &str,
    dir: &Path,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<PathBuf, InstallerDownloadError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let sha_text = fetch_text(&server::sha256_url(base_url)).await?;
    let expected = sha256_file::parse_sha256_file(&sha_text)
        .map_err(InstallerDownloadError::Failed)?;

    let final_path = dir.join(server::INSTALLER_FILENAME);
    let part_path = dir.join(format!("{}.part", server::INSTALLER_FILENAME));
    let _ = std::fs::remove_file(&final_path);

    download_verify::download_to_file(
        &server::installer_url(base_url),
        &part_path,
        &expected,
        cancel,
        on_progress,
    )
    .await
    .map_err(|error| match error {
        DownloadError::Cancelled => InstallerDownloadError::Cancelled,
        other => InstallerDownloadError::Failed(other.to_string()),
    })?;

    std::fs::rename(&part_path, &final_path).map_err(|e| {
        let _ = std::fs::remove_file(&part_path);
        InstallerDownloadError::Failed(format!("rename installer: {e}"))
    })?;
    Ok(final_path)
}

/// 小文件 GET(.sha256):30 秒超时,与更新器一致禁用系统代理。
async fn fetch_text(url: &str) -> Result<String, InstallerDownloadError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .build()
        .map_err(|e| InstallerDownloadError::Failed(format!("http client: {e}")))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| InstallerDownloadError::Failed(format!("network: {e}")))?;
    if !response.status().is_success() {
        return Err(InstallerDownloadError::Failed(format!(
            "HTTP {} for {url}",
            response.status().as_u16()
        )));
    }
    response
        .text()
        .await
        .map_err(|e| InstallerDownloadError::Failed(format!("read body: {e}")))
}
```

`mod.rs` 追加 `pub mod download;`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::download`
Expected: PASS(4 个测试;取消清理路径已由 `updater::download` / download_verify 的回归测试覆盖)

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/download.rs
git add src-tauri/src/webview2_bootstrap/
git commit -m "feat(webview2): 安装包下载编排（.part 校验改名 + 自建 tokio runtime）"
```

---

### Task 8: `install.rs` 静默安装与装后轮询

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/install.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod install;`)

**Interfaces:**
- Consumes: `super::detect::detect_webview2_runtime`。
- Produces:
  - `pub const INSTALL_ARGS: [&str; 2] = ["/silent", "/install"];`
  - `pub fn install_command(installer: &Path) -> Command`
  - `pub fn run_silent_install(installer: &Path) -> Result<(), String>`
  - `pub fn wait_for_runtime(timeout: Duration) -> Option<String>`(2 秒间隔轮询)

- [ ] **Step 1: 写失败测试**

```rust
//! Spawn the Evergreen Standalone Installer silently and poll detection.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn install_command_uses_exact_silent_args() {
        let cmd = install_command(Path::new(r"C:\tmp\installer.exe"));
        assert_eq!(cmd.get_program(), OsStr::new(r"C:\tmp\installer.exe"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, [OsStr::new("/silent"), OsStr::new("/install")]);
    }

    #[test]
    fn wait_returns_immediately_when_probe_succeeds() {
        let hit = wait_with(
            || Some("109.0.1518.78".to_string()),
            Duration::from_secs(60),
            Duration::from_millis(1),
        );
        assert_eq!(hit.as_deref(), Some("109.0.1518.78"));
    }

    #[test]
    fn wait_retries_until_probe_succeeds() {
        let calls = std::cell::Cell::new(0);
        let hit = wait_with(
            || {
                calls.set(calls.get() + 1);
                (calls.get() >= 3).then(|| "1.0.0.1".to_string())
            },
            Duration::from_secs(5),
            Duration::from_millis(1),
        );
        assert_eq!(hit.as_deref(), Some("1.0.0.1"));
        assert!(calls.get() >= 3);
    }

    #[test]
    fn wait_times_out_to_none() {
        let hit = wait_with(|| None, Duration::from_millis(10), Duration::from_millis(2));
        assert!(hit.is_none());
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::install`
Expected: FAIL(编译错误)

- [ ] **Step 3: 实现**

```rust
pub const INSTALL_ARGS: [&str; 2] = ["/silent", "/install"];

/// 直接 spawn 安装器本体(不经 cmd.exe),参数逐字 `/silent /install`。
pub fn install_command(installer: &Path) -> Command {
    let mut cmd = Command::new(installer);
    cmd.args(INSTALL_ARGS);
    cmd
}

pub fn run_silent_install(installer: &Path) -> Result<(), String> {
    let status = install_command(installer)
        .status()
        .map_err(|e| format!("无法启动安装程序 / failed to spawn installer: {e}"))?;
    if status.success() {
        return Ok(());
    }
    let code = status.code().unwrap_or(-1);
    // 0x80070005 = E_ACCESSDENIED:非提权静默安装被拒时给出定向提示。
    let hint = if code as u32 == 0x8007_0005 {
        "（可能需要管理员权限,请尝试以管理员身份运行 / may require administrator rights）"
    } else {
        ""
    };
    Err(format!(
        "安装程序退出码 / installer exit code {code}{hint}"
    ))
}

/// 安装器零退出码后,注册表写入可能有延迟:按间隔轮询直到超时。
pub fn wait_for_runtime(timeout: Duration) -> Option<String> {
    wait_with(
        super::detect::detect_webview2_runtime,
        timeout,
        Duration::from_secs(2),
    )
}

fn wait_with(
    probe: impl Fn() -> Option<String>,
    timeout: Duration,
    interval: Duration,
) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(version) = probe() {
            return Some(version);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(interval);
    }
}
```

`mod.rs` 追加 `pub mod install;`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::install`
Expected: PASS(4 个测试)

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/install.rs
git add src-tauri/src/webview2_bootstrap/
git commit -m "feat(webview2): 静默安装与装后 60 秒检测轮询"
```

---

### Task 9: `restart.rs` 防环自重启

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/restart.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod restart;`)

**Interfaces:**
- Produces:
  - `pub const RESTARTED_ENV: &str = "FST_WEBVIEW2_BOOTSTRAP_RESTARTED";`
  - `pub fn restart_command(exe: &Path, args: &[String]) -> Command`
  - `pub fn restart_and_exit() -> !`

- [ ] **Step 1: 写失败测试**

```rust
//! Relaunch the current exe with original args plus the loop-prevention env.

use std::path::Path;
use std::process::Command;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn preserves_original_args() {
        let cmd = restart_command(
            Path::new(r"C:\app\file-sync-tool.exe"),
            &["--minimized".to_string(), "--from-autostart".to_string()],
        );
        assert_eq!(cmd.get_program(), OsStr::new(r"C:\app\file-sync-tool.exe"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, [OsStr::new("--minimized"), OsStr::new("--from-autostart")]);
    }

    #[test]
    fn sets_restarted_env_flag() {
        let cmd = restart_command(Path::new("app.exe"), &[]);
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.contains(&(OsStr::new(RESTARTED_ENV), Some(OsStr::new("1")))));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::restart`
Expected: FAIL(编译错误)

- [ ] **Step 3: 实现**

```rust
pub const RESTARTED_ENV: &str = "FST_WEBVIEW2_BOOTSTRAP_RESTARTED";

pub fn restart_command(exe: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    cmd.env(RESTARTED_ENV, "1");
    cmd
}

/// 仅在装后检测成功后调用。spawn 新进程后立即退出当前进程;此时本进程
/// 尚未创建单实例守卫互斥体(自举先于守卫),子进程不会误判双开。
pub fn restart_and_exit() -> ! {
    let exe = std::env::current_exe().unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match restart_command(&exe, &args).spawn() {
        Ok(_) => crate::startup_log("info", "webview2 bootstrap: 安装完成,已拉起新实例"),
        Err(error) => crate::startup_log(
            "error",
            &format!("webview2 bootstrap: 重启失败,请手动启动程序: {error}"),
        ),
    }
    std::process::exit(0);
}
```

`mod.rs` 追加 `pub mod restart;`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::restart`
Expected: PASS(2 个测试)

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/restart.rs
git add src-tauri/src/webview2_bootstrap/
git commit -m "feat(webview2): 携带原参数与防环环境变量的自重启"
```

---

### Task 10: `native_ui.rs` 原生对话框与进度窗

**Files:**
- Create: `src-tauri/src/webview2_bootstrap/native_ui.rs`
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(追加 `pub mod native_ui;`)
- Modify: `src-tauri/Cargo.toml`(windows features 追加)

**Interfaces:**
- Produces:
  - `pub enum Phase { Preparing, Downloading, Verifying, Installing, Restarting }`
  - `pub struct ProgressState`(原子字段:phase/downloaded/total/cancelled/done)+ 方法 `set_phase/phase/set_progress/request_cancel/is_cancelled/mark_done/is_done`
  - `pub fn format_downloading_text(downloaded: u64, total: Option<u64>) -> String`(纯函数,可测)
  - `pub fn confirm_install() -> bool`、`pub fn show_error(message: &str)`、`pub fn show_info(message: &str)`
  - `pub fn try_create_progress_window(state: Arc<ProgressState>) -> Option<()>`(失败 → 调用方走 MessageBox 降级)
  - `pub fn run_message_loop()`(窗口销毁时返回)
- 注:windows crate 以 0.58 为准;个别函数返回值的 `Result` 包装如有编译差异,按编译器提示微调,不改变结构与行为。

- [ ] **Step 1: Cargo.toml 追加 features**

在 `src-tauri/Cargo.toml:83` 的 windows features 数组中(按现有顺序风格)追加三行:

```toml
    "Win32_UI_Controls",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_System_LibraryLoader",
```

- [ ] **Step 2: 写失败测试(纯函数部分)**

`native_ui.rs` 底部:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_text_shows_percent_when_total_known() {
        let text = format_downloading_text(50 * 1024 * 1024, Some(100 * 1024 * 1024));
        assert!(text.contains("50%"), "unexpected text: {text}");
        assert!(text.contains("50.0 MB"), "unexpected text: {text}");
        assert!(text.contains("100.0 MB"), "unexpected text: {text}");
    }

    #[test]
    fn download_text_degrades_without_total() {
        let text = format_downloading_text(3 * 1024 * 1024, None);
        assert!(text.contains("3.0 MB"), "unexpected text: {text}");
        assert!(!text.contains('%'), "unexpected text: {text}");
    }

    #[test]
    fn progress_state_round_trips_phase() {
        let state = ProgressState::default();
        assert_eq!(state.phase(), Phase::Preparing);
        state.set_phase(Phase::Installing);
        assert_eq!(state.phase(), Phase::Installing);
    }
}
```

- [ ] **Step 3: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::native_ui`
Expected: FAIL(编译错误)

- [ ] **Step 4: 实现共享状态与文本(平台无关部分)**

```rust
//! Windows-native bootstrap UI: confirmation/error dialogs and a Win32
//! progress window with MessageBox fallback. WebView2/Tauri are unavailable
//! here, so everything is raw Win32; texts are bilingual (no vue-i18n yet).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

pub const DIALOG_TITLE: &str = "File Sync Tool";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Preparing,
    Downloading,
    Verifying,
    Installing,
    Restarting,
}

#[derive(Default)]
pub struct ProgressState {
    phase: AtomicU8,
    downloaded: AtomicU64,
    total: AtomicU64, // 0 = unknown
    cancelled: AtomicBool,
    done: AtomicBool,
}

impl ProgressState {
    pub fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::SeqCst);
    }
    pub fn phase(&self) -> Phase {
        match self.phase.load(Ordering::SeqCst) {
            0 => Phase::Preparing,
            1 => Phase::Downloading,
            2 => Phase::Verifying,
            3 => Phase::Installing,
            _ => Phase::Restarting,
        }
    }
    pub fn set_progress(&self, downloaded: u64, total: Option<u64>) {
        self.downloaded.store(downloaded, Ordering::SeqCst);
        self.total.store(total.unwrap_or(0), Ordering::SeqCst);
    }
    pub fn progress(&self) -> (u64, u64) {
        (
            self.downloaded.load(Ordering::SeqCst),
            self.total.load(Ordering::SeqCst),
        )
    }
    pub fn request_cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub fn mark_done(&self) {
        self.done.store(true, Ordering::SeqCst);
    }
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

pub fn format_downloading_text(downloaded: u64, total: Option<u64>) -> String {
    fn mb(bytes: u64) -> f64 {
        bytes as f64 / (1024.0 * 1024.0)
    }
    match total {
        Some(total) if total > 0 => {
            let percent = (downloaded as f64 / total as f64 * 100.0).min(100.0);
            format!(
                "正在下载 WebView2 运行时 / Downloading WebView2 Runtime… {percent:.0}%（{:.1} MB / {:.1} MB）",
                mb(downloaded),
                mb(total)
            )
        }
        _ => format!(
            "正在下载 WebView2 运行时 / Downloading WebView2 Runtime…（已下载 / downloaded {:.1} MB）",
            mb(downloaded)
        ),
    }
}

pub fn phase_text(state: &ProgressState) -> String {
    let (downloaded, total) = state.progress();
    match state.phase() {
        Phase::Preparing => "正在连接内部更新服务器… / Connecting to the internal update server…".into(),
        Phase::Downloading => format_downloading_text(downloaded, (total > 0).then_some(total)),
        Phase::Verifying => "正在校验安装包完整性… / Verifying installer integrity…".into(),
        Phase::Installing => {
            "正在静默安装 WebView2 运行时,请勿关闭… / Installing WebView2 Runtime silently…".into()
        }
        Phase::Restarting => "安装完成,正在重启 File Sync Tool… / Restarting File Sync Tool…".into(),
    }
}
```

- [ ] **Step 5: 实现 Windows 对话框与进度窗**

同文件继续(所有 Win32 代码 `#[cfg(target_os = "windows")]`,非 Windows 提供哑实现保编译):

```rust
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn confirm_install() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_SYSTEMMODAL, MB_YESNO,
    };
    let text = wide(
        "File Sync Tool 需要 Microsoft Edge WebView2 运行时才能启动,\n\
         本机未检测到该组件。\n\n\
         是否现在从内部更新服务器下载并安装?\n\n\
         File Sync Tool requires Microsoft Edge WebView2 Runtime to start.\n\
         The component was not detected on this computer.\n\
         Install it now from the internal update server?",
    );
    let title = wide(DIALOG_TITLE);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONQUESTION | MB_SYSTEMMODAL,
        ) == IDYES
    }
}

#[cfg(target_os = "windows")]
pub fn show_error(message: &str) {
    message_box(message, windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR);
}

#[cfg(target_os = "windows")]
pub fn show_info(message: &str) {
    message_box(
        message,
        windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
    );
}

#[cfg(target_os = "windows")]
fn message_box(message: &str, icon: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_SYSTEMMODAL};
    let text = wide(message);
    let title = wide(DIALOG_TITLE);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_SYSTEMMODAL | icon,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn confirm_install() -> bool {
    false
}
#[cfg(not(target_os = "windows"))]
pub fn show_error(_message: &str) {}
#[cfg(not(target_os = "windows"))]
pub fn show_info(_message: &str) {}
#[cfg(not(target_os = "windows"))]
pub fn try_create_progress_window(_state: Arc<ProgressState>) -> Option<()> {
    None
}
#[cfg(not(target_os = "windows"))]
pub fn run_message_loop() {}

/// 进度窗:主线程创建并跑消息循环;工作线程只写 `ProgressState`,窗口用
/// 100ms 定时器拉取状态刷新。取消按钮/关闭按钮置 `cancelled`(仅下载阶段)。
#[cfg(target_os = "windows")]
mod progress_window {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT, HBRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Controls::{
        InitCommonControlsEx, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETMARQUEE,
        PBM_SETPOS, PBM_SETRANGE32, PBS_MARQUEE, PROGRESS_CLASSW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const CLASS_NAME: PCWSTR = w!("fst-wv2-bootstrap");
    const ID_CANCEL: i32 = 100;
    const ID_TEXT: i32 = 101;
    const ID_BAR: i32 = 102;
    const TIMER_ID: usize = 1;
    const BAR_RANGE: i32 = 1000;

    pub fn try_create(state: Arc<ProgressState>) -> Option<()> {
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS,
            };
            let _ = InitCommonControlsEx(&icc);
            let hinstance = GetModuleHandleW(None).ok()?;

            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: CLASS_NAME,
                hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
                hbrBackground: HBRUSH(((COLOR_WINDOW.0 + 1) as isize) as *mut core::ffi::c_void),
                ..Default::default()
            };
            // 0 = 注册失败;类名重复注册(理论上单次)也按失败降级。
            if RegisterClassW(&wc) == 0 {
                return None;
            }

            let width = 460;
            let height = 170;
            let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - height) / 2;
            // Arc 所有权移交窗口(WM_NCDESTROY 归还释放)。
            let state_ptr = Arc::into_raw(state) as *const core::ffi::c_void;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS_NAME,
                w!("File Sync Tool"),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                x,
                y,
                width,
                height,
                None,
                None,
                hinstance,
                Some(state_ptr),
            );
            match hwnd {
                Ok(hwnd) if !hwnd.is_invalid() => Some(()),
                _ => {
                    // 创建失败要收回 Arc,避免泄漏。
                    drop(Arc::from_raw(state_ptr as *const ProgressState));
                    None
                }
            }
        }
    }

    pub fn run_message_loop() {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe fn state_of(hwnd: HWND) -> Option<&'static ProgressState> {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const ProgressState;
        (!ptr.is_null()).then(|| &*ptr)
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                let hinstance = create.hInstance;
                let font = GetStockObject(DEFAULT_GUI_FONT);

                let text = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!("…"),
                    WS_CHILD | WS_VISIBLE,
                    16, 16, 412, 36,
                    hwnd,
                    HMENU(ID_TEXT as *mut core::ffi::c_void),
                    hinstance,
                    None,
                );
                let bar = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    PROGRESS_CLASSW,
                    PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE,
                    16, 58, 412, 20,
                    hwnd,
                    HMENU(ID_BAR as *mut core::ffi::c_void),
                    hinstance,
                    None,
                );
                let cancel = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    w!("取消 / Cancel"),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                    308, 92, 120, 28,
                    hwnd,
                    HMENU(ID_CANCEL as *mut core::ffi::c_void),
                    hinstance,
                    None,
                );
                for child in [&text, &bar, &cancel] {
                    if let Ok(child) = child {
                        SendMessageW(*child, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
                    }
                }
                if let Ok(bar) = bar {
                    SendMessageW(bar, PBM_SETRANGE32, WPARAM(0), LPARAM(BAR_RANGE as isize));
                }
                let _ = SetTimer(hwnd, TIMER_ID, 100, None);
                LRESULT(0)
            }
            WM_TIMER => {
                if let Some(state) = state_of(hwnd) {
                    refresh(hwnd, state);
                    if state.is_done() {
                        let _ = KillTimer(hwnd, TIMER_ID);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                if (wparam.0 & 0xffff) as i32 == ID_CANCEL {
                    request_cancel(hwnd);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                // 安装阶段不可取消:忽略关闭;其余阶段等同取消。
                request_cancel(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const ProgressState;
                if !ptr.is_null() {
                    drop(Arc::from_raw(ptr));
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn request_cancel(hwnd: HWND) {
        if let Some(state) = state_of(hwnd) {
            if state.phase() == Phase::Installing || state.phase() == Phase::Restarting {
                return; // 安装/重启阶段禁止取消
            }
            state.request_cancel();
            if let Ok(cancel) = GetDlgItem(hwnd, ID_CANCEL) {
                let _ = EnableWindow(cancel, false);
            }
        }
    }

    unsafe fn refresh(hwnd: HWND, state: &ProgressState) {
        let text = wide(&phase_text(state));
        if let Ok(label) = GetDlgItem(hwnd, ID_TEXT) {
            let _ = SetWindowTextW(label, PCWSTR(text.as_ptr()));
        }
        let Ok(bar) = GetDlgItem(hwnd, ID_BAR) else { return };
        match state.phase() {
            Phase::Downloading | Phase::Verifying | Phase::Preparing => {
                let (downloaded, total) = state.progress();
                if total > 0 {
                    let pos = (downloaded.saturating_mul(BAR_RANGE as u64) / total) as isize;
                    SendMessageW(bar, PBM_SETPOS, WPARAM(pos as usize), LPARAM(0));
                }
            }
            Phase::Installing | Phase::Restarting => {
                // 切换 marquee(不定进度)并禁用取消。
                let style = GetWindowLongPtrW(bar, GWL_STYLE);
                if style & (PBS_MARQUEE.0 as isize) == 0 {
                    SetWindowLongPtrW(bar, GWL_STYLE, style | PBS_MARQUEE.0 as isize);
                    SendMessageW(bar, PBM_SETMARQUEE, WPARAM(1), LPARAM(0));
                    if let Ok(cancel) = GetDlgItem(hwnd, ID_CANCEL) {
                        let _ = EnableWindow(cancel, false);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn try_create_progress_window(state: Arc<ProgressState>) -> Option<()> {
    progress_window::try_create(state)
}

#[cfg(target_os = "windows")]
pub fn run_message_loop() {
    progress_window::run_message_loop()
}
```

`mod.rs` 追加 `pub mod native_ui;`。

- [ ] **Step 6: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::native_ui`
Expected: PASS(3 个测试;Win32 窗口部分编译通过即可,交互走 Task 12 手动 QA)

- [ ] **Step 7: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/native_ui.rs
git add src-tauri/src/webview2_bootstrap/ src-tauri/Cargo.toml
git commit -m "feat(webview2): 原生确认/错误对话框与 Win32 进度窗（MessageBox 降级）"
```

---

### Task 11: `mod.rs` 编排 + `main.rs` 接线

**Files:**
- Modify: `src-tauri/src/webview2_bootstrap/mod.rs`(补编排逻辑)
- Modify: `src-tauri/src/main.rs:3563-3570`(main() 接线)

**Interfaces:**
- Consumes: 本模块全部子模块;`crate::startup_log`(main.rs 私有 fn,子模块可见,签名 `fn startup_log(level: &str, msg: &str)`)。
- Produces:
  - `pub enum BootstrapOutcome { Continue, Exit }`
  - `pub fn ensure_webview2_runtime() -> BootstrapOutcome`
  - 内部 `fn preflight(skip: bool, restarted: bool, detected: Option<&str>) -> PreflightDecision`(纯函数,可测)

- [ ] **Step 1: 写失败测试(preflight 决策)**

`mod.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_env_bypasses_everything() {
        assert_eq!(preflight(true, false, None), PreflightDecision::SkipRequested);
        assert_eq!(
            preflight(true, true, Some("1.0.0.1")),
            PreflightDecision::SkipRequested
        );
    }

    #[test]
    fn present_runtime_continues() {
        assert_eq!(
            preflight(false, false, Some("109.0.1518.78")),
            PreflightDecision::ContinueToApp
        );
        // 重启后检测到运行时:正常继续,不报错。
        assert_eq!(
            preflight(false, true, Some("109.0.1518.78")),
            PreflightDecision::ContinueToApp
        );
    }

    #[test]
    fn restarted_and_still_missing_fails_loop_guard() {
        assert_eq!(preflight(false, true, None), PreflightDecision::FailRestartLoop);
    }

    #[test]
    fn missing_runtime_prompts_install() {
        assert_eq!(preflight(false, false, None), PreflightDecision::PromptInstall);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap::tests`
Expected: FAIL(`preflight` / `PreflightDecision` 未定义)

- [ ] **Step 3: 实现编排**

`mod.rs` 在子模块声明之后补齐:

```rust
use std::sync::Arc;
use std::time::Duration;

use native_ui::{Phase, ProgressState};

pub const SKIP_ENV: &str = "FST_SKIP_WEBVIEW2_BOOTSTRAP";
const BOOTSTRAP_MUTEX_NAME: &str = "com.filesync.tool-wv2-bootstrap";

#[derive(Debug, PartialEq, Eq)]
pub enum BootstrapOutcome {
    Continue,
    Exit,
}

#[derive(Debug, PartialEq, Eq)]
enum PreflightDecision {
    ContinueToApp,
    SkipRequested,
    FailRestartLoop,
    PromptInstall,
}

fn preflight(skip: bool, restarted: bool, detected: Option<&str>) -> PreflightDecision {
    if skip {
        return PreflightDecision::SkipRequested;
    }
    if detected.is_some() {
        return PreflightDecision::ContinueToApp;
    }
    if restarted {
        return PreflightDecision::FailRestartLoop;
    }
    PreflightDecision::PromptInstall
}

pub fn ensure_webview2_runtime() -> BootstrapOutcome {
    #[cfg(not(target_os = "windows"))]
    {
        BootstrapOutcome::Continue
    }
    #[cfg(target_os = "windows")]
    {
        windows_flow()
    }
}

#[cfg(target_os = "windows")]
fn windows_flow() -> BootstrapOutcome {
    let skip = std::env::var(SKIP_ENV).map(|v| v == "1").unwrap_or(false);
    let restarted = std::env::var(restart::RESTARTED_ENV)
        .map(|v| v == "1")
        .unwrap_or(false);
    // 读后即删:防止透传给后代进程(如 updater.bat 重启链)。
    std::env::remove_var(restart::RESTARTED_ENV);

    let detected = detect::detect_webview2_runtime();
    match preflight(skip, restarted, detected.as_deref()) {
        PreflightDecision::ContinueToApp => {
            crate::startup_log(
                "info",
                &format!(
                    "webview2 bootstrap: 检测到运行时 pv={}",
                    detected.as_deref().unwrap_or("?")
                ),
            );
            BootstrapOutcome::Continue
        }
        PreflightDecision::SkipRequested => {
            crate::startup_log("warn", "webview2 bootstrap: FST_SKIP_WEBVIEW2_BOOTSTRAP=1,跳过检测");
            BootstrapOutcome::Continue
        }
        PreflightDecision::FailRestartLoop => {
            crate::startup_log("error", "webview2 bootstrap: 重启后仍未检测到运行时,终止防环");
            native_ui::show_error(
                "WebView2 运行时安装后仍未检测到,程序无法启动。\n请联系管理员检查内部更新服务器或手动安装 WebView2 Runtime。\n\nWebView2 Runtime is still missing after installation.\nPlease contact your administrator.",
            );
            BootstrapOutcome::Exit
        }
        PreflightDecision::PromptInstall => install_flow(),
    }
}

#[cfg(target_os = "windows")]
fn install_flow() -> BootstrapOutcome {
    // 自举专用互斥体:双击两次只允许一个安装流程。句柄随进程退出释放;
    // 成功路径重启的子进程检测到运行时后不会再进入 install_flow,无交接竞态。
    match acquire_bootstrap_mutex() {
        MutexState::Acquired => {}
        MutexState::AlreadyRunning => {
            native_ui::show_info(
                "另一个 File Sync Tool 实例正在安装 WebView2 运行时,本实例将退出。\n\nAnother instance is already installing the WebView2 Runtime.",
            );
            return BootstrapOutcome::Exit;
        }
        MutexState::Unavailable => {
            crate::startup_log("warn", "webview2 bootstrap: 自举互斥体创建失败,继续无守卫安装");
        }
    }

    if !native_ui::confirm_install() {
        crate::startup_log("info", "webview2 bootstrap: 用户拒绝安装,退出");
        return BootstrapOutcome::Exit;
    }

    let base_url = match startup_config::resolve_update_server_url() {
        Ok(url) => url,
        Err(reason) => {
            crate::startup_log("error", &format!("webview2 bootstrap: 更新服务器地址不可用: {reason}"));
            native_ui::show_error(&format!(
                "无法确定内部更新服务器地址:\n{reason}\n\nCannot resolve the internal update server URL."
            ));
            return BootstrapOutcome::Exit;
        }
    };
    crate::startup_log("info", &format!("webview2 bootstrap: 使用更新服务器 {base_url}"));

    let state = Arc::new(ProgressState::default());
    let has_window = native_ui::try_create_progress_window(state.clone()).is_some();
    if !has_window {
        crate::startup_log("warn", "webview2 bootstrap: 进度窗创建失败,降级为 MessageBox 提示");
        native_ui::show_info(
            "即将下载并安装 WebView2 运行时,请稍候。\n完成后程序会自动重启;失败会弹出错误提示。\n\nDownloading and installing the WebView2 Runtime.\nThe app restarts automatically when finished.",
        );
    }

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let result: Arc<std::sync::Mutex<Option<Result<(), WorkerError>>>> =
        Arc::new(std::sync::Mutex::new(None));

    if has_window {
        let worker = {
            let state = state.clone();
            let result = result.clone();
            let base_url = base_url.clone();
            std::thread::spawn(move || {
                let outcome = worker_pipeline(&base_url, &state, cancel_rx, cancel_tx);
                *result.lock().unwrap() = Some(outcome);
                state.mark_done();
            })
        };
        native_ui::run_message_loop();
        let _ = worker.join();
    } else {
        let outcome = worker_pipeline(&base_url, &state, cancel_rx, cancel_tx);
        *result.lock().unwrap() = Some(outcome);
    }

    let outcome = result.lock().unwrap().take();
    let Some(outcome) = outcome else {
        // 工作线程 panic 等罕见情况:结果缺失按失败处理。
        let message = "安装流程异常终止 / bootstrap worker aborted".to_string();
        crate::startup_log("error", &format!("webview2 bootstrap: {message}"));
        native_ui::show_error(&message);
        return BootstrapOutcome::Exit;
    };
    match outcome {
        Ok(()) => {
            state.set_phase(Phase::Restarting);
            restart::restart_and_exit();
        }
        Err(WorkerError::Cancelled) => {
            crate::startup_log("info", "webview2 bootstrap: 用户取消下载,退出");
            BootstrapOutcome::Exit
        }
        Err(WorkerError::Failed(message)) => {
            crate::startup_log("error", &format!("webview2 bootstrap: 失败: {message}"));
            native_ui::show_error(&format!(
                "WebView2 运行时安装失败:\n{message}\n\n请联系管理员检查内部更新服务器。\nWebView2 Runtime installation failed; contact your administrator."
            ));
            BootstrapOutcome::Exit
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
enum WorkerError {
    Cancelled,
    Failed(String),
}

/// 完整管线:下载(带进度/取消桥接)→ 静默安装 → 装后 60 秒轮询。
/// 取消桥接:UI 线程置 `state.cancelled`,进度回调发现后向 watch 发送取消。
#[cfg(target_os = "windows")]
fn worker_pipeline(
    base_url: &str,
    state: &Arc<ProgressState>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
) -> Result<(), WorkerError> {
    state.set_phase(Phase::Downloading);
    let dir = download::default_download_dir();
    let installer = {
        let state = state.clone();
        download::download_installer_blocking(base_url, &dir, cancel_rx, move |downloaded, total| {
            state.set_progress(downloaded, total);
            if state.is_cancelled() {
                let _ = cancel_tx.send(true);
            }
        })
        .map_err(|error| match error {
            download::InstallerDownloadError::Cancelled => WorkerError::Cancelled,
            download::InstallerDownloadError::Failed(message) => WorkerError::Failed(message),
        })?
    };

    state.set_phase(Phase::Verifying); // 哈希在流式下载中已校验;短暂展示该阶段。
    state.set_phase(Phase::Installing);
    crate::startup_log("info", "webview2 bootstrap: 下载校验完成,开始静默安装");
    install::run_silent_install(&installer).map_err(WorkerError::Failed)?;

    match install::wait_for_runtime(Duration::from_secs(60)) {
        Some(version) => {
            crate::startup_log("info", &format!("webview2 bootstrap: 安装成功 pv={version}"));
            Ok(())
        }
        None => Err(WorkerError::Failed(
            "安装程序已结束,但 60 秒内未检测到 WebView2 运行时 / runtime not detected within 60s"
                .to_string(),
        )),
    }
}

#[cfg(target_os = "windows")]
enum MutexState {
    Acquired,
    AlreadyRunning,
    Unavailable,
}

#[cfg(target_os = "windows")]
fn acquire_bootstrap_mutex() -> MutexState {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = BOOTSTRAP_MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(_handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    MutexState::AlreadyRunning
                } else {
                    // 句柄故意不关闭,互斥体存活到本进程退出(与单实例守卫同法)。
                    MutexState::Acquired
                }
            }
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                MutexState::AlreadyRunning
            }
            Err(_) => MutexState::Unavailable,
        }
    }
}
```

- [ ] **Step 4: main() 接线**

`src-tauri/src/main.rs` 的 `fn main()`(现 3563 行起)改为:

```rust
fn main() {
    install_panic_log_hook();

    // WebView2 运行时自举必须在单实例守卫之前:守卫互斥体存活到进程退出,
    // 若自举父进程先占守卫再重启子进程,子进程会误判"已有实例"并静默退出。
    // 自举内部用独立互斥体防止并发安装(见 webview2_bootstrap/mod.rs)。
    if matches!(
        webview2_bootstrap::ensure_webview2_runtime(),
        webview2_bootstrap::BootstrapOutcome::Exit
    ) {
        return;
    }

    // 跨提权等级的单实例判重必须在构建 Tauri 应用之前完成:开机时管理员计划
    // ...(原注释与代码保持不变)
    single_instance_guard::ensure_single_instance(APP_IDENTIFIER);
    ...
}
```

即在 `install_panic_log_hook();` 与原单实例注释之间插入上述 if 块,其余不动。**不要对 main.rs 跑 rustfmt。**

- [ ] **Step 5: 运行全模块测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p app webview2_bootstrap`
Expected: PASS(detect 4 + sha256_file 4 + server 3 + startup_config 6 + download 4 + install 4 + restart 2 + native_ui 3 + mod tests 4 = 34 个)

- [ ] **Step 6: 冒烟验证(开发机已装 WebView2,应无感直通)**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过。
再运行 debug exe 一次确认正常直接进入主窗口、`%APPDATA%\com.filesync.tool\app_data\app.log` 出现 `webview2 bootstrap: 检测到运行时 pv=...` 日志行。

- [ ] **Step 7: Commit**

```bash
rustfmt --edition 2021 src-tauri/src/webview2_bootstrap/mod.rs
git add src-tauri/src/webview2_bootstrap/ src-tauri/src/main.rs
git commit -m "feat(webview2): 启动前自举编排接入 main（先于单实例守卫）"
```

---

### Task 12: 服务器部署文档 + 最终构建验证 + 手动 QA

**Files:**
- Modify: `scripts/release-server/README.md`
- Modify: `scripts/release-server/UPDATE_DEPLOYMENT_GUIDE.md`

**Interfaces:**
- Consumes: Task 5 的服务器 URL 契约。

- [ ] **Step 1: README 增加 webview2 资产说明**

在 `scripts/release-server/README.md` 末尾追加(若已有目录结构章节则并入):

```markdown
## WebView2 Runtime 资产

裸 exe 启动自举会从更新服务器拉取 WebView2 安装器,目录与 manifest.json 无关:

```text
<server-root>/
├── manifest.json                  # 应用更新清单(既有)
├── file-sync-tool-*.exe           # 应用版本产物(既有)
└── webview2/
    ├── MicrosoftEdgeWebView2RuntimeInstallerX64.exe
    └── MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256
```

安装器从 https://developer.microsoft.com/microsoft-edge/webview2/ 下载
"Evergreen Standalone Installer x64",生成边车哈希文件:

```powershell
$f = "MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
$hash = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
"$hash  $f" | Out-File -Encoding ascii "$f.sha256" -NoNewline
```
```

- [ ] **Step 2: UPDATE_DEPLOYMENT_GUIDE 增补同样的部署步骤章节**

在 `scripts/release-server/UPDATE_DEPLOYMENT_GUIDE.md` 追加"WebView2 运行时资产部署"一节,内容同上(目录结构 + PowerShell 哈希命令),并注明:更新 WebView2 安装器版本时只需替换两个文件,无需改 manifest.json。

- [ ] **Step 3: 最终构建验证(项目硬性要求)**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: 构建成功,产物重命名为 `file-sync-tool-1.0.0-YYYYMMDDHHmm.exe`。

- [ ] **Step 4: Commit**

```bash
git add scripts/release-server/README.md scripts/release-server/UPDATE_DEPLOYMENT_GUIDE.md
git commit -m "docs(webview2): 更新服务器 webview2 资产部署说明"
```

- [ ] **Step 5: 手动 Windows QA(需要干净 VM,人工执行并勾选)**

```text
[ ] 干净 Windows VM(无 WebView2):确认提示 → 进度窗 → 静默安装 → 自动重启 → 主窗口打开
[ ] 【风险验证,最优先】非管理员账户静默安装:确认 per-user 安装成功,或按 0x80070005 提示引导管理员处理
[ ] 已装 WebView2 的机器:启动无任何自举 UI,日志有 pv 记录
[ ] 更新服务器不可达:原生错误弹窗后退出
[ ] update_server_url 配置为空:原生错误弹窗后退出
[ ] .sha256 内容错误:安装包被删除,错误弹窗后退出
[ ] 服务器缺少安装器(404):错误弹窗后退出
[ ] 下载中点击取消:.part 清理,进程退出
[ ] 配置了自定义数据目录(pivot.json):自举读取的是自定义目录里的 URL
[ ] 防环:装完重启后若仍检测不到运行时,报错退出而非再次安装
[ ] 双击两次 exe(安装进行中):第二个实例提示"正在安装"后退出
[ ] FST_SKIP_WEBVIEW2_BOOTSTRAP=1:跳过自举直接启动
```

> 其中第 2 项是设计 spec §9"非提权走 per-user 安装"假设的现实验证;如在目标环境不成立,
> 需要回到 spec 增补提权重试(ShellExecuteW runas)方案,再补一个小任务实现。

---

## Self-Review 记录

- **Spec 覆盖**:§2 决策(Task 1/11)、§3 流程含 skip/防环/确认/下载/安装/轮询/重启(Task 11)、§4 模块布局含 download_verify 抽取(Task 2-10)、§5 检测(Task 3)、§6 启动配置含 pivot(Task 6)、§7 原生 UI 三件套+降级(Task 10)、§8 下载校验含 .part(Task 7)、§9 安装重启(Task 8/9)、§10 错误矩阵(Task 11 编排 + 各模块错误路径)、§11 测试(各任务 TDD + Task 12 手动 QA)、§12 超范围项未实现、§13 实现注意事项(接线位置/无 Tauri 依赖/注册表为真相源/原生弹窗)。
- **错误矩阵逐条对照**:已在编排(Task 11)与 QA 清单(Task 12)一一落点;"Progress window creation fails → MessageBox fallback"由 `has_window=false` 分支覆盖。
- **类型一致性**:`BootstrapOutcome::{Continue,Exit}`、`InstallerDownloadError::{Cancelled,Failed}`、`WorkerError::{Cancelled,Failed}`、`ProgressState` 方法名在 Task 10/11 间一致;`server::INSTALLER_FILENAME` 在 Task 5/7 间一致;`restart::RESTARTED_ENV` 在 Task 9/11 间一致。
- **已知留白(显式声明,非占位符)**:windows crate 0.58 个别 API 的 `Result` 包装差异允许编译期微调(Task 10 注);非提权安装行为依赖 Task 12 第 2 项 QA 验证,不成立时按 spec 修订流程增补提权重试任务。
