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

/// A digest of the keyword itself, so a name depends on nothing else.
fn group_digest(group: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(group.as_bytes())[..DIGEST_HEX / 2])
}

/// Whether a base name already looks like it carries a digest, in which case a plain
/// name would sit in the namespace of a disambiguated one. Case-insensitive: a file
/// system that ignores case would see `-ABCD…` and `-abcd…` as one name.
fn looks_disambiguated(base: &str) -> bool {
    base.rsplit_once('-').is_some_and(|(_, tail)| {
        tail.len() == DIGEST_HEX && tail.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// How a file system that ignores case and normalization would see a name. Two names
/// that agree here would land on one file, and the second write would take the first
/// group's entries with it.
///
/// `to_lowercase` is not full case folding, so a directory carrying ext4's `casefold`
/// attribute would still see `ß` and `ss` as one name — and neither this key nor the
/// check in `cmd_export` can tell. Everything that follows from ordinary use of this
/// tool — case variants, and the NFC/NFD spellings macOS folds — is covered. That
/// last gap stays open knowingly: closing it means carrying a full case-folding table
/// for a directory flag almost nobody sets, and keywords are normalized on the way in
/// (`db::normalize_keyword`), so a pair like `ß`/`ss` has to be deliberate.
fn file_system_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfc().collect::<String>().to_lowercase()
}

/// The output file name for a group.
///
/// A pure function of the keyword: nothing about which other groups exist, or what
/// the store already holds, can change it. That matters twice over — a name that
/// depended on the current selection would drift between a full and a partial
/// `export`, and one that depended on the store would rename files as keywords come
/// and go, which `sync` reads as a delete plus an add.
///
/// The name carries a digest of the keyword unless the keyword is already exactly
/// what a file system would store: unchanged by flattening, already in the form
/// `file_system_key` produces, and not passing itself off as disambiguated. So
/// `feature/auth`, `AUTH`, and a decomposed `か\u{3099}` each get one, while `auth`
/// and `認証` keep the plain name they have always had.
fn export_file_name(group: &str) -> String {
    let base = export_base_name(group);
    let canonical = base == group && file_system_key(group) == group;
    if canonical && !looks_disambiguated(&base) {
        format!("exported-{base}.md")
    } else {
        format!("exported-{base}-{}.md", group_digest(group))
    }
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

    // Names are a pure function of the keyword, so this only guards against a digest
    // collision (2^-64): a clash is reported rather than silently overwriting one
    // group's file with another's. It compares under `file_system_key`, which does not
    // full-case-fold — see the note there for the gap that leaves.
    {
        let mut by_key: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for group in groups.keys() {
            let key = file_system_key(&export_file_name(group));
            if let Some(first) = by_key.insert(key, group) {
                return Err(format!(
                    "keywords {first:?} and {group:?} would export to the same file. \
                     Rename one of them."
                )
                .into());
            }
        }
    }

    let mut total = 0;
    for (group_name, group_entries) in &groups {
        // Sort entries within each group by title for stable output
        let mut sorted_entries: Vec<&db::Entry> = group_entries.iter().collect();
        sorted_entries.sort_by_key(|e| e.title.to_lowercase());

        let filename = export_file_name(group_name);
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

        // Written to a fresh file beside the target and renamed over it, rather than
        // written through whatever is already there. `std::fs::write` follows a
        // symlink and truncates a hard link's shared inode — `.knowledge/` travels
        // with the repository, so a committed `exported-auth.md -> ../README.md` or a
        // hard link to it was enough to damage a file outside the store — and it
        // leaves a half-written file if it fails midway. A rename replaces the entry
        // itself, atomically, and cannot be raced into following a link.
        let mut temp = tempfile::Builder::new()
            .prefix(".lk-export-")
            .tempfile_in(output_dir)?;
        std::io::Write::write_all(&mut temp, lines.join("\n").as_bytes())?;
        // A temp file is created 0600, and a rename keeps the source's mode rather
        // than the target's — so without this, replacing a world-readable
        // `.knowledge/*.md` (which is committed and read by other tools) would
        // quietly make it owner-only. Keep what the file had, or the usual default
        // for a new one; user-scope files are tightened to 0600 further down anyway.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&filepath)
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0o644);
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode))?;
        }
        temp.persist(&filepath)
            .map_err(|e| format!("failed to write {}: {}", filepath.display(), e.error))?;
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

    #[test]
    fn test_export_file_name_leaves_canonical_keywords_alone() {
        // The common case must keep the name it has always had: `sync` matches entries
        // by their stored `source_file`, so a gratuitous rename would look like the old
        // file was deleted and a new one added.
        assert_eq!(export_file_name("features"), "exported-features.md");
        assert_eq!(export_file_name("認証"), "exported-認証.md");
        assert_eq!(export_file_name("feature-auth"), "exported-feature-auth.md");
    }

    #[test]
    fn test_export_file_name_cannot_escape_the_output_directory() {
        // `x/../../README` used to resolve outside the store and overwrite the file it
        // landed on. Whatever the keyword, the result is one path segment with no
        // control or bidirectional characters left in it.
        let bidi = [
            '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
            '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ];
        let mut groups: Vec<String> = [
            "x/../../README",
            "../escape",
            "/etc/passwd",
            "a\\b",
            "..",
            ".",
            "",
            "  ",
            ".hidden",
            "tab\there",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        groups.extend(bidi.iter().map(|c| format!("left{c}right")));

        for group in &groups {
            let name = export_file_name(group);
            assert_eq!(
                Path::new(&name).components().count(),
                1,
                "{group:?} produced {name:?}"
            );
            assert!(!name.contains('/') && !name.contains('\\'), "{name:?}");
            assert!(
                !name.chars().any(|c| c.is_control() || bidi.contains(&c)),
                "{group:?} produced {name:?}"
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
            let name = export_file_name(&group);
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
            export_file_name(&"z".repeat(300)),
            export_file_name(&format!("{}{}", "z".repeat(300), "-tail"))
        );
    }

    #[test]
    fn test_no_two_keywords_share_a_file() {
        // The point of the digest: exporting one keyword must never take another's
        // entries with it. These are the pairs that collide on a plain string
        // comparison, after flattening, or only once a file system folds case and
        // normalization.
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
            // Passing itself off as disambiguated, in both cases — a file system that
            // ignores case would see one name.
            "foo-2c26b46b68ffc68f",
            "foo-2C26B46B68FFC68F",
            "foo/",
            "foo",
            "FOO",
        ];
        let mut keys: Vec<String> = groups
            .iter()
            .map(|g| file_system_key(&export_file_name(g)))
            .collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(
            keys.len(),
            total,
            "two keywords share a file: {:?}",
            groups
                .iter()
                .map(|g| (g, export_file_name(g)))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_names_do_not_depend_on_which_other_keywords_exist() {
        // A name is a pure function of its keyword. Anything else drifts between a
        // full and a partial export, and renames files as keywords come and go — which
        // `sync` reads as a delete plus an add.
        assert_eq!(export_file_name("foo"), "exported-foo.md");
        assert_eq!(
            export_file_name("feature/auth"),
            export_file_name("feature/auth")
        );
        // `FOO` existing cannot change what `foo` is called, and vice versa.
        assert_eq!(export_file_name("foo"), "exported-foo.md");
        assert!(export_file_name("FOO").starts_with("exported-FOO-"));
    }

    #[test]
    fn test_full_case_folding_is_a_known_gap() {
        // Pinned as a limitation, not a guarantee: `to_lowercase` is not full case
        // folding, so `ß` and `ss` are two names here even though a directory with
        // ext4's `casefold` attribute would see one — and the duplicate check in
        // `cmd_export` uses this same key, so it does not catch the pair either.
        // Closing it means carrying a case-folding table for a flag almost nobody
        // sets. If this assertion ever fails because the key learned to fold, the
        // gap is closed and the comments above should go.
        assert_ne!(
            file_system_key(&export_file_name("ß")),
            file_system_key(&export_file_name("ss")),
            "the key does not full-case-fold; see the note in `file_system_key`"
        );
    }
}
