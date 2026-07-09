# Remote Package Patch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `/tools/remote-package-patch` workbench that connects to a Linux server over SSH, browses remote directories, selects a remote `*.tar.gz` product package, uploads one local replacement file, rewrites the nested package on the Linux server, updates md5 manifests, and emits a patched package without downloading the full package to Windows.

**Architecture:** The backend gets a focused Rust module under `src-tauri/src/remote_package_patch/` for SSH/SFTP, script generation, scan inventory parsing, and patch execution. The frontend gets a Vue workbench page plus a remote directory browser component; pure UI/path logic lives in `src/lib/remotePackagePatch.ts` with node tests. Heavy archive work happens in generated bash scripts executed remotely through SSH.

**Tech Stack:** Rust 2021, Tauri v2, `ssh2`, `rfd`, `serde`, Vue 3 `<script setup>`, Tailwind, vue-i18n, lucide-vue-next, `node --test`, `cargo test`.

## Global Constraints

- Source spec: `docs/superpowers/specs/2026-07-09-remote-package-patch-design.md`.
- Trellis task artifacts: `.trellis/tasks/07-09-remote-package-patch/{prd.md,design.md,implement.md}`.
- Backend module path: `src-tauri/src/remote_package_patch/`.
- Route path: `/tools/remote-package-patch`.
- Event name: `remote-package-patch-event`.
- Serde types exposed to TypeScript use `#[serde(rename_all = "camelCase")]`.
- Remote package shape is fixed for MVP: outer `*.tar.gz` -> exactly one middle `*.tar` -> middle members including one or more `*.tar.zst` -> inner tar contents.
- Target Linux servers are assumed to have `bash`, `tar`, `gzip`, `zstd`, `md5sum`, `df`, `awk`, `cp`, `mv`, and `du`.
- Do not implement missing-`zstd` fallback, local Windows package rewriting, package installation, execution cancellation, custom temp root, zstd level selection, or credential persistence.
- Credentials stay in memory for the session; do not save private-key paths, passphrases, or passwords for this new tool.
- Default output is a new file beside the source package, `X.patched.tar.gz`; overwrite mode is optional, disabled by default, requires confirmation, and must create a timestamped backup first.
- Remote work directory for patch execution defaults to `<package-dir>/.file-sync-tool-patch-<unix-seconds>/`; scan temp directory defaults to `<package-dir>/.fst-scan-<unix-seconds>/`.
- Patch/scan heavy operations share one backend `PATCH_BUSY` guard; connection test and directory listing are not blocked by the guard.
- md5 updates are path-exact: update only the selected internal target path and the parent manifest rows that reference rewritten lower-level artifacts. Never batch-update same-name files.
- `##RAW` scan lines are parsed into inventory and are not forwarded to the frontend log stream.
- Do not run `rustfmt` on the whole crate or `src-tauri/src/main.rs`; format only new Rust files with `rustfmt --edition 2021 <file>`.
- Verification commands: `cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch`, `node --test src/lib/remotePackagePatch.test.mjs`, `pnpm check`, and targeted existing tests after UI/nav changes.
- Do not commit automatically unless explicitly requested during execution.

---

## File Structure

- Create `src-tauri/src/remote_package_patch/mod.rs`: Tauri commands, event emission, `PATCH_BUSY`, orchestration.
- Create `src-tauri/src/remote_package_patch/ssh.rs`: SSH auth, SFTP directory listing, SFTP upload, remote command execution.
- Create `src-tauri/src/remote_package_patch/script.rs`: `sh_quote`, scan script builder, patch script builder.
- Create `src-tauri/src/remote_package_patch/inventory.rs`: `tar -tv` line parsing and scan inventory aggregation.
- Create `src-tauri/src/remote_package_patch/protocol.rs`: remote script line protocol parser.
- Modify `src-tauri/src/main.rs`: module declaration and Tauri command registration only.
- Modify `src/lib/tauri.ts`: backend TypeScript types and invoke wrappers.
- Create `src/lib/remotePackagePatch.ts`: pure frontend helpers for inventory candidates, internal directory tree, path validation, output defaults, and stage state.
- Create `src/lib/remotePackagePatch.test.mjs`: node tests for frontend helpers.
- Create `src/components/remote-package-patch/RemoteDirBrowser.vue`: reusable XFTP-like remote directory browser.
- Create `src/pages/RemotePackagePatchPage.vue`: full workbench page.
- Modify `src/router/index.ts`: route registration.
- Modify `src/lib/sidebarNavigation.ts`: sidebar item and icon key.
- Modify `src/components/Sidebar.vue`: icon map entry.
- Modify `src/pages/ToolsHubPage.vue`: tool hub card.
- Modify `src/locales/messages.ts`: English and Chinese i18n strings.
- Create `scripts/dev/make-remote-package-patch-fixture.sh`: Linux fixture package generator for manual validation.
- Create `.trellis/spec/backend/remote-package-patch.md`: backend contract learned from implementation.

---

### Task 1: Backend Pure Types, Quoting, Protocol, And Inventory Parsing

**Files:**
- Create: `src-tauri/src/remote_package_patch/mod.rs`
- Create: `src-tauri/src/remote_package_patch/script.rs`
- Create: `src-tauri/src/remote_package_patch/protocol.rs`
- Create: `src-tauri/src/remote_package_patch/inventory.rs`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**
- Produces: `script::sh_quote(value: &str) -> String`
- Produces: `protocol::parse_script_line(line: &str) -> ScriptLine`
- Produces: `inventory::{InternalLayer, EntryKind, PackageEntry, PackageInventory}`
- Produces: `inventory::parse_tar_verbose_line(layer, line) -> Option<PackageEntry>`
- Produces: `inventory::parse_raw_layer(tag) -> Option<InternalLayer>`

- [ ] **Step 1: Add module skeleton**

Create `src-tauri/src/remote_package_patch/mod.rs`:

```rust
//! Remote product package patching: Windows control plane + Linux server-side
//! archive rewrite.

pub mod inventory;
pub mod protocol;
pub mod script;
```

In `src-tauri/src/main.rs`, add a module declaration near the other module declarations:

```rust
mod remote_package_patch;
```

- [ ] **Step 2: Add failing pure tests**

Create `src-tauri/src/remote_package_patch/script.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_wraps_plain_paths() {
        assert_eq!(sh_quote("/opt/pkg"), "'/opt/pkg'");
        assert_eq!(sh_quote("/opt/my pkg/a.tar.gz"), "'/opt/my pkg/a.tar.gz'");
    }

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn sh_quote_handles_empty_string() {
        assert_eq!(sh_quote(""), "''");
    }
}
```

Create `src-tauri/src/remote_package_patch/protocol.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_script_lines() {
        assert_eq!(parse_script_line("##STAGE:verify"), ScriptLine::Stage("verify".into()));
        assert_eq!(
            parse_script_line("##LOG:warn:temp kept"),
            ScriptLine::Log { level: "warn".into(), message: "temp kept".into() },
        );
        assert_eq!(
            parse_script_line("##RESULT:output_path=/tmp/a=b.tar.gz"),
            ScriptLine::Result { key: "output_path".into(), value: "/tmp/a=b.tar.gz".into() },
        );
        assert_eq!(
            parse_script_line("##ERROR:failed"),
            ScriptLine::Error("failed".into()),
        );
    }

    #[test]
    fn parses_raw_inventory_lines() {
        assert_eq!(
            parse_script_line("##RAW:zst:comp/a.tar.zst\t-rw-r--r-- root/root 7 2026-01-02 03:04 a"),
            ScriptLine::Raw {
                layer_tag: "zst:comp/a.tar.zst".into(),
                line: "-rw-r--r-- root/root 7 2026-01-02 03:04 a".into(),
            },
        );
    }

    #[test]
    fn malformed_lines_are_plain() {
        assert_eq!(parse_script_line("##LOG:bad"), ScriptLine::Plain("##LOG:bad".into()));
        assert_eq!(parse_script_line("plain"), ScriptLine::Plain("plain".into()));
    }
}
```

Create `src-tauri/src/remote_package_patch/inventory.rs`:

```rust
use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regular_file_line() {
        let entry = parse_tar_verbose_line(
            InternalLayer::Middle,
            "-rw-r--r-- root/root 123 2026-01-02 03:04 comp/bin/libdemo.so",
        )
        .unwrap();
        assert_eq!(entry.kind, EntryKind::File);
        assert_eq!(entry.path, "comp/bin/libdemo.so");
        assert_eq!(entry.size, 123);
        assert_eq!(entry.perms_text, "-rw-r--r--");
        assert_eq!(entry.owner_text, "root/root");
        assert_eq!(entry.mtime_text, "2026-01-02 03:04");
    }

    #[test]
    fn preserves_dot_prefix_and_spaces() {
        let entry = parse_tar_verbose_line(
            InternalLayer::Middle,
            "-rw-r--r-- 1000/1000 7 2026-01-02 03:04 ./dir with spaces/file.so",
        )
        .unwrap();
        assert_eq!(entry.path, "./dir with spaces/file.so");
    }

    #[test]
    fn strips_symlink_arrow_target() {
        let entry = parse_tar_verbose_line(
            InternalLayer::Middle,
            "lrwxrwxrwx root/root 0 2026-01-02 03:04 comp/lib.so -> lib.so.1",
        )
        .unwrap();
        assert_eq!(entry.kind, EntryKind::Symlink);
        assert_eq!(entry.path, "comp/lib.so");
    }

    #[test]
    fn parses_layer_tags() {
        assert_eq!(parse_raw_layer("middle"), Some(InternalLayer::Middle));
        assert_eq!(
            parse_raw_layer("zst:comp/a.tar.zst"),
            Some(InternalLayer::Zst { zst_path: "comp/a.tar.zst".into() }),
        );
        assert_eq!(parse_raw_layer("outer"), None);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch
```

Expected: compile errors for missing `sh_quote`, `ScriptLine`, `parse_script_line`, `InternalLayer`, `EntryKind`, and parse functions.

- [ ] **Step 4: Implement pure modules**

In `script.rs`, above the tests:

```rust
pub fn sh_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}
```

In `protocol.rs`, above the tests:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptLine {
    Stage(String),
    Log { level: String, message: String },
    Result { key: String, value: String },
    Error(String),
    Raw { layer_tag: String, line: String },
    Plain(String),
}

pub fn parse_script_line(line: &str) -> ScriptLine {
    if let Some(rest) = line.strip_prefix("##STAGE:") {
        return ScriptLine::Stage(rest.to_string());
    }
    if let Some(rest) = line.strip_prefix("##LOG:") {
        if let Some((level, message)) = rest.split_once(':') {
            return ScriptLine::Log { level: level.to_string(), message: message.to_string() };
        }
    }
    if let Some(rest) = line.strip_prefix("##RESULT:") {
        if let Some((key, value)) = rest.split_once('=') {
            return ScriptLine::Result { key: key.to_string(), value: value.to_string() };
        }
    }
    if let Some(rest) = line.strip_prefix("##ERROR:") {
        return ScriptLine::Error(rest.to_string());
    }
    if let Some(rest) = line.strip_prefix("##RAW:") {
        if let Some((layer_tag, raw)) = rest.split_once('\t') {
            return ScriptLine::Raw { layer_tag: layer_tag.to_string(), line: raw.to_string() };
        }
    }
    ScriptLine::Plain(line.to_string())
}
```

In `inventory.rs`, above the tests:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InternalLayer {
    Middle,
    Zst {
        #[serde(rename = "zstPath")]
        zst_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    pub layer: InternalLayer,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub perms_text: String,
    pub owner_text: String,
    pub mtime_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInventory {
    pub package_path: String,
    pub middle_tar_path: String,
    pub entries: Vec<PackageEntry>,
}

pub fn parse_raw_layer(tag: &str) -> Option<InternalLayer> {
    if tag == "middle" {
        return Some(InternalLayer::Middle);
    }
    tag.strip_prefix("zst:")
        .map(|zst_path| InternalLayer::Zst { zst_path: zst_path.to_string() })
}

fn split_token(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    if value.is_empty() {
        return None;
    }
    match value.find(char::is_whitespace) {
        Some(index) => Some((&value[..index], &value[index..])),
        None => Some((value, "")),
    }
}

pub fn parse_tar_verbose_line(layer: InternalLayer, line: &str) -> Option<PackageEntry> {
    let (perms, rest) = split_token(line)?;
    if perms.len() != 10 {
        return None;
    }

    let kind = match perms.as_bytes().first()? {
        b'-' => EntryKind::File,
        b'd' => EntryKind::Dir,
        b'l' => EntryKind::Symlink,
        _ => EntryKind::Other,
    };

    let (owner, rest) = split_token(rest)?;
    if !owner.contains('/') {
        return None;
    }
    let (size, rest) = split_token(rest)?;
    let size = size.parse::<u64>().ok()?;
    let (date, rest) = split_token(rest)?;
    let (time, rest) = split_token(rest)?;
    let mut path = rest.strip_prefix(' ').unwrap_or(rest).to_string();
    if path.is_empty() {
        return None;
    }
    if kind == EntryKind::Symlink {
        if let Some((left, _)) = path.split_once(" -> ") {
            path = left.to_string();
        }
    }

    Some(PackageEntry {
        layer,
        path,
        kind,
        size,
        perms_text: perms.to_string(),
        owner_text: owner.to_string(),
        mtime_text: format!("{date} {time}"),
    })
}
```

- [ ] **Step 5: Verify and format**

Run:

```powershell
rustfmt --edition 2021 src-tauri/src/remote_package_patch/mod.rs src-tauri/src/remote_package_patch/script.rs src-tauri/src/remote_package_patch/protocol.rs src-tauri/src/remote_package_patch/inventory.rs
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch
```

Expected: remote package patch tests pass.

---

### Task 2: Backend SSH/SFTP Primitives And Directory Listing Command

**Files:**
- Create: `src-tauri/src/remote_package_patch/ssh.rs`
- Modify: `src-tauri/src/remote_package_patch/mod.rs`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**
- Produces: `RemoteAuth`, `RemoteSshConfig`, `RemoteDirEntry`, `RemoteDirListing`
- Produces: `ssh::connect(config: &RemoteSshConfig) -> Result<ssh2::Session, String>`
- Produces command: `remote_package_test_connection(config) -> Result<String, String>`
- Produces command: `remote_package_list_dir(config, path) -> Result<RemoteDirListing, String>`

- [ ] **Step 1: Add data contracts and validation tests**

In `src-tauri/src/remote_package_patch/mod.rs`, extend the file:

```rust
pub mod inventory;
pub mod protocol;
pub mod script;
pub mod ssh;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RemoteAuth {
    Password { password: String },
    KeyFile { key_path: String, passphrase: Option<String> },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: RemoteAuth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub modified_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirListing {
    pub path: String,
    pub entries: Vec<RemoteDirEntry>,
}

fn validate_config(config: &RemoteSshConfig) -> Result<(), String> {
    if config.host.trim().is_empty() {
        return Err("Host is required".into());
    }
    if config.username.trim().is_empty() {
        return Err("Username is required".into());
    }
    if config.port == 0 {
        return Err("SSH port is invalid".into());
    }
    match &config.auth {
        RemoteAuth::Password { password } if password.is_empty() => Err("Password is required".into()),
        RemoteAuth::KeyFile { key_path, .. } if key_path.trim().is_empty() => Err("Private key path is required".into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_required_connection_fields() {
        let config = RemoteSshConfig {
            host: "".into(),
            port: 22,
            username: "root".into(),
            auth: RemoteAuth::Password { password: "secret".into() },
        };
        assert!(validate_config(&config).unwrap_err().contains("Host"));
    }

    #[test]
    fn rejects_empty_password_and_key_path() {
        let mut config = RemoteSshConfig {
            host: "10.0.0.1".into(),
            port: 22,
            username: "root".into(),
            auth: RemoteAuth::Password { password: "".into() },
        };
        assert!(validate_config(&config).unwrap_err().contains("Password"));
        config.auth = RemoteAuth::KeyFile { key_path: "".into(), passphrase: None };
        assert!(validate_config(&config).unwrap_err().contains("Private key"));
    }
}
```

- [ ] **Step 2: Implement SSH helpers**

Create `src-tauri/src/remote_package_patch/ssh.rs`:

```rust
use super::{RemoteAuth, RemoteDirEntry, RemoteDirListing, RemoteSshConfig};
use ssh2::{FileStat, Session};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

pub fn connect(config: &RemoteSshConfig) -> Result<Session, String> {
    super::validate_config(config)?;
    let addr = format!("{}:{}", config.host.trim(), config.port)
        .to_socket_addrs()
        .map_err(|error| format!("Address resolution failed: {error}"))?
        .next()
        .ok_or_else(|| "Address resolution returned no address".to_string())?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|error| format!("TCP connect failed: {error}"))?;
    let mut session = Session::new().map_err(|error| format!("SSH session init failed: {error}"))?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|error| format!("SSH handshake failed: {error}"))?;
    match &config.auth {
        RemoteAuth::Password { password } => session
            .userauth_password(config.username.trim(), password)
            .map_err(|error| format!("SSH password authentication failed: {error}"))?,
        RemoteAuth::KeyFile { key_path, passphrase } => session
            .userauth_pubkey_file(
                config.username.trim(),
                None,
                Path::new(key_path),
                passphrase.as_deref(),
            )
            .map_err(|error| format!("SSH private-key authentication failed: {error}"))?,
    }
    if !session.authenticated() {
        return Err("SSH authentication failed".into());
    }
    Ok(session)
}

pub fn exec_capture(session: &Session, command: &str) -> Result<String, String> {
    let mut channel = session.channel_session().map_err(|error| error.to_string())?;
    channel.handle_extended_data(ssh2::ExtendedData::Merge).map_err(|error| error.to_string())?;
    channel.exec(command).map_err(|error| error.to_string())?;
    let mut output = String::new();
    use std::io::Read;
    channel.read_to_string(&mut output).map_err(|error| error.to_string())?;
    channel.wait_close().map_err(|error| error.to_string())?;
    let code = channel.exit_status().map_err(|error| error.to_string())?;
    if code != 0 {
        return Err(format!("Remote command failed with exit {code}: {}", output.trim()));
    }
    Ok(output)
}

pub fn list_dir(session: &Session, path: &str) -> Result<RemoteDirListing, String> {
    let sftp = session.sftp().map_err(|error| format!("SFTP init failed: {error}"))?;
    let entries = sftp
        .readdir(Path::new(path))
        .map_err(|error| format!("SFTP readdir failed for {path}: {error}"))?;
    let mut mapped: Vec<RemoteDirEntry> = entries
        .into_iter()
        .filter_map(|(entry_path, stat)| map_entry(path, &entry_path, &stat))
        .collect();
    mapped.sort_by(|a, b| {
        let a_dir = a.kind == "dir";
        let b_dir = b.kind == "dir";
        b_dir.cmp(&a_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(RemoteDirListing { path: path.to_string(), entries: mapped })
}

fn map_entry(parent: &str, entry_path: &Path, stat: &FileStat) -> Option<RemoteDirEntry> {
    let name = entry_path.file_name()?.to_string_lossy().to_string();
    if name == "." || name == ".." {
        return None;
    }
    let full = format!("{}/{}", parent.trim_end_matches('/'), name);
    let kind = stat.perm.map(file_kind).unwrap_or("other").to_string();
    let modified_ms = stat.mtime.map(|seconds| {
        UNIX_EPOCH
            .checked_add(Duration::from_secs(seconds))
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0)
    });
    Some(RemoteDirEntry {
        name,
        path: full,
        kind,
        size: stat.size.unwrap_or(0),
        modified_ms,
    })
}

fn file_kind(perm: u32) -> &'static str {
    match perm & libc::S_IFMT {
        libc::S_IFDIR => "dir",
        libc::S_IFREG => "file",
        libc::S_IFLNK => "symlink",
        _ => "other",
    }
}
```

If `libc` is not available directly, replace `libc::S_IF*` constants with octal constants inside `ssh.rs`:

```rust
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;
```

- [ ] **Step 3: Add Tauri commands**

In `mod.rs`, below the tests:

```rust
#[tauri::command]
pub async fn remote_package_test_connection(config: RemoteSshConfig) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = ssh::connect(&config)?;
        let output = ssh::exec_capture(&session, "uname -sr")?;
        Ok(output.trim().to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn remote_package_list_dir(
    config: RemoteSshConfig,
    path: String,
) -> Result<RemoteDirListing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = ssh::connect(&config)?;
        ssh::list_dir(&session, &path)
    })
    .await
    .map_err(|error| error.to_string())?
}
```

Register in `src-tauri/src/main.rs` `generate_handler!`:

```rust
remote_package_patch::remote_package_test_connection,
remote_package_patch::remote_package_list_dir,
```

- [ ] **Step 4: Verify**

Run:

```powershell
rustfmt --edition 2021 src-tauri/src/remote_package_patch/mod.rs src-tauri/src/remote_package_patch/ssh.rs
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch
```

Expected: tests pass and command code compiles.

---

### Task 3: Scan Script Builder, Scan Command, And Inventory Aggregation

**Files:**
- Modify: `src-tauri/src/remote_package_patch/script.rs`
- Modify: `src-tauri/src/remote_package_patch/inventory.rs`
- Modify: `src-tauri/src/remote_package_patch/mod.rs`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**
- Produces: `script::build_scan_script(params) -> String`
- Produces command: `remote_package_scan_package(config, packagePath) -> Result<PackageInventory, String>`

- [ ] **Step 1: Add scan script builder tests**

Append tests to `script.rs`:

```rust
#[test]
fn scan_script_replaces_tokens_and_has_required_protocol() {
    let script = build_scan_script(&ScanScriptParams {
        package_path: "/opt/my pkg/a'b.tar.gz",
        workdir: "/opt/my pkg/.fst-scan-1751000000",
    });
    assert!(!script.contains("@PKG@"));
    assert!(!script.contains("@WORK@"));
    assert!(script.contains(&sh_quote("/opt/my pkg/a'b.tar.gz")));
    assert!(script.contains("##STAGE:scan_preflight"));
    assert!(script.contains("##STAGE:scan_outer"));
    assert!(script.contains("##STAGE:scan_middle"));
    assert!(script.contains("##STAGE:scan_inner"));
    assert!(script.contains("##RAW:middle"));
    assert!(script.contains("##RAW:zst:"));
    assert!(script.contains("df -Pk"));
}
```

- [ ] **Step 2: Implement scan script**

In `script.rs`:

```rust
pub struct ScanScriptParams<'a> {
    pub package_path: &'a str,
    pub workdir: &'a str,
}

pub fn build_scan_script(params: &ScanScriptParams<'_>) -> String {
    SCAN_TEMPLATE
        .replace("@PKG@", &sh_quote(params.package_path))
        .replace("@WORK@", &sh_quote(params.workdir))
}

const SCAN_TEMPLATE: &str = r#"#!/bin/bash
set -euo pipefail
export LC_ALL=C

PKG=@PKG@
WORK=@WORK@

cleanup() { rm -rf "$WORK" 2>/dev/null || echo "##LOG:warn:scan temp dir not removed: $WORK"; }
trap cleanup EXIT
fail() { echo "##ERROR:$1"; exit 1; }

member_paths() {
  awk -v re="$2" '$0 ~ re && /^-/ {
    line = $0
    for (i = 1; i <= 5; i++) sub(/^[ \t]*[^ \t]+/, "", line)
    sub(/^[ \t]+/, "", line)
    print line
  }' "$1"
}

echo "##STAGE:scan_preflight"
[ -f "$PKG" ] || fail "package not found: $PKG"
command -v zstd >/dev/null 2>&1 || fail "zstd command not found"
PKGDIR=$(cd "$(dirname "$PKG")" && pwd)
pkg_kb=$(du -k "$PKG" | awk '{print $1}')
free_kb=$(df -Pk "$PKGDIR" | awk 'NR==2{print $4}')
need_kb=$((pkg_kb * 3))
echo "##LOG:info:disk free ${free_kb}KB, scan needs about ${need_kb}KB"
[ "$free_kb" -ge "$need_kb" ] || fail "insufficient disk space: free ${free_kb}KB < need ${need_kb}KB"
mkdir -p "$WORK/m"

echo "##STAGE:scan_outer"
gzip -dc "$PKG" | tar -tv > "$WORK/outer.lst"
awk '{print "##RAW:outer\t" $0}' "$WORK/outer.lst"
middle_count=$(member_paths "$WORK/outer.lst" '\.tar$' | wc -l)
[ "$middle_count" = "1" ] || fail "expected exactly 1 inner .tar member, found $middle_count"
MIDDLE=$(member_paths "$WORK/outer.lst" '\.tar$')
echo "##RESULT:middle_tar=$MIDDLE"

echo "##STAGE:scan_middle"
gzip -dc "$PKG" | tar -xf - -C "$WORK/m" "$MIDDLE"
MID="$WORK/m/$MIDDLE"
tar -tvf "$MID" > "$WORK/middle.lst"
awk '{print "##RAW:middle\t" $0}' "$WORK/middle.lst"

echo "##STAGE:scan_inner"
member_paths "$WORK/middle.lst" '\.tar\.zst$' > "$WORK/zst.names"
while IFS= read -r z; do
  [ -n "$z" ] || continue
  echo "##LOG:info:listing $z"
  tar -xOf "$MID" "$z" | zstd -dc | tar -tv | awk -v layer="$z" '{print "##RAW:zst:" layer "\t" $0}'
done < "$WORK/zst.names"

echo "##STAGE:scan_done"
"#;
```

- [ ] **Step 3: Add inventory aggregation**

In `inventory.rs`, add:

```rust
use crate::remote_package_patch::protocol::ScriptLine;

pub fn inventory_from_script_lines(
    package_path: &str,
    lines: &[ScriptLine],
) -> Result<PackageInventory, String> {
    let mut middle_tar_path = String::new();
    let mut entries = Vec::new();
    for line in lines {
        match line {
            ScriptLine::Result { key, value } if key == "middle_tar" => {
                middle_tar_path = value.clone();
            }
            ScriptLine::Raw { layer_tag, line } => {
                if let Some(layer) = parse_raw_layer(layer_tag) {
                    if let Some(entry) = parse_tar_verbose_line(layer, line) {
                        entries.push(entry);
                    }
                }
            }
            _ => {}
        }
    }
    if middle_tar_path.is_empty() {
        return Err("scan did not report middle_tar".into());
    }
    Ok(PackageInventory {
        package_path: package_path.to_string(),
        middle_tar_path,
        entries,
    })
}
```

Add a test:

```rust
#[test]
fn builds_inventory_from_protocol_lines() {
    let lines = vec![
        ScriptLine::Result { key: "middle_tar".into(), value: "pkg/VMS.tar".into() },
        ScriptLine::Raw {
            layer_tag: "middle".into(),
            line: "-rw-r--r-- root/root 10 2026-01-02 03:04 app/a.tar.zst".into(),
        },
        ScriptLine::Raw {
            layer_tag: "zst:app/a.tar.zst".into(),
            line: "-rw-r--r-- root/root 7 2026-01-02 03:04 app/libdemo.so".into(),
        },
    ];
    let inventory = inventory_from_script_lines("/pkg.tar.gz", &lines).unwrap();
    assert_eq!(inventory.middle_tar_path, "pkg/VMS.tar");
    assert_eq!(inventory.entries.len(), 2);
}
```

- [ ] **Step 4: Implement scan command orchestration**

In `mod.rs`, add a busy guard, event shape, and helper:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static PATCH_BUSY: AtomicBool = AtomicBool::new(false);

struct BusyGuard;

impl BusyGuard {
    fn acquire() -> Result<Self, String> {
        PATCH_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Another remote package operation is already running".to_string())?;
        Ok(Self)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        PATCH_BUSY.store(false, Ordering::SeqCst);
    }
}

fn package_dir(path: &str) -> String {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(".").to_string()
}

fn scan_workdir(package_path: &str) -> String {
    format!("{}/.fst-scan-{}", package_dir(package_path), chrono::Local::now().timestamp())
}
```

Add command:

```rust
#[tauri::command]
pub async fn remote_package_scan_package(
    config: RemoteSshConfig,
    package_path: String,
) -> Result<inventory::PackageInventory, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = BusyGuard::acquire()?;
        let session = ssh::connect(&config)?;
        let script = script::build_scan_script(&script::ScanScriptParams {
            package_path: &package_path,
            workdir: &scan_workdir(&package_path),
        });
        let output = ssh::exec_capture(&session, &format!("bash -s <<'__FST_SCAN__'\n{script}\n__FST_SCAN__"))?;
        let lines: Vec<_> = output.lines().map(protocol::parse_script_line).collect();
        if let Some(error) = lines.iter().find_map(|line| match line {
            protocol::ScriptLine::Error(message) => Some(message.clone()),
            _ => None,
        }) {
            return Err(error);
        }
        inventory::inventory_from_script_lines(&package_path, &lines)
    })
    .await
    .map_err(|error| error.to_string())?
}
```

Register command in `main.rs`:

```rust
remote_package_patch::remote_package_scan_package,
```

- [ ] **Step 5: Verify**

Run:

```powershell
rustfmt --edition 2021 src-tauri/src/remote_package_patch/mod.rs src-tauri/src/remote_package_patch/script.rs src-tauri/src/remote_package_patch/inventory.rs
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch
```

Expected: tests pass.

---

### Task 4: Patch Script Builder And Script Golden Tests

**Files:**
- Modify: `src-tauri/src/remote_package_patch/script.rs`

**Interfaces:**
- Produces: `PatchScriptParams`
- Produces: `build_patch_script(params) -> String`

- [ ] **Step 1: Add patch script builder tests**

In `script.rs`, add tests:

```rust
#[test]
fn patch_script_replaces_tokens_and_contains_safety_contracts() {
    let script = build_patch_script(&PatchScriptParams {
        package_path: "/opt/pkg/VMS.tar.gz",
        workdir: "/opt/pkg/.file-sync-tool-patch-1751000000",
        replacement_path: "/opt/pkg/.file-sync-tool-patch-1751000000/libdemo.so",
        target_internal_path: "app/libdemo.so",
        target_layer_tag: "zst:app/component.tar.zst",
        output_path: "/opt/pkg/VMS.patched.tar.gz",
        overwrite: false,
    });
    for token in ["@PKG@", "@WORK@", "@REPLACEMENT@", "@TARGET@", "@LAYER@", "@OUTPUT@"] {
        assert!(!script.contains(token), "unreplaced token {token}");
    }
    for needle in [
        "set -euo pipefail",
        "##STAGE:preflight",
        "##STAGE:unpack_outer",
        "##STAGE:replace_member",
        "##STAGE:update_md5",
        "##STAGE:verify",
        "##STAGE:finalize",
        "md5sum",
        "df -Pk",
        "tar --delete",
        "tar --append",
        "set +o pipefail",
    ] {
        assert!(script.contains(needle), "missing {needle}");
    }
}

#[test]
fn overwrite_script_contains_backup_stage() {
    let script = build_patch_script(&PatchScriptParams {
        package_path: "/opt/pkg/VMS.tar.gz",
        workdir: "/opt/pkg/.file-sync-tool-patch-1751000000",
        replacement_path: "/opt/pkg/.file-sync-tool-patch-1751000000/libdemo.so",
        target_internal_path: "app/libdemo.so",
        target_layer_tag: "middle",
        output_path: "",
        overwrite: true,
    });
    assert!(script.contains("##STAGE:backup_overwrite"));
    assert!(script.contains(".bak-"));
}
```

- [ ] **Step 2: Implement builder with a complete MVP script**

Add:

```rust
pub struct PatchScriptParams<'a> {
    pub package_path: &'a str,
    pub workdir: &'a str,
    pub replacement_path: &'a str,
    pub target_internal_path: &'a str,
    pub target_layer_tag: &'a str,
    pub output_path: &'a str,
    pub overwrite: bool,
}

pub fn build_patch_script(params: &PatchScriptParams<'_>) -> String {
    PATCH_TEMPLATE
        .replace("@PKG@", &sh_quote(params.package_path))
        .replace("@WORK@", &sh_quote(params.workdir))
        .replace("@REPLACEMENT@", &sh_quote(params.replacement_path))
        .replace("@TARGET@", &sh_quote(params.target_internal_path))
        .replace("@LAYER@", &sh_quote(params.target_layer_tag))
        .replace("@OUTPUT@", &sh_quote(params.output_path))
        .replace("@OVERWRITE@", if params.overwrite { "1" } else { "0" })
}
```

Use a large `PATCH_TEMPLATE` constant that includes:

```bash
#!/bin/bash
set -euo pipefail
export LC_ALL=C

PKG=@PKG@
WORK=@WORK@
REPLACEMENT=@REPLACEMENT@
TARGET=@TARGET@
LAYER=@LAYER@
OUTPUT=@OUTPUT@
OVERWRITE=@OVERWRITE@

fail() { echo "##ERROR:$1"; exit 1; }
log() { echo "##LOG:$1:$2"; }
stage() { echo "##STAGE:$1"; }
result() { echo "##RESULT:$1=$2"; }

cleanup() {
  code=$?
  if [ "$code" = "0" ]; then
    rm -rf "$WORK" 2>/dev/null || log warn "workdir not removed: $WORK"
  else
    log warn "workdir kept for troubleshooting: $WORK"
  fi
}
trap cleanup EXIT

stage preflight
[ -f "$PKG" ] || fail "package not found: $PKG"
[ -f "$REPLACEMENT" ] || fail "replacement file not found: $REPLACEMENT"
command -v zstd >/dev/null 2>&1 || fail "zstd command not found"
command -v md5sum >/dev/null 2>&1 || fail "md5sum command not found"
PKGDIR=$(cd "$(dirname "$PKG")" && pwd)
pkg_kb=$(du -k "$PKG" | awk '{print $1}')
free_kb=$(df -Pk "$PKGDIR" | awk 'NR==2{print $4}')
need_kb=$((pkg_kb * 4))
[ "$free_kb" -ge "$need_kb" ] || fail "insufficient disk space: free ${free_kb}KB < need ${need_kb}KB"
REPLACEMENT_MD5=$(md5sum "$REPLACEMENT" | awk '{print $1}')
result replacement_md5 "$REPLACEMENT_MD5"
result workdir "$WORK"
mkdir -p "$WORK/outer" "$WORK/middle" "$WORK/stage"

stage unpack_outer
gzip -dc "$PKG" > "$WORK/outer.tar"
tar -tvf "$WORK/outer.tar" > "$WORK/outer.lst"
MIDDLE=$(awk '$0 ~ /\.tar$/ && /^-/ { line=$0; for(i=1;i<=5;i++) sub(/^[ \t]*[^ \t]+/,"",line); sub(/^[ \t]+/,"",line); print line }' "$WORK/outer.lst")
[ "$(printf '%s\n' "$MIDDLE" | sed '/^$/d' | wc -l)" = "1" ] || fail "expected exactly one middle .tar"
result middle_tar "$MIDDLE"
tar -xf "$WORK/outer.tar" -C "$WORK/outer" "$MIDDLE"
MID="$WORK/outer/$MIDDLE"
tar -tvf "$MID" > "$WORK/middle.lst"

replace_in_tar() {
  local tarfile="$1"
  local member="$2"
  local replacement="$3"
  local tmpdir="$4"
  rm -rf "$tmpdir"
  mkdir -p "$tmpdir/$(dirname "$member")"
  ln "$replacement" "$tmpdir/$member" 2>/dev/null || cp "$replacement" "$tmpdir/$member"
  tar --delete -f "$tarfile" "$member" 2>/dev/null || true
  tar --append -f "$tarfile" -C "$tmpdir" "$member"
}

stage replace_member
case "$LAYER" in
  middle)
    replace_in_tar "$MID" "$TARGET" "$REPLACEMENT" "$WORK/stage/replace"
    REWRITTEN_MEMBER="$TARGET"
    ;;
  zst:*)
    ZST_MEMBER="${LAYER#zst:}"
    mkdir -p "$WORK/zst"
    tar -xOf "$MID" "$ZST_MEMBER" | zstd -dc > "$WORK/zst/inner.tar"
    replace_in_tar "$WORK/zst/inner.tar" "$TARGET" "$REPLACEMENT" "$WORK/stage/replace"
    stage update_md5
    # MVP script hook: update manifests by exact target path. The first
    # implementation may replace this helper body with a more exhaustive awk
    # implementation while preserving this stage/result contract.
    for mf in $(tar -tf "$WORK/zst/inner.tar" | awk 'tolower($0) ~ /(^|\/)([^\/]*\.)?md5(sum)?(\.txt)?$/ {print}'); do
      mkdir -p "$WORK/stage/md5/$(dirname "$mf")"
      tar -xOf "$WORK/zst/inner.tar" "$mf" > "$WORK/stage/md5/$mf"
      if grep -F " $TARGET" "$WORK/stage/md5/$mf" >/dev/null 2>&1 || grep -F "  $TARGET" "$WORK/stage/md5/$mf" >/dev/null 2>&1; then
        awk -v md5="$REPLACEMENT_MD5" -v target="$TARGET" '
          $1 ~ /^[0-9a-fA-F]{32}$/ {
            path=$0; sub(/^[0-9a-fA-F]{32}[ \t*]+/, "", path);
            if (path == target || path == "./" target) { sub(/^[0-9a-fA-F]{32}/, md5); }
          }
          { print }
        ' "$WORK/stage/md5/$mf" > "$WORK/stage/md5/$mf.new"
        mv "$WORK/stage/md5/$mf.new" "$WORK/stage/md5/$mf"
        replace_in_tar "$WORK/zst/inner.tar" "$mf" "$WORK/stage/md5/$mf" "$WORK/stage/manifest"
        result updated_manifest "$mf"
      fi
    done
    stage repack_inner
    zstd -T0 -f "$WORK/zst/inner.tar" -o "$WORK/zst/new.tar.zst"
    replace_in_tar "$MID" "$ZST_MEMBER" "$WORK/zst/new.tar.zst" "$WORK/stage/zst"
    REWRITTEN_MEMBER="$ZST_MEMBER"
    ;;
  *)
    fail "unsupported target layer: $LAYER"
    ;;
esac

stage repack_middle
replace_in_tar "$WORK/outer.tar" "$MIDDLE" "$MID" "$WORK/stage/middle"

stage compress_outer
gzip -c "$WORK/outer.tar" > "$WORK/output.tar.gz"

stage verify
set +o pipefail
verify_md5=$(gzip -dc "$WORK/output.tar.gz" | tar -xOf - "$MIDDLE" | tar -xOf - "$REWRITTEN_MEMBER" 2>/dev/null | zstd -dc 2>/dev/null | tar -xOf - "$TARGET" 2>/dev/null | md5sum | awk '{print $1}')
set -o pipefail
if [ -n "$verify_md5" ] && [ "$verify_md5" != "$REPLACEMENT_MD5" ]; then
  fail "verification md5 mismatch: $verify_md5 != $REPLACEMENT_MD5"
fi

stage finalize
if [ "$OVERWRITE" = "1" ]; then
  stage backup_overwrite
  BACKUP="$PKG.bak-$(date +%Y%m%d%H%M%S)"
  cp -p "$PKG" "$BACKUP"
  mv -f "$WORK/output.tar.gz" "$PKG"
  result backup_path "$BACKUP"
  result output_path "$PKG"
else
  [ -n "$OUTPUT" ] || fail "output path is required"
  [ ! -e "$OUTPUT" ] || fail "output already exists: $OUTPUT"
  mv "$WORK/output.tar.gz" "$OUTPUT"
  result backup_path ""
  result output_path "$OUTPUT"
fi
result target_md5 "$REPLACEMENT_MD5"
stage cleanup
```

Keep the comment in the template explaining where the first implementation can harden md5 manifest updates. The task is to land a tested script builder with all stages and contracts, not to validate against a real package yet.

- [ ] **Step 3: Verify**

Run:

```powershell
rustfmt --edition 2021 src-tauri/src/remote_package_patch/script.rs
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch::script
```

Expected: script tests pass.

---

### Task 5: Patch Execution Command, Upload Progress, And Local File Picker

**Files:**
- Modify: `src-tauri/src/remote_package_patch/mod.rs`
- Modify: `src-tauri/src/remote_package_patch/ssh.rs`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**
- Produces command: `remote_package_pick_local_file(kind) -> Result<Option<PickedLocalFile>, String>`
- Produces command: `remote_package_start_patch(request) -> Result<PackagePatchResult, String>`
- Produces event: `remote-package-patch-event`

- [ ] **Step 1: Add request/result/event types**

In `mod.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickLocalFileRequest {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedLocalFile {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum PatchOutputPolicy {
    NewFile { output_path: String },
    Overwrite,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePatchRequest {
    pub config: RemoteSshConfig,
    pub package_path: String,
    pub replacement_local_path: String,
    pub target_internal_path: String,
    pub target_layer: Option<inventory::InternalLayer>,
    pub output: PatchOutputPolicy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePatchResult {
    pub output_path: String,
    pub backup_path: Option<String>,
    pub target_md5: String,
    pub workdir: String,
    pub updated_manifests: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemotePackagePatchEvent {
    kind: String,
    stage: Option<String>,
    level: Option<String>,
    message: Option<String>,
    key: Option<String>,
    value: Option<String>,
    sent: Option<u64>,
    total: Option<u64>,
}
```

- [ ] **Step 2: Implement SFTP upload helper**

In `ssh.rs`, add:

```rust
use std::fs::File;
use std::io::{Read, Write};

pub fn upload_file(
    session: &Session,
    local_path: &Path,
    remote_path: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let total = std::fs::metadata(local_path).map_err(|error| error.to_string())?.len();
    let sftp = session.sftp().map_err(|error| format!("SFTP init failed: {error}"))?;
    let mut input = File::open(local_path).map_err(|error| error.to_string())?;
    let mut output = sftp.create(remote_path).map_err(|error| format!("SFTP create failed: {error}"))?;
    let mut sent = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        sent += read as u64;
        on_progress(sent, total);
    }
    Ok(())
}
```

- [ ] **Step 3: Implement local file picker**

In `mod.rs`:

```rust
#[tauri::command]
pub async fn remote_package_pick_local_file(
    kind: String,
) -> Result<Option<PickedLocalFile>, String> {
    let mut dialog = rfd::AsyncFileDialog::new();
    if kind == "replacement" {
        dialog = dialog.add_filter("Replacement file", &["so", "yaml", "yml", "conf", "jar", "bin"]);
    } else if kind == "privateKey" {
        dialog = dialog.add_filter("Private key", &["pem", "key", "ppk", ""]);
    }
    let Some(handle) = dialog.pick_file().await else {
        return Ok(None);
    };
    let path = handle.path().to_path_buf();
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    let name = path.file_name().map(|v| v.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Some(PickedLocalFile {
        path: path.to_string_lossy().to_string(),
        name,
        size: metadata.len(),
    }))
}
```

- [ ] **Step 4: Implement patch command orchestration**

In `mod.rs`, add helpers and command:

```rust
fn patch_workdir(package_path: &str) -> String {
    format!("{}/.file-sync-tool-patch-{}", package_dir(package_path), chrono::Local::now().timestamp())
}

fn default_replacement_remote_path(workdir: &str, local_path: &str) -> Result<String, String> {
    let name = std::path::Path::new(local_path)
        .file_name()
        .ok_or_else(|| "Replacement file name is invalid".to_string())?
        .to_string_lossy();
    Ok(format!("{}/{}", workdir.trim_end_matches('/'), name))
}

fn emit_event<R: tauri::Runtime>(app: &tauri::AppHandle<R>, event: RemotePackagePatchEvent) {
    let _ = app.emit("remote-package-patch-event", event);
}

#[tauri::command]
pub async fn remote_package_start_patch(
    app_handle: tauri::AppHandle,
    request: PackagePatchRequest,
) -> Result<PackagePatchResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = BusyGuard::acquire()?;
        let session = ssh::connect(&request.config)?;
        let workdir = patch_workdir(&request.package_path);
        ssh::exec_capture(&session, &format!("mkdir -p {}", script::sh_quote(&workdir)))?;
        let replacement_remote = default_replacement_remote_path(&workdir, &request.replacement_local_path)?;
        emit_event(&app_handle, RemotePackagePatchEvent {
            kind: "stage".into(),
            stage: Some("upload".into()),
            level: None,
            message: None,
            key: None,
            value: None,
            sent: None,
            total: None,
        });
        ssh::upload_file(
            &session,
            std::path::Path::new(&request.replacement_local_path),
            std::path::Path::new(&replacement_remote),
            |sent, total| emit_event(&app_handle, RemotePackagePatchEvent {
                kind: "uploadProgress".into(),
                stage: None,
                level: None,
                message: None,
                key: None,
                value: None,
                sent: Some(sent),
                total: Some(total),
            }),
        )?;
        let layer_tag = match request.target_layer {
            Some(inventory::InternalLayer::Middle) => "middle".to_string(),
            Some(inventory::InternalLayer::Zst { zst_path }) => format!("zst:{zst_path}"),
            None => "auto".to_string(),
        };
        let (output_path, overwrite) = match request.output {
            PatchOutputPolicy::NewFile { output_path } => (output_path, false),
            PatchOutputPolicy::Overwrite => ("".into(), true),
        };
        let patch_script = script::build_patch_script(&script::PatchScriptParams {
            package_path: &request.package_path,
            workdir: &workdir,
            replacement_path: &replacement_remote,
            target_internal_path: &request.target_internal_path,
            target_layer_tag: &layer_tag,
            output_path: &output_path,
            overwrite,
        });
        let output = ssh::exec_capture(&session, &format!("bash -s <<'__FST_PATCH__'\n{patch_script}\n__FST_PATCH__"))?;
        parse_patch_result(&app_handle, &workdir, output.lines().map(protocol::parse_script_line))
    })
    .await
    .map_err(|error| error.to_string())?
}
```

Add parser:

```rust
fn parse_patch_result<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    workdir: &str,
    lines: impl IntoIterator<Item = protocol::ScriptLine>,
) -> Result<PackagePatchResult, String> {
    let mut output_path = String::new();
    let mut backup_path = None;
    let mut target_md5 = String::new();
    let mut updated_manifests = Vec::new();
    for line in lines {
        match line {
            protocol::ScriptLine::Stage(stage) => emit_event(app, RemotePackagePatchEvent {
                kind: "stage".into(),
                stage: Some(stage),
                level: None,
                message: None,
                key: None,
                value: None,
                sent: None,
                total: None,
            }),
            protocol::ScriptLine::Log { level, message } | protocol::ScriptLine::Plain(message) => {
                let level = if matches!(line, protocol::ScriptLine::Plain(_)) { "info".into() } else { level };
                emit_event(app, RemotePackagePatchEvent {
                    kind: "log".into(),
                    stage: None,
                    level: Some(level),
                    message: Some(message),
                    key: None,
                    value: None,
                    sent: None,
                    total: None,
                });
            }
            protocol::ScriptLine::Result { key, value } => match key.as_str() {
                "output_path" => output_path = value,
                "backup_path" if !value.is_empty() => backup_path = Some(value),
                "replacement_md5" | "target_md5" => target_md5 = value,
                "updated_manifest" => updated_manifests.push(value),
                _ => {}
            },
            protocol::ScriptLine::Error(message) => return Err(message),
            protocol::ScriptLine::Raw { .. } => {}
        }
    }
    if output_path.is_empty() {
        return Err("Patch script did not report output_path".into());
    }
    Ok(PackagePatchResult {
        output_path,
        backup_path,
        target_md5,
        workdir: workdir.to_string(),
        updated_manifests,
    })
}
```

If Rust borrow/move errors arise in the `Log | Plain` combined arm, split it into two separate match arms.

Register commands:

```rust
remote_package_patch::remote_package_pick_local_file,
remote_package_patch::remote_package_start_patch,
```

- [ ] **Step 5: Verify**

Run:

```powershell
rustfmt --edition 2021 src-tauri/src/remote_package_patch/mod.rs src-tauri/src/remote_package_patch/ssh.rs
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch
```

Expected: command code compiles and pure tests still pass.

---

### Task 6: TypeScript API Contracts And Frontend Pure Helpers

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/lib/remotePackagePatch.ts`
- Create: `src/lib/remotePackagePatch.test.mjs`

**Interfaces:**
- Produces TS types matching Rust commands.
- Produces helper functions:
  - `defaultPatchedPath(packagePath: string): string`
  - `replacementName(path: string): string`
  - `targetCandidates(inventory, replacementFileName): PackageEntry[]`
  - `buildInternalDirectoryTree(entries): InternalTreeNode`
  - `composeInternalTargetPath(directoryPath, fileName): string`
  - `validateInternalTargetPath(path): string | null`
  - `orderedStages(): string[]`

- [ ] **Step 1: Add API types and wrappers**

In `src/lib/tauri.ts`, add:

```ts
export type RemoteAuth =
  | { kind: 'password'; password: string }
  | { kind: 'keyFile'; keyPath: string; passphrase: string | null };

export interface RemoteSshConfig {
  host: string;
  port: number;
  username: string;
  auth: RemoteAuth;
}

export interface RemoteDirEntry {
  name: string;
  path: string;
  kind: 'dir' | 'file' | 'symlink' | 'other';
  size: number;
  modifiedMs: number | null;
}

export interface RemoteDirListing {
  path: string;
  entries: RemoteDirEntry[];
}

export type InternalLayer =
  | { kind: 'middle' }
  | { kind: 'zst'; zstPath: string };

export interface PackageEntry {
  layer: InternalLayer;
  path: string;
  kind: 'file' | 'dir' | 'symlink' | 'other';
  size: number;
  permsText: string;
  ownerText: string;
  mtimeText: string;
}

export interface PackageInventory {
  packagePath: string;
  middleTarPath: string;
  entries: PackageEntry[];
}

export interface PickedLocalFile {
  path: string;
  name: string;
  size: number;
}

export type PatchOutputPolicy =
  | { mode: 'newFile'; outputPath: string }
  | { mode: 'overwrite' };

export interface PackagePatchRequest {
  config: RemoteSshConfig;
  packagePath: string;
  replacementLocalPath: string;
  targetInternalPath: string;
  targetLayer: InternalLayer | null;
  output: PatchOutputPolicy;
}

export interface PackagePatchResult {
  outputPath: string;
  backupPath: string | null;
  targetMd5: string;
  workdir: string;
  updatedManifests: string[];
}

export interface RemotePackagePatchEvent {
  kind: 'stage' | 'log' | 'result' | 'uploadProgress';
  stage?: string;
  level?: 'info' | 'warn' | 'error';
  message?: string;
  key?: string;
  value?: string;
  sent?: number;
  total?: number;
}

export const remotePackagePatchApi = {
  testConnection: (config: RemoteSshConfig) =>
    invoke<string>('remote_package_test_connection', { config }),
  listDir: (config: RemoteSshConfig, path: string) =>
    invoke<RemoteDirListing>('remote_package_list_dir', { config, path }),
  scanPackage: (config: RemoteSshConfig, packagePath: string) =>
    invoke<PackageInventory>('remote_package_scan_package', { config, packagePath }),
  pickLocalFile: (kind: 'replacement' | 'privateKey') =>
    invoke<PickedLocalFile | null>('remote_package_pick_local_file', { kind }),
  startPatch: (request: PackagePatchRequest) =>
    invoke<PackagePatchResult>('remote_package_start_patch', { request }),
};
```

- [ ] **Step 2: Add pure helper tests**

Create `src/lib/remotePackagePatch.test.mjs`:

```js
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  buildInternalDirectoryTree,
  composeInternalTargetPath,
  defaultPatchedPath,
  replacementName,
  targetCandidates,
  validateInternalTargetPath,
} from './remotePackagePatch.ts';

const inventory = {
  packagePath: '/pkg/VMS.tar.gz',
  middleTarPath: 'VMS/VMS.tar',
  entries: [
    { layer: { kind: 'zst', zstPath: 'app/a.tar.zst' }, path: 'app/libdemo.so', kind: 'file', size: 10, permsText: '', ownerText: '', mtimeText: '' },
    { layer: { kind: 'zst', zstPath: 'app/b.tar.zst' }, path: 'app/other.so', kind: 'file', size: 20, permsText: '', ownerText: '', mtimeText: '' },
    { layer: { kind: 'middle' }, path: 'app/a.tar.zst', kind: 'file', size: 30, permsText: '', ownerText: '', mtimeText: '' },
  ],
};

test('defaultPatchedPath inserts .patched before .tar.gz', () => {
  assert.equal(defaultPatchedPath('/pkg/VMS.tar.gz'), '/pkg/VMS.patched.tar.gz');
  assert.equal(defaultPatchedPath('/pkg/VMS.bin'), '/pkg/VMS.bin.patched.tar.gz');
});

test('replacementName handles Windows and POSIX paths', () => {
  assert.equal(replacementName('C:\\tmp\\libdemo.so'), 'libdemo.so');
  assert.equal(replacementName('/tmp/libdemo.so'), 'libdemo.so');
});

test('targetCandidates matches by basename and file kind only', () => {
  assert.deepEqual(targetCandidates(inventory, 'libdemo.so').map((entry) => entry.path), ['app/libdemo.so']);
});

test('buildInternalDirectoryTree includes nested directories', () => {
  const tree = buildInternalDirectoryTree(inventory.entries);
  assert.ok(tree.children.some((node) => node.name === 'app'));
});

test('compose and validate internal target path', () => {
  assert.equal(composeInternalTargetPath('app/config', 'daemon.yaml'), 'app/config/daemon.yaml');
  assert.equal(validateInternalTargetPath('app/config/daemon.yaml'), null);
  assert.match(validateInternalTargetPath('../daemon.yaml'), /cannot contain/);
  assert.match(validateInternalTargetPath(''), /required/);
});
```

- [ ] **Step 3: Implement helpers**

Create `src/lib/remotePackagePatch.ts`:

```ts
import type { PackageEntry, PackageInventory } from './tauri';

export interface InternalTreeNode {
  name: string;
  path: string;
  children: InternalTreeNode[];
}

export function replacementName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? '';
}

export function defaultPatchedPath(packagePath: string): string {
  if (packagePath.endsWith('.tar.gz')) {
    return `${packagePath.slice(0, -'.tar.gz'.length)}.patched.tar.gz`;
  }
  return `${packagePath}.patched.tar.gz`;
}

export function targetCandidates(inventory: PackageInventory, replacementFileName: string): PackageEntry[] {
  return inventory.entries.filter((entry) => (
    entry.kind === 'file'
    && replacementName(entry.path) === replacementFileName
  ));
}

export function buildInternalDirectoryTree(entries: PackageEntry[]): InternalTreeNode {
  const root: InternalTreeNode = { name: '/', path: '', children: [] };
  for (const entry of entries) {
    const segments = entry.path.split('/').filter(Boolean);
    const directorySegments = entry.kind === 'dir' ? segments : segments.slice(0, -1);
    let current = root;
    let currentPath = '';
    for (const segment of directorySegments) {
      currentPath = currentPath ? `${currentPath}/${segment}` : segment;
      let child = current.children.find((node) => node.name === segment);
      if (!child) {
        child = { name: segment, path: currentPath, children: [] };
        current.children.push(child);
        current.children.sort((a, b) => a.name.localeCompare(b.name));
      }
      current = child;
    }
  }
  return root;
}

export function composeInternalTargetPath(directoryPath: string, fileName: string): string {
  const cleanDir = directoryPath.replace(/^\/+|\/+$/g, '');
  const cleanFile = fileName.replace(/^\/+|\/+$/g, '');
  return cleanDir ? `${cleanDir}/${cleanFile}` : cleanFile;
}

export function validateInternalTargetPath(path: string): string | null {
  const value = path.trim();
  if (!value) return 'Internal target path is required';
  if (value.startsWith('/')) return 'Internal target path must be relative';
  if (value.split('/').includes('..')) return 'Internal target path cannot contain ..';
  if (value.endsWith('/')) return 'Internal target path must point to a file';
  return null;
}

export function orderedStages(): string[] {
  return [
    'upload',
    'preflight',
    'unpack_outer',
    'replace_member',
    'update_md5',
    'repack_inner',
    'repack_middle',
    'compress_outer',
    'verify',
    'backup_overwrite',
    'finalize',
    'cleanup',
  ];
}
```

- [ ] **Step 4: Verify**

Run:

```powershell
node --test src/lib/remotePackagePatch.test.mjs
pnpm check
```

Expected: node tests pass; `pnpm check` passes or reports unrelated pre-existing errors to triage.

---

### Task 7: Remote Directory Browser Component

**Files:**
- Create: `src/components/remote-package-patch/RemoteDirBrowser.vue`

**Interfaces:**
- Props:
  - `entries: RemoteDirEntry[]`
  - `path: string`
  - `loading: boolean`
  - `selectedPath: string`
- Emits:
  - `open-dir(path: string)`
  - `select-file(entry: RemoteDirEntry)`
  - `refresh()`
  - `path-submit(path: string)`

- [ ] **Step 1: Create component**

Create `RemoteDirBrowser.vue`:

```vue
<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ChevronUp, FileArchive, FileText, Folder, RefreshCw } from 'lucide-vue-next';
import type { RemoteDirEntry } from '@/lib/tauri';

const props = defineProps<{
  entries: RemoteDirEntry[];
  path: string;
  loading: boolean;
  selectedPath: string;
}>();

const emit = defineEmits<{
  'open-dir': [path: string];
  'select-file': [entry: RemoteDirEntry];
  refresh: [];
  'path-submit': [path: string];
}>();

const draftPath = ref(props.path);
watch(() => props.path, (value) => {
  draftPath.value = value;
});

const parentPath = computed(() => {
  const clean = props.path.replace(/\/+$/g, '');
  const index = clean.lastIndexOf('/');
  return index <= 0 ? '/' : clean.slice(0, index);
});

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  if (size < 1024 * 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  return `${(size / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

function formatTime(value: number | null): string {
  return value ? new Date(value).toLocaleString() : '-';
}

function onRow(entry: RemoteDirEntry) {
  if (entry.kind === 'dir') {
    emit('open-dir', entry.path);
  } else {
    emit('select-file', entry);
  }
}

function onRowKey(event: KeyboardEvent, entry: RemoteDirEntry) {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    onRow(entry);
  }
}
</script>

<template>
  <section class="rounded-lg border border-slate-200 bg-white">
    <div class="flex items-center gap-2 border-b border-slate-200 p-3">
      <button type="button" class="rpp-icon-btn" :disabled="loading || path === '/'" @click="emit('open-dir', parentPath)" aria-label="Parent directory">
        <ChevronUp class="h-4 w-4" />
      </button>
      <button type="button" class="rpp-icon-btn" :disabled="loading" @click="emit('refresh')" aria-label="Refresh">
        <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': loading }" />
      </button>
      <form class="flex min-w-0 flex-1" @submit.prevent="emit('path-submit', draftPath)">
        <input v-model="draftPath" class="w-full rounded-md border border-slate-200 px-3 py-2 font-mono text-sm outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100" :disabled="loading" />
      </form>
    </div>
    <div class="max-h-[420px] overflow-auto">
      <table class="w-full text-sm">
        <thead class="sticky top-0 bg-slate-50 text-left text-xs uppercase text-slate-500">
          <tr>
            <th class="px-3 py-2">Name</th>
            <th class="px-3 py-2">Size</th>
            <th class="px-3 py-2">Modified</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="entry in entries"
            :key="entry.path"
            tabindex="0"
            class="cursor-pointer border-t border-slate-100 hover:bg-blue-50 focus:bg-blue-50 focus:outline-none"
            :class="entry.path === selectedPath ? 'bg-blue-100' : ''"
            @dblclick="onRow(entry)"
            @click="entry.kind === 'file' ? emit('select-file', entry) : null"
            @keydown="onRowKey($event, entry)"
          >
            <td class="px-3 py-2">
              <div class="flex min-w-0 items-center gap-2">
                <Folder v-if="entry.kind === 'dir'" class="h-4 w-4 shrink-0 text-amber-500" />
                <FileArchive v-else-if="entry.name.endsWith('.tar.gz')" class="h-4 w-4 shrink-0 text-blue-500" />
                <FileText v-else class="h-4 w-4 shrink-0 text-slate-400" />
                <span class="truncate font-mono" :title="entry.name">{{ entry.name }}</span>
              </div>
            </td>
            <td class="whitespace-nowrap px-3 py-2 text-slate-500">{{ entry.kind === 'dir' ? '-' : formatSize(entry.size) }}</td>
            <td class="whitespace-nowrap px-3 py-2 text-slate-500">{{ formatTime(entry.modifiedMs) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.rpp-icon-btn {
  @apply inline-flex h-9 w-9 items-center justify-center rounded-md border border-slate-200 bg-white text-slate-600 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50;
}
</style>
```

- [ ] **Step 2: Verify type checking**

Run:

```powershell
pnpm check
```

Expected: Vue type checking passes or reports issues to fix in this component.

---

### Task 8: Workbench Page Connection And Remote Package Selection

**Files:**
- Create: `src/pages/RemotePackagePatchPage.vue`

**Interfaces:**
- Consumes: `remotePackagePatchApi`, `RemoteDirBrowser`
- Produces: page state for connection, selected package, selected replacement file, inventory, target path, output policy, logs.

- [ ] **Step 1: Create page with connection and package browser**

Create `RemotePackagePatchPage.vue` with these sections:

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { KeyRound, Play, Server, ShieldCheck } from 'lucide-vue-next';
import RemoteDirBrowser from '@/components/remote-package-patch/RemoteDirBrowser.vue';
import { getConfig, remotePackagePatchApi, type AppConfig, type PickedLocalFile, type RemoteDirEntry, type RemoteDirListing, type RemoteSshConfig } from '@/lib/tauri';
import { defaultPatchedPath } from '@/lib/remotePackagePatch';

defineOptions({ name: 'RemotePackagePatchPage' });

const config = ref<AppConfig | null>(null);
const host = ref('');
const port = ref(22);
const username = ref('root');
const authMode = ref<'password' | 'keyFile'>('password');
const password = ref('');
const keyPath = ref('');
const passphrase = ref('');
const connected = ref(false);
const connectionMessage = ref('');
const busy = ref(false);
const remotePath = ref('/');
const remoteEntries = ref<RemoteDirEntry[]>([]);
const selectedPackage = ref('');
const replacement = ref<PickedLocalFile | null>(null);

const sshConfig = computed<RemoteSshConfig>(() => ({
  host: host.value.trim(),
  port: Number(port.value),
  username: username.value.trim(),
  auth: authMode.value === 'password'
    ? { kind: 'password', password: password.value }
    : { kind: 'keyFile', keyPath: keyPath.value, passphrase: passphrase.value || null },
}));

const canConnect = computed(() => Boolean(host.value.trim() && username.value.trim() && port.value > 0 && (authMode.value === 'password' ? password.value : keyPath.value)));
const defaultOutput = computed(() => selectedPackage.value ? defaultPatchedPath(selectedPackage.value) : '');

onMounted(async () => {
  config.value = await getConfig();
});

async function testConnection() {
  busy.value = true;
  try {
    connectionMessage.value = await remotePackagePatchApi.testConnection(sshConfig.value);
    connected.value = true;
    await loadDir(remotePath.value);
  } finally {
    busy.value = false;
  }
}

async function loadDir(path: string) {
  busy.value = true;
  try {
    const listing: RemoteDirListing = await remotePackagePatchApi.listDir(sshConfig.value, path);
    remotePath.value = listing.path;
    remoteEntries.value = listing.entries;
  } finally {
    busy.value = false;
  }
}

function selectRemoteFile(entry: RemoteDirEntry) {
  if (entry.kind === 'file' && entry.name.endsWith('.tar.gz')) {
    selectedPackage.value = entry.path;
  }
}

async function pickReplacement() {
  replacement.value = await remotePackagePatchApi.pickLocalFile('replacement');
}
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-slate-50 p-6">
    <div class="mx-auto flex max-w-7xl flex-col gap-5">
      <header>
        <h1 class="text-2xl font-bold text-slate-950">Remote Package Patch</h1>
        <p class="mt-1 text-sm text-slate-500">Patch a Linux product package on the server without downloading the full archive.</p>
      </header>

      <section class="rounded-lg border border-slate-200 bg-white p-4">
        <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-800"><Server class="h-4 w-4" /> Connection</div>
        <div class="grid grid-cols-1 gap-3 lg:grid-cols-6">
          <input v-model="host" class="rpp-input lg:col-span-2" placeholder="Host / IP" />
          <input v-model.number="port" class="rpp-input" type="number" min="1" max="65535" placeholder="22" />
          <input v-model="username" class="rpp-input" placeholder="Username" />
          <select v-model="authMode" class="rpp-input">
            <option value="password">Password</option>
            <option value="keyFile">Private key</option>
          </select>
          <button class="rpp-primary" :disabled="busy || !canConnect" @click="testConnection">
            <ShieldCheck class="h-4 w-4" /> Connect
          </button>
        </div>
        <div class="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
          <input v-if="authMode === 'password'" v-model="password" class="rpp-input" type="password" autocomplete="new-password" placeholder="SSH password" />
          <template v-else>
            <input v-model="keyPath" class="rpp-input" placeholder="Private key file path" />
            <input v-model="passphrase" class="rpp-input" type="password" autocomplete="new-password" placeholder="Passphrase (optional)" />
          </template>
        </div>
        <p v-if="connectionMessage" class="mt-2 text-xs text-emerald-600">{{ connectionMessage }}</p>
      </section>

      <div class="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1.35fr)_minmax(360px,0.65fr)]">
        <RemoteDirBrowser
          :entries="remoteEntries"
          :path="remotePath"
          :loading="busy"
          :selected-path="selectedPackage"
          @open-dir="loadDir"
          @path-submit="loadDir"
          @refresh="loadDir(remotePath)"
          @select-file="selectRemoteFile"
        />

        <section class="rounded-lg border border-slate-200 bg-white p-4">
          <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-800"><KeyRound class="h-4 w-4" /> Patch Setup</div>
          <div class="space-y-3 text-sm">
            <div>
              <div class="text-xs font-medium uppercase text-slate-500">Selected package</div>
              <div class="mt-1 break-all rounded-md bg-slate-50 p-2 font-mono text-xs">{{ selectedPackage || 'Select a .tar.gz package from the remote browser' }}</div>
            </div>
            <button class="rpp-secondary" :disabled="!connected" @click="pickReplacement">Choose local replacement file</button>
            <div v-if="replacement" class="break-all rounded-md bg-slate-50 p-2 font-mono text-xs">{{ replacement.path }}</div>
            <div>
              <div class="text-xs font-medium uppercase text-slate-500">Default output</div>
              <div class="mt-1 break-all rounded-md bg-slate-50 p-2 font-mono text-xs">{{ defaultOutput || '-' }}</div>
            </div>
            <button class="rpp-primary w-full" disabled><Play class="h-4 w-4" /> Scan package structure</button>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.rpp-input {
  @apply rounded-md border border-slate-200 px-3 py-2 text-sm outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100;
}
.rpp-primary {
  @apply inline-flex items-center justify-center gap-2 rounded-md bg-blue-600 px-3 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:bg-slate-300;
}
.rpp-secondary {
  @apply inline-flex items-center justify-center gap-2 rounded-md border border-slate-200 bg-white px-3 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50;
}
</style>
```

- [ ] **Step 2: Verify**

Run:

```powershell
pnpm check
```

Expected: page compiles. Fix any missing imports or event type mismatches.

---

### Task 9: Replacement Target Selection, Scan Inventory, And Output Policy UI

**Files:**
- Modify: `src/pages/RemotePackagePatchPage.vue`
- Modify: `src/lib/remotePackagePatch.ts`

**Interfaces:**
- Consumes: `remotePackagePatchApi.scanPackage`
- Produces: selected `targetInternalPath`, `targetLayer`, output policy, overwrite confirmation state.

- [ ] **Step 1: Add inventory and target state**

In page script:

```ts
import type { InternalLayer, PackageEntry, PackageInventory, PackagePatchRequest } from '@/lib/tauri';
import { buildInternalDirectoryTree, composeInternalTargetPath, replacementName, targetCandidates, validateInternalTargetPath } from '@/lib/remotePackagePatch';

const inventory = ref<PackageInventory | null>(null);
const scanBusy = ref(false);
const selectedCandidate = ref<PackageEntry | null>(null);
const internalDirectory = ref('');
const internalFileName = ref('');
const manualInternalPath = ref('');
const targetMode = ref<'candidate' | 'directory' | 'manual'>('candidate');
const outputPath = ref('');
const overwrite = ref(false);
const overwriteConfirmed = ref(false);

const candidates = computed(() => inventory.value && replacement.value
  ? targetCandidates(inventory.value, replacementName(replacement.value.name || replacement.value.path))
  : []);
const internalTree = computed(() => buildInternalDirectoryTree(inventory.value?.entries ?? []));
const targetInternalPath = computed(() => {
  if (targetMode.value === 'candidate') return selectedCandidate.value?.path ?? '';
  if (targetMode.value === 'directory') return composeInternalTargetPath(internalDirectory.value, internalFileName.value);
  return manualInternalPath.value;
});
const targetLayer = computed<InternalLayer | null>(() => {
  if (targetMode.value === 'candidate') return selectedCandidate.value?.layer ?? null;
  return null;
});
const targetError = computed(() => validateInternalTargetPath(targetInternalPath.value));
const canStartPatch = computed(() => connected.value && selectedPackage.value && replacement.value && !targetError.value && (overwrite.value ? overwriteConfirmed.value : outputPath.value));

async function scanPackage() {
  if (!selectedPackage.value) return;
  scanBusy.value = true;
  try {
    inventory.value = await remotePackagePatchApi.scanPackage(sshConfig.value, selectedPackage.value);
    outputPath.value = defaultPatchedPath(selectedPackage.value);
    internalFileName.value = replacement.value ? replacementName(replacement.value.path) : '';
    selectedCandidate.value = candidates.value[0] ?? null;
    targetMode.value = selectedCandidate.value ? 'candidate' : 'directory';
  } finally {
    scanBusy.value = false;
  }
}
```

- [ ] **Step 2: Add target selection UI**

In setup panel template, replace the disabled scan button with:

```vue
<button class="rpp-primary w-full" :disabled="!selectedPackage || !replacement || scanBusy" @click="scanPackage">
  <Play class="h-4 w-4" /> {{ scanBusy ? 'Scanning...' : 'Scan package structure' }}
</button>

<div v-if="inventory" class="space-y-3 border-t border-slate-100 pt-3">
  <div class="flex gap-2">
    <button class="rpp-tab" :class="targetMode === 'candidate' ? 'rpp-tab-active' : ''" @click="targetMode = 'candidate'">Candidates</button>
    <button class="rpp-tab" :class="targetMode === 'directory' ? 'rpp-tab-active' : ''" @click="targetMode = 'directory'">Directory</button>
    <button class="rpp-tab" :class="targetMode === 'manual' ? 'rpp-tab-active' : ''" @click="targetMode = 'manual'">Manual</button>
  </div>

  <div v-if="targetMode === 'candidate'" class="max-h-48 overflow-auto rounded-md border border-slate-200">
    <label v-for="entry in candidates" :key="`${JSON.stringify(entry.layer)}:${entry.path}`" class="flex cursor-pointer gap-2 border-b border-slate-100 p-2 text-xs hover:bg-blue-50">
      <input v-model="selectedCandidate" type="radio" :value="entry" />
      <span class="break-all font-mono">{{ entry.path }}</span>
    </label>
    <div v-if="candidates.length === 0" class="p-3 text-xs text-slate-500">No same-name candidates. Use Directory or Manual.</div>
  </div>

  <div v-else-if="targetMode === 'directory'" class="space-y-2">
    <input v-model="internalDirectory" class="rpp-input w-full" placeholder="Internal directory, e.g. app/func-msserver/bin" />
    <input v-model="internalFileName" class="rpp-input w-full" placeholder="Target file name" />
  </div>

  <div v-else>
    <input v-model="manualInternalPath" class="rpp-input w-full" placeholder="Full internal target path" />
  </div>

  <p v-if="targetError" class="text-xs font-medium text-red-600">{{ targetError }}</p>

  <div class="space-y-2">
    <label class="flex items-center gap-2 text-sm">
      <input v-model="overwrite" type="checkbox" class="rounded border-slate-300" />
      Backup original package and overwrite source path
    </label>
    <input v-if="!overwrite" v-model="outputPath" class="rpp-input w-full font-mono" />
    <label v-if="overwrite" class="flex items-start gap-2 rounded-md border border-amber-200 bg-amber-50 p-2 text-xs text-amber-800">
      <input v-model="overwriteConfirmed" type="checkbox" class="mt-0.5" />
      I understand the original package will be backed up first, then replaced after verification succeeds.
    </label>
  </div>
</div>
```

Add styles:

```css
.rpp-tab {
  @apply rounded-md border border-slate-200 px-3 py-1.5 text-xs font-semibold text-slate-600 hover:bg-slate-50;
}
.rpp-tab-active {
  @apply border-blue-200 bg-blue-50 text-blue-700;
}
```

- [ ] **Step 3: Verify**

Run:

```powershell
pnpm check
node --test src/lib/remotePackagePatch.test.mjs
```

Expected: type check and helper tests pass.

---

### Task 10: Execute Patch UI, Event Handling, Stages, And Results

**Files:**
- Modify: `src/pages/RemotePackagePatchPage.vue`

**Interfaces:**
- Consumes: `remote-package-patch-event` via Tauri event listener.
- Produces: execution logs, stage status, result panel.

- [ ] **Step 1: Add event state**

In page script:

```ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { onBeforeUnmount } from 'vue';
import type { PackagePatchResult, RemotePackagePatchEvent } from '@/lib/tauri';
import { orderedStages } from '@/lib/remotePackagePatch';

const running = ref(false);
const activeStage = ref('');
const completedStages = ref<string[]>([]);
const logs = ref<Array<{ level: string; message: string }>>([]);
const result = ref<PackagePatchResult | null>(null);
const uploadProgress = ref<{ sent: number; total: number } | null>(null);
let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  config.value = await getConfig();
  unlisten = await listen<RemotePackagePatchEvent>('remote-package-patch-event', (event) => {
    const payload = event.payload;
    if (payload.kind === 'stage' && payload.stage) {
      if (activeStage.value && !completedStages.value.includes(activeStage.value)) {
        completedStages.value.push(activeStage.value);
      }
      activeStage.value = payload.stage;
    } else if (payload.kind === 'log' && payload.message) {
      logs.value.push({ level: payload.level ?? 'info', message: payload.message });
    } else if (payload.kind === 'uploadProgress' && payload.sent != null && payload.total != null) {
      uploadProgress.value = { sent: payload.sent, total: payload.total };
    }
  });
});

onBeforeUnmount(() => {
  unlisten?.();
});

async function startPatch() {
  if (!replacement.value) return;
  running.value = true;
  result.value = null;
  logs.value = [];
  completedStages.value = [];
  activeStage.value = 'upload';
  try {
    const request: PackagePatchRequest = {
      config: sshConfig.value,
      packagePath: selectedPackage.value,
      replacementLocalPath: replacement.value.path,
      targetInternalPath: targetInternalPath.value,
      targetLayer: targetLayer.value,
      output: overwrite.value ? { mode: 'overwrite' } : { mode: 'newFile', outputPath: outputPath.value },
    };
    result.value = await remotePackagePatchApi.startPatch(request);
    if (activeStage.value && !completedStages.value.includes(activeStage.value)) {
      completedStages.value.push(activeStage.value);
    }
  } finally {
    running.value = false;
  }
}
```

If duplicate `onMounted` exists from Task 8, merge the existing config load and listener setup into one `onMounted`.

- [ ] **Step 2: Add execution panel template**

Below the main grid:

```vue
<section class="rounded-lg border border-slate-200 bg-white p-4">
  <div class="mb-3 flex items-center justify-between gap-3">
    <div class="text-sm font-semibold text-slate-800">Execution</div>
    <button class="rpp-primary" :disabled="running || !canStartPatch" @click="startPatch">
      <Play class="h-4 w-4" /> {{ running ? 'Running...' : 'Start patch' }}
    </button>
  </div>

  <div v-if="uploadProgress" class="mb-3 h-2 overflow-hidden rounded-full bg-slate-100">
    <div class="h-full bg-blue-500" :style="{ width: `${Math.min(100, (uploadProgress.sent / Math.max(1, uploadProgress.total)) * 100)}%` }"></div>
  </div>

  <div class="grid grid-cols-1 gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
    <ol class="space-y-1">
      <li v-for="stage in orderedStages()" :key="stage" class="rounded-md px-2 py-1 text-xs"
          :class="completedStages.includes(stage) ? 'bg-emerald-50 text-emerald-700' : activeStage === stage ? 'bg-blue-50 text-blue-700' : 'bg-slate-50 text-slate-500'">
        {{ stage }}
      </li>
    </ol>
    <div class="max-h-64 overflow-auto rounded-md bg-slate-950 p-3 font-mono text-xs text-slate-100">
      <div v-for="(log, index) in logs" :key="index" :class="log.level === 'error' ? 'text-red-300' : log.level === 'warn' ? 'text-amber-300' : 'text-slate-100'">
        [{{ log.level }}] {{ log.message }}
      </div>
      <div v-if="logs.length === 0" class="text-slate-500">No logs yet.</div>
    </div>
  </div>

  <div v-if="result" class="mt-4 grid grid-cols-1 gap-2 rounded-md border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-900">
    <div><strong>Output:</strong> <span class="font-mono break-all">{{ result.outputPath }}</span></div>
    <div v-if="result.backupPath"><strong>Backup:</strong> <span class="font-mono break-all">{{ result.backupPath }}</span></div>
    <div><strong>Target MD5:</strong> <span class="font-mono">{{ result.targetMd5 }}</span></div>
    <div><strong>Workdir:</strong> <span class="font-mono break-all">{{ result.workdir }}</span></div>
  </div>
</section>
```

- [ ] **Step 3: Verify**

Run:

```powershell
pnpm check
```

Expected: page compiles.

---

### Task 11: Register Tool In Navigation, Tool Hub, Icons, And I18n

**Files:**
- Modify: `src/router/index.ts`
- Modify: `src/lib/sidebarNavigation.ts`
- Modify: `src/components/Sidebar.vue`
- Modify: `src/pages/ToolsHubPage.vue`
- Modify: `src/locales/messages.ts`
- Modify/Test: `src/lib/sidebarNavigation.test.mjs` if it asserts exact tool paths.

**Interfaces:**
- Route `/tools/remote-package-patch`.
- Sidebar icon key `remotePackagePatch`.
- Locale keys under `remotePackagePatch` or `tools.remotePackagePatch`.

- [ ] **Step 1: Add route**

In `src/router/index.ts`:

```ts
{
  path: '/tools/remote-package-patch',
  component: () => import('../pages/RemotePackagePatchPage.vue'),
},
```

- [ ] **Step 2: Add sidebar item**

In `src/lib/sidebarNavigation.ts`, add icon key:

```ts
| 'remotePackagePatch'
```

Add a tools item:

```ts
{
  key: 'remote-package-patch',
  labelKey: 'sidebar.remotePackagePatch',
  path: '/tools/remote-package-patch',
  iconKey: 'remotePackagePatch',
  matchMode: 'prefix',
},
```

- [ ] **Step 3: Add icon mapping**

In `src/components/Sidebar.vue`, import `PackageSearch` or `Archive` from lucide if available:

```ts
PackageSearch,
```

Add:

```ts
remotePackagePatch: PackageSearch,
```

- [ ] **Step 4: Add tool hub card**

In `src/pages/ToolsHubPage.vue`, import the same icon and add a card:

```ts
{
  key: 'remote-package-patch',
  titleKey: 'sidebar.remotePackagePatch',
  descriptionKey: 'toolsHub.cards.remotePackagePatch.description',
  path: '/tools/remote-package-patch',
  icon: markRaw(PackageSearch as LucideIcon),
  iconClasses: 'from-blue-500 to-emerald-600 shadow-blue-500/20',
  chipKey: 'toolsHub.cards.remotePackagePatch.chip',
},
```

- [ ] **Step 5: Add locale strings**

In both `en` and `zh` sections of `src/locales/messages.ts`, add:

```ts
sidebar: {
  remotePackagePatch: 'Remote Package Patch',
}
```

and:

```ts
toolsHub: {
  cards: {
    remotePackagePatch: {
      description: 'Patch a remote Linux tar.gz package in place on the server, with md5 manifest updates.',
      chip: 'PACKAGE',
    },
  },
}
```

Use Chinese translations in the `zh` block.

- [ ] **Step 6: Verify nav tests and type check**

Run:

```powershell
node src/lib/sidebarNavigation.test.mjs
pnpm check
```

Expected: navigation tests and type check pass. If the navigation test asserts the exact tool count/path list, update it to include `/tools/remote-package-patch`.

---

### Task 12: Fixture Generator, Manual Validation Notes, Trellis Spec, And Final Checks

**Files:**
- Create: `scripts/dev/make-remote-package-patch-fixture.sh`
- Create: `.trellis/spec/backend/remote-package-patch.md`
- Modify: `.trellis/spec/backend/index.md`
- Modify: `.trellis/tasks/07-09-remote-package-patch/implement.md`

**Interfaces:**
- Produces a small Linux fixture package for manual validation.
- Records backend contracts for future sessions.

- [ ] **Step 1: Add fixture generator**

Create `scripts/dev/make-remote-package-patch-fixture.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-/tmp/fst-rpp-fixture}"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/root/app/demo/bin" "$OUT_DIR/work"

printf 'old library\n' > "$OUT_DIR/root/app/demo/bin/libdemo.so"
(
  cd "$OUT_DIR/root"
  md5sum app/demo/bin/libdemo.so > md5
  tar -cf "$OUT_DIR/work/inner.tar" app md5
)
zstd -q -f "$OUT_DIR/work/inner.tar" -o "$OUT_DIR/work/demo.tar.zst"

mkdir -p "$OUT_DIR/middle/pkg/app"
cp "$OUT_DIR/work/demo.tar.zst" "$OUT_DIR/middle/pkg/app/demo.tar.zst"
(
  cd "$OUT_DIR/middle"
  md5sum pkg/app/demo.tar.zst > pkg/md5
  tar -cf "$OUT_DIR/work/middle.tar" pkg
)

mkdir -p "$OUT_DIR/outer/VMS"
cp "$OUT_DIR/work/middle.tar" "$OUT_DIR/outer/VMS/VMS.tar"
(
  cd "$OUT_DIR/outer"
  tar -czf "$OUT_DIR/VMS-fixture.tar.gz" VMS
)

printf 'Fixture written: %s\n' "$OUT_DIR/VMS-fixture.tar.gz"
printf 'Replacement target: app/demo/bin/libdemo.so in zst layer pkg/app/demo.tar.zst\n'
```

- [ ] **Step 2: Add backend spec**

Create `.trellis/spec/backend/remote-package-patch.md`:

```markdown
# Remote Package Patch Backend Contracts

## Commands

- `remote_package_test_connection(config)` validates SSH credentials and returns `uname -sr`.
- `remote_package_list_dir(config, path)` lists one remote directory via SFTP. Entries are sorted with directories first.
- `remote_package_pick_local_file(kind)` opens a native local file picker for replacement files or private keys.
- `remote_package_scan_package(config, packagePath)` runs a remote scan script and returns a full package inventory.
- `remote_package_start_patch(request)` uploads the replacement file, runs the remote patch script, and returns output path, backup path, target md5, workdir, and updated manifest paths.

## Safety Rules

- Default output mode never overwrites the source package.
- Overwrite mode backs up the source package first and only replaces it after verification succeeds.
- Workdir is created beside the package by default and is logged.
- Credentials are session-only and must not be written to config or logs.
- md5 updates are exact-path only; same-name files are not batch-updated.
- Scan and patch share one `PATCH_BUSY` guard; connection test and directory listing do not use that guard.
```

Add to `.trellis/spec/backend/index.md` table:

```markdown
| [Remote Package Patch](./remote-package-patch.md) | Contracts for remote SSH/SFTP package browsing, nested tar.zst rewrite, md5 updates, and safety defaults | Active |
```

- [ ] **Step 3: Sync Trellis implementation artifact**

Replace `.trellis/tasks/07-09-remote-package-patch/implement.md` with a short pointer:

```markdown
# Remote Package Patch Implementation Plan

The detailed implementation plan is maintained at:

- `docs/superpowers/plans/2026-07-09-remote-package-patch-implementation.md`

Use that plan as the task-by-task execution source. Trellis inline mode skips jsonl curation; load backend/frontend specs through `trellis-before-dev` before editing.

Validation commands:

- `cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch`
- `node --test src/lib/remotePackagePatch.test.mjs`
- `node src/lib/sidebarNavigation.test.mjs`
- `pnpm check`
```

- [ ] **Step 4: Run final automated checks**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch
node --test src/lib/remotePackagePatch.test.mjs
node src/lib/sidebarNavigation.test.mjs
pnpm check
```

Expected: all pass, except any known unrelated existing issues must be documented with exact output summary.

- [ ] **Step 5: Manual validation on Linux**

On a Linux host with `zstd`:

```bash
bash scripts/dev/make-remote-package-patch-fixture.sh /tmp/fst-rpp-fixture
printf 'new library\n' > /tmp/fst-rpp-fixture/libdemo.so
```

Then use the app:

- Connect to that host.
- Select `/tmp/fst-rpp-fixture/VMS-fixture.tar.gz`.
- Select `/tmp/fst-rpp-fixture/libdemo.so` as the local replacement if running the app on the same machine, or a Windows local equivalent if testing from Windows.
- Scan package structure.
- Select `app/demo/bin/libdemo.so` in zst layer `pkg/app/demo.tar.zst`.
- Run with default new-file output.

Expected:

- Output package exists as `VMS-fixture.patched.tar.gz`.
- Original `VMS-fixture.tar.gz` remains unchanged.
- Extracted target file content is `new library`.
- Target file md5 equals the replacement file md5.
- Logs show workdir and stages through `cleanup`.

---

## Execution Order

Implement tasks in order. Stop after each task to run its listed verification command and fix failures before proceeding. Keep commits optional unless the user asks for them.

## Self-Review

- Spec coverage: Tasks 1-5 cover backend commands/scripts; Tasks 6-11 cover frontend workbench and navigation; Task 12 covers fixture, spec capture, and final validation.
- Marker scan: no unfinished-plan markers remain.
- Type consistency: Rust/TS command names align: `remote_package_test_connection`, `remote_package_list_dir`, `remote_package_scan_package`, `remote_package_pick_local_file`, `remote_package_start_patch`.
