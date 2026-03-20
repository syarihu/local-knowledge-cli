## Knowledge Base (local-knowledge-cli)

This project has a local knowledge base. Install: `brew install syarihu/tap/lk && lk init`
Always run `lk` by command name (not full path) so it resolves via PATH.

- **Shared knowledge** (`.knowledge/*.md`, git-tracked): Stable project knowledge. Use `/lk-knowledge-from-branch` to write shared markdown from a feature branch.
- **Local knowledge** (DB only, git-ignored): LLM investigation cache. If stale, re-investigate instead of updating.

### When to Use lk search
Run `lk search "<keywords>" --json --full --limit 5` **before investigating unfamiliar code or architecture**.

**Skip lk search when:**
- The user specifies an exact file path or line to edit
- The task is mechanical (formatting, renaming, version bumps, git operations)
- You already have sufficient context from the current conversation

**Using results:**
- If a result has `"status": "deprecated"` with `"superseded_by": <id>`, use the superseding entry
- If a result has `"stale": true`, verify against current code, then run `lk edit <id> --content "..."` (if outdated) or `lk edit <id> --touch` (if still correct)
- If no results found, proceed with normal code exploration (Glob/Grep/Read)

### How to Search
- **Use 1–3 short keywords**, separated by spaces
  - BAD: `lk search "ユーザー認証APIのエンドポイント設計について"`, `lk search "auth-API"`
  - GOOD: `lk search "auth API"`
- **Try both English and Japanese** — knowledge may be stored in either language
- If no results, broaden by using fewer keywords (e.g., `lk search "auth API endpoint"` → `lk search "auth"`)

### Saving Knowledge
After investigating unfamiliar code, save noteworthy discoveries. Skip for mechanical tasks.

- `lk add "<title>" --keywords "kw1,kw2" --content "..." --category "features"` — saves to local DB
- If `lk add` returns `"added": false` with `similar_entries`, use `lk edit <id>` to update instead
- Use `--force` to skip duplicate check when certain a new entry is needed
- Use lowercase, hyphen-separated keywords (e.g., "auth-flow")
- **Content rules:** Use stable identifiers (function/struct names), not line numbers. Include "why" alongside "what". NEVER include secrets.

### Agent Launch Rule
When launching Explore or general-purpose agents **to investigate unfamiliar code**, prepend this instruction. Skip for mechanical tasks:
> **lk search first:** Before using Read/Grep/Glob, run `lk search "<keywords>" --json --full --limit 5`.
> - Use 1–3 space-separated keywords (e.g., "auth API" not "auth-API")
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

### Post-Explore Knowledge Capture
After an agent returns a `## Knowledge to Save` section:
1. If `None.`, skip.
2. For each entry: `lk add "<title>" --keywords "<kw1,kw2>" --category "<category>" --content "<content>" --json`
3. If `lk add` returns `"added": false`, use `lk edit <id>` to merge instead.

### Available Commands
- `lk search "<query>" --json --full` - Search with full content
- `lk get <id> --json` - Get entry details
- `lk add "<title>" --keywords "kw1,kw2" --content "..." --category "cat"` - Add (checks duplicates; use `--force` to skip)
- `lk list --json` - List entries (supports `--category`, `--source`, `--limit N`, `--offset N`)
- `lk edit <id> --content "..."` - Update entry (also: `--title`, `--keywords`, `--touch`, `--status deprecated --superseded-by <id>`)
- `lk purge --source local` - Bulk delete
- `lk export` / `lk export --ids 1,2,3` / `lk export --query "auth"` - Export entries
- `lk sync` - Sync markdown files with DB
- Skills: `/lk-knowledge-search` `/lk-knowledge-add-db` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write-md` `/lk-knowledge-discover` `/lk-knowledge-refresh` `/lk-knowledge-from-branch` `/lk-knowledge-export-select`
