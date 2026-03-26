---
keywords: [commands, slash-commands, claude-code, embedded, install-commands]
category: features
---

# Embedded Claude Commands

## Entry: Command Distribution Mechanism
keywords: [EMBEDDED_COMMANDS, include_str, install-commands]

Nine Claude Code slash commands are compiled into the binary via `include_str!()` macro in the `EMBEDDED_COMMANDS` constant in `src/cmd/update.rs`. The `install_embedded_commands()` function writes them to `~/.claude/commands/`. Commands are: `lk-knowledge-search.md` (search), `lk-knowledge-add-db.md` (add to local DB), `lk-knowledge-export.md` (export), `lk-knowledge-sync.md` (sync), `lk-knowledge-write-md.md` (write shared markdown), `lk-knowledge-discover.md` (project auto-scan), `lk-knowledge-refresh.md` (check and update stale entries), `lk-knowledge-from-branch.md` (extract knowledge from branch diff before merging), and `lk-knowledge-save-context.md` (save conversation context). Embedding in the binary provides MITM protection over network-fetched commands.
