---
description: Export local knowledge entries to shareable markdown files
allowed-tools: Bash(lk *), Bash(ls *), Bash(mv *), Glob, Grep, Read, Edit
---

Export local-only knowledge entries to .knowledge/ markdown files for sharing via git.

## Procedure
1. Run: `lk list --category local --json` to see what local entries exist
2. Show the user what will be exported
3. Run: `lk export` to generate markdown files in .knowledge/
4. Show the user which files were created
5. Check the existing directory structure under `.knowledge/` (e.g., `architecture/`, `features/`, `conventions/`, `infrastructure/`)
6. For each exported file (`exported-*.md`), determine which subdirectory it best fits into based on its content and keywords:
   - `architecture/` — module structure, data flow, system design
   - `features/` — specific features, commands, workflows
   - `conventions/` — coding conventions, naming rules, patterns
   - `infrastructure/` — CI/CD, deployment, build tooling
   - If a file contains entries spanning multiple categories, split or leave at root
7. Move files to the appropriate subdirectories and rename them to remove the `exported-` prefix (e.g., `exported-auth.md` → `features/auth.md`)
8. **Reference check**: After moving files, scan all `.knowledge/**/*.md` files for internal references (e.g., `[text](path.md)`, `see features/xxx.md`). If any references point to old paths that were moved, update them to the new paths.
9. Run `lk sync` to update the DB with the new file locations
10. Remind the user to `git add .knowledge/` and commit to share with the team
