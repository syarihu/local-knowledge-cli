use crate::db;
use crate::util::{confirm, days_since, get_knowledge_dir, now_iso, open_db_with_migrate};

pub fn cmd_get(
    id: &str,
    scope: Option<super::Scope>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command("get", &[("id", id)]);
    let (conn, entry) = super::resolve_target(id, scope)?;
    let config = crate::config::Config::load(&get_knowledge_dir());
    let kws = db::get_keywords(&conn, entry.id)?;

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
            "project": entry.project,
        });
        if stale && let Some(d) = days {
            out["days_since_update"] = serde_json::json!(d);
        }
        if let Some(ref sb) = entry.superseded_by {
            out["superseded_by"] = serde_json::json!(sb);
        }
        if let Some(ref ss) = entry.supersedes {
            out["supersedes"] = serde_json::json!(
                ss.split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            );
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
        if let Some(ref project) = entry.project {
            println!("Project: {project}");
        }
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
            let parts: Vec<String> = ss
                .split(',')
                .map(|uid| {
                    let uid = uid.trim();
                    if let Ok(Some(target)) = db::get_entry_by_uid(&conn, uid) {
                        format!("#{} \"{}\" ({uid})", target.id, target.title)
                    } else {
                        uid.to_string()
                    }
                })
                .collect();
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
    id: &str,
    title: Option<&str>,
    keywords_str: Option<&str>,
    content: Option<&str>,
    status: Option<&str>,
    superseded_by: Option<&str>,
    project: Option<&str>,
    touch: bool,
    scope: Option<super::Scope>,
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
    if project.is_some() {
        fields.push("project");
    }
    if touch {
        fields.push("touch");
    }
    super::log_command("edit", &[("id", id), ("fields", &fields.join(","))]);

    // Validate status before resolving the target / opening the DB, so a bad value
    // errors with the same message as add/search/list/MCP.
    if let Some(s) = status
        && !db::is_valid_status(s)
    {
        return Err(format!(
            "Invalid status: {s}. Must be one of: {}",
            db::VALID_STATUSES.join(", ")
        )
        .into());
    }

    let (conn, entry) = super::resolve_target(id, scope)?;
    let local_id = entry.id;

    if title.is_none()
        && keywords_str.is_none()
        && content.is_none()
        && status.is_none()
        && superseded_by.is_none()
        && project.is_none()
        && !touch
    {
        return Err("Nothing to edit. Specify --title, --keywords, --content, --status, --superseded-by, --project, or --touch.".into());
    }

    // Warn if setting to superseded without --superseded-by
    if status == Some("superseded") && superseded_by.is_none() {
        eprintln!("Warning: Setting status to 'superseded' without --superseded-by.");
    }

    // Resolved before the transaction: an explicit value goes through the same
    // expansion `lk add --project` uses, and an empty one clears the attribution
    // (`--project ""`), which is how a wrong value gets removed rather than replaced.
    let project_update: Option<Option<String>> = match project {
        None => None,
        Some(p) if p.trim().is_empty() => Some(None),
        Some(p) => {
            let (key, note) = crate::util::resolve_project_arg(p);
            if let Some(note) = note
                && !json_output
            {
                eprintln!("Note: {note}");
            }
            Some(key)
        }
    };

    let kws = keywords_str.map(|s| {
        s.split(',')
            .map(|k| k.trim().to_string())
            .collect::<Vec<_>>()
    });

    // Pre-resolve the superseded_by target BEFORE any write, so a bad/cross-scope
    // reference can't leave a partial update behind. Outer Some = perform a status
    // update; inner is the resolved superseded_by uid (None clears it).
    // --superseded-by 0 clears; otherwise resolve <id-or-uid> in the SAME DB.
    let status_update: Option<Option<String>> = if status.is_some() || superseded_by.is_some() {
        let sb: Option<String> = match superseded_by {
            Some("0") => None,
            Some(s) => {
                let target = super::lookup_in_conn(&conn, s)?.ok_or_else(|| {
                    format!("Entry '{s}' not found in the same scope. Cannot set superseded-by.")
                })?;
                Some(target.uid.clone())
            }
            None => entry.superseded_by.clone(),
        };
        Some(sb)
    } else {
        None
    };

    // Apply all writes atomically.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if touch && title.is_none() && keywords_str.is_none() && content.is_none() {
            // --touch only: just update the timestamp
            conn.execute(
                "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now_iso(), local_id],
            )?;
        } else {
            db::update_entry(&conn, local_id, title, content, kws.as_deref(), &now_iso())?;
        }
        if let Some(sb) = &status_update {
            db::update_entry_status(
                &conn,
                local_id,
                status.unwrap_or(entry.status.as_str()),
                sb.as_deref(),
            )?;
        }
        if let Some(p) = &project_update {
            db::update_entry_project(&conn, local_id, p.as_deref())?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            return Err(e);
        }
    }

    let updated = db::get_entry(&conn, local_id)?.unwrap();
    let updated_kws = db::get_keywords(&conn, local_id)?;

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
            out["supersedes"] = serde_json::json!(
                ss.split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            );
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

pub fn cmd_delete(
    id: &str,
    scope: Option<super::Scope>,
    yes: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (conn, entry) = super::resolve_target(id, scope)?;

    // Show both id and uid so the deletion is unambiguous (ids collide across scopes).
    if !yes
        && !confirm(&format!(
            "Delete entry #{} (uid {}) \"{}\"?",
            entry.id, entry.uid, entry.title
        ))
    {
        println!("Cancelled.");
        return Ok(());
    }

    db::delete_entry(&conn, entry.id)?;
    println!(
        "Deleted entry #{} (uid {}): {}",
        entry.id, entry.uid, entry.title
    );
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
    old: &str,
    new: &str,
    scope: Option<super::Scope>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command("supersede", &[("old", old), ("new", new)]);

    // Both targets are resolved in a SINGLE connection (same scope) so the two
    // updates stay in one transaction; cross-scope supersede is unsupported.
    let (conn, old_entry, new_entry) = super::resolve_supersede_pair(old, new, scope)?;
    if old_entry.id == new_entry.id {
        return Err("old and new must be different entries.".into());
    }
    let old_id = old_entry.id;
    let new_id = new_entry.id;

    // Atomic: both updates in a transaction
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        db::update_entry_status(&conn, old_id, "superseded", Some(&new_entry.uid))?;
        let new_supersedes = db::append_supersedes(new_entry.supersedes.as_deref(), &old_entry.uid);
        db::update_entry_supersedes(&conn, new_id, Some(&new_supersedes))?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            return Err(e);
        }
    }

    let new_supersedes = db::append_supersedes(new_entry.supersedes.as_deref(), &old_entry.uid);

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
