mod cmd;
mod config;
mod db;
mod keywords;
mod markdown;
mod secrets;
mod util;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use util::VERSION;

#[derive(Parser)]
#[command(name = "lk", version = VERSION, about = "Local knowledge base CLI for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::enum_variant_names)]
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
        /// Allow content that contains potential secrets
        #[arg(long)]
        allow_secrets: bool,
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
        /// Only return entries updated since this date (ISO 8601, e.g., 2026-01-01 or 2026-01-01T09:00:00)
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
    /// Edit an existing entry
    Edit {
        /// Entry ID
        id: i64,
        /// New title
        #[arg(short, long)]
        title: Option<String>,
        /// New comma-separated keywords
        #[arg(short, long)]
        keywords: Option<String>,
        /// New content
        #[arg(short, long)]
        content: Option<String>,
        /// Set status ("active" or "deprecated")
        #[arg(long)]
        status: Option<String>,
        /// Set superseded-by entry ID (use 0 to clear)
        #[arg(long)]
        superseded_by: Option<i64>,
        /// Reset updated_at timestamp to now (mark as freshly reviewed)
        #[arg(long)]
        touch: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete an entry
    Delete {
        /// Entry ID
        id: i64,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Delete all entries in a category or by source
    Purge {
        /// Category to purge (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Source to purge (e.g., "local", "shared")
        #[arg(long)]
        source: Option<String>,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// List all entries
    List {
        /// Filter by category (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Filter by source ("local" or "shared")
        #[arg(long)]
        source: Option<String>,
        /// Max results (default: unlimited)
        #[arg(short, long)]
        limit: Option<usize>,
        /// Skip first N results
        #[arg(long, default_value = "0")]
        offset: usize,
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
        /// Output directory (default: .knowledge/)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Export only specific entry IDs (comma-separated, e.g., "1,2,3")
        #[arg(long)]
        ids: Option<String>,
        /// Export only entries matching a search query
        #[arg(long)]
        query: Option<String>,
        /// Allow content that contains potential secrets
        #[arg(long)]
        allow_secrets: bool,
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
        /// Show verbose details (project root, schema version)
        #[arg(long, short)]
        verbose: bool,
    },
    /// Show recent command log entries
    #[command(alias = "search-log")]
    CommandLog {
        /// Number of log lines to show
        #[arg(short, long, default_value = "20")]
        lines: usize,
    },
    /// Update lk to the latest version
    Update {
        /// Skip checksum verification (not recommended)
        #[arg(long)]
        skip_verify: bool,
    },
    /// Install Claude Code slash commands
    InstallCommands,
    /// Uninstall lk from current project (removes .knowledge/, CLAUDE.md section, .gitignore entries)
    Uninstall {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

impl Commands {
    fn is_json_mode(&self) -> bool {
        match self {
            Commands::Add { json, .. }
            | Commands::Search { json, .. }
            | Commands::Get { json, .. }
            | Commands::Edit { json, .. }
            | Commands::List { json, .. }
            | Commands::Sync { json, .. }
            | Commands::Keywords { json, .. }
            | Commands::Stats { json, .. } => *json,
            _ => false,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let json_mode = cli.command.is_json_mode();

    // Auto-sync before read commands (if enabled)
    let needs_auto_sync = matches!(
        cli.command,
        Commands::Search { .. }
            | Commands::Get { .. }
            | Commands::List { .. }
            | Commands::Keywords { .. }
            | Commands::Stats { .. }
            | Commands::Add { .. }
            | Commands::Edit { .. }
            | Commands::Delete { .. }
            | Commands::Purge { .. }
            | Commands::Export { .. }
    );

    if needs_auto_sync {
        cmd::maybe_auto_sync();
    }

    let result = match cli.command {
        Commands::Init => cmd::cmd_init(),
        Commands::Add {
            title,
            keywords,
            content,
            category,
            force,
            allow_secrets,
            json,
        } => cmd::cmd_add(
            &title,
            keywords.as_deref(),
            content.as_deref(),
            category.as_deref(),
            force,
            allow_secrets,
            json,
        ),
        Commands::Search {
            query,
            keyword_only,
            category,
            source,
            since,
            limit,
            full,
            json,
        } => cmd::cmd_search(
            &query,
            keyword_only,
            category.as_deref(),
            source.as_deref(),
            since.as_deref(),
            limit,
            full,
            json,
        ),
        Commands::Get { id, json } => cmd::cmd_get(id, json),
        Commands::Edit {
            id,
            title,
            keywords,
            content,
            status,
            superseded_by,
            touch,
            json,
        } => cmd::cmd_edit(
            id,
            title.as_deref(),
            keywords.as_deref(),
            content.as_deref(),
            status.as_deref(),
            superseded_by,
            touch,
            json,
        ),
        Commands::Delete { id, yes } => cmd::cmd_delete(id, yes),
        Commands::Purge {
            category,
            source,
            yes,
        } => cmd::cmd_purge(category.as_deref(), source.as_deref(), yes),
        Commands::List {
            category,
            source,
            limit,
            offset,
            json,
        } => cmd::cmd_list(category.as_deref(), source.as_deref(), limit, offset, json),
        Commands::Sync { json } => cmd::cmd_sync(json),
        Commands::Export {
            dir,
            ids,
            query,
            allow_secrets,
        } => cmd::cmd_export(dir, ids.as_deref(), query.as_deref(), allow_secrets),
        Commands::Import { path } => cmd::cmd_import(&path),
        Commands::Keywords { json } => cmd::cmd_keywords(json),
        Commands::Stats { json, verbose } => cmd::cmd_stats(json, verbose),
        Commands::CommandLog { lines } => cmd::cmd_command_log(lines),
        Commands::Update { skip_verify } => cmd::cmd_update(skip_verify),
        Commands::InstallCommands => cmd::install_embedded_commands(),
        Commands::Uninstall { yes } => cmd::cmd_uninstall(yes),
    };

    if let Err(e) = result {
        if json_mode {
            let err = serde_json::json!({ "error": e.to_string() });
            eprintln!("{}", serde_json::to_string(&err).unwrap_or_default());
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}
