use crate::db;

pub fn cmd_list(
    category: Option<&str>,
    source: Option<&str>,
    status: Option<&str>,
    limit: Option<usize>,
    offset: usize,
    scope: Option<&str>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate the status filter before any DB work so a typo errors loudly instead
    // of silently returning an empty list (matching add/search/edit/MCP behaviour).
    if let Some(st) = status
        && !db::is_valid_status(st)
    {
        return Err(format!(
            "Invalid status: {st}. Must be one of: {}",
            db::VALID_STATUSES.join(", ")
        )
        .into());
    }
    super::log_command(
        "list",
        &[
            ("category", category.unwrap_or("")),
            ("source", source.unwrap_or("")),
            ("status", status.unwrap_or("")),
        ],
    );
    let conns = super::read_connections(scope)?;

    // Merge entries across scopes, tagging each with its scope label.
    let mut tagged: Vec<(&'static str, db::Entry)> = Vec::new();
    for (conn, label) in &conns {
        let mut entries = db::list_entries(conn, category)?;
        if let Some(src) = source {
            entries.retain(|e| e.source == src);
        }
        if let Some(st) = status {
            entries.retain(|e| e.status == st);
        }
        for e in entries {
            tagged.push((label, e));
        }
    }

    // Re-sort the merged set by updated_at DESC so pagination is globally correct
    // (each DB returns its own updated_at DESC; concatenation alone would not be).
    tagged.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));

    // Apply pagination
    let total = tagged.len();
    if offset > 0 {
        tagged = tagged.into_iter().skip(offset).collect();
    }
    if let Some(lim) = limit {
        tagged.truncate(lim);
    }

    if json_output {
        let output: Vec<serde_json::Value> = tagged
            .iter()
            .map(|(label, e)| {
                let mut obj = serde_json::json!({
                    "id": e.id,
                    "uid": e.uid,
                    "title": e.title,
                    "category": e.category,
                    "source": e.source,
                    "scope": label,
                    "status": e.status,
                    "updated_at": e.updated_at,
                });
                if let Some(ref sb) = e.superseded_by {
                    obj["superseded_by"] = serde_json::json!(sb);
                }
                obj
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if tagged.is_empty() {
        println!("No entries found.");
    } else {
        for (label, e) in &tagged {
            let status_badge = if e.status != "active" {
                format!(" [{}]", e.status.to_uppercase())
            } else {
                String::new()
            };
            // Show the uid (globally unique, copy/pasteable) for user-scope entries.
            let id_disp = if *label == "user" {
                e.uid.clone()
            } else {
                e.id.to_string()
            };
            println!(
                "  [{}] {} ({}/{}){} - {}",
                id_disp, e.title, e.category, e.source, status_badge, e.updated_at
            );
        }
        if limit.is_some() || offset > 0 {
            println!(
                "  ({}-{} of {} entries)",
                offset + 1,
                offset + tagged.len(),
                total
            );
        }
    }
    Ok(())
}
