use std::path::PathBuf;

use crate::db;
use crate::markdown;
use crate::util::{get_knowledge_dir, get_project_root, open_db_with_migrate};

pub struct SyncStats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
}

pub fn cmd_sync(json_output: bool, write_uids: bool) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command("sync", &[("write_uids", if write_uids { "true" } else { "false" })]);
    let conn = open_db_with_migrate()?;
    let root = get_project_root();
    let knowledge_dir = get_knowledge_dir();
    let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;

    if write_uids {
        let uid_count = write_uids_to_md(&conn, &knowledge_dir, &root)?;
        if uid_count > 0 {
            if json_output {
                // Will include in output below
            } else {
                println!("Wrote UIDs to {uid_count} entries in markdown files.");
            }
            // Re-sync after writing UIDs to update file hashes
            sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "added": stats.added,
                "updated": stats.updated,
                "removed": stats.removed,
                "unchanged": stats.unchanged,
            }))?
        );
    } else {
        println!("Sync complete:");
        println!("  Added:     {}", stats.added);
        println!("  Updated:   {}", stats.updated);
        println!("  Removed:   {}", stats.removed);
        println!("  Unchanged: {}", stats.unchanged);
    }
    Ok(())
}

/// Write UIDs back to markdown files for entries that don't have them.
fn write_uids_to_md(
    conn: &rusqlite::Connection,
    knowledge_dir: &std::path::Path,
    root: &std::path::Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut total_written = 0;
    let uid_re = regex::Regex::new(r"(?m)^uid:\s*.+$").unwrap();

    for filepath in walkdir_md(knowledge_dir) {
        let fname = filepath.file_name().and_then(|n| n.to_str());
        if fname == Some("README.md") || fname == Some("lk-instructions.md") {
            continue;
        }

        let text = std::fs::read_to_string(&filepath)?;
        let entries = markdown::parse_md_entries(&text);

        // Check if any entry is missing a uid
        let needs_update = entries.iter().any(|e| e.uid.is_none());
        if !needs_update {
            continue;
        }

        let rel_path = filepath
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| filepath.to_string_lossy().to_string());

        // Get DB entries for this file to match UIDs
        let db_entries = crate::db::list_entries_by_source_file(conn, &rel_path)?;

        let mut new_text = text.clone();
        for md_entry in &entries {
            if md_entry.uid.is_some() {
                continue;
            }
            // Find matching DB entry by title
            if let Some(db_entry) = db_entries.iter().find(|e| e.title == md_entry.title) {
                // Insert uid: line after the ## Entry: line or after keywords line
                let entry_header = format!("## Entry: {}", md_entry.title);
                if let Some(pos) = new_text.find(&entry_header) {
                    let after_header = pos + entry_header.len();
                    // Find the end of the header line
                    let line_end = new_text[after_header..].find('\n').map(|p| after_header + p + 1).unwrap_or(new_text.len());
                    // Check if next line is keywords:
                    let insert_pos = if new_text[line_end..].starts_with("keywords:") {
                        let kw_end = new_text[line_end..].find('\n').map(|p| line_end + p + 1).unwrap_or(new_text.len());
                        kw_end
                    } else {
                        line_end
                    };
                    let uid_line = format!("uid: {}\n", db_entry.uid);
                    if !uid_re.is_match(&new_text[pos..insert_pos.min(pos + 500)]) {
                        new_text.insert_str(insert_pos, &uid_line);
                        total_written += 1;
                    }
                }
            }
        }

        if new_text != text {
            std::fs::write(&filepath, new_text)?;
        }
    }

    Ok(total_written)
}

pub fn sync_knowledge_dir(
    conn: &rusqlite::Connection,
    knowledge_dir: &std::path::Path,
    root: &std::path::Path,
) -> Result<SyncStats, Box<dyn std::error::Error>> {
    if !knowledge_dir.exists() {
        return Ok(SyncStats {
            added: 0,
            updated: 0,
            removed: 0,
            unchanged: 0,
        });
    }

    let mut stats = SyncStats {
        added: 0,
        updated: 0,
        removed: 0,
        unchanged: 0,
    };

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let existing = db::get_shared_file_hashes(conn)?;
        let mut found_files = std::collections::HashSet::new();

        for entry in walkdir_md(knowledge_dir) {
            let fname = entry.file_name().and_then(|n| n.to_str());
            if fname == Some("README.md") || fname == Some("lk-instructions.md") {
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
                    db::delete_entries_by_source_file(conn, &rel_path)?;
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
                db::delete_entries_by_source_file(conn, rel_path)?;
                stats.removed += 1;
            }
        }

        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            return Err(e);
        }
    }

    Ok(stats)
}

pub fn import_md_file(
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
        let supersedes = if entry.supersedes.is_empty() {
            None
        } else {
            Some(entry.supersedes.join(","))
        };
        db::add_entry_full(
            conn,
            &entry.title,
            &entry.content,
            &entry.keywords,
            &entry.category,
            "shared",
            Some(&rel_path),
            Some(&fhash),
            entry.uid.as_deref(),
            entry.status.as_deref(),
            entry.superseded_by.as_deref(),
            supersedes.as_deref(),
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
