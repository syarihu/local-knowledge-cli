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
/// Lowercase first, compose second — the order `db::normalize_keyword` uses, and for
/// the same reason: lowercasing can break composition, so composing first leaves a key
/// that is not NFC after all. `H` with a combining macron below has no precomposed
/// uppercase form, so it survives NFC as two characters and lowercases to `h` + the
/// mark, while the `ẖ` a keyword normalizes to is one — two keys for the one file a
/// case-folding file system would give them. That is exactly the collision this key
/// exists to catch, and it reaches here through a name already on disk, which nothing
/// normalized on the way in.
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
    name.to_lowercase().nfc().collect()
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

/// Refuse a destination that is not a plain file inside `output_dir`.
///
/// The name is one segment by construction; checking it keeps that an invariant rather
/// than a comment, so a future change to the naming cannot quietly reintroduce a path.
/// A symlink is refused because `.knowledge/` travels with the repository — a link
/// committed as `exported-auth.md -> ../README.md` is a request to write outside the
/// store. (The store's *directory* being a symlink is the dotfiles case and stays
/// allowed.) The refusal is for the user's sake rather than for safety: the write
/// itself cannot follow a link. Anything else that is not a plain file is refused for
/// the preflight's sake — see the match below.
fn check_destination(output_dir: &Path, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    if Path::new(filename).components().count() != 1 {
        return Err(format!("refusing to export to a non-file name: {filename:?}").into());
    }
    let filepath = output_dir.join(filename);
    match std::fs::symlink_metadata(&filepath) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "{} is a symlink; refusing to write through it. Remove it and export again.",
            filepath.display()
        )
        .into()),
        // A directory (or a fifo, a socket, a device) is refused here rather than left
        // to fail at the `rename`: that failure lands after the earlier groups are
        // written and flipped to `shared`, which is the mixed state this preflight
        // exists to prevent.
        Ok(meta) if !meta.is_file() => Err(format!(
            "{} is not a plain file; refusing to replace it. Move it and export again.",
            filepath.display()
        )
        .into()),
        Ok(_) => Ok(()),
        // Nothing there is the normal case for a first export.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // Anything else (a permission problem on the directory, say) would make the
        // check above vacuous, so it stops the export instead of passing silently.
        Err(e) => Err(format!("could not inspect {}: {e}", filepath.display()).into()),
    }
}

/// The name one attempt at a temp file uses.
///
/// Both the nonce and the attempt index, because they cover different failures. The
/// nonce is what keeps a temp left behind by a kill or a power cut from sitting on the
/// one name a later run with the same pid is bound to retry — every export from that pid
/// would fail. The index is what keeps sixteen retries sixteen names: a clock too coarse
/// to advance inside the loop, or one that cannot be read at all (the `unwrap_or(0)`
/// below), hands every attempt the same nonce, and then a single stale file is once again
/// enough to fail them all.
fn temp_file_name(pid: u32, nonce: u32, attempt: u32) -> String {
    format!(".lk-export-{pid}-{nonce:08x}-{attempt:x}.tmp")
}

/// Create a file in `dir` that nobody else holds, at `mode` before the umask narrows it.
///
/// `create_new` in a loop, so this never opens something that already exists. See
/// `temp_file_name` for what makes the attempts distinct.
///
/// Separate from `write_atomically` so the mode a file is *created* with can be asserted
/// on its own — that is the part protecting content while it is being written, and a
/// test of the final mode passes whether or not it is right.
fn create_temp_file(
    dir: &Path,
    mode: Option<u32>,
) -> Result<(PathBuf, std::fs::File), Box<dyn std::error::Error>> {
    for attempt in 0..16 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let candidate = dir.join(temp_file_name(std::process::id(), nonce, attempt));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Let the kernel apply umask to this, exactly as a plain create would.
            opts.mode(mode.unwrap_or(0o666));
        }
        #[cfg(not(unix))]
        let _ = mode;
        match opts.open(&candidate) {
            Ok(f) => return Ok((candidate, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err("could not create a temporary file to export into".into())
}

/// Write `contents` to `path` by creating a fresh file beside it and renaming over it.
///
/// Not `std::fs::write`: that follows a symlink and truncates a hard link's shared
/// inode, and `.knowledge/` travels with the repository — a link committed as
/// `exported-auth.md -> ../README.md`, or a hard link to it, was enough to damage a
/// file outside the store. It also leaves a half-written file if it fails midway. A
/// rename replaces the directory entry itself, atomically, and cannot be raced into
/// following a link.
///
/// An `owner_only` file is 0600 from the moment it exists, rather than tightened after
/// the rename: user-scope knowledge is private, and a mode fixed up afterwards leaves
/// the contents readable while they are being written and readable for good if the
/// process dies in between. Otherwise the new file is created with the mode of the file
/// it replaces, or `0666` for a new one — as `std::fs::write` would, so the caller's
/// umask still applies. Restoring a replaced file's mode is an explicit
/// `set_permissions`, since umask must not narrow it.
///
/// Only the usual rwx bits survive a replacement: setuid/setgid/sticky are dropped, and
/// the file is a new inode, so ACLs and xattrs are not carried over either.
///
/// The parent directory is deliberately left unsynced. An export is regenerable from the
/// DB, so a rename lost to a power cut costs a re-run rather than data.
fn write_atomically(
    path: &Path,
    contents: &str,
    owner_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let dir = path.parent().ok_or("output path has no parent directory")?;
    #[cfg(not(unix))]
    let _ = owner_only;
    #[cfg(unix)]
    let target_mode = {
        use std::os::unix::fs::PermissionsExt;
        if owner_only {
            Some(0o600)
        } else {
            std::fs::metadata(path)
                .ok()
                .map(|m| m.permissions().mode() & 0o777)
        }
    };

    #[cfg(unix)]
    let (temp_path, mut file) = create_temp_file(dir, target_mode)?;
    #[cfg(not(unix))]
    let (temp_path, mut file) = create_temp_file(dir, None)?;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        if let Some(mode) = target_mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(mode))?;
        }
        std::fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        std::fs::remove_file(&temp_path).ok();
    }
    result
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
    //
    // Every destination is checked here, before the first one is written: refusing
    // half-way through leaves the earlier groups written and flipped to `shared` while
    // the command reports failure, and that mixed state is worse than either outcome.
    {
        // The store's existing files count too, not just the selected groups. A pre-v7
        // export left `exported-AUTH.md` owning its entries, and migration 7 lowercased
        // that keyword without renaming the file — so a later `auth` export plans
        // `exported-auth.md`, which is the same directory entry on a file system that
        // ignores case. Replacing it makes the next `sync` see the old path as gone and
        // delete every entry it owned. Only a name that *differs* is refused:
        // re-exporting a group over its own file is the ordinary case.
        let mut owned: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if flip_to_shared {
            for rel_path in db::get_shared_file_hashes(conn)?.into_keys() {
                let path = root.join(&rel_path);
                // Only files sitting directly in this directory can collide; a same-named
                // file in a subdirectory of the store is a different directory entry.
                if !path
                    .parent()
                    .is_some_and(|parent| crate::util::paths_equivalent(parent, output_dir))
                {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    owned.insert(file_system_key(name), name.to_string());
                }
            }
        }

        let mut by_key: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for group in groups.keys() {
            let filename = export_file_name(group);
            check_destination(output_dir, &filename)?;
            let key = file_system_key(&filename);
            if let Some(first) = by_key.insert(key.clone(), group) {
                return Err(format!(
                    "keywords {first:?} and {group:?} would export to the same file. \
                     Rename one of them."
                )
                .into());
            }
            if let Some(existing) = owned.get(&key).filter(|name| *name != &filename) {
                return Err(format!(
                    "keyword {group:?} would export to {filename}, which this file system \
                     sees as the same file as {existing} — and that file already holds \
                     shared entries. Rename it (or the keyword) and export again."
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
        // Checked in the preflight above too. Repeated here because a link appearing in
        // between would otherwise be replaced by a regular file — the target is safe
        // either way, since a rename cannot write through a link, but a link the user
        // put there deliberately should not vanish silently.
        check_destination(output_dir, &filename)?;
        let filepath = output_dir.join(&filename);

        let mut all_kws: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in &sorted_entries {
            let kws = db::get_keywords(conn, entry.id)?;
            all_kws.extend(kws);
        }

        // The group keyword heads the file-level list, and the rest follow in sorted
        // order. Not cosmetic: a re-imported entry's keywords are seeded from this list
        // (`extract_entry_metadata`), so whatever sits at its head becomes the entry's
        // first keyword — and the first keyword names the file. Sorted alone, an entry
        // keyworded `zebra, apple` came back as `apple, zebra` after an edit and a
        // `sync`, and the next export renamed `exported-zebra.md` to `exported-apple.md`.
        // A group with no keyword at all (the `general` fallback) is left out rather than
        // injected, so the round trip cannot invent a keyword either.
        let mut ordered: Vec<String> = Vec::with_capacity(all_kws.len());
        if all_kws.contains(group_name) {
            ordered.push(group_name.clone());
        }
        ordered.extend(all_kws.iter().filter(|kw| *kw != group_name).cloned());

        let mut lines = Vec::new();
        lines.push("---".to_string());
        lines.push(format!("keywords: [{}]", ordered.join(", ")));
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

        // `restrict_files` is passed down rather than applied afterwards, so the file is
        // never briefly readable while private knowledge is being written into it.
        write_atomically(&filepath, &lines.join("\n"), restrict_files)?;
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
            // Every entry in the file flips together, or none of them does. Half of them
            // left `local` is a state the store cannot recover from: the next export
            // rebuilds this file from the `local` ones alone, and `sync` then deletes the
            // `shared` rows still pointing at it — so the entries that did flip are lost.
            conn.execute_batch("SAVEPOINT export_flip")?;
            let flipped = (|| -> Result<(), Box<dyn std::error::Error>> {
                for entry in group_entries {
                    db::update_entry_to_shared(conn, entry.id, &rel_path, &fhash, &now)?;
                }
                Ok(())
            })();
            match flipped {
                Ok(()) => conn.execute_batch("RELEASE export_flip")?,
                Err(e) => {
                    conn.execute_batch("ROLLBACK TO export_flip; RELEASE export_flip;")
                        .ok();
                    return Err(e);
                }
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
    fn test_the_key_composes_after_lowercasing() {
        // The other order leaves a key that is not NFC: `H` with a combining macron
        // below has no precomposed uppercase form, so it survives NFC as two characters
        // and only lowercasing brings it to the one `ẖ` is. A name already on disk goes
        // through this key without having been normalized anywhere, so the two
        // spellings have to meet — a file system that folds case and normalization gives
        // them one file.
        assert_eq!(
            file_system_key("exported-H\u{0331}.md"),
            file_system_key("exported-\u{1E96}.md"),
            "the two spellings are one file name"
        );
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

    #[cfg(unix)]
    #[test]
    fn test_a_frozen_clock_still_gives_every_attempt_its_own_name() {
        // The retries are only retries if they try different names. A clock that cannot
        // be read leaves the nonce at 0 for all sixteen, and one stale temp file from a
        // killed run then fails every export this process attempts.
        let names: std::collections::HashSet<String> = (0..16)
            .map(|attempt| temp_file_name(4242, 0, attempt))
            .collect();
        assert_eq!(
            names.len(),
            16,
            "sixteen attempts, sixteen names: {names:?}"
        );
    }

    #[test]
    fn test_an_owner_only_temp_file_is_private_before_anything_is_written() {
        // The mode has to be right at creation. Asserting the final mode instead passes
        // even if the file is created world-readable and tightened just before the
        // rename — which leaves the contents readable for the whole write.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode_of = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let (private, _f) = create_temp_file(dir.path(), Some(0o600)).unwrap();
        assert_eq!(
            mode_of(&private),
            0o600,
            "a user-scope temp must be private from the moment it exists"
        );

        // Without a mode, whatever a plain create would have produced under this umask.
        let probe = dir.path().join("probe");
        std::fs::write(&probe, "").unwrap();
        let (plain, _f2) = create_temp_file(dir.path(), None).unwrap();
        assert_eq!(
            mode_of(&plain),
            mode_of(&probe),
            "a project-scope temp should follow the umask"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_an_owner_only_write_starts_out_owner_only() {
        // The mode has to be right at creation, not fixed up after the rename: with the
        // flag ignored the file would be created `0666 & !umask` — 0644 under the usual
        // 022 — and stay readable until the caller tightened it, which is too late for
        // anything already written into it.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode_of = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let private = dir.path().join("private.md");
        write_atomically(&private, "secret", true).unwrap();
        assert_eq!(mode_of(&private), 0o600, "user-scope md must be owner-only");
        assert_eq!(std::fs::read_to_string(&private).unwrap(), "secret");

        // Without the flag: whatever a plain write would have produced here, so the
        // umask still decides and this holds wherever the suite runs.
        let probe = dir.path().join("probe");
        std::fs::write(&probe, "").unwrap();
        let plain = dir.path().join("plain.md");
        write_atomically(&plain, "public", false).unwrap();
        assert_eq!(
            mode_of(&plain),
            mode_of(&probe),
            "a project-scope export should get the mode a plain write would"
        );
    }
}
