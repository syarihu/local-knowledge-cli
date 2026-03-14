---
keywords: [database, sqlite, schema, fts, full-text-search]
category: features
---

# Database Schema & Operations

## Entry: Tables and Indexes
keywords: [entries, keywords, fts5, table, index]

The DB schema (defined at `src/db.rs:34-78`) has 3 tables: `entries` (id, title, content, category, source_file, file_hash, created_at, updated_at), `keywords` (id, entry_id FK CASCADE, keyword), and `entries_fts` (FTS5 virtual table on title+content). Indexes include `idx_keywords_keyword`, `idx_keywords_entry_id`, `idx_entries_category`, and `idx_entries_source_file`. Three triggers (`entries_ai/ad/au`) keep the FTS index in sync with the entries table.

## Entry: Search Implementation
keywords: [search, fts, keyword-fallback, search_entries]

The `search_entries()` function in `src/db.rs` implements a two-phase search: first FTS5 full-text search on title+content (ordered by `updated_at DESC`), then a supplementary keyword table search for additional matches. Results can be filtered by category and by date via `--since`. Same-title entries are deduplicated, keeping the newest. The FTS query uses SQLite's MATCH syntax.

## Entry: Database Configuration
keywords: [wal, foreign-keys, open_db, init_db]

`open_db()` at `src/db.rs:83-94` enables WAL mode and foreign keys on every connection. `init_db()` at `src/db.rs:27-81` creates the database file, parent directories, and runs all CREATE TABLE/INDEX/TRIGGER statements. The DB uses rusqlite with the "bundled" feature so SQLite is compiled into the binary.
