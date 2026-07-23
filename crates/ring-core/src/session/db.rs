use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::session::paths;
use crate::session::SessionMeta;

const SCHEMA_VERSION: i32 = 1;

fn db_path() -> PathBuf {
    paths::ring_home().join("state.sqlite")
}

type DbConn = Arc<Mutex<Connection>>;

static DB: OnceLock<DbConn> = OnceLock::new();

pub fn init() {
    DB.get_or_init(|| {
        let path = db_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&path).unwrap_or_else(|e| {
            panic!("Failed to open session DB at {}: {e}", path.display())
        });

        configure_pragmas(&conn);
        run_migrations(&conn);

        debug!("session DB initialized at {}", path.display());
        Arc::new(Mutex::new(conn))
    });
}

fn configure_pragmas(conn: &Connection) {
    const PRAGMAS: &[(&str, &str)] = &[
        ("journal_mode", "WAL"),
        ("synchronous", "NORMAL"),
        ("busy_timeout", "5000"),
        ("foreign_keys", "ON"),
        ("auto_vacuum", "INCREMENTAL"),
    ];
    for (key, val) in PRAGMAS {
        if let Err(e) = conn.pragma_update(None, key, val) {
            warn!("session DB: PRAGMA {key}={val} failed: {e}");
        }
    }
}

fn run_migrations(conn: &Connection) {
    let current: i32 = conn
        .query_row("SELECT user_version FROM pragma_user_version", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);

    if current >= SCHEMA_VERSION {
        debug!("session DB schema v{current}, up to date");
        return;
    }

    debug!("session DB migrating v{current} -> v{SCHEMA_VERSION}");

    if current < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS threads (
                id            TEXT PRIMARY KEY,
                title         TEXT,
                cwd           TEXT NOT NULL,
                created_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                model         TEXT,
                archived      INTEGER NOT NULL DEFAULT 0,
                compressed    INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_threads_updated_at
                ON threads (updated_at DESC);

            CREATE INDEX IF NOT EXISTS idx_threads_archived
                ON threads (archived);
            "#,
        )
        .expect("session DB migration v1 failed");
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .expect("failed to set user_version");
}

fn conn() -> &'static DbConn {
    DB.get().expect("session DB not initialized -- call init() first")
}

// ── 公共 API ──────────────────────────────────────────────────────────────────

pub fn upsert_thread(meta: &SessionMeta) {
    let c = conn();
    let guard = c.lock().unwrap();
    let id = meta.id.to_string();
    let title = meta.title.as_deref().unwrap_or("");
    let cwd = meta.cwd.to_string_lossy();
    let model = meta.model.as_deref().unwrap_or("");

    let _ = guard.execute(
        "INSERT OR REPLACE INTO threads
            (id, title, cwd, created_at, updated_at, message_count, model, archived, compressed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                 (SELECT archived   FROM threads WHERE id = ?1),
                 (SELECT compressed FROM threads WHERE id = ?1))",
        params![
            id,
            title,
            cwd,
            meta.created_at,
            meta.updated_at,
            meta.message_count as i64,
            model,
        ],
    );
}

pub fn delete_thread(id: Uuid) {
    let c = conn();
    let guard = c.lock().unwrap();
    let _ = guard.execute(
        "DELETE FROM threads WHERE id = ?1",
        params![id.to_string()],
    );
}

pub fn get_thread(id: Uuid) -> Option<SessionMeta> {
    let c = conn();
    let guard = c.lock().unwrap();
    guard
        .query_row(
            "SELECT id, title, cwd, created_at, updated_at, message_count, model
             FROM threads WHERE id = ?1",
            params![id.to_string()],
            row_to_meta,
        )
        .optional()
        .ok()
        .flatten()
}

pub fn list_threads(limit: i64) -> Vec<SessionMeta> {
    let c = conn();
    let Ok(guard) = c.lock() else {
        return Vec::new();
    };

    let Ok(mut stmt) = guard.prepare(
        "SELECT id, title, cwd, created_at, updated_at, message_count, model
         FROM threads
         WHERE archived = 0
         ORDER BY updated_at DESC
         LIMIT ?1",
    ) else {
        return Vec::new();
    };

    stmt.query_map(params![limit], row_to_meta)
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub fn search_threads(query: &str) -> Vec<SessionMeta> {
    let c = conn();
    let Ok(guard) = c.lock() else {
        return Vec::new();
    };

    let pattern = format!("%{query}%");

    let Ok(mut stmt) = guard.prepare(
        "SELECT id, title, cwd, created_at, updated_at, message_count, model
         FROM threads
         WHERE archived = 0 AND title LIKE ?1
         ORDER BY updated_at DESC
         LIMIT 100",
    ) else {
        return Vec::new();
    };

    stmt.query_map(params![pattern], row_to_meta)
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub fn set_compressed(id: Uuid, compressed: bool) {
    let c = conn();
    let guard = c.lock().unwrap();
    let _ = guard.execute(
        "UPDATE threads SET compressed = ?1 WHERE id = ?2",
        params![compressed as i64, id.to_string()],
    );
}

pub fn set_archived(id: Uuid, archived: bool) {
    let c = conn();
    let guard = c.lock().unwrap();
    let _ = guard.execute(
        "UPDATE threads SET archived = ?1 WHERE id = ?2",
        params![archived as i64, id.to_string()],
    );
}

pub fn count() -> usize {
    let c = conn();
    let guard = c.lock().unwrap();
    guard
        .query_row("SELECT COUNT(*) FROM threads", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|n| n as usize)
        .unwrap_or(0)
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn row_to_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMeta> {
    let id_str: String = row.get(0)?;
    let title: String = row.get(1)?;
    let cwd_str: String = row.get(2)?;
    let created_at: i64 = row.get(3)?;
    let updated_at: i64 = row.get(4)?;
    let message_count: i64 = row.get(5)?;
    let model: String = row.get(6)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_default();

    Ok(SessionMeta {
        id,
        title: if title.is_empty() { None } else { Some(title) },
        cwd: PathBuf::from(cwd_str),
        created_at,
        updated_at,
        message_count: message_count as usize,
        model: if model.is_empty() { None } else { Some(model) },
    })
}
