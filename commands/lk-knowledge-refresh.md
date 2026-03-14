---
description: Check all knowledge entries for staleness and update outdated ones
allowed-tools: Bash(lk *), Bash(wc *), Bash(git *), Read, Glob, Grep, Agent, Edit
---

Review all knowledge entries against the current codebase and update any that are outdated.

## Arguments
$ARGUMENTS optionally specifies a focus area (e.g., "architecture", "features") or entry IDs. If empty, review all entries.

## Procedure

### Phase 1: Collect entries and current state
1. Run `lk list --json` to get all entries
2. For each entry, run `lk get <id> --json` to get full content
3. Gather current codebase metrics for comparison:
   - `wc -l` on key source files to check line counts
   - `git log --oneline -10` for recent changes

### Phase 2: Identify stale entries
For each entry, check if it references:
- **Incorrect line numbers or line counts** — compare against actual files
- **Old command/file names** — e.g., renamed slash commands, moved files
- **Missing features** — new options, flags, or behaviors not documented
- **Wrong descriptions** — logic that has changed since the entry was written
- **Test entries** — entries that appear to be test data (e.g., "テスト知識")

Present a summary table to the user:
| ID | Title | Status | Issue |
|----|-------|--------|-------|
| #N | ... | Stale / OK / Delete? | what's wrong |

### Phase 3: Update with user confirmation
1. Ask user for confirmation before making changes
2. For entries sourced from `.knowledge/` markdown files (`source_file` field):
   - Edit the markdown file directly, then run `lk sync`
3. For local-only entries (no `source_file`):
   - Delete and re-add with corrected content via `lk add`
4. For test/junk entries:
   - Delete with `lk delete <id>` after confirmation

### Phase 4: Report
1. Run `lk sync` if any markdown files were edited
2. Run `lk stats` to show updated state
3. Summarize what was changed

## Guidelines
- Do NOT update entries just because line numbers shifted by a few lines — only flag significant changes
- Focus on factual accuracy: wrong names, missing features, incorrect descriptions
- Prefer editing `.knowledge/` markdown files over deleting/re-adding entries
- Keep entries concise (2-5 sentences) — don't inflate during updates
- Avoid removing entries that are still broadly correct even if slightly outdated
