//! Clipboard SQLite database layer (spec §7.2).

use std::path::Path;
use rusqlite::{Connection, Result as SqlResult};

pub fn open(db_path: &Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS clipboard_items (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            kind                TEXT NOT NULL CHECK (kind IN ('text','html','image','file')),
            content_preview     TEXT NOT NULL,
            content_full        TEXT,
            html                TEXT,
            image_path          TEXT,
            image_width         INTEGER,
            image_height        INTEGER,
            file_paths_json     TEXT,
            byte_size           INTEGER NOT NULL DEFAULT 0,
            hash                TEXT NOT NULL UNIQUE,
            source_app          TEXT,
            is_favorite         INTEGER NOT NULL DEFAULT 0,
            favorite_sort_index INTEGER,
            created_at          INTEGER NOT NULL,
            updated_at          INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cb_kind       ON clipboard_items(kind);
        CREATE INDEX IF NOT EXISTS idx_cb_favorite   ON clipboard_items(is_favorite) WHERE is_favorite = 1;
        CREATE INDEX IF NOT EXISTS idx_cb_created_at ON clipboard_items(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_cb_fav_sort   ON clipboard_items(favorite_sort_index) WHERE is_favorite = 1;

        CREATE TABLE IF NOT EXISTS schema_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('version', '1');
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_applies_cleanly_in_memory() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).expect("migrate should succeed");
        let v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "1");
    }

    #[test]
    fn indexes_created() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).expect("migrate");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_cb_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 4);
    }
}
