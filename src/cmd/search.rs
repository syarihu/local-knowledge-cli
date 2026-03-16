use crate::db;
use crate::util::{
    days_since, get_project_root, now_iso, open_db_with_migrate, truncate_str,
    STALE_THRESHOLD_DAYS,
};

pub fn cmd_search(
    query: &str,
    keyword_only: bool,
    category: Option<&str>,
    source: Option<&str>,
    since: Option<&str>,
    limit: usize,
    full: bool,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let results = db::search_entries(&conn, query, keyword_only, category, source, since, limit)?;

    log_search(query, &results);

    if json_output {
        let output: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let kws = db::get_keywords(&conn, r.id).unwrap_or_default();
                let days = days_since(&r.updated_at);
                let stale = days.map(|d| d >= STALE_THRESHOLD_DAYS).unwrap_or(false);
                let mut obj = serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "keywords": kws,
                    "category": r.category,
                    "source": r.source,
                    "score": r.rank,
                    "status": r.status,
                    "stale": stale,
                });
                if stale {
                    if let Some(d) = days {
                        obj["days_since_update"] = serde_json::json!(d);
                    }
                }
                if let Some(sb) = r.superseded_by {
                    obj["superseded_by"] = serde_json::json!(sb);
                }
                if full {
                    obj["content"] = serde_json::Value::String(r.content.clone());
                } else {
                    obj["snippet"] = serde_json::Value::String(truncate_str(&r.content, 300));
                }
                obj
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if results.is_empty() {
        println!("No results found.");
    } else {
        for r in &results {
            let snippet = truncate_str(&r.content, 80);
            let kws = db::get_keywords(&conn, r.id).unwrap_or_default();
            let days = days_since(&r.updated_at);
            let stale = days.map(|d| d >= STALE_THRESHOLD_DAYS).unwrap_or(false);
            if r.status == "deprecated" {
                print!("  \u{26a0} [{}] {} ({}) [DEPRECATED]", r.id, r.title, r.category);
            } else if stale {
                print!("  \u{26a0} [{}] {} ({}) [STALE: {} days since update]", r.id, r.title, r.category, days.unwrap_or(0));
            } else {
                print!("  [{}] {} ({})", r.id, r.title, r.category);
            }
            println!();
            println!("       Keywords: {}", kws.join(", "));
            println!("       {snippet}");
            if let Some(sb) = r.superseded_by {
                println!("       \u{2192} Superseded by: #{sb}");
            }
            println!();
        }
    }
    Ok(())
}

fn log_search(query: &str, results: &[db::Entry]) {
    if std::env::var("LK_SEARCH_LOG").unwrap_or_default() != "1" {
        return;
    }
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let log_path = get_project_root().join(".knowledge").join("search.log");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let titles: Vec<&str> = results.iter().take(5).map(|r| r.title.as_str()).collect();
        writeln!(
            f,
            "[{}] query=\"{}\" results={} titles={:?}",
            now_iso(),
            query,
            results.len(),
            titles,
        )?;
        Ok(())
    })();
}

pub fn cmd_search_log(lines: usize) -> Result<(), Box<dyn std::error::Error>> {
    let log_path = get_project_root().join(".knowledge").join("search.log");
    if !log_path.exists() {
        println!("No search log found.");
        return Ok(());
    }
    let content = std::fs::read_to_string(&log_path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    for line in &all_lines[start..] {
        println!("{line}");
    }
    Ok(())
}
