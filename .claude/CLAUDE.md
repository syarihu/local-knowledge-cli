## Knowledge Base (local-knowledge-cli)

This project has a local knowledge base.

### Pre-investigation Rule
- Before reading code with Read, Grep, or Glob tools, first run `lk search "<keyword>" --json --limit 5` to check existing knowledge
- If results are found, use `lk get <id> --json` for details — skip unnecessary code exploration

### Auto-accumulation of Knowledge
- After investigating code or design, save noteworthy discoveries with `lk add "<title>" --keywords "kw1,kw2" --content "..."`
- Do not save trivial or obvious facts
- Briefly report what was saved (e.g., "Added to knowledge base: <title>")

### Available Commands
- `lk search "<query>" --json` - Search knowledge (use `--since YYYY-MM-DD` to filter by date)
- `lk get <id> --json` - Get entry details
- `lk add "<title>" --keywords "kw1,kw2" --content "..."` - Add knowledge
- `lk edit <id> --title "..." --keywords "..." --content "..."` - Edit existing entry
- `lk sync` - Sync markdown files with DB
- `/lk-knowledge-search` `/lk-knowledge-add` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write` `/lk-knowledge-discover` `/lk-knowledge-refresh` - Claude skills
