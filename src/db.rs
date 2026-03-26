use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;

use crate::keywords;
use crate::util::now_iso;

const ENTRY_COLS: &str = "id, title, content, category, source, source_file, file_hash, status, uid, superseded_by, supersedes, created_at, updated_at";
const ENTRY_COLS_E: &str = "e.id, e.title, e.content, e.category, e.source, e.source_file, e.file_hash, e.status, e.uid, e.superseded_by, e.supersedes, e.created_at, e.updated_at";

pub const VALID_STATUSES: &[&str] = &["active", "deprecated", "proposed", "accepted", "superseded"];

pub fn is_valid_status(s: &str) -> bool {
    VALID_STATUSES.contains(&s)
}

pub struct Entry {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub category: String,
    pub source: String,
    pub source_file: Option<String>,
    #[allow(dead_code)]
    pub file_hash: Option<String>,
    pub status: String,
    pub uid: String,
    pub superseded_by: Option<String>,
    pub supersedes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub rank: Option<f64>,
}

pub struct DbStats {
    pub total: i64,
    pub shared: i64,
    pub local: i64,
    pub keywords: i64,
}

/// Current schema version. Increment when adding new migrations.
const SCHEMA_VERSION: i64 = 5;

pub fn init_db(db_path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS entries (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL,
            category    TEXT NOT NULL DEFAULT '',
            source      TEXT NOT NULL DEFAULT 'local',
            source_file TEXT,
            file_hash   TEXT,
            status      TEXT NOT NULL DEFAULT 'active',
            uid         TEXT NOT NULL DEFAULT '',
            superseded_by TEXT,
            supersedes  TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS keywords (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
            keyword  TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_keywords_keyword ON keywords(keyword);
        CREATE INDEX IF NOT EXISTS idx_keywords_entry_id ON keywords(entry_id);
        CREATE INDEX IF NOT EXISTS idx_entries_category ON entries(category);
        CREATE INDEX IF NOT EXISTS idx_entries_source ON entries(source);
        CREATE INDEX IF NOT EXISTS idx_entries_source_file ON entries(source_file);
        CREATE INDEX IF NOT EXISTS idx_entries_source_status ON entries(source, status);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_entries_uid ON entries(uid) WHERE uid != '';

        CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
            title,
            content,
            content='entries',
            content_rowid='id',
            tokenize='trigram'
        );

        CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
            INSERT INTO entries_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
        END;

        CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
            INSERT INTO entries_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
        END;
        ",
    )?;

    // Set initial schema version for new databases
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
    if count == 0 {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )?;
    }

    Ok(conn)
}

pub fn open_db(db_path: &Path) -> Result<(Connection, bool), Box<dyn std::error::Error>> {
    if !db_path.exists() {
        return Err(format!(
            "Knowledge DB not found at {}. Run 'lk init' first.",
            db_path.display()
        )
        .into());
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )?;
    let migrated = migrate(db_path, &conn)?;
    if migrated {
        // Keep only the 3 most recent backups
        if let Ok(removed) = cleanup_backups(db_path, 3)
            && removed > 0
        {
            eprintln!("Note: Removed {removed} old backup(s).");
        }
    }
    Ok((conn, migrated))
}

/// Get current schema version from DB (0 if table doesn't exist yet).
fn get_schema_version(conn: &Connection) -> i64 {
    // Check if schema_version table exists
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);

    if !has_table {
        return 0;
    }

    conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
        r.get(0)
    })
    .unwrap_or(0)
}

fn set_schema_version(conn: &Connection, version: i64) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);")?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
    if count == 0 {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![version],
        )?;
    } else {
        conn.execute("UPDATE schema_version SET version = ?1", params![version])?;
    }
    Ok(())
}

/// Create a backup of the DB file before migration.
pub fn backup_db(db_path: &Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let timestamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
        .replace(':', "-");
    let backup_name = format!("knowledge.db.bak.{timestamp}");
    let backup_path = db_path.with_file_name(backup_name);
    std::fs::copy(db_path, &backup_path)?;
    Ok(backup_path)
}

/// Remove old backup files, keeping only the most recent `keep` backups.
pub fn cleanup_backups(db_path: &Path, keep: usize) -> Result<usize, Box<dyn std::error::Error>> {
    let parent = db_path.parent().ok_or("no parent directory")?;
    let mut backups: Vec<std::path::PathBuf> = std::fs::read_dir(parent)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("knowledge.db.bak."))
        })
        .collect();

    if backups.len() <= keep {
        return Ok(0);
    }

    // Sort by filename (contains timestamp) descending — newest first
    backups.sort();
    backups.reverse();

    let mut removed = 0;
    for old in &backups[keep..] {
        std::fs::remove_file(old)?;
        removed += 1;
    }
    Ok(removed)
}

fn migrate(db_path: &Path, conn: &Connection) -> Result<bool, Box<dyn std::error::Error>> {
    let current_version = get_schema_version(conn);

    if current_version >= SCHEMA_VERSION {
        return Ok(false);
    }

    // For legacy DBs without schema_version table, detect version from schema
    let effective_version = if current_version == 0 {
        let schema: String = conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='entries'")?
            .query_row([], |row| row.get(0))?;
        if schema.contains("uid") {
            // Has all columns including uid — schema version 5
            set_schema_version(conn, SCHEMA_VERSION)?;
            return Ok(false);
        } else if schema.contains("status") {
            4 // Has status but not uid — needs migration 5
        } else if schema.contains("source ") {
            1 // Has source but not status
        } else {
            0 // Original schema
        }
    } else {
        current_version
    };

    // Back up before migration
    let backup_path = backup_db(db_path)?;
    eprintln!("Note: DB backed up to {}", backup_path.display());

    let mut migrated = false;

    // Migration 1: Add source column (version 0 -> 1)
    if effective_version < 1 {
        conn.execute_batch(
            "ALTER TABLE entries ADD COLUMN source TEXT NOT NULL DEFAULT 'local';
             CREATE INDEX IF NOT EXISTS idx_entries_source ON entries(source);
             UPDATE entries SET source = 'shared', category = '' WHERE category = 'shared';
             UPDATE entries SET source = 'local', category = '' WHERE category = 'local';
             DELETE FROM entries WHERE source = 'shared';",
        )?;
        migrated = true;
    }

    // Migration 2: Add status and superseded_by columns (version 1 -> 2)
    if effective_version < 2 {
        conn.execute_batch(
            "ALTER TABLE entries ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
             ALTER TABLE entries ADD COLUMN superseded_by INTEGER;",
        )?;
        migrated = true;
    }

    // Migration 3: Add schema_version table and busy_timeout (version 2 -> 3)
    // (schema_version table creation and busy_timeout are handled above)

    // Migration 4: Rebuild FTS with trigram tokenizer for CJK support (version 3 -> 4)
    if effective_version < 4 {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS entries_ai;
             DROP TRIGGER IF EXISTS entries_ad;
             DROP TRIGGER IF EXISTS entries_au;
             DROP TABLE IF EXISTS entries_fts;
             CREATE VIRTUAL TABLE entries_fts USING fts5(
                 title, content,
                 content='entries', content_rowid='id',
                 tokenize='trigram'
             );
             INSERT INTO entries_fts(rowid, title, content) SELECT id, title, content FROM entries;
             CREATE TRIGGER entries_ai AFTER INSERT ON entries BEGIN
                 INSERT INTO entries_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
             END;
             CREATE TRIGGER entries_ad AFTER DELETE ON entries BEGIN
                 INSERT INTO entries_fts(entries_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
             END;
             CREATE TRIGGER entries_au AFTER UPDATE ON entries BEGIN
                 INSERT INTO entries_fts(entries_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
                 INSERT INTO entries_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
             END;",
        )?;
        migrated = true;
    }

    // Migration 5: Add uid, supersedes columns; change superseded_by to TEXT (version 4 -> 5)
    if effective_version < 5 {
        // Step 1: Read existing superseded_by relationships (as integers)
        let mut supersede_pairs: Vec<(i64, i64)> = Vec::new();
        {
            let mut stmt = conn
                .prepare("SELECT id, superseded_by FROM entries WHERE superseded_by IS NOT NULL")?;
            let rows =
                stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
            for row in rows {
                supersede_pairs.push(row?);
            }
        }

        // Step 2: Drop FTS triggers (they reference the old table)
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS entries_ai;
             DROP TRIGGER IF EXISTS entries_ad;
             DROP TRIGGER IF EXISTS entries_au;",
        )?;

        // Step 3: Create new table with uid, supersedes, and TEXT superseded_by
        conn.execute_batch(
            "CREATE TABLE entries_new (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                title       TEXT NOT NULL,
                content     TEXT NOT NULL,
                category    TEXT NOT NULL DEFAULT '',
                source      TEXT NOT NULL DEFAULT 'local',
                source_file TEXT,
                file_hash   TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                uid         TEXT NOT NULL DEFAULT '',
                superseded_by TEXT,
                supersedes  TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;

        // Step 4: Copy data (without superseded_by for now)
        conn.execute_batch(
            "INSERT INTO entries_new (id, title, content, category, source, source_file, file_hash, status, uid, created_at, updated_at)
             SELECT id, title, content, category, source, source_file, file_hash, status, '', created_at, updated_at
             FROM entries;",
        )?;

        // Step 5: Generate UIDs for all entries (with collision avoidance)
        let mut id_uid_map: HashMap<i64, String> = HashMap::new();
        {
            let mut used_uids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut stmt = conn.prepare("SELECT id FROM entries_new")?;
            let ids: Vec<i64> = stmt.query_map([], |row| row.get(0))?.flatten().collect();
            for id in ids {
                let mut uid = generate_uid();
                while used_uids.contains(&uid) {
                    uid = generate_uid();
                }
                used_uids.insert(uid.clone());
                conn.execute(
                    "UPDATE entries_new SET uid = ?1 WHERE id = ?2",
                    params![uid, id],
                )?;
                id_uid_map.insert(id, uid);
            }
        }

        // Step 6: Convert superseded_by INTEGER -> UID TEXT
        for (entry_id, old_superseded_by_id) in &supersede_pairs {
            if let Some(uid) = id_uid_map.get(old_superseded_by_id) {
                conn.execute(
                    "UPDATE entries_new SET superseded_by = ?1 WHERE id = ?2",
                    params![uid, entry_id],
                )?;
            }
        }

        // Step 7: Swap tables (disable FK to avoid constraint errors during swap)
        conn.execute_batch(
            "PRAGMA foreign_keys=OFF;
             DROP TABLE entries;
             ALTER TABLE entries_new RENAME TO entries;
             PRAGMA foreign_keys=ON;",
        )?;

        // Step 8: Recreate indexes
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_keywords_keyword ON keywords(keyword);
             CREATE INDEX IF NOT EXISTS idx_keywords_entry_id ON keywords(entry_id);
             CREATE INDEX IF NOT EXISTS idx_entries_category ON entries(category);
             CREATE INDEX IF NOT EXISTS idx_entries_source ON entries(source);
             CREATE INDEX IF NOT EXISTS idx_entries_source_file ON entries(source_file);
             CREATE INDEX IF NOT EXISTS idx_entries_source_status ON entries(source, status);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_entries_uid ON entries(uid) WHERE uid != '';",
        )?;

        // Step 9: Recreate FTS table and triggers
        conn.execute_batch(
            "DROP TABLE IF EXISTS entries_fts;
             CREATE VIRTUAL TABLE entries_fts USING fts5(
                 title, content,
                 content='entries', content_rowid='id',
                 tokenize='trigram'
             );
             INSERT INTO entries_fts(rowid, title, content) SELECT id, title, content FROM entries;
             CREATE TRIGGER entries_ai AFTER INSERT ON entries BEGIN
                 INSERT INTO entries_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
             END;
             CREATE TRIGGER entries_ad AFTER DELETE ON entries BEGIN
                 INSERT INTO entries_fts(entries_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
             END;
             CREATE TRIGGER entries_au AFTER UPDATE ON entries BEGIN
                 INSERT INTO entries_fts(entries_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
                 INSERT INTO entries_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
             END;",
        )?;

        migrated = true;
    }

    set_schema_version(conn, SCHEMA_VERSION)?;
    Ok(migrated)
}

/// Generate a unique 12-character hex ID using SHA256 of timestamp + pid + counter.
pub fn generate_uid() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static UID_COUNTER: AtomicU64 = AtomicU64::new(0);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let counter = UID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let input = format!("{}-{}-{}", now.as_nanos(), pid, counter);
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..6]) // 6 bytes = 12 hex chars
}

#[allow(clippy::too_many_arguments)]
pub fn add_entry(
    conn: &Connection,
    title: &str,
    content: &str,
    kws: &[String],
    category: &str,
    source: &str,
    source_file: Option<&str>,
    file_hash: Option<&str>,
) -> Result<i64, Box<dyn std::error::Error>> {
    add_entry_full(
        conn,
        title,
        content,
        kws,
        category,
        source,
        source_file,
        file_hash,
        None,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_entry_full(
    conn: &Connection,
    title: &str,
    content: &str,
    kws: &[String],
    category: &str,
    source: &str,
    source_file: Option<&str>,
    file_hash: Option<&str>,
    uid: Option<&str>,
    status: Option<&str>,
    superseded_by: Option<&str>,
    supersedes: Option<&str>,
) -> Result<i64, Box<dyn std::error::Error>> {
    let now = now_iso();
    let uid = uid
        .filter(|u| !u.trim().is_empty())
        .map(String::from)
        .unwrap_or_else(generate_uid);
    let status = status.unwrap_or("active");

    // Auto-extract keywords if none provided
    let auto_kws;
    let final_kws = if kws.is_empty() {
        auto_kws = keywords::extract_keywords(title, content);
        &auto_kws
    } else {
        kws
    };

    conn.execute(
        "INSERT INTO entries (title, content, category, source, source_file, file_hash, uid, status, superseded_by, supersedes, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![title, content, category, source, source_file, file_hash, uid, status, superseded_by, supersedes, now, now],
    )?;
    let entry_id = conn.last_insert_rowid();

    for kw in final_kws {
        conn.execute(
            "INSERT INTO keywords (entry_id, keyword) VALUES (?1, ?2)",
            params![entry_id, kw.to_lowercase()],
        )?;
    }

    Ok(entry_id)
}

pub fn get_entry_by_uid(
    conn: &Connection,
    uid: &str,
) -> Result<Option<Entry>, Box<dyn std::error::Error>> {
    let sql = format!("SELECT {ENTRY_COLS} FROM entries WHERE uid = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![uid], row_to_entry)?;
    match rows.next() {
        Some(Ok(entry)) => Ok(Some(entry)),
        _ => Ok(None),
    }
}

pub fn search_entries(
    conn: &Connection,
    query: &str,
    keyword_only: bool,
    category: Option<&str>,
    source: Option<&str>,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();

    // Helper to append optional filters and return next param index
    fn append_filters(
        sql: &mut String,
        params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
        category: Option<&str>,
        source: Option<&str>,
        since: Option<&str>,
    ) {
        if let Some(cat) = category {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND e.category = ?{idx}"));
            params.push(Box::new(cat.to_string()));
        }
        if let Some(src) = source {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND e.source = ?{idx}"));
            params.push(Box::new(src.to_string()));
        }
        if let Some(s) = since {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND e.updated_at >= ?{idx}"));
            params.push(Box::new(s.to_string()));
        }
    }

    if keyword_only {
        let words = split_query_words(query);
        let mut sql = format!(
            "SELECT DISTINCT {ENTRY_COLS_E} \
             FROM entries e JOIN keywords k ON e.id = k.entry_id WHERE ("
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for (i, word) in words.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            let idx = i + 1;
            sql.push_str(&format!("k.keyword LIKE ?{idx}"));
            param_values.push(Box::new(format!("%{}%", word.to_lowercase())));
        }
        sql.push(')');

        append_filters(&mut sql, &mut param_values, category, source, since);
        sql.push_str(" ORDER BY e.updated_at DESC LIMIT ?");
        param_values.push(Box::new(limit as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), row_to_entry)?;
        for row in rows {
            results.push(row?);
        }
    } else {
        // FTS search — sanitize query to prevent FTS5 syntax injection
        let sanitized = sanitize_fts_query(query);

        let fts_sql_base = format!(
            "SELECT {ENTRY_COLS_E}, fts.rank \
             FROM entries_fts fts JOIN entries e ON fts.rowid = e.id WHERE entries_fts MATCH ?1"
        );
        let mut fts_sql = fts_sql_base;
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(sanitized)];

        append_filters(&mut fts_sql, &mut param_values, category, source, since);
        fts_sql.push_str(" ORDER BY rank, e.updated_at DESC LIMIT ?");
        param_values.push(Box::new(limit as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        match conn.prepare(&fts_sql) {
            Ok(mut stmt) => match stmt.query_map(params_ref.as_slice(), row_to_entry_with_rank) {
                Ok(rows) => {
                    for entry in rows.flatten() {
                        results.push(entry);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: FTS query failed: {e}");
                }
            },
            Err(e) => {
                eprintln!("Warning: FTS prepare failed (index may be corrupted): {e}");
            }
        }

        // Supplement with keyword search if needed
        if results.len() < limit {
            let seen_ids: std::collections::HashSet<i64> = results.iter().map(|r| r.id).collect();
            let remaining = limit - results.len();

            let words = split_query_words(query);
            let mut kw_sql = format!(
                "SELECT DISTINCT {ENTRY_COLS_E} \
                 FROM entries e JOIN keywords k ON e.id = k.entry_id WHERE ("
            );

            let mut kw_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            for (i, word) in words.iter().enumerate() {
                if i > 0 {
                    kw_sql.push_str(" OR ");
                }
                let idx = i + 1;
                kw_sql.push_str(&format!("k.keyword LIKE ?{idx}"));
                kw_params.push(Box::new(format!("%{}%", word.to_lowercase())));
            }
            kw_sql.push(')');

            append_filters(&mut kw_sql, &mut kw_params, category, source, since);
            kw_sql.push_str(" ORDER BY e.updated_at DESC LIMIT ?");
            kw_params.push(Box::new(remaining as i64));

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                kw_params.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&kw_sql)?;
            let rows = stmt.query_map(params_ref.as_slice(), row_to_entry)?;
            for row in rows {
                let entry = row?;
                if !seen_ids.contains(&entry.id) {
                    results.push(entry);
                }
            }
        }

        // LIKE fallback for short queries (e.g. 2-char CJK words) that trigram FTS cannot match
        if results.len() < limit {
            let seen_ids: std::collections::HashSet<i64> = results.iter().map(|r| r.id).collect();
            let remaining = limit - results.len();

            let words = split_query_words(query);
            let mut like_sql = format!("SELECT {ENTRY_COLS} FROM entries e WHERE (");
            let mut like_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            for (i, word) in words.iter().enumerate() {
                if i > 0 {
                    like_sql.push_str(" OR ");
                }
                let idx = i * 2 + 1;
                like_sql.push_str(&format!(
                    "e.title LIKE ?{idx} OR e.content LIKE ?{}",
                    idx + 1
                ));
                let pattern = format!("%{word}%");
                like_params.push(Box::new(pattern.clone()));
                like_params.push(Box::new(pattern));
            }
            like_sql.push(')');

            append_filters(&mut like_sql, &mut like_params, category, source, since);
            like_sql.push_str(" ORDER BY e.updated_at DESC LIMIT ?");
            like_params.push(Box::new(remaining as i64));

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                like_params.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&like_sql)?;
            let rows = stmt.query_map(params_ref.as_slice(), row_to_entry)?;
            for row in rows {
                let entry = row?;
                if !seen_ids.contains(&entry.id) {
                    results.push(entry);
                }
            }
        }
    }

    // Deduplicate by title, keeping the newest (first) entry
    let mut seen_titles = std::collections::HashSet::new();
    results.retain(|e| seen_titles.insert(e.title.to_lowercase()));

    Ok(results)
}

pub fn get_entry(conn: &Connection, id: i64) -> Result<Option<Entry>, Box<dyn std::error::Error>> {
    let sql = format!("SELECT {ENTRY_COLS} FROM entries WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], row_to_entry)?;
    match rows.next() {
        Some(Ok(entry)) => Ok(Some(entry)),
        _ => Ok(None),
    }
}

pub fn get_keywords(
    conn: &Connection,
    entry_id: i64,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare("SELECT keyword FROM keywords WHERE entry_id = ?1")?;
    let rows = stmt.query_map(params![entry_id], |row| row.get::<_, String>(0))?;
    let mut kws = Vec::new();
    for row in rows {
        kws.push(row?);
    }
    Ok(kws)
}

pub fn delete_entry(conn: &Connection, id: i64) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn delete_entries_by_category(
    conn: &Connection,
    category: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let count = conn.execute("DELETE FROM entries WHERE category = ?1", params![category])?;
    Ok(count)
}

pub fn purge_by_source(
    conn: &Connection,
    source: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let count = conn.execute("DELETE FROM entries WHERE source = ?1", params![source])?;
    Ok(count)
}

pub fn update_entry(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    content: Option<&str>,
    keywords: Option<&[String]>,
    now: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(t) = title {
        conn.execute(
            "UPDATE entries SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![t, now, id],
        )?;
    }
    if let Some(c) = content {
        conn.execute(
            "UPDATE entries SET content = ?1, updated_at = ?2 WHERE id = ?3",
            params![c, now, id],
        )?;
    }
    if let Some(kws) = keywords {
        conn.execute_batch("SAVEPOINT update_keywords")?;
        match (|| -> Result<(), Box<dyn std::error::Error>> {
            conn.execute("DELETE FROM keywords WHERE entry_id = ?1", params![id])?;
            for kw in kws {
                conn.execute(
                    "INSERT INTO keywords (entry_id, keyword) VALUES (?1, ?2)",
                    params![id, kw],
                )?;
            }
            conn.execute(
                "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        })() {
            Ok(()) => conn.execute_batch("RELEASE update_keywords")?,
            Err(e) => {
                conn.execute_batch("ROLLBACK TO update_keywords").ok();
                return Err(e);
            }
        }
    }
    Ok(())
}

pub fn update_entry_status(
    conn: &Connection,
    id: i64,
    status: &str,
    superseded_by: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = now_iso();
    conn.execute(
        "UPDATE entries SET status = ?1, superseded_by = ?2, updated_at = ?3 WHERE id = ?4",
        params![status, superseded_by, now, id],
    )?;
    Ok(())
}

pub fn update_entry_supersedes(
    conn: &Connection,
    id: i64,
    supersedes: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = now_iso();
    conn.execute(
        "UPDATE entries SET supersedes = ?1, updated_at = ?2 WHERE id = ?3",
        params![supersedes, now, id],
    )?;
    Ok(())
}

pub fn append_supersedes(existing: Option<&str>, new_uid: &str) -> String {
    let new_uid = new_uid.trim();
    let mut parts: Vec<String> = existing
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !new_uid.is_empty() && !parts.contains(&new_uid.to_string()) {
        parts.push(new_uid.to_string());
    }
    parts.join(",")
}

pub fn list_entries(
    conn: &Connection,
    category: Option<&str>,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let sql = if category.is_some() {
        format!("SELECT {ENTRY_COLS} FROM entries WHERE category = ?1 ORDER BY updated_at DESC")
    } else {
        format!("SELECT {ENTRY_COLS} FROM entries ORDER BY updated_at DESC")
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(cat) = category {
        stmt.query_map(params![cat], row_to_entry)?
    } else {
        stmt.query_map([], row_to_entry)?
    };

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

pub fn list_entries_by_source(
    conn: &Connection,
    source: &str,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let sql =
        format!("SELECT {ENTRY_COLS} FROM entries WHERE source = ?1 ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![source], row_to_entry)?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

pub fn list_entries_by_source_file(
    conn: &Connection,
    source_file: &str,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let sql = format!("SELECT {ENTRY_COLS} FROM entries WHERE source_file = ?1 ORDER BY id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![source_file], row_to_entry)?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

pub fn get_shared_file_hashes(
    conn: &Connection,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT source_file, file_hash FROM entries WHERE source = 'shared' AND source_file IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (file, hash) = row?;
        map.insert(file, hash);
    }
    Ok(map)
}

pub fn delete_entries_by_source_file(
    conn: &Connection,
    source_file: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "DELETE FROM entries WHERE source_file = ?1",
        params![source_file],
    )?;
    Ok(())
}

pub fn update_entry_to_shared(
    conn: &Connection,
    id: i64,
    source_file: &str,
    file_hash: &str,
    updated_at: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "UPDATE entries SET source = 'shared', source_file = ?1, file_hash = ?2, updated_at = ?3 WHERE id = ?4",
        params![source_file, file_hash, updated_at, id],
    )?;
    Ok(())
}

pub fn keyword_counts(conn: &Connection) -> Result<Vec<(String, i64)>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT keyword, COUNT(*) as count FROM keywords GROUP BY keyword ORDER BY count DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Public accessor for schema version (for stats --verbose).
pub fn get_schema_version_public(conn: &Connection) -> i64 {
    get_schema_version(conn)
}

pub fn get_stats(conn: &Connection) -> Result<DbStats, Box<dyn std::error::Error>> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;
    let shared: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE source = 'shared'",
        [],
        |r| r.get(0),
    )?;
    let local: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE source = 'local'",
        [],
        |r| r.get(0),
    )?;
    let keywords: i64 =
        conn.query_row("SELECT COUNT(DISTINCT keyword) FROM keywords", [], |r| {
            r.get(0)
        })?;
    Ok(DbStats {
        total,
        shared,
        local,
        keywords,
    })
}

pub fn find_similar_entries(
    conn: &Connection,
    title: &str,
    kws: &[String],
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // 1. Title exact match (case-insensitive)
    {
        let sql =
            format!("SELECT {ENTRY_COLS} FROM entries WHERE LOWER(title) = LOWER(?1) LIMIT 3");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![title], row_to_entry)?;
        for entry in rows.flatten() {
            seen_ids.insert(entry.id);
            results.push(entry);
        }
    }

    if results.len() >= 3 {
        results.truncate(3);
        return Ok(results);
    }

    // 2. FTS MATCH on title
    {
        let fts_query = title.to_string();
        let sql = format!(
            "SELECT {ENTRY_COLS_E} \
             FROM entries_fts fts JOIN entries e ON fts.rowid = e.id WHERE entries_fts MATCH ?1 ORDER BY rank LIMIT 3"
        );
        if let Ok(mut stmt) = conn.prepare(&sql)
            && let Ok(rows) = stmt.query_map(params![fts_query], row_to_entry)
        {
            for row in rows {
                if let Ok(entry) = row
                    && !seen_ids.contains(&entry.id)
                {
                    seen_ids.insert(entry.id);
                    results.push(entry);
                }
            }
        }
    }

    if results.len() >= 3 {
        results.truncate(3);
        return Ok(results);
    }

    // 3. Keyword overlap
    if !kws.is_empty() {
        let remaining = 3 - results.len();
        let placeholders: Vec<String> = kws
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT DISTINCT {ENTRY_COLS_E} \
             FROM entries e JOIN keywords k ON e.id = k.entry_id WHERE k.keyword IN ({}) \
             ORDER BY e.updated_at DESC LIMIT ?{}",
            placeholders.join(", "),
            kws.len() + 1
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = kws
            .iter()
            .map(|k| Box::new(k.to_lowercase()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        param_values.push(Box::new(remaining as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), row_to_entry)?;
        for row in rows {
            if let Ok(entry) = row
                && !seen_ids.contains(&entry.id)
            {
                seen_ids.insert(entry.id);
                results.push(entry);
            }
        }
    }

    results.truncate(3);
    Ok(results)
}

/// Sanitize a user query for FTS5 MATCH.
/// Wraps each word in double quotes to treat it as a literal phrase token,
/// preventing FTS5 operators (OR, NOT, NEAR, *, etc.) from being interpreted.
/// Split a query into words by whitespace, hyphens, underscores, and CamelCase boundaries.
/// This ensures queries like "auth-API", "feature_name", and "AuthAPI" are split into
/// individual tokens for better search coverage.
fn split_query_words(query: &str) -> Vec<&str> {
    let mut words = Vec::new();
    for segment in query.split(|c: char| c.is_whitespace() || c == '-' || c == '_') {
        if segment.is_empty() {
            continue;
        }
        // Split CamelCase: find boundaries where lowercase->uppercase transition occurs
        let bytes = segment.as_bytes();
        let mut start = 0;
        for i in 1..bytes.len() {
            let prev = bytes[i - 1] as char;
            let curr = bytes[i] as char;
            // Split at lowercase->uppercase boundary (e.g., "auth" | "API")
            // Also split at uppercase->uppercase+lowercase boundary (e.g., "API" | "Key")
            let split_here = (prev.is_lowercase() && curr.is_uppercase())
                || (i + 1 < bytes.len()
                    && prev.is_uppercase()
                    && curr.is_uppercase()
                    && (bytes[i + 1] as char).is_lowercase());
            if split_here {
                let part = &segment[start..i];
                if !part.is_empty() {
                    words.push(part);
                }
                start = i;
            }
        }
        let rest = &segment[start..];
        if !rest.is_empty() {
            words.push(rest);
        }
    }
    words
}

fn sanitize_fts_query(query: &str) -> String {
    let words: Vec<String> = split_query_words(query)
        .into_iter()
        .map(|w| {
            // Escape any embedded double quotes
            let escaped = w.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect();
    if words.is_empty() {
        "\"\"".to_string()
    } else {
        words.join(" ")
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        category: row.get(3)?,
        source: row.get(4)?,
        source_file: row.get(5)?,
        file_hash: row.get(6)?,
        status: row.get(7)?,
        uid: row.get(8)?,
        superseded_by: row.get(9)?,
        supersedes: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        rank: None,
    })
}

/// Build Entry from a row with rank at column index 13
fn row_to_entry_with_rank(row: &rusqlite::Row) -> rusqlite::Result<Entry> {
    let raw_rank: f64 = row.get(13)?;
    let score = 1.0 / (1.0 + raw_rank.abs());
    Ok(Entry {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        category: row.get(3)?,
        source: row.get(4)?,
        source_file: row.get(5)?,
        file_hash: row.get(6)?,
        status: row.get(7)?,
        uid: row.get(8)?,
        superseded_by: row.get(9)?,
        supersedes: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        rank: Some(score),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup_test_db() -> (Connection, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let conn = init_db(tmp.path()).unwrap();
        (conn, tmp)
    }

    #[test]
    fn test_init_db_creates_tables() {
        let (conn, _tmp) = setup_test_db();
        // Verify entries table exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='entries'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify schema_version table exists
        let sv_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sv_count, 1);

        // Verify schema version is set
        let version: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_busy_timeout_is_set() {
        let (conn, _tmp) = setup_test_db();
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn test_add_and_get_entry() {
        let (conn, _tmp) = setup_test_db();
        let kws = vec!["rust".to_string(), "test".to_string()];
        let id = add_entry(
            &conn,
            "Test",
            "Content here",
            &kws,
            "arch",
            "local",
            None,
            None,
        )
        .unwrap();
        assert!(id > 0);

        let entry = get_entry(&conn, id).unwrap().unwrap();
        assert_eq!(entry.title, "Test");
        assert_eq!(entry.content, "Content here");
        assert_eq!(entry.category, "arch");
        assert_eq!(entry.source, "local");
        assert_eq!(entry.status, "active");
    }

    #[test]
    fn test_get_keywords() {
        let (conn, _tmp) = setup_test_db();
        let kws = vec!["alpha".to_string(), "Beta".to_string()];
        let id = add_entry(&conn, "T", "C", &kws, "", "local", None, None).unwrap();
        let retrieved = get_keywords(&conn, id).unwrap();
        // Keywords are lowercased on insert
        assert!(retrieved.contains(&"alpha".to_string()));
        assert!(retrieved.contains(&"beta".to_string()));
    }

    #[test]
    fn test_delete_entry() {
        let (conn, _tmp) = setup_test_db();
        let id = add_entry(&conn, "Del", "To delete", &[], "", "local", None, None).unwrap();
        delete_entry(&conn, id).unwrap();
        assert!(get_entry(&conn, id).unwrap().is_none());
    }

    #[test]
    fn test_fts_search() {
        let (conn, _tmp) = setup_test_db();
        add_entry(
            &conn,
            "OAuth Login",
            "OAuth 2.0 PKCE flow for authentication",
            &["oauth".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();
        add_entry(
            &conn,
            "Database Schema",
            "SQLite with FTS5",
            &["database".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();

        let results = search_entries(&conn, "OAuth", false, None, None, None, 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].title, "OAuth Login");
    }

    #[test]
    fn test_keyword_search() {
        let (conn, _tmp) = setup_test_db();
        add_entry(
            &conn,
            "A",
            "content",
            &["mykey".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();

        let results = search_entries(&conn, "mykey", true, None, None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "A");
    }

    #[test]
    fn test_update_entry() {
        let (conn, _tmp) = setup_test_db();
        let id = add_entry(&conn, "Old", "old content", &[], "", "local", None, None).unwrap();
        let now = "2026-01-01T00:00:00".to_string();
        update_entry(
            &conn,
            id,
            Some("New Title"),
            Some("new content"),
            None,
            &now,
        )
        .unwrap();

        let entry = get_entry(&conn, id).unwrap().unwrap();
        assert_eq!(entry.title, "New Title");
        assert_eq!(entry.content, "new content");
    }

    #[test]
    fn test_update_entry_status() {
        let (conn, _tmp) = setup_test_db();
        let old_id = add_entry(&conn, "Old", "old", &[], "", "local", None, None).unwrap();
        let new_id = add_entry(&conn, "New", "new", &[], "", "local", None, None).unwrap();
        let new_entry = get_entry(&conn, new_id).unwrap().unwrap();
        update_entry_status(&conn, old_id, "deprecated", Some(&new_entry.uid)).unwrap();

        let entry = get_entry(&conn, old_id).unwrap().unwrap();
        assert_eq!(entry.status, "deprecated");
        assert_eq!(entry.superseded_by, Some(new_entry.uid));
    }

    #[test]
    fn test_fts_trigram_japanese() {
        let (conn, _tmp) = setup_test_db();
        add_entry(
            &conn,
            "認証フロー",
            "JWTトークンを使った認証フローの説明",
            &["auth".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();
        add_entry(
            &conn,
            "レート制限",
            "APIのレート制限は100リクエスト/分",
            &["rate-limit".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();

        // 3+ char Japanese query via trigram FTS
        let results = search_entries(&conn, "トークン", false, None, None, None, 10).unwrap();
        assert!(!results.is_empty(), "trigram should match 3+ char Japanese");
        assert_eq!(results[0].title, "認証フロー");

        // 2-char Japanese query falls back to LIKE
        let results = search_entries(&conn, "認証", false, None, None, None, 10).unwrap();
        assert!(
            !results.is_empty(),
            "LIKE fallback should match 2-char Japanese"
        );
        assert_eq!(results[0].title, "認証フロー");

        // Multi-word Japanese query
        let results = search_entries(&conn, "レート制限", false, None, None, None, 10).unwrap();
        assert!(!results.is_empty(), "should match multi-char Japanese");
    }

    #[test]
    fn test_find_similar_entries() {
        let (conn, _tmp) = setup_test_db();
        add_entry(
            &conn,
            "OAuth Flow",
            "OAuth details",
            &["oauth".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();

        let similar = find_similar_entries(&conn, "OAuth Flow", &["oauth".to_string()]).unwrap();
        assert!(!similar.is_empty());
    }

    #[test]
    fn test_stats() {
        let (conn, _tmp) = setup_test_db();
        add_entry(
            &conn,
            "A",
            "a",
            &["k1".to_string()],
            "",
            "local",
            None,
            None,
        )
        .unwrap();
        add_entry(
            &conn,
            "B",
            "b",
            &["k2".to_string()],
            "",
            "shared",
            Some("f.md"),
            Some("hash"),
        )
        .unwrap();

        let stats = get_stats(&conn).unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.local, 1);
        assert_eq!(stats.shared, 1);
        assert_eq!(stats.keywords, 2);
    }

    #[test]
    fn test_purge_by_source() {
        let (conn, _tmp) = setup_test_db();
        add_entry(&conn, "A", "a", &[], "", "local", None, None).unwrap();
        add_entry(&conn, "B", "b", &[], "", "local", None, None).unwrap();
        add_entry(&conn, "C", "c", &[], "", "shared", Some("f.md"), Some("h")).unwrap();

        let count = purge_by_source(&conn, "local").unwrap();
        assert_eq!(count, 2);
        let stats = get_stats(&conn).unwrap();
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn test_migration_from_legacy_db() {
        // Create a DB with the original schema (no source, no status)
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        conn.execute_batch(
            "CREATE TABLE entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT '',
                source_file TEXT,
                file_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE keywords (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
                keyword TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE entries_fts USING fts5(title, content, content='entries', content_rowid='id');",
        ).unwrap();
        conn.execute(
            "INSERT INTO entries (title, content, category) VALUES ('Test', 'Content', 'local')",
            [],
        )
        .unwrap();
        drop(conn);

        // Open with migration
        let (conn, migrated) = open_db(tmp.path()).unwrap();
        assert!(migrated);

        // Verify new columns exist
        let entry: String = conn
            .query_row("SELECT status FROM entries LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(entry, "active");

        // Verify schema version was set
        assert_eq!(get_schema_version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn test_backup_db() {
        let tmp = NamedTempFile::new().unwrap();
        let _conn = init_db(tmp.path()).unwrap();

        let backup_path = backup_db(tmp.path()).unwrap();
        assert!(backup_path.exists());
        // Clean up
        std::fs::remove_file(&backup_path).ok();
    }

    #[test]
    fn test_split_query_words_whitespace() {
        assert_eq!(split_query_words("auth API"), vec!["auth", "API"]);
    }

    #[test]
    fn test_split_query_words_hyphen() {
        assert_eq!(split_query_words("auth-API"), vec!["auth", "API"]);
    }

    #[test]
    fn test_split_query_words_underscore() {
        assert_eq!(split_query_words("auth_flow"), vec!["auth", "flow"]);
    }

    #[test]
    fn test_split_query_words_mixed_separators() {
        assert_eq!(
            split_query_words("auth-flow test"),
            vec!["auth", "flow", "test"]
        );
    }

    #[test]
    fn test_split_query_words_camel_case() {
        assert_eq!(split_query_words("AuthAPI"), vec!["Auth", "API"]);
        assert_eq!(split_query_words("authFlow"), vec!["auth", "Flow"]);
        assert_eq!(
            split_query_words("APIKeyManager"),
            vec!["API", "Key", "Manager"]
        );
    }

    #[test]
    fn test_split_query_words_camel_case_with_separators() {
        assert_eq!(
            split_query_words("AuthFlow-apiKey"),
            vec!["Auth", "Flow", "api", "Key"]
        );
    }

    #[test]
    fn test_split_query_words_empty() {
        assert!(split_query_words("").is_empty());
        assert!(split_query_words("  - _ ").is_empty());
    }

    #[test]
    fn test_sanitize_fts_query_splits_hyphens() {
        assert_eq!(sanitize_fts_query("auth-API"), "\"auth\" \"API\"");
    }

    #[test]
    fn test_sanitize_fts_query_splits_underscores() {
        assert_eq!(sanitize_fts_query("auth_flow"), "\"auth\" \"flow\"");
    }

    #[test]
    fn test_sanitize_fts_query_splits_camel_case() {
        assert_eq!(sanitize_fts_query("AuthAPI"), "\"Auth\" \"API\"");
    }

    #[test]
    fn test_search_finds_entry_with_hyphenated_query() {
        let (conn, _tmp) = setup_test_db();
        let kws = vec!["auth".to_string(), "api".to_string()];
        add_entry(
            &conn,
            "Auth API design",
            "Authentication API endpoint design",
            &kws,
            "arch",
            "local",
            None,
            None,
        )
        .unwrap();

        // Hyphenated query should still find the entry
        let results = search_entries(&conn, "auth-API", false, None, None, None, 10).unwrap();
        assert!(!results.is_empty(), "hyphenated query should find entry");
        assert!(results[0].title.contains("Auth"));
    }
}
