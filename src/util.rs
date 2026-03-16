use std::path::PathBuf;

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

pub fn get_db_path() -> PathBuf {
    let root = get_project_root();
    let new_path = root.join(".knowledge").join("knowledge.db");
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
        if !required.is_empty() && compare_versions(VERSION, required).is_some_and(|o| o == std::cmp::Ordering::Less) {
            eprintln!(
                "Warning: This project requires lk >= {required}, but you have {VERSION}. Run `lk update` or `brew upgrade lk` to update."
            );
        }
    }
}

/// Compare two semver strings (e.g., "0.7.2" vs "0.8.0"). Returns None on parse failure.
fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
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
