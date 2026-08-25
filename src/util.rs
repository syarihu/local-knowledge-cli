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
    // Normal repo: .git is a directory → use project_root
    if project_root.join(".git").is_dir() {
        return project_root.to_path_buf();
    }
    // Worktree: share the main worktree's DB, but only when it actually holds one
    if let Some(main_root) = main_worktree_root(project_root)
        && main_root.join(".knowledge").exists()
    {
        return main_root;
    }
    project_root.to_path_buf()
}

/// Resolve the main worktree's root for `project_root`, or `None` when it is not a
/// linked worktree (a normal repo, or not git at all). In a worktree, `.git` is a
/// file holding `gitdir: <path>` pointing at `.git/worktrees/<name>`, so walking up
/// two levels reaches the main `.git` and its parent is the main worktree root.
/// The returned path is canonicalized.
pub fn main_worktree_root(project_root: &Path) -> Option<PathBuf> {
    let git_path = project_root.join(".git");
    if !git_path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&git_path).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir: ")?;
    let gitdir_path = if Path::new(gitdir).is_absolute() {
        PathBuf::from(gitdir)
    } else {
        project_root.join(gitdir)
    };
    let main_git = gitdir_path.parent()?.parent()?;
    let main_root = main_git.parent().unwrap_or(main_git);
    std::fs::canonicalize(main_root).ok()
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

// ── Project key (recorded on new entries as `entries.project`) ───────

/// Normalize a git remote URL — or a value already in slug form — to `owner/repo`.
/// Handles the three shapes git hands out (`git@host:owner/repo.git`,
/// `https://host/owner/repo.git`, `ssh://git@host/owner/repo`) so the same repo
/// always yields the same key regardless of how the remote was cloned. Namespaces
/// deeper than `owner/repo` (GitLab subgroups) are preserved. Returns `None` for a
/// value that normalizes to nothing.
pub fn normalize_project_key(raw: &str) -> Option<String> {
    /// A filesystem path, whose parent directories are this machine's layout.
    /// Windows shapes count too — a `C:\\...` drive path or a `\\\\server\\share` UNC
    /// path leaks a directory layout exactly like a POSIX one does.
    fn is_fs_path(s: &str) -> bool {
        let b = s.as_bytes();
        let windows_drive = b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/');
        s.starts_with('/')
            || s.starts_with('~')
            || s.starts_with('.')
            || windows_drive
            // A backslash anywhere means a path: no slug ever contains one, so this
            // also catches shapes the prefix checks miss (`\\\\server\\share`, or a
            // scp-looking value whose path half uses Windows separators).
            || s.contains('\\')
    }
    fn last_segment(s: &str) -> &str {
        s.trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(s)
    }

    let raw = raw.trim();
    // A key is written into markdown as a `project:` line, so a control character —
    // a newline above all — could inject further metadata: `owner/repo` followed by
    // a newline and `status: deprecated` would flip the entry's status on the next
    // `sync`. Reject rather than sanitize; no legitimate key contains one.
    if raw.chars().any(char::is_control) {
        return None;
    }
    // URI schemes are case-insensitive (RFC 3986), so `FILE://` must not slip past
    // the check below and get treated as a host-based URL.
    let file_prefix = raw
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("file://"))
        .map(|_| &raw[7..]);
    // `file://` addresses a filesystem path (its authority is empty or `localhost`),
    // so it joins the path cases rather than the host-based URL case.
    let (s, from_path) = if let Some(rest) = file_prefix {
        (rest, true)
    } else if let Some((_, rest)) = raw.split_once("://") {
        // `scheme://[user@]host[:port]/path` — drop the authority, keep the path.
        (
            rest.split_once('/').map(|(_, path)| path).unwrap_or(""),
            false,
        )
    } else {
        match raw.split_once(':') {
            // scp-like `[user@]host:path`: the head is a host, so it holds no `/` and
            // is either `user@host` or a dotted hostname. That excludes a bare
            // `owner/repo` (no colon at all) and a Windows drive letter, whose head is
            // a single letter — while still accepting a single-segment path
            // (`git@host:repo.git`), an ordinary remote that a `/`-in-path rule missed.
            Some((head, path))
                if !head.contains('/') && (head.contains('@') || head.contains('.')) =>
            {
                (path, false)
            }
            _ => (raw, false),
        }
    };
    // Only the last segment of a path names the repo — the rest is machine-specific,
    // which is exactly what a project key must not depend on. A URL's path keeps
    // every segment so deeper namespaces (GitLab subgroups) survive.
    let s = if from_path || is_fs_path(s) {
        last_segment(s)
    } else {
        s
    };
    let s = s.trim_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s).trim_matches('/');
    if s.is_empty() || s == "." || s == ".." {
        return None;
    }
    Some(s.to_string())
}

/// The repo name of a project key: its last path segment. Used for display, so a
/// slug reads as `local-knowledge-cli` rather than `syarihu/local-knowledge-cli`.
pub fn project_repo_name(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

/// `origin`'s remote URL for `root`, normalized to a slug. `None` when `root` is not
/// a repo, has no `origin`, or git is unavailable.
fn git_remote_slug(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    normalize_project_key(String::from_utf8_lossy(&out.stdout).trim())
}

/// The project key to record on entries added from `root`.
/// Resolution order: `origin`'s remote slug → the main worktree's directory name →
/// `root`'s own directory name. `None` when none of those yields a usable key.
/// (`LK_PROJECT` is honored by [`current_project_key`], deliberately not here.)
///
/// The remote slug is preferred because a linked worktree's directory name varies
/// per branch (so one repo would otherwise scatter across keys) and because it
/// carries the owner, keeping same-named repos in different orgs apart.
pub fn project_key_for(root: &Path) -> Option<String> {
    if let Some(slug) = git_remote_slug(root) {
        return Some(slug);
    }
    let dir = main_worktree_root(root);
    let dir = dir.as_deref().unwrap_or(root);
    // Through `normalize_project_key` too: a directory name may legally contain a
    // newline, and it must not reach markdown unchecked.
    dir.file_name()
        .and_then(|n| n.to_str())
        .and_then(normalize_project_key)
}

/// [`project_key_for`] against the current directory's project root, with
/// `LK_PROJECT` taking precedence.
///
/// The environment override lives here rather than in `project_key_for` on purpose:
/// it is a per-invocation escape hatch for the CLI, and a long-running MCP server
/// that inherited the variable would otherwise stamp it onto every registered
/// project's entries.
pub fn current_project_key() -> Option<String> {
    if let Ok(v) = std::env::var("LK_PROJECT")
        && let Some(key) = normalize_project_key(&v)
    {
        return Some(key);
    }
    project_key_for(&get_project_root())
}

/// Resolve an explicit `--project` value to the form stored in `entries.project`.
///
/// A bare name is expanded to the current repo's full slug when it names that repo,
/// so an explicit flag stores the same value auto-detection would have. When it
/// can't be expanded the name is kept as given and the returned note explains why —
/// bare and slug forms still match each other at query time, but the stored value is
/// then less precise.
pub fn resolve_project_arg(arg: &str) -> (Option<String>, Option<String>) {
    let Some(key) = normalize_project_key(arg) else {
        // Nothing usable left (e.g. `--project ///`). Fall back to what omitting the
        // flag would have recorded, rather than silently recording nothing.
        let fallback = current_project_key();
        let outcome = if fallback.is_some() {
            "falling back to the detected project"
        } else {
            "and nothing could be detected here, so no project is recorded"
        };
        return (
            fallback,
            // `{arg:?}` so a rejected control character is escaped rather than
            // printed — a raw newline would let a crafted value fake extra output.
            Some(format!("{arg:?} is not a usable project name; {outcome}")),
        );
    };
    if key.contains('/') {
        return (Some(key), None);
    }
    match current_project_key() {
        Some(current) if current.contains('/') && project_repo_name(&current) == key => {
            (Some(current), None)
        }
        // Don't name a cause: the current key may be another repo's slug, a bare
        // directory name, or an `LK_PROJECT` override that hides the real remote.
        // Naming a different repo is legitimate anyway (recording where knowledge
        // came from), so this is a note about precision, not an error.
        current => {
            let here = current
                .as_deref()
                .map(|c| format!(" (here: {c})"))
                .unwrap_or_default();
            (
                Some(key.clone()),
                Some(format!(
                    "'{key}' could not be expanded to a full owner/repo slug{here}, \
                     so it is stored as-is"
                )),
            )
        }
    }
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

/// Single-quote a string so it is a safe, copy/pastable shell argument.
///
/// Single quotes because they suppress every expansion — in POSIX shells and in
/// PowerShell alike. An argument may contain `$`, a backtick or `"`, and handing
/// those back inside double quotes would produce a command that substitutes or
/// executes when pasted.
///
/// The escape for an embedded `'` is picked by target OS, not by the shell that
/// will receive the text, since the shell is not knowable here. So a value
/// containing an apostrophe comes out POSIX-escaped on Unix even if the caller is
/// running PowerShell Core there, and PowerShell-escaped on Windows even under
/// Git Bash. Everything else is quoted identically for both, so only that one
/// character is affected.
///
/// Arguments made only of characters no shell treats specially are returned bare,
/// so a suggested command stays readable (`lk edit 42`, not `lk edit '42'`).
///
/// The two shell families differ only in how an embedded `'` is escaped, which
/// [`escape_single_quotes`] handles per platform. `cmd.exe` is not covered — it
/// does not treat `'` as quoting at all — so a suggestion containing a space is
/// only pasteable there after manual quoting.
pub fn shell_quote(s: &str) -> String {
    // Only characters that are inert in *both* shell families. Notably absent:
    // `,` splits a PowerShell array, so a bare keyword list (`kw1,kw2`) would
    // arrive as two arguments; a leading `@` is PowerShell splatting; `+` is an
    // operator. All three are ordinary text to a POSIX shell, which is exactly
    // why the set has to be the intersection rather than either side's.
    const ALSO_SAFE: &str = "-_./=:";
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || ALSO_SAFE.contains(c));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", escape_single_quotes(s))
    }
}

/// POSIX shells have no escape inside single quotes, so the quote is closed, a
/// literal `'` is emitted, and quoting reopens: `'\''`.
#[cfg(not(windows))]
fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// PowerShell escapes a single quote inside a literal string by doubling it.
/// Without this, a suggestion containing an apostrophe would be unpasteable on
/// the platform the release workflow builds for.
#[cfg(windows)]
fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "''")
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

/// Render one duplicate-detection hit as JSON.
///
/// The single definition of that shape. Both `lk add --json` and the MCP
/// `add_knowledge` tool report hits — under `similar_entries` when the add was
/// refused, `possibly_related` when it succeeded — and agents fall back to the
/// CLI when MCP is not wired up, so the two must agree field for field. They
/// were previously built from two separate JSON literals and had already drifted
/// apart (the MCP side omitted `keywords` and `snippet`); sharing the builder is
/// what keeps them from drifting again.
pub fn similar_entry_json(
    conn: &rusqlite::Connection,
    s: &db::SimilarEntry,
    scope_label: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": s.entry.id,
        "uid": s.entry.uid,
        "scope": scope_label,
        "title": s.entry.title,
        "keywords": db::get_keywords(conn, s.entry.id).unwrap_or_default(),
        "snippet": truncate_str(&s.entry.content, 300),
        "match_reason": s.reason.as_str(),
        "title_similarity": round2(s.title_sim),
        "keyword_similarity": round2(s.kw_sim),
    })
}

/// Two decimal places — the scores are advisory, and full float noise in the
/// output only invites agents to treat them as precise.
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
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
    fn test_normalize_project_key_accepts_every_remote_shape() {
        // The same repo must yield one key no matter how the remote was cloned.
        for raw in [
            "git@github.com:syarihu/local-knowledge-cli.git",
            "https://github.com/syarihu/local-knowledge-cli.git",
            "https://github.com/syarihu/local-knowledge-cli",
            "ssh://git@github.com/syarihu/local-knowledge-cli.git",
            "git://github.com/syarihu/local-knowledge-cli.git",
            "syarihu/local-knowledge-cli",
        ] {
            assert_eq!(
                normalize_project_key(raw).as_deref(),
                Some("syarihu/local-knowledge-cli"),
                "failed for {raw}"
            );
        }
    }

    #[test]
    fn test_normalize_project_key_keeps_deeper_namespaces() {
        // GitLab subgroups: keep every segment rather than flattening to owner/repo.
        assert_eq!(
            normalize_project_key("git@gitlab.com:group/sub/repo.git").as_deref(),
            Some("group/sub/repo")
        );
    }

    #[test]
    fn test_normalize_project_key_bare_name_and_empty() {
        assert_eq!(
            normalize_project_key("local-knowledge-cli").as_deref(),
            Some("local-knowledge-cli")
        );
        // Nothing usable left after stripping.
        assert_eq!(normalize_project_key("   "), None);
        assert_eq!(normalize_project_key("https://github.com/"), None);
    }

    #[test]
    fn test_normalize_project_key_local_path_remote_keeps_only_the_name() {
        // A path remote must not bake this machine's directory layout into the key.
        assert_eq!(
            normalize_project_key("/Users/me/git/other-repo.git").as_deref(),
            Some("other-repo")
        );
        assert_eq!(
            normalize_project_key("../sibling-repo").as_deref(),
            Some("sibling-repo")
        );
        assert_eq!(normalize_project_key("/"), None);
    }

    #[test]
    fn test_normalize_project_key_file_url_keeps_only_the_name() {
        // `git clone file:///path/repo` is a normal clone; its remote must not bake
        // this machine's directory layout (or the username in it) into the key.
        assert_eq!(
            normalize_project_key("file:///Users/me/git/other-repo.git").as_deref(),
            Some("other-repo")
        );
        assert_eq!(
            normalize_project_key("file://localhost/Users/me/git/other-repo.git").as_deref(),
            Some("other-repo")
        );
        // Same for an ssh remote pointing at an absolute path.
        assert_eq!(
            normalize_project_key("git@host:/srv/git/other-repo.git").as_deref(),
            Some("other-repo")
        );
    }

    #[test]
    fn test_normalize_project_key_windows_paths_keep_only_the_name() {
        // `\` is a separator too: a Windows path leaks a directory layout (and the
        // username in it) just like a POSIX one.
        for raw in [
            r"C:\Users\me\repo.git",
            r"c:/Users/me/repo.git",
            r"\\server\share\repo.git",
            r"..\repo",
            r"~\repo",
        ] {
            assert_eq!(
                normalize_project_key(raw).as_deref(),
                Some("repo"),
                "failed for {raw}"
            );
        }
    }

    #[test]
    fn test_normalize_project_key_rejects_control_characters() {
        // A newline would become a second metadata line once exported to markdown.
        assert_eq!(
            normalize_project_key("owner/repo\nstatus: deprecated"),
            None
        );
        assert_eq!(normalize_project_key("owner/repo\r\n## Entry: fake"), None);
        assert_eq!(normalize_project_key("owner\u{0}repo"), None);
    }

    #[test]
    fn test_normalize_project_key_scp_with_single_segment_path() {
        // `git@host:repo.git` is an ordinary remote; the host must not survive in the
        // key just because the path has no `/`.
        assert_eq!(
            normalize_project_key("git@example.internal:repo.git").as_deref(),
            Some("repo")
        );
        assert_eq!(
            normalize_project_key("example.internal:repo.git").as_deref(),
            Some("repo")
        );
    }

    #[test]
    fn test_normalize_project_key_ignores_scheme_case() {
        // URI schemes are case-insensitive, and `file://` must always be treated as
        // a path — otherwise the machine's directory layout lands in the key.
        assert_eq!(
            normalize_project_key("FILE:///Users/me/repo.git").as_deref(),
            Some("repo")
        );
        assert_eq!(
            normalize_project_key("HTTPS://github.com/syarihu/repo.git").as_deref(),
            Some("syarihu/repo")
        );
    }

    #[test]
    fn test_normalize_project_key_scp_shape_with_windows_separators() {
        // A pathological but reachable mix: an scp-looking value whose path half uses
        // `\`. No slug contains a backslash, so it must be read as a path.
        assert_eq!(
            normalize_project_key(r"git@host:owner\Users\me\repo.git").as_deref(),
            Some("repo")
        );
    }

    #[test]
    fn test_normalize_project_key_scp_without_user() {
        // `host:owner/repo` is valid scp syntax; it must land on the same key as the
        // usual `git@host:owner/repo` rather than keeping the host in the slug.
        assert_eq!(
            normalize_project_key("github.com:syarihu/local-knowledge-cli.git").as_deref(),
            Some("syarihu/local-knowledge-cli")
        );
    }

    #[test]
    fn test_project_repo_name_is_last_segment() {
        assert_eq!(
            project_repo_name("syarihu/local-knowledge-cli"),
            "local-knowledge-cli"
        );
        assert_eq!(project_repo_name("group/sub/repo"), "repo");
        // A bare name is already the repo name.
        assert_eq!(
            project_repo_name("local-knowledge-cli"),
            "local-knowledge-cli"
        );
    }

    #[test]
    fn test_main_worktree_root_none_outside_worktree() {
        // A normal repo (.git is a directory) and a non-git dir are both "not a
        // linked worktree", so DB/key resolution stays on the given root.
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(main_worktree_root(tmp.path()), None);
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        assert_eq!(main_worktree_root(tmp.path()), None);
    }

    #[test]
    fn test_main_worktree_root_follows_gitdir_file() {
        // Mirror git's layout: main/.git/worktrees/<name>, and the worktree's .git is
        // a file pointing at it. The main worktree root is two levels up from there.
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let wt_meta = main.join(".git").join("worktrees").join("feature");
        std::fs::create_dir_all(&wt_meta).unwrap();
        let wt = tmp.path().join("feature");
        std::fs::create_dir(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", wt_meta.display())).unwrap();

        assert_eq!(
            main_worktree_root(&wt),
            Some(std::fs::canonicalize(&main).unwrap())
        );
    }

    #[test]
    fn test_canonicalize_or_falls_back_when_missing() {
        let p = Path::new("/no/such/path/lk-test-xyz");
        assert_eq!(canonicalize_or(p), p.to_path_buf());
    }

    #[cfg(unix)]
    #[test]
    fn test_shell_quote_wraps_anything_special() {
        assert_eq!(shell_quote("/a/b c"), "'/a/b c'");
    }

    /// POSIX has no escape inside single quotes, so quoting is closed and reopened.
    #[cfg(not(windows))]
    #[test]
    fn test_shell_quote_escapes_single_quotes_posix() {
        assert_eq!(shell_quote("/a/o'brien"), "'/a/o'\\''brien'");
    }

    /// PowerShell doubles the quote instead. Not exercised by CI — the test matrix
    /// is ubuntu and macos — so it only runs for someone building on Windows.
    #[cfg(windows)]
    #[test]
    fn test_shell_quote_doubles_single_quotes_powershell() {
        assert_eq!(shell_quote("/a/o'brien"), "'/a/o''brien'");
    }

    /// Single quotes, not double: these must not expand when pasted.
    #[test]
    fn test_shell_quote_neutralizes_expansion() {
        assert_eq!(shell_quote("costs $HOME"), "'costs $HOME'");
        assert_eq!(shell_quote("run `date`"), "'run `date`'");
        assert_eq!(shell_quote("a\"b"), "'a\"b'");
    }

    /// Plain arguments stay bare so a suggested command reads naturally, while a
    /// path with nothing special in it is still safe to hand to a shell.
    #[test]
    fn test_shell_quote_leaves_plain_arguments_bare() {
        assert_eq!(shell_quote("42"), "42");
        assert_eq!(shell_quote("--content"), "--content");
        assert_eq!(shell_quote("--status=accepted"), "--status=accepted");
        assert_eq!(shell_quote("/usr/local/bin/lk"), "/usr/local/bin/lk");
        assert_eq!(shell_quote("7918518402e5"), "7918518402e5");
        // Empty stays quoted — bare would vanish from the command line.
        assert_eq!(shell_quote(""), "''");
    }

    /// Inert to a POSIX shell but not to PowerShell, so they cannot be left bare:
    /// `,` would split a keyword list into two arguments, a leading `@` splats, and
    /// `+` is an operator.
    #[test]
    fn test_shell_quote_wraps_powershell_metacharacters() {
        assert_eq!(shell_quote("kw1,kw2"), "'kw1,kw2'");
        assert_eq!(shell_quote("@mention"), "'@mention'");
        assert_eq!(shell_quote("a+b"), "'a+b'");
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
