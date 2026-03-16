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
5. Report what was saved

## Guidelines
- Keep entries granular: one concept per entry
- Content should be 2-5 sentences, factual and concise
- Focus on **why** (design decisions, rationale, trade-offs) rather than **what** (line numbers, counts, implementation details that can be read from code)
  - BAD: "The schema has 3 tables at db.rs:34-78" — goes stale when code changes
  - GOOD: "FTS5 is used for full-text search to avoid external dependencies" — stays true
- Reference function/struct names rather than line numbers
- Keywords should include both English and Japanese terms if applicable
