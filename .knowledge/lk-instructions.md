## Knowledge Base (local-knowledge-cli)

This project has a local knowledge base (`lk`). Run `lk` by command name (resolves via PATH).
If the `lk-knowledge` MCP server is available, **prefer its tools** (`search_knowledge`, `add_knowledge`, …) over CLI — faster, structured I/O. CLI is the fallback.

- **Shared knowledge** (`.knowledge/*.md`, git-tracked): stable project knowledge.
- **Local knowledge** (DB only, git-ignored): investigation cache; if stale, re-investigate rather than patch.

### Search BEFORE investigating
Search first **only when you're about to investigate unfamiliar code/architecture in this repo to answer the request** — not reflexively on every prompt (`search_knowledge` / `lk search "<kw>" --json`).
- Keywords: **1–3 short content words**, space-separated. **Don't paste the user's raw sentence/question** — extract the key nouns and drop stopwords/particles (は/が/を/the/how/…). Try **both English and Japanese**; broaden if no hits.
- `"stale": true` → verify vs current code, then update (if outdated) or touch (if still correct). `"superseded"`/`"deprecated"` → use the superseding entry.
- **Skip** when: exact file/line given; mechanical task (format/rename/version/git); conversational or trivial prompt; writing new code from a clear spec; the request isn't about this codebase's internals; or you already have the context.

### Search past context
When the user signals continuation ("last time", "we looked into this", "where did we leave off"), or a topic likely has prior discussion, search `category: context` / `conversation-log` first (`search_knowledge(query, category: "context")`).

### Save AFTER discovering
After investigating unfamiliar code, save non-trivial, reusable findings (skip mechanical tasks).
- Use stable identifiers (function/struct names), not line numbers. Include the "why". **Always pass 5–10 curated keywords** (lowercase hyphenated; key components, concepts, proper nouns — English + Japanese where useful) instead of relying on auto-extraction. **Never store secrets.**
- Only a **same-title** collision (or an all-but-identical title) refuses the add (`added: false` + `similar_entries`) — edit that entry instead (`lk edit` / `edit_knowledge`), or pass `--force` / `force: true` to add it anyway. Nothing else refuses: the add succeeds and any loosely related entries come back under `possibly_related`, which is **informational only** — the entry is already saved, so do not edit one of those unless it genuinely covers the same topic.
- **Design decisions** → `category: decisions`, ADR format, `status: proposed` → `accepted`. Details: `/lk-knowledge-add-db`.
- **Session/conversation context** to carry over → **save it proactively, without asking first**, as `category: context` + keyword `conversation-log` when a decision/conclusion is reached, a non-obvious discovery is made, or the conversation has grown long. Briefly note what was saved (id/uid). Manual entry point: `/lk-knowledge-save-context`.
- **Plans you design** (plan-mode or in conversation) → **save proactively, without asking** (CLI `lk add` / MCP `add_knowledge`), as `category: plan` + `status: proposed`, once out of plan mode. Make it self-contained and dense: what/why, steps, files, rejected options. Persists across compaction/session loss. Full procedure & lifecycle: `/lk-knowledge-plan`.
- **Scope**: project vs user (global `~/.config/lk/knowledge.db`, shared across all projects). Defaults are per-command: `add`=auto (project if initialized, else user), reads=all (merged). Use `--scope user` / `scope: "user"` for personal prefs or cross-project context not tied to this repo. Reads merge both by default. Address user entries by uid (numeric ids are project-only). In a project without `lk init`, `add` (default scope) auto-falls back to user scope, so saving always works.
- **User-scope markdown (dotfiles sync)**: user knowledge can be exported to markdown for versioning/sync across machines, mirroring the project md⇄DB model. `lk export --scope user` writes `~/.config/lk/knowledge/*.md` (md becomes the source of truth, entries flip to `shared`); `lk sync --scope user` imports those md back into the user DB (md wins). `lk sync --scope user --write-uids` bakes the globally-unique `uid` into the md as a cross-machine merge key. Override the markdown location via `user_knowledge_dir` in `~/.config/lk/config.toml` (e.g. point it at a dotfiles repo, or symlink it). lk only writes the markdown — committing/pushing it is the user's job (same as project scope).

### CLI signatures — the common forms, so these need no `--help` trip
`<id-or-uid>`: numeric id (project) or uid (resolves across scopes). All six below also take `--json` and `--scope` (`delete` takes `--scope` but not `--json`; `purge`/`import` take neither). Seldom-needed flags are left out — `lk <cmd> --help` for the complete list.

```
lk search "<kw>" [--category C] [--status S] [--source local|shared] [--since YYYY-MM-DD] [--limit N] [--full]
lk get    <id-or-uid>
lk add    "<title>" -k "kw1,kw2" -c "<body>" [--category C] [--status S] [--force]
lk edit   <id-or-uid> [-t "<title>"] [-k "kw1,kw2"] [-c "<body>"] [--status S] [--superseded-by <id-or-uid>] [--touch]
lk list   [--category C] [--status S] [--source local|shared] [--limit N] [--offset N]
lk supersede <old-id-or-uid> <new-id-or-uid>
```

**`lk edit` is the CLI equivalent of the `edit_knowledge` MCP tool.** `lk update` is *not* an entry command — it upgrades the lk binary itself. `-t/-k/-c` are short for `--title/--keywords/--content`; `edit` replaces each field it is given and leaves the rest alone.

### Delegating to sub-agents
When launching Explore/general-purpose agents to investigate unfamiliar code, tell them to **`lk search` first** and to return a **`## Knowledge to Save`** section; then save what they return. Full prepend text + capture procedure: `/lk-knowledge-agent-brief`. Skip for mechanical tasks.

### More
Workflows: `/lk-knowledge-discover` `/lk-knowledge-refresh` `/lk-knowledge-from-branch` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write-md` `/lk-knowledge-search` `/lk-knowledge-plan` (save/resume "do it later" plans via `category: plan` + `status`). For commands not listed above (`init` `sync` `export` `import` `delete` `purge` `keywords` `stats`): `lk <cmd> --help`.
