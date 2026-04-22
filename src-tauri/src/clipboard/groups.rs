use tauri::{AppHandle, Emitter};

use crate::clipboard::db;
use crate::clipboard::models::ClipboardGroup;

pub const GROUPS_CHANGED_EVENT: &str = "clipboard-groups-changed";

pub fn normalize_group_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("clipboard group name cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn emit_groups_changed(app: &AppHandle, groups: &[ClipboardGroup]) {
    let _ = app.emit(GROUPS_CHANGED_EVENT, groups);
}

pub fn list_groups_snapshot(conn: &rusqlite::Connection) -> Result<Vec<ClipboardGroup>, String> {
    db::list_groups(conn).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_group_name;

    #[test]
    fn normalize_group_name_trims_outer_whitespace() {
        assert_eq!(normalize_group_name("  Work  ").unwrap(), "Work");
    }

    #[test]
    fn normalize_group_name_rejects_blank_values() {
        assert!(normalize_group_name(" \n\t ").is_err());
    }
}
