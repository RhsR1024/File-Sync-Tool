# Tauri Native Dialogs

## Scenario: Windows-Safe Folder Picker

### 1. Scope / Trigger

- Trigger: any Tauri command that opens a native Windows folder picker.
- Applies to: selecting manual-copy target directories, file-share roots, settings directories, and any future command using `rfd` folder selection.
- Problem prevented: on Windows, creating a new folder inside the system folder picker and then confirming can crash the process when the picker runs on a transient async worker/dialog thread.

### 2. Signatures

```rust
#[tauri::command]
async fn open_directory(window: WebviewWindow) -> Result<Option<String>, String>;

#[tauri::command]
pub async fn file_share_pick_directory(
    window: WebviewWindow,
) -> Result<Option<SharedDir>, String>;

pub(crate) async fn run_dialog_task_on_main_thread<T, Run>(
    window: &WebviewWindow,
    run: Run,
) -> Result<T, String>
where
    T: Send + 'static,
    Run: FnOnce() -> Result<T, String> + Send + 'static;
```

### 3. Contracts

- Folder pickers must execute `rfd::FileDialog::pick_folder()` through `WebviewWindow::run_on_main_thread`.
- Do not run folder pickers with `tauri::async_runtime::spawn_blocking` or `rfd::AsyncFileDialog`.
- A cancelled picker returns `Ok(None)`.
- A selected directory returns `Ok(Some(path_string))`, using `PathBuf::to_string_lossy().to_string()`.
- File-share folder selection maps the selected string into `SharedDir { alias: make_alias(&path), path }`.
- Main-thread dispatch failures return `MAIN_THREAD_DIALOG_DISPATCH_FAILED::{error}`.
- Dropped dialog result channels return `MAIN_THREAD_DIALOG_RESULT_DROPPED`.

### 4. Validation & Error Matrix

| Case | Expected Result |
| --- | --- |
| User selects an existing directory | `Ok(Some(path))` |
| User creates a directory in the picker and confirms it | `Ok(Some(new_path))`, no process crash |
| User cancels the picker | `Ok(None)` |
| Tauri cannot schedule the main-thread task | `Err("MAIN_THREAD_DIALOG_DISPATCH_FAILED::...")` |
| Dialog task runs but result channel is dropped | `Err("MAIN_THREAD_DIALOG_RESULT_DROPPED")` |

### 5. Good/Base/Bad Cases

- Good: `open_directory` schedules sync `rfd::FileDialog::new().pick_folder()` on the Tauri main thread.
- Base: frontend calls `invoke("open_directory")`; the `WebviewWindow` argument is injected by Tauri, not passed from TypeScript.
- Bad: using `rfd::AsyncFileDialog::new().pick_folder().await` for Windows folder picking.

### 6. Tests Required

- Unit test that the scheduler runs a dialog task and returns its result.
- Unit test that scheduler dispatch failure is surfaced unchanged.
- Unit test that a picked `PathBuf` maps to the expected `Option<String>`.
- Unit test that directory picker dispatch failure returns an error.
- Manual Windows smoke test before release: open manual-copy target picker, create a new folder in the native picker, select it, and confirm the app stays alive.

### 7. Wrong vs Correct

#### Wrong

```rust
let picked = rfd::AsyncFileDialog::new().pick_folder().await;
Ok(picked.map(|handle| handle.path().to_string_lossy().to_string()))
```

#### Correct

```rust
let picked = run_dialog_task_on_main_thread(&window, || {
    Ok(rfd::FileDialog::new()
        .pick_folder()
        .map(|path| path.to_string_lossy().to_string()))
})
.await?;
```
