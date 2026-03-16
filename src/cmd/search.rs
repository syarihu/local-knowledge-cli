use crate::db;
use crate::util::{days_since, get_knowledge_dir, get_project_root, open_db_with_migrate, truncate_str};

#[allow(clippy::too_many_arguments)]
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
    let config = crate::config::Config::load(&get_knowledge_dir());
    let results = db::search_entries(&conn, query, keyword_only, category, source, since, limit)?;

    let result_count = results.len().to_string();
    super::log_command("search", &[("query", query), ("results", &result_count)]);

    if json_output {
        let output: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let kws = db::get_keywords(&conn, r.id).unwrap_or_default();
                let days = days_since(&r.updated_at);
                let threshold = config.stale_threshold_for(&r.source);
                let stale = days.map(|d| d >= threshold).unwrap_or(false);
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
                if stale && let Some(d) = days {
                    obj["days_since_update"] = serde_json::json!(d);
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
            let threshold = config.stale_threshold_for(&r.source);
            let stale = days.map(|d| d >= threshold).unwrap_or(false);
            if r.status == "deprecated" {
                print!(
                    "  \u{26a0} [{}] {} ({}) [DEPRECATED]",
                    r.id, r.title, r.category
                );
            } else if stale {
                print!(
                    "  \u{26a0} [{}] {} ({}) [STALE: {} days since update]",
                    r.id,
                    r.title,
                    r.category,
                    days.unwrap_or(0)
                );
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

pub fn cmd_command_log(lines: usize) -> Result<(), Box<dyn std::error::Error>> {
    let log_path = get_project_root().join(".knowledge").join("command.log");
    if !log_path.exists() {
        println!("No command log found. Set LK_COMMAND_LOG=1 to enable.");
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
