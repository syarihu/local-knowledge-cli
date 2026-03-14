use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct MdEntry {
    pub title: String,
    pub content: String,
    pub keywords: Vec<String>,
}

/// Parse YAML-ish frontmatter. Returns (frontmatter_keywords, category, body).
fn parse_frontmatter(text: &str) -> (Vec<String>, String, &str) {
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").unwrap();
    if let Some(cap) = re.captures(text) {
        let fm_text = cap.get(1).unwrap().as_str();
        let body = &text[cap.get(0).unwrap().end()..];

        let mut keywords = Vec::new();
        let mut category = String::new();

        for line in fm_text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("keywords:") {
                let kw_re = Regex::new(r"[\w\u{3040}-\u{309F}\u{30A0}-\u{30FF}\u{4E00}-\u{9FFF}-]+").unwrap();
                for mat in kw_re.find_iter(rest) {
                    keywords.push(mat.as_str().to_string());
                }
            } else if let Some(rest) = line.strip_prefix("category:") {
                category = rest.trim().to_string();
            }
        }

        (keywords, category, body)
    } else {
        (Vec::new(), String::new(), text)
    }
}

/// Parse markdown text into individual entries.
pub fn parse_md_entries(text: &str) -> Vec<MdEntry> {
    let (file_kws, _category, body) = parse_frontmatter(text);

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

        let (entry_kws, content) = extract_entry_keywords(&content, &file_kws);

        if !content.is_empty() {
            entries.push(MdEntry {
                title,
                content,
                keywords: entry_kws,
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
            let (entry_kws, content) = extract_entry_keywords(&raw_content, &file_kws);

            if !content.is_empty() {
                entries.push(MdEntry {
                    title,
                    content,
                    keywords: entry_kws,
                });
            }
        }
    }

    entries
}

/// Extract inline `keywords: [...]` from entry content, merge with file-level keywords.
fn extract_entry_keywords(content: &str, file_kws: &[String]) -> (Vec<String>, String) {
    let mut kws: Vec<String> = file_kws.to_vec();
    let mut cleaned = content.to_string();

    let kw_re = Regex::new(r"(?m)^keywords:\s*\[(.+)\]").unwrap();
    if let Some(cap) = kw_re.captures(content) {
        let kw_str = cap.get(1).unwrap().as_str();
        for kw in kw_str.split(',') {
            let kw = kw.trim().to_string();
            if !kw.is_empty() && !kws.contains(&kw) {
                kws.push(kw);
            }
        }
        cleaned = kw_re.replace(content, "").trim().to_string();
    }

    (kws, cleaned)
}

/// Compute SHA256 hash of a file.
pub fn file_hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    let hash = Sha256::digest(&data);
    Ok(hex::encode(hash))
}
