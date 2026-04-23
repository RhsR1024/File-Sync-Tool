//! Clipboard SQLite database layer (spec §7.2).

use rusqlite::{params, params_from_iter, Connection, OpenFlags, Result as SqlResult, ToSql};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::clipboard::models::{
    ClipboardDedupStrategy, ClipboardFilter, ClipboardGroup, ClipboardItem, ClipboardListQuery,
    ClipboardListResult, ContentKind,
};

const CLIPBOARD_SCHEMA_VERSION: i64 = 3;
const WRITE_CACHE_SIZE_KIB: i64 = -65_536;
const READ_CACHE_SIZE_KIB: i64 = -32_768;
const MMAP_SIZE_BYTES: i64 = 268_435_456;
static ALWAYS_NEW_HASH_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn open(db_path: &Path) -> SqlResult<Connection> {
    let conn = open_write(db_path)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_write(db_path: &Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(&format!(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = {WRITE_CACHE_SIZE_KIB};
        PRAGMA mmap_size = {MMAP_SIZE_BYTES};
        PRAGMA temp_store = MEMORY;
        "
    ))?;
    Ok(conn)
}

pub fn open_read(db_path: &Path) -> SqlResult<Connection> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch(&format!(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA cache_size = {READ_CACHE_SIZE_KIB};
        PRAGMA mmap_size = {MMAP_SIZE_BYTES};
        PRAGMA query_only = ON;
        "
    ))?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> SqlResult<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('version', '1');
        "#,
    )?;
    ensure_clipboard_groups_table(conn)?;
    let mut version = read_schema_version(conn)?;
    let needs_rebuild = clipboard_items_needs_rebuild(conn)?;
    if version < 2 || needs_rebuild {
        migrate_clipboard_items_v2(conn)?;
        version = 2;
        set_schema_version(conn, version)?;
    }
    if version < 3 && !table_has_column(conn, "clipboard_items", "from_self")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN from_self INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if version < CLIPBOARD_SCHEMA_VERSION {
        set_schema_version(conn, CLIPBOARD_SCHEMA_VERSION)?;
    }
    ensure_clipboard_indexes(conn)?;
    Ok(())
}

fn read_schema_version(conn: &Connection) -> SqlResult<i64> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'version'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(value
        .as_deref()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0))
}

fn set_schema_version(conn: &Connection, version: i64) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO schema_meta(key, value) VALUES('version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![version.to_string()],
    )?;
    Ok(())
}

fn ensure_clipboard_indexes(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_cb_kind       ON clipboard_items(kind);
        CREATE INDEX IF NOT EXISTS idx_cb_kind_created ON clipboard_items(kind, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_cb_favorite   ON clipboard_items(is_favorite) WHERE is_favorite = 1;
        CREATE INDEX IF NOT EXISTS idx_cb_pinned     ON clipboard_items(is_pinned) WHERE is_pinned = 1;
        CREATE INDEX IF NOT EXISTS idx_cb_group      ON clipboard_items(group_id) WHERE group_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_cb_created_at ON clipboard_items(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_cb_fav_sort   ON clipboard_items(favorite_sort_index) WHERE is_favorite = 1;
        "#,
    )
}

fn ensure_clipboard_groups_table(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS clipboard_groups (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL UNIQUE,
            sort_index  INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cb_group_sort ON clipboard_groups(sort_index);
        "#,
    )
}

fn clipboard_items_needs_rebuild(conn: &Connection) -> SqlResult<bool> {
    let table_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='clipboard_items'",
        [],
        |r| r.get(0),
    )?;
    if table_exists == 0 {
        return Ok(true);
    }

    for column in [
        "rtf_content",
        "char_count",
        "source_app_icon",
        "group_id",
        "is_pinned",
    ] {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('clipboard_items') WHERE name = ?1",
            params![column],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Ok(true);
        }
    }

    if !clipboard_items_has_group_fk_set_null(conn)? {
        return Ok(true);
    }

    Ok(false)
}

fn clipboard_items_has_group_fk_set_null(conn: &Connection) -> SqlResult<bool> {
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM pragma_foreign_key_list('clipboard_items')
        WHERE "table" = 'clipboard_groups'
          AND "from" = 'group_id'
          AND on_delete = 'SET NULL'
        "#,
        [],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> SqlResult<bool> {
    let sql = format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1");
    let exists: i64 = conn.query_row(&sql, params![column], |r| r.get(0))?;
    Ok(exists > 0)
}

fn create_clipboard_items_table(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS clipboard_items (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            kind                TEXT NOT NULL CHECK (kind IN ('text','html','rtf','image','file')),
            content_preview     TEXT NOT NULL,
            content_full        TEXT,
            rtf_content         TEXT,
            html                TEXT,
            image_path          TEXT,
            image_width         INTEGER,
            image_height        INTEGER,
            file_paths_json     TEXT,
            byte_size           INTEGER NOT NULL DEFAULT 0,
            char_count          INTEGER NOT NULL DEFAULT 0,
            hash                TEXT NOT NULL UNIQUE,
            source_app          TEXT,
            source_app_icon     TEXT,
            from_self           INTEGER NOT NULL DEFAULT 0,
            group_id            INTEGER REFERENCES clipboard_groups(id) ON DELETE SET NULL,
            is_favorite         INTEGER NOT NULL DEFAULT 0,
            is_pinned           INTEGER NOT NULL DEFAULT 0,
            favorite_sort_index INTEGER,
            created_at          INTEGER NOT NULL,
            updated_at          INTEGER NOT NULL
        );
        "#,
    )?;
    ensure_clipboard_indexes(conn)
}

fn migrate_clipboard_items_v2(conn: &Connection) -> SqlResult<()> {
    let has_items: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='clipboard_items'",
        [],
        |r| r.get(0),
    )?;
    if has_items == 0 {
        create_clipboard_items_table(conn)?;
        return Ok(());
    }

    let legacy_has_rtf = table_has_column(conn, "clipboard_items", "rtf_content")?;
    let legacy_has_char_count = table_has_column(conn, "clipboard_items", "char_count")?;
    let legacy_has_source_app_icon = table_has_column(conn, "clipboard_items", "source_app_icon")?;
    let legacy_has_from_self = table_has_column(conn, "clipboard_items", "from_self")?;
    let legacy_has_group_id = table_has_column(conn, "clipboard_items", "group_id")?;
    let legacy_has_is_pinned = table_has_column(conn, "clipboard_items", "is_pinned")?;

    let rtf_select = if legacy_has_rtf {
        "rtf_content"
    } else {
        "NULL"
    };
    let source_app_icon_select = if legacy_has_source_app_icon {
        "source_app_icon"
    } else {
        "NULL"
    };
    let from_self_select = if legacy_has_from_self {
        "from_self"
    } else {
        "0"
    };
    let group_id_select = if legacy_has_group_id {
        "CASE WHEN group_id IS NOT NULL AND EXISTS (SELECT 1 FROM clipboard_groups WHERE id = group_id) THEN group_id ELSE NULL END"
    } else {
        "NULL"
    };
    let is_pinned_select = if legacy_has_is_pinned {
        "is_pinned"
    } else {
        "0"
    };
    let char_count_select = if legacy_has_char_count {
        "COALESCE(char_count, COALESCE(LENGTH(COALESCE(content_full, content_preview)), 0))"
    } else {
        "COALESCE(LENGTH(COALESCE(content_full, content_preview)), 0)"
    };

    let migrate_sql = format!(
        r#"
        BEGIN IMMEDIATE;
        ALTER TABLE clipboard_items RENAME TO clipboard_items_legacy;
        CREATE TABLE clipboard_items (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            kind                TEXT NOT NULL CHECK (kind IN ('text','html','rtf','image','file')),
            content_preview     TEXT NOT NULL,
            content_full        TEXT,
            rtf_content         TEXT,
            html                TEXT,
            image_path          TEXT,
            image_width         INTEGER,
            image_height        INTEGER,
            file_paths_json     TEXT,
            byte_size           INTEGER NOT NULL DEFAULT 0,
            char_count          INTEGER NOT NULL DEFAULT 0,
            hash                TEXT NOT NULL UNIQUE,
            source_app          TEXT,
            source_app_icon     TEXT,
            from_self           INTEGER NOT NULL DEFAULT 0,
            group_id            INTEGER REFERENCES clipboard_groups(id) ON DELETE SET NULL,
            is_favorite         INTEGER NOT NULL DEFAULT 0,
            is_pinned           INTEGER NOT NULL DEFAULT 0,
            favorite_sort_index INTEGER,
            created_at          INTEGER NOT NULL,
            updated_at          INTEGER NOT NULL
        );
        INSERT INTO clipboard_items (
            id, kind, content_preview, content_full, rtf_content, html, image_path,
            image_width, image_height, file_paths_json, byte_size, char_count, hash,
            source_app, source_app_icon, from_self, group_id, is_favorite, is_pinned,
            favorite_sort_index, created_at, updated_at
        )
        SELECT
            id, kind, content_preview, content_full, {rtf_select}, html, image_path,
            image_width, image_height, file_paths_json, byte_size,
            {char_count_select},
            hash, source_app, {source_app_icon_select}, {from_self_select}, {group_id_select},
            is_favorite, {is_pinned_select},
            favorite_sort_index, created_at, updated_at
        FROM clipboard_items_legacy;
        DROP TABLE clipboard_items_legacy;
        COMMIT;
        "#,
    );
    conn.execute_batch(&migrate_sql)?;
    ensure_clipboard_indexes(conn)
}

#[derive(Debug, Clone)]
pub struct NewItem {
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub rtf_content: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub hash: String,
    pub source_app: Option<String>,
    pub source_app_icon: Option<String>,
    pub from_self: bool,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn insert_item_with_hash(conn: &Connection, item: &NewItem, stored_hash: &str) -> SqlResult<i64> {
    let now = now_ms();
    let paths_json = item
        .file_paths
        .as_ref()
        .map(|p| serde_json::to_string(p).unwrap_or_default());
    let char_count = item
        .content_full
        .as_ref()
        .or(Some(&item.content_preview))
        .map(|s| s.chars().count() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO clipboard_items
          (kind, content_preview, content_full, rtf_content, html, image_path, image_width, image_height,
           file_paths_json, byte_size, char_count, hash, source_app, source_app_icon, from_self,
           group_id, is_favorite, is_pinned, favorite_sort_index, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL,0,0,NULL,?16,?16)",
        params![
            item.kind.as_sql(),
            item.content_preview,
            item.content_full,
            item.rtf_content,
            item.html,
            item.image_path,
            item.image_width,
            item.image_height,
            paths_json,
            item.byte_size,
            char_count,
            stored_hash,
            item.source_app,
            item.source_app_icon,
            item.from_self,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_item(conn: &Connection, item: &NewItem) -> SqlResult<i64> {
    insert_item_with_hash(conn, item, &item.hash)
}

pub fn item_exists_by_hash(conn: &Connection, hash: &str) -> SqlResult<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM clipboard_items WHERE hash = ?1 LIMIT 1")?;
    let mut rows = stmt.query(params![hash])?;
    Ok(rows.next()?.is_some())
}

fn refresh_duplicate_item_by_hash(
    conn: &Connection,
    item: &NewItem,
    touch_updated_at: bool,
) -> SqlResult<bool> {
    let affected = if touch_updated_at {
        conn.execute(
            "UPDATE clipboard_items
             SET updated_at = ?1, source_app = ?2, source_app_icon = ?3, from_self = ?4
             WHERE hash = ?5",
            params![
                now_ms(),
                item.source_app,
                item.source_app_icon,
                item.from_self,
                item.hash
            ],
        )?
    } else {
        conn.execute(
            "UPDATE clipboard_items
             SET source_app = ?1, source_app_icon = ?2, from_self = ?3
             WHERE hash = ?4",
            params![item.source_app, item.source_app_icon, item.from_self, item.hash],
        )?
    };
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
    // When viewing ONLY favorites, respect user-defined drag order
    // (favorite_sort_index). Otherwise (All/Text/Image/File), sort purely by
    // recency so favoriting an item doesn't change its position in the list.
    let order_sql = if matches!(q.filter, ClipboardFilter::Favorite) || q.op_fav_only {
        "COALESCE(favorite_sort_index, 9999999) ASC,
         COALESCE(updated_at, created_at) DESC,
         id DESC"
    } else {
        "COALESCE(updated_at, created_at) DESC, id DESC"
    };
    let list_sql = format!(
        "SELECT id, kind, content_preview, content_full, rtf_content, html, image_path, image_width,
                image_height, file_paths_json, byte_size, char_count, hash, source_app,
                source_app_icon, from_self, group_id, is_favorite, is_pinned, favorite_sort_index,
                created_at, updated_at
         FROM clipboard_items
         WHERE {where_sql}
         ORDER BY {order_sql}
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
    fn local_date_boundary_ms(date_str: &str, end_of_day: bool) -> Option<i64> {
        use chrono::TimeZone;

        let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        let naive = if end_of_day {
            date.and_hms_opt(23, 59, 59)?
        } else {
            date.and_hms_opt(0, 0, 0)?
        };
        chrono::Local
            .from_local_datetime(&naive)
            .earliest()
            .or_else(|| chrono::Local.from_local_datetime(&naive).latest())
            .map(|dt| dt.timestamp_millis())
    }

    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    match q.filter {
        ClipboardFilter::All => {}
        ClipboardFilter::Text => clauses.push("kind IN ('text','html','rtf')".into()),
        ClipboardFilter::Image => clauses.push("kind='image'".into()),
        ClipboardFilter::File => clauses.push("kind='file'".into()),
        ClipboardFilter::Favorite => clauses.push("is_favorite=1".into()),
        ClipboardFilter::Pinned => clauses.push("is_pinned=1".into()),
    }

    if let Some(group_id) = q.group_id {
        let p = values.len() + 1;
        clauses.push(format!("group_id = ?{p}"));
        values.push(Box::new(group_id));
    }
    if q.pinned_only {
        clauses.push("is_pinned=1".into());
    }

    let mut search_terms: Vec<String> = Vec::new();
    let trimmed = q.search.trim();
    if !trimmed.is_empty() {
        search_terms.push(trimmed.to_string());
    }
    if let Some(payload) = &q.search_payload {
        search_terms.extend(
            payload
                .keywords
                .iter()
                .map(|term| term.trim())
                .filter(|term| !term.is_empty())
                .map(ToString::to_string),
        );
    }

    if let Some(payload) = &q.search_payload {
        if let Some(kind) = payload.filters.kind.as_ref() {
            let p = values.len() + 1;
            clauses.push(format!("kind = ?{p}"));
            values.push(Box::new(kind.as_sql().to_string()));
        }
        if let Some(from) = payload.filters.from.as_ref() {
            if let Some(from_ms) = local_date_boundary_ms(from, false) {
                let p = values.len() + 1;
                clauses.push(format!("COALESCE(updated_at, created_at) >= ?{p}"));
                values.push(Box::new(from_ms));
            }
        }
        if let Some(to) = payload.filters.to.as_ref() {
            if let Some(to_ms) = local_date_boundary_ms(to, true) {
                let p = values.len() + 1;
                clauses.push(format!("COALESCE(updated_at, created_at) <= ?{p}"));
                values.push(Box::new(to_ms));
            }
        }
        if let Some(app) = payload.filters.app.as_ref() {
            let like = format!(
                "%{}%",
                app.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_"),
            );
            let p = values.len() + 1;
            clauses.push(format!("source_app LIKE ?{p} ESCAPE '\\'"));
            values.push(Box::new(like));
        }
        if payload.filters.fav {
            clauses.push("is_favorite = 1".into());
        }
        if let Some(group_id) = payload.filters.group_id {
            let p = values.len() + 1;
            clauses.push(format!("group_id = ?{p}"));
            values.push(Box::new(group_id));
        }
        if payload.filters.pinned_only {
            clauses.push("is_pinned = 1".into());
        }
        if let Some(size_gt) = payload.filters.size_gt {
            let p = values.len() + 1;
            clauses.push(format!("byte_size > ?{p}"));
            values.push(Box::new(size_gt));
        }
        if let Some(size_lt) = payload.filters.size_lt {
            let p = values.len() + 1;
            clauses.push(format!("byte_size < ?{p}"));
            values.push(Box::new(size_lt));
        }
    }

    for term in search_terms {
        let like = format!(
            "%{}%",
            term.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_"),
        );
        let mut term_clauses = Vec::new();
        for column in [
            "content_preview",
            "content_full",
            "html",
            "rtf_content",
            "file_paths_json",
        ] {
            let p = values.len() + 1;
            term_clauses.push(format!("{column} LIKE ?{p} ESCAPE '\\'"));
            values.push(Box::new(like.clone()));
        }
        clauses.push(format!("({})", term_clauses.join(" OR ")));
    }

    // Operator DSL predicates (spec §9.1). Each uses parameter binding.
    if let Some(ref k) = q.op_type {
        let p = values.len() + 1;
        clauses.push(format!("kind = ?{p}"));
        values.push(Box::new(k.clone()));
    }
    if let Some(from_ms) = q.op_from_ms {
        let p = values.len() + 1;
        clauses.push(format!("COALESCE(updated_at, created_at) >= ?{p}"));
        values.push(Box::new(from_ms));
    }
    if let Some(to_ms) = q.op_to_ms {
        let p = values.len() + 1;
        clauses.push(format!("COALESCE(updated_at, created_at) <= ?{p}"));
        values.push(Box::new(to_ms));
    }
    if let Some(ref app) = q.op_app {
        let like = format!(
            "%{}%",
            app.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_"),
        );
        let p = values.len() + 1;
        clauses.push(format!("source_app LIKE ?{p} ESCAPE '\\'"));
        values.push(Box::new(like));
    }
    if q.op_fav_only {
        clauses.push("is_favorite = 1".into());
    }
    if let Some(n) = q.op_size_gt {
        let p = values.len() + 1;
        clauses.push(format!("byte_size > ?{p}"));
        values.push(Box::new(n));
    }
    if let Some(n) = q.op_size_lt {
        let p = values.len() + 1;
        clauses.push(format!("byte_size < ?{p}"));
        values.push(Box::new(n));
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
    let file_paths_json: Option<String> = r.get(9)?;
    let file_paths = file_paths_json.and_then(|s| serde_json::from_str(&s).ok());
    Ok(ClipboardItem {
        id: r.get(0)?,
        kind: ContentKind::from_sql(&kind_str),
        content_preview: r.get(2)?,
        content_full: r.get(3)?,
        rtf_content: r.get(4)?,
        html: r.get(5)?,
        image_path: r.get(6)?,
        image_width: r.get(7)?,
        image_height: r.get(8)?,
        file_paths,
        byte_size: r.get(10)?,
        char_count: r.get(11)?,
        hash: r.get(12)?,
        source_app: r.get(13)?,
        source_app_icon: r.get(14)?,
        from_self: r.get::<_, i64>(15)? != 0,
        group_id: r.get(16)?,
        is_favorite: r.get::<_, i64>(17)? != 0,
        is_pinned: r.get::<_, i64>(18)? != 0,
        favorite_sort_index: r.get(19)?,
        created_at: r.get(20)?,
        updated_at: r.get(21)?,
    })
}

pub fn get_item(conn: &Connection, id: i64) -> SqlResult<ClipboardItem> {
    conn.query_row(
        "SELECT id, kind, content_preview, content_full, rtf_content, html, image_path, image_width,
                image_height, file_paths_json, byte_size, char_count, hash, source_app,
                source_app_icon, from_self, group_id, is_favorite, is_pinned, favorite_sort_index,
                created_at, updated_at
         FROM clipboard_items WHERE id=?1",
        params![id],
        row_to_item,
    )
}

fn get_item_by_hash(conn: &Connection, hash: &str) -> SqlResult<ClipboardItem> {
    conn.query_row(
        "SELECT id, kind, content_preview, content_full, rtf_content, html, image_path, image_width,
                image_height, file_paths_json, byte_size, char_count, hash, source_app,
                source_app_icon, from_self, group_id, is_favorite, is_pinned, favorite_sort_index,
                created_at, updated_at
         FROM clipboard_items
         WHERE hash = ?1
         ORDER BY COALESCE(updated_at, created_at) DESC, id DESC
         LIMIT 1",
        params![hash],
        row_to_item,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn compute_hash(kind: &ContentKind, data: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(kind.as_sql().as_bytes());
    hasher.update(b":");
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}

fn make_always_new_hash(base_hash: &str) -> String {
    let seq = ALWAYS_NEW_HASH_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{base_hash}:{}:{seq}", now_ms())
}

pub fn upsert_item_with_dedup(
    conn: &Connection,
    item: &NewItem,
    strategy: ClipboardDedupStrategy,
) -> SqlResult<ClipboardItem> {
    match strategy {
        ClipboardDedupStrategy::MoveToTop => {
            if refresh_duplicate_item_by_hash(conn, item, true)? {
                return get_item_by_hash(conn, &item.hash);
            }
            let id = insert_item(conn, item)?;
            get_item(conn, id)
        }
        ClipboardDedupStrategy::Ignore => {
            if refresh_duplicate_item_by_hash(conn, item, false)? {
                return get_item_by_hash(conn, &item.hash);
            }
            let id = insert_item(conn, item)?;
            get_item(conn, id)
        }
        ClipboardDedupStrategy::AlwaysNew => {
            let id = insert_item_with_hash(conn, item, &make_always_new_hash(&item.hash))?;
            get_item(conn, id)
        }
    }
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

pub fn toggle_pin(conn: &Connection, id: i64) -> SqlResult<ClipboardItem> {
    conn.execute(
        "UPDATE clipboard_items SET is_pinned = CASE is_pinned WHEN 1 THEN 0 ELSE 1 END WHERE id=?1",
        params![id],
    )?;
    get_item(conn, id)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn update_text_content(conn: &Connection, id: i64, new_text: &str) -> SqlResult<()> {
    if new_text.is_empty() {
        delete_item(conn, id)?;
        return Ok(());
    }

    let preview: String = new_text.chars().take(200).collect();
    let byte_size = new_text.len() as i64;
    let char_count = new_text.chars().count() as i64;
    let hash = compute_hash(&ContentKind::Text, new_text.as_bytes());
    let affected = conn.execute(
        "UPDATE clipboard_items
         SET kind = 'text',
             content_preview = ?1,
             content_full = ?2,
             rtf_content = NULL,
             html = NULL,
             byte_size = ?3,
             char_count = ?4,
             hash = ?5,
             updated_at = ?6
         WHERE id = ?7",
        params![preview, new_text, byte_size, char_count, hash, now_ms(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

fn get_group(conn: &Connection, id: i64) -> SqlResult<ClipboardGroup> {
    conn.query_row(
        "SELECT id, name, sort_index, created_at FROM clipboard_groups WHERE id=?1",
        params![id],
        |r| {
            Ok(ClipboardGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                sort_index: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )
}

pub fn list_groups(conn: &Connection) -> SqlResult<Vec<ClipboardGroup>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, sort_index, created_at
         FROM clipboard_groups
         ORDER BY sort_index ASC, created_at ASC, id ASC",
    )?;
    let groups = stmt
        .query_map([], |r| {
            Ok(ClipboardGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                sort_index: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?
        .collect::<SqlResult<Vec<_>>>()?;
    Ok(groups)
}

pub fn create_group(conn: &Connection, name: &str) -> SqlResult<ClipboardGroup> {
    let now = chrono::Utc::now().timestamp_millis();
    let next_sort: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM clipboard_groups",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO clipboard_groups (name, sort_index, created_at) VALUES (?1, ?2, ?3)",
        params![name, next_sort, now],
    )?;
    get_group(conn, conn.last_insert_rowid())
}

pub fn rename_group(conn: &Connection, id: i64, name: &str) -> SqlResult<ClipboardGroup> {
    conn.execute(
        "UPDATE clipboard_groups SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    get_group(conn, id)
}

pub fn delete_group(conn: &Connection, id: i64) -> SqlResult<bool> {
    let deleted = conn.execute("DELETE FROM clipboard_groups WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

pub fn move_item_to_group(
    conn: &Connection,
    item_id: i64,
    group_id: Option<i64>,
) -> SqlResult<ClipboardItem> {
    conn.execute(
        "UPDATE clipboard_items SET group_id = ?1 WHERE id = ?2",
        params![group_id, item_id],
    )?;
    get_item(conn, item_id)
}

pub fn db_optimize(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch("PRAGMA optimize;")
}

pub fn db_vacuum(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch("VACUUM;")
}

fn list_referenced_paths(
    conn: &Connection,
    column: &str,
) -> SqlResult<std::collections::HashSet<String>> {
    let sql = match column {
        "image_path" => {
            "SELECT DISTINCT image_path FROM clipboard_items WHERE image_path IS NOT NULL AND image_path != ''"
        }
        "source_app_icon" => {
            "SELECT DISTINCT source_app_icon FROM clipboard_items WHERE source_app_icon IS NOT NULL AND source_app_icon != ''"
        }
        _ => unreachable!("unsupported asset column"),
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<SqlResult<std::collections::HashSet<_>>>()
}

pub fn list_referenced_image_paths(
    conn: &Connection,
) -> SqlResult<std::collections::HashSet<String>> {
    list_referenced_paths(conn, "image_path")
}

pub fn list_referenced_icon_paths(
    conn: &Connection,
) -> SqlResult<std::collections::HashSet<String>> {
    list_referenced_paths(conn, "source_app_icon")
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
    use crate::clipboard::models::{ClipboardSearchFilters, ClipboardSearchPayload};

    fn sample_text_item(hash: &str, text: &str) -> NewItem {
        NewItem {
            kind: ContentKind::Text,
            content_preview: text.into(),
            content_full: Some(text.into()),
            rtf_content: None,
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: text.len() as i64,
            hash: hash.into(),
            source_app: None,
            source_app_icon: None,
            from_self: false,
        }
    }

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
        assert_eq!(v, "3");

        let group_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='clipboard_groups'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(group_tables, 1);
    }

    #[test]
    fn migrate_upgrades_legacy_clipboard_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE clipboard_items (
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
            CREATE TABLE schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT INTO schema_meta(key, value) VALUES ('version', '1');
            INSERT INTO clipboard_items (
                kind, content_preview, content_full, html, image_path, image_width, image_height,
                file_paths_json, byte_size, hash, source_app, is_favorite, favorite_sort_index,
                created_at, updated_at
            ) VALUES (
                'text', 'legacy', 'legacy full', NULL, NULL, NULL, NULL,
                NULL, 11, 'legacy_hash', 'oldapp', 0, NULL, 1, 1
            );
            "#,
        )
        .unwrap();

        migrate(&conn).expect("legacy schema should upgrade");

        let version: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, "3");

        let has_rtf_column: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('clipboard_items') WHERE name='rtf_content'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_rtf_column, 1);

        conn.execute(
            "INSERT INTO clipboard_items (kind, content_preview, byte_size, char_count, hash, created_at, updated_at)
             VALUES ('rtf', 'rtf preview', 4, 4, 'rtf_hash', 2, 2)",
            [],
        )
        .unwrap();

        let rtf_kind: String = conn
            .query_row(
                "SELECT kind FROM clipboard_items WHERE hash='rtf_hash'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rtf_kind, "rtf");
    }

    #[test]
    fn migrate_repairs_v2_marked_but_incomplete_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE clipboard_items (
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
            CREATE TABLE schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT INTO schema_meta(key, value) VALUES ('version', '2');
            INSERT INTO clipboard_items (
                kind, content_preview, content_full, html, image_path, image_width, image_height,
                file_paths_json, byte_size, hash, source_app, is_favorite, favorite_sort_index,
                created_at, updated_at
            ) VALUES (
                'text', 'broken', 'broken full', NULL, NULL, NULL, NULL,
                NULL, 6, 'broken_hash', 'oldapp', 0, NULL, 3, 3
            );
            "#,
        )
        .unwrap();

        migrate(&conn).expect("version-2-but-incomplete schema should repair");

        let has_new_columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('clipboard_items') WHERE name IN ('rtf_content','char_count','source_app_icon','group_id','is_pinned')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_new_columns, 5);

        let repaired: ClipboardItem = conn
            .query_row(
                "SELECT id, kind, content_preview, content_full, rtf_content, html, image_path, image_width,
                        image_height, file_paths_json, byte_size, char_count, hash, source_app,
                        source_app_icon, from_self, group_id, is_favorite, is_pinned, favorite_sort_index,
                        created_at, updated_at
                 FROM clipboard_items WHERE hash='broken_hash'",
                [],
                row_to_item,
            )
            .unwrap();
        assert_eq!(repaired.content_preview, "broken");
        assert_eq!(repaired.char_count, 11);
        assert!(!repaired.from_self);
        assert!(!repaired.is_pinned);
    }

    #[test]
    fn migrate_adds_from_self_column_with_zero_default_for_existing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO schema_meta(key, value) VALUES ('version', '2');
            CREATE TABLE clipboard_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                sort_index INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                content_preview TEXT NOT NULL,
                content_full TEXT,
                rtf_content TEXT,
                html TEXT,
                image_path TEXT,
                image_width INTEGER,
                image_height INTEGER,
                file_paths_json TEXT,
                byte_size INTEGER NOT NULL DEFAULT 0,
                char_count INTEGER NOT NULL DEFAULT 0,
                hash TEXT NOT NULL UNIQUE,
                source_app TEXT,
                source_app_icon TEXT,
                group_id INTEGER,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                is_pinned INTEGER NOT NULL DEFAULT 0,
                favorite_sort_index INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            INSERT INTO clipboard_items(kind, content_preview, byte_size, char_count, hash, created_at, updated_at)
            VALUES ('text', 'legacy', 6, 6, 'legacy-hash', 1, 1);
            "#,
        )
        .unwrap();

        migrate(&conn).unwrap();

        let from_self: i64 = conn
            .query_row(
                "SELECT from_self FROM clipboard_items WHERE hash = 'legacy-hash'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(from_self, 0);
    }

    #[test]
    fn migrate_repairs_v2_schema_missing_group_foreign_key_behavior() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE clipboard_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                sort_index INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE clipboard_items (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                kind                TEXT NOT NULL CHECK (kind IN ('text','html','rtf','image','file')),
                content_preview     TEXT NOT NULL,
                content_full        TEXT,
                rtf_content         TEXT,
                html                TEXT,
                image_path          TEXT,
                image_width         INTEGER,
                image_height        INTEGER,
                file_paths_json     TEXT,
                byte_size           INTEGER NOT NULL DEFAULT 0,
                char_count          INTEGER NOT NULL DEFAULT 0,
                hash                TEXT NOT NULL UNIQUE,
                source_app          TEXT,
                source_app_icon     TEXT,
                group_id            INTEGER,
                is_favorite         INTEGER NOT NULL DEFAULT 0,
                is_pinned           INTEGER NOT NULL DEFAULT 0,
                favorite_sort_index INTEGER,
                created_at          INTEGER NOT NULL,
                updated_at          INTEGER NOT NULL
            );
            CREATE TABLE schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT INTO schema_meta(key, value) VALUES ('version', '2');
            INSERT INTO clipboard_groups (id, name, sort_index, created_at) VALUES (1, 'Legacy', 0, 1);
            INSERT INTO clipboard_items (
                kind, content_preview, content_full, rtf_content, html, image_path, image_width, image_height,
                file_paths_json, byte_size, char_count, hash, source_app, source_app_icon, group_id,
                is_favorite, is_pinned, favorite_sort_index, created_at, updated_at
            ) VALUES (
                'text', 'legacy group item', 'legacy group item', NULL, NULL, NULL, NULL, NULL,
                NULL, 17, 17, 'legacy_group_hash', NULL, NULL, 1,
                0, 0, NULL, 10, 10
            );
            "#,
        )
        .unwrap();

        migrate(&conn).expect("version-2 schema without FK should repair");

        assert!(delete_group(&conn, 1).unwrap());
        let item = get_item_by_hash(&conn, "legacy_group_hash").unwrap();
        assert_eq!(item.group_id, None);
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
        assert_eq!(count, 8);
    }

    #[test]
    fn read_connection_is_query_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("clipboard.db");
        let write = open(&db_path).unwrap();
        drop(write);

        let read = open_read(&db_path).unwrap();
        let query_only: i64 = read
            .query_row("PRAGMA query_only", [], |r| r.get(0))
            .unwrap();
        assert_eq!(query_only, 1);
        assert!(read.execute("DELETE FROM clipboard_items", []).is_err());
    }

    #[test]
    fn dedup_move_to_top_keeps_one_row_and_refreshes_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let item = sample_text_item("dedup_move", "same text");

        let first =
            upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::MoveToTop).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second =
            upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::MoveToTop).unwrap();

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(first.id, second.id);
        assert!(second.updated_at >= first.updated_at);
    }

    #[test]
    fn dedup_move_to_top_refreshes_source_metadata() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let mut item = sample_text_item("dedup_move_source", "same text");
        item.source_app = Some("Word".into());
        item.source_app_icon = Some("C:\\icons\\word.png".into());

        let first =
            upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::MoveToTop).unwrap();

        item.source_app = Some("Excel".into());
        item.source_app_icon = Some("C:\\icons\\excel.png".into());
        item.from_self = true;
        let second =
            upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::MoveToTop).unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(second.source_app.as_deref(), Some("Excel"));
        assert_eq!(
            second.source_app_icon.as_deref(),
            Some("C:\\icons\\excel.png")
        );
        assert!(second.from_self);
    }

    #[test]
    fn dedup_ignore_keeps_one_row_without_touching_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let item = sample_text_item("dedup_ignore", "same text");

        let first = upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::Ignore).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::Ignore).unwrap();

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(first.id, second.id);
        assert_eq!(second.updated_at, first.updated_at);
    }

    #[test]
    fn dedup_ignore_refreshes_source_metadata_without_touching_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let mut item = sample_text_item("dedup_ignore_source", "same text");
        item.source_app = Some("Word".into());
        item.source_app_icon = Some("C:\\icons\\word.png".into());

        let first = upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::Ignore).unwrap();

        item.source_app = Some("Excel".into());
        item.source_app_icon = Some("C:\\icons\\excel.png".into());
        item.from_self = true;
        let second = upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::Ignore).unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(second.updated_at, first.updated_at);
        assert_eq!(second.source_app.as_deref(), Some("Excel"));
        assert_eq!(
            second.source_app_icon.as_deref(),
            Some("C:\\icons\\excel.png")
        );
        assert!(second.from_self);
    }

    #[test]
    fn dedup_always_new_inserts_distinct_rows() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let item = sample_text_item("dedup_always_new", "same text");

        let first =
            upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::AlwaysNew).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second =
            upsert_item_with_dedup(&conn, &item, ClipboardDedupStrategy::AlwaysNew).unwrap();

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);
        assert_ne!(first.id, second.id);
        assert_ne!(first.hash, second.hash);
        assert!(first.hash.starts_with("dedup_always_new:"));
        assert!(second.hash.starts_with("dedup_always_new:"));
    }

    #[test]
    fn always_new_hashes_are_unique_across_burst_calls() {
        let mut hashes = std::collections::BTreeSet::new();
        for _ in 0..128 {
            hashes.insert(make_always_new_hash("burst"));
        }
        assert_eq!(hashes.len(), 128);
    }

    #[test]
    fn rtf_kind_roundtrips_through_sql() {
        assert_eq!(ContentKind::from_sql("rtf"), ContentKind::Rtf);
        assert_eq!(ContentKind::Rtf.as_sql(), "rtf");
    }

    #[test]
    fn pinned_and_group_filters_apply() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let group = create_group(&conn, "Pinned").unwrap();
        let id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "pin".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "pin_hash".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE clipboard_items SET is_pinned = 1, group_id = ?2 WHERE id = ?1",
            params![id, group.id],
        )
        .unwrap();

        let q = ClipboardListQuery {
            filter: ClipboardFilter::Pinned,
            search: String::new(),
            search_payload: None,
            group_id: Some(group.id),
            pinned_only: true,
            op_type: None,
            op_from_ms: None,
            op_to_ms: None,
            op_app: None,
            op_fav_only: false,
            op_size_gt: None,
            op_size_lt: None,
            offset: 0,
            limit: 10,
        };
        let result = list_items(&conn, &q).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].group_id, Some(group.id));
        assert!(result.items[0].is_pinned);
    }

    #[test]
    fn toggle_pin_flips_flag() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "pin".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "pin_toggle".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        let item = toggle_pin(&conn, id).unwrap();
        assert!(item.is_pinned);
        let item = toggle_pin(&conn, id).unwrap();
        assert!(!item.is_pinned);
    }

    #[test]
    fn group_crud_and_item_moves_work() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let work = create_group(&conn, "Work").unwrap();
        let personal = create_group(&conn, "Personal").unwrap();
        let groups = list_groups(&conn).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "Work");
        assert_eq!(groups[1].name, "Personal");

        let renamed = rename_group(&conn, personal.id, "Home").unwrap();
        assert_eq!(renamed.name, "Home");

        let item_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "group me".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "group_item".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        let moved = move_item_to_group(&conn, item_id, Some(work.id)).unwrap();
        assert_eq!(moved.group_id, Some(work.id));

        let cleared = move_item_to_group(&conn, item_id, None).unwrap();
        assert_eq!(cleared.group_id, None);

        let moved_again = move_item_to_group(&conn, item_id, Some(renamed.id)).unwrap();
        assert_eq!(moved_again.group_id, Some(renamed.id));

        assert!(delete_group(&conn, renamed.id).unwrap());
        let after_delete = get_item(&conn, item_id).unwrap();
        assert_eq!(after_delete.group_id, None);

        assert!(delete_group(&conn, work.id).unwrap());
        let remaining = list_groups(&conn).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn update_text_content_refreshes_char_count_and_deletes_empty() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "old".into(),
                content_full: Some("old".into()),
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 3,
                hash: "update_me".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        update_text_content(&conn, id, "hello there").unwrap();
        let updated = get_item(&conn, id).unwrap();
        assert_eq!(updated.content_preview, "hello there");
        assert_eq!(updated.content_full.as_deref(), Some("hello there"));
        assert_eq!(updated.char_count, 11);
        assert_eq!(updated.byte_size, "hello there".len() as i64);

        update_text_content(&conn, id, "").unwrap();
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn db_maintenance_helpers_run() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        db_optimize(&conn).unwrap();
        db_vacuum(&conn).unwrap();
    }

    #[test]
    fn search_scans_html_rtf_and_file_paths() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let html_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Html,
                content_preview: "shared preview".into(),
                content_full: Some("shared full".into()),
                rtf_content: None,
                html: Some("<div>needle html</div>".into()),
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 21,
                hash: "html_hash".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        let rtf_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Rtf,
                content_preview: "shared preview".into(),
                content_full: None,
                rtf_content: Some("needle rtf content".into()),
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 18,
                hash: "rtf_hash".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        let file_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::File,
                content_preview: "shared preview".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: Some(vec!["C:\\tmp\\needle-file.txt".into()]),
                byte_size: 0,
                hash: "file_hash".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        let search = |kind: ContentKind| {
            list_items(
                &conn,
                &ClipboardListQuery {
                    filter: ClipboardFilter::All,
                    search: String::new(),
                    search_payload: Some(ClipboardSearchPayload {
                        keywords: vec!["needle".into()],
                        filters: ClipboardSearchFilters {
                            kind: Some(kind),
                            ..ClipboardSearchFilters::default()
                        },
                    }),
                    group_id: None,
                    pinned_only: false,
                    op_type: None,
                    op_from_ms: None,
                    op_to_ms: None,
                    op_app: None,
                    op_fav_only: false,
                    op_size_gt: None,
                    op_size_lt: None,
                    offset: 0,
                    limit: 10,
                },
            )
        };

        let html_hits = search(ContentKind::Html).unwrap();
        assert_eq!(html_hits.total, 1);
        assert_eq!(html_hits.items[0].id, html_id);

        let rtf_hits = search(ContentKind::Rtf).unwrap();
        assert_eq!(rtf_hits.total, 1);
        assert_eq!(rtf_hits.items[0].id, rtf_id);

        let file_hits = search(ContentKind::File).unwrap();
        assert_eq!(file_hits.total, 1);
        assert_eq!(file_hits.items[0].id, file_id);
    }

    #[test]
    fn op_type_queries_keep_kind_specific_payloads() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let html_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Html,
                content_preview: "html preview".into(),
                content_full: Some("html text".into()),
                rtf_content: None,
                html: Some("<p>html text</p>".into()),
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 9,
                hash: "task4-html".into(),
                source_app: Some("Chrome".into()),
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        let rtf_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Rtf,
                content_preview: "rtf preview".into(),
                content_full: Some("rtf text".into()),
                rtf_content: Some("{\\rtf1\\ansi rtf text}".into()),
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 8,
                hash: "task4-rtf".into(),
                source_app: Some("Word".into()),
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        let file_id = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::File,
                content_preview: "files".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: Some(vec!["C:\\tmp\\task4.txt".into()]),
                byte_size: 0,
                hash: "task4-file".into(),
                source_app: Some("Explorer".into()),
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();

        let list_for = |op_type: &str| {
            list_items(
                &conn,
                &ClipboardListQuery {
                    filter: ClipboardFilter::All,
                    search: String::new(),
                    search_payload: None,
                    group_id: None,
                    pinned_only: false,
                    op_type: Some(op_type.into()),
                    op_from_ms: None,
                    op_to_ms: None,
                    op_app: None,
                    op_fav_only: false,
                    op_size_gt: None,
                    op_size_lt: None,
                    offset: 0,
                    limit: 10,
                },
            )
            .unwrap()
        };

        let html_items = list_for("html");
        assert_eq!(html_items.total, 1);
        assert_eq!(html_items.items[0].id, html_id);
        assert_eq!(
            html_items.items[0].html.as_deref(),
            Some("<p>html text</p>")
        );

        let rtf_items = list_for("rtf");
        assert_eq!(rtf_items.total, 1);
        assert_eq!(rtf_items.items[0].id, rtf_id);
        assert_eq!(
            rtf_items.items[0].rtf_content.as_deref(),
            Some("{\\rtf1\\ansi rtf text}")
        );

        let file_items = list_for("file");
        assert_eq!(file_items.total, 1);
        assert_eq!(file_items.items[0].id, file_id);
        assert_eq!(
            file_items.items[0].file_paths.as_deref(),
            Some(&["C:\\tmp\\task4.txt".to_string()][..])
        );
    }

    #[test]
    fn insert_and_list_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let item = NewItem {
            kind: ContentKind::Text,
            content_preview: "hello".into(),
            content_full: Some("hello world".into()),
            rtf_content: None,
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: 11,
            hash: "abc".into(),
            source_app: None,
            source_app_icon: None,
            from_self: false,
        };
        let id = insert_item(&conn, &item).unwrap();
        assert!(id > 0);

        let q = ClipboardListQuery {
            filter: ClipboardFilter::All,
            search: String::new(),
            search_payload: None,
            group_id: None,
            pinned_only: false,
            op_type: None,
            op_from_ms: None,
            op_to_ms: None,
            op_app: None,
            op_fav_only: false,
            op_size_gt: None,
            op_size_lt: None,
            offset: 0,
            limit: 10,
        };
        let result = list_items(&conn, &q).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].content_preview, "hello");
        assert_eq!(result.items[0].char_count, 11);
        assert_eq!(result.items[0].rtf_content, None);
        assert_eq!(result.items[0].source_app_icon, None);
        assert_eq!(result.items[0].group_id, None);
        assert!(!result.items[0].is_pinned);
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
                    rtf_content: None,
                    html: None,
                    image_path: None,
                    image_width: None,
                    image_height: None,
                    file_paths: None,
                    byte_size: text.len() as i64,
                    hash: format!("hash_{i}"),
                    source_app: None,
                    source_app_icon: None,
                    from_self: false,
                },
            )
            .unwrap();
        }

        let q = ClipboardListQuery {
            filter: ClipboardFilter::All,
            search: "ana".into(),
            search_payload: None,
            group_id: None,
            pinned_only: false,
            op_type: None,
            op_from_ms: None,
            op_to_ms: None,
            op_app: None,
            op_fav_only: false,
            op_size_gt: None,
            op_size_lt: None,
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
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "h_fav".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        let item = toggle_favorite(&conn, id).unwrap();
        assert!(item.is_favorite);
        let item = toggle_favorite(&conn, id).unwrap();
        assert!(!item.is_favorite);
    }

    #[test]
    fn favoriting_preserves_time_order_in_all_filter() {
        // Favoriting an item must NOT move it in the "All" view — list stays
        // purely time-ordered; is_favorite is only a flag.
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        for (i, text) in ["old", "middle", "new"].iter().enumerate() {
            insert_item(
                &conn,
                &NewItem {
                    kind: ContentKind::Text,
                    content_preview: (*text).to_string(),
                    content_full: None,
                    rtf_content: None,
                    html: None,
                    image_path: None,
                    image_width: None,
                    image_height: None,
                    file_paths: None,
                    byte_size: 0,
                    hash: format!("h_{i}"),
                    source_app: None,
                    source_app_icon: None,
                    from_self: false,
                },
            )
            .unwrap();
        }
        // Favorite the "old" (id=1, oldest) item.
        toggle_favorite(&conn, 1).unwrap();

        let q = ClipboardListQuery {
            filter: ClipboardFilter::All,
            search: String::new(),
            search_payload: None,
            group_id: None,
            pinned_only: false,
            op_type: None,
            op_from_ms: None,
            op_to_ms: None,
            op_app: None,
            op_fav_only: false,
            op_size_gt: None,
            op_size_lt: None,
            offset: 0,
            limit: 10,
        };
        let items = list_items(&conn, &q).unwrap().items;
        assert_eq!(items.len(), 3);
        // Newest first; favorited "old" stays at bottom (its original position).
        assert_eq!(items[0].content_preview, "new");
        assert_eq!(items[1].content_preview, "middle");
        assert_eq!(items[2].content_preview, "old");
        assert!(items[2].is_favorite);
    }

    #[test]
    fn reorder_favorites_updates_sort_index() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let a = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "a".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "ha".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        let b = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "b".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "hb".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        // Mark both as favorites
        conn.execute(
            "UPDATE clipboard_items SET is_favorite = 1 WHERE id IN (?1, ?2)",
            params![a, b],
        )
        .unwrap();

        reorder_favorites(&mut conn, &[b, a]).unwrap();

        let idx_b: i64 = conn
            .query_row(
                "SELECT favorite_sort_index FROM clipboard_items WHERE id = ?1",
                params![b],
                |r| r.get(0),
            )
            .unwrap();
        let idx_a: i64 = conn
            .query_row(
                "SELECT favorite_sort_index FROM clipboard_items WHERE id = ?1",
                params![a],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx_b, 0);
        assert_eq!(idx_a, 1);
    }

    #[test]
    fn reorder_favorites_skips_non_favorites() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let non_fav = insert_item(
            &conn,
            &NewItem {
                kind: ContentKind::Text,
                content_preview: "x".into(),
                content_full: None,
                rtf_content: None,
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size: 0,
                hash: "hx".into(),
                source_app: None,
                source_app_icon: None,
                from_self: false,
            },
        )
        .unwrap();
        // is_favorite stays 0
        reorder_favorites(&mut conn, &[non_fav]).unwrap();
        let idx: Option<i64> = conn
            .query_row(
                "SELECT favorite_sort_index FROM clipboard_items WHERE id = ?1",
                params![non_fav],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            idx.is_none(),
            "non-favorite should not receive a sort index"
        );
    }

    #[test]
    fn op_type_and_size_filters_rows() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let mk = |kind: ContentKind, hash: &str, bytes: i64| NewItem {
            kind,
            content_preview: "x".into(),
            content_full: None,
            rtf_content: None,
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: bytes,
            hash: hash.into(),
            source_app: None,
            source_app_icon: None,
            from_self: false,
        };
        insert_item(&conn, &mk(ContentKind::Text, "t1", 100)).unwrap();
        insert_item(&conn, &mk(ContentKind::Image, "i1", 5_000)).unwrap();
        insert_item(&conn, &mk(ContentKind::Image, "i2", 50_000)).unwrap();

        let q = ClipboardListQuery {
            filter: ClipboardFilter::All,
            search: String::new(),
            search_payload: None,
            group_id: None,
            pinned_only: false,
            op_type: Some("image".into()),
            op_size_gt: Some(10_000),
            op_size_lt: None,
            op_from_ms: None,
            op_to_ms: None,
            op_app: None,
            op_fav_only: false,
            offset: 0,
            limit: 10,
        };
        let result = list_items(&conn, &q).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].hash, "i2");
    }
}
