use regex::Regex;

pub struct SecretMatch {
    pub pattern_name: &'static str,
    /// A REDACTED preview of the match (a short prefix + `***`), safe to print or emit
    /// in JSON. Never holds the raw secret — the warning must not leak what it detects.
    pub matched: String,
}

/// Redact a detected secret to a short, non-reconstructable preview: a recognizable
/// prefix (e.g. `AKIA`, `ghp_`) plus `***`, with a fixed trailing marker so the length
/// isn't revealed. The prefix is capped to at most `len - 1` characters so the full
/// value is never shown — even for very short matches (`"abc"` → `"ab***"`, `"a"` → `"***"`).
fn redact(raw: &str) -> String {
    let len = raw.chars().count();
    let show = len.saturating_sub(1).min(4);
    let prefix: String = raw.chars().take(show).collect();
    format!("{prefix}***")
}

/// Check text for potential secrets. Returns a list of matches.
pub fn check_for_secrets(text: &str) -> Vec<SecretMatch> {
    let patterns: &[(&str, &str)] = &[
        // API keys / tokens with specific prefixes
        ("OpenAI API key", r"sk-[a-zA-Z0-9]{20,}"),
        ("GitHub PAT", r"ghp_[a-zA-Z0-9]{36}"),
        ("GitHub OAuth", r"gho_[a-zA-Z0-9]{36}"),
        ("GitHub App token", r"(?:ghu|ghs|ghr)_[a-zA-Z0-9]{36}"),
        ("AWS Access Key ID", r"AKIA[0-9A-Z]{16}"),
        ("Slack token", r"xox[bpors]-[a-zA-Z0-9\-]{10,}"),
        ("Stripe key", r"(?:sk|pk)_(?:test|live)_[a-zA-Z0-9]{20,}"),
        // Private keys
        (
            "Private key",
            r"-----BEGIN\s+(?:RSA\s+|EC\s+|DSA\s+|OPENSSH\s+)?PRIVATE\s+KEY-----",
        ),
        // Generic patterns (key=value assignments with suspicious names)
        (
            "Generic secret assignment",
            r#"(?i)(?:api[_-]?key|api[_-]?secret|secret[_-]?key|access[_-]?token|auth[_-]?token|private[_-]?key|password|passwd)\s*[:=]\s*['"]?[a-zA-Z0-9/+_.=-]{8,}['"]?"#,
        ),
    ];

    let mut matches = Vec::new();
    for (name, pattern) in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for m in re.find_iter(text) {
                matches.push(SecretMatch {
                    pattern_name: name,
                    // Store a REDACTED preview, never the raw match: this value is printed
                    // to stderr / emitted in JSON, so showing the real secret would leak it
                    // into terminal/CI logs even though we block the operation.
                    matched: redact(m.as_str()),
                });
            }
        }
    }
    matches
}

/// Format secret matches as a warning message.
pub fn format_warning(matches: &[SecretMatch]) -> String {
    let mut msg = String::from("Potential secrets detected in content:\n");
    for m in matches {
        msg.push_str(&format!("  - {}: {}\n", m.pattern_name, m.matched));
    }
    msg.push_str("\nUse --allow-secrets to override this check.");
    msg
}

/// Format secret matches as JSON warning objects for CLI (`--json`) and MCP replies.
pub fn warnings_json(matches: &[SecretMatch]) -> Vec<serde_json::Value> {
    matches
        .iter()
        .map(|m| {
            serde_json::json!({
                "pattern": m.pattern_name,
                "matched": m.matched,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_openai_key() {
        let matches = check_for_secrets("key is sk-abcdefghijklmnopqrstuvwxyz");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_name, "OpenAI API key");
    }

    #[test]
    fn test_detect_github_pat() {
        let matches = check_for_secrets("token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_name, "GitHub PAT");
    }

    #[test]
    fn test_detect_aws_key() {
        let matches = check_for_secrets("aws key is AKIAIOSFODNN7EXAMPLE");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_name, "AWS Access Key ID");
    }

    #[test]
    fn test_match_is_redacted_not_leaked() {
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let matches = check_for_secrets(&format!("aws key is {secret}"));
        assert!(!matches.is_empty());
        // The stored/printed value must not contain the full secret.
        assert!(
            !matches[0].matched.contains(secret),
            "redacted value must not leak the raw secret: {:?}",
            matches[0].matched
        );
        assert!(matches[0].matched.ends_with("***"));
        // The warning text built from matches is likewise safe.
        assert!(!format_warning(&matches).contains(secret));
    }

    #[test]
    fn test_redact_never_returns_full_input() {
        // Even short inputs must not be shown whole (at least one char hidden).
        for raw in ["a", "ab", "abc", "abcd", "abcdefghij"] {
            let r = redact(raw);
            assert!(r.ends_with("***"));
            let shown = r.trim_end_matches("***");
            assert!(
                shown.len() < raw.len(),
                "redact({raw:?}) = {r:?} revealed the whole input"
            );
        }
        assert_eq!(redact("a"), "***");
    }

    #[test]
    fn test_detect_private_key() {
        let matches = check_for_secrets(
            "-----BEGIN RSA PRIVATE KEY-----\nblah\n-----END RSA PRIVATE KEY-----",
        );
        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_name, "Private key");
    }

    #[test]
    fn test_detect_generic_secret() {
        let matches = check_for_secrets("api_key=abc123defghij456");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_name, "Generic secret assignment");
    }

    #[test]
    fn test_no_false_positive_on_normal_text() {
        let matches = check_for_secrets(
            "The API uses JWT tokens for authentication. Rate limit is 100 req/min.",
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn test_no_false_positive_on_env_var_reference() {
        let matches =
            check_for_secrets("Set AUTH_TOKEN environment variable before running the app");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_warnings_json_format() {
        let matches = check_for_secrets("aws key is AKIAIOSFODNN7EXAMPLE");
        assert!(!matches.is_empty());
        let json = warnings_json(&matches);
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["pattern"], "AWS Access Key ID");
        assert_eq!(json[0]["matched"], "AKIA***");
    }
}
