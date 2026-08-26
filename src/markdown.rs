use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct MdEntry {
    pub title: String,
    pub content: String,
    pub keywords: Vec<String>,
    pub category: String,
    pub uid: Option<String>,
    pub status: Option<String>,
    pub project: Option<String>,
    pub superseded_by: Option<String>,
    pub supersedes: Vec<String>,
}

struct Frontmatter {
    keywords: Vec<String>,
    category: String,
    uid: Option<String>,
    status: Option<String>,
    project: Option<String>,
    superseded_by: Option<String>,
    supersedes: Vec<String>,
}

/// Parse YAML-ish frontmatter. Returns (Frontmatter, body).
fn parse_frontmatter(text: &str) -> (Frontmatter, &str) {
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").unwrap();
    if let Some(cap) = re.captures(text) {
        let fm_text = cap.get(1).unwrap().as_str();
        let body = &text[cap.get(0).unwrap().end()..];

        let mut fm = Frontmatter {
            keywords: Vec::new(),
            category: String::new(),
            uid: None,
            status: None,
            project: None,
            superseded_by: None,
            supersedes: Vec::new(),
        };

        // Ids and uids, so tokenizing is the right read here — unlike `keywords:`,
        // whose values are free text and are parsed by `parse_keyword_list`.
        let id_re =
            Regex::new(r"[\w\u{3040}-\u{309F}\u{30A0}-\u{30FF}\u{4E00}-\u{9FFF}-]+").unwrap();
        for line in fm_text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("keywords:") {
                fm.keywords.extend(parse_keyword_list(rest));
            } else if let Some(rest) = line.strip_prefix("category:") {
                fm.category = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("uid:") {
                let val = rest.trim().trim_matches('"').to_string();
                if !val.is_empty() {
                    fm.uid = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("status:") {
                let val = rest.trim().trim_matches('"').to_string();
                if !val.is_empty() {
                    fm.status = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("project:") {
                let val = rest.trim().trim_matches('"').to_string();
                if !val.is_empty() {
                    fm.project = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("superseded_by:") {
                let val = rest.trim().trim_matches('"').to_string();
                if !val.is_empty() {
                    fm.superseded_by = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("supersedes:") {
                for mat in id_re.find_iter(rest) {
                    fm.supersedes.push(mat.as_str().to_string());
                }
            }
        }

        (fm, body)
    } else {
        (
            Frontmatter {
                keywords: Vec::new(),
                category: String::new(),
                uid: None,
                status: None,
                project: None,
                superseded_by: None,
                supersedes: Vec::new(),
            },
            text,
        )
    }
}

/// Parse markdown text into individual entries.
pub fn parse_md_entries(text: &str) -> Vec<MdEntry> {
    let (fm, body) = parse_frontmatter(text);

    let entry_re = Regex::new(r"(?m)^## Entry:\s*(.+)$").unwrap();
    let matches: Vec<_> = entry_re.captures_iter(body).collect();

    let mut entries = Vec::new();

    if matches.is_empty() {
        // No entry sections - treat entire body as single entry
        let title_re = Regex::new(r"(?m)^#\s+(.+)$").unwrap();
        let (title, content) = if let Some(cap) = title_re.captures(body) {
            let t = cap.get(1).unwrap().as_str().trim().to_string();
            let after = &body[cap.get(0).unwrap().end()..];
            (t, after.trim().to_string())
        } else {
            ("Untitled".to_string(), body.trim().to_string())
        };

        let (entry_kws, content, meta) = extract_entry_metadata(&content, &fm.keywords);

        if !content.is_empty() {
            entries.push(MdEntry {
                title,
                content,
                keywords: entry_kws,
                category: fm.category.clone(),
                uid: meta.uid.or(fm.uid.clone()),
                status: meta.status.or(fm.status.clone()),
                project: meta.project.or(fm.project.clone()),
                superseded_by: meta.superseded_by.or(fm.superseded_by.clone()),
                supersedes: if meta.supersedes.is_empty() {
                    fm.supersedes.clone()
                } else {
                    meta.supersedes
                },
            });
        }
    } else {
        let byte_positions: Vec<_> = entry_re
            .find_iter(body)
            .map(|m| (m.start(), m.end()))
            .collect();

        for (i, cap) in matches.iter().enumerate() {
            let title = cap.get(1).unwrap().as_str().trim().to_string();
            let start = byte_positions[i].1;
            let end = if i + 1 < byte_positions.len() {
                byte_positions[i + 1].0
            } else {
                body.len()
            };
            let raw_content = body[start..end].trim().to_string();
            let (entry_kws, content, meta) = extract_entry_metadata(&raw_content, &fm.keywords);

            if !content.is_empty() {
                entries.push(MdEntry {
                    title,
                    content,
                    keywords: entry_kws,
                    category: fm.category.clone(),
                    uid: meta.uid,
                    status: meta.status,
                    // A per-entry line wins; the frontmatter value covers a file
                    // whose entries all came from the same project.
                    project: meta.project.or_else(|| fm.project.clone()),
                    superseded_by: meta.superseded_by,
                    supersedes: meta.supersedes,
                });
            }
        }
    }

    entries
}

struct EntryMeta {
    uid: Option<String>,
    status: Option<String>,
    project: Option<String>,
    superseded_by: Option<String>,
    supersedes: Vec<String>,
}

/// Keys recognized as entry metadata when they head an entry.
const ENTRY_META_KEYS: [&str; 6] = [
    "keywords",
    "uid",
    "status",
    "project",
    "superseded_by",
    "supersedes",
];

/// Parse a `keywords:` value — `[a, b]`, or the bare `a, b` inside it — into keywords.
///
/// A keyword is whatever sits between the commas, not a run of word characters. The
/// frontmatter used to tokenize instead, which split `feature/auth` into `feature` and
/// `auth` (and `main.rs` into `main` and `rs`) while the per-entry parser below kept
/// them whole. Since `export` writes both lines, an export/sync round trip grew
/// `feature/auth` into three keywords — and then renamed the file on the next export,
/// because the first keyword decides the name. One parser, so the two cannot drift.
fn parse_keyword_list(value: &str) -> Vec<String> {
    let value = value.trim();
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .map(|kw| unquote(kw.trim()))
        .filter(|kw| !kw.is_empty())
        .map(str::to_string)
        .collect()
}

/// Strip one matched pair of surrounding quotes, and nothing else.
///
/// `trim_matches` took every quote at either end, so a keyword is not free text after
/// all: `users'` came back as `users` from an export/`sync` round trip, and `'quoted`
/// as `quoted`. A quote only delimits when there is one at each end.
fn unquote(value: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(q).and_then(|v| v.strip_suffix(q)) {
            return inner.trim();
        }
    }
    value
}

/// Whether `line` is a metadata line that actually carries a value.
///
/// The check is deliberately as strict as the value parsers below: `keywords: foo`
/// (no brackets) and a bare `uid:` yield nothing, so treating them as metadata would
/// delete text from the content while recording nothing.
fn is_meta_line(line: &str) -> bool {
    let Some(key) = ENTRY_META_KEYS
        .iter()
        .find(|k| line.strip_prefix(**k).is_some_and(|r| r.starts_with(':')))
    else {
        return false;
    };
    // Keys are ASCII, so slicing past `key:` is byte-safe.
    let value = line[key.len() + 1..].trim().trim_matches('"');
    if *key == "keywords" {
        // The whole value must be one `[...]` list. `keywords: [auth] and more` also
        // parses as `[auth]`, but classifying the line as metadata would delete the
        // trailing prose from the content — the old parser kept it.
        value.starts_with('[') && value.ends_with(']') && !value[1..value.len() - 1].contains(']')
    } else {
        !value.is_empty()
    }
}

/// Split an entry body into its leading metadata block and the content after it.
///
/// Only the run of well-formed `key: value` lines at the top counts (blank lines
/// between them are tolerated). A `project:` or `status:` line further down is prose
/// or an example — consuming it would both mislabel the entry and delete the line
/// from the stored content, which is how documentation about this very format used
/// to lose lines on `sync`.
fn split_entry_metadata(content: &str) -> (&str, &str) {
    let mut meta_end = 0;
    let mut cursor = 0;
    for line in content.split_inclusive('\n') {
        if is_meta_line(line) {
            cursor += line.len();
            meta_end = cursor;
        } else if line.trim().is_empty() {
            cursor += line.len();
        } else {
            break;
        }
    }
    (&content[..meta_end], &content[meta_end..])
}

/// Take a `uid:` line from outside the leading block, for markdown written before
/// metadata was confined to that block.
///
/// A lost uid is not a cosmetic regression: `sync` would delete and re-insert the
/// entry under a fresh uid, breaking the cross-machine merge key, supersede
/// references, and duplicate detection. Only a uid-shaped value is taken, so prose
/// like `uid: see the table below` cannot hijack an entry — which is why this
/// leniency is safe for `uid` alone and not for `status`/`project`.
fn take_trailing_uid(body: &str) -> Option<(String, String)> {
    // Uppercase too: the old parser took any value, so a `uid: A1B2C3D4E5F6`
    // written by hand must keep identifying its entry. The value is used verbatim.
    let re = Regex::new(r"(?m)^uid:[ \t]*([0-9a-fA-F]{6,32})[ \t]*\r?$").unwrap();
    let cap = re.captures(body)?;
    let uid = cap.get(1)?.as_str().to_string();
    Some((uid, re.replace(body, "").to_string()))
}

/// Extract inline metadata (keywords, uid, status, project, superseded_by, supersedes)
/// from the metadata block heading an entry, and return the content below it.
fn extract_entry_metadata(content: &str, file_kws: &[String]) -> (Vec<String>, String, EntryMeta) {
    let mut kws: Vec<String> = file_kws.to_vec();
    let (block, body) = split_entry_metadata(content);
    let mut meta = EntryMeta {
        uid: None,
        status: None,
        project: None,
        superseded_by: None,
        supersedes: Vec::new(),
    };

    let kw_re = Regex::new(r"(?m)^keywords:\s*\[(.*)\]").unwrap();
    if let Some(cap) = kw_re.captures(block) {
        for kw in parse_keyword_list(cap.get(1).unwrap().as_str()) {
            if !kws.contains(&kw) {
                kws.push(kw);
            }
        }
    }

    /// Read a single `key: value` metadata line, unquoted and trimmed.
    fn single(block: &str, key: &str) -> Option<String> {
        let re = Regex::new(&format!(r"(?m)^{key}:\s*(.+)$")).unwrap();
        let val = re
            .captures(block)?
            .get(1)?
            .as_str()
            .trim()
            .trim_matches('"')
            .to_string();
        (!val.is_empty()).then_some(val)
    }

    meta.uid = single(block, "uid");
    meta.status = single(block, "status");
    meta.project = single(block, "project");
    meta.superseded_by = single(block, "superseded_by");

    // supersedes: [uid1, uid2] or supersedes: uid1
    if let Some(val) = single(block, "supersedes") {
        let val = val.trim_start_matches('[').trim_end_matches(']');
        for uid in val.split(',') {
            let uid = uid.trim().trim_matches('"').to_string();
            if !uid.is_empty() {
                meta.supersedes.push(uid);
            }
        }
    }

    // Backward compatibility: honor a uid that sits below the block (see
    // `take_trailing_uid`) and, as the old parser did, remove that line.
    let mut body = body.to_string();
    if meta.uid.is_none()
        && let Some((uid, rest)) = take_trailing_uid(&body)
    {
        meta.uid = Some(uid);
        body = rest;
    }

    (kws, body.trim().to_string(), meta)
}

/// Compute SHA256 hash of a file.
pub fn file_hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    let hash = Sha256::digest(&data);
    Ok(hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_keywords_are_comma_separated_not_tokenized() {
        // The frontmatter used to tokenize on word characters, so `feature/auth` and
        // `main.rs` each arrived as two keywords while the per-entry parser kept them
        // whole. `export` writes both lines, so a round trip grew one keyword into
        // three and then renamed the file, since the first keyword names it.
        let md = "---\nkeywords: [feature/auth, main.rs, \"quoted\"]\ncategory: features\n---\n\n# T\n\nBody.\n";
        let entries = parse_md_entries(md);
        assert_eq!(
            entries[0].keywords,
            vec!["feature/auth", "main.rs", "quoted"],
            "got {:?}",
            entries[0].keywords
        );
    }

    #[test]
    fn test_only_a_matched_pair_of_quotes_is_stripped() {
        // A keyword is free text, and an apostrophe is part of it. `trim_matches` took
        // every quote at either end, so `users'` came back from an export/`sync` round
        // trip as `users` — a different keyword, under a different file name.
        let md = "---\nkeywords: [users', 'quoted, \"paired\", ']\ncategory: features\n---\n\n# T\n\nBody.\n";
        let entries = parse_md_entries(md);
        assert_eq!(
            entries[0].keywords,
            vec!["users'", "'quoted", "paired", "'"],
            "got {:?}",
            entries[0].keywords
        );
    }

    #[test]
    fn test_both_keyword_lines_read_a_slashed_keyword_the_same_way() {
        // The frontmatter and per-entry lines are merged, so the two parsers agreeing
        // is what keeps the merge from inventing keywords.
        let md = "---\nkeywords: [feature/auth]\ncategory: exported\n---\n\n# Exported: feature/auth\n\n## Entry: auth flow\nkeywords: [feature/auth]\n\nbody\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].keywords,
            vec!["feature/auth"],
            "got {:?}",
            entries[0].keywords
        );
    }

    #[test]
    fn test_parse_single_entry_with_frontmatter() {
        let md = "---\nkeywords: [auth, login]\ncategory: architecture\n---\n\n# Auth Flow\n\nOAuth 2.0 with PKCE.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Auth Flow");
        assert!(entries[0].content.contains("OAuth 2.0"));
        assert!(entries[0].keywords.contains(&"auth".to_string()));
        assert!(entries[0].keywords.contains(&"login".to_string()));
        assert_eq!(entries[0].category, "architecture");
    }

    #[test]
    fn test_parse_entry_project_line() {
        // `project:` must round-trip through md, or a sync (which deletes and
        // re-inserts a file's entries) would drop the recorded project.
        let md = "---\nkeywords: [auth]\ncategory: features\n---\n\n# Title\n\n## Entry: First\nuid: a1b2c3d4e5f6\nproject: syarihu/local-knowledge-cli\n\nFirst content.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].project.as_deref(),
            Some("syarihu/local-knowledge-cli")
        );
        // The metadata line is stripped from the stored content.
        assert!(!entries[0].content.contains("project:"));
        assert!(entries[0].content.contains("First content."));
    }

    #[test]
    fn test_parse_project_from_frontmatter_and_override() {
        // Frontmatter covers a whole file; a per-entry line wins for its own entry.
        let md = "---\nkeywords: [auth]\ncategory: features\nproject: syarihu/from-file\n---\n\n# Title\n\n## Entry: First\n\nFirst content.\n\n## Entry: Second\nproject: syarihu/from-entry\n\nSecond content.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].project.as_deref(), Some("syarihu/from-file"));
        assert_eq!(entries[1].project.as_deref(), Some("syarihu/from-entry"));
    }

    #[test]
    fn test_metadata_lines_below_the_block_stay_in_the_content() {
        // Documentation about this md format contains lines that LOOK like metadata.
        // They must stay in the content and must not become the entry's metadata.
        let md = "---\nkeywords: [mdformat]\ncategory: features\n---\n\n# Doc\n\n## Entry: How the format looks\nkeywords: [mdformat]\nproject: real/repo\n\nMetadata goes under the heading:\n\nproject: demo/app\nstatus: deprecated\n\nThose two lines are documentation.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        // The heading block still wins for the real metadata.
        assert_eq!(entries[0].project.as_deref(), Some("real/repo"));
        assert_eq!(entries[0].status, None);
        // ...and the prose below keeps both lines verbatim.
        assert!(
            entries[0].content.contains("project: demo/app"),
            "content: {}",
            entries[0].content
        );
        assert!(entries[0].content.contains("status: deprecated"));
    }

    #[test]
    fn test_metadata_block_tolerates_a_blank_line_before_it() {
        // Hand-written md sometimes leaves a blank line under the heading; the block
        // is still metadata there, so a uid keeps identifying the same entry.
        let md = "---\ncategory: features\n---\n\n# Doc\n\n## Entry: Spaced out\n\nuid: a1b2c3d4e5f6\nproject: syarihu/repo\n\nBody text.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uid.as_deref(), Some("a1b2c3d4e5f6"));
        assert_eq!(entries[0].project.as_deref(), Some("syarihu/repo"));
        assert_eq!(entries[0].content, "Body text.");
    }

    #[test]
    fn test_uid_below_the_block_still_identifies_the_entry() {
        // Markdown written before metadata was confined to the leading block. Losing
        // this uid would make the next sync insert a NEW entry (broken merge key,
        // duplicates), so it is still honored — and still stripped from the content.
        let md = "---\ncategory: features\n---\n\n# Doc\n\n## Entry: Legacy layout\n\nBody first.\n\nuid: a1b2c3d4e5f6\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uid.as_deref(), Some("a1b2c3d4e5f6"));
        assert!(!entries[0].content.contains("uid:"));
        assert!(entries[0].content.contains("Body first."));
    }

    #[test]
    fn test_prose_cannot_hijack_a_uid() {
        // Only a uid-shaped value is taken from below the block, so documentation
        // that mentions the field keeps its line and the entry keeps no uid.
        let md = "---\ncategory: features\n---\n\n# Doc\n\n## Entry: Prose\n\nWrite it as:\n\nuid: see the table below\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uid, None);
        assert!(entries[0].content.contains("uid: see the table below"));
    }

    #[test]
    fn test_keywords_line_with_trailing_prose_stays_in_the_content() {
        // `keywords: [auth] and more` is not a clean metadata line. Treating it as one
        // would silently delete " and more" from the entry.
        let md = "---\ncategory: features\n---\n\n# Doc\n\n## Entry: Trailing prose\nkeywords: [auth] and more\n\nBody.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0].content.contains("keywords: [auth] and more"),
            "content: {}",
            entries[0].content
        );
    }

    #[test]
    fn test_uppercase_uid_below_the_block_is_honored() {
        // The old parser accepted any uid value, so an uppercase-hex one written by
        // hand must keep identifying its entry — verbatim, not lowercased.
        let md = "---\ncategory: features\n---\n\n# Doc\n\n## Entry: Upper\n\nBody first.\n\nuid: A1B2C3D4E5F6\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uid.as_deref(), Some("A1B2C3D4E5F6"));
    }

    #[test]
    fn test_malformed_metadata_lines_stay_in_the_content() {
        // A line that looks like metadata but parses to nothing must not be eaten:
        // `keywords:` needs brackets, and a bare `project:` has no value.
        let md = "---\ncategory: features\n---\n\n# Doc\n\n## Entry: Malformed\nkeywords: foo\nproject:\n\nBody.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].project, None);
        assert!(
            entries[0].content.contains("keywords: foo"),
            "content: {}",
            entries[0].content
        );
        assert!(entries[0].content.contains("project:"));
    }

    #[test]
    fn test_parse_multiple_entries() {
        let md = "---\nkeywords: [base]\ncategory: features\n---\n\n# Title\n\n## Entry: First\n\nFirst content.\n\n## Entry: Second\nkeywords: [extra]\n\nSecond content.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First");
        assert_eq!(entries[1].title, "Second");
        assert!(entries[1].keywords.contains(&"extra".to_string()));
        assert!(entries[1].keywords.contains(&"base".to_string()));
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let md = "# Simple Title\n\nSome content here.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Simple Title");
        assert_eq!(entries[0].category, "");
        assert!(entries[0].keywords.is_empty());
    }

    #[test]
    fn test_parse_empty_body() {
        let md = "---\nkeywords: [test]\n---\n\n";
        let entries = parse_md_entries(md);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_no_heading() {
        let md = "---\nkeywords: [test]\n---\n\nJust some text without a heading.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Untitled");
    }

    #[test]
    fn test_parse_entry_inline_keywords_merged() {
        let md =
            "---\nkeywords: [file-kw]\n---\n\n## Entry: Test\nkeywords: [inline-kw]\n\nContent.\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].keywords.contains(&"file-kw".to_string()));
        assert!(entries[0].keywords.contains(&"inline-kw".to_string()));
        // Content should not contain the keywords line
        assert!(!entries[0].content.contains("keywords:"));
    }

    #[test]
    fn test_parse_malformed_frontmatter() {
        // Missing closing ---
        let md = "---\nkeywords: [test]\n\n# Title\n\nContent.\n";
        let entries = parse_md_entries(md);
        // Should not crash; treats entire text as body
        assert!(!entries.is_empty());
    }

    #[test]
    fn test_file_hash() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "hello world").unwrap();
        let hash = file_hash(tmp.path()).unwrap();
        // SHA256 of "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_parse_japanese_keywords() {
        let md = "---\nkeywords: [認証, ログイン]\ncategory: features\n---\n\n# テスト\n\nコンテンツ。\n";
        let entries = parse_md_entries(md);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].keywords.contains(&"認証".to_string()));
        assert!(entries[0].keywords.contains(&"ログイン".to_string()));
    }
}
