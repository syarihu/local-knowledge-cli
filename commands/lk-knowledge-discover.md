---
description: Explore the entire project and auto-generate knowledge markdown files for .knowledge/
allowed-tools: Bash(lk *), Bash(git *), Bash(ls *), Bash(wc *), Read, Glob, Grep, Agent
---

Explore the current project's codebase and automatically generate .knowledge/ markdown files to bootstrap the knowledge base.

## Arguments
$ARGUMENTS optionally specifies focus areas or depth (e.g., "architecture only", "focus on API layer", "deep"). If empty, do a comprehensive scan.

## Procedure

### Phase 1: Project Overview
1. Check existing knowledge: `lk list --json` and `ls .knowledge/` to avoid duplicating what's already known
2. Gather project metadata:
   - Read README.md, CLAUDE.md, and any docs/ directory
   - Check package manager files (package.json, Cargo.toml, go.mod, pyproject.toml, etc.)
   - Run `git log --oneline -20` to understand recent development activity
3. Identify the tech stack, frameworks, and project type

### Phase 2: Codebase Exploration
Use the Agent tool (subagent_type=Explore) to investigate these areas in parallel where possible:

1. **Architecture & Structure**
   - Directory layout and module organization
   - Entry points (main files, index files, app bootstrap)
   - Key abstractions and design patterns used

2. **Core Features & Logic**
   - Main business logic locations
   - Data models / schemas / types
   - API endpoints or CLI commands
   - Important algorithms or processing pipelines

3. **Configuration & Infrastructure**
   - Config files and environment variables
   - Database setup and migrations
   - CI/CD pipelines
   - Build system and scripts

4. **Conventions & Patterns**
   - Coding conventions (naming, file organization)
   - Error handling patterns
   - Testing patterns and test structure
   - Logging and observability

### Phase 3: Generate Knowledge Files
For each major area discovered, create a markdown file in `.knowledge/` following this format:

```markdown
---
keywords: [keyword1, keyword2, keyword3]
category: architecture|features|conventions|infrastructure
---

# Topic Title

## Entry: Subtopic 1
keywords: [specific, keywords]

2-5 sentences of factual content. Include file paths and class/function names.

## Entry: Subtopic 2
keywords: [specific, keywords]

2-5 sentences of factual content.
```

Organize files into subdirectories:
- `.knowledge/architecture/` - system design, module structure, data flow
- `.knowledge/features/` - feature-specific knowledge
- `.knowledge/conventions/` - coding patterns, style, testing approach
- `.knowledge/infrastructure/` - build, deploy, config, CI/CD

### Phase 4: Deprecate superseded entries
1. After syncing, check if any newly created entries replace existing ones
2. If so, mark old entries as deprecated: `lk edit <old_id> --status deprecated --superseded-by <new_id>`

### Phase 5: Sync & Report
1. Run `lk sync` to import all new markdown files into the DB
2. Run `lk stats` to show the updated knowledge base status
3. Present a summary to the user:
   - How many files were created and in which categories
   - List of topics covered
   - Suggestions for areas that need manual documentation (e.g., business logic nuances, team conventions)

## Guidelines
- Skip trivially obvious facts (e.g., "this is a Node.js project" if package.json exists)
- Focus on knowledge that would help a NEW developer (or Claude) understand the project quickly
- Stable facts are valuable (technology choices, function/struct names, architecture structure)
- Avoid **volatile details** that go stale quickly (line numbers, exact counts, specific file paths)
  - BAD: "release.yml (86 lines) has a 4-platform matrix" — line numbers and counts drift
  - GOOD: "Binary is embedded with include_str! to prevent MITM attacks on network-fetched commands"
- Each entry should be 2-5 sentences, factual and concise
- Reference function/struct names instead of line numbers
- Include **why** (design decisions, rationale) alongside **what** when possible
- Use both English and Japanese keywords if the project uses Japanese
- Do NOT overwrite existing .knowledge/ files — only create new ones or suggest updates
- If $ARGUMENTS specifies a focus area, prioritize that area but still do a light scan of others
- Ask the user for confirmation before writing files if more than 10 files would be created
