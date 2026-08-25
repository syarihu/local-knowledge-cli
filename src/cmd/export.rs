use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cmd::sync::import_md_file;
use crate::db;
use crate::markdown;
use crate::util::{get_knowledge_dir, get_project_root, now_iso, open_db_with_migrate};

/// The base name for a group, as a single safe path segment (no `exported-` prefix,
/// no extension, no digest).
///
/// A group is a keyword, and keywords are written freely: `feature/auth` turned into
/// a path that no directory existed for (so `export` failed outright), and
/// `x/../../README` resolved outside the store and overwrote whatever was there.
/// Characters a file name cannot carry fold to `-`.
fn export_base_name(group: &str) -> String {
    const UNSAFE: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    // Bidirectional controls are not `char::is_control`, and a file name that renders
    // differently from its bytes is a name nobody can act on.
    const BIDI: &[char] = &[
        '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
        '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ];
    let folded: String = group
        .chars()
        .map(|c| {
            if c.is_control() || UNSAFE.contains(&c) || BIDI.contains(&c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    // A leading dot hides the file (and `.` / `..` are not names at all); Windows
    // drops trailing dots and spaces.
    let trimmed = folded.trim_matches(|c: char| c == '.' || c.is_whitespace());

    // File systems cap a path component at 255 bytes, and a keyword is free text: a
    // 300-character one failed the export with ENAMETOOLONG, the same way an
    // unflattened separator did. Cut on a character boundary, well inside the cap.
    const MAX_NAME_BYTES: usize = 120;
    let mut end = trimmed.len().min(MAX_NAME_BYTES);
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let capped = trimmed[..end].trim_matches(|c: char| c == '.' || c == '-' || c.is_whitespace());
    if capped.is_empty() {
        "general".to_string()
    } else {
        capped.to_string()
    }
}

/// Length of the hex digest appended to disambiguate a name.
const DIGEST_HEX: usize = 16;

/// A digest of the group itself, so a disambiguated name depends on nothing else and
/// stays stable across runs.
fn group_digest(group: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(group.as_bytes())[..DIGEST_HEX / 2])
}

/// Whether a base name already looks like it carries a digest, in which case an
/// undisambiguated name would sit in the same namespace as a disambiguated one.
fn looks_disambiguated(base: &str) -> bool {
    base.rsplit_once('-').is_some_and(|(_, tail)| {
        tail.len() == DIGEST_HEX
            && tail
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    })
}

/// How a file system that ignores case and normalization would see a name. Two
/// groups whose names differ only that way would land on one file, and the second
/// write would take the first group's entries with it.
fn file_system_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfc().collect::<String>().to_lowercase()
}

/// The output file name for every group, with collisions resolved before anything is
/// written.
///
/// A name carries a digest of its keyword when the keyword had to be changed to
/// become a file name — otherwise `feature/auth` and a literal `feature-auth` would
/// share a file and overwrite each other's entries. Names that need no change keep
/// the one they have always had, so existing stores see no churn and `sync` keeps
/// matching entries by their stored `source_file`.
fn export_file_names<'a>(groups: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    let planned: Vec<(String, String, bool)> = groups
        .map(|group| {
            let base = export_base_name(group);
            let needs = base != group || looks_disambiguated(&base);
            (group.to_string(), base, needs)
        })
        .collect();

    let name_of = |base: &str, digested: bool, group: &str| -> String {
        if digested {
            format!("exported-{base}-{}.md", group_digest(group))
        } else {
            format!("exported-{base}.md")
        }
    };

    // Anything two groups would share on a case- or normalization-insensitive file
    // system gets a digest on both sides, which the group's own bytes decide.
    let mut clashing: HashSet<String> = HashSet::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    for (group, base, needs) in &planned {
        let key = file_system_key(&name_of(base, *needs, group));
        match seen.get(&key) {
            Some(first) => {
                clashing.insert(first.clone());
                clashing.insert(group.clone());
            }
            None => {
                seen.insert(key, group.clone());
            }
        }
    }

    planned
        .into_iter()
        .map(|(group, base, needs)| {
            let digested = needs || clashing.contains(&group);
            let name = name_of(&base, digested, &group);
            (group, name)
        })
        .collect()
}

pub fn cmd_export(
    dir: Option<PathBuf>,
    ids: Option<&str>,
    query: Option<&str>,
    allow_secrets: bool,
    scope: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let scope = super::parse_scope(scope)?;

    // Resolve the (connection, default output dir, root for rel-path, secret config)
    // per scope. Project keeps its historical root (the project root) so stored
    // source_file paths are unchanged; user scope derives its root from the
    // canonicalized knowledge dir (see `root` below).
    let (conn, default_dir, secret_detection) = match scope {
        super::Scope::Project => (
            open_db_with_migrate()?,
            get_knowledge_dir(),
            crate::config::Config::load(&get_knowledge_dir()).secret_detection,
        ),
        super::Scope::User => {
            if let Some(path) = crate::util::ensure_global_config_scaffold() {
                println!(
                    "Created {} (edit to customize user_knowledge_dir).",
                    path.display()
                );
            }
            (
                crate::util::open_or_create_user_db()?,
                crate::util::get_user_knowledge_dir(),
                crate::config::GlobalConfig::load().secret_detection,
            )
        }
    };

    // The managed store for this scope (project `.knowledge` or user `user_knowledge_dir`).
    let managed_dir = default_dir.clone();
    let output_dir = dir.unwrap_or(default_dir);
    // Only harden a directory we actually create — never clobber the permissions of
    // a pre-existing dir the user manages (e.g. a custom `user_knowledge_dir`).
    let dir_existed = output_dir.exists();
    std::fs::create_dir_all(&output_dir)?;

    // An export to a dir OTHER than the scope's managed store is a one-off DUMP, not a
    // managed export: `lk sync` only reads the managed dir (project `.knowledge/` or
    // user `user_knowledge_dir`), so if we flipped these entries to `shared` with a
    // source_file pointing at the custom dir, the next sync would treat those files as
    // "missing" and DELETE the entries (data loss). So dump-only: write the md but leave
    // entries `local`. When the dir equals the managed store (compared canonically), it's
    // a normal managed export.
    let dump_only = !crate::util::paths_equivalent(&output_dir, &managed_dir);
    if dump_only {
        let hint = match scope {
            super::Scope::Project => {
                "export without `--dir` (into the project's .knowledge/) to manage a synced store."
            }
            super::Scope::User => {
                "set `user_knowledge_dir` in ~/.config/lk/config.toml to manage a synced store."
            }
        };
        eprintln!(
            "Warning: `--dir` is a one-off dump; entries stay local and are NOT synced. {hint}"
        );
    }

    let restrict_files = scope == super::Scope::User;
    let root = match scope {
        super::Scope::Project => get_project_root(),
        // Canonicalized parent so export and sync agree on the rel-path even through
        // a symlinked knowledge dir (keeps source_file relative + portable).
        super::Scope::User => {
            if dir_existed {
                // We won't clobber an existing dir's mode, but a group/world-readable
                // store leaks filenames even though the md files are 0600 — so warn.
                crate::util::warn_if_not_owner_only(&output_dir);
            } else {
                crate::util::restrict_to_owner(&output_dir, true);
            }
            crate::util::user_md_root(&output_dir)
        }
    };

    export_to_dir(
        &conn,
        &output_dir,
        &root,
        ids,
        query,
        allow_secrets,
        secret_detection,
        restrict_files,
        !dump_only,
    )
}

#[allow(clippy::too_many_arguments)]
fn export_to_dir(
    conn: &rusqlite::Connection,
    output_dir: &std::path::Path,
    root: &std::path::Path,
    ids: Option<&str>,
    query: Option<&str>,
    allow_secrets: bool,
    secret_detection: bool,
    restrict_files: bool,
    flip_to_shared: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Canonical rel-path root: the stored source_file is computed from the canonicalized
    // written file, so the root it's stripped against must be canonical too (else a
    // symlinked project/knowledge path yields an unstable absolute source_file). Matches
    // what sync derives from walkdir, keeping the md→DB round-trip stable.
    let canonical_root = crate::util::canonicalize_or(root);
    let root = canonical_root.as_path();

    let entries = if let Some(ids_str) = ids {
        // Export specific entries by ID
        let mut selected = Vec::new();
        for id_str in ids_str.split(',') {
            let id: i64 = id_str
                .trim()
                .parse()
                .map_err(|_| format!("Invalid ID: {}", id_str.trim()))?;
            match db::get_entry(conn, id)? {
                Some(entry) => {
                    if entry.source != "local" {
                        eprintln!("Warning: Entry #{id} is already shared, skipping.");
                    } else {
                        selected.push(entry);
                    }
                }
                None => {
                    return Err(format!("Entry #{id} not found").into());
                }
            }
        }
        selected
    } else if let Some(q) = query {
        // Export entries matching a search query

        db::search_entries(
            conn,
            q,
            false,
            None,
            Some("local"),
            None,
            None,
            None,
            None,
            100,
        )?
    } else {
        // Export all local entries
        db::list_entries_by_source(conn, "local")?
    };

    if entries.is_empty() {
        println!("No local entries to export.");
        return Ok(());
    }

    // Secret detection before export
    if !allow_secrets && secret_detection {
        let mut all_matches = Vec::new();
        for entry in &entries {
            let text = format!("{}\n{}", entry.title, entry.content);
            let matches = crate::secrets::check_for_secrets(&text);
            for m in matches {
                all_matches.push((entry.id, entry.title.clone(), m));
            }
        }
        if !all_matches.is_empty() {
            eprintln!("Potential secrets detected in entries to export:");
            for (id, title, m) in &all_matches {
                eprintln!(
                    "  Entry #{id} \"{title}\": {} ({})",
                    m.pattern_name, m.matched
                );
            }
            eprintln!("\nUse --allow-secrets to override this check.");
            return Err("secret_detected".into());
        }
    }

    // Group by first keyword — use BTreeMap for stable alphabetical order
    let mut groups: std::collections::BTreeMap<String, Vec<db::Entry>> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let kws = db::get_keywords(conn, entry.id)?;
        let group = kws
            .first()
            .cloned()
            .unwrap_or_else(|| "general".to_string());
        groups.entry(group).or_default().push(entry);
    }

    // Resolved for every group before the first write, so a collision can be given a
    // distinct name rather than discovered as a truncated file.
    let file_names = export_file_names(groups.keys().map(String::as_str));

    let mut total = 0;
    for (group_name, group_entries) in &groups {
        // Sort entries within each group by title for stable output
        let mut sorted_entries: Vec<&db::Entry> = group_entries.iter().collect();
        sorted_entries.sort_by_key(|e| e.title.to_lowercase());

        let filename = file_names
            .get(group_name)
            .cloned()
            .unwrap_or_else(|| format!("exported-{}.md", export_base_name(group_name)));
        // The name is one segment by construction; this keeps that an invariant
        // rather than a comment, so a future change to the naming can't quietly
        // reintroduce a path.
        if Path::new(&filename).components().count() != 1 {
            return Err(format!("refusing to export to a non-file name: {filename:?}").into());
        }
        let filepath = output_dir.join(&filename);
        // A symlink here would be followed, and `.knowledge/` travels with the repo —
        // a link committed as `exported-auth.md -> ../README.md` would let an export
        // truncate a file outside the store. The store's directory may well be a
        // symlink (that is how a dotfiles setup works); an individual entry file being
        // one is not something to write through.
        if std::fs::symlink_metadata(&filepath).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!(
                "{} is a symlink; refusing to write through it. Remove it and export again.",
                filepath.display()
            )
            .into());
        }

        let mut all_kws: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in &sorted_entries {
            let kws = db::get_keywords(conn, entry.id)?;
            all_kws.extend(kws);
        }

        let mut lines = Vec::new();
        lines.push("---".to_string());
        lines.push(format!(
            "keywords: [{}]",
            all_kws.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        lines.push("category: exported".to_string());
        lines.push("---\n".to_string());
        lines.push(format!("# Exported: {group_name}\n"));

        for entry in &sorted_entries {
            let kws = db::get_keywords(conn, entry.id)?;
            lines.push(format!("## Entry: {}", entry.title));
            lines.push(format!("keywords: [{}]", kws.join(", ")));
            if !entry.uid.is_empty() {
                lines.push(format!("uid: {}", entry.uid));
            }
            if entry.status != "active" {
                lines.push(format!("status: {}", entry.status));
            }
            // Carried through md so a `sync` (which deletes and re-inserts the
            // file's entries) doesn't drop the recorded project.
            if let Some(ref project) = entry.project {
                lines.push(format!("project: {project}"));
            }
            if let Some(ref sb) = entry.superseded_by {
                lines.push(format!("superseded_by: {sb}"));
            }
            if let Some(ref ss) = entry.supersedes {
                lines.push(format!("supersedes: [{ss}]"));
            }
            lines.push(String::new());
            lines.push(entry.content.clone());
            lines.push(String::new());
        }

        std::fs::write(&filepath, lines.join("\n"))?;
        // User-scope md can hold private knowledge — keep it owner-only even if the
        // containing dir is loosened. (Git tracks only the exec bit, so 0600 vs 0644
        // causes no diff churn for a dotfiles-tracked store.)
        if restrict_files {
            crate::util::restrict_to_owner(&filepath, false);
        }

        // Flip entries to `shared` and record their source_file/hash — but ONLY for a
        // managed store. A dump-only export (user-scope `--dir`) leaves entries `local`
        // so a later `sync --scope user` (which never sees this dir) can't delete them.
        if flip_to_shared {
            // Compute the stored source_file from the canonicalized path so it matches
            // what `sync`/`import_md_file` derive from walkdir (which canonicalizes).
            // Without this, a symlinked knowledge dir (the dotfiles use case) would make
            // export and sync disagree on the path, breaking the md→DB round-trip.
            let canonical = std::fs::canonicalize(&filepath).unwrap_or_else(|_| filepath.clone());
            let rel_path = canonical
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| canonical.to_string_lossy().to_string());

            let fhash = markdown::file_hash(&filepath)?;
            let now = now_iso();
            for entry in group_entries {
                db::update_entry_to_shared(conn, entry.id, &rel_path, &fhash, &now)?;
            }
        }

        total += group_entries.len();
        println!(
            "  Exported {} entries to {}",
            group_entries.len(),
            filepath.display()
        );
    }

    println!("\nExported {total} entries total.");
    Ok(())
}

pub fn cmd_import(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db_with_migrate()?;
    let root = get_project_root();
    let count = import_md_file(&conn, path, &root)?;
    println!("Imported {count} entries from {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_for(group: &str) -> String {
        export_file_names(std::iter::once(group))
            .remove(group)
            .unwrap()
    }

    #[test]
    fn test_export_file_name_leaves_ordinary_keywords_alone() {
        // The common case must keep the name it has always had: `sync` matches entries
        // by their stored `source_file`, so a gratuitous rename would look like the old
        // file was deleted and a new one added.
        assert_eq!(name_for("features"), "exported-features.md");
        assert_eq!(name_for("認証"), "exported-認証.md");
        assert_eq!(name_for("feature-auth"), "exported-feature-auth.md");
    }

    #[test]
    fn test_export_file_name_cannot_escape_the_output_directory() {
        // `x/../../README` used to resolve outside the store and overwrite the file it
        // landed on. Whatever the keyword, the result is one path segment.
        for group in [
            "x/../../README",
            "../escape",
            "/etc/passwd",
            "a\\b",
            "..",
            ".",
            "",
            "  ",
            ".hidden",
            "left\u{202E}right",
        ] {
            let name = name_for(group);
            assert_eq!(
                Path::new(&name).components().count(),
                1,
                "{group:?} produced {name:?}"
            );
            assert!(!name.contains('/') && !name.contains('\\'), "{name:?}");
            assert!(
                !name.chars().any(|c| c.is_control() || c == '\u{202E}'),
                "{name:?}"
            );
        }
    }

    #[test]
    fn test_export_file_name_stays_within_the_component_limit() {
        // A long keyword used to fail the export with ENAMETOOLONG — the same failure
        // mode as an unflattened separator, from equally ordinary input.
        for group in [
            "a".repeat(300),
            "認".repeat(100), // 300 bytes in UTF-8
            format!("{}/{}", "x".repeat(200), "y".repeat(200)),
        ] {
            let name = name_for(&group);
            assert!(
                name.len() <= 255,
                "{} bytes for a {}-byte keyword: {name}",
                name.len(),
                group.len()
            );
            assert!(
                name.starts_with("exported-") && name.ends_with(".md"),
                "{name}"
            );
        }
        assert_ne!(
            name_for(&"z".repeat(300)),
            name_for(&format!("{}{}", "z".repeat(300), "-tail"))
        );
    }

    #[test]
    fn test_every_group_gets_its_own_file() {
        // The point of the digest: no two keywords may share a file, or exporting one
        // would take the other's entries with it. Includes the shapes that collide
        // only after folding, only on a case- or normalization-insensitive file
        // system, or only because a keyword looks like it already carries a digest.
        let groups = [
            "feature/auth",
            "feature-auth",
            "feature:auth",
            "|/</////",
            ":/\\:////",
            "auth",
            "AUTH",
            "が",         // U+304C
            "か\u{3099}", // the same text, decomposed
            "x-0123456789abcdef",
            "x/0123456789abcdef",
        ];
        let names = export_file_names(groups.iter().copied());
        assert_eq!(names.len(), groups.len());

        let mut keys: Vec<String> = names.values().map(|n| file_system_key(n)).collect();
        keys.sort();
        let unique = keys.len();
        keys.dedup();
        assert_eq!(
            keys.len(),
            unique,
            "two groups share a file name: {:?}",
            names
        );
    }

    #[test]
    fn test_names_are_stable_across_runs() {
        // A name may not depend on which other groups happen to exist, or a later
        // keyword would rename an existing file and `sync` would read it as a delete
        // plus an add.
        let alone = name_for("feature/auth");
        let together = export_file_names(["feature/auth", "unrelated", "another/one"].into_iter())
            .remove("feature/auth")
            .unwrap();
        assert_eq!(alone, together);
    }
}
