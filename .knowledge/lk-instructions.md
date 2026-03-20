## Knowledge Base (local-knowledge-cli)

This project has a local knowledge base.
If `lk` command is not available, install it first: `brew install syarihu/tap/lk && lk init`
Always run `lk` by command name (not full path) so it resolves via PATH.

### Design Philosophy
- **Shared knowledge** (`.knowledge/*.md`, git-tracked): Stable project knowledge — architecture, design decisions, conventions. Staleness threshold configurable in `config.toml`.
- **Local knowledge** (DB only, git-ignored): LLM investigation cache — reduces context consumption when working on similar tasks repeatedly. Staleness threshold configurable in `config.toml`. Do NOT export local cache to markdown; if stale, re-investigate instead.
- When capturing knowledge from a completed feature branch, use `/lk-knowledge-from-branch` to write shared markdown directly (not `lk add`).

### Pre-investigation Rule
- Before reading code with Read, Grep, or Glob tools, first run `lk search "<keyword>" --json --limit 5` to check existing knowledge
- Use `--full` to include full content directly: `lk search "<keyword>" --json --full --limit 5`
- If results are found and `--full` was not used, use `lk get <id> --json` for details
- If a result has `"status": "deprecated"` with `"superseded_by": <id>`, use the superseding entry instead
- If a result has `"stale": true`, **do not use it as-is**. Verify against current code with Grep/Read, then **must** run one of:
  - `lk edit <id> --content "..." --keywords "..."` if outdated
  - `lk edit <id> --touch` if still correct
- If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read

### Agent Launch Rule
When launching Explore or general-purpose agents for code investigation, always prepend the following instruction to the agent prompt:
> **lk search first:** Before using Read/Grep/Glob, run `lk search "<keywords>" --json --full --limit 5`.
> - Use 1–3 space-separated keywords (e.g., "auth API" not "auth-API" or "AuthAPI")
> - Try both English and Japanese if first search finds nothing
> - If a result has `"stale": true`, verify against current code and include correction in `## Knowledge to Save`
> - If no useful results, proceed with Glob/Grep/Read
>
> **After investigation**, append a `## Knowledge to Save` section (or `None.` if nothing new). Only include non-trivial, reusable discoveries. Do not duplicate existing entries. Never include secrets.
> Format:
> ```
> ## Knowledge to Save
>
> ### Entry 1: <title>
> - **keywords**: kw1, kw2, kw3
> - **category**: <category-name>
> - **content**: <2-5 sentences. Use stable identifiers (function/struct names), not line numbers. Include "why" alongside "what".>
> ```

### Post-Explore Knowledge Capture Rule
After an Explore or general-purpose agent returns results containing a `## Knowledge to Save` section:
1. If the section says `None.`, skip — no action needed.
2. For each entry listed, run:
   `lk add "<title>" --keywords "<kw1,kw2>" --category "<category>" --content "<content>" --json`
3. If `lk add` returns `"added": false` with `similar_entries`, use `lk edit <id>` to merge the new information into the existing entry instead of creating a duplicate.
4. Before running `lk add`, check keywords and categories against existing conventions using `lk list --json --limit 10` if unsure.
5. Briefly report what was saved (e.g., "Saved 2 knowledge entries from Explore results: <title1>, <title2>").

### Auto-accumulation of Knowledge
- After investigating code or design, save noteworthy discoveries with `lk add "<title>" --keywords "kw1,kw2" --content "..."` — these go to the local DB as cache
- If `lk add` returns `"added": false` with `similar_entries`, use `lk edit <id>` to update the existing entry instead of creating a duplicate
- Use `--force` to skip duplicate check when you are certain a new entry is needed
- When adding knowledge that replaces an older approach, mark the old entry: `lk edit <old_id> --status deprecated --superseded-by <new_id>`
- Do not save trivial or obvious facts
- Briefly report what was saved (e.g., "Added to knowledge base: <title>")

### Content Guidelines (to prevent staleness)
- Stable facts are valuable: technology choices, function/struct names, architecture structure
- Avoid **volatile details** that go stale quickly: line numbers, exact counts, specific file paths
- BAD: "DB schema has 3 tables defined at src/db.rs:34-78" — line numbers and counts drift
- GOOD: "DB uses FTS5 for full-text search; schema is defined in `init_db()`" — stays true
- Reference function/struct names instead of line numbers
- Include **why** (design decisions, rationale) alongside **what** when possible

### Content Safety Rule
- NEVER save API keys, tokens, passwords, or secrets in knowledge entries
- Before running `lk add`, verify the content does not contain sensitive data
- If content references credentials, describe them abstractly (e.g., "uses OAuth token from env var AUTH_TOKEN")

### Category/Keyword Consistency Rule
- Before adding, check existing categories and keywords with `lk list --json` or `lk search` to align naming
- Prefer existing category names over creating new ones
- Use lowercase, hyphen-separated keywords (e.g., "auth-flow", not "AuthFlow" or "auth_flow")

### Staleness Management Rule
- When modifying code that relates to an existing knowledge entry, **must** update that entry with `lk edit <id>`
- Use `--touch` flag when reviewing an entry and confirming it is still accurate
- Mark outdated entries with `lk edit <id> --status deprecated --superseded-by <new_id>`
- Local cache entries (source=local) have a shorter stale threshold — when stale, prefer re-investigation over updating

### Keywords Rule (when adding)
- Include feature names, screen names, or module names as keywords
(e.g., "login", "settings-screen", "auth-module")

### Search Rule (when searching)
- **Use 1–3 short keywords per search** (do not use long phrases or sentences)
  - BAD: `lk search "ユーザー認証APIのエンドポイント設計について"`
  - GOOD: `lk search "auth API"`
- **Always use spaces to separate search terms** (never hyphens, underscores, or CamelCase)
  - BAD: `lk search "auth-API"`, `lk search "settings_screen"`, `lk search "AuthFlow"`
  - GOOD: `lk search "auth API"`, `lk search "settings screen"`, `lk search "auth flow"`
- **Try both English and Japanese keywords** — knowledge may be stored in either language
  - Example: `lk search "auth"` finds nothing → also try `lk search "認証"`
- If no results found, broaden by using fewer or more general keywords
  (e.g., `lk search "auth API endpoint"` → `lk search "auth"`)

### Available Commands
- `lk search "<query>" --json` - Search knowledge (use `--since`, `--category`, `--source`, `--full` to filter)
- `lk search "<query>" --json --full` - Search with full content (no need for `lk get`)
- `lk get <id> --json` - Get entry details
- `lk add "<title>" --keywords "kw1,kw2" --content "..." --category "features"` - Add knowledge to local cache (checks duplicates)
- `lk add "<title>" --force --content "..."` - Add knowledge (skip duplicate check)
- `lk list --category "features" --source "local" --json` - List entries with filters (supports `--limit N` and `--offset N` for pagination)
- `lk edit <id> --title "..." --keywords "..." --content "..."` - Edit existing entry
- `lk edit <id> --status deprecated --superseded-by <new_id>` - Mark entry as deprecated
- `lk purge --source local` / `lk purge --category features` - Bulk delete entries
- `lk export` - Export all local entries / `lk export --ids 1,2,3` - Export specific entries / `lk export --query "auth"` - Export by search
- `lk sync` - Sync markdown files with DB
- `/lk-knowledge-search` `/lk-knowledge-add-db` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write-md` `/lk-knowledge-discover` `/lk-knowledge-refresh` `/lk-knowledge-from-branch` - Claude skills
