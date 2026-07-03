use crate::db;
use crate::util::{get_db_path, get_project_root, open_db_with_migrate};

pub fn cmd_keywords(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let rows = db::keyword_counts(&conn)?;

    if json_output {
        let output: Vec<serde_json::Value> = rows
            .iter()
            .map(|(kw, count)| serde_json::json!({"keyword": kw, "count": count}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (kw, count) in &rows {
            println!("  {kw} ({count})");
        }
    }
    Ok(())
}

/// Regenerate per-entry keywords with the ranked/capped extractor.
///
/// Only `local` entries are touched: `shared` entries' keywords are owned by
/// their markdown source files, so rewriting them in the DB would silently
/// diverge from the files (fix the markdown and re-sync instead). By default
/// only "noisy" entries (more keywords than `threshold`) are regenerated, so
/// curated keyword sets are left alone; `--all` regenerates every local entry.
pub fn cmd_keywords_regen(
    all: bool,
    threshold: usize,
    dry_run: bool,
    scope: Option<&str>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conns = super::read_connections(scope)?;
    super::log_command("keywords-regen", &[("dry_run", &dry_run.to_string())]);

    struct RegenChange {
        id: i64,
        uid: String,
        scope: &'static str,
        title: String,
        old_count: usize,
        new_count: usize,
        keywords: Vec<String>,
    }
    let mut changed: Vec<RegenChange> = Vec::new();
    let mut skipped_shared = 0usize;

    for (conn, label) in &conns {
        for entry in db::list_entries(conn, None)? {
            let current = db::get_keywords(conn, entry.id)?;
            let noisy = current.len() > threshold;
            if entry.source != "local" {
                if noisy {
                    skipped_shared += 1;
                }
                continue;
            }
            if !all && !noisy {
                continue;
            }
            let new_kws = crate::keywords::extract_keywords(&entry.title, &entry.content);
            if new_kws == current {
                continue;
            }
            if !dry_run {
                db::replace_keywords(conn, entry.id, &new_kws)?;
            }
            changed.push(RegenChange {
                id: entry.id,
                uid: entry.uid,
                scope: label,
                title: entry.title,
                old_count: current.len(),
                new_count: new_kws.len(),
                keywords: new_kws,
            });
        }
    }

    if json_output {
        let entries: Vec<serde_json::Value> = changed
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "uid": c.uid,
                    "scope": c.scope,
                    "title": c.title,
                    "old_count": c.old_count,
                    "new_count": c.new_count,
                    "keywords": c.keywords,
                })
            })
            .collect();
        let out = serde_json::json!({
            "dry_run": dry_run,
            "regenerated": changed.len(),
            "skipped_shared_noisy": skipped_shared,
            "entries": entries,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let verb = if dry_run {
            "Would regenerate"
        } else {
            "Regenerated"
        };
        println!("{verb} keywords for {} entries:", changed.len());
        for c in &changed {
            println!(
                "  [{}] {} ({} -> {} keywords, {} scope)",
                c.id, c.title, c.old_count, c.new_count, c.scope
            );
        }
        if skipped_shared > 0 {
            println!(
                "Note: {skipped_shared} shared entries have more than {threshold} keywords. \
                 Their keywords come from .knowledge/*.md — add curated keywords to the \
                 markdown frontmatter and run `lk sync` instead."
            );
        }
        if dry_run && !changed.is_empty() {
            println!("(dry run — nothing written; rerun without --dry-run to apply)");
        }
    }
    Ok(())
}

pub fn cmd_stats(
    json_output: bool,
    verbose: bool,
    scope: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conns = super::read_connections(scope)?;

    let mut total = 0i64;
    let mut shared = 0i64;
    let mut local = 0i64;
    // unique keywords must be a UNION across DBs, not a sum of per-DB DISTINCT counts.
    let mut kw_union: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut per_scope: Vec<serde_json::Value> = Vec::new();
    let mut per_scope_text: Vec<String> = Vec::new();

    for (conn, label) in &conns {
        let s = db::get_stats(conn)?;
        total += s.total;
        shared += s.shared;
        local += s.local;
        for (kw, _) in db::keyword_counts(conn)? {
            kw_union.insert(kw);
        }
        let path = if *label == "user" {
            crate::util::get_user_db_path()
        } else {
            get_db_path()
        };
        per_scope.push(serde_json::json!({
            "scope": label,
            "total_entries": s.total,
            "shared_entries": s.shared,
            "local_entries": s.local,
            "unique_keywords": s.keywords,
            "db_path": path.to_string_lossy(),
            "schema_version": db::get_schema_version_public(conn),
        }));
        per_scope_text.push(format!(
            "  [{}] total={} shared={} local={} keywords={} ({})",
            label,
            s.total,
            s.shared,
            s.local,
            s.keywords,
            path.display()
        ));
    }
    let unique_keywords = kw_union.len();

    if json_output {
        let mut obj = serde_json::json!({
            "total_entries": total,
            "shared_entries": shared,
            "local_entries": local,
            "unique_keywords": unique_keywords,
            "scopes": per_scope,
        });
        if verbose {
            obj["project_root"] = serde_json::json!(get_project_root().to_string_lossy());
        }
        println!("{}", serde_json::to_string(&obj)?);
    } else {
        println!("Knowledge Base Stats:");
        println!("  Total entries:    {total}");
        println!("  Shared entries:   {shared}");
        println!("  Local entries:    {local}");
        println!("  Unique keywords:  {unique_keywords}");
        for line in &per_scope_text {
            println!("{line}");
        }
        if verbose {
            println!("  Project root:     {}", get_project_root().display());
        }
    }
    Ok(())
}
