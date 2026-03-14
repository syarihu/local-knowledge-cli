---
description: Search the local knowledge base for existing knowledge
allowed-tools: Bash(lk *)
---

Search the knowledge base for information relevant to the query.

## Arguments
$ARGUMENTS contains the search query.

## Procedure
1. Run: `lk search "$ARGUMENTS" --json --limit 5`
2. Display results as a compact list showing id, title, and snippet
3. If the user wants details on a specific entry, run: `lk get <id> --json`
4. Use the knowledge to answer questions without unnecessary code exploration
