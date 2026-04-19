//! Global shortcut registration (spec §8.1). Implemented in M3.

pub struct HotkeyHandle;

impl HotkeyHandle {
    pub fn unregister(self) {
        // TODO(M3): unregister via tauri-plugin-global-shortcut.
    }
}
