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

    // For user scope, a custom `--dir` is a one-off dump: `lk sync --scope user`
    // only reads `user_knowledge_dir`, so md written elsewhere can't be synced back.
    if scope == super::Scope::User && dir.is_some() {
        eprintln!(
            "Warning: `lk sync --scope user` won't read a custom --dir; \
             set `user_knowledge_dir` in ~/.config/lk/config.toml to sync this location."
        );
    }

    let output_dir = dir.unwrap_or(default_dir);
    // Only harden a directory we actually create — never clobber the permissions of
    // a pre-existing dir the user manages (e.g. a custom `user_knowledge_dir`).
    let dir_existed = output_dir.exists();
    std::fs::create_dir_all(&output_dir)?;
    let restrict_files = scope == super::Scope::User;
    let root = match scope {
        super::Scope::Project => get_project_root(),
        // Canonicalized parent so export and sync agree on the rel-path even through
        // a symlinked knowledge dir (keeps source_file relative + portable).
        super::Scope::User => {
            if !dir_existed {
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
) -> Result<(), Box<dyn std::error::Error>> {
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

        db::search_entries(conn, q, false, None, Some("local"), None, 100)?
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
