---
description: "Save conversation context to lk knowledge base"
---

Review the conversation and extract important context to save via lk add.

## What to save
- Design decisions and their rationale (why that choice was made)
- Investigation results and discoveries (what was looked into and what was found)
- Discussion flow and conclusions (how the conclusion was reached)
- Unresolved issues and next steps

## Save rules
- category: context
- keywords: always include conversation-log + topic-specific keywords
- Do not duplicate content already saved in lk (check with lk search first)
- If continuing an existing entry, use update_knowledge to append

## What NOT to save
- Mechanical work records (formatting fixes, renames, etc.)
- Code itself (that lives in git)
- Content that duplicates already-saved confirmed knowledge
