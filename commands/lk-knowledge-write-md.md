---
description: Help write well-structured knowledge markdown files from code or design info
allowed-tools: Bash(lk *), Read, Glob, Grep
---

Help create or improve knowledge markdown files for the .knowledge/ directory.

## Arguments
$ARGUMENTS describes what knowledge to document (e.g., "login authentication flow", "payment module architecture").

## Procedure
1. If $ARGUMENTS references code or a feature, explore the codebase to understand it
2. Draft a knowledge markdown file following this format:

```markdown
---
keywords: [keyword1, keyword2, keyword3]
category: architecture|features|conventions|decisions
---

# Topic Title

## Entry: Subtopic 1
keywords: [specific, keywords]

2-5 sentences of factual content about this subtopic.
Include file paths and class/function names.

## Entry: Subtopic 2
keywords: [specific, keywords]

2-5 sentences of factual content about this subtopic.
```

3. Key rules for entries:
   - Each `## Entry:` should cover ONE concept/fact
   - 2-5 sentences per entry (keeps context consumption low)
   - Stable facts are valuable (technology choices, function/struct names, architecture structure)
   - Avoid **volatile details** that go stale quickly (line numbers, exact counts, specific file paths)
     - BAD: "15 commands defined at main.rs:18-157" — line numbers and counts drift
     - GOOD: "Commands use clap derive API; dispatch is in `main()` match block" — stays true
   - Reference function/struct names instead of line numbers
   - Include **why** (design decisions, rationale) alongside **what** when possible
   - Keywords should cover both the concept and implementation terms
4. Show the draft to the user for review
5. **ADR detection**: If any entries contain design decisions (technology choices, architecture rationale, "why A over B"), suggest to the user:
   - Use `category: decisions` with `keywords: [adr, ...]`
   - Use the ADR entry format with `status: proposed` (or `accepted` if already approved):
     ```
     ## Entry: Decision Title
     keywords: [adr, relevant-topic]
     status: proposed

     ### Context
     Why this decision was needed.

     ### Decision
     What was decided.

     ### Alternatives Considered
     What else was evaluated and why it was rejected.

     ### Consequences
     What this decision means going forward.
     ```
   - If the decision replaces a previous one, use `lk supersede <old_id> <new_id>` after syncing
6. After approval, save to the appropriate .knowledge/ subdirectory
7. Run `lk sync` to import the new file into the DB
8. If the new entries replace existing ones:
   - For decisions: `lk supersede <old_id> <new_id>` (sets status=superseded + bidirectional link)
   - For other entries: `lk edit <old_id> --status deprecated --superseded-by <new_id>`
