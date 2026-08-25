use std::path::PathBuf;

use crate::cmd::sync::import_md_file;
use crate::db;
use crate::markdown;
use crate::util::{get_knowledge_dir, get_project_root, now_iso, open_db_with_migrate};

pub fn cmd_export(
    dir: Option<PathBuf>,
    ids: Option<&str>,
    query: Option<&str>,
    allow_secrets: bool,
    scope: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let scope = super::parse_scope(scope)?;

    // Resolve the (connection, default output dir, root for rel-path, secret config)
    // per scope. Project keeps its historical root (the project root) so stored
    // source_file paths are unchanged; user scope derives its root from the
    // canonicalized knowledge dir (see `root` below).
    let (conn, default_dir, secret_detection) = match scope {
        super::Scope::Project => (
            open_db_with_migrate()?,
            get_knowledge_dir(),
            crate::config::Config::load(&get_knowledge_dir()).secret_detection,
        ),
        super::Scope::User => {
            if let Some(path) = crate::util::ensure_global_config_scaffold() {
                println!(
                    "Created {} (edit to customize user_knowledge_dir).",
                    path.display()
                );
            }
            (
                crate::util::open_or_create_user_db()?,
                crate::util::get_user_knowledge_dir(),
                crate::config::GlobalConfig::load().secret_detection,
            )
        }
    };

    // The managed store for this scope (project `.knowledge` or user `user_knowledge_dir`).
    let managed_dir = default_dir.clone();
    let output_dir = dir.unwrap_or(default_dir);
    // Only harden a directory we actually create — never clobber the permissions of
    // a pre-existing dir the user manages (e.g. a custom `user_knowledge_dir`).
    let dir_existed = output_dir.exists();
    std::fs::create_dir_all(&output_dir)?;

    // An export to a dir OTHER than the scope's managed store is a one-off DUMP, not a
    // managed export: `lk sync` only reads the managed dir (project `.knowledge/` or
    // user `user_knowledge_dir`), so if we flipped these entries to `shared` with a
    // source_file pointing at the custom dir, the next sync would treat those files as
    // "missing" and DELETE the entries (data loss). So dump-only: write the md but leave
    // entries `local`. When the dir equals the managed store (compared canonically), it's
    // a normal managed export.
    let dump_only = !crate::util::paths_equivalent(&output_dir, &managed_dir);
    if dump_only {
        let hint = match scope {
            super::Scope::Project => {
                "export without `--dir` (into the project's .knowledge/) to manage a synced store."
            }
            super::Scope::User => {
                "set `user_knowledge_dir` in ~/.config/lk/config.toml to manage a synced store."
            }
        };
        eprintln!(
            "Warning: `--dir` is a one-off dump; entries stay local and are NOT synced. {hint}"
        );
    }

    let restrict_files = scope == super::Scope::User;
    let root = match scope {
        super::Scope::Project => get_project_root(),
        // Canonicalized parent so export and sync agree on the rel-path even through
        // a symlinked knowledge dir (keeps source_file relative + portable).
        super::Scope::User => {
            if dir_existed {
                // We won't clobber an existing dir's mode, but a group/world-readable
                // store leaks filenames even though the md files are 0600 — so warn.
                crate::util::warn_if_not_owner_only(&output_dir);
            } else {
                crate::util::restrict_to_owner(&output_dir, true);
            }
            crate::util::user_md_root(&output_dir)
        }
    };

    export_to_dir(
        &conn,
        &output_dir,
        &root,
        ids,
        query,
        allow_secrets,
        secret_detection,
        restrict_files,
        !dump_only,
    )
}

#[allow(clippy::too_many_arguments)]
fn export_to_dir(
    conn: &rusqlite::Connection,
    output_dir: &std::path::Path,
    root: &std::path::Path,
    ids: Option<&str>,
    query: Option<&str>,
    allow_secrets: bool,
    secret_detection: bool,
    restrict_files: bool,
    flip_to_shared: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Canonical rel-path root: the stored source_file is computed from the canonicalized
    // written file, so the root it's stripped against must be canonical too (else a
    // symlinked project/knowledge path yields an unstable absolute source_file). Matches
    // what sync derives from walkdir, keeping the md→DB round-trip stable.
    let canonical_root = crate::util::canonicalize_or(root);
    let root = canonical_root.as_path();

    let entries = if let Some(ids_str) = ids {
        // Export specific entries by ID
        let mut selected = Vec::new();
        for id_str in ids_str.split(',') {
            let id: i64 = id_str
                .trim()
                .parse()
                .map_err(|_| format!("Invalid ID: {}", id_str.trim()))?;
            match db::get_entry(conn, id)? {
                Some(entry) => {
                    if entry.source != "local" {
                        eprintln!("Warning: Entry #{id} is already shared, skipping.");
                    } else {
                        selected.push(entry);
                    }
                }
                None => {
                    return Err(format!("Entry #{id} not found").into());
                }
            }
        }
        selected
    } else if let Some(q) = query {
        // Export entries matching a search query

        db::search_entries(
            conn,
            q,
            false,
            None,
            Some("local"),
            None,
            None,
            None,
            None,
            100,
        )?
    } else {
        // Export all local entries
        db::list_entries_by_source(conn, "local")?
    };

    if entries.is_empty() {
        println!("No local entries to export.");
        return Ok(());
    }

    // Secret detection before export
    if !allow_secrets && secret_detection {
        let mut all_matches = Vec::new();
        for entry in &entries {
            let text = format!("{}\n{}", entry.title, entry.content);
            let matches = crate::secrets::check_for_secrets(&text);
            for m in matches {
                all_matches.push((entry.id, entry.title.clone(), m));
            }
        }
        if !all_matches.is_empty() {
            eprintln!("Potential secrets detected in entries to export:");
            for (id, title, m) in &all_matches {
                eprintln!(
                    "  Entry #{id} \"{title}\": {} ({})",
                    m.pattern_name, m.matched
                );
            }
            eprintln!("\nUse --allow-secrets to override this check.");
            return Err("secret_detected".into());
        }
    }

    // Group by first keyword — use BTreeMap for stable alphabetical order
    let mut groups: std::collections::BTreeMap<String, Vec<db::Entry>> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let kws = db::get_keywords(conn, entry.id)?;
        let group = kws
            .first()
            .cloned()
            .unwrap_or_else(|| "general".to_string());
        groups.entry(group).or_default().push(entry);
    }

    let mut total = 0;
    for (group_name, group_entries) in &groups {
        // Sort entries within each group by title for stable output
        let mut sorted_entries: Vec<&db::Entry> = group_entries.iter().collect();
        sorted_entries.sort_by_key(|e| e.title.to_lowercase());

        let filename = format!("exported-{group_name}.md");
        let filepath = output_dir.join(&filename);

        let mut all_kws: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in &sorted_entries {
            let kws = db::get_keywords(conn, entry.id)?;
            all_kws.extend(kws);
        }

        let mut lines = Vec::new();
        lines.push("---".to_string());
        lines.push(format!(
            "keywords: [{}]",
            all_kws.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        lines.push("category: exported".to_string());
        lines.push("---\n".to_string());
        lines.push(format!("# Exported: {group_name}\n"));

        for entry in &sorted_entries {
            let kws = db::get_keywords(conn, entry.id)?;
            lines.push(format!("## Entry: {}", entry.title));
            lines.push(format!("keywords: [{}]", kws.join(", ")));
            if !entry.uid.is_empty() {
                lines.push(format!("uid: {}", entry.uid));
            }
            if entry.status != "active" {
                lines.push(format!("status: {}", entry.status));
            }
            // Carried through md so a `sync` (which deletes and re-inserts the
            // file's entries) doesn't drop the recorded project.
            if let Some(ref project) = entry.project {
                lines.push(format!("project: {project}"));
            }
            if let Some(ref sb) = entry.superseded_by {
                lines.push(format!("superseded_by: {sb}"));
            }
            if let Some(ref ss) = entry.supersedes {
                lines.push(format!("supersedes: [{ss}]"));
            }
            lines.push(String::new());
            lines.push(entry.content.clone());
            lines.push(String::new());
        }

        std::fs::write(&filepath, lines.join("\n"))?;
        // User-scope md can hold private knowledge — keep it owner-only even if the
        // containing dir is loosened. (Git tracks only the exec bit, so 0600 vs 0644
        // causes no diff churn for a dotfiles-tracked store.)
        if restrict_files {
            crate::util::restrict_to_owner(&filepath, false);
        }

        // Flip entries to `shared` and record their source_file/hash — but ONLY for a
        // managed store. A dump-only export (user-scope `--dir`) leaves entries `local`
        // so a later `sync --scope user` (which never sees this dir) can't delete them.
        if flip_to_shared {
            // Compute the stored source_file from the canonicalized path so it matches
            // what `sync`/`import_md_file` derive from walkdir (which canonicalizes).
            // Without this, a symlinked knowledge dir (the dotfiles use case) would make
            // export and sync disagree on the path, breaking the md→DB round-trip.
            let canonical = std::fs::canonicalize(&filepath).unwrap_or_else(|_| filepath.clone());
            let rel_path = canonical
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| canonical.to_string_lossy().to_string());

            let fhash = markdown::file_hash(&filepath)?;
            let now = now_iso();
            for entry in group_entries {
                db::update_entry_to_shared(conn, entry.id, &rel_path, &fhash, &now)?;
            }
        }

        total += group_entries.len();
        println!(
            "  Exported {} entries to {}",
            group_entries.len(),
            filepath.display()
        );
    }

    println!("\nExported {total} entries total.");
    Ok(())
}

pub fn cmd_import(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let root = get_project_root();
    let count = import_md_file(&conn, path, &root)?;
    println!("Imported {count} entries from {}", path.display());
    Ok(())
}
