use crate::db;
use crate::util::{STALE_THRESHOLD_DAYS, confirm, days_since, now_iso, open_db_with_migrate};

pub fn cmd_get(id: i64, json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command("get", &[("id", &id.to_string())]);
    let conn = open_db_with_migrate()?;
    let entry = db::get_entry(&conn, id)?.ok_or_else(|| format!("Entry #{id} not found"))?;
    let kws = db::get_keywords(&conn, id)?;

    let days = days_since(&entry.updated_at);
    let stale = days.map(|d| d >= STALE_THRESHOLD_DAYS).unwrap_or(false);

    if json_output {
        let mut out = serde_json::json!({
            "id": entry.id,
            "title": entry.title,
            "content": entry.content,
            "keywords": kws,
            "category": entry.category,
            "source": entry.source,
            "source_file": entry.source_file,
            "status": entry.status,
            "stale": stale,
            "created_at": entry.created_at,
            "updated_at": entry.updated_at,
        });
        if stale && let Some(d) = days {
            out["days_since_update"] = serde_json::json!(d);
        }
        if let Some(sb) = entry.superseded_by {
            out["superseded_by"] = serde_json::json!(sb);
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        if entry.status == "deprecated" {
            println!(
                "\u{26a0} #{} - {} ({}/{}) [DEPRECATED]",
                entry.id, entry.title, entry.category, entry.source
            );
        } else if stale {
            println!(
                "\u{26a0} #{} - {} ({}/{}) [STALE: {} days since update]",
                entry.id,
                entry.title,
                entry.category,
                entry.source,
                days.unwrap_or(0)
            );
        } else {
            println!(
                "#{} - {} ({}/{})",
                entry.id, entry.title, entry.category, entry.source
            );
        }
        println!("Keywords: {}", kws.join(", "));
        if let Some(ref sf) = entry.source_file {
            println!("Source: {sf}");
        }
        if let Some(sb) = entry.superseded_by {
            println!("Superseded by: #{sb}");
        }
        println!("Created: {}", entry.created_at);
        println!("Updated: {}", entry.updated_at);
        println!("\n{}", entry.content);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_edit(
    id: i64,
    title: Option<&str>,
    keywords_str: Option<&str>,
    content: Option<&str>,
    status: Option<&str>,
    superseded_by: Option<i64>,
    touch: bool,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut fields = Vec::new();
    if title.is_some() { fields.push("title"); }
    if keywords_str.is_some() { fields.push("keywords"); }
    if content.is_some() { fields.push("content"); }
    if status.is_some() { fields.push("status"); }
    if superseded_by.is_some() { fields.push("superseded_by"); }
    if touch { fields.push("touch"); }
    super::log_command("edit", &[("id", &id.to_string()), ("fields", &fields.join(","))]);
    let conn = open_db_with_migrate()?;
    let _entry = db::get_entry(&conn, id)?.ok_or_else(|| format!("Entry #{id} not found"))?;

    if title.is_none()
        && keywords_str.is_none()
        && content.is_none()
        && status.is_none()
        && superseded_by.is_none()
        && !touch
    {
        return Err("Nothing to update. Specify --title, --keywords, --content, --status, --superseded-by, or --touch.".into());
    }

    // Validate status
    if let Some(s) = status
        && s != "active"
        && s != "deprecated"
    {
        return Err("Status must be 'active' or 'deprecated'.".into());
    }

    let kws = keywords_str.map(|s| {
        s.split(',')
            .map(|k| k.trim().to_string())
            .collect::<Vec<_>>()
    });

    if touch && title.is_none() && keywords_str.is_none() && content.is_none() {
        // --touch only: just update the timestamp
        conn.execute(
            "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_iso(), id],
        )?;
    } else {
        db::update_entry(&conn, id, title, content, kws.as_deref(), &now_iso())?;
    }

    if status.is_some() || superseded_by.is_some() {
        let current = db::get_entry(&conn, id)?.unwrap();
        // --superseded-by 0 clears the field (sets to None)
        let new_superseded = match superseded_by {
            Some(0) => None,
            Some(v) => Some(v),
            None => current.superseded_by,
        };
        db::update_entry_status(&conn, id, status.unwrap_or(&current.status), new_superseded)?;
    }

    let updated = db::get_entry(&conn, id)?.unwrap();
    let updated_kws = db::get_keywords(&conn, id)?;

    if json_output {
        let mut out = serde_json::json!({
            "id": updated.id,
            "title": updated.title,
            "content": updated.content,
            "keywords": updated_kws,
            "category": updated.category,
            "source": updated.source,
            "status": updated.status,
            "updated_at": updated.updated_at,
        });
        if let Some(sb) = updated.superseded_by {
            out["superseded_by"] = serde_json::json!(sb);
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Updated entry #{id}: {}", updated.title);
        println!("Keywords: {}", updated_kws.join(", "));
        if updated.status == "deprecated" {
            println!("Status: DEPRECATED");
        }
        if let Some(sb) = updated.superseded_by {
            println!("Superseded by: #{sb}");
        }
    }
    Ok(())
}

pub fn cmd_delete(id: i64, yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let entry = db::get_entry(&conn, id)?.ok_or_else(|| format!("Entry #{id} not found"))?;

    if !yes && !confirm(&format!("Delete entry #{id} \"{}\"?", entry.title)) {
        println!("Cancelled.");
        return Ok(());
    }

    db::delete_entry(&conn, id)?;
    println!("Deleted entry #{id}: {}", entry.title);
    Ok(())
}

pub fn cmd_purge(
    category: Option<&str>,
    source: Option<&str>,
    yes: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command("purge", &[
        ("category", category.unwrap_or("")),
        ("source", source.unwrap_or("")),
    ]);
    if category.is_none() && source.is_none() {
        return Err("Specify --category or --source (or both)".into());
    }
    let conn = open_db_with_migrate()?;

    // Count entries that will be affected before confirming
    if !yes {
        let mut desc_parts = Vec::new();
        if let Some(src) = source {
            let entries = db::list_entries_by_source(&conn, src)?;
            desc_parts.push(format!("{} entries with source \"{}\"", entries.len(), src));
        }
        if let Some(cat) = category {
            let entries = db::list_entries(&conn, Some(cat))?;
            desc_parts.push(format!(
                "{} entries with category \"{}\"",
                entries.len(),
                cat
            ));
        }
        let desc = desc_parts.join(" and ");
        if !confirm(&format!("Purge {desc}?")) {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut total = 0;
    if let Some(src) = source {
        let count = db::purge_by_source(&conn, src)?;
        println!("Purged {count} entries with source \"{src}\"");
        total += count;
    }
    if let Some(cat) = category {
        let count = db::delete_entries_by_category(&conn, cat)?;
        println!("Purged {count} entries with category \"{cat}\"");
        total += count;
    }
    if total == 0 {
        println!("No entries matched.");
    }
    Ok(())
}
