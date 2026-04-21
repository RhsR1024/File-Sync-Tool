//! Clipboard retention / capacity cleanup (spec §7.5).

use rusqlite::{params, Connection, Result as SqlResult};

use crate::clipboard::models::ClipboardSettings;

/// Apply `retain_days` and `max_items` limits. Favorites and pinned items are
/// always exempt.
/// Returns `(deleted_by_age, deleted_by_cap)`.
pub fn run_cleanup(conn: &Connection, settings: &ClipboardSettings) -> SqlResult<(u64, u64)> {
    let mut deleted_by_age = 0u64;
    if settings.retain_days > 0 {
        let cutoff =
            chrono::Utc::now().timestamp_millis() - (settings.retain_days as i64) * 86_400_000;
        deleted_by_age = conn.execute(
            "DELETE FROM clipboard_items
             WHERE is_favorite = 0
               AND is_pinned = 0
               AND COALESCE(updated_at, created_at) < ?1",
            params![cutoff],
        )? as u64;
    }

    let mut deleted_by_cap = 0u64;
    if settings.max_items > 0 {
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM clipboard_items WHERE is_favorite = 0 AND is_pinned = 0",
            [],
            |r| r.get(0),
        )?;
        let excess = total - (settings.max_items as i64);
        if excess > 0 {
            deleted_by_cap = conn.execute(
                "DELETE FROM clipboard_items
                 WHERE id IN (
                     SELECT id FROM clipboard_items
                     WHERE is_favorite = 0
                       AND is_pinned = 0
                     ORDER BY COALESCE(updated_at, created_at) ASC
                     LIMIT ?1
                 )",
                params![excess],
            )? as u64;
        }
    }

    Ok((deleted_by_age, deleted_by_cap))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::db;
    use crate::clipboard::models::ContentKind;

    fn seed(n: i64) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        for i in 0..n {
            db::insert_item(
                &conn,
                &db::NewItem {
                    kind: ContentKind::Text,
                    content_preview: format!("item {i}"),
                    content_full: None,
                    rtf_content: None,
                    html: None,
                    image_path: None,
                    image_width: None,
                    image_height: None,
                    file_paths: None,
                    byte_size: 0,
                    hash: format!("h{i}"),
                    source_app: None,
                    source_app_icon: None,
                },
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn cleanup_respects_max_items_and_keeps_favorites() {
        let conn = seed(10);
        conn.execute(
            "UPDATE clipboard_items SET is_favorite = 1 WHERE id IN (1, 2)",
            [],
        )
        .unwrap();

        let settings = ClipboardSettings {
            max_items: 5,
            retain_days: 0,
            ..ClipboardSettings::default()
        };
        let (by_age, by_cap) = run_cleanup(&conn, &settings).unwrap();
        assert_eq!(by_age, 0);
        assert_eq!(by_cap, 3); // 8 non-fav - 5 cap = 3 deleted

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 7); // 2 favorites + 5 non-favorites
    }

    #[test]
    fn cleanup_skips_when_limits_are_zero() {
        let conn = seed(5);
        let settings = ClipboardSettings {
            max_items: 0,
            retain_days: 0,
            ..ClipboardSettings::default()
        };
        let (by_age, by_cap) = run_cleanup(&conn, &settings).unwrap();
        assert_eq!(by_age, 0);
        assert_eq!(by_cap, 0);
    }

    #[test]
    fn cleanup_keeps_pinned_items() {
        let conn = seed(5);
        conn.execute("UPDATE clipboard_items SET is_pinned = 1 WHERE id = 1", [])
            .unwrap();

        let settings = ClipboardSettings {
            max_items: 1,
            retain_days: 0,
            ..ClipboardSettings::default()
        };
        let (_, by_cap) = run_cleanup(&conn, &settings).unwrap();
        assert_eq!(by_cap, 3);

        let pinned_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE is_pinned = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pinned_count, 1);
    }
}
