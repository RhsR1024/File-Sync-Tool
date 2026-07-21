//! Global shortcut registration (spec §8.1).

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;
use std::str::FromStr;

use parking_lot::Mutex;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// Owned handle representing one live global-shortcut registration.
/// Drop or call `unregister` to free it.
pub struct HotkeyHandle {
    shortcut: Shortcut,
    app: AppHandle,
}

impl HotkeyHandle {
    pub fn unregister(self) {
        let _ = self.app.global_shortcut().unregister(self.shortcut);
    }
}

/// Register `hotkey_str` (e.g. `"Alt+C"`, `"Ctrl+Shift+V"`). On press, toggles the clipboard panel.
pub fn register(app: AppHandle, hotkey_str: &str) -> Result<HotkeyHandle, String> {
    let shortcut =
        Shortcut::from_str(hotkey_str).map_err(|e| format!("parse hotkey '{hotkey_str}': {e}"))?;

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            // global-hotkey 0.7 / plugin 2.3.1 fires for both Pressed and Released.
            // Gate so one keypress toggles exactly once.
            if event.state != ShortcutState::Pressed {
                return;
            }
            if let Err(e) = crate::clipboard::commands::cb_toggle_panel_internal(app.clone()) {
                eprintln!("[clipboard] toggle panel failed: {e}");
            }
        })
        .map_err(|e| format!("register hotkey '{hotkey_str}': {e}"))?;

    Ok(HotkeyHandle { shortcut, app })
}

/// Register the image-copy shortcut. On Windows it copies the image currently selected in
/// File Explorer; other platforms retain the native image picker fallback.
pub fn register_image_copy(app: AppHandle, hotkey_str: &str) -> Result<HotkeyHandle, String> {
    let shortcut =
        Shortcut::from_str(hotkey_str).map_err(|e| format!("parse hotkey '{hotkey_str}': {e}"))?;

    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }

            #[cfg(target_os = "windows")]
            std::thread::spawn(|| {
                let Some(path) = selected_explorer_image() else {
                    return;
                };
                if let Err(error) = crate::clipboard::image_copy::copy_image_file(&path, None) {
                    eprintln!("[clipboard] image-copy hotkey failed: {error}");
                }
            });

            #[cfg(not(target_os = "windows"))]
            std::thread::spawn(|| {
                let selected = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg"])
                    .pick_file();
                let Some(path) = selected else {
                    return;
                };
                if let Err(error) = crate::clipboard::image_copy::copy_image_file(&path, None) {
                    eprintln!("[clipboard] image-copy hotkey failed: {error}");
                }
            });
        })
        .map_err(|e| format!("register hotkey '{hotkey_str}': {e}"))?;

    Ok(HotkeyHandle { shortcut, app })
}

#[cfg(target_os = "windows")]
fn selected_explorer_image() -> Option<PathBuf> {
    // Capture the foreground window before starting PowerShell so a stale selection from a
    // background Explorer window cannot win.
    let foreground_window = unsafe { GetForegroundWindow() };
    if foreground_window.0.is_null() {
        return None;
    }
    let foreground_window = foreground_window.0 as isize;

    // Shell.Application exposes Explorer's selected items. The command is read-only and
    // emits one UTF-8 path per line; unsupported files are ignored below.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$foregroundWindow = [long]$env:FST_FOREGROUND_WINDOW
$shell = New-Object -ComObject Shell.Application
foreach ($window in $shell.Windows()) {
  try {
    if ([long]$window.HWND -eq $foregroundWindow -and $window.FullName -match '(?i)\\explorer\.exe$') {
      foreach ($item in $window.Document.SelectedItems()) {
        if ($item.Path) { $item.Path }
      }
    }
  } catch {}
}
"#;

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            SCRIPT,
        ])
        .env("FST_FOREGROUND_WINDOW", foreground_window.to_string())
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .find(|path| path.is_file() && has_supported_image_extension(path))
}

#[cfg(target_os = "windows")]
fn has_supported_image_extension(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg")
    )
}

/// Swap the currently-registered hotkey for a new one. Rolls back on failure.
pub fn change(
    app: AppHandle,
    current: &Mutex<Option<HotkeyHandle>>,
    new_hotkey: &str,
) -> Result<(), String> {
    // Parse the new hotkey first so we don't unregister on a bogus input.
    let _ =
        Shortcut::from_str(new_hotkey).map_err(|e| format!("parse hotkey '{new_hotkey}': {e}"))?;

    // Unregister old.
    if let Some(old) = current.lock().take() {
        old.unregister();
    }
    // Register new. If it fails, leave `current` as None — caller should surface the error.
    let handle = register(app, new_hotkey)?;
    *current.lock() = Some(handle);
    Ok(())
}

pub fn change_image_copy(
    app: AppHandle,
    current: &Mutex<Option<HotkeyHandle>>,
    enabled: bool,
    new_hotkey: &str,
) -> Result<(), String> {
    let next = if enabled {
        Some(register_image_copy(app, new_hotkey)?)
    } else {
        None
    };
    let old = std::mem::replace(&mut *current.lock(), next);
    if let Some(handle) = old {
        handle.unregister();
    }
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::has_supported_image_extension;
    use std::path::Path;

    #[test]
    fn image_copy_hotkey_filters_supported_extensions_case_insensitively() {
        assert!(has_supported_image_extension(Path::new("photo.PNG")));
        assert!(has_supported_image_extension(Path::new("photo.jpeg")));
        assert!(!has_supported_image_extension(Path::new("photo.webp")));
        assert!(!has_supported_image_extension(Path::new("photo")));
    }
}
