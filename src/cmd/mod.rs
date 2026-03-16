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
/// Enabled by setting LK_COMMAND_LOG=1 (also accepts legacy LK_SEARCH_LOG=1).
fn log_command(cmd: &str, meta: &[(&str, &str)]) {
    let enabled = std::env::var("LK_COMMAND_LOG").unwrap_or_default() == "1"
        || std::env::var("LK_SEARCH_LOG").unwrap_or_default() == "1";
    if !enabled {
        return;
    }
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let log_path = crate::util::get_project_root()
            .join(".knowledge")
            .join("command.log");

        const MAX_LOG_BYTES: u64 = 1_048_576; // 1 MB
        const KEEP_LINES: usize = 500;

        if let Ok(file_meta) = std::fs::metadata(&log_path) {
            if file_meta.len() > MAX_LOG_BYTES {
                if let Ok(content) = std::fs::read_to_string(&log_path) {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = lines.len().saturating_sub(KEEP_LINES);
                    let truncated = lines[start..].join("\n") + "\n";
                    let _ = std::fs::write(&log_path, truncated);
                }
            }
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
