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

pub fn cmd_sync(
    json_output: bool,
    write_uids: bool,
    scope: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let scope = super::parse_scope(scope)?;
    super::log_command(
        "sync",
        &[
            ("write_uids", if write_uids { "true" } else { "false" }),
            ("scope", scope.label()),
        ],
    );

    // Resolve (connection, markdown dir, root for rel-path) per scope. User scope
    // reads the configured `user_knowledge_dir` into the global ~/.config/lk DB,
    // creating that DB on first run so a fresh machine can bootstrap from markdown alone.
    let (conn, knowledge_dir, root) = match scope {
        super::Scope::Project => (
            open_db_with_migrate()?,
            get_knowledge_dir(),
            get_project_root(),
        ),
        super::Scope::User => {
            // Scaffold the global config on first touch so `user_knowledge_dir` is
            // discoverable even via a "hand-write md, then sync" flow.
            if let Some(path) = crate::util::ensure_global_config_scaffold()
                && !json_output
            {
                println!(
                    "Created {} (edit to customize user_knowledge_dir).",
                    path.display()
                );
            }
            // sync is a write op: create the user DB on first run so a fresh machine with
            // only the markdown store (e.g. freshly cloned dotfiles) can bootstrap
            // ~/.config/lk/knowledge.db directly with `lk sync --scope user`.
            let conn = crate::util::open_or_create_user_db()?;
            let knowledge_dir = crate::util::get_user_knowledge_dir();
            let root = crate::util::user_md_root(&knowledge_dir);
            (conn, knowledge_dir, root)
        }
    };

    let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;

    let mut uids_written = 0;
    if write_uids {
        uids_written = write_uids_to_md(&conn, &knowledge_dir, &root)?;
        if uids_written > 0 {
            if !json_output {
                println!("Wrote UIDs to {uids_written} entries in markdown files.");
            }
            // Re-sync after writing UIDs to update file hashes
            sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
        }
    }

    if json_output {
        let mut out = serde_json::json!({
            "added": stats.added,
            "updated": stats.updated,
            "removed": stats.removed,
            "unchanged": stats.unchanged,
        });
        if write_uids {
            out["uids_written"] = serde_json::json!(uids_written);
        }
        println!("{}", serde_json::to_string(&out)?);
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

    // Match the canonical rel-path root used by sync_knowledge_dir (see note there).
    let canonical_root = crate::util::canonicalize_or(root);
    let root = canonical_root.as_path();

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
            // Find matching DB entry by title (skip if ambiguous)
            let matching: Vec<_> = db_entries
                .iter()
                .filter(|e| e.title == md_entry.title)
                .collect();
            if matching.len() != 1 {
                if matching.len() > 1 {
                    eprintln!(
                        "sync: skipping UID write for ambiguous title {:?} in {:?}",
                        md_entry.title, rel_path,
                    );
                }
                continue;
            }
            if let Some(db_entry) = matching.into_iter().next() {
                // Insert uid: line after the ## Entry: line or after keywords line
                let entry_header = format!("## Entry: {}", md_entry.title);
                if let Some(pos) = new_text.find(&entry_header) {
                    let after_header = pos + entry_header.len();
                    // Find the end of the header line
                    let line_end = new_text[after_header..]
                        .find('\n')
                        .map(|p| after_header + p + 1)
                        .unwrap_or(new_text.len());
                    // Check if next line is keywords:
                    let insert_pos = if new_text[line_end..].starts_with("keywords:") {
                        new_text[line_end..]
                            .find('\n')
                            .map(|p| line_end + p + 1)
                            .unwrap_or(new_text.len())
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

    // Canonicalize the rel-path root: walkdir returns canonicalized file paths, so the
    // root they're stripped against must be canonical too — otherwise (e.g. a symlinked
    // project/knowledge path) strip_prefix fails and source_file is stored as an unstable
    // absolute path, causing churn / wrong add-remove decisions across runs.
    let canonical_root = crate::util::canonicalize_or(root);
    let root = canonical_root.as_path();

    // Pre-flight: a uid appearing in more than one markdown file is an identity
    // conflict. Fail with a clear, actionable message *before* mutating anything —
    // silently skipping the duplicate insert could later delete the entry when one
    // of the files is removed while the other (unchanged) file still holds the uid.
    check_no_duplicate_uids(knowledge_dir)?;

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

/// Fail if any uid appears in more than one markdown file under `knowledge_dir`.
/// Such a duplicate is an identity conflict (e.g. a stale/renamed copy left behind,
/// or a hand-copied entry) that the user must resolve — proceeding would either abort
/// mid-import or, worse, silently drop the entry on a later sync.
fn check_no_duplicate_uids(
    knowledge_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut locations: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    // walkdir_md yields canonicalized paths, so strip against a canonical base to keep
    // the displayed filenames relative/readable (e.g. through a symlinked knowledge dir).
    let base = crate::util::canonicalize_or(knowledge_dir);
    for filepath in walkdir_md(knowledge_dir) {
        let fname = filepath.file_name().and_then(|n| n.to_str());
        if fname == Some("README.md") || fname == Some("lk-instructions.md") {
            continue;
        }
        let display = filepath
            .strip_prefix(&base)
            .unwrap_or(&filepath)
            .to_string_lossy()
            .to_string();
        let text = std::fs::read_to_string(&filepath)?;
        for entry in markdown::parse_md_entries(&text) {
            if let Some(uid) = entry.uid.as_deref().filter(|u| !u.trim().is_empty()) {
                locations
                    .entry(uid.to_string())
                    .or_default()
                    .push(display.clone());
            }
        }
    }

    // A uid is a conflict if it occurs more than once total — whether across files or
    // twice within one file. Render per-file occurrence counts so the message is accurate
    // in both cases (a single file shown as `file (x2)` rather than listed twice).
    let mut dups: Vec<(&String, &Vec<String>)> = locations
        .iter()
        .filter(|(_, occurrences)| occurrences.len() > 1)
        .collect();
    if dups.is_empty() {
        return Ok(());
    }
    dups.sort_by_key(|(uid, _)| uid.as_str());
    let detail = dups
        .iter()
        .map(|(uid, occurrences)| {
            let mut counts: std::collections::BTreeMap<&str, usize> =
                std::collections::BTreeMap::new();
            for f in occurrences.iter() {
                *counts.entry(f.as_str()).or_default() += 1;
            }
            let files = counts
                .iter()
                .map(|(f, n)| {
                    if *n > 1 {
                        format!("{f} (x{n})")
                    } else {
                        f.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("  uid {uid}: {files}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "duplicate uid(s) found in markdown entries — each uid must appear in exactly \
         one entry. Remove the stale/duplicate copy and retry:\n{detail}"
    )
    .into())
}

/// True if `e` is a SQLite UNIQUE-constraint violation on `entries.uid`.
///
/// Matches the structured `rusqlite` error (constraint-violation code) rather than the
/// human-readable "UNIQUE constraint failed:" prose, which can vary across SQLite/rusqlite
/// versions. `entries.uid` is the schema identifier SQLite echoes for the offending
/// constraint, so it stays stable and scopes the match to the uid column.
fn is_duplicate_uid_error(e: &(dyn std::error::Error + 'static)) -> bool {
    matches!(
        e.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::SqliteFailure(info, Some(msg)))
            if info.code == rusqlite::ErrorCode::ConstraintViolation
                && msg.contains("entries.uid")
    )
}

pub fn import_md_file(
    conn: &rusqlite::Connection,
    filepath: &std::path::Path,
    root: &std::path::Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let filepath = std::fs::canonicalize(filepath).unwrap_or_else(|_| filepath.to_path_buf());
    let text = std::fs::read_to_string(&filepath)?;
    let fhash = markdown::file_hash(&filepath)?;
    // Strip against a canonical root so source_file stays relative/stable even when the
    // root is reached via a symlink (the file path above is already canonicalized).
    let root = crate::util::canonicalize_or(root);
    let rel_path = filepath
        .strip_prefix(&root)
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
        let result = db::add_entry_full(
            conn,
            &entry.title,
            &entry.content,
            &entry.keywords,
            &entry.category,
            "shared",
            Some(&rel_path),
            Some(&fhash),
            entry.uid.as_deref(),
            entry
                .status
                .as_deref()
                .filter(|s| crate::db::is_valid_status(s)),
            entry.superseded_by.as_deref(),
            supersedes.as_deref(),
        );
        match result {
            Ok(_) => count += 1,
            // Cross-file md duplicates are caught up front by `check_no_duplicate_uids`,
            // so a UNIQUE(uid) failure here means the md uid collides with an existing
            // entry already in the DB (e.g. a `local` entry of the same uid). Fail with a
            // clear, actionable message rather than the opaque SQLite error; the caller's
            // transaction rolls back, leaving the DB untouched.
            Err(e) if is_duplicate_uid_error(e.as_ref()) => {
                return Err(format!(
                    "duplicate uid {} for '{}' in {} — it collides with an existing entry \
                     (e.g. a local entry of the same uid). Resolve the conflict and retry.",
                    entry.uid.as_deref().unwrap_or("?"),
                    entry.title,
                    rel_path,
                )
                .into());
            }
            Err(e) => return Err(e),
        }
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
