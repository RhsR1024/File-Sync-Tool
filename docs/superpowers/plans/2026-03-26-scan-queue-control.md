# Scan Queue Control — Skip / Remove / Per-Task Cancel

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow users to skip/cancel individual copy tasks during a scheduled scan without stopping the entire scan, and remove queued tasks before they start copying.

**Architecture:** Add a `should_skip_current` AtomicBool to AppState that cancels only the current copy task while allowing the scan loop to continue to the next folder. Modify `scan_and_copy` to collect all candidate folders first, emit them as "queued" events, then process each with skip/removal checks. Add new Tauri commands and frontend UI buttons.

**Tech Stack:** Rust (Tauri 2.x, AtomicBool, Arc<Mutex>), Vue 3 (Composition API), TypeScript, Tailwind CSS, vue-i18n

---

### Task 1: Add `should_skip_current` and `scan_queue_removals` to AppState

**Files:**
- Modify: `src-tauri/src/main.rs:32-42` (AppState struct)
- Modify: `src-tauri/src/main.rs:1428-1438` (AppState initialization)

- [ ] **Step 1: Add new fields to AppState struct**

In `src-tauri/src/main.rs`, add two fields to the `AppState` struct:

```rust
struct AppState {
    config: Arc<Mutex<AppConfig>>,
    is_scanning: Arc<AtomicBool>,
    is_manually_deploying: Arc<AtomicBool>,
    manual_copy_queue: Arc<Mutex<VecDeque<ManualCopyQueueItem>>>,
    manual_copy_keys: Arc<Mutex<HashSet<String>>>,
    manual_copy_worker_running: Arc<AtomicBool>,
    should_cancel: Arc<AtomicBool>,
    should_skip_current: Arc<AtomicBool>,          // NEW
    scan_queue_removals: Arc<Mutex<HashSet<String>>>, // NEW
    is_paused: Arc<AtomicBool>,
    is_quitting: Arc<AtomicBool>,
}
```

- [ ] **Step 2: Initialize new fields in setup**

In the `app.manage(AppState { ... })` block (~line 1428), add:

```rust
should_skip_current: Arc::new(AtomicBool::new(false)),
scan_queue_removals: Arc::new(Mutex::new(HashSet::new())),
```

- [ ] **Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles successfully (new fields are unused for now, which is OK)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat: 添加 should_skip_current 和 scan_queue_removals 到 AppState"
```

---

### Task 2: Add `skip_current_copy` and `remove_from_scan_queue` commands

**Files:**
- Modify: `src-tauri/src/main.rs` (add commands near `cancel_scan` at ~line 467, register at ~line 1441)

- [ ] **Step 1: Add the `skip_current_copy` command**

Add after the `cancel_scan` function (~line 472):

```rust
#[tauri::command]
fn skip_current_copy(state: State<AppState>) {
    // Set skip flag — copy_file_chunked will detect this and stop only the current copy.
    // Unlike should_cancel, the scan loop will continue to the next folder.
    state.should_skip_current.store(true, Ordering::SeqCst);
    // Also unpause so the copy loop can proceed to detect the skip flag.
    state.is_paused.store(false, Ordering::SeqCst);
}
```

- [ ] **Step 2: Add the `remove_from_scan_queue` command**

Add after `skip_current_copy`:

```rust
#[tauri::command]
fn remove_from_scan_queue(state: State<AppState>, folder: String) {
    state.scan_queue_removals.lock().unwrap().insert(folder);
}
```

- [ ] **Step 3: Register the new commands**

In the `.invoke_handler(tauri::generate_handler![...])` block, add after `resume_scan,`:

```rust
skip_current_copy,
remove_from_scan_queue,
```

- [ ] **Step 4: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat: 添加 skip_current_copy 和 remove_from_scan_queue 命令"
```

---

### Task 3: Modify `copy_file_chunked` to support skip flag

**Files:**
- Modify: `src-tauri/src/scanner.rs:178-219` (copy_file_chunked)
- Modify: `src-tauri/src/scanner.rs:231-287` (copy_file_with_overwrite_mode)

- [ ] **Step 1: Add `should_skip` parameter to `copy_file_chunked`**

Update the function signature and loop body:

```rust
fn copy_file_chunked<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64),
) -> Result<u64, String> {
    let mut file_in = std::fs::File::open(from).map_err(|e| e.to_string())?;
    let mut file_out = std::fs::File::create(to).map_err(|e| e.to_string())?;

    let mut buffer = vec![0u8; buffer_size];
    let mut total_copied = 0;

    loop {
        // Check cancel
        if should_cancel.load(Ordering::SeqCst) {
            return Err("Cancelled by user".to_string());
        }
        // Check skip
        if should_skip.load(Ordering::SeqCst) {
            return Err("Skipped by user".to_string());
        }

        // Check pause
        while is_paused.load(Ordering::SeqCst) {
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Cancelled by user".to_string());
            }
            if should_skip.load(Ordering::SeqCst) {
                return Err("Skipped by user".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let n = file_in.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }

        file_out
            .write_all(&buffer[..n])
            .map_err(|e| e.to_string())?;
        total_copied += n as u64;
        on_progress(n as u64);
    }

    Ok(total_copied)
}
```

- [ ] **Step 2: Add `should_skip` parameter to `copy_file_with_overwrite_mode`**

Update signature to include `should_skip: &Arc<AtomicBool>` and pass it through to `copy_file_chunked`:

```rust
fn copy_file_with_overwrite_mode<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    overwrite_existing: bool,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64),
) -> Result<u64, String> {
```

In the body, pass `should_skip` to both calls to `copy_file_chunked`:

```rust
        let copy_result = copy_file_chunked(
            from,
            &temp_target,
            should_cancel,
            should_skip,
            is_paused,
            buffer_size,
            on_progress,
        );
```

and:

```rust
        copy_file_chunked(
            from,
            target,
            should_cancel,
            should_skip,
            is_paused,
            buffer_size,
            on_progress,
        )
```

- [ ] **Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Errors from callers of `copy_file_chunked` and `copy_file_with_overwrite_mode` that don't pass `should_skip` yet — this is expected and will be fixed in Task 4.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/scanner.rs
git commit -m "feat: copy_file_chunked 支持 should_skip 标志"
```

---

### Task 4: Update `perform_copy` to accept and pass `should_skip`

**Files:**
- Modify: `src-tauri/src/scanner.rs:290-926` (perform_copy function)

- [ ] **Step 1: Add `should_skip` parameter to `perform_copy` signature**

Update the function signature at line ~290:

```rust
async fn perform_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    source_path: PathBuf,
    folder_name: String,
    target_parent_path: &Path,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    should_cancel: Arc<AtomicBool>,
    should_skip: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    overwrite_existing: bool,
    result: &mut ScanResult,
    task_id: Option<String>,
    allow_deploy: bool,
    source: &str,
    filter_extensions: &[String],
    filter_includes: &[String],
) {
```

- [ ] **Step 2: Clone and pass `should_skip` into the blocking closure**

Near line ~377 where other clones are made, add:

```rust
let should_skip_clone = should_skip.clone();
```

- [ ] **Step 3: Update the stability check loop to also check `should_skip`**

In the stability check loop (~line 610-619), add skip check:

```rust
for _ in 0..intervals {
    if should_cancel_clone.load(Ordering::SeqCst) {
        return Err(fs_extra::error::Error::new(
            fs_extra::error::ErrorKind::Interrupted,
            "Cancelled by user",
        ));
    }
    if should_skip_clone.load(Ordering::SeqCst) {
        return Err(fs_extra::error::Error::new(
            fs_extra::error::ErrorKind::Interrupted,
            "Skipped by user",
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
}
```

- [ ] **Step 4: Pass `should_skip_clone` to `copy_file_with_overwrite_mode`**

At line ~780, update the call:

```rust
let copy_res = copy_file_with_overwrite_mode(
    &src,
    &dst,
    overwrite_existing,
    &should_cancel_clone,
    &should_skip_clone,
    &is_paused_clone,
    copy_buffer_size,
    &mut |delta| {
        copied_bytes_total += delta;
        update_stats(copied_bytes_total, total_filtered_bytes);
    },
);
```

- [ ] **Step 5: Handle "Skipped" differently from "Cancelled" in copy result**

In the error handling after `copy_file_with_overwrite_mode` (~line 793-831), add a check for "Skipped":

```rust
Err(e) => {
    if e.contains("Skipped") {
        // Log partial history for skipped task
        if !copied_files_list.is_empty() {
            add_history_entry(
                &handle,
                HistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: Local::now().to_rfc3339(),
                    action_type: "COPY_SKIPPED".to_string(),
                    description: format!("Skipped copying {}", folder_name_clone),
                    folder_name: format!("{} (Skipped)", folder_name_clone),
                    source_path: source_path_clone.to_string_lossy().to_string(),
                    target_path: target_full_path_clone.to_string_lossy().to_string(),
                    copied_files_count: copied_files_list.len(),
                    total_size: copied_bytes_total,
                    files: copied_files_list,
                },
            );
        }
        return Err(fs_extra::error::Error::new(
            fs_extra::error::ErrorKind::Interrupted,
            "Skipped by user",
        ));
    } else if e.contains("Cancelled") {
        // existing cancel handling...
```

Also update the pre-file cancel check (~line 739) to also check skip:

```rust
if should_skip_clone.load(Ordering::SeqCst) {
    if !copied_files_list.is_empty() {
        add_history_entry(
            &handle,
            HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Local::now().to_rfc3339(),
                action_type: "COPY_SKIPPED".to_string(),
                description: format!("Skipped copying {}", folder_name_clone),
                folder_name: format!("{} (Skipped)", folder_name_clone),
                source_path: source_path_clone.to_string_lossy().to_string(),
                target_path: target_full_path_clone.to_string_lossy().to_string(),
                copied_files_count: copied_files_list.len(),
                total_size: copied_bytes_total,
                files: copied_files_list.clone(),
            },
        );
    }
    return Err(fs_extra::error::Error::new(
        fs_extra::error::ErrorKind::Interrupted,
        "Skipped by user",
    ));
}
```

- [ ] **Step 6: Handle skip in `perform_copy` result matching**

At the bottom of `perform_copy` (~line 898-925), update the match to handle skipped differently:

```rust
match copy_task.await {
    Ok(Ok(0)) => {
        // Nothing was copied
    }
    Ok(Ok(_)) => {
        emit_log(
            app_handle,
            format!("Successfully copied: {}", folder_name),
            "success",
        );
        result.copied_folders.push(folder_name);
    }
    Ok(Err(e)) => {
        if let fs_extra::error::ErrorKind::Interrupted = e.kind {
            let is_skip = e.to_string().contains("Skipped");
            let msg = if is_skip {
                format!("Copy skipped: {}", folder_name)
            } else {
                format!("Copy cancelled: {}", folder_name)
            };
            emit_log(app_handle, msg, "warn");
        } else {
            let err_msg = format!("Failed to copy {}: {}", folder_name, e);
            emit_log(app_handle, err_msg.clone(), "error");
            result.errors.push(err_msg);
        }
    }
    Err(e) => {
        let err_msg = format!("Copy task panic: {}", e);
        emit_log(app_handle, err_msg.clone(), "error");
        result.errors.push(err_msg);
    }
}
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/scanner.rs
git commit -m "feat: perform_copy 支持 should_skip 标志，区分跳过和取消"
```

---

### Task 5: Update `scan_and_copy` — two-pass scan with queued events + skip/removal logic

**Files:**
- Modify: `src-tauri/src/scanner.rs:1283-1604` (scan_and_copy function)

This is the core change: scan_and_copy now accepts `should_skip` and `scan_queue_removals`, collects all candidate folders first, emits them as queued events, then processes each with skip/removal checks.

- [ ] **Step 1: Update `scan_and_copy` signature**

```rust
pub async fn scan_and_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    should_cancel: Arc<AtomicBool>,
    should_skip: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    scan_queue_removals: Arc<Mutex<HashSet<String>>>,
) -> ScanResult {
```

- [ ] **Step 2: Add a new event emission helper for queued items**

Add a helper struct and emit function near the top of the file (after `ProgressEvent`):

```rust
#[derive(Debug, serde::Serialize, Clone)]
struct ScanQueuedEvent {
    folder: String,
    local_path: String,
    remote_path: String,
}

fn emit_scan_queued<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    folder: &str,
    local_path: &str,
    remote_path: &str,
) {
    let _ = app_handle.emit(
        "scan-queued",
        ScanQueuedEvent {
            folder: folder.to_string(),
            local_path: local_path.to_string(),
            remote_path: remote_path.to_string(),
        },
    );
}
```

- [ ] **Step 3: Refactor DateMatch branch to two-pass**

Replace the DateMatch branch's inner `while let` loop with a two-pass approach:

```rust
MatchRule::DateMatch(format_str) => {
    // ... existing code to determine dirs_to_check stays the same ...

    for target_name in dirs_to_check {
        if should_cancel.load(Ordering::SeqCst) {
            emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
            return result;
        }

        let target_path = path.join(&target_name);

        if !target_path.exists() || !target_path.is_dir() {
            emit_log(
                app_handle,
                format!(
                    "Folder {} does not exist in {}",
                    target_name, task.remote_path
                ),
                "info",
            );
            continue;
        }

        emit_log(
            app_handle,
            format!("Found candidate folder: {}", target_name),
            "success",
        );

        let local_target_base = local_parent.join(&target_name);

        let mut sub_entries = match fs::read_dir(&target_path).await {
            Ok(e) => e,
            Err(e) => {
                let err = format!(
                    "Failed to list contents of {}: {}",
                    target_path.display(),
                    e
                );
                emit_log(app_handle, err.clone(), "error");
                result.errors.push(err);
                continue;
            }
        };

        // --- Pass 1: Collect all sub-directories ---
        let mut sub_dirs: Vec<(PathBuf, String)> = Vec::new();
        while let Ok(Some(entry)) = sub_entries.next_entry().await {
            let sub_path = entry.path();
            if sub_path.is_dir() {
                let sub_name = entry.file_name().to_string_lossy().to_string();
                sub_dirs.push((sub_path, sub_name));
            }
        }

        if sub_dirs.is_empty() {
            emit_log(
                app_handle,
                format!("No build directories found in {}", target_name),
                "info",
            );
            continue;
        }

        // --- Emit all as queued ---
        for (sub_path, sub_name) in &sub_dirs {
            let display_name = format!("{}/{}", target_name, sub_name);
            result.found_folders.push(display_name);
            emit_scan_queued(
                app_handle,
                sub_name,
                &local_target_base.join(sub_name).to_string_lossy(),
                &sub_path.to_string_lossy(),
            );
        }

        // --- Pass 2: Process each folder ---
        // Clear any stale removals from previous scans
        scan_queue_removals.lock().unwrap().clear();

        for (sub_path, sub_name) in sub_dirs {
            // Check global cancel
            if should_cancel.load(Ordering::SeqCst) {
                emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                return result;
            }

            // Check if this folder was removed from queue by user
            {
                let removals = scan_queue_removals.lock().unwrap();
                if removals.contains(&sub_name) {
                    emit_log(
                        app_handle,
                        format!("Removed from queue by user: {}", sub_name),
                        "info",
                    );
                    continue;
                }
            }

            // Reset skip flag before each copy
            should_skip.store(false, Ordering::SeqCst);

            perform_copy(
                app_handle,
                sub_path,
                sub_name,
                &local_target_base,
                config,
                live_config.clone(),
                should_cancel.clone(),
                should_skip.clone(),
                is_paused.clone(),
                false,
                &mut result,
                Some(task.id.clone()),
                true,
                "scheduled",
                &config.file_extensions,
                &config.filename_includes,
            )
            .await;

            // Reset skip flag after copy completes (whether skipped or not)
            should_skip.store(false, Ordering::SeqCst);
        }
    }
}
```

- [ ] **Step 4: Update VersionMatch branch to pass `should_skip`**

In the VersionMatch branch, update the `perform_copy` call (~line 1459) to pass `should_skip.clone()`:

```rust
perform_copy(
    app_handle,
    latest.path.clone(),
    latest.name.clone(),
    local_parent,
    config,
    live_config.clone(),
    should_cancel.clone(),
    should_skip.clone(),
    is_paused.clone(),
    false,
    &mut result,
    Some(task.id.clone()),
    true,
    "scheduled",
    &config.file_extensions,
    &config.filename_includes,
)
.await;
```

- [ ] **Step 5: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Errors from callers of `scan_and_copy` that don't pass the new parameters yet — fixed in Task 6.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/scanner.rs
git commit -m "feat: scan_and_copy 两阶段扫描 — 先收集所有文件夹，再逐个复制（支持跳过/移除）"
```

---

### Task 6: Update callers of `scan_and_copy` and `perform_copy` in `main.rs`

**Files:**
- Modify: `src-tauri/src/main.rs:440-465` (scan_now command)
- Modify: `src-tauri/src/scanner.rs:928-1044` (temporary_copy — pass dummy skip flag)
- Modify: `src-tauri/src/scanner.rs:1047-1281` (temporary_copy_file — pass dummy skip flag)

- [ ] **Step 1: Update `scan_now` to pass new parameters**

```rust
#[tauri::command]
async fn scan_now(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ScanResult, String> {
    if state.is_scanning.load(Ordering::SeqCst) {
        return Err("Scan already in progress".to_string());
    }

    state.is_scanning.store(true, Ordering::SeqCst);
    state.should_cancel.store(false, Ordering::SeqCst);
    state.should_skip_current.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);
    // Clear any stale removals
    state.scan_queue_removals.lock().unwrap().clear();

    let config = state.config.lock().unwrap().clone();
    let live_config = state.config.clone();
    let result = scanner::scan_and_copy(
        &app_handle,
        &config,
        live_config,
        state.should_cancel.clone(),
        state.should_skip_current.clone(),
        state.is_paused.clone(),
        state.scan_queue_removals.clone(),
    )
    .await;

    state.is_scanning.store(false, Ordering::SeqCst);
    Ok(result)
}
```

- [ ] **Step 2: Update `temporary_copy` to pass a dummy `should_skip`**

In `scanner.rs`, update the `perform_copy` call inside `temporary_copy` (~line 1020):

```rust
let no_skip = Arc::new(AtomicBool::new(false));

perform_copy(
    app_handle,
    source_path,
    folder_name,
    &target_root_path,
    config,
    live_config,
    should_cancel,
    no_skip,
    is_paused,
    overwrite_existing,
    &mut result,
    None,
    false,
    "manual",
    &file_extensions,
    &filename_includes,
)
.await;
```

Add `use std::sync::atomic::AtomicBool;` if not already imported (it is already imported at line 11).

- [ ] **Step 3: Update `temporary_copy_file` to pass dummy `should_skip` to `copy_file_with_overwrite_mode`**

In the `temporary_copy_file` function (~line 1230), update:

```rust
let no_skip = Arc::new(AtomicBool::new(false));

copy_file_with_overwrite_mode(
    &source_clone,
    &target_file_clone,
    overwrite_existing,
    &should_cancel,
    &no_skip,
    &is_paused,
    copy_buffer_size,
    &mut on_progress,
)
```

- [ ] **Step 4: Update `start_manual_copy_worker` to pass dummy `should_skip` to `temporary_copy`**

The manual copy worker in `main.rs` calls `scanner::temporary_copy` which already handles its own skip — no changes needed there since `temporary_copy` creates its own `no_skip` internally.

- [ ] **Step 5: Verify full compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles successfully with no errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/main.rs src-tauri/src/scanner.rs
git commit -m "feat: 更新所有调用方传递 should_skip 参数"
```

---

### Task 7: Add frontend invoke wrappers

**Files:**
- Modify: `src/lib/tauri.ts` (add new functions after `resumeScan`)

- [ ] **Step 1: Add `skipCurrentCopy` and `removeFromScanQueue` functions**

Add after the `resumeScan` function (~line 106):

```typescript
export async function skipCurrentCopy(): Promise<void> {
  await invoke('skip_current_copy');
}

export async function removeFromScanQueue(folder: string): Promise<void> {
  await invoke('remove_from_scan_queue', { folder });
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat: 添加 skipCurrentCopy 和 removeFromScanQueue 前端调用"
```

---

### Task 8: Handle `scan-queued` event and add skip/remove store functions

**Files:**
- Modify: `src/lib/store.ts` (add `markTaskRecordSkipped`, `removeQueuedTaskRecord` functions)
- Modify: `src/App.vue` (listen for `scan-queued` event)

- [ ] **Step 1: Add `markTaskRecordSkipped` to store.ts**

Add after `markTaskRecordCancelled` function (~line 485):

```typescript
export function markTaskRecordSkipped(folder?: string) {
    const target = findTargetRecord(folder, appStore.progress?.localPath);
    if (!target) return;
    target.phase = 'cancelled';
    target.speed = 0;
    target.finishedAtMs = Date.now();
    touchTaskRecord(target);
}

export function removeQueuedTaskRecord(folder: string) {
    const idx = appStore.taskRecords.findIndex(
        r => r.folder === folder && r.phase === 'queued'
    );
    if (idx >= 0) {
        appStore.taskRecords.splice(idx, 1);
    }
}
```

- [ ] **Step 2: Update `syncTaskRecordByLog` to handle "Copy skipped:" log messages**

In `syncTaskRecordByLog` (~line 516), add handling for skipped messages after the cancelled block:

```typescript
const skippedFolder = extractFolderByPrefix(msg, 'Copy skipped:');
if (skippedFolder) {
    markTaskRecordSkipped(skippedFolder);
    return;
}
```

- [ ] **Step 3: Add `scan-queued` event listener in App.vue**

In `App.vue`'s `onMounted`, add a new listener after the existing `copy-progress` listener:

```typescript
listen('scan-queued', (event: { payload: { folder: string; local_path: string; remote_path: string } }) => {
    const p = event.payload;
    // Only create a queued record if there isn't already an active one for this folder
    const existing = appStore.taskRecords.find(
        r => r.folder === p.folder && (r.phase === 'queued' || r.phase === 'copying' || r.phase === 'paused')
    );
    if (existing) return;

    const now = Date.now();
    const record: TaskRecord = {
        id: `${p.folder}-${now}`,
        startTime: new Date(now).toLocaleString(),
        startedAtMs: now,
        updatedAt: now,
        folder: p.folder,
        sourcePath: p.remote_path,
        localPath: p.local_path,
        copyPercentage: 0,
        copyCompleted: false,
        copyTotal: 0,
        hasRemote: false,
        remoteServers: [],
        remoteExpanded: false,
        deployPercentage: 0,
        deployCompleted: false,
        speed: 0,
        copied: 0,
        total: 0,
        phase: 'queued' as const,
        source: 'scheduled' as const,
        filterExtensions: [],
        filterKeywords: [],
    };
    appStore.taskRecords.unshift(record);
    if (appStore.taskRecords.length > appStore.maxTaskRecords) appStore.taskRecords.pop();
});
```

Import `TaskRecord` type in `App.vue` if not already imported. Currently the imports are:

```typescript
import { appStore, addLog, upsertTaskRecord, syncTaskRecordByLog, updateManualCopyTaskState, markStaleTasksInterrupted } from '@/lib/store';
```

Add `type TaskRecord` to this import.

- [ ] **Step 4: Commit**

```bash
git add src/lib/store.ts src/App.vue
git commit -m "feat: 处理 scan-queued 事件，添加跳过/移除队列 store 函数"
```

---

### Task 9: Add Skip and Remove buttons to TaskStatusPage

**Files:**
- Modify: `src/pages/TaskStatusPage.vue:35-37` (isActivePhase function)
- Modify: `src/pages/TaskStatusPage.vue:89-104` (handleCancel function area)
- Modify: `src/pages/TaskStatusPage.vue:551-574` (action buttons template)

- [ ] **Step 1: Update `isActivePhase` to include `queued`**

```typescript
function isActivePhase(phase: string): boolean {
  return phase === 'copying' || phase === 'paused' || phase === 'queued';
}
```

- [ ] **Step 2: Add import for new functions**

Update the import from `@/lib/tauri` to include the new functions:

```typescript
import { getConfig, cancelScan, pauseScan, resumeScan, skipCurrentCopy, removeFromScanQueue, addSystemEvent, openPathParent, type AppConfig, type DeployServer, type ScanTask } from '@/lib/tauri';
```

Update the import from `@/lib/store` to include new functions:

```typescript
import { appStore, addLog, markTaskRecordCancelled, markTaskRecordSkipped, removeQueuedTaskRecord, setTaskRecordPaused, type TaskRecord } from '@/lib/store';
```

- [ ] **Step 3: Add `handleSkip` and `handleRemoveFromQueue` functions**

Add after `handleCancel`:

```typescript
async function handleSkip(target: TaskRecord) {
  if (isCancelling.value) return;
  isCancelling.value = true;
  const msg = `${t('console.skipping')} (${target.folder})`;
  addLog(msg, 'info');
  markTaskRecordSkipped(target.folder);

  try {
    await skipCurrentCopy();
  } catch (e) {
    addLog(`Skip failed: ${e}`, 'error');
  } finally {
    isCancelling.value = false;
  }
}

async function handleRemoveFromQueue(target: TaskRecord) {
  try {
    await removeFromScanQueue(target.folder);
    removeQueuedTaskRecord(target.folder);
    addLog(`${t('console.removedFromQueue')} (${target.folder})`, 'info');
  } catch (e) {
    addLog(`Remove failed: ${e}`, 'error');
  }
}
```

- [ ] **Step 4: Update the action buttons template**

Replace the existing per-task actions section (~line 551-574):

```html
<!-- Per-task Actions -->
<div class="flex justify-center gap-1.5">
  <template v-if="rec.phase === 'queued'">
    <button
      @click="handleRemoveFromQueue(rec)"
      class="inline-flex items-center justify-center rounded-md border border-red-200 bg-white text-red-600 p-1.5 hover:bg-red-50 hover:border-red-300 transition-colors active:scale-95"
      :title="t('console.removeFromQueue')"
    >
      <XCircle class="w-4 h-4" />
    </button>
  </template>
  <template v-else-if="rec.phase === 'copying' || rec.phase === 'paused'">
    <button
      @click="togglePause(rec)"
      class="inline-flex items-center justify-center rounded-md border p-1.5 transition-colors active:scale-95"
      :class="rec.phase === 'paused'
        ? 'border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-100'
        : 'border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-100'"
      :title="rec.phase === 'paused' ? t('console.resume') : t('console.pause')"
    >
      <component :is="rec.phase === 'paused' ? PlayCircle : Pause" class="w-4 h-4" />
    </button>
    <button
      @click="handleSkip(rec)"
      class="inline-flex items-center justify-center rounded-md border border-orange-200 bg-white text-orange-600 p-1.5 hover:bg-orange-50 hover:border-orange-300 transition-colors active:scale-95"
      :disabled="isCancelling"
      :title="t('console.skipCurrent')"
    >
      <SkipForward class="w-4 h-4" />
    </button>
    <button
      @click="handleCancel(rec)"
      class="inline-flex items-center justify-center rounded-md border border-red-200 bg-white text-red-600 p-1.5 hover:bg-red-50 hover:border-red-300 transition-colors active:scale-95"
      :disabled="isCancelling"
      :title="t('console.cancel')"
    >
      <XCircle class="w-4 h-4" />
    </button>
  </template>
  <span v-else class="text-xs text-slate-400">-</span>
</div>
```

- [ ] **Step 5: Add `SkipForward` icon import**

Update the lucide import at line 3:

```typescript
import { Play, Square, RefreshCw, Clock, Activity, Pause, PlayCircle, XCircle, SkipForward, Copy, Trash2, FolderOpen, HardDrive, Cloud, Info, X } from 'lucide-vue-next';
```

- [ ] **Step 6: Commit**

```bash
git add src/pages/TaskStatusPage.vue
git commit -m "feat: 任务列表添加跳过和移除队列按钮"
```

---

### Task 10: Add i18n translations

**Files:**
- Modify: `src/locales/messages.ts` (add new keys to both `en` and `zh` sections)

- [ ] **Step 1: Add English translations**

In the English `console` section, add near the existing cancel/pause translations:

```typescript
skipCurrent: 'Skip',
skipping: 'Skipping...',
removeFromQueue: 'Remove from queue',
removedFromQueue: 'Removed from queue',
phaseSkipped: 'Skipped',
```

- [ ] **Step 2: Add Chinese translations**

In the Chinese `console` section, add at the same location:

```typescript
skipCurrent: '跳过',
skipping: '正在跳过...',
removeFromQueue: '移出队列',
removedFromQueue: '已移出队列',
phaseSkipped: '已跳过',
```

- [ ] **Step 3: Commit**

```bash
git add src/locales/messages.ts
git commit -m "feat: 添加跳过/移除队列 i18n 翻译"
```

---

### Task 11: Build and verify

**Files:** None (verification only)

- [ ] **Step 1: Run full build**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: Build succeeds, producing a versioned `.exe` file.

- [ ] **Step 2: Commit final state**

If any build fixes were needed, commit them:

```bash
git add -A
git commit -m "fix: 构建修复"
```
