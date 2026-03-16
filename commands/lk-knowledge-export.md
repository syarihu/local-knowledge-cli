---
description: Export local knowledge entries to shareable markdown files
allowed-tools: Bash(lk *), Bash(ls *), Bash(mv *), Glob, Grep, Read, Edit
---

Export local-only knowledge entries to .knowledge/ markdown files for sharing via git.

## Procedure

### Phase 1: Overview
1. Run: `lk list --source local --json` to get all local entries
2. If there are no local entries, inform the user and stop
3. Show a summary based on the count:
   - **10 entries or fewer**: Show the full list (ID, title, category, updated_at)
   - **11–50 entries**: Show the total count, then group by category with counts (e.g., "architecture: 8, features: 12, uncategorized: 5"). List only the 5 most recent entry titles as examples.
   - **50+ entries**: Show the total count and category breakdown only. Do NOT list individual entries.

### Phase 2: Ask export mode
4. Ask the user which mode they prefer:
   - **A) Export all** — Export every local entry at once
   - **B) Select entries** — Analyze and pick which entries to export
5. If the user chooses **A**, skip to Phase 4
6. If the user chooses **B**, proceed to Phase 3

### Phase 3: Analyze and select (only if mode B)

**Important: Avoid fetching all entries individually. Work in batches to save tokens.**

7. Use the list data already obtained in Phase 1 for classification. The title, category, and updated_at are usually sufficient to classify most entries. Only use `lk get <id> --json` for entries whose title alone is ambiguous (limit to at most 10 individual fetches).
8. For large sets (20+ entries), classify in groups rather than individually:
   - Group entries by category first
   - Classify entire groups when possible (e.g., "All 8 entries in 'debugging' category → Low value")
   - Only call out individual entries when they differ from their group's classification
9. Classify into:
   - **High value** — Stable architectural decisions, design rationale, non-obvious conventions that would help teammates
   - **Medium value** — Useful facts about specific features/modules, but may need polishing before sharing
   - **Low value / Personal** — Debugging notes, temporary investigation results, things only useful to the original author
   - **Stale / Redundant** — Outdated information, or entries that duplicate what's already in shared knowledge
10. Present the classification to the user:
    - For **20 or fewer** entries: list each entry individually with classification
    - For **20+** entries: show group-level summaries, only list individual entries that are exceptions to their group
    ```
    ### High value (recommended to export) — N entries
    - [ID] Title — reason  (only list individually if ≤20 total, otherwise summarize)

    ### Medium value (consider exporting) — N entries
    ...

    ### Low value (suggest keeping local) — N entries
    ...

    ### Stale / Redundant (suggest deleting) — N entries
    ...
    ```
11. Ask the user which entries to export. Accept:
    - "all high" / "all high and medium"
    - Specific IDs like "1,5,12"
    - Category-based like "all in architecture"
    - "all" to export everything after all
    - The user may also ask to delete stale entries at this point

### Phase 4: Export
12. Run the appropriate export command:
    - Mode A (all): `lk export`
    - Mode B (selected): `lk export --ids <comma-separated-ids>`
    - Mode B (by query): `lk export --query "<search term>"` when the user selected by topic
13. Show the user which files were created
14. Check the existing directory structure under `.knowledge/` (e.g., `architecture/`, `features/`, `conventions/`, `infrastructure/`)
15. For each exported file (`exported-*.md`), determine which subdirectory it best fits into based on its content and keywords:
    - `architecture/` — module structure, data flow, system design
    - `features/` — specific features, commands, workflows
    - `conventions/` — coding conventions, naming rules, patterns
    - `infrastructure/` — CI/CD, deployment, build tooling
    - If a file contains entries spanning multiple categories, split or leave at root
16. Move files to the appropriate subdirectories and rename them to remove the `exported-` prefix (e.g., `exported-auth.md` → `features/auth.md`)
17. **Reference check**: After moving files, scan all `.knowledge/**/*.md` files for internal references (e.g., `[text](path.md)`, `see features/xxx.md`). If any references point to old paths that were moved, update them to the new paths.
18. Run `lk sync` to update the DB with the new file locations

### Phase 5: Cleanup (only if mode B)
19. If the user agreed to delete stale/redundant entries in Phase 3, run: `lk delete <id> -y` for each
20. Remind the user to `git add .knowledge/` and commit to share with the team
