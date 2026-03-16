use std::path::PathBuf;

use crate::db;
use crate::markdown;
use crate::util::{get_knowledge_dir, get_project_root, now_iso, open_db_with_migrate};
use crate::cmd::sync::import_md_file;

pub fn cmd_export(dir: Option<PathBuf>, ids: Option<&str>, query: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let output_dir = dir.unwrap_or_else(get_knowledge_dir);
    std::fs::create_dir_all(&output_dir)?;
    let root = get_project_root();

    let entries = if let Some(ids_str) = ids {
        // Export specific entries by ID
        let mut selected = Vec::new();
        for id_str in ids_str.split(',') {
            let id: i64 = id_str.trim().parse()
                .map_err(|_| format!("Invalid ID: {}", id_str.trim()))?;
            match db::get_entry(&conn, id)? {
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
        let results = db::search_entries(&conn, q, false, None, Some("local"), None, 100)?;
        results
    } else {
        // Export all local entries
        db::list_entries_by_source(&conn, "local")?
    };

    if entries.is_empty() {
        println!("No local entries to export.");
        return Ok(());
    }

    // Group by first keyword
    let mut groups: std::collections::HashMap<String, Vec<db::Entry>> =
        std::collections::HashMap::new();
    for entry in entries {
        let kws = db::get_keywords(&conn, entry.id)?;
        let group = kws.first().cloned().unwrap_or_else(|| "general".to_string());
        groups.entry(group).or_default().push(entry);
    }

    let mut total = 0;
    for (group_name, group_entries) in &groups {
        let filename = format!("exported-{group_name}.md");
        let filepath = output_dir.join(&filename);
        let rel_path = filepath
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| filepath.to_string_lossy().to_string());

        let mut all_kws: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in group_entries {
            let kws = db::get_keywords(&conn, entry.id)?;
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

        for entry in group_entries {
            let kws = db::get_keywords(&conn, entry.id)?;
            lines.push(format!("## Entry: {}", entry.title));
            lines.push(format!("keywords: [{}]\n", kws.join(", ")));
            lines.push(entry.content.clone());
            lines.push(String::new());
        }

        std::fs::write(&filepath, lines.join("\n"))?;

        let fhash = markdown::file_hash(&filepath)?;
        let now = now_iso();
        for entry in group_entries {
            db::update_entry_to_shared(&conn, entry.id, &rel_path, &fhash, &now)?;
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
