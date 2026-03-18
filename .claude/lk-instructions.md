## Knowledge Base (local-knowledge-cli)

This project has a local knowledge base.
If `lk` command is not available, install it first: `brew install syarihu/tap/lk && lk init`
Always run `lk` by command name (not full path) so it resolves via PATH.

### Design Philosophy
- **Shared knowledge** (`.knowledge/*.md`, git-tracked): Stable project knowledge — architecture, design decisions, conventions. Stale after 30 days (configurable).
- **Local knowledge** (DB only, git-ignored): LLM investigation cache — reduces context consumption when working on similar tasks repeatedly. Stale after 14 days (configurable). Do NOT export local cache to markdown; if stale, re-investigate instead.
- When capturing knowledge from a completed feature branch, use `/lk-knowledge-from-branch` to write shared markdown directly (not `lk add`).

### Pre-investigation Rule
- Before reading code with Read, Grep, or Glob tools, first run `lk search "<keyword>" --json --limit 5` to check existing knowledge
- Use `--full` to include full content directly: `lk search "<keyword>" --json --full --limit 5`
- If results are found and `--full` was not used, use `lk get <id> --json` for details
- If a result has `"status": "deprecated"` with `"superseded_by": <id>`, use the superseding entry instead
- If a result has `"stale": true`, verify against the current code; if outdated update with `lk edit <id>`, if still correct run `lk edit <id> --touch` to reset the stale warning
- If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read

### Agent Launch Rule
When launching Explore or general-purpose agents for code investigation, always prepend the following instruction to the agent prompt:
> Before using Read/Grep/Glob, first run `lk search "<relevant keywords>" --json --full --limit 5` to check existing knowledge. If useful results are found, use that as your starting point. If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read.
>
> After completing your investigation, append a `## Knowledge to Save` section at the end of your response. This section captures reusable discoveries for the local knowledge base. Follow these rules:
> - Only include knowledge that is **non-trivial and reusable** — architectural patterns, design decisions, non-obvious behavior, key function/struct roles. Skip obvious or task-specific-only findings.
> - If `lk search` already returned an entry covering the same topic, do NOT re-include it. Only include genuinely new or corrected knowledge.
> - Follow Content Guidelines: use stable identifiers (function/struct names, module names), avoid volatile details (line numbers, exact counts, specific file paths).
> - NEVER include API keys, tokens, passwords, or secrets.
> - If no noteworthy new knowledge was discovered, write `## Knowledge to Save` followed by `None.` instead.
> - Use this exact format for each entry:
>
> ```
> ## Knowledge to Save
>
> ### Entry 1: <title>
> - **keywords**: kw1, kw2, kw3
> - **category**: <category-name>
> - **content**: <2-5 sentence description of the discovery. Include "why" alongside "what" when possible.>
>
> ### Entry 2: <title>
> ...
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
- When modifying code that relates to an existing knowledge entry, update that entry with `lk edit <id>`
- Use `--touch` flag when reviewing an entry and confirming it is still accurate
- Mark outdated entries with `lk edit <id> --status deprecated --superseded_by <new_id>`
- Local cache entries (source=local) become stale after 14 days — when stale, prefer re-investigation over updating

### Keywords Rule (when adding)
- Include feature names, screen names, or module names as keywords
(e.g., "login", "settings-screen", "auth-module")

### Search Rule (when searching)
- Search by both abstract topic AND concrete names
(e.g., `lk search "word book detail"` and `lk search "navigation"`)

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
