# lk - Local Knowledge CLI

A local knowledge base CLI for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Store, search, and share project knowledge using a local SQLite database and markdown files.

## Features

- Project-local knowledge base stored in `.knowledge/knowledge.db`
- Full-text search with trigram tokenizer (supports Japanese/CJK) and LIKE fallback
- Duplicate detection when adding entries (skip with `--force`)
- Sync knowledge from `.knowledge/` markdown files (shareable via Git)
- Export local entries to markdown for team sharing
- Bulk delete with `purge` by category or source
- Auto-extract keywords from entries
- Self-update from GitHub Releases
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

## How It Works

### Storage

All lk-managed files are stored under the `.knowledge/` and `.claude/` directories:

- **SQLite DB** at `.knowledge/knowledge.db` (git-ignored) - local search index
- **Markdown files** in `.knowledge/` (git-tracked) - shareable knowledge
- **Version file** at `.knowledge/.lk-version` (git-tracked) - minimum required lk version for the project
- **Instructions** at `.claude/lk-instructions.md` (git-tracked) - Claude Code instructions, imported via `@` syntax
- **Command log** at `.knowledge/command.log` (git-ignored) - optional command logging

### What to commit

| Path | Git | Description |
|------|-----|-------------|
| `.knowledge/*.md` | Yes | Shared knowledge (markdown files) |
| `.knowledge/.lk-version` | Yes | Minimum required lk version |
| `.claude/lk-instructions.md` | Yes | Claude Code instructions |
| `CLAUDE.md` or `.claude/CLAUDE.md` | Yes | Contains `@.claude/lk-instructions.md` import |
| `.knowledge/knowledge.db` | No (auto-ignored) | Local search index |
| `.knowledge/command.log` | No (auto-ignored) | Command log |

### Shared vs local knowledge

Knowledge entries have two categories:

- **Shared** (`.knowledge/` markdown files, git-tracked) — Architecture, design decisions, team conventions, and other knowledge that the whole team should know. Export with `lk export` and commit.
- **Local** (DB only, git-ignored) — Personal investigation notes, debugging findings, and frequently used facts specific to your workflow. These stay on your machine and don't need to be shared.

Not everything needs to be shared. A good rule of thumb: if it would help a new team member or Claude understand the project, export it. If it's only useful to you during active development, keep it local.

### Team workflow

1. Run `lk init` in your project — each team member runs this once after cloning
2. Claude Code automatically discovers and saves knowledge as you work (`lk add`)
3. Run `lk export` to write local knowledge to `.knowledge/` markdown files, then commit and push — only export knowledge worth sharing with the team
4. When pulling changes, run `lk sync` to import new/updated `.knowledge/` files into your local DB
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

`lk init` creates `.claude/lk-instructions.md` with Claude Code instructions and adds an `@.claude/lk-instructions.md` import line to your CLAUDE.md. This keeps your CLAUDE.md minimal while providing full instructions to Claude Code via the [`@import` syntax](https://docs.anthropic.com/en/docs/claude-code/memory#import-additional-files).

After `lk init`, Claude Code will automatically:

1. Search the knowledge base before exploring code
2. Add new discoveries via `/lk-knowledge-add-db`
3. Use slash commands: `/lk-knowledge-search`, `/lk-knowledge-add-db`, `/lk-knowledge-export`, `/lk-knowledge-sync`, `/lk-knowledge-write-md`, `/lk-knowledge-discover`, `/lk-knowledge-refresh`, `/lk-knowledge-from-branch`

## Search Logging

Search logging is disabled by default. To enable it, set the `LK_SEARCH_LOG` environment variable:

```bash
# Enable search logging for a single command
LK_SEARCH_LOG=1 lk search "query"

# Or export it to enable for the entire session
export LK_SEARCH_LOG=1
```

Logs are written to `.knowledge/search.log` with timestamp, query, result count, and matched titles:

```
[2026-03-14T13:57:48] query="database" results=2 titles=["Database Configuration", "Project Root Detection"]
```

View recent log entries:

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
