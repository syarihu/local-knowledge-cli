use crate::db;
use crate::keywords;
use crate::util::{open_db_with_migrate, truncate_str};

pub fn cmd_add(
    title: &str,
    keywords_str: Option<&str>,
    content: Option<&str>,
    category: Option<&str>,
    force: bool,
    allow_secrets: bool,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    super::log_command(
        "add",
        &[("title", title), ("category", category.unwrap_or(""))],
    );
    let conn = open_db_with_migrate()?;
    let content = content.unwrap_or("");
    let category = category.unwrap_or("");

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
                if json_output {
                    let similar_json: Vec<serde_json::Value> = similar
                        .iter()
                        .map(|e| {
                            let ekws = db::get_keywords(&conn, e.id).unwrap_or_default();
                            let snippet = truncate_str(&e.content, 300);
                            serde_json::json!({
                                "id": e.id,
                                "title": e.title,
                                "keywords": ekws,
                                "snippet": snippet,
                            })
                        })
                        .collect();
                    let out = serde_json::json!({
                        "added": false,
                        "similar_entries": similar_json,
                    });
                    println!("{}", serde_json::to_string_pretty(&out)?);
                } else {
                    println!("Similar entries found (use --force to add anyway):");
                    for e in &similar {
                        let ekws = db::get_keywords(&conn, e.id).unwrap_or_default();
                        println!("  [{}] {} (keywords: {})", e.id, e.title, ekws.join(", "));
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
            print_success(entry_id, title, &kws, json_output);
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

fn print_success(entry_id: i64, title: &str, kws: &[String], json_output: bool) {
    if json_output {
        let out = serde_json::json!({
            "added": true,
            "id": entry_id,
            "title": title,
            "keywords": kws,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        println!("Added entry #{entry_id}: {title}");
        println!("Keywords: {}", kws.join(", "));
    }
}
