---
keywords: [init, setup, claude-md, gitignore]
category: features
---

# Init Workflow

## Entry: Five-Stage Initialization
keywords: [init, cmd_init, knowledge-directory, claude-md]

The `cmd_init()` function in `src/main.rs` performs 6 stages: (1) creates SQLite DB at `.claude/knowledge.db` via `init_db()`, (2) creates `.knowledge/` directory with `architecture/`, `features/`, `conventions/` subdirectories and a `README.md`, (3) syncs any existing `.knowledge/*.md` files, (4) appends `.claude/knowledge.db` and `.claude/search.log` to `.gitignore` if not already present, (5) adds the `CLAUDE_MD_SECTION` constant (English instructions for knowledge base usage including auto-accumulation rules) to CLAUDE.md — searches in priority order: root `CLAUDE.md` > root `AGENTS.md` > `.claude/CLAUDE.md`, creating root `CLAUDE.md` if none exist, and (6) installs embedded Claude slash commands (`lk-knowledge-*.md`).
