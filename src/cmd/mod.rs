mod add;
mod entry;
mod export;
mod init;
mod install_mcp;
mod list;
mod search;
mod stats;
mod sync;
mod uninstall;
mod update;

pub use add::cmd_add;
pub use entry::{cmd_delete, cmd_edit, cmd_get, cmd_purge, cmd_supersede};
pub use export::{cmd_export, cmd_import};
pub use init::cmd_init;
pub use install_mcp::{cmd_install_mcp, cmd_uninstall_mcp};
pub use list::cmd_list;
pub use search::{cmd_command_log, cmd_search};
pub use stats::{cmd_keywords, cmd_stats};
pub use sync::cmd_sync;
pub use uninstall::cmd_uninstall;
pub use update::{cmd_install_commands, cmd_update};

use rusqlite::Connection;

// ── Scope resolution (project vs user-scope DB) ──────────────────────

/// Which knowledge store a write/target command operates on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Project,
    User,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::User => "user",
        }
    }
}

/// Parse a write/target `--scope` value (`project` | `user`).
pub fn parse_scope(s: &str) -> Result<Scope, Box<dyn std::error::Error>> {
    match s {
        "project" => Ok(Scope::Project),
        "user" => Ok(Scope::User),
        other => Err(format!("Invalid --scope '{other}' (expected: project, user)").into()),
    }
}

/// Parse an optional write/target `--scope` value (None = auto-resolve).
pub fn parse_scope_opt(s: Option<&str>) -> Result<Option<Scope>, Box<dyn std::error::Error>> {
    match s {
        Some(v) => Ok(Some(parse_scope(v)?)),
        None => Ok(None),
    }
}

/// Open the connection for a single scope. `User` errors if no user DB exists yet.
fn open_scope_conn(scope: Scope) -> Result<Connection, Box<dyn std::error::Error>> {
    match scope {
        Scope::Project => crate::util::open_db_with_migrate(),
        Scope::User => crate::util::open_user_db()?.ok_or_else(|| {
            "No user-scope knowledge DB exists yet (~/.config/lk/knowledge.db). \
             Create one with `lk add \"...\" --scope user`."
                .into()
        }),
    }
}

/// Look up `<id-or-uid>` within an already-open connection (None if absent).
/// Numeric args are matched by id, otherwise by UID.
pub fn lookup_in_conn(
    conn: &Connection,
    arg: &str,
) -> Result<Option<crate::db::Entry>, Box<dyn std::error::Error>> {
    if let Ok(id) = arg.parse::<i64>() {
        crate::db::get_entry(conn, id)
    } else {
        crate::db::get_entry_by_uid(conn, arg)
    }
}

/// Resolve `<id-or-uid>` to an entry within an already-open connection.
fn resolve_in_conn(
    conn: &Connection,
    arg: &str,
    scope: Scope,
) -> Result<crate::db::Entry, Box<dyn std::error::Error>> {
    lookup_in_conn(conn, arg)?
        .ok_or_else(|| format!("Entry '{arg}' not found in {} scope", scope.label()).into())
}

/// Infer the scope of an `<id-or-uid>` when `--scope` is not given.
/// Numeric ids are project-only (back-compat); UIDs are looked up project-then-user.
fn infer_scope(arg: &str) -> Result<Scope, Box<dyn std::error::Error>> {
    if arg.parse::<i64>().is_ok() {
        return Ok(Scope::Project);
    }
    let pconn = crate::util::open_db_with_migrate()?;
    if crate::db::get_entry_by_uid(&pconn, arg)?.is_some() {
        return Ok(Scope::Project);
    }
    if let Some(uconn) = crate::util::open_user_db()?
        && crate::db::get_entry_by_uid(&uconn, arg)?.is_some()
    {
        return Ok(Scope::User);
    }
    Err(format!("Entry '{arg}' not found in any scope").into())
}

/// Resolve a single target (`get`/`edit`/`delete`) to its owning DB connection
/// and entry. Mutations then run on the returned connection, so project and
/// user-scope entries are edited in the right DB.
pub fn resolve_target(
    arg: &str,
    scope: Option<Scope>,
) -> Result<(Connection, crate::db::Entry), Box<dyn std::error::Error>> {
    let scope = match scope {
        Some(s) => s,
        None if arg.parse::<i64>().is_ok() => Scope::Project,
        None => infer_scope(arg)?,
    };
    let conn = open_scope_conn(scope)?;
    let entry = resolve_in_conn(&conn, arg, scope)?;
    Ok((conn, entry))
}

/// Resolve both `supersede` targets in a SINGLE connection so the two updates
/// stay in one transaction. Cross-scope supersede is unsupported: if `new` is
/// not in the same DB as `old`, resolution errors.
pub fn resolve_supersede_pair(
    old: &str,
    new: &str,
    scope: Option<Scope>,
) -> Result<(Connection, crate::db::Entry, crate::db::Entry), Box<dyn std::error::Error>> {
    let scope = match scope {
        Some(s) => s,
        None if old.parse::<i64>().is_ok() => Scope::Project,
        None => infer_scope(old)?,
    };
    let conn = open_scope_conn(scope)?;
    let old_entry = resolve_in_conn(&conn, old, scope)?;
    let new_entry = resolve_in_conn(&conn, new, scope)?;
    Ok((conn, old_entry, new_entry))
}

/// Connections to query for a read command (`search`/`list`/`stats`), honoring
/// `--scope` (`project` | `user` | `all`, default `all`). Each is paired with its
/// scope label. The user DB is skipped when it does not exist.
pub fn read_connections(
    scope: Option<&str>,
) -> Result<Vec<(Connection, &'static str)>, Box<dyn std::error::Error>> {
    let (want_project, want_user) = match scope {
        None | Some("all") => (true, true),
        Some("project") => (true, false),
        Some("user") => (false, true),
        Some(other) => {
            return Err(format!("Invalid --scope '{other}' (expected: project, user, all)").into());
        }
    };
    let mut conns: Vec<(Connection, &'static str)> = Vec::new();
    if want_project {
        conns.push((crate::util::open_db_with_migrate()?, "project"));
    }
    if want_user && let Some(c) = crate::util::open_user_db()? {
        conns.push((c, "user"));
    }
    Ok(conns)
}

/// Log a command invocation to .knowledge/command.log (fire-and-forget).
/// Enabled by config `command_log = true` or env `LK_COMMAND_LOG=1` / `LK_SEARCH_LOG=1`.
fn log_command(cmd: &str, meta: &[(&str, &str)]) {
    let config = crate::config::Config::load(&crate::util::get_knowledge_dir());
    if !config.command_log {
        return;
    }
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let log_path = crate::util::get_project_root()
            .join(".knowledge")
            .join("command.log");

        const MAX_LOG_BYTES: u64 = 1_048_576; // 1 MB
        const KEEP_LINES: usize = 500;

        if let Ok(file_meta) = std::fs::metadata(&log_path)
            && file_meta.len() > MAX_LOG_BYTES
            && let Ok(content) = std::fs::read_to_string(&log_path)
        {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(KEEP_LINES);
            let truncated = lines[start..].join("\n") + "\n";
            let _ = std::fs::write(&log_path, truncated);
        }

        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let meta_str: Vec<String> = meta.iter().map(|(k, v)| format!("{k}={v}")).collect();
        writeln!(
            f,
            "[{}] cmd={cmd} {}",
            crate::util::now_iso(),
            meta_str.join(" ")
        )?;
        Ok(())
    })();
}

/// Auto-sync .knowledge/ markdown files if enabled and changes are detected.
/// Runs silently — errors are ignored since this is a best-effort optimization.
pub fn maybe_auto_sync() {
    maybe_auto_sync_for(&crate::util::get_project_root());
}

/// Auto-sync for a specific project root path.
/// Used by MCP server to sync a resolved project instead of CWD-based resolution.
pub fn maybe_auto_sync_for(project_root: &std::path::Path) {
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        let knowledge_dir = project_root.join(".knowledge");
        if !knowledge_dir.exists() {
            return Ok(());
        }

        let config = crate::config::Config::load(&knowledge_dir);
        if !config.auto_sync {
            return Ok(());
        }

        let db_root = crate::util::resolve_db_root(project_root);
        let db_path = db_root.join(".knowledge").join("knowledge.db");
        if !db_path.exists() {
            return Ok(());
        }

        let (conn, _) = crate::db::open_db(&db_path)?;
        let existing = crate::db::get_shared_file_hashes(&conn)?;

        // Quick check: are there any changes?
        let mut has_changes = false;

        let md_files = collect_md_files(&knowledge_dir);
        let mut found_files = std::collections::HashSet::new();

        for filepath in &md_files {
            let rel_path = filepath
                .strip_prefix(project_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| filepath.to_string_lossy().to_string());
            found_files.insert(rel_path.clone());

            let current_hash = crate::markdown::file_hash(filepath)?;
            match existing.get(&rel_path) {
                Some(old_hash) if *old_hash == current_hash => {}
                _ => {
                    has_changes = true;
                    break;
                }
            }
        }

        // Check for removed files
        if !has_changes {
            for rel_path in existing.keys() {
                if !found_files.contains(rel_path) {
                    has_changes = true;
                    break;
                }
            }
        }

        if has_changes {
            let stats = sync::sync_knowledge_dir(&conn, &knowledge_dir, project_root)?;
            let total = stats.added + stats.updated + stats.removed;
            if total > 0 {
                eprintln!(
                    "Auto-synced: {} added, {} updated, {} removed",
                    stats.added, stats.updated, stats.removed
                );
            }
        }

        Ok(())
    })();
}

/// Collect .md files from knowledge dir (excluding README.md).
fn collect_md_files(knowledge_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let base = match std::fs::canonicalize(knowledge_dir) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    collect_md_files_inner(&base, &base, &mut files);
    files.sort();
    files
}

fn collect_md_files_inner(
    dir: &std::path::Path,
    base: &std::path::Path,
    files: &mut Vec<std::path::PathBuf>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let real_path = match std::fs::canonicalize(&path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !real_path.starts_with(base) {
            continue;
        }
        if real_path.is_dir() {
            collect_md_files_inner(&real_path, base, files);
        } else if real_path.extension().and_then(|e| e.to_str()) == Some("md")
            && real_path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n != "README.md")
        {
            files.push(real_path);
        }
    }
}
