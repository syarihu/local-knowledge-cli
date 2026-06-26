use crate::db;
use crate::keywords;
use crate::util::{open_db_with_migrate, truncate_str};

#[allow(clippy::too_many_arguments)]
pub fn cmd_add(
    title: &str,
    keywords_str: Option<&str>,
    content: Option<&str>,
    category: Option<&str>,
    force: bool,
    allow_secrets: bool,
    scope: &str,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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

    let mut kws: Vec<String> = if let Some(ks) = keywords_str {
        ks.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    };

    // Auto-extract additional keywords
    let auto_kws = keywords::extract_keywords(title, content);
    for kw in auto_kws {
        let lower = kw.to_lowercase();
        if !kws.iter().any(|k| k.to_lowercase() == lower) {
            kws.push(kw);
        }
    }
    kws.sort_by_key(|a| a.to_lowercase());

    // Use BEGIN IMMEDIATE to acquire a write lock before duplicate check,
    // preventing race conditions when multiple processes call `lk add` concurrently.
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = (|| -> Result<i64, Box<dyn std::error::Error>> {
        // Duplicate check (skip with --force)
        if !force {
            let similar = db::find_similar_entries(&conn, title, &kws)?;
            if !similar.is_empty() {
                let scope_label = scope.label();
                if json_output {
                    let similar_json: Vec<serde_json::Value> = similar
                        .iter()
                        .map(|e| {
                            let ekws = db::get_keywords(&conn, e.id).unwrap_or_default();
                            let snippet = truncate_str(&e.content, 300);
                            serde_json::json!({
                                "id": e.id,
                                "uid": e.uid,
                                "scope": scope_label,
                                "title": e.title,
                                "keywords": ekws,
                                "snippet": snippet,
                            })
                        })
                        .collect();
                    let mut out = serde_json::json!({
                        "added": false,
                        "scope": scope_label,
                        "similar_entries": similar_json,
                    });
                    if fell_back {
                        out["fell_back_to_user"] = serde_json::json!(true);
                    }
                    println!("{}", serde_json::to_string_pretty(&out)?);
                } else {
                    println!("Similar entries found (use --force to add anyway):");
                    for e in &similar {
                        let ekws = db::get_keywords(&conn, e.id).unwrap_or_default();
                        // user-scope ids collide with project ids, so show scope+uid.
                        let id_disp = if scope == super::Scope::User {
                            format!("user:{}", e.uid)
                        } else {
                            e.id.to_string()
                        };
                        println!(
                            "  [{}] {} (keywords: {})",
                            id_disp,
                            e.title,
                            ekws.join(", ")
                        );
                    }
                }
                return Err("duplicate_found".into());
            }
        }

        db::add_entry(&conn, title, content, &kws, category, "local", None, None)
    })();

    match result {
        Ok(entry_id) => {
            conn.execute_batch("COMMIT")?;
            let uid = db::get_entry(&conn, entry_id)
                .ok()
                .flatten()
                .map(|e| e.uid)
                .unwrap_or_default();
            print_success(entry_id, &uid, title, &kws, scope, fell_back, json_output);
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

fn print_success(
    entry_id: i64,
    uid: &str,
    title: &str,
    kws: &[String],
    scope: super::Scope,
    fell_back: bool,
    json_output: bool,
) {
    if json_output {
        let mut out = serde_json::json!({
            "added": true,
            "id": entry_id,
            "uid": uid,
            "title": title,
            "keywords": kws,
            "scope": scope.label(),
        });
        if fell_back {
            out["fell_back_to_user"] = serde_json::json!(true);
        }
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        // User-scope ids collide with project ids, so reference user entries by uid.
        match scope {
            super::Scope::User => println!("Added entry user:{uid}: {title} (user scope)"),
            super::Scope::Project => println!("Added entry #{entry_id}: {title}"),
        }
        println!("Keywords: {}", kws.join(", "));
    }
}
