# 屏幕共享 Phase 1：生命周期重构 + WGC 默认 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让屏幕共享会话在锁屏/UAC/采集异常/端口占用等所有已知故障下不再死亡：采集失败只暂停并自动恢复，停止后端口确定性释放，WGC 成为默认采集后端，观看页自动感知会话重启与采集暂停。

**Architecture:** 单文件重构 `src-tauri/src/screenshare.rs`（约 3900 行）：① cancel 标志改为 per-session token，杜绝新会话复活旧连接；② 采集后端选择改为纯函数（初始启动严格按用户模式、运行期重建按"存活优先"）并删除整套 DXGI 冲突扫描/强杀机制；③ 捕获循环重建失败不再终止会话，改为无限指数退避重试 + `capture_paused` 状态；④ 停止路径为 graceful drain 加 3 秒上限，超时强制丢弃 listener 释放端口；⑤ 采集创建失败时记录输入桌面状态（锁屏/UAC 定性）；⑥ `/status` 暴露 `session_id` 与 `capture_paused`，观看页与 Vue 端显示"重试中"并自动重连。

**Tech Stack:** Rust (Tauri 2, tokio, axum, scrap, windows 0.58), 内嵌 viewer HTML/JS, Vue 3 + TypeScript 前端。

## Global Constraints

- 目标系统：Windows 10 LTSC 21H2 (build 19044)；WGC `CreateForMonitor` 可用，`IsBorderRequired` **不可用**（不要调用）。
- DXGI 保留为"无边框"选项与降级路径，**不删除**；删除的是冲突扫描/强杀进程机制。
- 提交信息用中文；每个 Task 一个 commit。
- Rust 验证命令统一在仓库根执行：`cargo test --manifest-path src-tauri/Cargo.toml <过滤器>`；前端类型检查 `pnpm check`。
- 收尾必须跑 `cmd /c pnpm tauri:build:versioned-exe` 验证完整构建（约 15 分钟）。
- 所有面向用户的新文案必须同时加 `en`/`zh` 到 `src/locales/messages.ts`。
- 不修改 `ScreenShareConfig` 序列化格式（`capture_backend_mode` 的 `auto`/`wgc`/`dxgi` 值保持不变，仅语义变化）。

---

### Task 1: Per-session cancel token

**Files:**
- Modify: `src-tauri/src/screenshare.rs`（`ScreenShareHandle` 定义 ~L198-230、`clear_runtime_state` ~L274、`screen_share_start` ~L579-661、`screen_share_stop` ~L753、`shutdown_after_capture_failure` ~L368、既有测试 ~L3523-3576）

**Interfaces:**
- Produces: `ScreenShareHandle.cancel: Mutex<Arc<AtomicBool>>`（字段类型变更）；`fn current_cancel_token(handle: &ScreenShareHandle) -> Arc<AtomicBool>`。后续 Task 全部通过 `current_cancel_token` 或启动时捕获的 `session_cancel` 克隆访问 cancel，不再直接 `handle.cancel.clone()`/`store()`。

- [ ] **Step 1: 写失败测试**

在 `screenshare.rs` 的 `mod tests` 中新增：

```rust
    #[test]
    fn new_session_gets_fresh_cancel_token_and_old_token_stays_cancelled() {
        let handle = ScreenShareHandle::new();
        let old_token = current_cancel_token(&handle);

        // 停止/失败路径：当前 token 被取消
        reset_runtime_state(&handle);
        assert!(old_token.load(Ordering::SeqCst));

        // 新会话启动：拿到全新的未取消 token，旧 token 永久保持取消
        prepare_runtime_state_for_start(&handle);
        let new_token = current_cancel_token(&handle);
        assert!(!new_token.load(Ordering::SeqCst));
        assert!(old_token.load(Ordering::SeqCst));
        assert!(!Arc::ptr_eq(&old_token, &new_token));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml new_session_gets_fresh_cancel_token`
Expected: 编译失败 `cannot find function current_cancel_token`（这就是本测试的失败形态）。

- [ ] **Step 3: 实现**

3a. `ScreenShareHandle` 字段与构造（~L198-230）：

```rust
pub struct ScreenShareHandle {
    active: Arc<AtomicBool>,
    starting: AtomicBool,
    /// Current session's cancel token. Each session gets a FRESH Arc so a new
    /// start can never un-cancel streams/threads left over from a previous
    /// session (the old token stays cancelled forever).
    cancel: Mutex<Arc<AtomicBool>>,
    session_id: AtomicU64,
    // ...其余字段不变...
}
```

`new()` 中 `cancel: Arc::new(AtomicBool::new(false))` 改为 `cancel: Mutex::new(Arc::new(AtomicBool::new(false)))`。

3b. 新增 helper（放在 `is_current_session` 旁）：

```rust
fn current_cancel_token(handle: &ScreenShareHandle) -> Arc<AtomicBool> {
    handle.cancel.lock().unwrap().clone()
}
```

3c. `clear_runtime_state`（~L274）中 `handle.cancel.store(cancel, Ordering::SeqCst);` 替换为：

```rust
    {
        let mut token = handle.cancel.lock().unwrap();
        // Cancel whatever session owned this token so its capture thread and
        // MJPEG streams always exit, even if a new session starts right after.
        token.store(true, Ordering::SeqCst);
        if !cancel {
            // Fresh start: install a brand-new, un-cancelled token.
            *token = Arc::new(AtomicBool::new(false));
        }
    }
```

3d. 调用点替换（全部在 screenshare.rs 内）：
- `screen_share_start`：`let capture_cancel = handle.cancel.clone();` → 在 `begin_screen_share_start` 之后加 `let session_cancel = current_cancel_token(handle);`，`capture_cancel` 改为 `session_cancel.clone()`；启动后检查 `handle.cancel.load(...)` 改为 `session_cancel.load(Ordering::SeqCst)`；`HttpServerState` 的 `cancel: handle.cancel.clone()` 改为 `cancel: session_cancel.clone()`。
- `screen_share_stop`：`handle.cancel.store(true, Ordering::SeqCst);` → `current_cancel_token(handle).store(true, Ordering::SeqCst);`
- `shutdown_after_capture_failure`：同上替换（该函数已有 `is_current_session` 守卫，语义不变；本函数将在 Task 3 删除，此处先保持编译通过）。

3e. 更新两个既有测试中直接触碰 `handle.cancel` 的行：
- `prepare_runtime_state_for_start_clears_stale_runtime_state`：`handle.cancel.store(true, ...)` → `current_cancel_token(&handle).store(true, Ordering::SeqCst);`；断言 `!handle.cancel.load(...)` → `assert!(!current_cancel_token(&handle).load(Ordering::SeqCst));`
- `reset_runtime_state_marks_handle_inactive_and_clears_runtime_fields`：`handle.cancel.store(false, ...)` 删除（新 handle 默认就是 false）；断言 `handle.cancel.load(...)` → `assert!(current_cancel_token(&handle).load(Ordering::SeqCst));`

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare`
Expected: 全部 PASS（含新测试与两条更新后的既有测试）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/screenshare.rs
git commit -m "refactor(screenshare): cancel 标志改为每会话独立 token，杜绝新会话复活旧连接占用端口"
```

---

### Task 2: 采集后端选择纯函数化（WGC 默认）+ 删除冲突扫描/强杀机制

**Files:**
- Modify: `src-tauri/src/screenshare.rs`（常量区 ~L78-124、conflict 机制 ~L820-1170、`capture_loop` 的 recreate_mode ~L1735、`create_capture_source` ~L1956-2111、`create_capturer` ~L2570-2707）
- Modify: `src-tauri/src/main.rs`（invoke_handler 中删除 `screenshare::screen_share_scan_conflicts`、`screenshare::screen_share_force_close_conflicts` 两行）
- Modify: `src/lib/tauri.ts`（删除 `ScreenShareConflictProcess`、`ScreenShareConflictCloseResult` 接口与 `screenShareScanConflicts`、`screenShareForceCloseConflicts` 函数——已确认无任何页面引用）
- Modify: `src/locales/messages.ts`（backendMode 文案，en ~L645-651、zh ~L2561-2567）

**Interfaces:**
- Produces: `enum CaptureStartKind { InitialStart, RuntimeRecreate }`；`fn capture_backend_order(mode: ScreenShareBackendMode, kind: CaptureStartKind, current: Option<CaptureBackendKind>) -> Vec<CaptureBackendKind>`；`create_capture_source` 新签名（见下）。Task 3 的重建路径将以 `CaptureStartKind::RuntimeRecreate` + `Some(current_backend)` 调用它。
- Consumes: Task 1 的 `session_cancel`（`create_capturer` 参数 `cancel: &AtomicBool` 不变，传入的是 per-session token）。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn auto_mode_tries_wgc_before_dxgi_on_initial_start() {
        assert_eq!(
            capture_backend_order(ScreenShareBackendMode::Auto, CaptureStartKind::InitialStart, None),
            vec![CaptureBackendKind::Wgc, CaptureBackendKind::Dxgi]
        );
    }

    #[test]
    fn explicit_modes_are_strict_on_initial_start() {
        assert_eq!(
            capture_backend_order(ScreenShareBackendMode::Dxgi, CaptureStartKind::InitialStart, None),
            vec![CaptureBackendKind::Dxgi]
        );
        assert_eq!(
            capture_backend_order(ScreenShareBackendMode::Wgc, CaptureStartKind::InitialStart, None),
            vec![CaptureBackendKind::Wgc]
        );
    }

    #[test]
    fn runtime_recreate_prefers_current_backend_then_survival_fallback() {
        // 运行中重建：先试当前存活过的后端，另一个作为保命降级——即使用户显式选了 DXGI
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Dxgi,
                CaptureStartKind::RuntimeRecreate,
                Some(CaptureBackendKind::Dxgi)
            ),
            vec![CaptureBackendKind::Dxgi, CaptureBackendKind::Wgc]
        );
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Auto,
                CaptureStartKind::RuntimeRecreate,
                Some(CaptureBackendKind::Wgc)
            ),
            vec![CaptureBackendKind::Wgc, CaptureBackendKind::Dxgi]
        );
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml capture_backend_order -- --list`
Expected: 编译失败 `cannot find function capture_backend_order`。

- [ ] **Step 3: 实现选择函数与 create_capture_source 重构**

3a. 常量区：删除 `CAPTURE_CREATE_RETRY_DELAYS_MS`（8 档）与 `CAPTURE_AUTO_DXGI_RETRY_DELAYS_MS`、`capture_retry_delays_for_mode`，新增：

```rust
/// DXGI DuplicateOutput 偶发瞬时失败，创建时做 3 次短重试；
/// 长退避由捕获循环的暂停-重试机制负责（Task 3），此处不再需要 8 档梯子。
const DXGI_CREATE_RETRY_DELAYS_MS: [u64; 3] = [0, 200, 400];
```

3b. 新增类型与纯函数：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureStartKind {
    InitialStart,
    RuntimeRecreate,
}

/// 决定采集后端尝试顺序。
/// - 初始启动：严格尊重用户选择（显式 DXGI 表示"要无边框"，失败就报错，不悄悄换成有黄框的 WGC）。
///   Auto 模式 WGC 优先——它无独占语义、不被锁屏杀死，是稳定性默认。
/// - 运行期重建：保命优先，先试刚才还活着的后端，另一个作为降级，绝不因单后端失败而停止共享。
fn capture_backend_order(
    mode: ScreenShareBackendMode,
    kind: CaptureStartKind,
    current: Option<CaptureBackendKind>,
) -> Vec<CaptureBackendKind> {
    match kind {
        CaptureStartKind::InitialStart => match mode {
            ScreenShareBackendMode::Auto => vec![CaptureBackendKind::Wgc, CaptureBackendKind::Dxgi],
            ScreenShareBackendMode::Wgc => vec![CaptureBackendKind::Wgc],
            ScreenShareBackendMode::Dxgi => vec![CaptureBackendKind::Dxgi],
        },
        CaptureStartKind::RuntimeRecreate => {
            let first = current.unwrap_or(match mode {
                ScreenShareBackendMode::Dxgi => CaptureBackendKind::Dxgi,
                _ => CaptureBackendKind::Wgc,
            });
            let second = match first {
                CaptureBackendKind::Wgc => CaptureBackendKind::Dxgi,
                CaptureBackendKind::Dxgi => CaptureBackendKind::Wgc,
            };
            vec![first, second]
        }
    }
}
```

3c. `create_capture_source` 改为按顺序迭代（替换现有 ~L1956-2111 的整个 match 结构）：

```rust
fn create_capture_source(
    monitor_index: usize,
    show_cursor: bool,
    backend_mode: ScreenShareBackendMode,
    start_kind: CaptureStartKind,
    current_backend: Option<CaptureBackendKind>,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
    app_handle: &AppHandle,
) -> Result<CaptureSource, String> {
    let order = capture_backend_order(backend_mode, start_kind, current_backend);
    let mut failures: Vec<String> = Vec::new();

    for (index, backend) in order.iter().enumerate() {
        let result = match backend {
            CaptureBackendKind::Dxgi => create_capturer(
                monitor_index,
                cancel,
                runtime_handle,
                session_id,
                app_handle,
            )
            .map(CaptureSource::Dxgi),
            #[cfg(target_os = "windows")]
            CaptureBackendKind::Wgc => {
                create_wgc_capturer(monitor_index, show_cursor, session_id, app_handle)
                    .map(CaptureSource::Wgc)
            }
            #[cfg(not(target_os = "windows"))]
            CaptureBackendKind::Wgc => {
                Err("WGC capture backend is only available on Windows".to_string())
            }
        };

        match result {
            Ok(source) => {
                emit_capture_create_diagnostic(
                    app_handle,
                    "success",
                    format_capture_backend_selected_message(
                        source.backend_kind(),
                        session_id,
                        monitor_index,
                        source.width(),
                        source.height(),
                    ),
                );
                return Ok(source);
            }
            Err(error) => {
                let has_next = index + 1 < order.len();
                if has_next {
                    emit_capture_create_diagnostic(
                        app_handle,
                        "warn",
                        format_capture_backend_fallback_message(
                            *backend,
                            order[index + 1],
                            session_id,
                            monitor_index,
                            &error,
                        ),
                    );
                } else {
                    emit_capture_create_diagnostic(
                        app_handle,
                        "error",
                        format_capture_backend_failure_message(
                            *backend,
                            session_id,
                            monitor_index,
                            &error,
                        ),
                    );
                }
                failures.push(format!("{}: {}", backend.label(), error));
            }
        }

        if cancel.load(Ordering::Relaxed) || !is_current_session(runtime_handle, session_id) {
            return Err("screen capture init cancelled".to_string());
        }
    }

    Err(failures.join("; "))
}
```

注意：`#[cfg(not(target_os = "windows"))]` 分支里非 Windows 的 DXGI 路径保持可编译（`create_capturer` 本身跨平台，scrap 提供）。

3d. `create_capturer` 修改（~L2570）：
- 删除参数 `backend_mode`，内部 `retry_delays` 固定为 `&DXGI_CREATE_RETRY_DELAYS_MS`；
- 删除"blocking conflict 短路"块（`if retryable { if let Some(conflict_error) = capture_blocking_conflict_error_snapshot() { return Err(...) } }`）；
- 日志行中的 `capture_conflict_diagnostics_snapshot()` 调用整体删除（Task 5 会补 desktop 状态字段）；
- `capture_creation_hint` 文案改为：`"possible cause: the desktop is on the lock/UAC secure desktop, or the session is disconnected"`（删除 ScreenTask/关闭工具的误导性提示）。

3e. `capture_loop` 两个调用点适配新签名：
- 初始创建（~L1553）：`create_capture_source(monitor_index, show_cursor, backend_mode, CaptureStartKind::InitialStart, None, &cancel, &runtime_handle, session_id, &app_handle)`；
- 重建（~L1741）：删除 `recreate_mode` 计算块，改为 `create_capture_source(monitor_index, show_cursor, backend_mode, CaptureStartKind::RuntimeRecreate, Some(current_backend), &cancel, &runtime_handle, session_id, &app_handle)`。

3f. 删除整套冲突机制（screenshare.rs）：
- 类型：`ScreenShareConflictProcess`、`ScreenShareConflictCloseResult`、`ScreenCaptureConflictPolicy`
- 函数：`normalize_process_name`、`screen_capture_conflict_policy`、`utf16_cstr_to_string`、`collect_window_titles_by_pid`、`scan_screen_share_conflicts`（两个 cfg 版本）、`blocking_capture_conflicts`、`format_capture_conflict_summary`、`capture_blocking_conflict_error`、`format_capture_conflict_diagnostics`、`capture_conflict_diagnostics_snapshot`、`capture_blocking_conflict_error_snapshot`、`force_close_screen_share_conflicts_inner`（两个 cfg 版本）
- Commands：`screen_share_scan_conflicts`、`screen_share_force_close_conflicts`
- 相应删掉不再使用的 imports：`CreateToolhelp32Snapshot/Process32FirstW/Process32NextW/PROCESSENTRY32W/TH32CS_SNAPPROCESS`、`EnumWindows/GetWindowTextLengthW/GetWindowTextW/GetWindowThreadProcessId/IsWindowVisible`、`std::process::Command`（若仅冲突强杀在用）、`HashSet`（若仅冲突代码在用）。以 `cargo check` 的 unused import 警告为准清理。
- 若 tests 模块存在引用上述符号的测试（用 `conflict` 关键词搜索），一并删除。
- 同时删除依赖旧重试梯子的 `#[cfg(test)]` 辅助函数 `capture_create_retry_window`、`capture_create_retry_window_for_mode`（~L1836-1852）及引用它们的既有测试（如 `automatic_backend_mode_uses_shorter_dxgi_retry_window_than_dxgi_only`，用 `retry_window` 关键词搜索）——它们测的是被移除的 8 档重试行为，已被本 Task 的 `capture_backend_order` 测试取代。

3g. `main.rs` invoke_handler 删除两行：`screenshare::screen_share_scan_conflicts,` 与 `screenshare::screen_share_force_close_conflicts,`。

3h. `src/lib/tauri.ts` 删除 L1046-1079 区间的两个 interface 与两个函数（精确边界以当前文件为准）。

3i. `src/locales/messages.ts` 文案更新（en L645-651）：

```ts
        backendModeHint: 'Auto is recommended. Pick DXGI only if the yellow capture border bothers you.',
        backendModeAuto: 'Auto (Recommended)',
        backendModeAutoDesc: 'Starts with WGC — the most stable backend (survives lock screen, no conflicts with DingTalk etc.). Falls back to DXGI if WGC is unavailable.',
        backendModeWgc: 'WGC Only',
        backendModeWgcDesc: 'Windows Graphics Capture only. Note: Windows draws a yellow border around the captured screen.',
        backendModeDxgi: 'DXGI (Borderless)',
        backendModeDxgiDesc: 'No yellow capture border, but less resilient: killed by the lock screen and conflicts with other capture apps. If the stream breaks mid-session it will temporarily switch to WGC to keep the share alive.',
```

zh（L2561-2567）：

```ts
        backendModeHint: '默认自动即可；仅当介意屏幕四周的系统采集黄框时选 DXGI。',
        backendModeAuto: '自动（推荐）',
        backendModeAutoDesc: '优先使用 WGC——最稳定的采集后端（锁屏不掉、与钉钉等共存），不可用时自动回退 DXGI。',
        backendModeWgc: '仅 WGC',
        backendModeWgcDesc: '只用 Windows Graphics Capture。注意：系统会在被采集屏幕四周显示黄色边框。',
        backendModeDxgi: 'DXGI（无边框）',
        backendModeDxgiDesc: '画面无黄色采集边框，但稳定性较差：会被锁屏中断、与其他采集软件冲突。运行中断流时会临时切换 WGC 保住共享不中断。',
```

- [ ] **Step 4: 运行测试与类型检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare` → 全部 PASS
Run: `pnpm check` → 无错误（确认 tauri.ts 删除无遗留引用）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/screenshare.rs src-tauri/src/main.rs src/lib/tauri.ts src/locales/messages.ts
git commit -m "feat(screenshare): WGC 成为默认采集后端，重建保命降级；删除冲突扫描与强杀进程机制"
```

---

### Task 3: 捕获循环不死——暂停 + 无限退避重试 + capture_paused 状态

**Files:**
- Modify: `src-tauri/src/screenshare.rs`（`ScreenShareHandle` 增字段、`ScreenShareStatus` 增字段、`capture_loop` Err 分支 ~L1700-1789、`shutdown_after_capture_failure` 删除、`inactive_status`/`screen_share_get_status`/`status_reporter`/`clear_runtime_state`）
- Modify: `src/lib/tauri.ts`（`ScreenShareStatus` 接口）

**Interfaces:**
- Consumes: Task 2 的 `create_capture_source(..., CaptureStartKind::RuntimeRecreate, Some(current_backend), ...)`。
- Produces: `ScreenShareHandle.capture_paused: Arc<AtomicBool>`；`ScreenShareStatus.capture_paused: bool`（serde 序列化字段名 `capture_paused`）；`fn capture_recreate_backoff(attempt: u32) -> Duration`。Task 6 的 `/status` 与前端将读取 `capture_paused`。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn capture_recreate_backoff_grows_and_caps_at_30s() {
        assert_eq!(capture_recreate_backoff(0), Duration::from_millis(1000));
        assert_eq!(capture_recreate_backoff(1), Duration::from_millis(2000));
        assert_eq!(capture_recreate_backoff(2), Duration::from_millis(4000));
        assert_eq!(capture_recreate_backoff(5), Duration::from_millis(30000));
        assert_eq!(capture_recreate_backoff(100), Duration::from_millis(30000));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml capture_recreate_backoff`
Expected: 编译失败 `cannot find function capture_recreate_backoff`。

- [ ] **Step 3: 实现**

3a. 常量 + 纯函数：

```rust
/// 采集器重建的无限重试退避表；到顶后维持 30s 间隔直到会话被取消。
/// 锁屏可能持续数小时——共享必须活着等到解锁自动恢复。
const CAPTURE_RECREATE_BACKOFF_MS: [u64; 6] = [1000, 2000, 4000, 8000, 15000, 30000];

fn capture_recreate_backoff(attempt: u32) -> Duration {
    let index = (attempt as usize).min(CAPTURE_RECREATE_BACKOFF_MS.len() - 1);
    Duration::from_millis(CAPTURE_RECREATE_BACKOFF_MS[index])
}
```

3b. `ScreenShareHandle` 增字段 `capture_paused: Arc<AtomicBool>`（`new()` 中 `Arc::new(AtomicBool::new(false))`）；`clear_runtime_state` 中加 `handle.capture_paused.store(false, Ordering::SeqCst);`

3c. `ScreenShareStatus` 增 `pub capture_paused: bool,`：
- `inactive_status()` → `capture_paused: false,`
- `screen_share_get_status` → `capture_paused: handle.capture_paused.load(Ordering::Relaxed),`
- `status_reporter`：函数签名增加参数 `capture_paused: Arc<AtomicBool>`（在 `screen_share_start` 传 `handle.capture_paused.clone()`），构造 status 时 `capture_paused: capture_paused.load(Ordering::Relaxed),`

3d. `capture_loop` 的 `Err(e)` 分支整体替换为（保留原有第一条"捕获循环异常"日志，去掉 recreate_attempts 一次性语义）：

```rust
            Err(e) => {
                let capture_error_detail = format!(
                    "捕获循环异常，进入暂停重试: monitor_index={}, viewers={}, first_real_frame={}, error_kind={:?}, error={}",
                    monitor_index,
                    viewer_count.load(Ordering::Relaxed),
                    first_real_frame,
                    e.kind(),
                    e
                );
                log::warn!("{}", capture_error_detail);
                crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &capture_error_detail, "warn");
                let _ = app_handle.emit(
                    "screen-share-log",
                    serde_json::json!({ "level": "warn", "message": capture_error_detail }),
                );

                if is_current_session(&runtime_handle, session_id) {
                    runtime_handle.capture_paused.store(true, Ordering::SeqCst);
                }
                drop(source);

                let mut retry_attempt = 0u32;
                let recovered = loop {
                    if !wait_for_capture_retry_delay(
                        capture_recreate_backoff(retry_attempt),
                        &cancel,
                        &runtime_handle,
                        session_id,
                    ) {
                        break None;
                    }
                    match create_capture_source(
                        monitor_index,
                        show_cursor,
                        backend_mode,
                        CaptureStartKind::RuntimeRecreate,
                        Some(current_backend),
                        &cancel,
                        &runtime_handle,
                        session_id,
                        &app_handle,
                    ) {
                        Ok(new_source) => break Some(new_source),
                        Err(err) => {
                            retry_attempt = retry_attempt.saturating_add(1);
                            let retry_msg = format!(
                                "屏幕捕获器重建失败，{}s 后继续重试: attempt={}, monitor_index={}, viewers={}, cause={}",
                                capture_recreate_backoff(retry_attempt).as_secs(),
                                retry_attempt,
                                monitor_index,
                                viewer_count.load(Ordering::Relaxed),
                                err
                            );
                            log::warn!("{}", retry_msg);
                            crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &retry_msg, "warn");
                            let _ = app_handle.emit(
                                "screen-share-log",
                                serde_json::json!({ "level": "warn", "message": retry_msg }),
                            );
                        }
                    }
                };

                match recovered {
                    Some(new_source) => {
                        source = new_source;
                        if is_current_session(&runtime_handle, session_id) {
                            runtime_handle.capture_paused.store(false, Ordering::SeqCst);
                        }
                        let resumed_msg = format!(
                            "屏幕捕获已恢复: retries={}, monitor_index={}, backend={}",
                            retry_attempt,
                            monitor_index,
                            source.backend_kind().label()
                        );
                        log::info!("{}", resumed_msg);
                        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &resumed_msg, "success");
                        let _ = app_handle.emit(
                            "screen-share-log",
                            serde_json::json!({ "level": "info", "message": resumed_msg }),
                        );
                        first_real_frame = false;
                        continue;
                    }
                    None => break, // 会话被取消，正常退出
                }
            }
```

同时删除循环前的 `let mut recreate_attempts = 0u32;`。

3e. 删除 `shutdown_after_capture_failure` 函数，并删除 `capture_loop` 初始化失败路径中对它的调用（初始失败只走 `startup_tx.send(Err(...))`；`else` 分支已不可达，简化为仅 send）：

```rust
        Err(err) => {
            let detail = format!(
                "屏幕捕获初始化失败: monitor_index={}, viewers={}, cause={}",
                monitor_index,
                viewer_count.load(Ordering::Relaxed),
                err
            );
            if let Some(tx) = startup_tx.take() {
                let _ = tx.send(Err(detail));
            } else {
                log::error!("{}", detail);
            }
            return;
        }
```

3f. `src/lib/tauri.ts` 的 `ScreenShareStatus` 接口增加 `capture_paused: boolean;`

- [ ] **Step 4: 运行测试与类型检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare` → PASS
Run: `pnpm check` → 无错误

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/screenshare.rs src/lib/tauri.ts
git commit -m "feat(screenshare): 捕获失败不再终止共享，改为暂停+无限退避重试，新增 capture_paused 状态"
```

---

### Task 4: 停止路径确定性释放端口（drain 3s 上限 + 等待真实关闭）

**Files:**
- Modify: `src-tauri/src/screenshare.rs`（`ScreenShareHandle` 增字段、`run_http_server` ~L2854、`screen_share_start` 的 watcher 任务 ~L669-701、`screen_share_stop` ~L753-785、`clear_runtime_state`）

**Interfaces:**
- Produces: `ScreenShareHandle.server_done_rx: Mutex<Option<oneshot::Receiver<()>>>`；常量 `SERVER_DRAIN_DEADLINE: Duration`（3s）、`STOP_WAIT_TIMEOUT: Duration`（4s）。
- Consumes: Task 1 的 per-session cancel（drain 期间旧流靠自己 token 退出）。

- [ ] **Step 1: 实现 run_http_server 的 drain 上限**

（本任务核心是异步 IO 行为，无法脱离 AppHandle 单测；以 Step 3 的手工验证为准，先实现。）

```rust
/// graceful shutdown 后允许连接 drain 的最长时间；超时直接丢弃 serve future
/// （连带 listener），确保端口一定释放——半死的 viewer 连接不能扣住端口。
const SERVER_DRAIN_DEADLINE: Duration = Duration::from_secs(3);
/// screen_share_stop 等待服务真正退出的上限（略大于 drain 上限）。
const STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(4);

async fn run_http_server(
    listener: tokio::net::TcpListener,
    state: Arc<HttpServerState>,
    shutdown_rx: oneshot::Receiver<()>,
) {
    let app = Router::new()
        .route("/", get(handler_index))
        .route("/stream", get(handler_stream))
        .route("/auth", post(handler_auth))
        .route("/status", get(handler_status))
        .with_state(state);

    let (drain_started_tx, drain_started_rx) = oneshot::channel::<()>();
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_rx.await.ok();
        let _ = drain_started_tx.send(());
    });

    tokio::select! {
        result = serve => {
            if let Err(e) = result {
                log::error!("Screen share HTTP server error: {}", e);
            }
        }
        _ = async {
            drain_started_rx.await.ok();
            tokio::time::sleep(SERVER_DRAIN_DEADLINE).await;
        } => {
            // 超时分支：select 丢弃 serve future → listener 关闭 → 端口立即可复用。
            log::warn!(
                "Screen share drain deadline ({}s) exceeded; forcing listener close",
                SERVER_DRAIN_DEADLINE.as_secs()
            );
        }
    }

    log::info!("Screen share HTTP server stopped");
}
```

- [ ] **Step 2: 打通"停止等待真实关闭"**

2a. `ScreenShareHandle` 增字段 `server_done_rx: Mutex<Option<oneshot::Receiver<()>>>`（`new()` 中 `Mutex::new(None)`）；`clear_runtime_state` 中加 `*handle.server_done_rx.lock().unwrap() = None;`

2b. `screen_share_start` 中，watcher 任务处（`let server_join = tokio::spawn(...)` 之后）：

```rust
    let (server_done_tx, server_done_rx) = oneshot::channel::<()>();
    *handle.server_done_rx.lock().unwrap() = Some(server_done_rx);
```

watcher 闭包末尾（原有清理逻辑之后）追加 `let _ = server_done_tx.send(());`（把 `server_done_tx` move 进 watcher）。

2c. `screen_share_stop` 中，将 `tokio::time::sleep(Duration::from_millis(1200)).await;` 替换为：

```rust
    // 等待 HTTP 服务真正退出（graceful 或 drain 超时强制关闭），
    // 返回后端口保证已释放，前端可立即用同端口重启。
    let done_rx = handle.server_done_rx.lock().unwrap().take();
    if let Some(done_rx) = done_rx {
        let _ = tokio::time::timeout(STOP_WAIT_TIMEOUT, done_rx).await;
    }
```

注意借用：`done_rx` 先取出再 await（不能在持有 MutexGuard 时 await）。

2d. 既有测试 `prepare_runtime_state_for_start_clears_stale_runtime_state` 与 `reset_runtime_state_...` 增加字段设置与断言：

```rust
        *handle.server_done_rx.lock().unwrap() = Some(oneshot::channel::<()>().1);
        // ...调用被测函数后...
        assert!(handle.server_done_rx.lock().unwrap().is_none());
```

- [ ] **Step 3: 编译 + 手工验证端口立即复用**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare` → PASS

手工验证（`pnpm tauri dev` 启动应用）：
1. 屏幕共享页启动共享（端口 9870）；
2. 另开 PowerShell 挂一个不读数据的假观看者：`$c = New-Object Net.Sockets.TcpClient('127.0.0.1',9870); $s=$c.GetStream(); $w=New-Object IO.StreamWriter($s); $w.Write("GET /stream HTTP/1.1`r`nHost: x`r`n`r`n"); $w.Flush()`（保持不关）；
3. 应用内点停止 → 立即再点启动（同端口 9870）。
Expected: 第二次启动成功，无 10048；停止操作最多阻塞 ~4 秒；app.log 出现 `drain deadline ... exceeded` 或正常 graceful 退出。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/screenshare.rs
git commit -m "fix(screenshare): 停止路径 drain 3 秒上限+强制关闭 listener，停止返回即端口可复用"
```

---

### Task 5: 采集失败时记录输入桌面状态（锁屏/UAC 定性诊断）

**Files:**
- Modify: `src-tauri/Cargo.toml`（windows features 增 `"Win32_System_StationsAndDesktops"`）
- Modify: `src-tauri/src/screenshare.rs`（新增 `describe_input_desktop`、接入 `create_capturer`/`create_wgc_capturer` 失败日志与 `capture_loop` 异常日志）

**Interfaces:**
- Produces: `fn describe_input_desktop() -> String`，返回形如 `input_desktop=Default` / `input_desktop=Winlogon`（锁屏/UAC）/ `input_desktop=unavailable(...)`。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn describe_input_desktop_reports_a_desktop_state() {
        let described = describe_input_desktop();
        assert!(described.starts_with("input_desktop="), "got: {described}");
        assert!(described.len() > "input_desktop=".len(), "got: {described}");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml describe_input_desktop`
Expected: 编译失败 `cannot find function describe_input_desktop`。

- [ ] **Step 3: 实现**

3a. `src-tauri/Cargo.toml` windows features 列表加一行 `"Win32_System_StationsAndDesktops",`。

3b. screenshare.rs 实现（cfg 双版本）：

```rust
/// 报告当前输入桌面：交互桌面为 "Default"；锁屏/UAC 期间为 "Winlogon" 或直接打不开。
/// 用于在采集创建失败时一条日志区分"锁屏/安全桌面"与"真实的采集冲突"。
#[cfg(target_os = "windows")]
fn describe_input_desktop() -> String {
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetUserObjectInformationW, UOI_NAME};

    unsafe {
        let desktop = match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)
        {
            Ok(desktop) => desktop,
            Err(error) => {
                return format!(
                    "input_desktop=unavailable(likely lock screen or UAC secure desktop, error={})",
                    sanitize_log_field(&error.message())
                );
            }
        };

        let mut name_buf = [0u16; 128];
        let mut needed = 0u32;
        let name = if GetUserObjectInformationW(
            windows::Win32::Foundation::HANDLE(desktop.0),
            UOI_NAME,
            Some(name_buf.as_mut_ptr() as *mut _),
            (name_buf.len() * 2) as u32,
            Some(&mut needed),
        )
        .is_ok()
        {
            let len = name_buf.iter().position(|c| *c == 0).unwrap_or(name_buf.len());
            String::from_utf16_lossy(&name_buf[..len])
        } else {
            "unknown".to_string()
        };
        let _ = CloseDesktop(desktop);
        format!("input_desktop={}", name)
    }
}

#[cfg(not(target_os = "windows"))]
fn describe_input_desktop() -> String {
    "input_desktop=n/a".to_string()
}
```

（若 windows 0.58 中 `OpenInputDesktop`/`GetUserObjectInformationW` 签名与上述有出入，以 `cargo check` 报错为准就地修正——参数语义不变。）

3c. 接入日志（原 conflict_scan 字段的位置）：
- `create_capturer` 的"屏幕捕获器创建开始"与"创建失败"日志 format! 末尾追加 `, {}` / `describe_input_desktop()`；
- `capture_loop` Err 分支的 `capture_error_detail` 追加同字段；
- `create_wgc_capturer` 失败路径（`create_capture_source` 中 WGC Err 分支的 fallback/failure 消息）无需改动——`format_capture_backend_failure_message` 的 cause 已含错误详情，仅在 `capture_loop` 与 `create_capturer` 层面补桌面状态即可。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml describe_input_desktop`
Expected: PASS（本地交互会话应返回 `input_desktop=Default`）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/screenshare.rs
git commit -m "feat(screenshare): 采集失败日志附带输入桌面状态，一条日志区分锁屏/UAC 与真实冲突"
```

---

### Task 6: /status 会话纪元 + 观看页与 Vue 端的"采集重试中"显示

**Files:**
- Modify: `src-tauri/src/screenshare.rs`（`HttpServerState` 增字段、`handler_status`、`viewer_html` 的 JS）
- Modify: `src/pages/ScreenSharePage.vue`（状态区显示 capture_paused 徽标）
- Modify: `src/locales/messages.ts`（新增 `tools.screenShare.capturePaused` en/zh）

**Interfaces:**
- Consumes: Task 3 的 `handle.capture_paused`、`ScreenShareStatus.capture_paused`。
- Produces: `/status` JSON 新增字段 `session_id: u64`、`capture_paused: bool`（浏览器端依赖）。

- [ ] **Step 1: 服务端 /status 扩展**

1a. `HttpServerState` 增字段：

```rust
    session_id: u64,
    capture_paused: Arc<AtomicBool>,
```

`screen_share_start` 构造处传 `session_id,` 与 `capture_paused: handle.capture_paused.clone(),`。

1b. `handler_status` 返回体改为：

```rust
    Json(serde_json::json!({
        "active": !state.cancel.load(Ordering::Relaxed),
        "viewers": state.viewer_count.load(Ordering::Relaxed),
        "session_id": state.session_id,
        "capture_paused": state.capture_paused.load(Ordering::Relaxed),
    }))
```

- [ ] **Step 2: 观看页 JS**

在 `viewer_html` 的 `<script>` 中：

2a. `T` 表增加一项：

```js
  serverRetrying:isZh?'画面中断，服务端自动重试中':'Capture interrupted — server is retrying',
```

2b. 心跳回调中（`if(r.ok){ const d=await r.json(); ...}` 内、`heartbeatFails=0;` 之后）加入会话纪元与采集暂停处理：

```js
      // 会话纪元变化 = 服务端重启过共享（旧流已死但 TCP 可能还挂着）→ 主动重连
      if(typeof d.session_id!=='undefined'){
        if(window.__ssSession!==undefined&&window.__ssSession!==d.session_id&&!paused){
          window.__ssSession=d.session_id;
          holdCurrentFrame();
          tryReconnect();
        } else {
          window.__ssSession=d.session_id;
        }
      }
      // 服务端采集暂停（锁屏等）→ 显示重试提示；恢复后自动隐藏
      const captureRetryEl=document.getElementById('captureRetry');
      if(captureRetryEl){
        captureRetryEl.style.display=(d.capture_paused&&!paused)?'flex':'none';
      }
```

2c. HTML 中 `paused-overlay` div 之后新增一个不遮挡画面的顶部提示条：

```html
    <div id="captureRetry" style="display:none;position:absolute;top:12px;left:50%;transform:translateX(-50%);z-index:6;align-items:center;gap:8px;background:rgba(245,158,11,.15);border:1px solid rgba(245,158,11,.35);color:#fbbf24;padding:8px 16px;border-radius:10px;font-size:13px;font-weight:500;backdrop-filter:blur(6px)">
      <span class="dot dot-retry"></span><span id="captureRetryText"></span>
    </div>
```

并在 i18n 应用区加 `document.getElementById('captureRetryText').textContent=T.serverRetrying;`

- [ ] **Step 3: Vue 端徽标**

3a. `messages.ts` 增加（en 的 `tools.screenShare` 区）：`capturePaused: 'Capture interrupted — auto-retrying',`；zh 区：`capturePaused: '画面中断，自动重试中',`

3b. `ScreenSharePage.vue`：页面已订阅 `screen-share-status`（状态对象含新字段 `capture_paused`）。在运行状态显示区域（状态徽标/URL 展示附近，执行时以实际模板为准）加：

```html
<span v-if="status?.capture_paused" class="inline-flex items-center gap-1 rounded-full bg-amber-50 border border-amber-200 px-2 py-0.5 text-[11px] text-amber-600">
  {{ t('tools.screenShare.capturePaused') }}
</span>
```

（`status` 为页面既有的 ScreenShareStatus ref；若变量名不同，跟随现有代码。）

- [ ] **Step 4: 验证**

Run: `cargo test --manifest-path src-tauri/Cargo.toml screenshare` → PASS
Run: `pnpm check` → 无错误
手工验证（`pnpm tauri dev`）：
1. 启动共享（Auto 模式），浏览器打开观看页 → 正常显示；
2. `Win+L` 锁屏 5~10 秒后解锁 → 观看页出现"画面中断，服务端自动重试中"提示条后自动消失，画面恢复，**共享全程未停止**；应用日志有 `捕获循环异常，进入暂停重试` 与 `屏幕捕获已恢复`，且失败日志含 `input_desktop=` 字段；
3. 应用内停止再启动 → 观看页无需手动刷新，自动恢复画面（session_id 纪元变化触发重连）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/screenshare.rs src/pages/ScreenSharePage.vue src/locales/messages.ts
git commit -m "feat(screenshare): /status 暴露会话纪元与采集暂停状态，观看页与主界面自动显示重试中并重连"
```

---

### Task 7: 收尾——多观看者冒烟 + 完整构建验证

**Files:**
- 无新改动；仅验证与（如有）修复。

- [ ] **Step 1: 全量 Rust 测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全部 PASS。

- [ ] **Step 2: 50 观看者冒烟**

`pnpm tauri dev` 启动共享后：

```powershell
1..50 | ForEach-Object { Start-Job -ScriptBlock { curl.exe -s -o NUL --max-time 20 "http://127.0.0.1:9870/stream" } } | Out-Null
Start-Sleep 5
curl.exe -s http://127.0.0.1:9870/status
Get-Job | Remove-Job -Force
```

Expected: `/status` 的 `viewers` 接近 50；应用不卡顿；20 秒后连接自然结束、viewer 数回落。

- [ ] **Step 3: 完整构建**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: exit 0，产出 `file-sync-tool-*.exe` 并更新 manifest。

- [ ] **Step 4: 收尾提交（如 Step 1-3 有修复）**

```bash
git add -A -- src-tauri src
git commit -m "fix(screenshare): Phase1 收尾修复"
```

---

## Phase 2/3 预告（不在本计划内，进入时另写计划）

- **Phase 2**：Media Foundation 硬件 H.264（硬→软→MJPEG 三级探测降级）+ WebSocket/fMP4/MSE 播放器 + 自适应降档 —— 满足 50 人接入的带宽要求。
- **Phase 3**：WebRTC（webrtc-rs，单编码共享 Track 广播、PLI→IDR）+ 播放器三层自动协商（WebRTC→MSE→MJPEG）。
