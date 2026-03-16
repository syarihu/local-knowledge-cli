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
category: architecture|features|conventions
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
5. After approval, save to the appropriate .knowledge/ subdirectory
6. Run `lk sync` to import the new file into the DB
7. If the new entries replace existing ones, mark old entries as deprecated:
   `lk edit <old_id> --status deprecated --superseded-by <new_id>`
