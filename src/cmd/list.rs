use crate::db;
use crate::util::open_db_with_migrate;

pub fn cmd_list(
    category: Option<&str>,
    source: Option<&str>,
    limit: Option<usize>,
    offset: usize,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let mut entries = db::list_entries(&conn, category)?;
    if let Some(src) = source {
        entries.retain(|e| e.source == src);
    }

    // Apply pagination
    let total = entries.len();
    if offset > 0 {
        entries = entries.into_iter().skip(offset).collect();
    }
    if let Some(lim) = limit {
        entries.truncate(lim);
    }

    if json_output {
        let output: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                let mut obj = serde_json::json!({
                    "id": e.id,
                    "title": e.title,
                    "category": e.category,
                    "source": e.source,
                    "status": e.status,
                    "updated_at": e.updated_at,
                });
                if let Some(sb) = e.superseded_by {
                    obj["superseded_by"] = serde_json::json!(sb);
                }
                obj
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if entries.is_empty() {
        println!("No entries found.");
    } else {
        for e in &entries {
            println!(
                "  [{}] {} ({}/{}) - {}",
                e.id, e.title, e.category, e.source, e.updated_at
            );
        }
        if limit.is_some() || offset > 0 {
            println!("  ({}-{} of {} entries)", offset + 1, offset + entries.len(), total);
        }
    }
    Ok(())
}
