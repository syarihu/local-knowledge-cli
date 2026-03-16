mod add;
mod entry;
mod export;
mod init;
mod list;
mod search;
mod stats;
mod sync;
mod uninstall;
mod update;

pub use add::cmd_add;
pub use entry::{cmd_delete, cmd_edit, cmd_get, cmd_purge};
pub use export::{cmd_export, cmd_import};
pub use init::cmd_init;
pub use list::cmd_list;
pub use search::{cmd_command_log, cmd_search};
pub use stats::{cmd_keywords, cmd_stats};
pub use sync::cmd_sync;
pub use uninstall::cmd_uninstall;
pub use update::{cmd_update, install_embedded_commands};

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
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        let knowledge_dir = crate::util::get_knowledge_dir();
        if !knowledge_dir.exists() {
            return Ok(());
        }

        let config = crate::config::Config::load(&knowledge_dir);
        if !config.auto_sync {
            return Ok(());
        }

        let db_path = crate::util::get_db_path();
        if !db_path.exists() {
            return Ok(());
        }

        let (conn, _) = crate::db::open_db(&db_path)?;
        let existing = crate::db::get_shared_file_hashes(&conn)?;
        let root = crate::util::get_project_root();

        // Quick check: are there any changes?
        let mut has_changes = false;

        let md_files = collect_md_files(&knowledge_dir);
        let mut found_files = std::collections::HashSet::new();

        for filepath in &md_files {
            let rel_path = filepath
                .strip_prefix(&root)
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
            let stats = sync::sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
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
