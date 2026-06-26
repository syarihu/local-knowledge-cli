use std::path::{Path, PathBuf};

use crate::db;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_REPO: &str = "syarihu/local-knowledge-cli";

pub fn get_project_root() -> PathBuf {
    let cwd = std::env::current_dir().expect("Cannot get current directory");
    let mut current = cwd.as_path();
    loop {
        for marker in [".git", ".knowledge"] {
            if current.join(marker).exists() {
                return current.to_path_buf();
            }
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return cwd,
        }
    }
}

/// Resolve the root directory for DB storage.
/// In a git worktree, returns the main worktree's root so all worktrees share one DB.
/// In a normal repo or non-git project, returns the given project_root as-is.
pub fn resolve_db_root(project_root: &Path) -> PathBuf {
    let git_path = project_root.join(".git");
    // Normal repo: .git is a directory → use project_root
    if git_path.is_dir() {
        return project_root.to_path_buf();
    }
    // Worktree: .git is a file containing "gitdir: <path>"
    if git_path.is_file()
        && let Ok(content) = std::fs::read_to_string(&git_path)
        && let Some(gitdir) = content.trim().strip_prefix("gitdir: ")
    {
        let gitdir_path = if Path::new(gitdir).is_absolute() {
            PathBuf::from(gitdir)
        } else {
            project_root.join(gitdir)
        };
        // gitdir points to .git/worktrees/<name>
        // Go up to .git, then up to main worktree root
        if let Some(main_git) = gitdir_path.parent().and_then(|p| p.parent()) {
            let main_root = main_git.parent().unwrap_or(main_git);
            if let Ok(canonical) = std::fs::canonicalize(main_root)
                && canonical.join(".knowledge").exists()
            {
                return canonical;
            }
        }
    }
    project_root.to_path_buf()
}

pub fn get_db_path() -> PathBuf {
    get_db_path_for(&get_project_root())
}

/// Resolve the DB path for an explicit project root, migrating the legacy
/// `.claude/knowledge.db` location to `.knowledge/knowledge.db` if needed.
/// Has a migration side effect (rename + stderr note) when a legacy DB exists.
pub fn get_db_path_for(root: &Path) -> PathBuf {
    let db_root = resolve_db_root(root);
    let new_path = db_root.join(".knowledge").join("knowledge.db");
    if new_path.exists() {
        return new_path;
    }
    // Check old location and migrate
    let old_path = root.join(".claude").join("knowledge.db");
    if old_path.exists() {
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if std::fs::rename(&old_path, &new_path).is_ok() {
            eprintln!(
                "Note: Migrated DB from {} to {}",
                old_path.display(),
                new_path.display()
            );
            return new_path;
        }
    }
    new_path
}

pub fn get_knowledge_dir() -> PathBuf {
    get_project_root().join(".knowledge")
}

/// Pure path of the current project's DB (no side effects). Unlike `get_db_path`,
/// this never migrates the legacy `.claude/knowledge.db` location, so it is safe to
/// call from existence checks and guards. Honors worktree resolution.
pub fn project_db_path() -> PathBuf {
    let root = get_project_root();
    resolve_db_root(&root)
        .join(".knowledge")
        .join("knowledge.db")
}

/// Whether the current project has an initialized knowledge DB. Treats the legacy
/// `.claude/knowledge.db` location as "initialized" too (it migrates on first open).
/// Side-effect free.
pub fn project_db_exists() -> bool {
    project_db_path().is_file()
        || get_project_root()
            .join(".claude")
            .join("knowledge.db")
            .is_file()
}

/// Load a category template from `.knowledge/templates/{category}.md`.
/// Returns None if the template file doesn't exist or category is invalid.
pub fn load_category_template(category: &str) -> Option<String> {
    load_category_template_from(&get_knowledge_dir(), category)
}

/// Load a category template from a specific knowledge directory.
pub fn load_category_template_from(
    knowledge_dir: &std::path::Path,
    category: &str,
) -> Option<String> {
    if category.is_empty()
        || category.contains("..")
        || category.chars().any(std::path::is_separator)
    {
        return None;
    }
    let templates_dir = knowledge_dir.join("templates");
    let template_path = templates_dir.join(format!("{category}.md"));
    let base = std::fs::canonicalize(&templates_dir).ok()?;
    let resolved = std::fs::canonicalize(&template_path).ok()?;
    if !resolved.starts_with(&base) {
        return None;
    }
    std::fs::read_to_string(resolved).ok()
}

pub fn open_db_with_migrate() -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let db_path = get_db_path();
    let (conn, migrated) = db::open_db(&db_path)?;
    if migrated {
        eprintln!("Note: DB schema was migrated to the latest version.");
    }
    check_lk_version();
    Ok(conn)
}

/// Path to the user-scope (global) knowledge DB: `~/.config/lk/knowledge.db`.
pub fn get_user_db_path() -> PathBuf {
    home_dir().join(".config").join("lk").join("knowledge.db")
}

/// The global lk config directory: `~/.config/lk`.
pub fn get_user_config_dir() -> PathBuf {
    home_dir().join(".config").join("lk")
}

/// Directory holding user-scope markdown (source of truth for user knowledge).
/// Defaults to `~/.config/lk/knowledge`, overridable via `user_knowledge_dir` in
/// `~/.config/lk/config.toml` (e.g. to point at a dotfiles repo).
pub fn get_user_knowledge_dir() -> PathBuf {
    crate::config::GlobalConfig::load().user_knowledge_dir
}

/// Canonicalize a path (resolving symlinks and `.`/`..`), falling back to the path
/// unchanged when it doesn't exist yet. Used to derive a stable rel-path root for
/// `source_file`: `walkdir`/`export` work with canonicalized file paths, so the root
/// they're stripped against must be canonical too, or `strip_prefix` fails and the
/// stored `source_file` becomes an absolute, non-portable, run-dependent path.
pub fn canonicalize_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The rel-path root for user-scope markdown. Computed from the **canonicalized**
/// knowledge dir so that `export` (which canonicalizes the file it just wrote) and
/// `sync` (which canonicalizes via walkdir) derive the *same, relative* `source_file`
/// even when the knowledge dir is reached through a symlink (the dotfiles use case).
/// Falls back to the uncanonicalized parent when the dir doesn't exist yet.
pub fn user_md_root(knowledge_dir: &Path) -> PathBuf {
    let base = canonicalize_or(knowledge_dir);
    base.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| base.clone())
}

/// Restrict a path to owner-only access on Unix (files `0600`, dirs `0700`).
/// No-op on non-Unix. Best-effort — failures are ignored. The user-scope store can
/// hold private knowledge, so it should not be world-readable on shared machines.
pub fn restrict_to_owner(path: &Path, is_dir: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(mode);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, is_dir);
    }
}

/// Whether two paths refer to the same location. Compares canonicalized paths
/// (resolving symlinks and `.`/`..`), falling back to the literal path when a side
/// doesn't exist yet so equal-as-typed paths still compare equal.
pub fn paths_equivalent(a: &Path, b: &Path) -> bool {
    canonicalize_or(a) == canonicalize_or(b)
}

/// POSIX single-quote a string so it's a safe, copy/pastable shell argument for ANY
/// path — spaces, `$`, `"`, and embedded `'` (escaped as `'\''`) are all handled.
#[cfg(unix)]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// On Unix, warn if an existing directory is group/world-accessible. Even when the
/// markdown files inside are forced to `0600`, a readable/executable dir lets other
/// users list filenames (which can themselves leak sensitive info). No-op on non-Unix.
pub fn warn_if_not_owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                eprintln!(
                    "Warning: {} is not owner-only ({mode:#o}); other users can list its \
                     filenames. Run `chmod 700 {}` to restrict it.",
                    path.display(),
                    shell_quote(&path.display().to_string()),
                );
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Write a default `~/.config/lk/config.toml` if none exists, so users can
/// discover the `user_knowledge_dir` option. Best-effort; returns the path if
/// it created the file (for an informational note), else `None`.
pub fn ensure_global_config_scaffold() -> Option<PathBuf> {
    use std::io::Write;
    let config_dir = get_user_config_dir();
    let config_path = config_dir.join("config.toml");
    std::fs::create_dir_all(&config_dir).ok()?;
    // Atomic create-or-bail (O_CREAT|O_EXCL): a plain exists()-then-write is racy and
    // could clobber a real config written by a concurrent `lk`. create_new fails with
    // AlreadyExists instead, so we never overwrite an existing file.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config_path)
    {
        Ok(mut f) => {
            if f.write_all(crate::config::DEFAULT_GLOBAL_CONFIG_CONTENT.as_bytes())
                .is_err()
            {
                // Don't strand a half-written/empty config that future runs would skip
                // over (the file now "exists"); remove it so scaffolding retries later.
                drop(f);
                let _ = std::fs::remove_file(&config_path);
                return None;
            }
            drop(f);
            // Harden only when we actually created the store (mirrors the DB path).
            restrict_to_owner(&config_dir, true);
            restrict_to_owner(&config_path, false);
            Some(config_path)
        }
        Err(_) => None, // AlreadyExists (or any error) → don't claim we scaffolded it.
    }
}

/// Open the user-scope DB if it already exists, else `None`.
/// Reads must not create it. It is a DB-only store, so no auto-sync or
/// `.lk-version` checks run here (those apply to per-project markdown).
pub fn open_user_db() -> Result<Option<rusqlite::Connection>, Box<dyn std::error::Error>> {
    let path = get_user_db_path();
    if !path.is_file() {
        return Ok(None);
    }
    let (conn, _) = db::open_db(&path)?;
    Ok(Some(conn))
}

/// Open the user-scope DB, creating it if absent (for `--scope user` writes).
/// A freshly created DB (and its config dir) is restricted to owner-only access.
pub fn open_or_create_user_db() -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let path = get_user_db_path();
    if path.is_file() {
        let (conn, _) = db::open_db(&path)?;
        Ok(conn)
    } else {
        let conn = db::init_db(&path)?;
        restrict_to_owner(&get_user_config_dir(), true);
        restrict_to_owner(&path, false);
        Ok(conn)
    }
}

/// Check .knowledge/.lk-version and warn if the current binary is older than the project requires.
fn check_lk_version() {
    let version_path = get_knowledge_dir().join(".lk-version");
    if let Ok(content) = std::fs::read_to_string(&version_path) {
        let required = content.trim();
        if !required.is_empty()
            && compare_versions(VERSION, required).is_some_and(|o| o == std::cmp::Ordering::Less)
        {
            eprintln!(
                "Warning: This project requires lk >= {required}, but you have {VERSION}. Run `lk update` or `brew upgrade lk` to update."
            );
        }
    }
}

/// Compare two semver strings (e.g., "0.7.2" vs "0.8.0"). Returns None on parse failure.
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };
    let a = parse(a)?;
    let b = parse(b)?;
    Some(a.cmp(&b))
}

/// Prompt user for confirmation. Returns true if confirmed.
pub fn confirm(prompt: &str) -> bool {
    use std::io::Write;
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush().ok();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let oneline: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if oneline.chars().count() <= max_chars {
        oneline
    } else {
        let truncated: String = oneline.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

pub fn home_dir() -> PathBuf {
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

/// Calculate days since an ISO datetime string. Returns None on parse failure.
pub fn days_since(updated_at: &str) -> Option<i64> {
    use time::Month;
    use time::OffsetDateTime;
    // Parse just the date portion (YYYY-MM-DD) manually
    let date_str = &updated_at[..10.min(updated_at.len())];
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u8 = parts[1].parse().ok()?;
    let day: u8 = parts[2].parse().ok()?;
    let month = Month::try_from(month).ok()?;
    let date = time::Date::from_calendar_date(year, month, day).ok()?;
    let now = OffsetDateTime::now_utc().date();
    let duration = now - date;
    Some(duration.whole_days())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonicalize_or_falls_back_when_missing() {
        let p = Path::new("/no/such/path/lk-test-xyz");
        assert_eq!(canonicalize_or(p), p.to_path_buf());
    }

    #[cfg(unix)]
    #[test]
    fn test_shell_quote_escapes_single_quotes_and_spaces() {
        assert_eq!(shell_quote("/a/b c"), "'/a/b c'");
        // An embedded single quote is closed, escaped, and reopened.
        assert_eq!(shell_quote("/a/o'brien"), "'/a/o'\\''brien'");
    }

    /// A symlinked path and its real target must compare equal, so `--dir` matching the
    /// configured store (and rel-path roots) are detected through symlinks.
    #[cfg(unix)]
    #[test]
    fn test_paths_equivalent_through_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(paths_equivalent(&link, &real));
        // user_md_root resolves the symlink before taking the parent, so both forms agree.
        assert_eq!(user_md_root(&link), user_md_root(&real));
    }
}
