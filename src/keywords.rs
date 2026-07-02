use regex::Regex;
use std::collections::HashMap;

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
    "need", "dare", "ought", "used", "to", "of", "in", "for", "on", "with", "at", "by", "from",
    "as", "into", "through", "during", "before", "after", "above", "below", "between", "out",
    "off", "over", "under", "again", "further", "then", "once", "here", "there", "when", "where",
    "why", "how", "all", "each", "every", "both", "few", "more", "most", "other", "some", "such",
    "no", "nor", "not", "only", "own", "same", "so", "than", "too", "very", "just", "because",
    "but", "and", "or", "if", "while", "that", "this", "these", "those", "it", "its", "file",
    "true", "false", "null", "none",
];

/// Maximum number of auto-extracted keywords per entry. Full-text search already
/// covers the entire title/content, so keywords only need to be the terms that best
/// represent the entry — an uncapped dump of every word just adds noise to keyword
/// search and duplicate detection.
pub const MAX_AUTO_KEYWORDS: usize = 15;

/// A term occurring in the title counts this many times a content occurrence.
const TITLE_WEIGHT: u32 = 5;

/// Extra multiplier for tokens that come from file paths — path segments are
/// high-signal identifiers (module/file names) worth keeping over prose words.
const PATH_WEIGHT: u32 = 3;

/// Extract keywords from title and content, ranked by weighted frequency and
/// capped at `MAX_AUTO_KEYWORDS`. Title occurrences and file-path segments are
/// weighted higher than plain content words.
/// Only ASCII words and katakana are extracted (other Japanese keywords should
/// be specified manually).
pub fn extract_keywords(title: &str, content: &str) -> Vec<String> {
    let mut scores: HashMap<String, u32> = HashMap::new();
    score_text(title, TITLE_WEIGHT, &mut scores);
    score_text(content, 1, &mut scores);

    let mut ranked: Vec<(String, u32)> = scores.into_iter().collect();
    // Highest score first; alphabetical tie-break keeps output deterministic.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(MAX_AUTO_KEYWORDS);

    let mut result: Vec<String> = ranked.into_iter().map(|(kw, _)| kw).collect();
    result.sort();
    result
}

fn score_text(text: &str, weight: u32, scores: &mut HashMap<String, u32>) {
    // File path segments
    let path_re = Regex::new(r"[\w./\\-]+\.[\w]+").unwrap();
    for mat in path_re.find_iter(text) {
        for part in mat.as_str().split(&['/', '\\', '.'][..]) {
            add_score(scores, &part.to_lowercase(), weight * PATH_WEIGHT);
        }
    }

    // ASCII words
    let word_re = Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap();
    for mat in word_re.find_iter(text) {
        let word = mat.as_str();
        // CamelCase / snake_case parts
        let mut parts = Vec::new();
        for camel_part in split_camel_case(word) {
            for sub in camel_part.split('_') {
                if !sub.is_empty() {
                    parts.push(sub.to_lowercase());
                }
            }
        }
        for part in &parts {
            add_score(scores, part, weight);
        }
        // Keep the whole compound identifier too (e.g. "sessionmanager") —
        // compound names are often the most precise search handle.
        if parts.len() > 1 {
            add_score(scores, &word.to_lowercase(), weight);
        }
    }

    // Katakana words (4+ chars; the regex enforces the length)
    let katakana_re = Regex::new(r"[\u30A0-\u30FF]{4,}").unwrap();
    for mat in katakana_re.find_iter(text) {
        *scores.entry(mat.as_str().to_string()).or_insert(0) += weight;
    }
}

fn add_score(scores: &mut HashMap<String, u32>, word: &str, weight: u32) {
    if word.len() > 3 && !STOP_WORDS.contains(&word) {
        *scores.entry(word.to_string()).or_insert(0) += weight;
    }
}

fn split_camel_case(word: &str) -> Vec<String> {
    let re = Regex::new(r"([a-z])([A-Z])").unwrap();
    let spaced = re.replace_all(word, "$1 $2");
    spaced.split_whitespace().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case_extraction() {
        let kws = extract_keywords("SessionManager", "");
        assert!(kws.contains(&"session".to_string()));
        assert!(kws.contains(&"manager".to_string()));
        // The whole compound identifier is kept as well
        assert!(kws.contains(&"sessionmanager".to_string()));
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

    #[test]
    fn test_capped_at_max() {
        // 30 distinct candidate words — output must be capped
        let content: String = (0..30)
            .map(|i| format!("uniqueword{i:02}"))
            .collect::<Vec<_>>()
            .join(" ");
        let kws = extract_keywords("", &content);
        assert_eq!(kws.len(), MAX_AUTO_KEYWORDS);
    }

    #[test]
    fn test_title_words_survive_cap() {
        // Title words are weighted higher, so they must survive even when the
        // content has more candidates than the cap.
        let content: String = (0..30)
            .map(|i| format!("fillerterm{i:02}"))
            .collect::<Vec<_>>()
            .join(" ");
        let kws = extract_keywords("PaymentGateway retry policy", &content);
        assert!(kws.contains(&"payment".to_string()));
        assert!(kws.contains(&"gateway".to_string()));
        assert!(kws.contains(&"paymentgateway".to_string()));
        assert!(kws.contains(&"retry".to_string()));
        assert!(kws.contains(&"policy".to_string()));
        assert_eq!(kws.len(), MAX_AUTO_KEYWORDS);
    }

    #[test]
    fn test_frequent_words_outrank_singletons() {
        // A word repeated in the content should survive the cap over words that
        // appear only once.
        let mut content: String = (0..30)
            .map(|i| format!("noiseterm{i:02}"))
            .collect::<Vec<_>>()
            .join(" ");
        content.push_str(" webhook webhook webhook");
        let kws = extract_keywords("", &content);
        assert!(kws.contains(&"webhook".to_string()));
    }
}
