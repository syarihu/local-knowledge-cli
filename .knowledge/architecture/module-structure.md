---
keywords: [architecture, module, structure, entry-point, main.rs]
category: architecture
---

# Module Structure

## Entry: Source Code Organization
keywords: [src, module, file-layout]

The project has 4 source files: `src/main.rs` handles CLI parsing via clap and all 15 command implementations (including `Edit` and `SearchLog`), `src/db.rs` manages SQLite operations, `src/markdown.rs` handles YAML frontmatter parsing and `## Entry:` heading extraction, and `src/keywords.rs` provides automatic keyword extraction. The `commands/` directory contains 7 embedded Claude Code slash command definitions as markdown files (`~lk-knowledge-*.md`, including `add-db` and `write-md` variants).

## Entry: Project Root Detection
keywords: [project-root, get_project_root, path]

The `get_project_root()` function in `src/main.rs:140-163` traverses parent directories looking for `.git`, `.knowledge`, or `.claude` directories to determine the project root. The database path is `{project_root}/.claude/knowledge.db` and knowledge markdown files are stored in `{project_root}/.knowledge/`.

## Entry: CLI Command Dispatch
keywords: [cli, commands, clap, dispatch]

The CLI uses clap's derive API with a `Cli` struct containing a `Commands` enum (15 variants including `Edit` and `SearchLog`) defined in `src/main.rs`. Each command variant maps to a handler function called in the main match block. All handlers return `Result<(), Box<dyn std::error::Error>>`.
