mod db;
mod keywords;
mod markdown;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "lk", version = VERSION, about = "Local knowledge base CLI for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize knowledge base for current project
    Init,
    /// Add a knowledge entry
    Add {
        /// Entry title
        title: String,
        /// Comma-separated keywords
        #[arg(short, long)]
        keywords: Option<String>,
        /// Entry content
        #[arg(short, long)]
        content: Option<String>,
        /// Category (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Skip duplicate check and force add
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search knowledge entries
    Search {
        /// Search query
        query: String,
        /// Search keywords only
        #[arg(long)]
        keyword_only: bool,
        /// Filter by category (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Filter by source ("local" or "shared")
        #[arg(long)]
        source: Option<String>,
        /// Only return entries updated since this date (e.g., 2026-01-01)
        #[arg(long)]
        since: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "5")]
        limit: usize,
        /// Include full content in JSON output (eliminates need for lk get)
        #[arg(long)]
        full: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single entry by ID
    Get {
        /// Entry ID
        id: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete an entry
    Delete {
        /// Entry ID
        id: i64,
    },
    /// Delete all entries in a category or by source
    Purge {
        /// Category to purge (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Source to purge (e.g., "local", "shared")
        #[arg(long)]
        source: Option<String>,
    },
    /// List all entries
    List {
        /// Filter by category (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Filter by source ("local" or "shared")
        #[arg(long)]
        source: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Sync .knowledge/ files with DB
    Sync {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Export local entries to markdown
    Export {
        /// Output directory
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Import a markdown file
    Import {
        /// Path to markdown file
        path: PathBuf,
    },
    /// List all unique keywords
    Keywords {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show database statistics
    Stats {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit an existing entry
    Edit {
        /// Entry ID
        id: i64,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New keywords (comma-separated)
        #[arg(long)]
        keywords: Option<String>,
        /// New content
        #[arg(long)]
        content: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show recent search log entries
    SearchLog {
        /// Number of recent entries to show
        #[arg(short = 'n', long, default_value = "20")]
        lines: usize,
    },
    /// Update lk to latest version
    Update,
    /// Install embedded Claude commands to ~/.claude/commands/
    InstallCommands,
    /// Remove knowledge base from current project
    Uninstall,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => cmd_init(),
        Commands::Add { title, keywords, content, category, force, json } => {
            cmd_add(&title, keywords.as_deref(), content.as_deref(), category.as_deref(), force, json)
        }
        Commands::Search { query, keyword_only, category, source, since, limit, full, json } => {
            cmd_search(&query, keyword_only, category.as_deref(), source.as_deref(), since.as_deref(), limit, full, json)
        }
        Commands::Get { id, json } => cmd_get(id, json),
        Commands::Edit { id, title, keywords, content, json } => {
            cmd_edit(id, title.as_deref(), keywords.as_deref(), content.as_deref(), json)
        }
        Commands::Delete { id } => cmd_delete(id),
        Commands::Purge { category, source } => cmd_purge(category.as_deref(), source.as_deref()),
        Commands::List { category, source, json } => cmd_list(category.as_deref(), source.as_deref(), json),
        Commands::Sync { json } => cmd_sync(json),
        Commands::Export { dir } => cmd_export(dir),
        Commands::Import { path } => cmd_import(&path),
        Commands::Keywords { json } => cmd_keywords(json),
        Commands::Stats { json } => cmd_stats(json),
        Commands::SearchLog { lines } => cmd_search_log(lines),
        Commands::Update => cmd_update(),
        Commands::InstallCommands => install_embedded_commands(),
        Commands::Uninstall => cmd_uninstall(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

// --- Project paths ---

fn get_project_root() -> PathBuf {
    let cwd = std::env::current_dir().expect("Cannot get current directory");
    let mut current = cwd.as_path();
    loop {
        if current.join(".git").exists()
            || current.join(".knowledge").exists()
            || current.join(".claude").exists()
        {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return cwd,
        }
    }
}

fn get_db_path() -> PathBuf {
    let root = get_project_root();
    let new_path = root.join(".knowledge").join("knowledge.db");
    let old_path = root.join(".claude").join("knowledge.db");
    // Auto-migrate DB location from .claude/ to .knowledge/
    if !new_path.exists() && old_path.exists() {
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if std::fs::rename(&old_path, &new_path).is_ok() {
            eprintln!("Note: Moved knowledge.db from .claude/ to .knowledge/");
        }
    }
    new_path
}

fn get_knowledge_dir() -> PathBuf {
    get_project_root().join(".knowledge")
}

/// Open DB and auto-sync .knowledge/ if a migration occurred.
fn open_db_with_migrate() -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let db_path = get_db_path();
    let (conn, migrated) = db::open_db(&db_path)?;
    if migrated {
        let root = get_project_root();
        let knowledge_dir = get_knowledge_dir();
        if knowledge_dir.exists() {
            let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
            eprintln!(
                "Note: DB migrated and re-synced .knowledge/ (added: {}, updated: {}, removed: {})",
                stats.added, stats.updated, stats.removed
            );
        }
    }
    Ok(conn)
}

// --- Commands ---

fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
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
        println!("Created .knowledge/ directory at {}", knowledge_dir.display());
    }

    // 3. Import existing .knowledge/ files
    let (conn, _) = db::open_db(&db_path)?;
    let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
    if stats.added > 0 {
        println!("Imported {} entries from .knowledge/", stats.added);
    }

    // 4. Update .gitignore
    let gitignore_path = root.join(".gitignore");
    let gitignore_entries = [".knowledge/knowledge.db", ".knowledge/search.log"];
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        let mut added = Vec::new();
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&gitignore_path)?;
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

    // 5. Add instructions to CLAUDE.md (or AGENTS.md)
    // Priority: root CLAUDE.md > root AGENTS.md > .claude/CLAUDE.md > create root CLAUDE.md
    let candidates = [
        root.join("CLAUDE.md"),
        root.join("AGENTS.md"),
        root.join(".claude").join("CLAUDE.md"),
    ];
    let claude_md_path = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| root.join("CLAUDE.md"));
    let marker = "## Knowledge Base (local-knowledge-cli)";

    if claude_md_path.exists() {
        let content = std::fs::read_to_string(&claude_md_path)?;
        if content.contains(marker) {
            // Check if the section is outdated and replace if so
            let section_start = content.find(marker).unwrap();
            let rest = &content[section_start + marker.len()..];
            let section_end = rest
                .match_indices("\n## ")
                .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                .map(|(i, _)| section_start + marker.len() + i)
                .unwrap_or(content.len());

            let existing = content[section_start..section_end].trim();
            let expected = CLAUDE_MD_SECTION.trim();

            if existing != expected {
                let mut new_content = content[..section_start].to_string();
                new_content.push_str(CLAUDE_MD_SECTION.trim_start());
                if section_end < content.len() {
                    new_content.push_str(&content[section_end..]);
                }
                std::fs::write(&claude_md_path, new_content)?;
                println!("Updated knowledge base instructions in {}", claude_md_path.display());
            } else {
                println!("CLAUDE.md already contains up-to-date knowledge base instructions");
            }
        } else {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&claude_md_path)?;
            if !content.ends_with('\n') {
                writeln!(f)?;
            }
            write!(f, "{CLAUDE_MD_SECTION}")?;
            println!("Added knowledge base instructions to {}", claude_md_path.display());
        }
    } else {
        std::fs::write(&claude_md_path, CLAUDE_MD_SECTION.trim_start())?;
        println!("Created {} with knowledge base instructions", claude_md_path.display());
    }

    // 6. Install embedded Claude commands
    install_embedded_commands()?;

    println!("\nInitialization complete!");
    Ok(())
}

const CLAUDE_MD_SECTION: &str = "\n\
## Knowledge Base (local-knowledge-cli)\n\
\n\
This project has a local knowledge base.\n\
If `lk` command is not available, install it first: `brew install syarihu/tap/lk && lk init`\n\
\n\
### Pre-investigation Rule\n\
- Before reading code with Read, Grep, or Glob tools, first run `lk search \"<keyword>\" --json --limit 5` to check existing knowledge\n\
- Use `--full` to include full content directly: `lk search \"<keyword>\" --json --full --limit 5`\n\
- If results are found and `--full` was not used, use `lk get <id> --json` for details\n\
- If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read\n\
\n\
### Agent Launch Rule\n\
When launching Explore or general-purpose agents for code investigation, always prepend the following instruction to the agent prompt:\n\
> Before using Read/Grep/Glob, first run `lk search \"<relevant keywords>\" --json --full --limit 5` to check existing knowledge. If useful results are found, use that as your starting point. If no results are found or the knowledge is insufficient, proceed with normal code exploration using Glob/Grep/Read.\n\
\n\
### Auto-accumulation of Knowledge\n\
- After investigating code or design, save noteworthy discoveries with `lk add \"<title>\" --keywords \"kw1,kw2\" --content \"...\"`\n\
- If `lk add` returns `\"added\": false` with `similar_entries`, use `lk edit <id>` to update the existing entry instead of creating a duplicate\n\
- Use `--force` to skip duplicate check when you are certain a new entry is needed\n\
- Do not save trivial or obvious facts\n\
- Briefly report what was saved (e.g., \"Added to knowledge base: <title>\")\n\
\n\
### Content Guidelines (to prevent staleness)\n\
- Focus on **why** (design decisions, rationale, trade-offs) rather than **what** (line numbers, counts, file paths)\n\
- BAD: \"DB schema has 3 tables defined at src/db.rs:34-78\" — goes stale when code changes\n\
- GOOD: \"FTS5 is used because it provides full-text search without external dependencies\" — stays true\n\
- Avoid embedding specific line numbers, exact counts, or volatile implementation details\n\
- If you must reference code locations, use function/struct names instead of line numbers\n\
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
- `lk add \"<title>\" --keywords \"kw1,kw2\" --content \"...\" --category \"features\"` - Add knowledge (checks duplicates)\n\
- `lk add \"<title>\" --force --content \"...\"` - Add knowledge (skip duplicate check)\n\
- `lk list --category \"features\" --source \"local\" --json` - List entries with filters\n\
- `lk edit <id> --title \"...\" --keywords \"...\" --content \"...\"` - Edit existing entry\n\
- `lk purge --source local` / `lk purge --category features` - Bulk delete entries\n\
- `lk sync` - Sync markdown files with DB\n\
- `/lk-knowledge-search` `/lk-knowledge-add-db` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write-md` `/lk-knowledge-discover` `/lk-knowledge-refresh` - Claude skills\n";

fn cmd_add(
    title: &str,
    keywords_str: Option<&str>,
    content: Option<&str>,
    category: Option<&str>,
    force: bool,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let content = content.unwrap_or("");
    let category = category.unwrap_or("");

    let mut kws: Vec<String> = if let Some(ks) = keywords_str {
        ks.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else {
        Vec::new()
    };

    // Auto-extract additional keywords
    let auto_kws = keywords::extract_keywords(title, content);
    for kw in auto_kws {
        let lower = kw.to_lowercase();
        if !kws.iter().any(|k| k.to_lowercase() == lower) {
            kws.push(kw);
        }
    }
    kws.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    // Duplicate check (skip with --force)
    if !force {
        let similar = db::find_similar_entries(&conn, title, &kws)?;
        if !similar.is_empty() {
            if json_output {
                let similar_json: Vec<serde_json::Value> = similar
                    .iter()
                    .map(|e| {
                        let ekws = db::get_keywords(&conn, e.id).unwrap_or_default();
                        let snippet = truncate_str(&e.content, 300);
                        serde_json::json!({
                            "id": e.id,
                            "title": e.title,
                            "keywords": ekws,
                            "snippet": snippet,
                        })
                    })
                    .collect();
                let out = serde_json::json!({
                    "added": false,
                    "similar_entries": similar_json,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Similar entries found (use --force to add anyway):");
                for e in &similar {
                    let ekws = db::get_keywords(&conn, e.id).unwrap_or_default();
                    println!("  [{}] {} (keywords: {})", e.id, e.title, ekws.join(", "));
                }
            }
            return Ok(());
        }
    }

    let entry_id = db::add_entry(&conn, title, content, &kws, category, "local", None, None)?;

    if json_output {
        let out = serde_json::json!({
            "added": true,
            "id": entry_id,
            "title": title,
            "keywords": kws,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Added entry #{entry_id}: {title}");
        println!("Keywords: {}", kws.join(", "));
    }
    Ok(())
}

fn cmd_search(
    query: &str,
    keyword_only: bool,
    category: Option<&str>,
    source: Option<&str>,
    since: Option<&str>,
    limit: usize,
    full: bool,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let mut results = db::search_entries(&conn, query, keyword_only, category, since, limit)?;
    if let Some(src) = source {
        results.retain(|e| e.source == src);
    }

    log_search(query, &results);

    if json_output {
        let output: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let kws = db::get_keywords(&conn, r.id).unwrap_or_default();
                let mut obj = serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "keywords": kws,
                    "category": r.category,
                    "source": r.source,
                    "score": r.rank,
                });
                if full {
                    obj["content"] = serde_json::Value::String(r.content.clone());
                } else {
                    obj["snippet"] = serde_json::Value::String(truncate_str(&r.content, 300));
                }
                obj
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if results.is_empty() {
        println!("No results found.");
    } else {
        for r in &results {
            let snippet = truncate_str(&r.content, 80);
            let kws = db::get_keywords(&conn, r.id).unwrap_or_default();
            println!("  [{}] {} ({})", r.id, r.title, r.category);
            println!("       Keywords: {}", kws.join(", "));
            println!("       {snippet}");
            println!();
        }
    }
    Ok(())
}

fn log_search(query: &str, results: &[db::Entry]) {
    if std::env::var("LK_SEARCH_LOG").unwrap_or_default() != "1" {
        return;
    }
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let log_path = get_project_root().join(".knowledge").join("search.log");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let titles: Vec<&str> = results.iter().take(5).map(|r| r.title.as_str()).collect();
        writeln!(
            f,
            "[{}] query=\"{}\" results={} titles={:?}",
            now_iso(),
            query,
            results.len(),
            titles,
        )?;
        Ok(())
    })();
}

fn cmd_search_log(lines: usize) -> Result<(), Box<dyn std::error::Error>> {
    let log_path = get_project_root().join(".knowledge").join("search.log");
    if !log_path.exists() {
        println!("No search log found.");
        return Ok(());
    }
    let content = std::fs::read_to_string(&log_path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    for line in &all_lines[start..] {
        println!("{line}");
    }
    Ok(())
}

fn cmd_get(id: i64, json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let entry = db::get_entry(&conn, id)?
        .ok_or_else(|| format!("Entry #{id} not found"))?;
    let kws = db::get_keywords(&conn, id)?;

    if json_output {
        let out = serde_json::json!({
            "id": entry.id,
            "title": entry.title,
            "content": entry.content,
            "keywords": kws,
            "category": entry.category,
            "source": entry.source,
            "source_file": entry.source_file,
            "created_at": entry.created_at,
            "updated_at": entry.updated_at,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("#{} - {} ({}/{})", entry.id, entry.title, entry.category, entry.source);
        println!("Keywords: {}", kws.join(", "));
        if let Some(ref sf) = entry.source_file {
            println!("Source: {sf}");
        }
        println!("Created: {}", entry.created_at);
        println!("Updated: {}", entry.updated_at);
        println!("\n{}", entry.content);
    }
    Ok(())
}

fn cmd_edit(
    id: i64,
    title: Option<&str>,
    keywords_str: Option<&str>,
    content: Option<&str>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let _entry = db::get_entry(&conn, id)?
        .ok_or_else(|| format!("Entry #{id} not found"))?;

    if title.is_none() && keywords_str.is_none() && content.is_none() {
        return Err("Nothing to update. Specify --title, --keywords, or --content.".into());
    }

    let kws = keywords_str.map(|s| {
        s.split(',').map(|k| k.trim().to_string()).collect::<Vec<_>>()
    });

    db::update_entry(
        &conn,
        id,
        title,
        content,
        kws.as_deref(),
        &now_iso(),
    )?;

    let updated = db::get_entry(&conn, id)?.unwrap();
    let updated_kws = db::get_keywords(&conn, id)?;

    if json_output {
        let out = serde_json::json!({
            "id": updated.id,
            "title": updated.title,
            "content": updated.content,
            "keywords": updated_kws,
            "category": updated.category,
            "source": updated.source,
            "updated_at": updated.updated_at,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Updated entry #{id}: {}", updated.title);
        println!("Keywords: {}", updated_kws.join(", "));
    }
    Ok(())
}

fn cmd_delete(id: i64) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let entry = db::get_entry(&conn, id)?
        .ok_or_else(|| format!("Entry #{id} not found"))?;
    db::delete_entry(&conn, id)?;
    println!("Deleted entry #{id}: {}", entry.title);
    Ok(())
}

fn cmd_purge(category: Option<&str>, source: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    if category.is_none() && source.is_none() {
        return Err("Specify --category or --source (or both)".into());
    }
    let conn = open_db_with_migrate()?;
    let mut total = 0;
    if let Some(src) = source {
        let count = db::purge_by_source(&conn, src)?;
        println!("Purged {count} entries with source \"{src}\"");
        total += count;
    }
    if let Some(cat) = category {
        let count = db::delete_entries_by_category(&conn, cat)?;
        println!("Purged {count} entries with category \"{cat}\"");
        total += count;
    }
    if total == 0 {
        println!("No entries matched.");
    }
    Ok(())
}

fn cmd_list(category: Option<&str>, source: Option<&str>, json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let mut entries = db::list_entries(&conn, category)?;
    if let Some(src) = source {
        entries.retain(|e| e.source == src);
    }

    if json_output {
        let output: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "title": e.title,
                    "category": e.category,
                    "source": e.source,
                    "updated_at": e.updated_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if entries.is_empty() {
        println!("No entries found.");
    } else {
        for e in &entries {
            println!("  [{}] {} ({}/{}) - {}", e.id, e.title, e.category, e.source, e.updated_at);
        }
    }
    Ok(())
}

fn cmd_sync(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let root = get_project_root();
    let stats = sync_knowledge_dir(&conn, &get_knowledge_dir(), &root)?;

    if json_output {
        println!("{}", serde_json::to_string(&serde_json::json!({
            "added": stats.added,
            "updated": stats.updated,
            "removed": stats.removed,
            "unchanged": stats.unchanged,
        }))?);
    } else {
        println!("Sync complete:");
        println!("  Added:     {}", stats.added);
        println!("  Updated:   {}", stats.updated);
        println!("  Removed:   {}", stats.removed);
        println!("  Unchanged: {}", stats.unchanged);
    }
    Ok(())
}

fn cmd_export(dir: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let output_dir = dir.unwrap_or_else(get_knowledge_dir);
    std::fs::create_dir_all(&output_dir)?;
    let root = get_project_root();

    let entries = db::list_entries_by_source(&conn, "local")?;
    if entries.is_empty() {
        println!("No local entries to export.");
        return Ok(());
    }

    // Group by first keyword
    let mut groups: std::collections::HashMap<String, Vec<db::Entry>> =
        std::collections::HashMap::new();
    for entry in entries {
        let kws = db::get_keywords(&conn, entry.id)?;
        let group = kws.first().cloned().unwrap_or_else(|| "general".to_string());
        groups.entry(group).or_default().push(entry);
    }

    let mut total = 0;
    for (group_name, group_entries) in &groups {
        let filename = format!("exported-{group_name}.md");
        let filepath = output_dir.join(&filename);
        let rel_path = filepath
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| filepath.to_string_lossy().to_string());

        let mut all_kws: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in group_entries {
            let kws = db::get_keywords(&conn, entry.id)?;
            all_kws.extend(kws);
        }

        let mut lines = Vec::new();
        lines.push("---".to_string());
        lines.push(format!(
            "keywords: [{}]",
            all_kws.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        lines.push("category: exported".to_string());
        lines.push("---\n".to_string());
        lines.push(format!("# Exported: {group_name}\n"));

        for entry in group_entries {
            let kws = db::get_keywords(&conn, entry.id)?;
            lines.push(format!("## Entry: {}", entry.title));
            lines.push(format!("keywords: [{}]\n", kws.join(", ")));
            lines.push(entry.content.clone());
            lines.push(String::new());
        }

        std::fs::write(&filepath, lines.join("\n"))?;

        let fhash = markdown::file_hash(&filepath)?;
        let now = now_iso();
        for entry in group_entries {
            db::update_entry_to_shared(&conn, entry.id, &rel_path, &fhash, &now)?;
        }

        total += group_entries.len();
        println!(
            "  Exported {} entries to {}",
            group_entries.len(),
            filepath.display()
        );
    }

    println!("\nExported {total} entries total.");
    Ok(())
}

fn cmd_import(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let root = get_project_root();
    let count = import_md_file(&conn, path, &root)?;
    println!("Imported {count} entries from {}", path.display());
    Ok(())
}

fn cmd_keywords(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let rows = db::keyword_counts(&conn)?;

    if json_output {
        let output: Vec<serde_json::Value> = rows
            .iter()
            .map(|(kw, count)| serde_json::json!({"keyword": kw, "count": count}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (kw, count) in &rows {
            println!("  {kw} ({count})");
        }
    }
    Ok(())
}

fn cmd_stats(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let stats = db::get_stats(&conn)?;

    if json_output {
        println!("{}", serde_json::to_string(&serde_json::json!({
            "total_entries": stats.total,
            "shared_entries": stats.shared,
            "local_entries": stats.local,
            "unique_keywords": stats.keywords,
            "db_path": get_db_path().to_string_lossy(),
        }))?);
    } else {
        println!("Knowledge Base Stats:");
        println!("  Total entries:    {}", stats.total);
        println!("  Shared entries:   {}", stats.shared);
        println!("  Local entries:    {}", stats.local);
        println!("  Unique keywords:  {}", stats.keywords);
        println!("  DB path:          {}", get_db_path().display());
    }
    Ok(())
}

fn cmd_uninstall() -> Result<(), Box<dyn std::error::Error>> {
    let root = get_project_root();
    let knowledge_dir = root.join(".knowledge");
    let marker = "## Knowledge Base (local-knowledge-cli)";

    println!("Uninstalling lk from project: {}", root.display());
    println!();

    // 1. Remove .knowledge/ directory
    if knowledge_dir.exists() {
        std::fs::remove_dir_all(&knowledge_dir)?;
        println!("  Removed .knowledge/");
    }

    // 2. Remove section from CLAUDE.md
    let candidates = [
        root.join("CLAUDE.md"),
        root.join("AGENTS.md"),
        root.join(".claude").join("CLAUDE.md"),
    ];
    for claude_md_path in &candidates {
        if !claude_md_path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(claude_md_path)?;
        if let Some(section_start) = content.find(marker) {
            let rest = &content[section_start + marker.len()..];
            let section_end = rest
                .match_indices("\n## ")
                .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                .map(|(i, _)| section_start + marker.len() + i)
                .unwrap_or(content.len());

            // Trim trailing whitespace before the section too
            let before = content[..section_start].trim_end_matches('\n');
            let after = &content[section_end..];
            let new_content = if after.trim().is_empty() {
                if before.is_empty() {
                    String::new()
                } else {
                    format!("{before}\n")
                }
            } else {
                format!("{before}\n{after}")
            };

            if new_content.trim().is_empty() {
                std::fs::remove_file(claude_md_path)?;
                println!("  Removed {} (was empty after section removal)", claude_md_path.display());
            } else {
                std::fs::write(claude_md_path, new_content)?;
                println!("  Removed knowledge section from {}", claude_md_path.display());
            }
        }
    }

    // 3. Remove lk entries from .gitignore
    let gitignore_path = root.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        let lk_entries = [
            ".knowledge/knowledge.db",
            ".knowledge/search.log",
            ".claude/knowledge.db",
            ".claude/search.log",
        ];
        let new_lines: Vec<&str> = content
            .lines()
            .filter(|line| !lk_entries.contains(&line.trim()))
            .collect();
        let new_content = new_lines.join("\n");
        if new_content.trim().is_empty() {
            std::fs::remove_file(&gitignore_path)?;
            println!("  Removed .gitignore (was empty after cleanup)");
        } else {
            let new_content = format!("{}\n", new_content.trim_end());
            std::fs::write(&gitignore_path, new_content)?;
            println!("  Removed lk entries from .gitignore");
        }
    }

    println!("\nProject cleanup complete!");
    println!();
    println!("To fully uninstall lk from your system, also run:");
    println!("  rm -rf ~/.claude/commands/lk-knowledge-*.md");
    println!("  rm -rf ~/.config/lk");
    if is_homebrew_install() {
        println!("  brew uninstall lk");
    } else {
        println!("  rm ~/.local/bin/lk");
    }

    Ok(())
}

/// Detect if the currently running binary was installed via Homebrew.
fn is_homebrew_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| {
            let s = p.to_string_lossy();
            s.contains("/homebrew/") || s.contains("/Cellar/") || s.contains("/linuxbrew/")
        })
        .unwrap_or(false)
}

fn cmd_update() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = home_dir().join(".config").join("lk");
    let config_path = config_dir.join("config.json");

    let repo = if config_path.exists() {
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        config["repo"]
            .as_str()
            .unwrap_or(DEFAULT_REPO)
            .to_string()
    } else {
        DEFAULT_REPO.to_string()
    };

    let homebrew = is_homebrew_install();

    let dest = if homebrew {
        // Homebrew installation: use brew upgrade
        println!("Homebrew installation detected. Running brew upgrade...");
        let status = std::process::Command::new("brew")
            .args(["upgrade", "syarihu/tap/lk"])
            .status()?;
        if !status.success() {
            return Err("brew upgrade failed. Try: brew update && brew upgrade syarihu/tap/lk".into());
        }
        // Use the current exe path (symlink resolves to new version after upgrade)
        std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| PathBuf::from("lk"))
    } else {
        // Manual installation: download from GitHub releases
        let target = detect_target()?;
        let asset_name = format!("lk-{target}.tar.gz");
        let checksum_name = "checksums.txt";

        println!("Checking for updates...");

        let latest_tag = fetch_latest_tag(&repo)?;
        println!("Latest version: {latest_tag}");

        let base_url = format!("https://github.com/{repo}/releases/download/{latest_tag}");

        // Use tempfile crate for secure temporary directory
        let tmpdir = tempfile::tempdir()?;
        let tmppath = tmpdir.path();

        // Download binary archive
        let download_url = format!("{base_url}/{asset_name}");
        println!("Downloading {download_url}...");

        let archive_path = tmppath.join(&asset_name);
        let dl = std::process::Command::new("curl")
            .args(["-fSL", &download_url, "-o"])
            .arg(&archive_path)
            .output()?;

        if !dl.status.success() {
            return Err(format!(
                "Download failed: {}",
                String::from_utf8_lossy(&dl.stderr)
            )
            .into());
        }

        // Download and verify checksum
        let checksum_url = format!("{base_url}/{checksum_name}");
        let checksum_path = tmppath.join(checksum_name);
        let dl_checksum = std::process::Command::new("curl")
            .args(["-fsSL", &checksum_url, "-o"])
            .arg(&checksum_path)
            .output()?;

        if dl_checksum.status.success() {
            verify_checksum(&archive_path, &checksum_path, &asset_name)?;
            println!("Checksum verified.");
        } else {
            eprintln!("Warning: checksums.txt not found in release, skipping verification.");
        }

        // Extract
        let extract = std::process::Command::new("tar")
            .args(["xzf"])
            .arg(&archive_path)
            .arg("-C")
            .arg(tmppath)
            .output()?;

        if !extract.status.success() {
            return Err("Failed to extract archive".into());
        }

        // Install binary
        let bin_dir = home_dir().join(".local").join("bin");
        std::fs::create_dir_all(&bin_dir)?;
        let d = bin_dir.join("lk");
        std::fs::remove_file(&d).ok();
        std::fs::copy(tmppath.join("lk"), &d)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755))?;
        }

        // tmpdir is automatically cleaned up when dropped
        d
    };

    // === Shared post-update logic ===

    // Install embedded Claude commands
    install_embedded_commands()?;

    // Get the version from the newly installed binary
    let new_version = std::process::Command::new(&dest)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().strip_prefix("lk ").map(|v| v.to_string()))
        .unwrap_or_else(|| VERSION.to_string());

    // Update config
    std::fs::create_dir_all(&config_dir)?;
    let config_json = serde_json::json!({
        "install_dir": "",
        "installed_at": now_iso(),
        "version": new_version,
        "repo": repo,
    });
    std::fs::write(&config_path, serde_json::to_string_pretty(&config_json)?)?;

    // Run DB migration if inside a project with a knowledge DB
    let db_path = get_db_path();
    if db_path.exists() {
        let _ = open_db_with_migrate(); // run migration + auto-sync if needed
    }

    println!("\nUpdate complete! (lk {new_version})");
    Ok(())
}

/// Verify SHA256 checksum of downloaded file against checksums.txt
fn verify_checksum(
    file_path: &std::path::Path,
    checksums_path: &std::path::Path,
    expected_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let checksums_content = std::fs::read_to_string(checksums_path)?;
    let expected_hash = checksums_content
        .lines()
        .find_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[1] == expected_filename {
                Some(parts[0].to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| format!("Checksum for {expected_filename} not found in checksums.txt"))?;

    let file_data = std::fs::read(file_path)?;
    let actual_hash = hex::encode(Sha256::digest(&file_data));

    if actual_hash != expected_hash {
        return Err(format!(
            "Checksum mismatch!\n  Expected: {expected_hash}\n  Actual:   {actual_hash}"
        )
        .into());
    }

    Ok(())
}

/// Install Claude commands embedded in the binary.
/// Commands are compiled into the binary so they can't be tampered with via MITM.
fn install_embedded_commands() -> Result<(), Box<dyn std::error::Error>> {
    let commands_dir = home_dir().join(".claude").join("commands");
    std::fs::create_dir_all(&commands_dir)?;

    // Clean up legacy ~ prefixed command files
    for (filename, _) in EMBEDDED_COMMANDS {
        let legacy = format!("~{filename}");
        let legacy_path = commands_dir.join(&legacy);
        if legacy_path.exists() {
            std::fs::remove_file(&legacy_path)?;
            println!("  Removed legacy: {legacy}");
        }
    }

    for (filename, content) in EMBEDDED_COMMANDS {
        std::fs::write(commands_dir.join(filename), content)?;
        println!("  Updated: {filename}");
    }
    Ok(())
}

/// Fetch the latest release tag from GitHub.
/// Tries `gh` CLI first (already authenticated), falls back to curl redirect.
fn fetch_latest_tag(repo: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Try gh CLI first
    if let Ok(output) = std::process::Command::new("gh")
        .args(["release", "view", "--repo", repo, "--json", "tagName", "-q", ".tagName"])
        .output()
    {
        if output.status.success() {
            let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !tag.is_empty() {
                println!("Latest version: {tag}");
                return Ok(tag);
            }
        }
    }

    // Fallback: curl redirect
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL", "-o", "/dev/null",
            "-w", "%{redirect_url}",
            &format!("https://github.com/{repo}/releases/latest"),
        ])
        .output()?;

    let redirect_url = String::from_utf8_lossy(&output.stdout).to_string();
    let tag = redirect_url
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();

    if tag.is_empty() {
        return Err("Could not determine latest version".into());
    }

    println!("Latest version: {tag}");
    Ok(tag)
}

const DEFAULT_REPO: &str = "syarihu/local-knowledge-cli";

const EMBEDDED_COMMANDS: &[(&str, &str)] = &[
    ("lk-knowledge-search.md", include_str!("../commands/lk-knowledge-search.md")),
    ("lk-knowledge-add-db.md", include_str!("../commands/lk-knowledge-add-db.md")),
    ("lk-knowledge-export.md", include_str!("../commands/lk-knowledge-export.md")),
    ("lk-knowledge-sync.md", include_str!("../commands/lk-knowledge-sync.md")),
    ("lk-knowledge-write-md.md", include_str!("../commands/lk-knowledge-write-md.md")),
    ("lk-knowledge-discover.md", include_str!("../commands/lk-knowledge-discover.md")),
    ("lk-knowledge-refresh.md", include_str!("../commands/lk-knowledge-refresh.md")),
    ("lk-knowledge-from-branch.md", include_str!("../commands/lk-knowledge-from-branch.md")),
];

fn detect_target() -> Result<String, Box<dyn std::error::Error>> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu".to_string()),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu".to_string()),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc".to_string()),
        _ => Err(format!("Unsupported platform: {os}-{arch}").into()),
    }
}

// --- Sync helpers ---

struct SyncStats {
    added: usize,
    updated: usize,
    removed: usize,
    unchanged: usize,
}

fn sync_knowledge_dir(
    conn: &rusqlite::Connection,
    knowledge_dir: &std::path::Path,
    root: &std::path::Path,
) -> Result<SyncStats, Box<dyn std::error::Error>> {
    if !knowledge_dir.exists() {
        return Ok(SyncStats { added: 0, updated: 0, removed: 0, unchanged: 0 });
    }

    let mut stats = SyncStats { added: 0, updated: 0, removed: 0, unchanged: 0 };
    let existing = db::get_shared_file_hashes(conn)?;
    let mut found_files = std::collections::HashSet::new();

    for entry in walkdir_md(knowledge_dir) {
        if entry.file_name().and_then(|n| n.to_str()) == Some("README.md") {
            continue;
        }
        let rel_path = entry
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| entry.to_string_lossy().to_string());
        found_files.insert(rel_path.clone());

        let current_hash = markdown::file_hash(&entry)?;

        if let Some(old_hash) = existing.get(&rel_path) {
            if *old_hash != current_hash {
                db::delete_entries_by_source_file(conn, &rel_path)?;
                let count = import_md_file(conn, &entry, root)?;
                stats.updated += count;
            } else {
                stats.unchanged += 1;
            }
        } else {
            let count = import_md_file(conn, &entry, root)?;
            stats.added += count;
        }
    }

    for rel_path in existing.keys() {
        if !found_files.contains(rel_path) {
            db::delete_entries_by_source_file(conn, rel_path)?;
            stats.removed += 1;
        }
    }

    Ok(stats)
}

fn import_md_file(
    conn: &rusqlite::Connection,
    filepath: &std::path::Path,
    root: &std::path::Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let filepath = std::fs::canonicalize(filepath).unwrap_or_else(|_| filepath.to_path_buf());
    let text = std::fs::read_to_string(&filepath)?;
    let fhash = markdown::file_hash(&filepath)?;
    let rel_path = filepath
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| filepath.to_string_lossy().to_string());

    let entries = markdown::parse_md_entries(&text);
    let mut count = 0;
    for entry in entries {
        db::add_entry(
            conn,
            &entry.title,
            &entry.content,
            &entry.keywords,
            &entry.category,
            "shared",
            Some(&rel_path),
            Some(&fhash),
        )?;
        count += 1;
    }
    Ok(count)
}

fn walkdir_md(dir: &std::path::Path) -> Vec<PathBuf> {
    let base = match std::fs::canonicalize(dir) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    walkdir_md_inner(&base, &base, &mut files);
    files.sort();
    files
}

fn walkdir_md_inner(dir: &std::path::Path, base: &std::path::Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();

        // Resolve symlinks and verify the real path is within the base directory
        let real_path = match std::fs::canonicalize(&path) {
            Ok(p) => p,
            Err(_) => continue, // Skip broken symlinks
        };
        if !real_path.starts_with(base) {
            eprintln!(
                "Warning: Skipping {} (resolves outside of {})",
                path.display(),
                base.display()
            );
            continue;
        }

        if real_path.is_dir() {
            walkdir_md_inner(&real_path, base, files);
        } else if real_path.extension().and_then(|e| e.to_str()) == Some("md") {
            files.push(real_path);
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .expect("HOME not set")
}

pub fn now_iso() -> String {
    use time::OffsetDateTime;
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}
