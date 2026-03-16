use crate::db;
use crate::util::{get_db_path, get_project_root, open_db_with_migrate};

pub fn cmd_keywords(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let rows = db::keyword_counts(&conn)?;

    if json_output {
        let output: Vec<serde_json::Value> = rows
            .iter()
            .map(|(kw, count)| serde_json::json!({"keyword": kw, "count": count}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (kw, count) in &rows {
            println!("  {kw} ({count})");
        }
    }
    Ok(())
}

pub fn cmd_stats(json_output: bool, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let stats = db::get_stats(&conn)?;

    if json_output {
        let mut obj = serde_json::json!({
            "total_entries": stats.total,
            "shared_entries": stats.shared,
            "local_entries": stats.local,
            "unique_keywords": stats.keywords,
            "db_path": get_db_path().to_string_lossy(),
        });
        if verbose {
            obj["project_root"] = serde_json::json!(get_project_root().to_string_lossy());
            obj["schema_version"] = serde_json::json!(db::get_schema_version_public(&conn));
        }
        println!("{}", serde_json::to_string(&obj)?);
    } else {
        println!("Knowledge Base Stats:");
        println!("  Total entries:    {}", stats.total);
        println!("  Shared entries:   {}", stats.shared);
        println!("  Local entries:    {}", stats.local);
        println!("  Unique keywords:  {}", stats.keywords);
        println!("  DB path:          {}", get_db_path().display());
        if verbose {
            println!("  Project root:     {}", get_project_root().display());
            println!("  Schema version:   {}", db::get_schema_version_public(&conn));
        }
    }
    Ok(())
}
