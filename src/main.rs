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
        /// Filter by category
        #[arg(long)]
        category: Option<String>,
        /// Only return entries updated since this date (e.g., 2026-01-01)
        #[arg(long)]
        since: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "5")]
        limit: usize,
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
    /// List all entries
    List {
        /// Filter by category
        #[arg(long)]
        category: Option<String>,
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
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => cmd_init(),
        Commands::Add { title, keywords, content, json } => {
            cmd_add(&title, keywords.as_deref(), content.as_deref(), json)
        }
        Commands::Search { query, keyword_only, category, since, limit, json } => {
            cmd_search(&query, keyword_only, category.as_deref(), since.as_deref(), limit, json)
        }
        Commands::Get { id, json } => cmd_get(id, json),
        Commands::Edit { id, title, keywords, content, json } => {
            cmd_edit(id, title.as_deref(), keywords.as_deref(), content.as_deref(), json)
        }
        Commands::Delete { id } => cmd_delete(id),
        Commands::List { category, json } => cmd_list(category.as_deref(), json),
        Commands::Sync { json } => cmd_sync(json),
        Commands::Export { dir } => cmd_export(dir),
        Commands::Import { path } => cmd_import(&path),
        Commands::Keywords { json } => cmd_keywords(json),
        Commands::Stats { json } => cmd_stats(json),
        Commands::SearchLog { lines } => cmd_search_log(lines),
        Commands::Update => cmd_update(),
        Commands::InstallCommands => install_embedded_commands(),
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
    get_project_root().join(".claude").join("knowledge.db")
}

fn get_knowledge_dir() -> PathBuf {
    get_project_root().join(".knowledge")
}

// --- Commands ---

fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    let root = get_project_root();
    let db_path = root.join(".claude").join("knowledge.db");

    // 1. Initialize DB
    if db_path.exists() {
        println!("DB already exists at {}", db_path.display());
    } else {
        db::init_db(&db_path)?;
        println!("Created DB at {}", db_path.display());
    }

    // 2. Create .knowledge/ directory
    let knowledge_dir = root.join(".knowledge");
    if !knowledge_dir.exists() {
        std::fs::create_dir_all(&knowledge_dir)?;
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
    let conn = db::open_db(&db_path)?;
    let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
    if stats.added > 0 {
        println!("Imported {} entries from .knowledge/", stats.added);
    }

    // 4. Update .gitignore
    let gitignore_path = root.join(".gitignore");
    let gitignore_entries = [".claude/knowledge.db", ".claude/search.log"];
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
        if !content.contains(marker) {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&claude_md_path)?;
            if !content.ends_with('\n') {
                writeln!(f)?;
            }
            write!(f, "{CLAUDE_MD_SECTION}")?;
            println!("Added knowledge base instructions to {}", claude_md_path.display());
        } else {
            println!("CLAUDE.md already contains knowledge base instructions");
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
\n\
### Pre-investigation Rule\n\
- Before reading code with Read, Grep, or Glob tools, first run `lk search \"<keyword>\" --json --limit 5` to check existing knowledge\n\
- If results are found, use `lk get <id> --json` for details — skip unnecessary code exploration\n\
\n\
### Auto-accumulation of Knowledge\n\
- After investigating code or design, save noteworthy discoveries with `lk add \"<title>\" --keywords \"kw1,kw2\" --content \"...\"`\n\
- Do not save trivial or obvious facts\n\
- Briefly report what was saved (e.g., \"Added to knowledge base: <title>\")\n\
\n\
### Available Commands\n\
- `lk search \"<query>\" --json` - Search knowledge (use `--since YYYY-MM-DD` to filter by date)\n\
- `lk get <id> --json` - Get entry details\n\
- `lk add \"<title>\" --keywords \"kw1,kw2\" --content \"...\"` - Add knowledge\n\
- `lk edit <id> --title \"...\" --keywords \"...\" --content \"...\"` - Edit existing entry\n\
- `lk sync` - Sync markdown files with DB\n\
- `/lk-knowledge-search` `/lk-knowledge-add` `/lk-knowledge-export` `/lk-knowledge-sync` `/lk-knowledge-write` `/lk-knowledge-discover` `/lk-knowledge-refresh` - Claude skills\n";

fn cmd_add(
    title: &str,
    keywords_str: Option<&str>,
    content: Option<&str>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = db::open_db(&get_db_path())?;
    let content = content.unwrap_or("");

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

    let entry_id = db::add_entry(&conn, title, content, &kws, "local", None, None)?;

    if json_output {
        let out = serde_json::json!({
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
    since: Option<&str>,
    limit: usize,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = db::open_db(&get_db_path())?;
    let results = db::search_entries(&conn, query, keyword_only, category, since, limit)?;

    log_search(query, &results);

    if json_output {
        let output: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let snippet = truncate_str(&r.content, 100);
                let kws = db::get_keywords(&conn, r.id).unwrap_or_default();
                serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "keywords": kws,
                    "snippet": snippet,
                    "category": r.category,
                })
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
        let log_path = get_project_root().join(".claude").join("search.log");
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
    let log_path = get_project_root().join(".claude").join("search.log");
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
    let conn = db::open_db(&get_db_path())?;
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
            "source_file": entry.source_file,
            "created_at": entry.created_at,
            "updated_at": entry.updated_at,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("#{} - {} ({})", entry.id, entry.title, entry.category);
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
    let conn = db::open_db(&get_db_path())?;
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
    let conn = db::open_db(&get_db_path())?;
    let entry = db::get_entry(&conn, id)?
        .ok_or_else(|| format!("Entry #{id} not found"))?;
    db::delete_entry(&conn, id)?;
    println!("Deleted entry #{id}: {}", entry.title);
    Ok(())
}

fn cmd_list(category: Option<&str>, json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = db::open_db(&get_db_path())?;
    let entries = db::list_entries(&conn, category)?;

    if json_output {
        let output: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "title": e.title,
                    "category": e.category,
                    "updated_at": e.updated_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if entries.is_empty() {
        println!("No entries found.");
    } else {
        for e in &entries {
            println!("  [{}] {} ({}) - {}", e.id, e.title, e.category, e.updated_at);
        }
    }
    Ok(())
}

fn cmd_sync(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = db::open_db(&get_db_path())?;
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
    let conn = db::open_db(&get_db_path())?;
    let output_dir = dir.unwrap_or_else(get_knowledge_dir);
    std::fs::create_dir_all(&output_dir)?;
    let root = get_project_root();

    let entries = db::list_local_entries_with_content(&conn)?;
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
    let conn = db::open_db(&get_db_path())?;
    let root = get_project_root();
    let count = import_md_file(&conn, path, &root)?;
    println!("Imported {count} entries from {}", path.display());
    Ok(())
}

fn cmd_keywords(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = db::open_db(&get_db_path())?;
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
    let conn = db::open_db(&get_db_path())?;
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
    let dest = bin_dir.join("lk");
    std::fs::remove_file(&dest).ok();
    std::fs::copy(tmppath.join("lk"), &dest)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    }

    // Install embedded Claude commands
    install_embedded_commands()?;

    // Update config
    let new_version = VERSION;
    std::fs::create_dir_all(&config_dir)?;
    let config_json = serde_json::json!({
        "install_dir": "",
        "installed_at": now_iso(),
        "version": new_version,
        "repo": repo,
    });
    std::fs::write(&config_path, serde_json::to_string_pretty(&config_json)?)?;

    // tmpdir is automatically cleaned up when dropped
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
    ("~lk-knowledge-search.md", include_str!("../commands/~lk-knowledge-search.md")),
    ("~lk-knowledge-add.md", include_str!("../commands/~lk-knowledge-add.md")),
    ("~lk-knowledge-export.md", include_str!("../commands/~lk-knowledge-export.md")),
    ("~lk-knowledge-sync.md", include_str!("../commands/~lk-knowledge-sync.md")),
    ("~lk-knowledge-write.md", include_str!("../commands/~lk-knowledge-write.md")),
    ("~lk-knowledge-discover.md", include_str!("../commands/~lk-knowledge-discover.md")),
    ("~lk-knowledge-refresh.md", include_str!("../commands/~lk-knowledge-refresh.md")),
];

fn detect_target() -> Result<String, Box<dyn std::error::Error>> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu".to_string()),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu".to_string()),
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
                db::delete_entries_by_source(conn, &rel_path)?;
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
            db::delete_entries_by_source(conn, rel_path)?;
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
