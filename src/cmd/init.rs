use crate::cmd::sync::sync_knowledge_dir;
use crate::cmd::update::install_embedded_commands;
use crate::db;
use crate::util::get_project_root;

pub fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    let root = get_project_root();
    let db_path = root.join(".knowledge").join("knowledge.db");

    // 1. Initialize DB
    if db_path.exists() {
        println!("DB already exists at {}", db_path.display());
    } else {
        db::init_db(&db_path)?;
        println!("Created DB at {}", db_path.display());
    }

    // 2. Create .knowledge/ directory structure
    let knowledge_dir = root.join(".knowledge");
    std::fs::create_dir_all(&knowledge_dir)?;
    if !knowledge_dir.join("README.md").exists() {
        std::fs::write(
            knowledge_dir.join("README.md"),
            "# Project Knowledge Base\n\n\
             This directory contains shared knowledge files for the project.\n\
             These files are managed by `lk` (local-knowledge-cli) and synced to a local SQLite DB.\n\n\
             ## Structure\n\
             - `architecture/` - System design and architecture knowledge\n\
             - `features/` - Feature-specific knowledge\n\
             - `conventions/` - Coding conventions and patterns\n\n\
             ## Format\n\
             Each markdown file uses YAML frontmatter for metadata and `## Entry:` headings to delimit entries.\n",
        )?;
        for subdir in ["architecture", "features", "conventions"] {
            std::fs::create_dir_all(knowledge_dir.join(subdir))?;
        }
        println!(
            "Created .knowledge/ directory at {}",
            knowledge_dir.display()
        );
    }

    // 3. Import existing .knowledge/ files
    let (conn, _) = db::open_db(&db_path)?;
    let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
    if stats.added > 0 {
        println!("Imported {} entries from .knowledge/", stats.added);
    }

    // 4. Update .gitignore
    let gitignore_path = root.join(".gitignore");
    let gitignore_entries = [
        ".knowledge/knowledge.db",
        ".knowledge/knowledge.db.bak.*",
        ".knowledge/search.log",
        ".knowledge/command.log",
    ];
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        let mut added = Vec::new();
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            let mut needs_newline = !content.ends_with('\n');
            for entry in &gitignore_entries {
                if !content.contains(entry) {
                    if needs_newline {
                        writeln!(f)?;
                        needs_newline = false;
                    }
                    writeln!(f, "{entry}")?;
                    added.push(*entry);
                }
            }
        }
        for entry in &added {
            println!("Added {entry} to .gitignore");
        }
    } else {
        let content = gitignore_entries.join("\n") + "\n";
        std::fs::write(&gitignore_path, content)?;
        println!("Created .gitignore");
    }

    // 5. Write instructions to .knowledge/lk-instructions.md and add import to CLAUDE.md
    let instructions_path = knowledge_dir.join("lk-instructions.md");
    let instructions_content = LK_INSTRUCTIONS_CONTENT;

    if instructions_path.exists() {
        let existing = std::fs::read_to_string(&instructions_path)?;
        if existing.trim() != instructions_content.trim() {
            std::fs::write(&instructions_path, instructions_content)?;
            println!("Updated {}", instructions_path.display());
        } else {
            println!("{} is already up-to-date", instructions_path.display());
        }
    } else {
        std::fs::write(&instructions_path, instructions_content)?;
        println!("Created {}", instructions_path.display());
    }

    // Add import line to AGENTS.md or CLAUDE.md
    // Priority: root AGENTS.md > root CLAUDE.md > .claude/CLAUDE.md > create root AGENTS.md
    let candidates = [
        root.join("AGENTS.md"),
        root.join("CLAUDE.md"),
        root.join(".claude").join("CLAUDE.md"),
    ];

    let import_line = "@.knowledge/lk-instructions.md";
    let old_import_line = "@.claude/lk-instructions.md";
    let old_marker = "## Knowledge Base (local-knowledge-cli)";

    // Migrate from legacy .claude/lk-instructions.md if it exists
    let legacy_instructions_path = root.join(".claude").join("lk-instructions.md");
    if legacy_instructions_path.exists() {
        std::fs::remove_file(&legacy_instructions_path)?;
        println!("Migrated .claude/lk-instructions.md -> .knowledge/lk-instructions.md");
    }

    // Migrate legacy import line in AGENTS.md / CLAUDE.md
    for candidate in &candidates {
        if candidate.exists() {
            let content = std::fs::read_to_string(candidate)?;
            if content.contains(old_import_line) {
                let new_content = content.replace(old_import_line, import_line);
                std::fs::write(candidate, new_content)?;
                println!("Updated import path in {}", candidate.display());
            }
        }
    }

    // Check if any candidate already contains the import line
    let already_imported = candidates.iter().any(|p| {
        p.exists()
            && std::fs::read_to_string(p)
                .map(|c| c.contains(import_line))
                .unwrap_or(false)
    });

    if already_imported {
        println!("lk import already exists in a project config file");
    } else {
        // Find the first existing file, or create AGENTS.md
        let target_path = candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .unwrap_or_else(|| root.join("AGENTS.md"));

        if target_path.exists() {
            let content = std::fs::read_to_string(&target_path)?;

            if content.contains(old_marker) {
                // Migrate: replace old inline section with import line
                let section_start = content.find(old_marker).unwrap();
                let rest = &content[section_start + old_marker.len()..];
                let section_end = rest
                    .match_indices("\n## ")
                    .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                    .map(|(i, _)| section_start + old_marker.len() + i)
                    .unwrap_or(content.len());

                let mut new_content = content[..section_start].to_string();
                new_content.push_str(import_line);
                new_content.push('\n');
                if section_end < content.len() {
                    new_content.push_str(&content[section_end..]);
                }
                std::fs::write(&target_path, new_content)?;
                println!(
                    "Migrated inline instructions to import in {}",
                    target_path.display()
                );
            } else {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&target_path)?;
                if !content.ends_with('\n') {
                    writeln!(f)?;
                }
                writeln!(f, "{import_line}")?;
                println!("Added import to {}", target_path.display());
            }
        } else {
            std::fs::write(&target_path, format!("{import_line}\n"))?;
            println!("Created {} with lk import", target_path.display());
        }
    }

    // 6. Create config.toml if it doesn't exist
    let config_path = knowledge_dir.join("config.toml");
    if !config_path.exists() {
        std::fs::write(&config_path, crate::config::DEFAULT_CONFIG_CONTENT)?;
        println!("Created {}", config_path.display());
    }

    // 7. Write .knowledge/.lk-version
    let version_path = knowledge_dir.join(".lk-version");
    std::fs::write(&version_path, format!("{}\n", crate::util::VERSION))?;

    // 8. Install embedded Claude commands
    install_embedded_commands()?;

    println!("\nInitialization complete!");
    Ok(())
}

const LK_INSTRUCTIONS_CONTENT: &str = "\
## Knowledge Base (local-knowledge-cli)\n\
\n\
This project has a local knowledge base.\n\
If `lk` command is not available, install it first: `brew install syarihu/tap/lk && lk init`\n\
Always run `lk` by command name (not full path) so it resolves via PATH.\n\
\n\
### Design Philosophy\n\
- **Shared knowledge** (`.knowledge/*.md`, git-tracked): Stable project knowledge — architecture, design decisions, conventions. Stale after 30 days (configurable).\n\
- **Local knowledge** (DB only, git-ignored): LLM investigation cache — reduces context consumption when working on similar tasks repeatedly. Stale after 7 days (configurable). Do NOT export local cache to markdown; if stale, re-investigate instead.\n\
- When capturing knowledge from a completed feature branch, use `/lk-knowledge-from-branch` to write shared markdown directly (not `lk add`).\n\
\n\
### Pre-investigation Rule\n\
- Before reading code with Read, Grep, or Glob tools, first run `lk search \"<keyword>\" --json --limit 5` to check existing knowledge\n\
- Use `--full` to include full content directly: `lk search \"<keyword>\" --json --full --limit 5`\n\
- If results are found and `--full` was not used, use `lk get <id> --json` for details\n\
- If a result has `\"status\": \"deprecated\"` with `\"superseded_by\": <id>`, use the superseding entry instead\n\
- If a result has `\"stale\": true`, **do not use it as-is**. Verify against current code with Grep/Read, then **must** run one of:\n\
  - `lk edit <id> --content \"...\" --keywords \"...\"` if outdated\n\
  - `lk edit <id> --touch` if still correct\n\
- If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read\n\
\n\
### Agent Launch Rule\n\
When launching Explore or general-purpose agents for code investigation, always prepend the following instruction to the agent prompt:\n\
> Before using Read/Grep/Glob, first run `lk search \"<relevant keywords>\" --json --full --limit 5` to check existing knowledge. If useful results are found, use that as your starting point. If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read.\n\
>\n\
> After completing your investigation, append a `## Knowledge to Save` section at the end of your response. This section captures reusable discoveries for the local knowledge base. Follow these rules:\n\
> - Only include knowledge that is **non-trivial and reusable** — architectural patterns, design decisions, non-obvious behavior, key function/struct roles. Skip obvious or task-specific-only findings.\n\
> - If `lk search` already returned an entry covering the same topic, do NOT re-include it. Only include genuinely new or corrected knowledge.\n\
> - Follow Content Guidelines: use stable identifiers (function/struct names, module names), avoid volatile details (line numbers, exact counts, specific file paths).\n\
> - NEVER include API keys, tokens, passwords, or secrets.\n\
> - If no noteworthy new knowledge was discovered, write `## Knowledge to Save` followed by `None.` instead.\n\
> - Use this exact format for each entry:\n\
>\n\
> ```\n\
> ## Knowledge to Save\n\
>\n\
> ### Entry 1: <title>\n\
> - **keywords**: kw1, kw2, kw3\n\
> - **category**: <category-name>\n\
> - **content**: <2-5 sentence description of the discovery. Include \"why\" alongside \"what\" when possible.>\n\
>\n\
> ### Entry 2: <title>\n\
> ...\n\
> ```\n\
\n\
### Post-Explore Knowledge Capture Rule\n\
After an Explore or general-purpose agent returns results containing a `## Knowledge to Save` section:\n\
1. If the section says `None.`, skip — no action needed.\n\
2. For each entry listed, run:\n\
   `lk add \"<title>\" --keywords \"<kw1,kw2>\" --category \"<category>\" --content \"<content>\" --json`\n\
3. If `lk add` returns `\"added\": false` with `similar_entries`, use `lk edit <id>` to merge the new information into the existing entry instead of creating a duplicate.\n\
4. Before running `lk add`, check keywords and categories against existing conventions using `lk list --json --limit 10` if unsure.\n\
5. Briefly report what was saved (e.g., \"Saved 2 knowledge entries from Explore results: <title1>, <title2>\").\n\
\n\
### Auto-accumulation of Knowledge\n\
- After investigating code or design, save noteworthy discoveries with `lk add \"<title>\" --keywords \"kw1,kw2\" --content \"...\"` — these go to the local DB as cache\n\
- If `lk add` returns `\"added\": false` with `similar_entries`, use `lk edit <id>` to update the existing entry instead of creating a duplicate\n\
- Use `--force` to skip duplicate check when you are certain a new entry is needed\n\
- When adding knowledge that replaces an older approach, mark the old entry: `lk edit <old_id> --status deprecated --superseded-by <new_id>`\n\
- Do not save trivial or obvious facts\n\
- Briefly report what was saved (e.g., \"Added to knowledge base: <title>\")\n\
\n\
### Content Guidelines (to prevent staleness)\n\
- Stable facts are valuable: technology choices, function/struct names, architecture structure\n\
- Avoid **volatile details** that go stale quickly: line numbers, exact counts, specific file paths\n\
- BAD: \"DB schema has 3 tables defined at src/db.rs:34-78\" — line numbers and counts drift\n\
- GOOD: \"DB uses FTS5 for full-text search; schema is defined in `init_db()`\" — stays true\n\
- Reference function/struct names instead of line numbers\n\
- Include **why** (design decisions, rationale) alongside **what** when possible\n\
\n\
### Content Safety Rule\n\
- NEVER save API keys, tokens, passwords, or secrets in knowledge entries\n\
- Before running `lk add`, verify the content does not contain sensitive data\n\
- If content references credentials, describe them abstractly (e.g., \"uses OAuth token from env var AUTH_TOKEN\")\n\
\n\
### Category/Keyword Consistency Rule\n\
- Before adding, check existing categories and keywords with `lk list --json` or `lk search` to align naming\n\
- Prefer existing category names over creating new ones\n\
- Use lowercase, hyphen-separated keywords (e.g., \"auth-flow\", not \"AuthFlow\" or \"auth_flow\")\n\
\n\
### Staleness Management Rule\n\
- When modifying code that relates to an existing knowledge entry, **must** update that entry with `lk edit <id>`\n\
- Use `--touch` flag when reviewing an entry and confirming it is still accurate\n\
- Mark outdated entries with `lk edit <id> --status deprecated --superseded_by <new_id>`\n\
- Local cache entries (source=local) become stale after 14 days — when stale, prefer re-investigation over updating\n\
\n\
### Keywords Rule (when adding)\n\
- Include feature names, screen names, or module names as keywords\n\
  (e.g., \"login\", \"settings-screen\", \"auth-module\")\n\
\n\
### Search Rule (when searching)\n\
- Search by both abstract topic AND concrete names\n\
  (e.g., `lk search \"word book detail\"` and `lk search \"navigation\"`)\n\
\n\
### Available Commands\n\
- `lk search \"<query>\" --json` - Search knowledge (use `--since`, `--category`, `--source`, `--full` to filter)\n\
- `lk search \"<query>\" --json --full` - Search with full content (no need for `lk get`)\n\
- `lk get <id> --json` - Get entry details\n\
- `lk add \"<title>\" --keywords \"kw1,kw2\" --content \"...\" --category \"features\"` - Add knowledge to local cache (checks duplicates)\n\
- `lk add \"<title>\" --force --content \"...\"` - Add knowledge (skip duplicate check)\n\
- `lk list --category \"features\" --source \"local\" --json` - List entries with filters (supports `--limit N` and `--offset N` for pagination)\n\
- `lk edit <id> --title \"...\" --keywords \"...\" --content \"...\"` - Edit existing entry\n\
- `lk edit <id> --status deprecated --superseded-by <new_id>` - Mark entry as deprecated\n\
- `lk purge --source local` / `lk purge --category features` - Bulk delete entries\n\
- `lk export` - Export all local entries / `lk export --ids 1,2,3` - Export specific entries / `lk export --query \"auth\"` - Export by search\n\
- `lk sync` - Sync markdown files with DB\n\
- `/lk-knowledge-search` `/lk-knowledge-add-db` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write-md` `/lk-knowledge-discover` `/lk-knowledge-refresh` `/lk-knowledge-from-branch` - Claude skills\n";
