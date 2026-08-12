---
description: Add knowledge discovered in this conversation to the local DB
allowed-tools: Bash(lk *)
---

Extract and save knowledge from the current conversation to the local knowledge base.

## Arguments
$ARGUMENTS contains a description of what knowledge to save, or is empty to auto-extract from conversation.

## Procedure
1. Review the current conversation for useful facts/findings about the codebase
2. For each piece of knowledge:
   - Formulate a concise title (e.g., "Login OAuth flow", "Payment retry logic")
   - Write 2-5 sentences of factual content
   - Identify 5-10 curated keywords (key components, concepts, proper nouns — not every word in the content)
3. Show the proposed entries to the user for confirmation
4. For each confirmed entry, run:
   `lk add "<title>" --keywords "<kw1>,<kw2>" --content "<content>"`
5. If a new entry replaces or supersedes an existing entry, mark the old one:
   `lk edit <old_id> --status deprecated --superseded-by <new_id>`
6. Report what was saved

## Design Decisions (ADR)
When the knowledge is a design decision (technology choice, architecture change, pattern adoption), record it as an ADR entry instead of a plain note:
- `lk add "<title>" --keywords "adr,<kw>" --category decisions --status proposed --content "<content>"` (MCP: `add_knowledge(..., category: "decisions", status: "proposed")`)
- Content follows ADR format: **Context / Decision / Alternatives Considered / Consequences**
- Status flow: `proposed` → `accepted` (or `superseded` if replaced). After approval: `lk edit <id> --status accepted`
- Both `lk add` and the `add_knowledge` MCP tool accept `--status`/`status` directly, so a decision can be created at `proposed` in one step.
- To replace a previous decision: `lk supersede <old_id> <new_id>` (MCP: `supersede_knowledge`) — bidirectional link

## Guidelines
- Keep entries granular: one concept per entry
- Content should be 2-5 sentences, factual and concise
- Stable facts are valuable (technology choices, function/struct names, architecture structure)
- Avoid **volatile details** that go stale quickly (line numbers, exact counts, specific file paths)
  - BAD: "The schema has 3 tables at db.rs:34-78" — line numbers and counts drift
  - GOOD: "DB uses FTS5 for full-text search; schema is defined in `init_db()`" — stays true
- Reference function/struct names instead of line numbers
- Include **why** (design decisions, rationale) alongside **what** when possible
- Always pass 5-10 curated keywords; include both English and Japanese terms if applicable (omitting keywords falls back to noisier frequency-based auto-extraction)
- When adding knowledge that replaces an older approach, check `lk add` output for `similar_entries` (same title — the add was refused) or `possibly_related` (added anyway, listed for information) and mark genuinely superseded entries as deprecated with `lk edit <id> --status deprecated --superseded-by <new_id>`
