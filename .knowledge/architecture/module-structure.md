---
keywords: [architecture, module, structure, entry-point, main.rs]
category: architecture
---

# Module Structure

## Entry: Source Code Organization
keywords: [src, module, file-layout]

The project uses a modular layout: `src/main.rs` handles CLI parsing via clap and command dispatch, `src/cmd/` contains individual command implementations (add, entry, export, init, install_mcp, list, search, stats, sync, uninstall, update), `src/db.rs` manages SQLite operations, `src/markdown.rs` handles YAML frontmatter parsing and `## Entry:` heading extraction, `src/keywords.rs` provides automatic keyword extraction, `src/config.rs` loads project settings from `.knowledge/config.toml`, `src/secrets.rs` detects potential secrets in content, and `src/util.rs` provides shared utilities (project root detection, version checks). The `commands/` directory contains embedded Claude Code slash command definitions as markdown files (`lk-knowledge-*.md`).

## Entry: Project Root Detection
keywords: [project-root, get_project_root, path]

The `get_project_root()` function in `src/util.rs` traverses parent directories looking for `.git` or `.knowledge` directories to determine the project root. The database path is `{project_root}/.knowledge/knowledge.db` (with auto-migration from the old `.claude/knowledge.db` location) and knowledge markdown files are stored in `{project_root}/.knowledge/`.

## Entry: CLI Command Dispatch
keywords: [cli, commands, clap, dispatch]

The CLI uses clap's derive API with a `Cli` struct containing a `Commands` enum defined in `src/main.rs`. Each command variant maps to a handler function called in the main match block. All handlers return `Result<(), Box<dyn std::error::Error>>`.
