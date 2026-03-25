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
    pub superseded_by: Option<String>,
    pub supersedes: Vec<String>,
}

struct Frontmatter {
    keywords: Vec<String>,
    category: String,
    uid: Option<String>,
    status: Option<String>,
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
            superseded_by: None,
            supersedes: Vec::new(),
        };

        let kw_re =
            Regex::new(r"[\w\u{3040}-\u{309F}\u{30A0}-\u{30FF}\u{4E00}-\u{9FFF}-]+").unwrap();
        for line in fm_text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("keywords:") {
                for mat in kw_re.find_iter(rest) {
                    fm.keywords.push(mat.as_str().to_string());
                }
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
            } else if let Some(rest) = line.strip_prefix("superseded_by:") {
                let val = rest.trim().trim_matches('"').to_string();
                if !val.is_empty() {
                    fm.superseded_by = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("supersedes:") {
                for mat in kw_re.find_iter(rest) {
                    fm.supersedes.push(mat.as_str().to_string());
                }
            }
        }

        (fm, body)
    } else {
        (Frontmatter {
            keywords: Vec::new(),
            category: String::new(),
            uid: None,
            status: None,
            superseded_by: None,
            supersedes: Vec::new(),
        }, text)
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
                superseded_by: meta.superseded_by.or(fm.superseded_by.clone()),
                supersedes: if meta.supersedes.is_empty() { fm.supersedes.clone() } else { meta.supersedes },
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
    superseded_by: Option<String>,
    supersedes: Vec<String>,
}

/// Extract inline metadata (keywords, uid, status, superseded_by, supersedes) from entry content.
fn extract_entry_metadata(content: &str, file_kws: &[String]) -> (Vec<String>, String, EntryMeta) {
    let mut kws: Vec<String> = file_kws.to_vec();
    let mut cleaned = content.to_string();
    let mut meta = EntryMeta {
        uid: None,
        status: None,
        superseded_by: None,
        supersedes: Vec::new(),
    };

    let kw_re = Regex::new(r"(?m)^keywords:\s*\[(.+)\]").unwrap();
    if let Some(cap) = kw_re.captures(content) {
        let kw_str = cap.get(1).unwrap().as_str();
        for kw in kw_str.split(',') {
            let kw = kw.trim().to_string();
            if !kw.is_empty() && !kws.contains(&kw) {
                kws.push(kw);
            }
        }
        cleaned = kw_re.replace(&cleaned, "").trim().to_string();
    }

    // Extract uid: <value>
    let uid_re = Regex::new(r"(?m)^uid:\s*(.+)$").unwrap();
    if let Some(cap) = uid_re.captures(&cleaned) {
        let val = cap.get(1).unwrap().as_str().trim().trim_matches('"').to_string();
        if !val.is_empty() {
            meta.uid = Some(val);
        }
        cleaned = uid_re.replace(&cleaned, "").trim().to_string();
    }

    // Extract status: <value>
    let status_re = Regex::new(r"(?m)^status:\s*(.+)$").unwrap();
    if let Some(cap) = status_re.captures(&cleaned) {
        let val = cap.get(1).unwrap().as_str().trim().trim_matches('"').to_string();
        if !val.is_empty() {
            meta.status = Some(val);
        }
        cleaned = status_re.replace(&cleaned, "").trim().to_string();
    }

    // Extract superseded_by: <value>
    let sb_re = Regex::new(r"(?m)^superseded_by:\s*(.+)$").unwrap();
    if let Some(cap) = sb_re.captures(&cleaned) {
        let val = cap.get(1).unwrap().as_str().trim().trim_matches('"').to_string();
        if !val.is_empty() {
            meta.superseded_by = Some(val);
        }
        cleaned = sb_re.replace(&cleaned, "").trim().to_string();
    }

    // Extract supersedes: [uid1, uid2] or supersedes: uid1
    let ss_re = Regex::new(r"(?m)^supersedes:\s*(.+)$").unwrap();
    if let Some(cap) = ss_re.captures(&cleaned) {
        let val = cap.get(1).unwrap().as_str().trim();
        let val = val.trim_start_matches('[').trim_end_matches(']');
        for uid in val.split(',') {
            let uid = uid.trim().trim_matches('"').to_string();
            if !uid.is_empty() {
                meta.supersedes.push(uid);
            }
        }
        cleaned = ss_re.replace(&cleaned, "").trim().to_string();
    }

    (kws, cleaned, meta)
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
