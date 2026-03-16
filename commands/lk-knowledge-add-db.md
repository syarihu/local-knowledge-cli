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
   - Identify relevant keywords
3. Show the proposed entries to the user for confirmation
4. For each confirmed entry, run:
   `lk add "<title>" --keywords "<kw1>,<kw2>" --content "<content>"`
5. If a new entry replaces or supersedes an existing entry, mark the old one:
   `lk edit <old_id> --status deprecated --superseded-by <new_id>`
6. Report what was saved

## Guidelines
- Keep entries granular: one concept per entry
- Content should be 2-5 sentences, factual and concise
- Stable facts are valuable (technology choices, function/struct names, architecture structure)
- Avoid **volatile details** that go stale quickly (line numbers, exact counts, specific file paths)
  - BAD: "The schema has 3 tables at db.rs:34-78" — line numbers and counts drift
  - GOOD: "DB uses FTS5 for full-text search; schema is defined in `init_db()`" — stays true
- Reference function/struct names instead of line numbers
- Include **why** (design decisions, rationale) alongside **what** when possible
- Keywords should include both English and Japanese terms if applicable
- When adding knowledge that replaces an older approach, check `lk add` output for `similar_entries` and mark old entries as deprecated with `lk edit <id> --status deprecated --superseded-by <new_id>`
