# lk - Local Knowledge CLI

A local knowledge base CLI for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Store, search, and share project knowledge using a local SQLite database and markdown files.

## Features

- Project-local knowledge base stored in `.knowledge/knowledge.db`
- Full-text search with trigram tokenizer (supports Japanese/CJK), keyword search, and LIKE fallback
- Smart query splitting — hyphens, underscores, and CamelCase are automatically split into separate tokens (e.g., `auth-API` → `auth` + `API`)
- Duplicate detection when adding entries (skip with `--force`)
- Sync knowledge from `.knowledge/` markdown files (shareable via Git)
- Auto-sync on command execution — no manual `lk sync` needed after `git pull`
- Export local entries to markdown for team sharing (stable output order)
- Secret detection — warns when content contains API keys, tokens, or credentials
- Project config via `.knowledge/config.toml` (git-tracked, team-shareable)
- Bulk delete with `purge` by category or source
- Auto-extract keywords from entries
- Self-update from GitHub Releases
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
  init              Initialize knowledge base for current project
  add <title>       Add a knowledge entry (with duplicate detection)
  search <query>    Search knowledge entries (with relevance scoring)
  get <id>          Get a single entry by ID
  edit <id>         Edit an existing entry
  delete <id>       Delete an entry
  purge             Delete all entries by category or source
  list              List all entries
  sync              Sync .knowledge/ files with DB
  export            Export local entries to markdown
  import <path>     Import a markdown file
  keywords          List all unique keywords
  stats             Show database statistics
  search-log        Show recent search log entries
  update            Update lk to latest version
  install-commands  Install Claude Code slash commands
  mcp               Start MCP server (JSON-RPC 2.0 over stdio)
  install-mcp       Install lk as MCP server for Claude Code / Claude Desktop
  uninstall-mcp     Uninstall lk MCP server from Claude Code / Claude Desktop
```

### Common Options

- `--json` - Output as JSON (available on most commands)
- `--keywords "kw1,kw2"` - Comma-separated keywords (for `add`)
- `--content "..."` - Entry content (for `add`)
- `--category <cat>` - Filter by category (for `search`, `list`, `purge`)
- `--source <src>` - Filter by source: `local` or `shared` (for `search`, `list`, `purge`)
- `--limit <n>` - Max results, default 5 (for `search`)
- `--since <YYYY-MM-DD>` - Only return entries updated since this date (for `search`)
- `--full` - Include full content in JSON output, eliminating the need for `lk get` (for `search`)
- `--force` - Skip duplicate check when adding (for `add`)
- `--allow-secrets` - Allow content that contains potential secrets (for `add`, `export`)

## How It Works

### Storage

All lk-managed files are stored under the `.knowledge/` directory:

- **SQLite DB** at `.knowledge/knowledge.db` (git-ignored) - local search index
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

### Team workflow

1. Run `lk init` in your project — each team member runs this once after cloning
2. Claude Code automatically discovers and saves knowledge as you work (`lk add`)
3. Run `lk export` to write local knowledge to `.knowledge/` markdown files, then commit and push — only export knowledge worth sharing with the team
4. After pulling changes, shared knowledge is **auto-synced** on the next `lk` command — no manual `lk sync` needed
5. Use `/lk-knowledge-discover` to bootstrap knowledge for a new project, or `/lk-knowledge-refresh` to update stale entries

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

The API uses JWT tokens for authentication...

## Entry: Rate Limiting
keywords: [api, rate-limit]

Rate limit is 100 requests per minute per API key...
```

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
| `update_knowledge` | Update title, content, keywords, or status of an entry |
| `get_stats` | Get knowledge base statistics |
| `list_projects` | List registered projects (multi-project mode only) |

No manual server startup is needed — Claude Code / Claude Desktop automatically launches `lk mcp` when a tool is called. When multiple projects are registered, each tool accepts an optional `project` parameter to specify which project to operate on.

### Slash Commands

`lk init` creates `.knowledge/lk-instructions.md` with Claude Code instructions and adds an `@.knowledge/lk-instructions.md` import line to your `AGENTS.md` (or `CLAUDE.md` if it exists). This keeps your config file minimal while providing full instructions to Claude Code via the [`@import` syntax](https://docs.anthropic.com/en/docs/claude-code/memory#import-additional-files).

After `lk init`, Claude Code will automatically:

1. Search the knowledge base before exploring code
2. Add new discoveries via `/lk-knowledge-add-db`
3. Use slash commands: `/lk-knowledge-search`, `/lk-knowledge-add-db`, `/lk-knowledge-export`, `/lk-knowledge-export-select`, `/lk-knowledge-sync`, `/lk-knowledge-write-md`, `/lk-knowledge-discover`, `/lk-knowledge-refresh`, `/lk-knowledge-from-branch`

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
lk search-log        # Show last 20 entries
lk search-log -n 50  # Show last 50 entries
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
