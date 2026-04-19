//! Clipboard SQLite database layer (spec §7.2).

use std::path::Path;
use rusqlite::{params, params_from_iter, Connection, Result as SqlResult, ToSql};

use crate::clipboard::models::{
    ClipboardFilter, ClipboardItem, ClipboardListQuery, ClipboardListResult, ContentKind,
};

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

pub struct NewItem {
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub hash: String,
    pub source_app: Option<String>,
}

pub fn insert_item(conn: &Connection, item: &NewItem) -> SqlResult<i64> {
    let now = chrono::Utc::now().timestamp_millis();
    let paths_json = item
        .file_paths
        .as_ref()
        .map(|p| serde_json::to_string(p).unwrap_or_default());
    conn.execute(
        "INSERT INTO clipboard_items
          (kind, content_preview, content_full, html, image_path, image_width, image_height,
           file_paths_json, byte_size, hash, source_app, is_favorite, favorite_sort_index,
           created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,0,NULL,?12,?12)",
        params![
            item.kind.as_sql(),
            item.content_preview,
            item.content_full,
            item.html,
            item.image_path,
            item.image_width,
            item.image_height,
            paths_json,
            item.byte_size,
            item.hash,
            item.source_app,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn touch_item_by_hash(conn: &Connection, hash: &str) -> SqlResult<bool> {
    let now = chrono::Utc::now().timestamp_millis();
    let affected = conn.execute(
        "UPDATE clipboard_items SET updated_at=?1 WHERE hash=?2",
        params![now, hash],
    )?;
    Ok(affected > 0)
}

pub fn list_items(conn: &Connection, q: &ClipboardListQuery) -> SqlResult<ClipboardListResult> {
    let (where_sql, filter_params) = build_where(q);

    // Count first (only filter params)
    let count_sql = format!("SELECT COUNT(*) FROM clipboard_items WHERE {where_sql}");
    let total: i64 = conn.query_row(
        &count_sql,
        params_from_iter(filter_params.iter().map(|p| p.as_ref())),
        |r| r.get(0),
    )?;

    // List: filter params + limit + offset
    let limit_placeholder = filter_params.len() + 1;
    let offset_placeholder = filter_params.len() + 2;
    let list_sql = format!(
        "SELECT id, kind, content_preview, content_full, html, image_path, image_width,
                image_height, file_paths_json, byte_size, hash, source_app,
                is_favorite, favorite_sort_index, created_at, updated_at
         FROM clipboard_items
         WHERE {where_sql}
         ORDER BY
           CASE WHEN is_favorite=1 THEN COALESCE(favorite_sort_index, 9999999) END ASC,
           COALESCE(updated_at, created_at) DESC
         LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}"
    );

    let mut all_params: Vec<Box<dyn ToSql>> = filter_params.into_iter().collect();
    all_params.push(Box::new(q.limit));
    all_params.push(Box::new(q.offset));

    let mut stmt = conn.prepare(&list_sql)?;
    let items = stmt
        .query_map(
            params_from_iter(all_params.iter().map(|p| p.as_ref())),
            row_to_item,
        )?
        .collect::<SqlResult<Vec<_>>>()?;

    Ok(ClipboardListResult { items, total })
}

fn build_where(q: &ClipboardListQuery) -> (String, Vec<Box<dyn ToSql>>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    match q.filter {
        ClipboardFilter::All => {}
        ClipboardFilter::Text => clauses.push("kind IN ('text','html')".into()),
        ClipboardFilter::Image => clauses.push("kind='image'".into()),
        ClipboardFilter::File => clauses.push("kind='file'".into()),
        ClipboardFilter::Favorite => clauses.push("is_favorite=1".into()),
    }

    let trimmed = q.search.trim();
    if !trimmed.is_empty() {
        let like = format!(
            "%{}%",
            trimmed.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"),
        );
        let p1 = values.len() + 1;
        let p2 = values.len() + 2;
        clauses.push(format!(
            "(content_preview LIKE ?{p1} ESCAPE '\\' OR content_full LIKE ?{p2} ESCAPE '\\')"
        ));
        values.push(Box::new(like.clone()));
        values.push(Box::new(like));
    }

    let where_sql = if clauses.is_empty() {
        "1=1".into()
    } else {
        clauses.join(" AND ")
    };
    (where_sql, values)
}

fn row_to_item(r: &rusqlite::Row) -> SqlResult<ClipboardItem> {
    let kind_str: String = r.get(1)?;
    let file_paths_json: Option<String> = r.get(8)?;
    let file_paths = file_paths_json.and_then(|s| serde_json::from_str(&s).ok());
    Ok(ClipboardItem {
        id: r.get(0)?,
        kind: ContentKind::from_sql(&kind_str),
        content_preview: r.get(2)?,
        content_full: r.get(3)?,
        html: r.get(4)?,
        image_path: r.get(5)?,
        image_width: r.get(6)?,
        image_height: r.get(7)?,
        file_paths,
        byte_size: r.get(9)?,
        hash: r.get(10)?,
        source_app: r.get(11)?,
        is_favorite: r.get::<_, i64>(12)? != 0,
        favorite_sort_index: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

pub fn get_item(conn: &Connection, id: i64) -> SqlResult<ClipboardItem> {
    conn.query_row(
        "SELECT id, kind, content_preview, content_full, html, image_path, image_width,
                image_height, file_paths_json, byte_size, hash, source_app,
                is_favorite, favorite_sort_index, created_at, updated_at
         FROM clipboard_items WHERE id=?1",
        params![id],
        row_to_item,
    )
}

pub fn delete_item(conn: &Connection, id: i64) -> SqlResult<()> {
    conn.execute("DELETE FROM clipboard_items WHERE id=?1", params![id])?;
    Ok(())
}

pub fn delete_batch(conn: &mut Connection, ids: &[i64]) -> SqlResult<()> {
    let tx = conn.transaction()?;
    for id in ids {
        tx.execute("DELETE FROM clipboard_items WHERE id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn clear_all(conn: &Connection, keep_favorites: bool) -> SqlResult<u64> {
    let sql = if keep_favorites {
        "DELETE FROM clipboard_items WHERE is_favorite=0"
    } else {
        "DELETE FROM clipboard_items"
    };
    let affected = conn.execute(sql, [])?;
    Ok(affected as u64)
}

pub fn toggle_favorite(conn: &Connection, id: i64) -> SqlResult<ClipboardItem> {
    conn.execute(
        "UPDATE clipboard_items SET is_favorite = CASE is_favorite WHEN 1 THEN 0 ELSE 1 END WHERE id=?1",
        params![id],
    )?;
    get_item(conn, id)
}

pub fn reorder_favorites(conn: &mut Connection, ids: &[i64]) -> SqlResult<()> {
    let tx = conn.transaction()?;
    for (idx, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE clipboard_items SET favorite_sort_index = ?1 WHERE id = ?2 AND is_favorite = 1",
            params![idx as i64, id],
        )?;
    }
    tx.commit()?;
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

    #[test]
    fn insert_and_list_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let item = NewItem {
            kind: ContentKind::Text,
            content_preview: "hello".into(),
            content_full: Some("hello world".into()),
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: 11,
            hash: "abc".into(),
            source_app: None,
        };
        let id = insert_item(&conn, &item).unwrap();
        assert!(id > 0);

        let q = ClipboardListQuery {
            filter: ClipboardFilter::All,
            search: String::new(),
            offset: 0,
            limit: 10,
        };
        let result = list_items(&conn, &q).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].content_preview, "hello");
    }

    #[test]
    fn search_filters_by_text() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        for (i, text) in ["apple", "banana", "cherry"].iter().enumerate() {
            insert_item(
                &conn,
                &NewItem {
                    kind: ContentKind::Text,
                    content_preview: (*text).to_string(),
                    content_full: Some((*text).to_string()),
                    html: None,
                    image_path: None,
                    image_width: None,
                    image_height: None,
                    file_paths: None,
                    byte_size: text.len() as i64,
                    hash: format!("hash_{i}"),
                    source_app: None,
                },
            )
            .unwrap();
        }

        let q = ClipboardListQuery {
            filter: ClipboardFilter::All,
            search: "ana".into(),
            offset: 0,
            limit: 10,
        };
        let result = list_items(&conn, &q).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].content_preview, "banana");
    }

    #[test]
    fn toggle_favorite_flips_flag() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "a".into(),
                content_full: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "h_fav".into(),
                source_app: None,
            },
        )
        .unwrap();
        let item = toggle_favorite(&conn, id).unwrap();
        assert!(item.is_favorite);
        let item = toggle_favorite(&conn, id).unwrap();
        assert!(!item.is_favorite);
    }

    #[test]
    fn reorder_favorites_updates_sort_index() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let a = insert_item(&conn, &NewItem {
            kind: ContentKind::Text,
            content_preview: "a".into(),
            content_full: None, html: None,
            image_path: None, image_width: None, image_height: None,
            file_paths: None, byte_size: 0,
            hash: "ha".into(), source_app: None,
        }).unwrap();
        let b = insert_item(&conn, &NewItem {
            kind: ContentKind::Text,
            content_preview: "b".into(),
            content_full: None, html: None,
            image_path: None, image_width: None, image_height: None,
            file_paths: None, byte_size: 0,
            hash: "hb".into(), source_app: None,
        }).unwrap();
        // Mark both as favorites
        conn.execute("UPDATE clipboard_items SET is_favorite = 1 WHERE id IN (?1, ?2)", params![a, b]).unwrap();

        reorder_favorites(&mut conn, &[b, a]).unwrap();

        let idx_b: i64 = conn.query_row(
            "SELECT favorite_sort_index FROM clipboard_items WHERE id = ?1",
            params![b], |r| r.get(0),
        ).unwrap();
        let idx_a: i64 = conn.query_row(
            "SELECT favorite_sort_index FROM clipboard_items WHERE id = ?1",
            params![a], |r| r.get(0),
        ).unwrap();
        assert_eq!(idx_b, 0);
        assert_eq!(idx_a, 1);
    }

    #[test]
    fn reorder_favorites_skips_non_favorites() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let non_fav = insert_item(&conn, &NewItem {
            kind: ContentKind::Text,
            content_preview: "x".into(),
            content_full: None, html: None,
            image_path: None, image_width: None, image_height: None,
            file_paths: None, byte_size: 0,
            hash: "hx".into(), source_app: None,
        }).unwrap();
        // is_favorite stays 0
        reorder_favorites(&mut conn, &[non_fav]).unwrap();
        let idx: Option<i64> = conn.query_row(
            "SELECT favorite_sort_index FROM clipboard_items WHERE id = ?1",
            params![non_fav], |r| r.get(0),
        ).unwrap();
        assert!(idx.is_none(), "non-favorite should not receive a sort index");
    }
}
