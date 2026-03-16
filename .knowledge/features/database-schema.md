---
keywords: [database, sqlite, schema, fts, full-text-search]
category: features
---

# Database Schema & Operations

## Entry: Tables and Indexes
keywords: [entries, keywords, fts5, table, index]

The DB schema (defined in `src/db.rs`) has 3 tables: `entries` (id, title, content, category, source, source_file, file_hash, created_at, updated_at), `keywords` (id, entry_id FK CASCADE, keyword), and `entries_fts` (FTS5 virtual table on title+content). Indexes include `idx_keywords_keyword`, `idx_keywords_entry_id`, `idx_entries_category`, `idx_entries_source`, and `idx_entries_source_file`. Three triggers (`entries_ai/ad/au`) keep the FTS index in sync with the entries table.

## Entry: Search Implementation
keywords: [search, fts, keyword-fallback, search_entries, relevance-scoring]

The `search_entries()` function in `src/db.rs` implements a two-phase search: first FTS5 full-text search on title+content with relevance scoring via `fts.rank`, then a supplementary keyword table search for additional matches. Results include a normalized `score` field (0.0–1.0) computed as `1.0 / (1.0 + abs(rank))`. Results can be filtered by category, source, and date via `--since`. The `--full` flag includes full content in JSON output, eliminating the need for `lk get`. The FTS query uses SQLite's MATCH syntax.

## Entry: Database Configuration
keywords: [wal, foreign-keys, open_db, init_db]

`open_db()` in `src/db.rs` enables WAL mode and foreign keys on every connection and runs migrations, returning `(Connection, bool)` where the bool indicates if a migration occurred. `init_db()` creates the database file, parent directories, and runs all CREATE TABLE/INDEX/TRIGGER statements. The DB uses rusqlite with the "bundled" feature so SQLite is compiled into the binary.
