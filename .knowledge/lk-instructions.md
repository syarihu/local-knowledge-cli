## Knowledge Base (local-knowledge-cli)

This project has a local knowledge base. Install: `brew install syarihu/tap/lk && lk init`
Always run `lk` by command name (not full path) so it resolves via PATH.

- **Shared knowledge** (`.knowledge/*.md`, git-tracked): Stable project knowledge. Use `/lk-knowledge-from-branch` to write shared markdown from a feature branch.
- **Local knowledge** (DB only, git-ignored): LLM investigation cache. If stale, re-investigate instead of updating.

### MCP Tools vs CLI

If the `lk-knowledge` MCP server is available, **prefer MCP tools over CLI commands** — they are faster (no per-call process spawn) and provide structured input/output natively.

| Action | MCP tool (preferred) | CLI fallback |
|--------|---------------------|-------------|
| Search | `search_knowledge` | `lk search "<query>" --json --full` |
| Add | `add_knowledge` | `lk add "<title>" --keywords "kw1,kw2" --content "..."` |
| List | `list_knowledge` | `lk list --json` |
| Get by ID | `get_knowledge` | `lk get <id> --json` |
| Update | `update_knowledge` | `lk edit <id> --content "..."` |
| Supersede | `supersede_knowledge` | `lk supersede <old_id> <new_id>` |
| Stats | `get_stats` | `lk stats --json` |

Slash commands (`/lk-knowledge-discover`, `/lk-knowledge-refresh`, etc.) provide complex multi-step workflows that are not available as MCP tools — continue using those as slash commands.

### When to Search Knowledge

Search **before investigating unfamiliar code or architecture**.

**Skip when:**
- The user specifies an exact file path or line to edit
- The task is mechanical (formatting, renaming, version bumps, git operations)
- You already have sufficient context from the current conversation

**Using results:**
- If a result has `"status": "deprecated"` or `"status": "superseded"` with `"superseded_by": <uid>`, use the superseding entry
- If a result has `"stale": true`, verify against current code, then update content (if outdated) or touch (if still correct)
- If no results found, proceed with normal code exploration (Glob/Grep/Read)

### How to Search
- **Use 1–3 short keywords**, separated by spaces
  - BAD: `lk search "ユーザー認証APIのエンドポイント設計について"`, `lk search "auth-API"`
  - GOOD: `search_knowledge(query: "auth API")` or `lk search "auth API"`
- **Try both English and Japanese** — knowledge may be stored in either language
- If no results, broaden by using fewer keywords (e.g., "auth API endpoint" → "auth")

### Saving Knowledge
After investigating unfamiliar code, save noteworthy discoveries. Skip for mechanical tasks.

- MCP: `add_knowledge(title: "...", content: "...", keywords: ["kw1", "kw2"], category: "features")`
- CLI: `lk add "<title>" --keywords "kw1,kw2" --content "..." --category "features"`
- If add returns duplicate/similar entries, use `update_knowledge` / `lk edit <id>` to update instead
- Use `force: true` / `--force` to skip duplicate check when certain a new entry is needed
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
2. For each entry, use `add_knowledge` (MCP) or `lk add "<title>" --keywords "<kw1,kw2>" --category "<category>" --content "<content>" --json` (CLI).
3. If add returns similar entries, use `update_knowledge` / `lk edit <id>` to merge instead.

### Design Decisions (ADR)

When a design decision is made during a conversation (technology choice, architecture change, pattern adoption), record it as an ADR entry:

- MCP: `add_knowledge(title: "...", content: "...", keywords: ["adr", ...], category: "decisions", status: "proposed")`
- CLI: `lk add "..." --keywords "adr,..." --category "decisions" --content "..."`
- After approval: `update_knowledge(id: <id>, status: "accepted")` or `lk edit <id> --status accepted`
- To replace a previous decision: `supersede_knowledge(old_id: <old>, new_id: <new>)` or `lk supersede <old_id> <new_id>`

Content should follow the ADR format (Context / Decision / Alternatives Considered / Consequences).

Status flow: `proposed` → `accepted` (or `superseded` if replaced by a newer decision).

### Available CLI Commands
- `lk search "<query>" --json --full` - Search with full content
- `lk get <id> --json` - Get entry details
- `lk add "<title>" --keywords "kw1,kw2" --content "..." --category "cat"` - Add (checks duplicates; use `--force` to skip)
- `lk list --json` - List entries (supports `--category`, `--source`, `--status`, `--limit N`, `--offset N`)
- `lk edit <id> --content "..."` - Update entry (also: `--title`, `--keywords`, `--touch`, `--status <status> --superseded-by <id>`)
- `lk supersede <old_id> <new_id>` - Mark old entry as superseded by new (bidirectional link)
- `lk purge --source local` - Bulk delete
- `lk export` / `lk export --ids 1,2,3` / `lk export --query "auth"` - Export entries
- `lk sync` - Sync markdown files with DB (`--write-uids` to write UIDs back to markdown)
- Skills: `/lk-knowledge-search` `/lk-knowledge-add-db` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write-md` `/lk-knowledge-discover` `/lk-knowledge-refresh` `/lk-knowledge-from-branch` `/lk-knowledge-export-select`
