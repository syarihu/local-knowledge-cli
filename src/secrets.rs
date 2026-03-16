use regex::Regex;

pub struct SecretMatch {
    pub pattern_name: &'static str,
    pub matched: String,
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
                let matched = m.as_str().to_string();
                // Truncate long matches for display
                let display = if matched.len() > 40 {
                    format!("{}...", &matched[..40])
                } else {
                    matched
                };
                matches.push(SecretMatch {
                    pattern_name: name,
                    matched: display,
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
    fn test_detect_private_key() {
        let matches = check_for_secrets("-----BEGIN RSA PRIVATE KEY-----\nblah\n-----END RSA PRIVATE KEY-----");
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
        let matches = check_for_secrets("The API uses JWT tokens for authentication. Rate limit is 100 req/min.");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_no_false_positive_on_env_var_reference() {
        let matches = check_for_secrets("Set AUTH_TOKEN environment variable before running the app");
        assert!(matches.is_empty());
    }
}
