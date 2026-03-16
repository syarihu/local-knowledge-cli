---
keywords: [data-flow, sync, export, import, markdown, sqlite]
category: architecture
---

# Data Flow

## Entry: Markdown to DB Pipeline
keywords: [sync, import, parse, MdEntry]

Markdown files in `.knowledge/` are parsed by `parse_md_entries()` in `src/markdown.rs:40-96` which extracts YAML frontmatter (keywords, category) and splits content by `## Entry:` headings into `MdEntry` structs. Each entry is then inserted via `db::add_entry()` with file-level and entry-level keywords merged. SHA256 file hashes (computed at `markdown.rs:119-123`) track changes for incremental sync.

## Entry: Sync Workflow
keywords: [sync, hash, shared, unchanged, updated]

The `sync_knowledge_dir()` function in `src/main.rs` performs a 3-stage sync: (1) fetches existing source_file→hash mappings via `get_shared_file_hashes()`, (2) walks `.knowledge/*.md` files comparing hashes, and (3) reports unchanged/updated/added/removed counts. Updated files have their old entries deleted before re-import. Symlink traversal is blocked by canonicalizing paths and checking they remain within the project base directory.

## Entry: Export Workflow
keywords: [export, local, shared, markdown-generation]

The `cmd_export()` function in `src/main.rs` takes entries with `source = "local"` from the DB, groups them by their first keyword, and writes `exported-{keyword}.md` files to `.knowledge/`. After export, entries are promoted from `source = "local"` to `source = "shared"` via `update_entry_to_shared()` so they won't be re-exported.
