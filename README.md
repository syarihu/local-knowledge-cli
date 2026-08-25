# lk - Local Knowledge CLI

<img alt="local-knowledge-cli-logo" src="docs/images/local-knowledge-cli-logo.png" />

A local knowledge base CLI for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Store, search, and share project knowledge using a local SQLite database and markdown files.

## Features

- Project-local knowledge base stored in `.knowledge/knowledge.db`
- Full-text search with trigram tokenizer (supports Japanese/CJK), keyword search, and LIKE fallback
- Smart query splitting — hyphens, underscores, and CamelCase are automatically split into separate tokens (e.g., `auth-API` → `auth` + `API`)
- OR-matched, relevance-ranked queries — multi-word queries match entries containing *any* term (not all), with bm25 ranking entries that hit more/rarer terms highest, so even a loosely-worded query still surfaces the most relevant entries
- Duplicate detection when adding entries (skip with `--force`)
- Sync knowledge from `.knowledge/` markdown files (shareable via Git)
- Auto-sync on command execution — no manual `lk sync` needed after `git pull`
- Export local entries to markdown for team sharing (stable output order)
- Secret detection — warns when content contains API keys, tokens, or credentials
- Project config via `.knowledge/config.toml` (git-tracked, team-shareable)
- User-scope (global) knowledge at `~/.config/lk/knowledge.db` — carry personal notes and cross-project context (e.g. session handoff logs) across all projects with `--scope user`; reads merge project + user by default. In projects without `lk init`, `lk add` automatically falls back to this global store, so it works anywhere
- User-scope markdown export/sync — `lk export/sync --scope user` mirrors user knowledge to `~/.config/lk/knowledge/*.md` so personal notes can be versioned and synced across machines (e.g. via a dotfiles repo)
- Bulk delete with `purge` by category or source
- Auto-extract keywords from entries — frequency-ranked and capped (top 15; title/file-path terms weighted higher), used only as a fallback when no keywords are given; `lk keywords --regen` cleans up noisy keyword sets from older versions
- Self-update from GitHub Releases
- Git worktree support — all worktrees share the main worktree's DB, so knowledge is available across worktrees
- MCP (Model Context Protocol) server — Claude Code / Claude Desktop can autonomously search, add, and manage knowledge
- Installs Claude Code slash commands for seamless integration

## Installation

### Homebrew (macOS / Linux)

```bash
brew install syarihu/tap/lk
```

### Shell script

```bash
curl -fsSL https://raw.githubusercontent.com/syarihu/local-knowledge-cli/main/setup.sh | bash
```

Or specify a version:

```bash
curl -fsSL https://raw.githubusercontent.com/syarihu/local-knowledge-cli/main/setup.sh | bash -s v0.1.0
```

### Build from source

```bash
cargo install --path .
```

> **Note:** Shell script and source builds require `~/.local/bin` in your `PATH`.

## Quick Start

```bash
# Initialize knowledge base in your project
cd your-project
lk init

# Add knowledge
lk add "API rate limit is 100 req/min" --keywords "api,rate-limit"

# Search
lk search "rate limit"

# List all entries
lk list
```

## Usage

```
lk <COMMAND>

Commands:
  init              Initialize knowledge base for current project (or globally with --global)
  add <title>       Add a knowledge entry (with duplicate detection)
  search <query>    Search knowledge entries (with relevance scoring)
  get <id>          Get a single entry by ID
  edit <id>         Edit an existing entry
  delete <id>       Delete an entry
  purge             Delete all entries by category or source
  supersede         Mark an entry as superseded by another (bidirectional)
  list              List all entries
  sync              Sync .knowledge/ files with DB
  export            Export local entries to markdown
  import <path>     Import a markdown file
  keywords          List all unique keywords (--regen regenerates noisy per-entry keywords)
  stats             Show database statistics
  command-log       Show recent command log entries
  update            Update lk to latest version
  install-commands  Install Claude Code slash commands and refresh existing lk-instructions.md
  uninstall         Uninstall lk from current project
  mcp               Start MCP server (JSON-RPC 2.0 over stdio)
  install-mcp       Install lk as MCP server for Claude Code / Claude Desktop
  uninstall-mcp     Uninstall lk MCP server from Claude Code / Claude Desktop
```

### Common Options

- `--json` - Output as JSON (available on most commands)
- `--keywords "kw1,kw2"` - Comma-separated keywords (for `add`; authoritative when given — auto-extraction only runs when omitted)
- `--content "..."` - Entry content (for `add`)
- `--category <cat>` - Filter by category (for `search`, `list`, `purge`)
- `--source <src>` - Filter by source: `local` or `shared` (for `search`, `list`, `purge`)
- `--status <status>` - Status `active`, `deprecated`, `proposed`, `accepted`, `superseded` — set the status (for `add`, `edit`) or filter by it (for `search`, `list`)
- `--limit <n>` - Max results, default 5 (for `search`)
- `--since <YYYY-MM-DD>` - Only return entries updated since this date (for `search`)
- `--full` - Include full content in JSON output, eliminating the need for `lk get` (for `search`)
- `--force` - Add even if an entry with the same (or an all-but-identical) title exists (for `add`). Rarely needed — nothing else is refused
- `--allow-secrets` - Allow content that contains potential secrets (for `add`, `export`)
- `--scope <scope>` - Knowledge store to use. `add`: `auto` (default — project if initialized, else user), `project`, or `user`. Targets (`get`/`edit`/`delete`/`supersede`): `project` or `user` (omit to auto-resolve). Reads (`search`/`list`/`stats`): `project`, `user`, or `all` (default, merged)
- `--project <owner/repo>` - Project to attribute the entry to (for `add`, `edit`; pass `""` to `edit` to clear it). Defaults to the git remote slug of the current repo. For `search` / `list` it filters instead: `owner/repo` matches exactly, a bare repo name matches any owner, and `.` means the current project — see [Project attribution](#project-attribution)

### Keywords

Keywords are the terms that best represent an entry — they power keyword search (fallback), duplicate detection, and human scanning. Full-text search already covers the whole title/content, so keywords don't need to (and shouldn't) mirror every word.

- When `--keywords` is given, it is used as-is (curated keywords are authoritative).
- When omitted, keywords are auto-extracted as a fallback: candidate terms (ASCII words, CamelCase/snake_case parts, file-path segments, katakana) are ranked by frequency — title and file-path terms weighted higher — and capped at 15.
- Entries created by older versions may carry large, noisy auto-extracted keyword sets. Clean them up with:

```bash
lk keywords --regen --dry-run   # preview which entries would change
lk keywords --regen             # regenerate local entries with > 15 keywords
lk keywords --regen --all       # regenerate every local entry
```

`--regen` only touches `local` entries — `shared` entries' keywords are owned by their `.knowledge/*.md` frontmatter, so fix the markdown and run `lk sync` instead. It also leaves `updated_at` untouched, so staleness tracking is unaffected.

### Duplicate detection

`add` compares the incoming entry against the existing ones on two independent signals and reacts in one of two ways:

| Outcome | When | What happens |
| --- | --- | --- |
| **Refused** | The title matches an existing one — ignoring case, spacing, punctuation and full-width forms — or is all but identical to it | Nothing is added. The colliding entries come back as `similar_entries` with `added: false`, each carrying a `match_reason` (`same-title` for an exact match after normalization, `similar-title` for a marginal difference). Edit that entry instead, or re-run with `--force`. |
| **Added, with a note** | The topic looks close but the title differs | **The entry is saved.** Related entries are listed under `possibly_related` purely for information. |

Anything below both thresholds is added silently.

The two signals are character-trigram overlap of the normalized titles, and IDF-weighted overlap of the keyword sets. Weighting keywords by inverse document frequency is what keeps generic terms (`test`, `main`, `update`) from reading as a match — they are common, so they count for little, and terms held by more than a quarter of the base are ignored outright. Keyword agreement alone never refuses an add: entries imported from one markdown file all inherit that file's keywords, so their keyword sets collide for reasons that have nothing to do with their topic.

## How It Works

### Storage

All lk-managed files are stored under the `.knowledge/` directory:

- **SQLite DB** at `.knowledge/knowledge.db` (git-ignored) - local search index (shared across git worktrees)
- **Markdown files** in `.knowledge/` (git-tracked) - shareable knowledge
- **Config file** at `.knowledge/config.toml` (git-tracked) - project settings
- **Version file** at `.knowledge/.lk-version` (git-tracked) - minimum required lk version for the project
- **Instructions** at `.knowledge/lk-instructions.md` (git-tracked) - Claude Code instructions, imported via `@` syntax
- **Command log** at `.knowledge/command.log` (git-ignored) - optional command logging

### What to commit

| Path | Git | Description |
|------|-----|-------------|
| `.knowledge/*.md` | Yes | Shared knowledge (markdown files) |
| `.knowledge/config.toml` | Yes | Project settings |
| `.knowledge/.lk-version` | Yes | Minimum required lk version |
| `.knowledge/lk-instructions.md` | Yes | Claude Code instructions |
| `.gitattributes` | Yes | Marks `.knowledge/*.md` as generated (configurable) |
| `AGENTS.md`, `CLAUDE.md`, or `.claude/CLAUDE.md` | Yes | Contains `@.knowledge/lk-instructions.md` import |
| `.knowledge/knowledge.db` | No (auto-ignored) | Local search index |
| `.knowledge/command.log` | No (auto-ignored) | Command log |

### Shared vs local knowledge

Knowledge entries have two categories:

- **Shared** (`.knowledge/` markdown files, git-tracked) — Architecture, design decisions, team conventions, and other stable knowledge that the whole team should know. Write with `/lk-knowledge-write-md` or `/lk-knowledge-from-branch` and commit. Stale after 30 days (configurable).
- **Local** (DB only, git-ignored) — LLM investigation cache that reduces context consumption when working on similar tasks repeatedly. These stay on your machine as disposable cache. Stale after 7 days (configurable). When stale, re-investigate rather than updating.

A good rule of thumb: shared knowledge is for stable facts that would help a new team member or Claude understand the project. Local knowledge is a performance optimization — it lets Claude skip re-reading code it recently investigated.

### Project scope vs user scope

Knowledge lives in one of two stores:

- **Project scope** — the per-project `.knowledge/knowledge.db` resolved from the current directory. This is where shared and local knowledge for *this* repo lives.
- **User scope** — a single global DB at `~/.config/lk/knowledge.db`, shared across every project. Use it for personal notes and cross-project context such as session handoff logs. The DB is created on first `--scope user` write. Unlike project scope (where `.knowledge/` is committed to the repo), user-scope markdown lives outside any project — see [User-scope markdown](#user-scope-markdown) below.

How scope is selected:

- **Writes** (`add`): `--scope auto` (default), `project`, or `user`. **`auto` falls back to user scope when the current project isn't initialized** (no `.knowledge/knowledge.db`), so `lk add` works anywhere without `lk init` — it saves to the project when initialized, otherwise globally (with a note). An explicit `--scope project` still errors with an init prompt when the project isn't set up.
- **Reads** (`search`, `list`, `stats`): `--scope all` (default — merges both stores and tags each result with its `scope`), or restrict to `project` / `user`. In an uninitialized project the (missing) project store is simply skipped, so reads return user-scope results instead of erroring.
- **Targets** (`get`, `edit`, `delete`, `supersede`): pass an entry id or uid. A numeric id resolves in the project DB (or the DB named by `--scope`); a uid resolves across scopes (project, then user — and skips the project store when it isn't initialized). Because project and user ids both start at 1, address user-scope entries by uid (shown in `--json` output). `supersede` requires both entries in the same scope.

The `lk-knowledge` MCP tools mirror this: `search_knowledge` / `list_knowledge` / `get_stats` take a `scope` and gracefully skip an uninitialized project; `add_knowledge` takes a `scope` (default `auto`, same user-scope fallback); and `get_knowledge` / `edit_knowledge` / `supersede_knowledge` accept an id or uid string.

#### Project attribution

Every entry records the project it was added from, so a user-scope hit can say where it came from even when that project was never `lk init`ed:

```console
$ lk search "release flow" --scope user
  [a1b2c3d4e5f6] Release checklist (context) @some-app
```

The recorded value is the repo's **git remote slug** (`owner/repo`), resolved in this order:

1. `--project <name>` on `lk add`
2. `LK_PROJECT` in the environment
3. `git config lk.project` in the repo
4. `origin`'s remote URL, normalized (`git@host:o/r.git`, `https://host/o/r.git` and `ssh://git@host/o/r` all become `o/r`)
5. the main worktree's directory name, then the project root's

`git config lk.project acme/thing` is the one to reach for when the detected key is wrong or unwanted — it persists, every worktree of the repo shares it, it needs no `lk init`, and because it belongs to the repo rather than the environment it is honored through MCP too. `--project` and `LK_PROJECT` stay one-off overrides for a single add or shell.

The slug is preferred over a directory name because a linked worktree's directory changes per branch (one repo would otherwise scatter across several names) and because the owner keeps same-named repos in different orgs apart. A bare name passed to `--project` is expanded to the full slug when it names the current repo.

A remote whose URL is a filesystem path (`file://`, a local clone, or an scp/ssh form with an absolute path) contributes only its last segment, so no local directory layout is stored. A self-hosted remote addressed by a server path — `ssh://host/home/alice/repo.git` — keeps that path as its key, because that path is the repo's identity and truncating it would merge unrelated repos; set `git config lk.project` in that repo to record something else instead.

Filter by it with `--project`:

```bash
lk search "release" --project syarihu/some-app   # exactly that repo
lk search "release" --project some-app           # any owner's some-app
lk search "release" --project .                  # the repo you are standing in
lk list --project some-app --scope user
```

A full slug matches exactly, so `hoge/app` never answers with `fuga/app` — that is what the owner in the key is for. A bare name matches on the *last* segment, so it finds `syarihu/some-app`, a deeper `group/sub/some-app`, and any bare value recorded where no remote was known. When a bare name matches more than one recorded value the command says so on stderr instead of quietly merging them — asked of the store, so a small `--limit` cannot hide the ambiguity. An unusable value is an error rather than an empty result, since "no results" reads as "nothing recorded". Entries with no project recorded never match a filter.

`lk edit <id-or-uid> --project owner/repo` fills in an entry added before the column existed, and `--project ""` clears it.

See the spread with `lk stats --by-project`, which merges both scopes so one project is one row, and names the entries with nothing recorded rather than hiding them:

```console
$ lk stats --by-project
By project:
  syarihu/local-knowledge-cli  42
  (unattributed)               18
  syarihu/some-app              7
```

Search results also use it as a **tie-break**: when two hits score the same — which is every hit on the keyword and substring fallbacks, since only full-text search produces a score — the one recorded in the project you are standing in comes first. Genuine relevance differences are never reordered. The preference is applied while candidates are still being selected, so it holds at `--limit 1` too, not just within a page that was already chosen.

`lk get` and `--json` output always show the full slug. The human-readable `search` / `list` badge shows just the repo name, and only for entries recorded against a *different* project than the one you are standing in — so results from this repo stay unadorned while a hit carried in from elsewhere is marked. Entries added before this existed have no project recorded. The value round-trips through markdown as a `project:` line, so `lk sync` preserves it.

#### User-scope markdown

User-scope knowledge can be mirrored to markdown so it can be versioned and synced across machines — the same md⇄DB model as project scope, but stored globally instead of in a repo.

```bash
# Export the user DB to markdown (md becomes the source of truth; entries flip to `shared`)
lk export --scope user        # writes ~/.config/lk/knowledge/*.md

# Sync those markdown files back into the user DB (e.g. on another machine after `git pull`)
lk sync --scope user

# Bake the globally-unique uid into the markdown as a cross-machine merge key
lk sync --scope user --write-uids
```

The markdown directory defaults to `~/.config/lk/knowledge` and is configurable via `user_knowledge_dir` in the global config (`~/.config/lk/config.toml`) — point it at a dotfiles repo (or a symlink) to version your personal knowledge. The global config is scaffolded automatically on the first `lk export --scope user`. See [Global config](#global-config-user-scope) for the available settings.

### Team workflow

1. Run `lk init` in your project — each team member runs this once after cloning
2. Claude Code automatically discovers and saves knowledge as you work (`lk add`)
3. Run `lk export` to write local knowledge to `.knowledge/` markdown files, then commit and push — only export knowledge worth sharing with the team
4. After pulling changes, shared knowledge is **auto-synced** on the next `lk` command — no manual `lk sync` needed
5. Use `/lk-knowledge-discover` to bootstrap knowledge for a new project, or `/lk-knowledge-refresh` to update stale entries

### Git worktree support

When using `git worktree`, all worktrees automatically share the main worktree's knowledge DB. Knowledge added in any worktree is immediately available in all others — no configuration needed.

- **Local knowledge** (DB) — shared across all worktrees via the main worktree's `.knowledge/knowledge.db`
- **Shared knowledge** (`.knowledge/*.md`) — each worktree has its own copy based on the checked-out branch, auto-synced as usual

### Version alignment

`lk init` writes the current version to `.knowledge/.lk-version`. When a team member runs any `lk` command with an older binary, they'll see a warning:

```
Warning: This project requires lk >= 0.8.0, but you have 0.7.2. Run `lk update` or `brew upgrade lk` to update.
```

Commit `.lk-version` to keep the team on a compatible version.

### Markdown Format

Knowledge markdown files use YAML frontmatter and `## Entry:` headings:

```markdown
---
keywords: [api, authentication]
category: architecture
---

# API Knowledge

## Entry: Authentication Flow
keywords: [auth, jwt]
project: acme/api-server

The API uses JWT tokens for authentication...

## Entry: Rate Limiting
keywords: [api, rate-limit]

Rate limit is 100 requests per minute per API key...
```

Per-entry metadata lines (`keywords:`, `uid:`, `status:`, `project:`, `superseded_by:`, `supersedes:`) sit directly under the `## Entry:` heading and are stripped from the stored content. Only that leading block counts, so a line like `project: demo/app` further down stays part of the content. `uid:` is the one exception: markdown written before this rule existed put it anywhere, and losing a uid would change an entry's identity on the next `sync`, so a uid-shaped value is still recognized below the block. `project:` may also be set once in the frontmatter to cover every entry in the file.

### ADR (Architecture Decision Records)

lk can be used to manage ADRs by leveraging its status and supersede features. Entries support a full decision lifecycle:

| Status | Meaning |
|--------|---------|
| `proposed` | Under discussion, not yet decided |
| `accepted` | Approved and in effect |
| `active` | General-purpose active entry (default) |
| `deprecated` | No longer relevant |
| `superseded` | Replaced by a newer decision |

#### Example workflow

```bash
# Propose a new decision
lk add "Use JWT for API auth" --category decisions --status proposed --content "We propose using JWT tokens for stateless authentication..."

# Accept it (using the entry ID from add)
lk edit 42 --status accepted

# Later, supersede it with a new decision
lk add "Migrate to OAuth 2.0" --category decisions --content "JWT approach has limitations with token revocation..."
lk supersede 42 55  # marks #42 as superseded, links both entries bidirectionally
```

#### UIDs for portable links

Each entry has a unique 12-character hex UID. The `supersede` command uses UIDs internally so that supersede links remain valid when sharing `.knowledge/` markdown files across team members (whose local DB IDs may differ).

```bash
# Write UIDs back to markdown files
lk sync --write-uids

# Filter by status
lk list --status proposed
lk list --status superseded
```

The `/lk-knowledge-write-md` and `/lk-knowledge-from-branch` slash commands automatically detect ADR-like content (design decisions, trade-off discussions) and suggest using the `decisions` category with appropriate status values.

### Context Persistence

Claude Code conversations lose context on compact or session end. lk's `context` category lets you carry over investigation results, design discussions, and conclusions into future conversations.

#### How it works

- **Auto-save**: Claude proactively saves context (without asking for confirmation) when a design decision is reached, a non-obvious discovery is made, or the conversation has accumulated significant context, and briefly notes what was saved
- **Manual save**: Run `/lk-knowledge-save-context` to extract and save important context from the current conversation
- **Retrieval**: When you say things like "we looked into this before" or "continuing from last time", Claude searches the `context` category automatically

#### What gets saved

Context entries use `category: context` and always include the `conversation-log` keyword. Content summarizes the flow: what was investigated → what was found → what was decided.

```bash
# Save context manually via CLI
lk add "Auth middleware rewrite discussion" \
  --category context \
  --keywords "conversation-log,auth,middleware" \
  --content "Investigated session token storage. Legal flagged compliance issue..."

# Search past context
lk search "auth middleware" --category context --json --full

# Context entries have a short stale threshold (7 days by default)
# since they are local investigation cache
```

This complements Claude Code's built-in memory — Claude memory stores user preferences and project background, while lk context stores technical investigation results and decision rationale.

### Deferred plans ("do it later")

"Let's plan this and tackle it later" is a recurring pattern. lk turns those plans into a resumable working list by combining a `plan` category with the entry status lifecycle — no schema change, since categories are free-form.

- **Save** a plan with `proposed` (= open) status, **list** open plans, **resume** one, then **close** it with `accepted` (= done) or drop it with `deprecated`. Entries are kept as a record rather than deleted.
- Reads merge the **current project + user scope** by default (not every project's `.knowledge`). For a single work list that follows you across every repo, save plans with `--scope user`.
- **Auto-save**: after `lk init`, Claude proactively saves every plan it designs (plan-mode plans and approaches worked out in conversation) as a `plan` entry — without asking — so the plan survives a compact or session crash and is instantly recoverable from lk later, the same way `context` entries are saved.

```bash
# Save a plan to do later (write it dense enough to resume cold)
lk add "Migrate auth to OAuth 2.0" --category plan --status proposed \
  --keywords "plan,auth,oauth" --content "Decision + approach + rejected options + concrete identifiers..."

# List open plans (across project + user scope)
lk list --category plan --status proposed

# Search within plans
lk search "oauth" --category plan --status proposed

# Close a plan when done (keeps it as a record)
lk edit 42 --status accepted
```

The `/lk-knowledge-plan` slash command wraps this flow (save / list-and-resume / done / drop). `lk add` and `lk search` accept `--status` to set the initial status and filter by it; `lk list` already supported `--status`.

## Claude Code Integration

There are two ways to integrate lk with Claude Code:

### MCP Server (recommended)

Register lk as an MCP server so Claude can autonomously search, add, and manage knowledge:

```bash
# Install for both Claude Code and Claude Desktop (auto-detects current project)
cd your-project
lk install-mcp

# Or install for a specific target
lk install-mcp --target claude-code
lk install-mcp --target claude-desktop

# To uninstall
lk uninstall-mcp
```

**Multiple projects:** Running `lk install-mcp` from different project directories automatically merges them into the existing config — no need to re-specify all projects each time.

```bash
# Register project-a
cd /path/to/project-a && lk install-mcp --target claude-desktop

# Add project-b (project-a stays registered)
cd /path/to/project-b && lk install-mcp --target claude-desktop

# Or register multiple projects explicitly
lk install-mcp --target claude-desktop --project /path/to/a --project /path/to/b

# Remove a specific project
lk install-mcp --target claude-desktop --remove-project /path/to/old-project
```

Once installed, Claude has access to these tools:

| Tool | Description |
|------|-------------|
| `search_knowledge` | Search the knowledge base with full-text or keyword search |
| `add_knowledge` | Add new entries with duplicate detection |
| `list_knowledge` | Browse entries with source/category filtering and pagination |
| `get_knowledge` | Retrieve full content of an entry by ID |
| `edit_knowledge` | Edit title, content, keywords, or status of an entry (CLI: `lk edit`) |
| `supersede_knowledge` | Mark an entry as superseded by another (bidirectional) |
| `get_stats` | Get knowledge base statistics |
| `list_projects` | List registered projects (multi-project mode only) |

No manual server startup is needed — Claude Code / Claude Desktop automatically launches `lk mcp` when a tool is called. When multiple projects are registered, each tool accepts an optional `project` parameter to specify which project to operate on.

### Slash Commands

`lk init` creates `.knowledge/lk-instructions.md` with Claude Code instructions and adds an `@.knowledge/lk-instructions.md` import line to your `AGENTS.md` (or `CLAUDE.md` if it exists). This keeps your config file minimal while providing full instructions to Claude Code via the [`@import` syntax](https://docs.anthropic.com/en/docs/claude-code/memory#import-additional-files).

`lk-instructions.md` is generated (not meant to be hand-edited), so `lk update` and `lk install-commands` refresh it in place wherever it already exists (the current project and the global `~/.claude/` copy) using the freshly installed binary's content. Locations that haven't run `lk init` are left untouched.

After `lk init`, Claude Code will automatically:

1. Search the knowledge base before exploring code
2. Add new discoveries via `/lk-knowledge-add-db`
3. Use slash commands: `/lk-knowledge-search`, `/lk-knowledge-add-db`, `/lk-knowledge-export`, `/lk-knowledge-sync`, `/lk-knowledge-write-md`, `/lk-knowledge-discover`, `/lk-knowledge-refresh`, `/lk-knowledge-from-branch`, `/lk-knowledge-save-context`, `/lk-knowledge-agent-brief`, `/lk-knowledge-plan`

### MCP + Slash Commands

Both methods can be used together. MCP lets Claude use knowledge tools autonomously during any conversation, while slash commands provide explicit user-invoked workflows like `/lk-knowledge-discover` (project-wide knowledge generation) and `/lk-knowledge-refresh` (stale entry updates).

## Configuration

`lk init` creates `.knowledge/config.toml` with project-level settings. This file is git-tracked so the whole team shares the same configuration.

```toml
# .knowledge/config.toml

# Days before a shared entry is considered stale (default: 30)
stale_threshold_days = 30

# Days before a local entry is considered stale (default: 7)
local_stale_threshold_days = 7

# Default limit for `lk search` results (default: 5)
search_default_limit = 5

# Auto-sync .knowledge/ markdown files before read commands (default: true)
# Override with LK_NO_AUTO_SYNC=1
auto_sync = true

# Detect potential secrets in content when adding/exporting entries (default: true)
secret_detection = true

# Enable command logging to .knowledge/command.log (default: false)
# Override with LK_COMMAND_LOG=1
command_log = false

# Mark .knowledge/*.md as linguist-generated in .gitattributes (default: true)
# Set to false to show full diffs for .knowledge/*.md in GitHub PRs
gitattributes_generated = true
```

### Global config (user scope)

User-scope markdown export/sync is governed by a separate global config at `~/.config/lk/config.toml`, scaffolded automatically on the first `lk export --scope user`.

```toml
# ~/.config/lk/config.toml

# Directory holding user-scope markdown — the source of truth for user knowledge.
# Default: ~/.config/lk/knowledge. Point this at a dotfiles repo to version it.
# Use an absolute path or one under ~/ (avoid `..`).
# user_knowledge_dir = ~/dotfiles/lk-knowledge

# Detect potential secrets when exporting user-scope entries (default: true)
secret_detection = true
```

### Environment variable overrides

Environment variables take precedence over config file values:

| Variable | Effect |
|----------|--------|
| `LK_NO_AUTO_SYNC=1` | Disable auto-sync |
| `LK_COMMAND_LOG=1` | Enable command logging |

### Auto-sync

When enabled (default), `lk` automatically syncs `.knowledge/` markdown files before commands like `search`, `get`, `list`, etc. This means after `git pull`, the next `lk` command picks up shared knowledge changes without manual `lk sync`.

The sync is hash-based — if no files have changed, the overhead is negligible.

### Secret detection

When enabled (default), `lk add` and `lk export` scan content for potential secrets (API keys, tokens, private keys, credentials). If detected, the command is blocked with a warning. Use `--allow-secrets` to override.

### GitHub PR diff collapsing

By default, `lk init` adds `.knowledge/**/*.md linguist-generated=true` to `.gitattributes`, which collapses knowledge markdown diffs in GitHub PRs (they can still be expanded by clicking). To disable this and show full diffs, set `gitattributes_generated = false` in `config.toml` and re-run `lk init`.

### Command logging

When enabled, all `lk` commands are logged to `.knowledge/command.log` with timestamps. View recent entries:

```bash
lk command-log        # Show last 20 entries
lk command-log -n 50  # Show last 50 entries
```

## Supported Platforms

| Platform | Architecture | Note |
|----------|-------------|------|
| macOS    | Apple Silicon (aarch64) | |
| macOS    | Intel (x86_64) | |
| Linux    | ARM64 (aarch64) | |
| Linux    | x86_64 | |
| Windows  | x86_64 | `lk update` is not supported; use `cargo install` to update |

## License

[MIT](LICENSE)
