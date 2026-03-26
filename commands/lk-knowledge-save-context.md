---
description: Save conversation context to lk knowledge base
allowed-tools: Bash(lk *)
---

Review the conversation and extract important context to save as knowledge entries.

## Arguments
$ARGUMENTS contains a hint about what to save, or is empty to auto-extract from conversation.

## Procedure
1. Review the current conversation for important context:
   - Design decisions and their rationale (why that choice was made)
   - Investigation results and discoveries (what was looked into and what was found)
   - Discussion flow and conclusions (how the conclusion was reached)
   - Unresolved issues and next steps
2. Check for existing entries: `lk search "<topic>" --json --full --category context`
   - If a matching entry exists, update it with `lk edit <id> --content "..."` instead of creating a new one
3. For each entry, run:
   `lk add "<title>" --keywords "conversation-log,<kw1>,<kw2>" --content "<content>" --category context`
4. Report what was saved

## Guidelines
- category must be `context`
- keywords must always include `conversation-log` plus topic-specific keywords
- Content should summarize the flow: what was investigated → what was found → what was decided
- Do not save mechanical work records (formatting fixes, renames, etc.)
- Do not save code itself (that lives in git)
- Do not duplicate content already saved as confirmed knowledge entries
