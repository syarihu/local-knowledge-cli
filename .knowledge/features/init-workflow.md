---
keywords: [init, setup, claude-md, gitignore]
category: features
---

# Init Workflow

## Entry: Eight-Stage Initialization
keywords: [init, cmd_init, knowledge-directory, claude-md, config]

The `cmd_init()` function in `src/cmd/init.rs` performs 8 stages: (1) creates SQLite DB at `.knowledge/knowledge.db` via `init_db()`, (2) creates `.knowledge/` directory with `architecture/`, `features/`, `conventions/` subdirectories and a `README.md`, (3) syncs any existing `.knowledge/*.md` files, (4) appends `.knowledge/knowledge.db`, `.knowledge/knowledge.db.bak.*`, `.knowledge/search.log`, and `.knowledge/command.log` to `.gitignore`, (5) writes `.claude/lk-instructions.md` and adds an `@.claude/lk-instructions.md` import line to CLAUDE.md (searches priority: root `CLAUDE.md` > root `AGENTS.md` > `.claude/CLAUDE.md`), (6) creates `.knowledge/config.toml` with default project settings, (7) writes `.knowledge/.lk-version` for version alignment, and (8) installs embedded Claude slash commands.
