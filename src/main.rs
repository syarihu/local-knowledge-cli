mod cmd;
mod config;
mod db;
mod keywords;
mod markdown;
mod mcp;
mod secrets;
mod similarity;
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
    /// Initialize knowledge base for current project (or globally with --global)
    Init {
        /// Install lk-instructions globally to ~/.claude/
        #[arg(long)]
        global: bool,
    },
    /// Add a knowledge entry
    Add {
        /// Entry title
        title: String,
        /// Comma-separated keywords (authoritative when given; auto-extracted from title/content otherwise)
        #[arg(short, long)]
        keywords: Option<String>,
        /// Entry content
        #[arg(short, long)]
        content: Option<String>,
        /// Category (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Initial status ("active", "deprecated", "proposed", "accepted", or "superseded"). Default: "active"
        #[arg(long)]
        status: Option<String>,
        /// Skip duplicate check and force add
        #[arg(long)]
        force: bool,
        /// Allow content that contains potential secrets
        #[arg(long)]
        allow_secrets: bool,
        /// Knowledge scope: "auto" (default — project if initialized, else user), "project", or "user" (global ~/.config/lk/knowledge.db)
        #[arg(long, default_value = "auto")]
        scope: String,
        /// Project this entry belongs to, as "owner/repo" (default: detected from the git remote). A bare repo name is expanded to the full slug when it names the current repo
        #[arg(long)]
        project: Option<String>,
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
        /// Filter by status (e.g., "accepted", "proposed", "superseded")
        #[arg(long)]
        status: Option<String>,
        /// Only return entries updated since this date (ISO 8601, e.g., 2026-01-01 or 2026-01-01T09:00:00)
        #[arg(long)]
        since: Option<String>,
        /// Filter by the project an entry was recorded against: "owner/repo", a bare repo name (matches any owner), or "." for the current project
        #[arg(long)]
        project: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "5")]
        limit: usize,
        /// Include full content in JSON output (eliminates need for lk get)
        #[arg(long)]
        full: bool,
        /// Scope to search: "project", "user", or "all" (default)
        #[arg(long, default_value = "all")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a single entry by ID or UID
    Get {
        /// Entry id (project) or uid (id-or-uid; uid resolves across scopes)
        id: String,
        /// Scope: "project" or "user" (omit to auto-resolve: numeric=project, uid=project then user)
        #[arg(long)]
        scope: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit an existing entry
    Edit {
        /// Entry id (project) or uid (id-or-uid; uid resolves across scopes)
        id: String,
        /// New title
        #[arg(short, long)]
        title: Option<String>,
        /// New comma-separated keywords
        #[arg(short, long)]
        keywords: Option<String>,
        /// New content
        #[arg(short, long)]
        content: Option<String>,
        /// Set status ("active", "deprecated", "proposed", "accepted", or "superseded")
        #[arg(long)]
        status: Option<String>,
        /// Set superseded-by id-or-uid in the same scope (use 0 to clear)
        #[arg(long)]
        superseded_by: Option<String>,
        /// Set the project this entry is attributed to ("owner/repo"; pass "" to clear)
        #[arg(long)]
        project: Option<String>,
        /// Reset updated_at timestamp to now (mark as freshly reviewed)
        #[arg(long)]
        touch: bool,
        /// Scope: "project" or "user" (omit to auto-resolve: numeric=project, uid=project then user)
        #[arg(long)]
        scope: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete an entry
    Delete {
        /// Entry id (project) or uid (id-or-uid; uid resolves across scopes)
        id: String,
        /// Scope: "project" or "user" (omit to auto-resolve: numeric=project, uid=project then user)
        #[arg(long)]
        scope: Option<String>,
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
    /// Mark an entry as superseded by another entry (bidirectional)
    Supersede {
        /// id-or-uid of the old entry being superseded
        old_id: String,
        /// id-or-uid of the new entry that supersedes it
        new_id: String,
        /// Scope: "project" or "user" (both entries must be in the same scope)
        #[arg(long)]
        scope: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List all entries
    List {
        /// Filter by category (e.g., "features", "architecture")
        #[arg(long)]
        category: Option<String>,
        /// Filter by source ("local" or "shared")
        #[arg(long)]
        source: Option<String>,
        /// Filter by status (e.g., "accepted", "proposed", "superseded")
        #[arg(long)]
        status: Option<String>,
        /// Filter by the project an entry was recorded against: "owner/repo", a bare repo name (matches any owner), or "." for the current project
        #[arg(long)]
        project: Option<String>,
        /// Max results (default: unlimited)
        #[arg(short, long)]
        limit: Option<usize>,
        /// Skip first N results
        #[arg(long, default_value = "0")]
        offset: usize,
        /// Scope to list: "project", "user", or "all" (default)
        #[arg(long, default_value = "all")]
        scope: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Sync markdown files with DB (project .knowledge/, or user-scope dir with --scope user)
    Sync {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Write UIDs back to markdown files that don't have them
        #[arg(long)]
        write_uids: bool,
        /// Scope to sync: "project" (default) or "user" (~/.config/lk/knowledge or configured dir)
        #[arg(long, default_value = "project")]
        scope: String,
    },
    /// Export local entries to markdown (project .knowledge/, or user-scope dir with --scope user)
    Export {
        /// Output directory (default: project .knowledge/, or user-scope dir for --scope user)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Export only specific entry IDs (comma-separated, e.g., "1,2,3")
        #[arg(long)]
        ids: Option<String>,
        /// Export only entries matching a search query
        #[arg(long)]
        query: Option<String>,
        /// Write everything to this one file instead of one file per first keyword
        /// (e.g. "release" or "release.md"). Renaming an exported file afterwards is
        /// what this replaces.
        #[arg(long)]
        file: Option<String>,
        /// Allow content that contains potential secrets
        #[arg(long)]
        allow_secrets: bool,
        /// Scope to export: "project" (default) or "user" (global ~/.config/lk store)
        #[arg(long, default_value = "project")]
        scope: String,
    },
    /// Import a markdown file
    Import {
        /// Path to markdown file
        path: PathBuf,
    },
    /// List all unique keywords, or regenerate noisy per-entry keywords with --regen
    Keywords {
        /// Regenerate auto keywords for noisy local entries (keyword count > threshold)
        #[arg(long)]
        regen: bool,
        /// With --regen: regenerate every local entry, not just noisy ones
        #[arg(long, requires = "regen")]
        all: bool,
        /// With --regen: entries with more keywords than this are considered noisy
        #[arg(long, default_value = "15", requires = "regen")]
        threshold: usize,
        /// With --regen: show what would change without writing
        #[arg(long, requires = "regen")]
        dry_run: bool,
        /// With --regen: scope to regenerate ("project", "user", or "all")
        #[arg(long, default_value = "project", requires = "regen")]
        scope: String,
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
        /// Break entry counts down by the project each was recorded against
        #[arg(long)]
        by_project: bool,
        /// Scope: "project", "user", or "all" (default)
        #[arg(long, default_value = "all")]
        scope: String,
    },
    /// Show recent command log entries
    #[command(alias = "search-log")]
    CommandLog {
        /// Number of log lines to show
        #[arg(short, long, default_value = "20")]
        lines: usize,
    },
    /// Update lk to the latest version (to edit an entry, use `lk edit`)
    #[command(long_about = "Update the lk binary itself to the latest release.\n\n\
                      This does NOT edit a knowledge entry. The CLI equivalent of the \
                      `edit_knowledge` MCP tool is `lk edit <id-or-uid>`.")]
    Update {
        /// Skip checksum verification (not recommended)
        #[arg(long)]
        skip_verify: bool,
        /// Swallows `lk update <id> ...` so the error can name `lk edit` instead of
        /// clap's bare "unexpected argument", which sends the caller off to --help.
        #[arg(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
        entry_args: Vec<String>,
    },
    /// Install Claude Code slash commands and refresh existing lk-instructions.md (project + global)
    InstallCommands,
    /// Uninstall lk from current project (removes .knowledge/, CLAUDE.md section, .gitignore entries)
    Uninstall {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Start MCP (Model Context Protocol) server over stdio
    Mcp {
        /// Project directories to serve (can be specified multiple times)
        #[arg(long)]
        project: Vec<PathBuf>,
    },
    /// Install lk as an MCP server for Claude Code and/or Claude Desktop
    InstallMcp {
        /// Target: "claude-code", "claude-desktop", or "all"
        #[arg(long, default_value = "all")]
        target: String,
        /// Project directories to register (can be specified multiple times; merged with existing)
        #[arg(long)]
        project: Vec<PathBuf>,
        /// Project directories to remove from registration (can be specified multiple times)
        #[arg(long)]
        remove_project: Vec<PathBuf>,
    },
    /// Uninstall lk MCP server from Claude Code and/or Claude Desktop
    UninstallMcp {
        /// Target: "claude-code", "claude-desktop", or "all"
        #[arg(long, default_value = "all")]
        target: String,
    },
}

impl Commands {
    fn is_json_mode(&self) -> bool {
        match self {
            Commands::Add { json, .. }
            | Commands::Search { json, .. }
            | Commands::Get { json, .. }
            | Commands::Edit { json, .. }
            | Commands::Supersede { json, .. }
            | Commands::List { json, .. }
            | Commands::Sync { json, .. }
            | Commands::Keywords { json, .. }
            | Commands::Stats { json, .. } => *json,
            _ => false,
        }
    }

    /// The `--status` value for commands that accept one (add/edit set it, search/list
    /// filter by it). Used to preflight-validate before any DB work / auto-sync.
    fn status_arg(&self) -> Option<&str> {
        match self {
            Commands::Add { status, .. }
            | Commands::Search { status, .. }
            | Commands::List { status, .. }
            | Commands::Edit { status, .. } => status.as_deref(),
            _ => None,
        }
    }
}

/// Turn a mistaken `lk update <id> …` into a pointer at `lk edit`.
///
/// The MCP tool that edits an entry is called `edit_knowledge`, so an agent
/// that has learned that vocabulary and then falls back to the CLI reaches for
/// `lk update` — which upgrades the binary instead. Clap answers that with
/// "unexpected argument '<id>'" and a pointer to `--help`, and finding `lk edit`
/// from there costs two more invocations (`lk update --help`, then `lk --help`).
/// Naming the right command immediately, with the caller's own arguments already
/// in place, ends it in one.
///
/// Flags `lk edit` accepts. A leading flag from this set means the caller wanted
/// to edit an entry; anything else is a bad option for the updater itself and has
/// to be reported as one, not answered with advice about a different command.
///
/// Used only to read that leading argument's intent. Once an id is present the
/// intent is settled, and the remaining arguments are echoed through untouched
/// even if one of them is wrong — validating them here would mean duplicating
/// `lk edit`'s option set, which then silently rejects any flag added to `edit`
/// later. A bad flag in the suggestion surfaces from `lk edit` itself, which is
/// where the answer belongs.
const EDIT_FLAGS: &[&str] = &[
    "-t",
    "--title",
    "-k",
    "--keywords",
    "-c",
    "--content",
    "--status",
    "--superseded-by",
    "--touch",
    "--scope",
    "--json",
];

/// Whether an argument names a flag `lk edit` takes.
///
/// Matching the whole argument is not enough: clap also accepts a long flag with
/// its value attached (`--content=x`) and a short one with the value run together
/// (`-cx`), and both of those are things a caller reaching for `lk edit` may well
/// type. Missing them would answer a genuine edit attempt with "bad option".
fn names_an_edit_flag(arg: &str) -> bool {
    if EDIT_FLAGS.contains(&arg.split('=').next().unwrap_or(arg)) {
        return true;
    }
    // Short flags only — a long typo like `--skip-verfiy` must not match on its
    // first two characters. Sliced by chars so non-ASCII input cannot panic.
    if !arg.starts_with("--") {
        let short: String = arg.chars().take(2).collect();
        return arg.chars().count() > 2 && EDIT_FLAGS.contains(&short.as_str());
    }
    false
}

/// Returns `None` when nothing was passed, which is the legitimate self-update.
fn misused_as_edit(entry_args: &[String]) -> Option<String> {
    let (first, rest) = entry_args.split_first()?;
    let preamble = "`lk update` upgrades the lk binary itself — it does not edit an entry.\n\
                    `lk edit` is the CLI equivalent of the `edit_knowledge` MCP tool.";

    if first.starts_with('-') {
        // A flag `lk edit` does not take is a mistyped or unknown option for the
        // updater. Answer it the way clap would, rather than sending the caller
        // off to an unrelated command.
        if !names_an_edit_flag(first) {
            return Some(format!(
                "unexpected argument '{first}' found\n\nUsage: lk update [--skip-verify]\n\nFor more information, try 'lk update --help'."
            ));
        }
        // An edit flag but no id, so there is no entry to name.
        return Some(format!(
            "{preamble}\n\nTo edit an entry:\n    lk edit <id-or-uid> [-t \"<title>\"] [-k \"kw1,kw2\"] [-c \"<body>\"] [--status S]"
        ));
    }

    // A value beginning with `-` cannot be passed as a separate word: the shell
    // strips the quotes, and clap then reads it as a flag (`--content -x` fails
    // with "unexpected argument '-x'"). Quoting cannot fix that — attaching the
    // value to its flag with `=` can. A following argument counts as a value when
    // it is not itself a flag `lk edit` takes.
    let mut args = String::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = &rest[i];
        let next_is_dashed_value = rest
            .get(i + 1)
            .is_some_and(|n| n.starts_with('-') && !names_an_edit_flag(n));
        if arg.starts_with('-') && next_is_dashed_value {
            args.push(' ');
            args.push_str(&util::shell_quote(&format!("{arg}={}", rest[i + 1])));
            i += 2;
        } else {
            args.push(' ');
            args.push_str(&util::shell_quote(arg));
            i += 1;
        }
    }

    Some(format!(
        "{preamble}\n\nTo edit entry {}:\n    lk edit {}{args}",
        first,
        util::shell_quote(first)
    ))
}

fn main() {
    let cli = Cli::parse();
    let json_mode = cli.command.is_json_mode();

    // Preflight: reject an invalid --status before any DB work (auto-sync opens and
    // syncs the project DB below). The per-command handlers re-validate as a guard,
    // but this keeps "bad input → no side effects" true at the CLI entry point.
    if let Some(st) = cli.command.status_arg()
        && !db::is_valid_status(st)
    {
        let msg = format!(
            "Invalid status: {st}. Must be one of: {}",
            db::VALID_STATUSES.join(", ")
        );
        if json_mode {
            let err = serde_json::json!({ "error": msg });
            eprintln!("{}", serde_json::to_string(&err).unwrap_or_default());
        } else {
            eprintln!("Error: {msg}");
        }
        std::process::exit(1);
    }

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
            | Commands::Supersede { .. }
            | Commands::Delete { .. }
            | Commands::Purge { .. }
            | Commands::Export { .. }
    );

    // Skip project auto-sync for user-scope-only operations (they never touch the
    // project DB), mirroring the MCP server's lazy per-scope open.
    let user_scope_only = match &cli.command {
        Commands::Add { scope, .. }
        | Commands::Search { scope, .. }
        | Commands::List { scope, .. }
        | Commands::Stats { scope, .. }
        | Commands::Export { scope, .. }
        | Commands::Keywords { scope, .. } => scope == "user",
        Commands::Get { scope, .. }
        | Commands::Edit { scope, .. }
        | Commands::Delete { scope, .. }
        | Commands::Supersede { scope, .. } => scope.as_deref() == Some("user"),
        _ => false,
    };

    // Auto-sync is a project-DB operation: skip it for user-scope-only commands and
    // for uninitialized projects (where reads/writes fall back to user scope).
    if needs_auto_sync && !user_scope_only && util::project_db_exists() {
        cmd::maybe_auto_sync();
    }

    let result = match cli.command {
        Commands::Init { global } => cmd::cmd_init(global),
        Commands::Add {
            title,
            keywords,
            content,
            category,
            status,
            force,
            allow_secrets,
            scope,
            project,
            json,
        } => cmd::cmd_add(
            &title,
            keywords.as_deref(),
            content.as_deref(),
            category.as_deref(),
            status.as_deref(),
            force,
            allow_secrets,
            &scope,
            project.as_deref(),
            json,
        ),
        Commands::Search {
            query,
            keyword_only,
            category,
            source,
            status,
            since,
            project,
            limit,
            full,
            scope,
            json,
        } => cmd::cmd_search(
            &query,
            keyword_only,
            category.as_deref(),
            source.as_deref(),
            status.as_deref(),
            since.as_deref(),
            project.as_deref(),
            limit,
            full,
            Some(&scope),
            json,
        ),
        Commands::Get { id, scope, json } => {
            cmd::parse_scope_opt(scope.as_deref()).and_then(|sc| cmd::cmd_get(&id, sc, json))
        }
        Commands::Edit {
            id,
            title,
            keywords,
            content,
            status,
            superseded_by,
            project,
            touch,
            scope,
            json,
        } => cmd::parse_scope_opt(scope.as_deref()).and_then(|sc| {
            cmd::cmd_edit(
                &id,
                title.as_deref(),
                keywords.as_deref(),
                content.as_deref(),
                status.as_deref(),
                superseded_by.as_deref(),
                project.as_deref(),
                touch,
                sc,
                json,
            )
        }),
        Commands::Supersede {
            old_id,
            new_id,
            scope,
            json,
        } => cmd::parse_scope_opt(scope.as_deref())
            .and_then(|sc| cmd::cmd_supersede(&old_id, &new_id, sc, json)),
        Commands::Delete { id, scope, yes } => {
            cmd::parse_scope_opt(scope.as_deref()).and_then(|sc| cmd::cmd_delete(&id, sc, yes))
        }
        Commands::Purge {
            category,
            source,
            yes,
        } => cmd::cmd_purge(category.as_deref(), source.as_deref(), yes),
        Commands::List {
            category,
            source,
            status,
            project,
            limit,
            offset,
            scope,
            json,
        } => cmd::cmd_list(
            category.as_deref(),
            source.as_deref(),
            status.as_deref(),
            project.as_deref(),
            limit,
            offset,
            Some(&scope),
            json,
        ),
        Commands::Sync {
            json,
            write_uids,
            scope,
        } => cmd::cmd_sync(json, write_uids, &scope),
        Commands::Export {
            dir,
            ids,
            query,
            file,
            allow_secrets,
            scope,
        } => cmd::cmd_export(
            dir,
            ids.as_deref(),
            query.as_deref(),
            file.as_deref(),
            allow_secrets,
            &scope,
        ),
        Commands::Import { path } => cmd::cmd_import(&path),
        Commands::Keywords {
            regen,
            all,
            threshold,
            dry_run,
            scope,
            json,
        } => {
            if regen {
                cmd::cmd_keywords_regen(all, threshold, dry_run, Some(&scope), json)
            } else {
                cmd::cmd_keywords(json)
            }
        }
        Commands::Stats {
            json,
            verbose,
            by_project,
            scope,
        } => cmd::cmd_stats(json, verbose, by_project, Some(&scope)),
        Commands::CommandLog { lines } => cmd::cmd_command_log(lines),
        Commands::Update {
            skip_verify,
            entry_args,
        } => match misused_as_edit(&entry_args) {
            Some(hint) => Err(hint.into()),
            None => cmd::cmd_update(skip_verify),
        },
        Commands::InstallCommands => cmd::cmd_install_commands(),
        Commands::Uninstall { yes } => cmd::cmd_uninstall(yes),
        Commands::Mcp { project } => mcp::run_server(project),
        Commands::InstallMcp {
            target,
            project,
            remove_project,
        } => cmd::cmd_install_mcp(&target, &project, &remove_project),
        Commands::UninstallMcp { target } => cmd::cmd_uninstall_mcp(&target),
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

#[cfg(test)]
mod tests {
    use super::misused_as_edit;

    #[test]
    fn bare_update_is_the_real_self_update() {
        assert!(
            misused_as_edit(&[]).is_none(),
            "no arguments means the caller wants to upgrade the binary"
        );
    }

    fn hint_for(args: &[&str]) -> String {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        misused_as_edit(&owned).expect("arguments were passed, so something must be said")
    }

    #[test]
    fn update_with_an_id_suggests_edit_with_the_same_arguments() {
        let hint = hint_for(&["42", "--content", "new body", "--status", "accepted"]);
        assert!(
            hint.contains(r#"lk edit 42 --content 'new body' --status accepted"#),
            "the suggestion must be pasteable, with spaces quoted: {hint}"
        );
    }

    /// The suggestion is advertised as pasteable, so it must survive a shell.
    /// Entry bodies routinely contain `$` and a title can contain a backtick;
    /// echoing those back inside double quotes would hand over a command that
    /// substitutes or executes when pasted.
    #[test]
    fn suggestion_is_safe_to_paste_into_a_shell() {
        let hint = hint_for(&["42", "--content", "costs $HOME and `date`"]);
        assert!(
            hint.contains(r#"--content 'costs $HOME and `date`'"#),
            "expansion-triggering characters must be single-quoted: {hint}"
        );
        assert!(
            !hint.contains(r#""costs $HOME"#),
            "double quotes would still expand $HOME: {hint}"
        );

        // An embedded single quote is escaped per platform; either way the value
        // stays inside single quotes, so nothing expands.
        let hint = hint_for(&["42", "--title", "it's here"]);
        assert!(
            hint.contains("--title 'it"),
            "the value must stay single-quoted: {hint}"
        );
        #[cfg(not(windows))]
        assert!(
            hint.contains(r"--title 'it'\''s here'"),
            "POSIX closes, escapes and reopens: {hint}"
        );
        #[cfg(windows)]
        assert!(
            hint.contains("--title 'it''s here'"),
            "PowerShell doubles the quote: {hint}"
        );
    }

    #[test]
    fn update_with_an_edit_flag_but_no_id_falls_back_to_the_generic_form() {
        let hint = hint_for(&["--content", "x"]);
        assert!(
            hint.contains("lk edit <id-or-uid>"),
            "with no id to echo back, name the shape instead: {hint}"
        );
        assert!(
            !hint.contains("edit --content"),
            "must not present the flag as if it were an id: {hint}"
        );
    }

    /// A flag `lk edit` does not take means the caller wanted the updater and
    /// mistyped. Advice about a different command would be actively misleading.
    #[test]
    fn update_with_an_unknown_flag_is_reported_as_a_bad_option() {
        let hint = hint_for(&["--skip-verfiy"]);
        assert!(
            hint.contains("unexpected argument '--skip-verfiy'"),
            "a typo'd updater option must be reported as one: {hint}"
        );
        assert!(
            !hint.contains("lk edit"),
            "must not send a would-be self-updater to the edit command: {hint}"
        );
    }

    /// Clap accepts a value attached to the flag, and someone reaching for
    /// `lk edit` may well write it that way. Recognising only the bare spelling
    /// would answer a genuine edit attempt with "bad option".
    #[test]
    fn edit_flags_are_recognized_with_attached_values() {
        for arg in ["--content=x", "--status=accepted", "-cx", "-tSome title"] {
            let hint = hint_for(&[arg]);
            assert!(
                hint.contains("lk edit"),
                "{arg:?} is an edit flag and must reach the edit hint: {hint}"
            );
        }
        // A long typo must not match on its first two characters.
        for arg in ["--skip-verfiy", "--ttl=3", "--cache"] {
            let hint = hint_for(&[arg]);
            assert!(
                hint.contains("unexpected argument"),
                "{arg:?} is not an edit flag: {hint}"
            );
        }
    }

    /// Sliced by chars, so a non-ASCII argument cannot panic on a byte boundary.
    #[test]
    fn non_ascii_flag_like_argument_is_handled() {
        let hint = hint_for(&["-あい"]);
        assert!(hint.contains("unexpected argument"), "got {hint}");
    }

    /// A value starting with `-` survives quoting only to be read as a flag by
    /// clap, so it has to be attached to its own flag with `=`.
    #[test]
    fn a_value_starting_with_a_dash_is_attached_to_its_flag() {
        let hint = hint_for(&["42", "--content", "-leading-dash"]);
        assert!(
            hint.contains("lk edit 42 --content=-leading-dash"),
            "a dashed value must be attached with `=`: {hint}"
        );
    }

    /// The `=` joining must not swallow a genuine following flag.
    #[test]
    fn flags_that_take_no_value_are_left_separate() {
        let hint = hint_for(&["42", "--touch", "--status", "accepted"]);
        assert!(
            hint.contains("lk edit 42 --touch --status accepted"),
            "--touch takes no value and --status is a real flag: {hint}"
        );
    }
}
