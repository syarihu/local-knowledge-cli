use crate::db;
use crate::util::{days_since, get_knowledge_dir, get_project_root, truncate_str};

#[allow(clippy::too_many_arguments)]
pub fn cmd_search(
    query: &str,
    keyword_only: bool,
    category: Option<&str>,
    source: Option<&str>,
    status: Option<&str>,
    since: Option<&str>,
    project: Option<&str>,
    limit: usize,
    full: bool,
    scope: Option<&str>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate the status filter so a typo errors loudly instead of silently
    // returning 0 results (matching `add`/`edit`/MCP behaviour).
    if let Some(st) = status
        && !db::is_valid_status(st)
    {
        return Err(format!(
            "Invalid status: {st}. Must be one of: {}",
            db::VALID_STATUSES.join(", ")
        )
        .into());
    }
    // Resolve `--project` once. An unusable value would otherwise filter to nothing
    // and look like "no results", so it errors instead.
    let project_filter = match project {
        Some(p) => Some(super::parse_project_filter(p)?),
        None => None,
    };
    let conns = super::read_connections(scope)?;
    let config = crate::config::Config::load(&get_knowledge_dir());

    // Query each scope's DB and collect. Keywords are fetched on the SAME conn the
    // entry came from (ids are per-DB), tagging each with its scope label.
    let mut items: Vec<(f64, &'static str, db::Entry, Vec<String>)> = Vec::new();
    for (conn, label) in &conns {
        let results = db::search_entries(
            conn,
            query,
            keyword_only,
            category,
            source,
            status,
            since,
            project_filter.as_ref(),
            limit,
        )?;
        for r in results {
            let kws = db::get_keywords(conn, r.id).unwrap_or_default();
            // Entry.rank is 1/(1+|bm25|): smaller value = better match, so we sort
            // ASCENDING to match the per-DB SQL order. Non-ranked rows (keyword/LIKE)
            // have rank=None and sort last, preserving their DB order.
            let score = r.rank.unwrap_or(f64::MAX);
            items.push((score, label, r, kws));
        }
    }
    // The project we are standing in, resolved once (it shells out to git) and only
    // when some hit is attributed at all. Used to order ties below and to badge the
    // hits that came from somewhere else.
    let here = items
        .iter()
        .any(|(_, _, r, _)| r.project.is_some())
        .then(crate::util::current_project_key)
        .flatten();

    // Sort by score ASC (smaller 1/(1+|bm25|) = better match). A tie means bm25 could
    // not tell the two apart — or that neither came from the ranked path at all, since
    // the keyword and LIKE fallbacks carry no score and every row ties. Among equals,
    // knowledge recorded in the repo you are standing in is the better guess; only
    // then does the per-DB order (updated_at DESC) decide.
    items.sort_by(|a, b| {
        let mine = |e: &db::Entry| here.is_some() && e.project.as_deref() == here.as_deref();
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| mine(&b.2).cmp(&mine(&a.2)))
            .then_with(|| b.2.updated_at.cmp(&a.2.updated_at))
    });
    items.truncate(limit);

    let result_count = items.len().to_string();
    super::log_command("search", &[("query", query), ("results", &result_count)]);
    super::warn_if_bare_name_is_ambiguous(&conns, project_filter.as_ref(), json_output);

    if json_output {
        let output: Vec<serde_json::Value> = items
            .iter()
            .map(|(_, label, r, kws)| {
                let days = days_since(&r.updated_at);
                let threshold = config.stale_threshold_for(&r.source);
                let stale = days.map(|d| d >= threshold).unwrap_or(false);
                let mut obj = serde_json::json!({
                    "id": r.id,
                    "uid": r.uid,
                    "title": r.title,
                    "keywords": kws,
                    "category": r.category,
                    "source": r.source,
                    "scope": label,
                    "score": r.rank,
                    "status": r.status,
                    "stale": stale,
                });
                if let Some(ref project) = r.project {
                    obj["project"] = serde_json::json!(project);
                }
                if stale && let Some(d) = days {
                    obj["days_since_update"] = serde_json::json!(d);
                }
                if let Some(ref sb) = r.superseded_by {
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
        if items.is_empty() {
            eprintln!(
                "Hint: No results found. Try fewer keywords, synonyms, or search in both English and Japanese."
            );
        }
    } else if items.is_empty() {
        println!(
            "No results found. Try: use fewer keywords, try synonyms, or search in both English and Japanese."
        );
    } else {
        // Badge a hit whose recorded project is not the one we are standing in. A
        // project-scope hit can carry another repo's project too (via `--project`, or
        // md synced from elsewhere), so the badge follows the value, not the scope.
        for (_, label, r, kws) in &items {
            let snippet = truncate_str(&r.content, 80);
            let days = days_since(&r.updated_at);
            let threshold = config.stale_threshold_for(&r.source);
            let stale = days.map(|d| d >= threshold).unwrap_or(false);
            // User-scope ids collide with project ids; show the uid (a globally
            // unique, copy/pasteable handle for `lk get`/`edit`/…) for user entries.
            let id_disp = if *label == "user" {
                r.uid.clone()
            } else {
                r.id.to_string()
            };
            let project_disp = match r.project.as_deref() {
                Some(p) if Some(p) != here.as_deref() => {
                    format!(" @{}", crate::util::project_repo_name(p))
                }
                _ => String::new(),
            };
            if r.status == "deprecated" {
                print!(
                    "  \u{26a0} [{}] {} ({}){} [DEPRECATED]",
                    id_disp, r.title, r.category, project_disp
                );
            } else if stale {
                print!(
                    "  \u{26a0} [{}] {} ({}){} [STALE: {} days since update]",
                    id_disp,
                    r.title,
                    r.category,
                    project_disp,
                    days.unwrap_or(0)
                );
            } else {
                print!(
                    "  [{}] {} ({}){}",
                    id_disp, r.title, r.category, project_disp
                );
            }
            println!();
            println!("       Keywords: {}", kws.join(", "));
            println!("       {snippet}");
            if let Some(ref sb) = r.superseded_by {
                println!("       \u{2192} Superseded by: {sb}");
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
