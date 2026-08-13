---
description: Canonical brief to prepend when delegating code investigation to Explore/general-purpose sub-agents — make them lk search first and return a "## Knowledge to Save" section
allowed-tools: Bash(lk *)
---

Use this when launching Explore or general-purpose sub-agents **to investigate unfamiliar code**. Prepend the brief below to the agent's prompt, then capture what it returns. Skip for mechanical tasks (formatting, renaming, version bumps, git operations).

## Prepend this to the sub-agent's prompt

> **lk search first:** Before using Read/Grep/Glob, run `lk search "<keywords>" --json --full --limit 5`.
> - Use 1–3 space-separated keywords (e.g., "auth API" not "auth-API")
> - Try both English and Japanese if first search finds nothing
> - If a result has `"stale": true`, verify against current code and include the correction in `## Knowledge to Save`
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

## After the agent returns

Process its `## Knowledge to Save` section:
1. If `None.`, skip.
2. For each entry, save it with the CLI (this command allows `Bash(lk *)`):
   `lk add "<title>" --keywords "<kw1,kw2>" --category "<category>" --content "<content>" --json`
   (Outside this slash command, the main agent may instead use the `add_knowledge` MCP tool when available.)
3. If add reports `added: false` with `similar_entries`, an entry with that title (or an all-but-identical one) already exists — use `lk edit <id-or-uid>` (or the `edit_knowledge` MCP tool) to merge into it instead of forcing a new entry. A successful add that also lists `possibly_related` needs no action: the entry is saved, and those are only shown for context.
