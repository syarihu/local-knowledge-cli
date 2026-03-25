use crate::db;
use crate::util::{confirm, days_since, get_knowledge_dir, now_iso, open_db_with_migrate};

pub fn cmd_get(id: i64, json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command("get", &[("id", &id.to_string())]);
    let conn = open_db_with_migrate()?;
    let config = crate::config::Config::load(&get_knowledge_dir());
    let entry = db::get_entry(&conn, id)?.ok_or_else(|| format!("Entry #{id} not found"))?;
    let kws = db::get_keywords(&conn, id)?;

    let days = days_since(&entry.updated_at);
    let threshold = config.stale_threshold_for(&entry.source);
    let stale = days.map(|d| d >= threshold).unwrap_or(false);

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
            "uid": entry.uid,
            "stale": stale,
            "created_at": entry.created_at,
            "updated_at": entry.updated_at,
        });
        if stale && let Some(d) = days {
            out["days_since_update"] = serde_json::json!(d);
        }
        if let Some(ref sb) = entry.superseded_by {
            out["superseded_by"] = serde_json::json!(sb);
        }
        if let Some(ref ss) = entry.supersedes {
            out["supersedes"] = serde_json::json!(ss.split(',').collect::<Vec<_>>());
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let badge = match entry.status.as_str() {
            "active" => None,
            other => Some(format!("[{}]", other.to_uppercase())),
        };
        if let Some(ref badge) = badge {
            println!(
                "\u{26a0} #{} - {} ({}/{}) {badge}",
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
        println!("UID: {}", entry.uid);
        println!("Keywords: {}", kws.join(", "));
        if let Some(ref sf) = entry.source_file {
            println!("Source: {sf}");
        }
        if let Some(ref sb) = entry.superseded_by {
            // Try to resolve UID to entry title
            if let Ok(Some(target)) = db::get_entry_by_uid(&conn, sb) {
                println!("Superseded by: #{} \"{}\" ({sb})", target.id, target.title);
            } else {
                println!("Superseded by: {sb}");
            }
        }
        if let Some(ref ss) = entry.supersedes {
            let parts: Vec<String> = ss.split(',').map(|uid| {
                let uid = uid.trim();
                if let Ok(Some(target)) = db::get_entry_by_uid(&conn, uid) {
                    format!("#{} \"{}\" ({uid})", target.id, target.title)
                } else {
                    uid.to_string()
                }
            }).collect();
            println!("Supersedes: {}", parts.join(", "));
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
    if title.is_some() {
        fields.push("title");
    }
    if keywords_str.is_some() {
        fields.push("keywords");
    }
    if content.is_some() {
        fields.push("content");
    }
    if status.is_some() {
        fields.push("status");
    }
    if superseded_by.is_some() {
        fields.push("superseded_by");
    }
    if touch {
        fields.push("touch");
    }
    super::log_command(
        "edit",
        &[("id", &id.to_string()), ("fields", &fields.join(","))],
    );
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
        && !db::is_valid_status(s)
    {
        return Err(format!("Status must be one of: {}", db::VALID_STATUSES.join(", ")).into());
    }

    // Warn if setting to superseded without --superseded-by
    if status == Some("superseded") && superseded_by.is_none() {
        eprintln!("Warning: Setting status to 'superseded' without --superseded-by.");
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
        let new_superseded: Option<String> = match superseded_by {
            Some(0) => None,
            Some(v) => {
                let target = db::get_entry(&conn, v)?.ok_or_else(|| {
                    format!("Entry #{v} not found. Cannot set superseded-by to a non-existent entry.")
                })?;
                Some(target.uid.clone())
            }
            None => current.superseded_by.clone(),
        };
        db::update_entry_status(&conn, id, status.unwrap_or(&current.status), new_superseded.as_deref())?;
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
            "uid": updated.uid,
            "updated_at": updated.updated_at,
        });
        if let Some(ref sb) = updated.superseded_by {
            out["superseded_by"] = serde_json::json!(sb);
        }
        if let Some(ref ss) = updated.supersedes {
            out["supersedes"] = serde_json::json!(ss.split(',').collect::<Vec<_>>());
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Updated entry #{id}: {}", updated.title);
        println!("Keywords: {}", updated_kws.join(", "));
        if updated.status != "active" {
            println!("Status: {}", updated.status.to_uppercase());
        }
        if let Some(ref sb) = updated.superseded_by {
            println!("Superseded by: {sb}");
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
    super::log_command(
        "purge",
        &[
            ("category", category.unwrap_or("")),
            ("source", source.unwrap_or("")),
        ],
    );
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

pub fn cmd_supersede(
    old_id: i64,
    new_id: i64,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command(
        "supersede",
        &[
            ("old_id", &old_id.to_string()),
            ("new_id", &new_id.to_string()),
        ],
    );
    let conn = open_db_with_migrate()?;

    let old_entry =
        db::get_entry(&conn, old_id)?.ok_or_else(|| format!("Entry #{old_id} not found"))?;
    let new_entry =
        db::get_entry(&conn, new_id)?.ok_or_else(|| format!("Entry #{new_id} not found"))?;

    // Set old entry: status=superseded, superseded_by=new_entry.uid
    db::update_entry_status(&conn, old_id, "superseded", Some(&new_entry.uid))?;

    // Set new entry: supersedes includes old_entry.uid
    let new_supersedes = db::append_supersedes(new_entry.supersedes.as_deref(), &old_entry.uid);
    db::update_entry_supersedes(&conn, new_id, Some(&new_supersedes))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "old_id": old_id,
                "old_uid": old_entry.uid,
                "new_id": new_id,
                "new_uid": new_entry.uid,
                "old_status": "superseded",
                "old_superseded_by": new_entry.uid,
                "new_supersedes": new_supersedes,
            }))?
        );
    } else {
        println!(
            "Entry #{old_id} \"{}\" is now superseded by #{new_id} \"{}\"",
            old_entry.title, new_entry.title
        );
    }
    Ok(())
}
