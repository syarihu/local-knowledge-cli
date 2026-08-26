use std::path::PathBuf;

use crate::db;
use crate::markdown;
use crate::util::{get_knowledge_dir, get_project_root, open_db_with_migrate};

pub struct SyncStats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// Entries an earlier `export` dropped from the file that owned them, handed back to
    /// `local` rather than deleted. Counted on its own because it is neither a change the
    /// user made nor a no-op: it is a repair, and it leaves work to do (`lk export` shares
    /// them again). See the divergent-hash arm in `sync_knowledge_dir`.
    pub restored: usize,
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
            // Always scaffold the global config on first touch (so `user_knowledge_dir`
            // is discoverable even via a "hand-write md, then sync" flow); only the
            // human-facing note is suppressed in --json mode.
            let scaffolded = crate::util::ensure_global_config_scaffold();
            if !json_output && let Some(path) = scaffolded {
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
            "restored": stats.restored,
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
        if stats.restored > 0 {
            println!(
                "  Restored:  {} (see the warnings above; `lk export` shares them again)",
                stats.restored
            );
        }
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
            restored: 0,
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
        restored: 0,
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

            match existing.get(&rel_path).map(|hashes| hashes.as_slice()) {
                // Entries under one path disagreeing on `file_hash` means the file was
                // rewritten without some of them — `export` stamps the new hash only on
                // what it wrote (see `get_shared_file_hashes`). Re-importing is the one
                // response that must not happen here: it deletes every row for the path
                // first, and the entries the file no longer lists exist nowhere else, so
                // they would go for good. Name them and leave the file alone; `lk export`
                // writes them back into it, which is also what re-unifies the hash.
                Some(hashes) if hashes.len() > 1 => {
                    // The file's entries disagree on `file_hash`, which no writer produces:
                    // `export` stamps one hash across a group and `sync` re-imports a file
                    // as a unit. It means an earlier `export` rewrote this file without some
                    // of the entries that answer to it, and those entries exist nowhere
                    // else. Re-importing would delete them, so they are handed back to
                    // `local` first — which is what they now are, knowledge with no file
                    // carrying it — and the next `export` writes them into a file again.
                    //
                    // Only entries the file no longer lists are moved. Doing this on
                    // *every* re-import would break removing an entry by deleting it from
                    // the markdown; a divergent hash is the signal that nobody asked for
                    // this removal.
                    //
                    // A uid settles it where the markdown carries one. Title is the
                    // fallback only for entries still waiting on `sync --write-uids`, and
                    // it counts rather than tests: two rows may share a title, and reading
                    // one listed entry as evidence for both would leave the second looking
                    // listed — the re-import below would then delete it. One listed
                    // uid-less entry vouches for exactly one row.
                    let text = std::fs::read_to_string(&entry)?;
                    let listed = markdown::parse_md_entries(&text);
                    let uids: std::collections::HashSet<&str> =
                        listed.iter().filter_map(|e| e.uid.as_deref()).collect();
                    let mut vouched_titles: std::collections::HashMap<&str, usize> =
                        std::collections::HashMap::new();
                    for md_entry in listed.iter().filter(|e| e.uid.is_none()) {
                        *vouched_titles.entry(md_entry.title.as_str()).or_insert(0) += 1;
                    }
                    let mut candidates = db::list_entries_by_source_file(conn, &rel_path)?;
                    // Rows carrying the file's current hash are matched against a uid-less
                    // title first: they were stamped by the export that produced the file
                    // as it stands, so where two rows share a title, they are the ones the
                    // listed entry describes. Taking them in id order instead would let the
                    // row the file dropped claim the title and send the row it kept to
                    // `local` — nothing lost, but the two would have swapped places.
                    candidates
                        .sort_by_key(|e| e.file_hash.as_deref() != Some(current_hash.as_str()));
                    let mut left_behind: Vec<db::Entry> = Vec::new();
                    for candidate in candidates {
                        if uids.contains(candidate.uid.as_str()) {
                            continue;
                        }
                        match vouched_titles.get_mut(candidate.title.as_str()) {
                            Some(left) if *left > 0 => *left -= 1,
                            _ => left_behind.push(candidate),
                        }
                    }
                    eprintln!(
                        "sync: {rel_path} no longer lists {} entr{} recorded against it (an \
                         earlier `export` replaced the file). Keeping them as local entries; \
                         `lk export` writes them back into a file.",
                        left_behind.len(),
                        if left_behind.len() == 1 { "y" } else { "ies" }
                    );
                    for e in &left_behind {
                        eprintln!("  now local: #{} {}", e.id, e.title);
                        db::detach_entry_from_file(conn, e.id)?;
                    }
                    stats.restored += left_behind.len();
                    // What is left under the path is only what the file carries, so the
                    // ordinary re-import applies — and it re-unifies the hash.
                    db::delete_entries_by_source_file(conn, &rel_path)?;
                    let count = import_md_file(conn, &entry, root)?;
                    stats.updated += count;
                }
                Some([old_hash]) if *old_hash == current_hash => {
                    stats.unchanged += 1;
                }
                Some(_) => {
                    db::delete_entries_by_source_file(conn, &rel_path)?;
                    let count = import_md_file(conn, &entry, root)?;
                    stats.updated += count;
                }
                None => {
                    let count = import_md_file(conn, &entry, root)?;
                    stats.added += count;
                }
            }
        }

        for (rel_path, hashes) in &existing {
            if found_files.contains(rel_path) {
                continue;
            }
            // A file that is gone takes its entries with it — that is how knowledge stops
            // being shared. But a path whose entries disagreed on `file_hash` was already
            // inconsistent before it went missing, and there is no file left to say which
            // entries it carried; deleting them all would throw away the ones it had
            // already dropped. They become local instead, to be shared or deleted
            // deliberately.
            if hashes.len() > 1 {
                let orphans = db::list_entries_by_source_file(conn, rel_path)?;
                eprintln!(
                    "sync: {rel_path} is gone and its entries disagreed on file_hash. \
                     Keeping {} as local entries rather than deleting them.",
                    orphans.len()
                );
                for e in &orphans {
                    eprintln!("  now local: #{} {}", e.id, e.title);
                    db::detach_entry_from_file(conn, e.id)?;
                }
                stats.restored += orphans.len();
                continue;
            }
            db::delete_entries_by_source_file(conn, rel_path)?;
            stats.removed += 1;
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
        // Hand-written md can hold a remote URL; normalize it the way `--project`
        // does so one repo never splits into a URL key and a slug key.
        let project = entry
            .project
            .as_deref()
            .and_then(crate::util::normalize_project_key);
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
            project.as_deref(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, rusqlite::Connection, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let kdir = dir.path().join(".knowledge");
        std::fs::create_dir_all(&kdir).unwrap();
        let conn = crate::db::init_db(&kdir.join("knowledge.db")).unwrap();
        (dir, conn, kdir)
    }

    fn add_shared(conn: &rusqlite::Connection, title: &str, rel: &str, hash: &str) {
        db::add_entry(
            conn,
            title,
            "body",
            &["auth".to_string()],
            "",
            "shared",
            Some(rel),
            Some(hash),
        )
        .unwrap();
    }

    #[test]
    fn test_sync_hands_back_the_entries_a_rewritten_file_dropped() {
        let (dir, conn, kdir) = store();
        let root = dir.path();
        let rel = ".knowledge/exported-auth.md";

        // The file lists Second only; First answers to the path with the hash of the
        // version that still carried it. Re-importing would delete both and bring back
        // one, so First — which exists nowhere else — must survive another way.
        let path = kdir.join("exported-auth.md");
        std::fs::write(
            &path,
            "---\nkeywords: [auth]\ncategory: exported\n---\n\n# Exported: auth\n\n\
             ## Entry: Second entry\nkeywords: [auth]\n\nsecond body\n",
        )
        .unwrap();
        add_shared(&conn, "First entry", rel, "stale-hash");
        add_shared(&conn, "Second entry", rel, &markdown::file_hash(&path).unwrap());

        let stats = sync_knowledge_dir(&conn, &kdir, root).unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(stats.removed, 0);

        let locals = db::list_entries_by_source(&conn, "local").unwrap();
        let titles: Vec<&str> = locals.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["First entry"]);
        assert!(locals[0].source_file.is_none());
        // The file keeps what it lists, and the store stops disagreeing with itself.
        let recorded = db::get_shared_file_hashes(&conn).unwrap();
        assert_eq!(recorded.get(rel).map(|h| h.len()), Some(1));
    }

    #[test]
    fn test_sync_lets_one_uid_less_title_vouch_for_only_one_row() {
        let (dir, conn, kdir) = store();
        let root = dir.path();
        let rel = ".knowledge/exported-auth.md";

        // The file lists one "Note" and carries no uid line yet; the store has two rows
        // called "Note" under the path. A title that merely appears in the file must not
        // vouch for both, or the re-import below would delete the one it dropped.
        let path = kdir.join("exported-auth.md");
        std::fs::write(
            &path,
            "---\nkeywords: [auth]\ncategory: exported\n---\n\n# Exported: auth\n\n\
             ## Entry: Note\nkeywords: [auth]\n\nthe copy the file kept\n",
        )
        .unwrap();
        let kws = ["auth".to_string()];
        db::add_entry(
            &conn,
            "Note",
            "the copy an export dropped",
            &kws,
            "",
            "shared",
            Some(rel),
            Some("stale-hash"),
        )
        .unwrap();
        db::add_entry(
            &conn,
            "Note",
            "the copy the file kept",
            &kws,
            "",
            "shared",
            Some(rel),
            Some(&markdown::file_hash(&path).unwrap()),
        )
        .unwrap();

        let stats = sync_knowledge_dir(&conn, &kdir, root).unwrap();
        assert_eq!(stats.restored, 1);

        // Both survive, and it is the dropped copy that came back as local — the file's
        // own copy stays with the file.
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);
        let locals = db::list_entries_by_source(&conn, "local").unwrap();
        let contents: Vec<&str> = locals.iter().map(|e| e.content.as_str()).collect();
        assert_eq!(contents, ["the copy an export dropped"]);
    }

    #[test]
    fn test_sync_keeps_a_divergent_paths_entries_when_the_file_is_gone() {
        let (dir, conn, kdir) = store();
        let root = dir.path();
        let divergent = ".knowledge/exported-auth.md";
        let ordinary = ".knowledge/exported-db.md";

        // Neither file exists. The divergent path cannot say which of its entries it
        // carried, so none of them are deleted; the consistent one behaves as always —
        // deleting a markdown file is how knowledge stops being shared.
        add_shared(&conn, "First entry", divergent, "h1");
        add_shared(&conn, "Second entry", divergent, "h2");
        add_shared(&conn, "Db entry", ordinary, "h3");

        let stats = sync_knowledge_dir(&conn, &kdir, root).unwrap();
        assert_eq!(stats.restored, 2);
        assert_eq!(stats.removed, 1);

        let locals = db::list_entries_by_source(&conn, "local").unwrap();
        let mut titles: Vec<&str> = locals.iter().map(|e| e.title.as_str()).collect();
        titles.sort_unstable();
        assert_eq!(titles, ["First entry", "Second entry"]);
        assert!(db::get_shared_file_hashes(&conn).unwrap().is_empty());
    }
}
