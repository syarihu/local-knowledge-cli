use crate::db;
use crate::keywords;
use crate::similarity::Tier;
use crate::util::open_db_with_migrate;

/// User-scope ids collide with project ids, so user entries are referenced by
/// their globally unique (and copy/pasteable) uid instead.
fn display_id(entry: &db::Entry, scope: super::Scope) -> String {
    match scope {
        super::Scope::User => entry.uid.clone(),
        super::Scope::Project => entry.id.to_string(),
    }
}

/// Render similar entries for both the block and the warn path. The per-entry
/// shape lives in [`crate::util::similar_entry_json`] so the MCP server reports
/// hits identically.
fn related_json(
    conn: &rusqlite::Connection,
    similar: &[db::SimilarEntry],
    scope: super::Scope,
) -> Vec<serde_json::Value> {
    similar
        .iter()
        .map(|s| crate::util::similar_entry_json(conn, s, scope.label()))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_add(
    title: &str,
    keywords_str: Option<&str>,
    content: Option<&str>,
    category: Option<&str>,
    status: Option<&str>,
    force: bool,
    allow_secrets: bool,
    scope: &str,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate status up front so a bad value fails before any DB work.
    if let Some(st) = status
        && !db::is_valid_status(st)
    {
        return Err(format!(
            "Invalid status: {st}. Must be one of: {}",
            db::VALID_STATUSES.join(", ")
        )
        .into());
    }
    // "auto" (default) saves to project when initialized, else falls back to user.
    let (scope, fell_back) = super::resolve_write_scope(scope)?;
    super::log_command(
        "add",
        &[
            ("title", title),
            ("category", category.unwrap_or("")),
            ("scope", scope.label()),
        ],
    );
    if fell_back && !json_output {
        eprintln!(
            "Note: this project is not initialized; saving to user scope (~/.config/lk/knowledge.db). \
             Run `lk init` for project-scoped knowledge."
        );
    }
    let conn = match scope {
        super::Scope::Project => open_db_with_migrate()?,
        super::Scope::User => crate::util::open_or_create_user_db()?,
    };
    let category = category.unwrap_or("");
    // Apply category template if content is not provided or empty
    let template_content;
    let content = match content {
        Some(c) if !c.is_empty() => c,
        _ => {
            template_content = crate::util::load_category_template(category).unwrap_or_default();
            &template_content
        }
    };

    // Secret detection
    if !allow_secrets {
        let config = crate::config::Config::load(&crate::util::get_knowledge_dir());
        if config.secret_detection {
            let text = format!("{title}\n{content}");
            let matches = crate::secrets::check_for_secrets(&text);
            if !matches.is_empty() {
                if json_output {
                    let warnings: Vec<serde_json::Value> = matches
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "pattern": m.pattern_name,
                                "matched": m.matched,
                            })
                        })
                        .collect();
                    let out = serde_json::json!({
                        "added": false,
                        "secret_detected": true,
                        "warnings": warnings,
                    });
                    println!("{}", serde_json::to_string_pretty(&out)?);
                } else {
                    eprintln!("{}", crate::secrets::format_warning(&matches));
                }
                return Err("secret_detected".into());
            }
        }
    }

    // Manually specified keywords are authoritative; auto-extraction (frequency-
    // ranked, capped) is only the fallback when none are provided. Merging auto
    // keywords into a curated set would drown it in noise.
    let mut kws: Vec<String> = if let Some(ks) = keywords_str {
        ks.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    };
    if kws.is_empty() {
        kws = keywords::extract_keywords(title, content);
    }
    kws.sort_by_key(|a| a.to_lowercase());
    let mut seen = std::collections::HashSet::new();
    kws.retain(|k| seen.insert(k.to_lowercase()));

    // Use BEGIN IMMEDIATE to acquire a write lock before duplicate check,
    // preventing race conditions when multiple processes call `lk add` concurrently.
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = (|| -> Result<(i64, Vec<db::SimilarEntry>), Box<dyn std::error::Error>> {
        // Duplicate check (skip with --force). Only a Block-tier hit refuses the
        // add — a near-identical title. Weaker hits are advisory: they are
        // reported *after* the entry is committed, because refusing on them is
        // what made duplicate detection reject almost every add.
        let similar = if force {
            Vec::new()
        } else {
            db::find_similar_entries(&conn, title, &kws, category)?
        };

        if similar.iter().any(|s| s.tier == Tier::Block) {
            if json_output {
                // Built inside the branch: `related_json` reads keywords and
                // snippets per hit, and the human-readable arm below needs only
                // the keywords, which it reads itself.
                let mut out = serde_json::json!({
                    "added": false,
                    "reason": "duplicate",
                    "scope": scope.label(),
                    "similar_entries": related_json(&conn, &similar, scope),
                });
                if fell_back {
                    out["fell_back_to_user"] = serde_json::json!(true);
                }
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Similar entries found (use --force to add anyway):");
                for s in &similar {
                    println!(
                        "  [{}] {} (keywords: {})",
                        display_id(&s.entry, scope),
                        s.entry.title,
                        db::get_keywords(&conn, s.entry.id)
                            .unwrap_or_default()
                            .join(", ")
                    );
                }
            }
            return Err("duplicate_found".into());
        }

        let id = db::add_entry_full(
            &conn, title, content, &kws, category, "local", None, None, None, status, None, None,
        )?;
        Ok((id, similar))
    })();

    match result {
        Ok((entry_id, similar)) => {
            conn.execute_batch("COMMIT")?;
            let uid = db::get_entry(&conn, entry_id)
                .ok()
                .flatten()
                .map(|e| e.uid)
                .unwrap_or_default();
            // Only the JSON arm of `print_success` reads this, and building it
            // costs a keywords query plus a snippet per hit.
            let related = if json_output {
                related_json(&conn, &similar, scope)
            } else {
                Vec::new()
            };
            print_success(
                entry_id,
                &uid,
                title,
                &kws,
                status.unwrap_or("active"),
                scope,
                fell_back,
                json_output,
                &related,
            );
            if !json_output && !similar.is_empty() {
                println!(
                    "\nNote: possibly related entries (this entry WAS added; update one of them \
                     instead only if it is genuinely the same topic):"
                );
                for s in &similar {
                    println!(
                        "  [{}] {} ({}, title {:.2}, keywords {:.2})",
                        display_id(&s.entry, scope),
                        s.entry.title,
                        s.reason.as_str(),
                        s.title_sim,
                        s.kw_sim
                    );
                }
            }
            Ok(())
        }
        Err(e) if e.to_string() == "duplicate_found" => {
            conn.execute_batch("ROLLBACK")?;
            Ok(())
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            Err(e)
        }
    }
}

/// `possibly_related` is only read when `json_output` is set; callers may pass an
/// empty slice otherwise rather than paying to build it. The human-readable
/// listing of related entries is printed by the caller, which has the scores.
#[allow(clippy::too_many_arguments)]
fn print_success(
    entry_id: i64,
    uid: &str,
    title: &str,
    kws: &[String],
    status: &str,
    scope: super::Scope,
    fell_back: bool,
    json_output: bool,
    possibly_related: &[serde_json::Value],
) {
    if json_output {
        let mut out = serde_json::json!({
            "added": true,
            "id": entry_id,
            "uid": uid,
            "title": title,
            "keywords": kws,
            "status": status,
            "scope": scope.label(),
        });
        if fell_back {
            out["fell_back_to_user"] = serde_json::json!(true);
        }
        // Deliberately a different key from the block path's `similar_entries`:
        // the presence of that key means "not added", and reusing it here would
        // invite exactly the wrong conclusion.
        if !possibly_related.is_empty() {
            out["possibly_related"] = serde_json::json!(possibly_related);
            out["possibly_related_note"] = serde_json::json!(
                "The entry WAS added successfully. These existing entries look related and are \
                 listed for information only. Update one of them ONLY if it covers genuinely the \
                 same topic; otherwise ignore this list."
            );
        }
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        // User-scope ids collide with project ids, so reference user entries by uid.
        match scope {
            super::Scope::User => println!("Added entry {uid}: {title} (user scope)"),
            super::Scope::Project => println!("Added entry #{entry_id}: {title}"),
        }
        println!("Keywords: {}", kws.join(", "));
    }
}
