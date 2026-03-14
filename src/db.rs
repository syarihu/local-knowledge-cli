use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

use crate::keywords;
use crate::now_iso;

pub struct Entry {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub category: String,
    pub source_file: Option<String>,
    #[allow(dead_code)]
    pub file_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct DbStats {
    pub total: i64,
    pub shared: i64,
    pub local: i64,
    pub keywords: i64,
}

pub fn init_db(db_path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS entries (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL,
            category    TEXT NOT NULL DEFAULT 'local',
            source_file TEXT,
            file_hash   TEXT,
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
        CREATE INDEX IF NOT EXISTS idx_entries_source_file ON entries(source_file);

        CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
            title,
            content,
            content='entries',
            content_rowid='id'
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

    Ok(conn)
}

pub fn open_db(db_path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    if !db_path.exists() {
        return Err(format!(
            "Knowledge DB not found at {}. Run 'lk init' first.",
            db_path.display()
        )
        .into());
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

pub fn add_entry(
    conn: &Connection,
    title: &str,
    content: &str,
    kws: &[String],
    category: &str,
    source_file: Option<&str>,
    file_hash: Option<&str>,
) -> Result<i64, Box<dyn std::error::Error>> {
    let now = now_iso();

    // Auto-extract keywords if none provided
    let auto_kws;
    let final_kws = if kws.is_empty() {
        auto_kws = keywords::extract_keywords(title, content);
        &auto_kws
    } else {
        kws
    };

    conn.execute(
        "INSERT INTO entries (title, content, category, source_file, file_hash, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![title, content, category, source_file, file_hash, now, now],
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

pub fn search_entries(
    conn: &Connection,
    query: &str,
    keyword_only: bool,
    category: Option<&str>,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();

    // Helper to append optional filters and return next param index
    fn append_filters(
        sql: &mut String,
        params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
        category: Option<&str>,
        since: Option<&str>,
    ) {
        if let Some(cat) = category {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND e.category = ?{idx}"));
            params.push(Box::new(cat.to_string()));
        }
        if let Some(s) = since {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND e.updated_at >= ?{idx}"));
            params.push(Box::new(s.to_string()));
        }
    }

    if keyword_only {
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut sql = String::from(
            "SELECT DISTINCT e.id, e.title, e.content, e.category, e.source_file, e.file_hash, e.created_at, e.updated_at \
             FROM entries e JOIN keywords k ON e.id = k.entry_id WHERE (",
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

        append_filters(&mut sql, &mut param_values, category, since);
        sql.push_str(" ORDER BY e.updated_at DESC LIMIT ?");
        param_values.push(Box::new(limit as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(Entry {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                source_file: row.get(4)?,
                file_hash: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        for row in rows {
            results.push(row?);
        }
    } else {
        // FTS search
        let mut fts_sql = String::from(
            "SELECT e.id, e.title, e.content, e.category, e.source_file, e.file_hash, e.created_at, e.updated_at \
             FROM entries_fts fts JOIN entries e ON fts.rowid = e.id WHERE entries_fts MATCH ?1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(query.to_string())];

        append_filters(&mut fts_sql, &mut param_values, category, since);
        fts_sql.push_str(" ORDER BY rank, e.updated_at DESC LIMIT ?");
        param_values.push(Box::new(limit as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        if let Ok(mut stmt) = conn.prepare(&fts_sql) {
            if let Ok(rows) = stmt.query_map(params_ref.as_slice(), |row| {
                Ok(Entry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    content: row.get(2)?,
                    category: row.get(3)?,
                    source_file: row.get(4)?,
                    file_hash: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            }) {
                for row in rows {
                    if let Ok(entry) = row {
                        results.push(entry);
                    }
                }
            }
        }

        // Supplement with keyword search if needed
        if results.len() < limit {
            let seen_ids: std::collections::HashSet<i64> =
                results.iter().map(|r| r.id).collect();
            let remaining = limit - results.len();

            let words: Vec<&str> = query.split_whitespace().collect();
            let mut kw_sql = String::from(
                "SELECT DISTINCT e.id, e.title, e.content, e.category, e.source_file, e.file_hash, e.created_at, e.updated_at \
                 FROM entries e JOIN keywords k ON e.id = k.entry_id WHERE (",
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

            append_filters(&mut kw_sql, &mut kw_params, category, since);
            kw_sql.push_str(" ORDER BY e.updated_at DESC LIMIT ?");
            kw_params.push(Box::new(remaining as i64));

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                kw_params.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&kw_sql)?;
            let rows = stmt.query_map(params_ref.as_slice(), |row| {
                Ok(Entry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    content: row.get(2)?,
                    category: row.get(3)?,
                    source_file: row.get(4)?,
                    file_hash: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?;
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
    let mut stmt = conn.prepare(
        "SELECT id, title, content, category, source_file, file_hash, created_at, updated_at FROM entries WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Entry {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            category: row.get(3)?,
            source_file: row.get(4)?,
            file_hash: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    match rows.next() {
        Some(Ok(entry)) => Ok(Some(entry)),
        _ => Ok(None),
    }
}

pub fn get_keywords(conn: &Connection, entry_id: i64) -> Result<Vec<String>, Box<dyn std::error::Error>> {
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

pub fn delete_entries_by_category(conn: &Connection, category: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let count = conn.execute("DELETE FROM entries WHERE category = ?1", params![category])?;
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
    }
    Ok(())
}

pub fn list_entries(
    conn: &Connection,
    category: Option<&str>,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    let sql = if category.is_some() {
        "SELECT id, title, content, category, source_file, file_hash, created_at, updated_at FROM entries WHERE category = ?1 ORDER BY updated_at DESC"
    } else {
        "SELECT id, title, content, category, source_file, file_hash, created_at, updated_at FROM entries ORDER BY updated_at DESC"
    };

    let mut stmt = conn.prepare(sql)?;
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

pub fn list_local_entries_with_content(
    conn: &Connection,
) -> Result<Vec<Entry>, Box<dyn std::error::Error>> {
    list_entries(conn, Some("local"))
}

pub fn get_shared_file_hashes(
    conn: &Connection,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT source_file, file_hash FROM entries WHERE category = 'shared' AND source_file IS NOT NULL",
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

pub fn delete_entries_by_source(
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
        "UPDATE entries SET category = 'shared', source_file = ?1, file_hash = ?2, updated_at = ?3 WHERE id = ?4",
        params![source_file, file_hash, updated_at, id],
    )?;
    Ok(())
}

pub fn keyword_counts(
    conn: &Connection,
) -> Result<Vec<(String, i64)>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT keyword, COUNT(*) as count FROM keywords GROUP BY keyword ORDER BY count DESC",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_stats(conn: &Connection) -> Result<DbStats, Box<dyn std::error::Error>> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;
    let shared: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE category = 'shared'",
        [],
        |r| r.get(0),
    )?;
    let local: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE category = 'local'",
        [],
        |r| r.get(0),
    )?;
    let keywords: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT keyword) FROM keywords",
        [],
        |r| r.get(0),
    )?;
    Ok(DbStats { total, shared, local, keywords })
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        category: row.get(3)?,
        source_file: row.get(4)?,
        file_hash: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
