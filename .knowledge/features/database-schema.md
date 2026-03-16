---
keywords: [database, sqlite, schema, fts, full-text-search]
category: features
---

# Database Schema & Operations

## Entry: Tables and Indexes
keywords: [entries, keywords, fts5, table, index]

The DB schema (defined in `init_db()` in `src/db.rs`) has 3 tables: `entries` (id, title, content, category, source, source_file, file_hash, status, superseded_by, created_at, updated_at), `keywords` (id, entry_id FK CASCADE, keyword), and `entries_fts` (FTS5 virtual table on title+content). The `status` field supports `active` and `deprecated` values; `superseded_by` links to a replacement entry. Indexes cover keywords, category, source, and source_file. Three triggers (`entries_ai/ad/au`) keep the FTS index in sync.

## Entry: Search Implementation
keywords: [search, fts, keyword-fallback, search_entries, relevance-scoring, stale-detection]

The `search_entries()` function in `src/db.rs` implements a two-phase search: first FTS5 full-text search with relevance scoring via `fts.rank`, then a supplementary keyword table search for additional matches. Results include a normalized `score` (0.0–1.0) and `stale` flag (true if not updated in 90+ days). Deprecated entries show a warning with `superseded_by` link. Results can be filtered by category, source, and date via `--since`. The `--full` flag includes full content in JSON output.

## Entry: Database Configuration
keywords: [wal, foreign-keys, open_db, init_db, migration]

`open_db()` in `src/db.rs` enables WAL mode and foreign keys on every connection and runs migrations, returning `(Connection, bool)` where the bool indicates if a migration occurred. Migrations handle adding `source`, `status`, and `superseded_by` columns to older databases. `init_db()` creates the database file, parent directories, and runs all CREATE TABLE/INDEX/TRIGGER statements. The DB uses rusqlite with the "bundled" feature so SQLite is compiled into the binary.
