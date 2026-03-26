---
keywords: [database, sqlite, schema, fts, full-text-search]
category: features
---

# Database Schema & Operations

## Entry: Tables and Indexes
keywords: [entries, keywords, fts5, table, index]

The DB schema (defined in `init_db()` in `src/db.rs`) has 3 tables: `entries` (id, title, content, category, source, source_file, file_hash, status, uid, superseded_by, supersedes, created_at, updated_at), `keywords` (id, entry_id FK CASCADE, keyword), and `entries_fts` (FTS5 virtual table on title+content). The `status` field supports `active`, `deprecated`, `proposed`, `accepted`, and `superseded` values; `uid` is a unique 8-char hex identifier for cross-DB portability; `superseded_by` and `supersedes` link entries bidirectionally via UID. Indexes cover keywords, category, source, source_file, source+status, and uid (unique, partial). Three triggers (`entries_ai/ad/au`) keep the FTS index in sync.

## Entry: Search Implementation
keywords: [search, fts, keyword-fallback, search_entries, relevance-scoring, stale-detection]

The `search_entries()` function in `src/db.rs` implements a three-phase search: (1) FTS5 full-text search with trigram tokenizer and relevance scoring via `fts.rank`, (2) supplementary keyword table search for additional matches, and (3) LIKE fallback for short queries (e.g., 2-char CJK words) that trigram FTS cannot match. Results include a normalized `score` (0.0–1.0) and `stale` flag (true if not updated within the configurable `stale_threshold_days`, default 90). Deprecated entries show a warning with `superseded_by` link. Results can be filtered by category, source, and date via `--since`. The `--full` flag includes full content in JSON output.

## Entry: Database Configuration
keywords: [wal, foreign-keys, open_db, init_db, migration]

`open_db()` in `src/db.rs` enables WAL mode and foreign keys on every connection and runs migrations, returning `(Connection, bool)` where the bool indicates if a migration occurred. Migrations handle adding `source`, `status`, `superseded_by` columns (v1→v2) and `uid`, `supersedes` columns with `superseded_by` INTEGER→TEXT(UID) conversion (v4→v5). `init_db()` creates the database file, parent directories, and runs all CREATE TABLE/INDEX/TRIGGER statements. The DB uses rusqlite with the "bundled" feature so SQLite is compiled into the binary.
