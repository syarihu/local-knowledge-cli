---
description: Export local knowledge entries to shareable markdown files
allowed-tools: Bash(lk *)
---

Export local-only knowledge entries to .knowledge/ markdown files for sharing via git.

## Procedure
1. Run: `lk list --category local --json` to see what local entries exist
2. Show the user what will be exported
3. Run: `lk export` to generate markdown files in .knowledge/
4. Show the user which files were created
5. Remind the user to `git add .knowledge/` and commit to share with the team
