---
description: Extract knowledge entries from the current branch diff before merging
allowed-tools: Bash(lk *), Bash(git *), Bash(gh *), Bash(wc *), Read, Write, Glob, Grep, Agent
---

Extract knowledge from the current branch's diff and write it as shared markdown files in `.knowledge/`. Useful before merging a feature branch to capture what was built, changed, or learned as team-shared knowledge.

## Arguments
$ARGUMENTS optionally specifies the base branch to diff against (e.g., "main", "develop"). If empty, auto-detect.

## Procedure

### Phase 1: Detect Base Branch & Collect Diff

1. **Detect base branch** (in priority order):
   a. If `$ARGUMENTS` is non-empty, use it as the base branch
   b. Try `gh pr view --json baseRefName -q .baseRefName 2>/dev/null` (PR already exists)
   c. Try `git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's|refs/remotes/origin/||'` (remote default)
   d. Fallback: check existence of `main`, `master`, `develop` in order (`git rev-parse --verify <branch> 2>/dev/null`)

2. **Verify** the detected base branch exists. If not, report an error and stop.

3. **Collect diff information**:
   ```bash
   git diff $BASE...HEAD --stat          # Changed files summary
   git log $BASE..HEAD --oneline         # Commit history
   git diff $BASE...HEAD                 # Full diff
   ```

4. **Measure diff size**: count changed files and total changed lines. Report the base branch and diff size to the user.

### Phase 2: Analyze the Diff

- If the diff is small (<=20 files AND <=2000 lines changed), analyze it directly.
- If the diff is large (>20 files OR >2000 lines changed), use the Agent tool (subagent_type=Explore) to investigate changed files and their surrounding context in parallel, grouped by category (e.g., new features, refactors, infrastructure changes).

**Analysis perspectives:**
- New features: design intent, how they work, key entry points
- Architecture changes: structural shifts, new patterns introduced
- New conventions or patterns: coding rules, naming changes, new abstractions
- Gotchas and caveats: tricky parts, non-obvious behavior, edge cases
- Migration or breaking changes: what consumers need to know

For each perspective, read the changed files and surrounding code to understand context, not just the diff lines.

### Phase 3: Generate Shared Knowledge Markdown

Write knowledge as `.knowledge/` markdown files (NOT `lk add` to local DB). These files are git-tracked and shared with the team.

**File naming**: `.knowledge/{topic}.md` — use a descriptive topic name derived from the branch or feature (e.g., `auth-refactor.md`, `payment-module.md`).

**File format**:
```markdown
---
keywords: [keyword1, keyword2]
category: features
---

# Topic Title

## Entry: Subtopic 1
keywords: [specific, keywords]

2-5 sentences of factual content about this subtopic.
Reference function/struct names, not line numbers.

## Entry: Subtopic 2
keywords: [specific, keywords]

2-5 sentences of factual content about this subtopic.
```

Rules:
- Choose appropriate categories: `features`, `architecture`, `conventions`, `infrastructure`
- Include the branch name or ticket number in keywords if identifiable from branch name or commit messages
- Each entry should be self-contained and useful to someone unfamiliar with this branch
- Do NOT create entries for trivial changes (version bumps, typo fixes, dependency updates with no logic change)
- Aim for 2-5 sentences per entry — stable facts, not volatile details
- Reference function/struct names instead of line numbers
- Check existing `.knowledge/` files first — update existing files if the topic already exists, rather than creating duplicates
- Run `lk sync` after writing files to import them into the local DB

### Phase 4: Report

1. Present a summary of what was created/updated:
   - List each markdown file with its entries and categories
   - Note any existing files that were updated rather than created new
2. Run `lk sync` to import the new files, then `lk stats` to show overall knowledge base status
3. Remind the user to review the generated files and commit them with the PR
4. If there are aspects of the branch that are hard to capture as knowledge (e.g., UX decisions, business context), mention them as suggestions for manual documentation

## Guidelines
- Focus on knowledge that would help someone understand this branch's changes WITHOUT reading the full diff
- Prefer fewer, higher-quality entries over many shallow ones
- If the current branch IS the base branch (e.g., on main), report that there's nothing to diff and stop
- When launching Explore agents, prepend: "Before using Read/Grep/Glob, first run `lk search \"<relevant keywords>\" --json --full --limit 5` to check existing knowledge."
