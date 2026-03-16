use regex::Regex;
use std::collections::HashSet;

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "shall", "can", "need", "dare", "ought",
    "used", "to", "of", "in", "for", "on", "with", "at", "by", "from",
    "as", "into", "through", "during", "before", "after", "above", "below",
    "between", "out", "off", "over", "under", "again", "further", "then",
    "once", "here", "there", "when", "where", "why", "how", "all", "each",
    "every", "both", "few", "more", "most", "other", "some", "such", "no",
    "nor", "not", "only", "own", "same", "so", "than", "too", "very",
    "just", "because", "but", "and", "or", "if", "while", "that", "this",
    "these", "those", "it", "its", "file", "true", "false", "null", "none",
];

/// Extract keywords from title and content.
/// Only extracts ASCII words (Japanese keywords should be specified manually).
pub fn extract_keywords(title: &str, content: &str) -> Vec<String> {
    let text = format!("{title} {content}");
    let mut keywords = HashSet::new();

    // Extract file path keywords
    extract_file_path_keywords(&text, &mut keywords);

    // Extract ASCII words
    let word_re = Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap();
    for mat in word_re.find_iter(&text) {
        let word = mat.as_str();
        // CamelCase split
        for part in split_camel_case(word) {
            // snake_case split
            for sub in part.split('_') {
                let lower = sub.to_lowercase();
                if lower.len() > 3 && !STOP_WORDS.contains(&lower.as_str()) {
                    keywords.insert(lower);
                }
            }
        }
    }

    // Extract katakana words (4+ chars)
    let katakana_re = Regex::new(r"[\u30A0-\u30FF]{4,}").unwrap();
    for mat in katakana_re.find_iter(&text) {
        keywords.insert(mat.as_str().to_string());
    }

    let mut result: Vec<String> = keywords.into_iter().collect();
    result.sort();
    result
}

fn split_camel_case(word: &str) -> Vec<String> {
    let re = Regex::new(r"([a-z])([A-Z])").unwrap();
    let spaced = re.replace_all(word, "$1 $2");
    spaced.split_whitespace().map(|s| s.to_string()).collect()
}

fn extract_file_path_keywords(text: &str, keywords: &mut HashSet<String>) {
    let path_re = Regex::new(r"[\w./\\-]+\.[\w]+").unwrap();
    for mat in path_re.find_iter(text) {
        let path = mat.as_str();
        for part in path.split(&['/', '\\', '.'][..]) {
            let lower = part.to_lowercase();
            if lower.len() > 3 && !STOP_WORDS.contains(&lower.as_str()) {
                keywords.insert(lower);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case_extraction() {
        let kws = extract_keywords("SessionManager", "");
        assert!(kws.contains(&"session".to_string()));
        assert!(kws.contains(&"manager".to_string()));
    }

    #[test]
    fn test_file_path_extraction() {
        let kws = extract_keywords("", "The file src/auth/session.ts handles tokens.");
        assert!(kws.contains(&"auth".to_string()));
        assert!(kws.contains(&"session".to_string()));
    }

    #[test]
    fn test_stop_words_excluded() {
        let kws = extract_keywords("", "this is the content with some words");
        assert!(!kws.contains(&"this".to_string()));
        assert!(!kws.contains(&"the".to_string()));
        assert!(!kws.contains(&"with".to_string()));
    }

    #[test]
    fn test_short_words_excluded() {
        let kws = extract_keywords("", "Go is a language by Rob Pike");
        // Words <= 3 chars should be excluded
        assert!(!kws.contains(&"go".to_string()));
        assert!(!kws.contains(&"rob".to_string()));
    }

    #[test]
    fn test_snake_case_extraction() {
        let kws = extract_keywords("get_user_session", "");
        assert!(kws.contains(&"user".to_string()));
        assert!(kws.contains(&"session".to_string()));
    }

    #[test]
    fn test_katakana_extraction() {
        let kws = extract_keywords("", "これはセッションマネージャーです");
        // 4+ char katakana should be extracted
        assert!(kws.contains(&"セッションマネージャー".to_string()));
    }

    #[test]
    fn test_empty_input() {
        let kws = extract_keywords("", "");
        assert!(kws.is_empty());
    }

    #[test]
    fn test_keywords_sorted() {
        let kws = extract_keywords("Zebra Apple", "Mango content");
        // Should be sorted
        let sorted = {
            let mut v = kws.clone();
            v.sort();
            v
        };
        assert_eq!(kws, sorted);
    }
}
