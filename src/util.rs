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
    let root = get_project_root();
    let db_root = resolve_db_root(&root);
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
