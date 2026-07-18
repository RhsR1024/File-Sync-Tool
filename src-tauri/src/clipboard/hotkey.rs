//! Global shortcut registration (spec §8.1).

use std::str::FromStr;

use parking_lot::Mutex;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

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

/// Register the image-copy shortcut. It opens a native image picker and copies the selected
/// image as image pixels without showing the main window or a success notification.
pub fn register_image_copy(app: AppHandle, hotkey_str: &str) -> Result<HotkeyHandle, String> {
    let shortcut =
        Shortcut::from_str(hotkey_str).map_err(|e| format!("parse hotkey '{hotkey_str}': {e}"))?;

    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
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
