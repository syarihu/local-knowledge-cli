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

pub fn cmd_stats(
    json_output: bool,
    verbose: bool,
    scope: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conns = super::read_connections(scope)?;

    let mut total = 0i64;
    let mut shared = 0i64;
    let mut local = 0i64;
    // unique keywords must be a UNION across DBs, not a sum of per-DB DISTINCT counts.
    let mut kw_union: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut per_scope: Vec<serde_json::Value> = Vec::new();
    let mut per_scope_text: Vec<String> = Vec::new();

    for (conn, label) in &conns {
        let s = db::get_stats(conn)?;
        total += s.total;
        shared += s.shared;
        local += s.local;
        for (kw, _) in db::keyword_counts(conn)? {
            kw_union.insert(kw);
        }
        let path = if *label == "user" {
            crate::util::get_user_db_path()
        } else {
            get_db_path()
        };
        per_scope.push(serde_json::json!({
            "scope": label,
            "total_entries": s.total,
            "shared_entries": s.shared,
            "local_entries": s.local,
            "unique_keywords": s.keywords,
            "db_path": path.to_string_lossy(),
            "schema_version": db::get_schema_version_public(conn),
        }));
        per_scope_text.push(format!(
            "  [{}] total={} shared={} local={} keywords={} ({})",
            label,
            s.total,
            s.shared,
            s.local,
            s.keywords,
            path.display()
        ));
    }
    let unique_keywords = kw_union.len();

    if json_output {
        let mut obj = serde_json::json!({
            "total_entries": total,
            "shared_entries": shared,
            "local_entries": local,
            "unique_keywords": unique_keywords,
            "scopes": per_scope,
        });
        if verbose {
            obj["project_root"] = serde_json::json!(get_project_root().to_string_lossy());
        }
        println!("{}", serde_json::to_string(&obj)?);
    } else {
        println!("Knowledge Base Stats:");
        println!("  Total entries:    {total}");
        println!("  Shared entries:   {shared}");
        println!("  Local entries:    {local}");
        println!("  Unique keywords:  {unique_keywords}");
        for line in &per_scope_text {
            println!("{line}");
        }
        if verbose {
            println!("  Project root:     {}", get_project_root().display());
        }
    }
    Ok(())
}
