# 文件共享 Web 界面 URL 路由 / 深链 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给文件共享 Web 界面加入 hash 路由（CHFS 风格 `#/UMS_TEMP/sub`），URL ↔ 目录状态双向同步，分享 / 书签 / 刷新不丢状态。

**Architecture:** 前端新增零依赖 `lib/url-state.ts` 做 URL ↔ state 转换；`App.vue` 在 `loadTree` / `executeSearch` / `clearSearch` 后写回 hash，监听 `popstate`/`hashchange` 重新加载。后端新增 `GET /api/resolve` 把 segments 数组解析成 `node_id`（只接受磁盘 `read_dir` 实际返回的 entry，杜绝路径穿越）。

**Tech Stack:** Rust (axum, tokio) / Vue 3 + TypeScript / vitest + jsdom（新引入）

**Spec:** [docs/superpowers/specs/2026-04-22-file-share-web-deep-link-design.md](../specs/2026-04-22-file-share-web-deep-link-design.md)

---

## File Structure

### 新建

| 路径 | 责任 |
|---|---|
| `src/share-web/lib/url-state.ts` | hash 解析 / 序列化 / push / replace / subscribe |
| `src/share-web/lib/__tests__/url-state.test.ts` | url-state 单测 |
| `vitest.config.ts` | vitest 配置（仅扫描 share-web） |
| `docs/superpowers/plans/2026-04-22-file-share-web-deep-link-implementation.md` | 本计划 |

### 修改

| 路径 | 责任 |
|---|---|
| `src-tauri/src/fileshare/ops.rs` | 新增 `resolve_path_segments` + 单测 |
| `src-tauri/src/fileshare/http.rs` | 新增 `/api/resolve` handler 与路由 |
| `src/share-web/api.ts` | 新增 `resolvePath` |
| `src/share-web/types.ts` | 新增 `FileShareResolveResponse` 类型 |
| `src/share-web/App.vue` | 集成 URL 同步：`bootstrapFromUrl`/`applyUrlState`/`loadTree(urlAction)` |
| `src/share-web/messages.ts` | 新增 `app.forbiddenDirectory`（en+zh） |
| `package.json` | 新增 `vitest`、`jsdom` devDep + `test:share-web` script |

### 范围外（不动）

- `src/main.ts`、`src/router/index.ts`（主 app 路由独立）
- `src/share-web/components/*`（只 App.vue 集成路由）
- `src-tauri/src/fileshare/{model,persist,auth,search,web_assets}.rs`

---

## 命名要点

- 后端 ops 新类型用 `ResolvedPathNode`（避免与 `http.rs` 已存在的 `ResolvedNode` 冲突）
- 后端枚举 `ResolveError { NotFound, Forbidden }`，节点种类枚举 `ResolveNodeKind { Home, ShareRoot, Directory }`
- 前端类型 `FileShareResolveResponse { node_id: string | null; kind: 'home' | 'share_root' | 'directory'; canonical_segments: string[] }`
- `loadTree` 新参 `urlAction: 'push' | 'replace' | 'none'`（默认 `'replace'`）

---

## Task 1: 新增 vitest 测试基础设施

**Files:**
- Modify: `package.json`
- Create: `vitest.config.ts`

- [ ] **Step 1: 装 vitest + jsdom**

Run:
```
pnpm add -D vitest@^1.6.0 jsdom@^24.0.0
```

- [ ] **Step 2: 在 `package.json` 新增 script**

将 `package.json` 中 `"check-env": "cargo --version"` 上面一行替换为：
```json
    "check-env": "cargo --version",
    "test:share-web": "vitest run --config vitest.config.ts"
```
（保留逗号与缩进与现有一致）

- [ ] **Step 3: 创建 `vitest.config.ts`**

写到 repo 根：
```ts
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'jsdom',
    include: ['src/share-web/**/*.test.ts'],
    globals: false,
  },
});
```

- [ ] **Step 4: 验证 vitest 能跑（空发现）**

Run: `pnpm test:share-web`
Expected: 输出 `No test files found, exiting with code 0`（或类似），exit code 0。

- [ ] **Step 5: 提交**

```
git add package.json pnpm-lock.yaml vitest.config.ts
git commit -m "chore(share-web): 引入 vitest 测试基础设施"
```

---

## Task 2: 后端 — 空 segments 返回 Home（红→绿）

**Files:**
- Modify: `src-tauri/src/fileshare/ops.rs`

- [ ] **Step 1: 在 `ops.rs` 末尾的 `mod tests` 之前定义新类型**

把以下追加到 `ops.rs` 中（位置：紧贴现有最后一个 `pub fn` 之后、`#[cfg(test)]` 之前）：
```rust
// ─── Path Resolution (deep-link support) ─────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveNodeKind {
    Home,
    ShareRoot,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPathNode {
    pub kind: ResolveNodeKind,
    pub root_id: Option<String>,         // None → Home
    pub relative_path: String,           // "" 表示 root 或 home
    pub canonical_segments: Vec<String>, // [] 表示 home
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    NotFound,
    Forbidden,
}
```

- [ ] **Step 2: 写第一个失败测试**

在 `ops.rs` 的 `#[cfg(test)] mod tests { ... }` 块内追加：
```rust
#[test]
fn resolve_empty_path_returns_home() {
    let roots = Vec::<ResolvedRoot>::new();
    let principal_perms = model::FileSharePermissionSet::read_only();
    let result = resolve_path_segments(&roots, &principal_perms, &[], &[]).unwrap();
    assert_eq!(result.kind, ResolveNodeKind::Home);
    assert_eq!(result.root_id, None);
    assert_eq!(result.relative_path, "");
    assert!(result.canonical_segments.is_empty());
}
```

注意：测试使用第 4 个参数 `&[]`（root_permissions slice）—签名后续步骤定义。

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_empty_path_returns_home`
Expected: 编译错误 `cannot find function resolve_path_segments`

- [ ] **Step 4: 实现最小函数**

在 `ops.rs` 同位置追加（紧跟类型定义之后）：
```rust
pub fn resolve_path_segments(
    _roots: &[ResolvedRoot],
    _principal_permissions: &model::FileSharePermissionSet,
    _root_permissions: &[model::UserRootPermissions],
    segments: &[String],
) -> Result<ResolvedPathNode, ResolveError> {
    if segments.is_empty() {
        return Ok(ResolvedPathNode {
            kind: ResolveNodeKind::Home,
            root_id: None,
            relative_path: String::new(),
            canonical_segments: Vec::new(),
        });
    }
    Err(ResolveError::NotFound)
}
```

确保文件顶部（如还没引入）有：
```rust
use crate::fileshare::model;
```
（如已存在跳过；ops.rs 现有代码若已有 `use super::model::...` 改为相对引用即可，按现有模式来）

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_empty_path_returns_home`
Expected: PASS

- [ ] **Step 6: 提交**

```
git add src-tauri/src/fileshare/ops.rs
git commit -m "feat(file-share/backend): resolve_path_segments 空路径返回 Home"
```

---

## Task 3: 后端 — share root 解析

**Files:**
- Modify: `src-tauri/src/fileshare/ops.rs`

- [ ] **Step 1: 写测试**

`mod tests` 内追加：
```rust
#[test]
fn resolve_root_only_returns_share_root() {
    let dir = TestDir::new("resolve-root");
    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "UMS_TEMP".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::ReadOnly,
        permissions: model::FileSharePermissionSet::read_only(),
    }];

    let result = resolve_path_segments(
        &roots,
        &perms,
        &root_perms,
        &["UMS_TEMP".to_string()],
    )
    .unwrap();

    assert_eq!(result.kind, ResolveNodeKind::ShareRoot);
    assert_eq!(result.root_id.as_deref(), Some("ums"));
    assert_eq!(result.relative_path, "");
    assert_eq!(result.canonical_segments, vec!["UMS_TEMP".to_string()]);
}
```

注意：`TestDir` 已存在于 `mod.rs::tests`，需要在 `ops.rs::tests` 顶部新增同结构（复制粘贴 mod.rs 中 14 行的 `TestDir` 定义），或在 ops 测试模块顶部 `use super::super::tests::TestDir;`——后者依赖 visibility，更脆弱。**采用复制方案**。

在 `ops.rs::tests` 的 `use super::*;` 下追加：
```rust
use std::fs;
use std::path::{Path, PathBuf};

struct TestDir(PathBuf);
impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "fst-fileshare-ops-test-{}-{}",
            label,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("test temp dir should be created");
        Self(path)
    }
    fn path(&self) -> &Path { &self.0 }
}
impl Drop for TestDir {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_root_only_returns_share_root`
Expected: FAIL（返回 NotFound）

- [ ] **Step 3: 扩展实现**

替换 Task 2 写的 `resolve_path_segments` 函数体为：
```rust
pub fn resolve_path_segments(
    roots: &[ResolvedRoot],
    _principal_permissions: &model::FileSharePermissionSet,
    root_permissions: &[model::UserRootPermissions],
    segments: &[String],
) -> Result<ResolvedPathNode, ResolveError> {
    if segments.is_empty() {
        return Ok(ResolvedPathNode {
            kind: ResolveNodeKind::Home,
            root_id: None,
            relative_path: String::new(),
            canonical_segments: Vec::new(),
        });
    }

    let root_segment = &segments[0];
    let root = roots
        .iter()
        .find(|r| r.id == *root_segment || r.alias == *root_segment)
        .ok_or(ResolveError::NotFound)?;

    let has_browse = root_permissions
        .iter()
        .find(|rp| rp.root_id == root.id)
        .map(|rp| rp.permissions.browse)
        .unwrap_or(false);
    if !has_browse {
        return Err(ResolveError::Forbidden);
    }

    if segments.len() == 1 {
        return Ok(ResolvedPathNode {
            kind: ResolveNodeKind::ShareRoot,
            root_id: Some(root.id.clone()),
            relative_path: String::new(),
            canonical_segments: vec![root_segment.clone()],
        });
    }

    Err(ResolveError::NotFound)
}
```

- [ ] **Step 4: 跑两个测试都通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_`
Expected: 两个测试 PASS

- [ ] **Step 5: 提交**

```
git add src-tauri/src/fileshare/ops.rs
git commit -m "feat(file-share/backend): resolve_path_segments 解析 share root + browse 权限校验"
```

---

## Task 4: 后端 — 嵌套目录解析（含 canonical 大小写校正）

**Files:**
- Modify: `src-tauri/src/fileshare/ops.rs`

- [ ] **Step 1: 写两个测试**

在 `mod tests` 追加：
```rust
#[test]
fn resolve_nested_directory_returns_canonical_segments() {
    let dir = TestDir::new("resolve-nested");
    fs::create_dir_all(dir.path().join("Sub").join("Leaf")).unwrap();

    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "UMS_TEMP".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::ReadOnly,
        permissions: model::FileSharePermissionSet::read_only(),
    }];

    let result = resolve_path_segments(
        &roots,
        &perms,
        &root_perms,
        &["UMS_TEMP".to_string(), "Sub".to_string(), "Leaf".to_string()],
    )
    .unwrap();

    assert_eq!(result.kind, ResolveNodeKind::Directory);
    assert_eq!(result.root_id.as_deref(), Some("ums"));
    assert_eq!(result.relative_path, "Sub/Leaf");
    assert_eq!(
        result.canonical_segments,
        vec!["UMS_TEMP".to_string(), "Sub".to_string(), "Leaf".to_string()]
    );
}

#[test]
fn resolve_corrects_subdirectory_case_from_disk() {
    let dir = TestDir::new("resolve-case");
    fs::create_dir_all(dir.path().join("Sub")).unwrap();

    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "UMS_TEMP".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::ReadOnly,
        permissions: model::FileSharePermissionSet::read_only(),
    }];

    let result = resolve_path_segments(
        &roots,
        &perms,
        &root_perms,
        &["UMS_TEMP".to_string(), "SUB".to_string()],
    )
    .unwrap();

    // Windows 下 read_dir 返回 "Sub"；canonical 应来自磁盘
    assert_eq!(result.canonical_segments, vec!["UMS_TEMP".to_string(), "Sub".to_string()]);
    assert_eq!(result.relative_path, "Sub");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_nested_directory_returns_canonical_segments fileshare::ops::tests::resolve_corrects_subdirectory_case_from_disk`
Expected: FAIL（返回 NotFound）

- [ ] **Step 3: 实现嵌套解析**

把 `resolve_path_segments` 函数末尾的 `Err(ResolveError::NotFound)` 之前的部分替换为：
```rust
    if segments.len() == 1 {
        return Ok(ResolvedPathNode {
            kind: ResolveNodeKind::ShareRoot,
            root_id: Some(root.id.clone()),
            relative_path: String::new(),
            canonical_segments: vec![root_segment.clone()],
        });
    }

    let mut current_path = root.path.clone();
    let mut canonical = vec![root_segment.clone()];

    for raw in &segments[1..] {
        if !is_valid_path_segment(raw) {
            return Err(ResolveError::NotFound);
        }
        let entries = std::fs::read_dir(&current_path).map_err(|_| ResolveError::NotFound)?;
        let mut matched: Option<String> = None;
        for entry in entries.flatten() {
            let name_os = entry.file_name();
            if let Some(name) = name_os.to_str() {
                if name.eq_ignore_ascii_case(raw) {
                    let file_type = entry.file_type().map_err(|_| ResolveError::NotFound)?;
                    if !file_type.is_dir() {
                        return Err(ResolveError::NotFound);
                    }
                    matched = Some(name.to_string());
                    break;
                }
            }
        }
        let canonical_name = matched.ok_or(ResolveError::NotFound)?;
        current_path.push(&canonical_name);
        canonical.push(canonical_name);
    }

    let relative_path = canonical[1..].join("/");
    Ok(ResolvedPathNode {
        kind: ResolveNodeKind::Directory,
        root_id: Some(root.id.clone()),
        relative_path,
        canonical_segments: canonical,
    })
}

fn is_valid_path_segment(segment: &str) -> bool {
    if segment.is_empty() || segment == "." || segment == ".." {
        return false;
    }
    !segment.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0'))
}
```

整体替换：把上面的 `if segments.len() == 1 { ... } Err(ResolveError::NotFound)` 段（来自 Task 3 的实现尾部）替换成本步骤的代码。注意结尾的 `}` 是 `resolve_path_segments` 函数的，紧跟其后是新的辅助函数 `is_valid_path_segment`。

- [ ] **Step 4: 跑所有 resolve 测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_`
Expected: 4 个测试 PASS

- [ ] **Step 5: 提交**

```
git add src-tauri/src/fileshare/ops.rs
git commit -m "feat(file-share/backend): resolve_path_segments 嵌套目录解析与 canonical 校正"
```

---

## Task 5: 后端 — 错误路径覆盖

**Files:**
- Modify: `src-tauri/src/fileshare/ops.rs`

- [ ] **Step 1: 写所有错误测试**

`mod tests` 追加：
```rust
#[test]
fn resolve_unknown_root_returns_not_found() {
    let roots: Vec<ResolvedRoot> = Vec::new();
    let perms = model::FileSharePermissionSet::read_only();
    let err = resolve_path_segments(&roots, &perms, &[], &["nope".into()]).unwrap_err();
    assert_eq!(err, ResolveError::NotFound);
}

#[test]
fn resolve_missing_segment_returns_not_found() {
    let dir = TestDir::new("resolve-missing");
    fs::create_dir_all(dir.path().join("only")).unwrap();

    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "ums".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::ReadOnly,
        permissions: model::FileSharePermissionSet::read_only(),
    }];

    let err = resolve_path_segments(
        &roots,
        &perms,
        &root_perms,
        &["ums".into(), "missing".into()],
    )
    .unwrap_err();
    assert_eq!(err, ResolveError::NotFound);
}

#[test]
fn resolve_file_segment_returns_not_found() {
    let dir = TestDir::new("resolve-file");
    fs::write(dir.path().join("a.txt"), b"x").unwrap();

    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "ums".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::ReadOnly,
        permissions: model::FileSharePermissionSet::read_only(),
    }];

    let err = resolve_path_segments(
        &roots,
        &perms,
        &root_perms,
        &["ums".into(), "a.txt".into()],
    )
    .unwrap_err();
    assert_eq!(err, ResolveError::NotFound);
}

#[test]
fn resolve_rejects_path_traversal_segments() {
    let dir = TestDir::new("resolve-traversal");
    fs::create_dir_all(dir.path().join("safe")).unwrap();

    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "ums".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::ReadOnly,
        permissions: model::FileSharePermissionSet::read_only(),
    }];

    for bad in [
        "..", ".", "", "a/b", "a\\b", "C:", "with\0null",
    ] {
        let err = resolve_path_segments(
            &roots,
            &perms,
            &root_perms,
            &["ums".into(), bad.into()],
        )
        .unwrap_err();
        assert_eq!(err, ResolveError::NotFound, "segment {:?} should be rejected", bad);
    }
}

#[test]
fn resolve_without_browse_permission_is_forbidden() {
    let dir = TestDir::new("resolve-forbidden");
    let roots = vec![ResolvedRoot {
        id: "ums".into(),
        alias: "ums".into(),
        path: dir.path().to_path_buf(),
    }];
    let perms = model::FileSharePermissionSet::read_only();
    let root_perms = vec![model::UserRootPermissions {
        root_id: "ums".into(),
        preset: model::PermissionPreset::Custom,
        permissions: model::FileSharePermissionSet::deny_all(),
    }];

    let err = resolve_path_segments(
        &roots,
        &perms,
        &root_perms,
        &["ums".into()],
    )
    .unwrap_err();
    assert_eq!(err, ResolveError::Forbidden);
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::ops::tests::resolve_`
Expected: 全部 9 个 PASS（之前 4 + 这次 5）

如果 `resolve_file_segment_returns_not_found` 因 `eq_ignore_ascii_case` 命中文件后才检查 `is_dir`，这是 Task 4 实现里已有的逻辑——预期通过。如果失败检查 Task 4 实现是否在匹配后正确判断 `is_dir`。

- [ ] **Step 3: 提交**

```
git add src-tauri/src/fileshare/ops.rs
git commit -m "test(file-share/backend): resolve_path_segments 错误路径覆盖"
```

---

## Task 6: 后端 — `/api/resolve` HTTP 端点

**Files:**
- Modify: `src-tauri/src/fileshare/http.rs`

- [ ] **Step 1: 在 `http.rs` Query 定义区追加**

在 `struct ApiTreeQuery` 上面（约 65 行）追加：
```rust
#[derive(Deserialize)]
struct ApiResolveQuery {
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiResolveResponse {
    node_id: Option<String>,
    kind: ApiTreeCurrentKind,
    canonical_segments: Vec<String>,
}
```

- [ ] **Step 2: 写 URL-decode helper**

文件底部（任意位置，建议紧跟 `decode_node_id_part` 之后）追加：
```rust
fn decode_path_query(raw: &str) -> Result<Vec<String>, ()> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut segments = Vec::new();
    for piece in raw.split('/') {
        if piece.is_empty() {
            continue;
        }
        let decoded = percent_decode(piece)?;
        segments.push(decoded);
    }
    Ok(segments)
}

fn percent_decode(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(());
                }
                let hi = hex_to_u8(bytes[i + 1])?;
                let lo = hex_to_u8(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex_to_u8(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(10 + b - b'a'),
        b'A'..=b'F' => Ok(10 + b - b'A'),
        _ => Err(()),
    }
}
```

- [ ] **Step 3: 写 handler**

文件中找到 `async fn handler_tree(...)` 函数（搜 `async fn handler_tree`），在它**之前**插入：
```rust
async fn handler_resolve(
    AxumState(state): AxumState<Arc<HttpState>>,
    Query(query): Query<ApiResolveQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let client_ip = addr.ip();
    let principal = match require_request_permission(
        &state,
        &headers,
        client_ip,
        model::FileSharePermission::Browse,
        false,
    ) {
        Ok(principal) => principal,
        Err(response) => return response,
    };

    let raw = query.path.as_deref().unwrap_or("");
    let segments = match decode_path_query(raw) {
        Ok(s) => s,
        Err(_) => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };

    let runtime = state.request_runtime();
    let result = ops::resolve_path_segments(
        &runtime.roots,
        &principal.permissions,
        &principal.root_permissions,
        &segments,
    );

    let node = match result {
        Ok(node) => node,
        Err(ops::ResolveError::NotFound) => {
            return plain_response(StatusCode::NOT_FOUND, "Not Found");
        }
        Err(ops::ResolveError::Forbidden) => {
            return plain_response(StatusCode::FORBIDDEN, "Forbidden");
        }
    };

    let (kind, node_id) = match node.kind {
        ops::ResolveNodeKind::Home => (ApiTreeCurrentKind::Home, None),
        ops::ResolveNodeKind::ShareRoot => (
            ApiTreeCurrentKind::ShareRoot,
            node.root_id
                .as_ref()
                .map(|root_id| encode_node_id(&NodeLocator::ShareRoot { root_id: root_id.clone() })),
        ),
        ops::ResolveNodeKind::Directory => (
            ApiTreeCurrentKind::Directory,
            node.root_id.as_ref().map(|root_id| {
                encode_node_id(&NodeLocator::Directory {
                    root_id: root_id.clone(),
                    relative_path: node.relative_path.clone(),
                })
            }),
        ),
    };

    Json(ApiResolveResponse {
        node_id,
        kind,
        canonical_segments: node.canonical_segments,
    })
    .into_response()
}
```

- [ ] **Step 4: 注册路由**

把 `build_router` 里的 `.route("/api/tree", get(handler_tree))` 那行改为：
```rust
        .route("/api/resolve", get(handler_resolve))
        .route("/api/tree", get(handler_tree))
```

- [ ] **Step 5: 暴露 ops 类型**

在 `ops.rs` 顶部确保 `ResolveError`、`ResolveNodeKind`、`ResolvedPathNode`、`resolve_path_segments` 都是 `pub`（Task 2 已设为 pub，验证）。

- [ ] **Step 6: 编译并跑现有测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare`
Expected: 所有现有测试 + Task 2-5 的新测试全部 PASS。

- [ ] **Step 7: 提交**

```
git add src-tauri/src/fileshare/http.rs src-tauri/src/fileshare/ops.rs
git commit -m "feat(file-share/backend): /api/resolve 解析路径段到 node_id"
```

---

## Task 7: 后端 — HTTP 端点集成测试

**Files:**
- Modify: `src-tauri/src/fileshare/http.rs`

- [ ] **Step 1: 看现有测试是否存在 mod tests in http.rs**

Run: `grep -n "mod tests" src-tauri/src/fileshare/http.rs`
Expected: 可能为空（http.rs 当前可能无单测）。

如果无 `mod tests`，跳到 Step 2 直接写。如果已有，把测试追加进去。

- [ ] **Step 2: 写 percent_decode 与 decode_path_query 单测**

由于直接对 axum handler 测要起服务，先只覆盖纯函数。在 `http.rs` 末尾追加（如已有 `#[cfg(test)] mod tests` 则追加在内部）：
```rust
#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn decode_path_query_empty() {
        assert!(decode_path_query("").unwrap().is_empty());
    }

    #[test]
    fn decode_path_query_skips_empty_segments() {
        let segs = decode_path_query("a//b").unwrap();
        assert_eq!(segs, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn decode_path_query_decodes_percent_encoded_chinese() {
        // "报表" → URL encoded as %E6%8A%A5%E8%A1%A8
        let segs = decode_path_query("UMS/%E6%8A%A5%E8%A1%A8").unwrap();
        assert_eq!(segs, vec!["UMS".to_string(), "报表".to_string()]);
    }

    #[test]
    fn decode_path_query_rejects_malformed_percent() {
        assert!(decode_path_query("a%ZZ").is_err());
        assert!(decode_path_query("a%").is_err());
    }
}
```

- [ ] **Step 3: 跑测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::http::resolve_tests`
Expected: 4 个 PASS

- [ ] **Step 4: 提交**

```
git add src-tauri/src/fileshare/http.rs
git commit -m "test(file-share/backend): decode_path_query 单测"
```

---

## Task 8: 前端 — `url-state.ts` 接口与第一批 parseHash 测试

**Files:**
- Create: `src/share-web/lib/url-state.ts`
- Create: `src/share-web/lib/__tests__/url-state.test.ts`

- [ ] **Step 1: 写测试文件骨架（红）**

写到 `src/share-web/lib/__tests__/url-state.test.ts`：
```ts
import { describe, it, expect, beforeEach } from 'vitest';
import { parseHash, serialize } from '../url-state';

describe('parseHash', () => {
  it('returns home for empty hash', () => {
    expect(parseHash('')).toEqual({ segments: [], q: '', scope: 'global' });
  });

  it('returns home for "#/"', () => {
    expect(parseHash('#/')).toEqual({ segments: [], q: '', scope: 'global' });
  });

  it('parses single root segment', () => {
    expect(parseHash('#/UMS_TEMP')).toEqual({
      segments: ['UMS_TEMP'],
      q: '',
      scope: 'current',
    });
  });

  it('parses nested segments', () => {
    expect(parseHash('#/UMS_TEMP/sub')).toEqual({
      segments: ['UMS_TEMP', 'sub'],
      q: '',
      scope: 'current',
    });
  });

  it('skips empty segments', () => {
    expect(parseHash('#/UMS_TEMP//sub')).toEqual({
      segments: ['UMS_TEMP', 'sub'],
      q: '',
      scope: 'current',
    });
  });

  it('parses query keyword and scope', () => {
    expect(parseHash('#/UMS_TEMP?q=foo&scope=current')).toEqual({
      segments: ['UMS_TEMP'],
      q: 'foo',
      scope: 'current',
    });
  });

  it('falls back scope when value is invalid', () => {
    expect(parseHash('#/UMS_TEMP?scope=bogus')).toEqual({
      segments: ['UMS_TEMP'],
      q: '',
      scope: 'current',
    });
  });

  it('decodes percent-encoded segments (chinese)', () => {
    const hash = '#/' + encodeURIComponent('报表');
    expect(parseHash(hash)).toEqual({
      segments: ['报表'],
      q: '',
      scope: 'current',
    });
  });

  it('treats "#?q=foo" like "#/?q=foo"', () => {
    expect(parseHash('#?q=foo')).toEqual({ segments: [], q: 'foo', scope: 'global' });
  });

  it('falls back to home on decode failure', () => {
    expect(parseHash('#/%ZZ')).toEqual({ segments: [], q: '', scope: 'global' });
  });
});

describe('serialize', () => {
  it('returns empty for home with no search', () => {
    expect(serialize({ segments: [], q: '', scope: 'global' })).toBe('');
  });

  it('emits leading slash + segments', () => {
    expect(serialize({ segments: ['UMS_TEMP', 'sub'], q: '', scope: 'current' })).toBe('/UMS_TEMP/sub');
  });

  it('emits query for keyword + scope', () => {
    const out = serialize({ segments: ['UMS_TEMP'], q: 'foo', scope: 'current' });
    expect(out).toBe('/UMS_TEMP?q=foo&scope=current');
  });

  it('emits query at home', () => {
    const out = serialize({ segments: [], q: 'foo', scope: 'global' });
    expect(out).toBe('/?q=foo&scope=global');
  });

  it('percent-encodes chinese segments', () => {
    const out = serialize({ segments: ['报表'], q: '', scope: 'current' });
    expect(out).toBe('/' + encodeURIComponent('报表'));
  });

  it('round-trips with parseHash', () => {
    const state = { segments: ['UMS_TEMP', '中文', 'leaf'], q: '关键字', scope: 'current' as const };
    const hash = '#' + serialize(state);
    expect(parseHash(hash)).toEqual(state);
  });
});
```

- [ ] **Step 2: 跑测试确认失败（模块不存在）**

Run: `pnpm test:share-web`
Expected: 全部 FAIL — `Cannot find module '../url-state'`

- [ ] **Step 3: 写 `url-state.ts` 实现**

写到 `src/share-web/lib/url-state.ts`：
```ts
export type SearchScope = 'global' | 'current';

export interface UrlState {
  segments: string[];
  q: string;
  scope: SearchScope;
}

const HOME_STATE: UrlState = { segments: [], q: '', scope: 'global' };

function defaultScope(segments: string[]): SearchScope {
  return segments.length === 0 ? 'global' : 'current';
}

function isSearchScope(value: string | null): value is SearchScope {
  return value === 'global' || value === 'current';
}

export function parseHash(hash: string = typeof location !== 'undefined' ? location.hash : ''): UrlState {
  if (!hash || hash === '#' || hash === '#/') {
    return { ...HOME_STATE };
  }
  const stripped = hash.startsWith('#') ? hash.slice(1) : hash;
  // "?q=..." without leading "/" → treat as home
  const normalized = stripped.startsWith('/') || stripped.startsWith('?')
    ? stripped
    : '/' + stripped;

  const queryIndex = normalized.indexOf('?');
  const pathPart = queryIndex >= 0 ? normalized.slice(0, queryIndex) : normalized;
  const queryPart = queryIndex >= 0 ? normalized.slice(queryIndex + 1) : '';

  let segments: string[];
  try {
    segments = pathPart
      .split('/')
      .filter((piece) => piece.length > 0)
      .map((piece) => decodeURIComponent(piece));
  } catch {
    return { ...HOME_STATE };
  }

  const params = new URLSearchParams(queryPart);
  const rawQ = params.get('q') ?? '';
  const rawScope = params.get('scope');
  const scope = isSearchScope(rawScope) ? rawScope : defaultScope(segments);

  return { segments, q: rawQ, scope };
}

export function serialize(state: UrlState): string {
  const hasSegments = state.segments.length > 0;
  const hasQuery = state.q.length > 0;

  if (!hasSegments && !hasQuery) {
    return '';
  }

  const path = hasSegments
    ? '/' + state.segments.map((segment) => encodeURIComponent(segment)).join('/')
    : '/';

  if (!hasQuery) {
    return path;
  }

  const params = new URLSearchParams();
  params.set('q', state.q);
  params.set('scope', state.scope);
  return `${path}?${params.toString()}`;
}
```

- [ ] **Step 4: 跑测试通过**

Run: `pnpm test:share-web`
Expected: parseHash + serialize 全部 PASS

- [ ] **Step 5: 提交**

```
git add src/share-web/lib/url-state.ts src/share-web/lib/__tests__/url-state.test.ts
git commit -m "feat(file-share/web): url-state parseHash + serialize"
```

---

## Task 9: 前端 — `url-state.ts` push/replace + subscribe

**Files:**
- Modify: `src/share-web/lib/url-state.ts`
- Modify: `src/share-web/lib/__tests__/url-state.test.ts`

- [ ] **Step 1: 写订阅 + 自写自读测试（红）**

把测试文件末尾追加：
```ts
import { pushPath, replacePath, subscribe } from '../url-state';

describe('subscribe', () => {
  beforeEach(() => {
    history.replaceState(null, '', '#');
  });

  it('debounces self-written hashes', () => {
    const calls: string[] = [];
    const off = subscribe((state) => calls.push(state.segments.join('/')));

    pushPath({ segments: ['UMS_TEMP', 'sub'], q: '', scope: 'current' });
    window.dispatchEvent(new HashChangeEvent('hashchange'));

    off();
    expect(calls).toEqual([]);
  });

  it('fires for external hash changes', () => {
    const calls: string[] = [];
    const off = subscribe((state) => calls.push(state.segments.join('/')));

    history.replaceState(null, '', '#/EXTERNAL/dir');
    window.dispatchEvent(new HashChangeEvent('hashchange'));

    off();
    expect(calls).toEqual(['EXTERNAL/dir']);
  });

  it('replacePath does not push history entry', () => {
    const initialLength = history.length;
    replacePath({ segments: ['x'], q: '', scope: 'current' });
    expect(history.length).toBe(initialLength);
  });
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test:share-web`
Expected: subscribe / pushPath / replacePath FAIL（未导出）

- [ ] **Step 3: 实现 push/replace/subscribe**

在 `url-state.ts` 末尾追加：
```ts
let lastWrittenHash: string | null = null;

function writeHash(serialized: string, replace: boolean): void {
  // 永远写非空 url 参数（'#' 表示空 hash），避免 pushState 把整个 URL 清掉
  const url = serialized.length > 0 ? '#' + serialized : '#';
  if (replace) {
    history.replaceState(null, '', url);
  } else {
    history.pushState(null, '', url);
  }
  // 浏览器会把 '#' 规范化成 '' 读出来，记录实际生效的 hash 才能去抖
  lastWrittenHash = location.hash;
}

export function pushPath(state: UrlState): void {
  writeHash(serialize(state), false);
}

export function replacePath(state: UrlState): void {
  writeHash(serialize(state), true);
}

export function subscribe(cb: (state: UrlState) => void): () => void {
  const handler = () => {
    if (location.hash === lastWrittenHash) {
      return;
    }
    lastWrittenHash = location.hash;
    cb(parseHash(location.hash));
  };
  window.addEventListener('popstate', handler);
  window.addEventListener('hashchange', handler);
  return () => {
    window.removeEventListener('popstate', handler);
    window.removeEventListener('hashchange', handler);
  };
}
```

- [ ] **Step 4: 跑全部测试**

Run: `pnpm test:share-web`
Expected: 全部 PASS

- [ ] **Step 5: 提交**

```
git add src/share-web/lib/url-state.ts src/share-web/lib/__tests__/url-state.test.ts
git commit -m "feat(file-share/web): url-state push/replace 与 subscribe 去抖"
```

---

## Task 10: 前端 — `api.ts` 新增 `resolvePath`

**Files:**
- Modify: `src/share-web/api.ts`
- Modify: `src/share-web/types.ts`

- [ ] **Step 1: 在 `types.ts` 追加类型**

在 `FileShareTreeResponse` 接口定义之后（[types.ts:62](src/share-web/types.ts#L62) 附近）追加：
```ts
export interface FileShareResolveResponse {
  node_id: string | null;
  kind: FileShareTreeCurrentKind;
  canonical_segments: string[];
}
```

- [ ] **Step 2: 在 `api.ts` 顶部 import 中加入新类型**

把 [api.ts:1-5](src/share-web/api.ts#L1-L5) 的 import 块改为：
```ts
import type {
  FileShareResolveResponse,
  FileShareSearchResponse,
  FileShareSession,
  FileShareTreeResponse,
} from './types';
```

- [ ] **Step 3: 在 `fileShareApi` 对象中 `getTree` 之后追加 `resolvePath` 方法**

把 [api.ts:79](src/share-web/api.ts#L79) 处的 `},` 紧接其后插入新方法（保持与 `getTree` 同样的 `request<T>` 调用风格）：
```ts
  resolvePath(segments: string[]) {
    const path = segments.map((segment) => encodeURIComponent(segment)).join('/');
    const suffix = path.length > 0 ? `?path=${path}` : '';
    return request<FileShareResolveResponse>(`/api/resolve${suffix}`);
  },
```

插入位置示例（前后上下文）：
```ts
  getTree(nodeId?: string | null) {
    const query = new URLSearchParams();
    if (nodeId) {
      query.set('node_id', nodeId);
    }
    const suffix = query.size > 0 ? `?${query.toString()}` : '';
    return request<FileShareTreeResponse>(`/api/tree${suffix}`);
  },
  resolvePath(segments: string[]) {                       // ← 新增
    const path = segments.map((segment) => encodeURIComponent(segment)).join('/');
    const suffix = path.length > 0 ? `?path=${path}` : '';
    return request<FileShareResolveResponse>(`/api/resolve${suffix}`);
  },
  search(keyword: string, nodeId?: string | null) {
    ...
```

- [ ] **Step 4: 跑前端类型检查**

Run: `pnpm check`
Expected: 无类型错误

- [ ] **Step 5: 提交**

```
git add src/share-web/api.ts src/share-web/types.ts
git commit -m "feat(file-share/web): api.resolvePath 调用 /api/resolve"
```

---

## Task 11: 前端 — i18n 新增 `forbiddenDirectory`

**Files:**
- Modify: `src/share-web/messages.ts`

- [ ] **Step 1: 在 en 块的 `directoryNotFound` 行下追加**

```ts
      forbiddenDirectory: 'You don\'t have access to this directory.',
```

- [ ] **Step 2: 在 zh 块的 `directoryNotFound` 行下追加**

```ts
      forbiddenDirectory: '\u65e0\u6743\u8bbf\u95ee\u8be5\u76ee\u5f55\u3002',
```
（即 "无权访问该目录。"）

- [ ] **Step 3: 跑类型检查**

Run: `pnpm check`
Expected: 通过

- [ ] **Step 4: 提交**

```
git add src/share-web/messages.ts
git commit -m "feat(file-share/web): i18n 新增 forbiddenDirectory"
```

---

## Task 12: 前端 — `App.vue` 集成 URL 同步

**Files:**
- Modify: `src/share-web/App.vue`

- [ ] **Step 1: 添加 import**

`App.vue` `<script setup>` 顶部 import 区追加：
```ts
import { onUnmounted } from 'vue';
import {
  parseHash,
  pushPath,
  replacePath,
  subscribe,
  type UrlState,
} from './lib/url-state';
```
确认 `onMounted` 已 import；如未一起 import，把 `onUnmounted` 合并到现有 vue import 行：
```ts
import { computed, onMounted, onUnmounted, ref, watchEffect, type Ref } from 'vue';
```

- [ ] **Step 2: 改 `loadTree` 签名**

把 `loadTree` 函数签名（约 [App.vue:166](src/share-web/App.vue#L166)）改为：
```ts
async function loadTree(
  nodeId: string | null = null,
  options: {
    preserveSearch?: boolean;
    allowHomeFallback?: boolean;
    urlAction?: 'push' | 'replace' | 'none';
  } = {},
) {
  const preserveSearch = options.preserveSearch ?? false;
  const allowHomeFallback = options.allowHomeFallback ?? true;
  const urlAction = options.urlAction ?? 'replace';
```

在函数尾部 `if (preserveSearch && activeKeyword.value) { await rerunSearch(); }` **之后**追加 URL 写回：
```ts
  syncUrlFromCurrentState(urlAction);
```

并新增辅助函数（紧贴 `loadTree` 之后）：
```ts
function currentUrlState(): UrlState {
  const segments = (tree.value?.breadcrumbs ?? [])
    .filter((crumb) => crumb.node_id !== null)
    .map((crumb) => crumb.label);
  return {
    segments,
    q: activeKeyword.value,
    scope: activeSearchScope.value,
  };
}

function syncUrlFromCurrentState(action: 'push' | 'replace' | 'none') {
  if (action === 'none') {
    return;
  }
  const state = currentUrlState();
  if (action === 'push') {
    pushPath(state);
  } else {
    replacePath(state);
  }
}
```

- [ ] **Step 3: 改 `executeSearch` / `clearSearch` 写 URL**

在 `executeSearch` 函数 try 块的 `searchResults.value = response.results;` **之后**追加：
```ts
    syncUrlFromCurrentState('replace');
```

在 `clearSearch` 函数末尾（`resetSearchState` 调用之后）追加：
```ts
  syncUrlFromCurrentState('replace');
```

- [ ] **Step 4: 改 `navigate` 用 push**

把 `navigate` 函数（[App.vue:354](src/share-web/App.vue#L354)）改为：
```ts
async function navigate(nodeId: string | null) {
  clearSearchWithoutUrl();
  await loadTree(nodeId, { urlAction: 'push' });
}
```

把 `clearSearch` 拆出无 URL 副作用的版本：
```ts
function clearSearchWithoutUrl() {
  resetSearchState(currentKind.value);
}

function clearSearch() {
  clearSearchWithoutUrl();
  syncUrlFromCurrentState('replace');
}
```

并把 `openEntry` 中（[App.vue:338](src/share-web/App.vue#L338)）的：
```ts
  if (node.is_dir) {
    clearSearch();
    await loadTree(node.node_id);
    return;
  }
```
改为：
```ts
  if (node.is_dir) {
    clearSearchWithoutUrl();
    await loadTree(node.node_id, { urlAction: 'push' });
    return;
  }
```

- [ ] **Step 5: 新增 `bootstrapFromUrl` 与 `applyUrlState`**

把现有 `bootstrap` 函数 **整体替换为**：
```ts
async function bootstrap(
  preferredNodeId: string | null = null,
  options: { preserveSearch?: boolean } = {},
) {
  loadingSession.value = true;
  pageError.value = '';

  try {
    session.value = await fileShareApi.getSession();
    loginOpen.value = false;
    loginError.value = '';
    await loadTree(preferredNodeId, {
      preserveSearch: options.preserveSearch ?? false,
      urlAction: 'replace',
    });
  } catch (error) {
    session.value = null;
    tree.value = null;
    searchResults.value = [];
    activeKeyword.value = '';

    if (isUnauthorized(error)) {
      loginOpen.value = true;
      return;
    }
    if (isForbidden(error)) {
      pageError.value = t('app.forbiddenIp');
      return;
    }
    pageError.value = getErrorMessage(error);
  } finally {
    loadingSession.value = false;
  }
}

async function bootstrapFromUrl(state: UrlState) {
  loadingSession.value = true;
  pageError.value = '';

  try {
    session.value = await fileShareApi.getSession();
    loginOpen.value = false;
    loginError.value = '';
    await applyUrlState(state, { skipSession: true });
  } catch (error) {
    session.value = null;
    tree.value = null;
    searchResults.value = [];
    activeKeyword.value = '';

    if (isUnauthorized(error)) {
      loginOpen.value = true;
      return;
    }
    if (isForbidden(error)) {
      pageError.value = t('app.forbiddenIp');
      return;
    }
    pageError.value = getErrorMessage(error);
  } finally {
    loadingSession.value = false;
  }
}

async function applyUrlState(
  state: UrlState,
  options: { skipSession?: boolean } = {},
) {
  if (!options.skipSession) {
    try {
      session.value = await fileShareApi.getSession();
    } catch (error) {
      if (isUnauthorized(error)) {
        loginOpen.value = true;
        return;
      }
      throw error;
    }
  }

  let nodeId: string | null = null;
  let canonicalSegments: string[] = [];

  if (state.segments.length > 0) {
    try {
      const resolved = await fileShareApi.resolvePath(state.segments);
      nodeId = resolved.node_id;
      canonicalSegments = resolved.canonical_segments;
    } catch (error) {
      if (isUnauthorized(error)) {
        loginOpen.value = true;
        return;
      }
      const message = isForbidden(error)
        ? t('app.forbiddenDirectory')
        : t('app.directoryNotFound');
      resetSearchState('home');
      await loadTree(null, { urlAction: 'replace' });
      pageError.value = message;
      return;
    }
  }

  await loadTree(nodeId, { urlAction: 'none' });

  // 还原搜索（若权限允许）
  if (state.q) {
    const scope = state.scope === 'current' && nodeId
      ? 'current'
      : 'global';
    const allowed = scope === 'current'
      ? Boolean(session.value?.permissions.search_current)
      : Boolean(session.value?.permissions.search_global);
    if (allowed) {
      keyword.value = state.q;
      searchScope.value = scope;
      await executeSearch(state.q, scope);
    }
  }

  // canonical 校正：用磁盘真实大小写覆写 URL
  const target: UrlState = {
    segments: state.segments.length > 0 ? canonicalSegments : [],
    q: activeKeyword.value,
    scope: activeSearchScope.value,
  };
  replacePath(target);
}
```

- [ ] **Step 6: 改 onMounted + 加 onUnmounted**

把 `onMounted` 块（[App.vue:548](src/share-web/App.vue#L548)）改为：
```ts
let unsubscribeUrl: (() => void) | null = null;

onMounted(async () => {
  await bootstrapFromUrl(parseHash());
  unsubscribeUrl = subscribe((state) => {
    void applyUrlState(state, { skipSession: true });
  });
});

onUnmounted(() => {
  unsubscribeUrl?.();
});
```

- [ ] **Step 7: 改 `handleLogin` 登录后重 bootstrap**

把 `handleLogin` 内（[App.vue:280](src/share-web/App.vue#L280)）的：
```ts
    await bootstrap(currentNodeId.value, {
      preserveSearch: searchActive.value,
    });
```
改为：
```ts
    await bootstrapFromUrl(parseHash());
```

- [ ] **Step 8: 跑前端类型检查**

Run: `pnpm check`
Expected: 通过

- [ ] **Step 9: 跑前端单测确认未破坏**

Run: `pnpm test:share-web`
Expected: PASS

- [ ] **Step 10: 提交**

```
git add src/share-web/App.vue
git commit -m "feat(file-share/web): App.vue 集成 hash 路由与深链支持"
```

---

## Task 13: 构建验证

**Files:** 无

- [ ] **Step 1: 跑 file-share-web 构建**

Run: `pnpm build:file-share-web`
Expected: 构建成功，`dist/file-share-web/index.html` 与 `dist/file-share-web/assets/` 生成。

- [ ] **Step 2: 跑后端测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare`
Expected: 全部 PASS

- [ ] **Step 3: 跑前端单测**

Run: `pnpm test:share-web`
Expected: 全部 PASS

- [ ] **Step 4: 跑完整 versioned build**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: 编译产物 `src-tauri/target/release/file-sync-tool-1.0.0-YYYYMMDDHHmm.exe` 生成。

如果失败：检查 cargo clippy 是否报新警告（项目可能开了 `deny(warnings)`），按报错处理。

- [ ] **Step 5: 提交（如有产物文件未追踪）**

仅提交源码相关变更，不提交构建产物。Run `git status` 检查。

---

## Task 14: 手动 E2E 测试清单

**Files:** 无

启动应用、开启文件共享，按顺序在浏览器中完成 10 项验证。任一失败需回到对应 Task 修复。

- [ ] **Step 1: 准备**

启动 Tauri 应用 (`pnpm tauri dev` 或运行 release 构建)，进入"文件共享"工具页，确保至少配置两个 root（其中一个名字含中文）并启动服务，记下 URL（如 `http://192.168.x.x:8080`）。

- [ ] **Step 2: 走 10 项手动验证**

1. 直接打开根 URL → home 显示 roots 列表，URL 仍为 `/`（hash 为空）
2. 点进任一 root → URL 变 `#/<root>`，浏览器后退回到 home
3. 进多级目录 → URL 累加段；前进后退按预期切换目录视图
4. 复制深链到无痕窗口（同 IP）→ 直达目标目录
5. 在子目录里搜索 → URL 加 `?q=xxx&scope=current`；刷新仍在目标目录 + 搜索结果
6. 输入不存在路径 `#/不存在` → 回退 home + 顶部错误提示，URL 校正为 `#/`
7. 退出登录 → 再粘贴深链 → 弹 login → 登录后直达目标
8. 路径段带空格 / 中文 / `?` / `#` → percent-encoded 进 URL，回来正确还原
9. 大小写不一致 `#/UMS_TEMP/SUB`（磁盘是 `Sub`） → 加载后 URL 校正成 `#/UMS_TEMP/Sub`
10. 在某目录下切换搜索关键词多次 → 浏览器历史只增加一条（replace 生效，按住后退一次回到上一页面而非上一关键词）

- [ ] **Step 3: 记录手测结果**

如全过 → 任务完成。如有失败项，定位到对应 Task 修复后重跑该项。

---

## 完成标准

- 所有 Task 1-14 步骤勾选完成
- `cmd /c pnpm tauri:build:versioned-exe` 通过
- `pnpm test:share-web` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml fileshare` 通过
- Task 14 手测 10 项全部通过
