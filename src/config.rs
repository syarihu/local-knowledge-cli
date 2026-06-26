use std::path::{Path, PathBuf};

/// Parse a boolean config value leniently. Accepts `true/false`, `yes/no`, `1/0`,
/// `on/off` (case-insensitive). Unknown values keep `current` and emit a warning,
/// so a typo (e.g. `secret_detection = TRUE`) can't silently flip a safety setting
/// to its non-default (fail-open) state.
fn parse_bool(value: &str, key: &str, current: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => true,
        "false" | "no" | "0" | "off" => false,
        other => {
            eprintln!(
                "Warning: invalid boolean for `{key}` in lk config: {other:?} (keeping {current})"
            );
            current
        }
    }
}

/// Project-level configuration loaded from `.knowledge/config.toml`.
pub struct Config {
    /// Days before a shared entry is considered stale (default: 30)
    pub stale_threshold_days: i64,
    /// Days before a local entry is considered stale (default: 7)
    pub local_stale_threshold_days: i64,
    /// Default limit for `lk search` results (default: 5)
    pub search_default_limit: usize,
    /// Auto-sync .knowledge/ markdown files before read commands (default: true)
    pub auto_sync: bool,
    /// Detect potential secrets in content (default: true)
    pub secret_detection: bool,
    /// Enable command logging to .knowledge/command.log (default: false)
    pub command_log: bool,
    /// Mark .knowledge/**/*.md as linguist-generated in .gitattributes (default: true)
    pub gitattributes_generated: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stale_threshold_days: 30,
            local_stale_threshold_days: 7,
            search_default_limit: 5,
            auto_sync: true,
            secret_detection: true,
            command_log: false,
            gitattributes_generated: true,
        }
    }
}

impl Config {
    /// Return the stale threshold for the given entry source.
    pub fn stale_threshold_for(&self, source: &str) -> i64 {
        if source == "local" {
            self.local_stale_threshold_days
        } else {
            self.stale_threshold_days
        }
    }

    /// Load config from `.knowledge/config.toml`. Returns defaults if file doesn't exist.
    /// Environment variables override file values:
    /// - `LK_NO_AUTO_SYNC=1` → auto_sync = false
    /// - `LK_COMMAND_LOG=1` or `LK_SEARCH_LOG=1` → command_log = true
    pub fn load(knowledge_dir: &Path) -> Self {
        let mut config = Self::default();
        let config_path = knowledge_dir.join("config.toml");

        if let Ok(content) = std::fs::read_to_string(&config_path) {
            for line in content.lines() {
                let line = line.trim();
                // Skip comments and empty lines
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    match key {
                        "stale_threshold_days" => {
                            if let Ok(v) = value.parse::<i64>()
                                && v > 0
                            {
                                config.stale_threshold_days = v;
                            }
                        }
                        "local_stale_threshold_days" => {
                            if let Ok(v) = value.parse::<i64>()
                                && v > 0
                            {
                                config.local_stale_threshold_days = v;
                            }
                        }
                        "search_default_limit" => {
                            if let Ok(v) = value.parse::<usize>()
                                && v > 0
                            {
                                config.search_default_limit = v;
                            }
                        }
                        "auto_sync" => {
                            config.auto_sync = parse_bool(value, key, config.auto_sync);
                        }
                        "secret_detection" => {
                            config.secret_detection =
                                parse_bool(value, key, config.secret_detection);
                        }
                        "command_log" => {
                            config.command_log = parse_bool(value, key, config.command_log);
                        }
                        "gitattributes_generated" => {
                            config.gitattributes_generated =
                                parse_bool(value, key, config.gitattributes_generated);
                        }
                        _ => {} // Ignore unknown keys
                    }
                }
            }
        }

        // Environment variable overrides
        if std::env::var("LK_NO_AUTO_SYNC").unwrap_or_default() == "1" {
            config.auto_sync = false;
        }
        if std::env::var("LK_COMMAND_LOG").unwrap_or_default() == "1"
            || std::env::var("LK_SEARCH_LOG").unwrap_or_default() == "1"
        {
            config.command_log = true;
        }

        config
    }
}

/// Default content for `.knowledge/config.toml`.
pub const DEFAULT_CONFIG_CONTENT: &str = "\
# lk configuration
# This file is read by lk commands. Environment variables override these values.

# Days before a shared entry is considered stale (default: 30)
stale_threshold_days = 30

# Days before a local entry is considered stale (default: 7)
local_stale_threshold_days = 7

# Default limit for `lk search` results (default: 5)
search_default_limit = 5

# Auto-sync .knowledge/ markdown files before read commands (default: true)
# Override with LK_NO_AUTO_SYNC=1
auto_sync = true

# Detect potential secrets in content when adding/exporting entries (default: true)
secret_detection = true

# Enable command logging to .knowledge/command.log (default: false)
# Override with LK_COMMAND_LOG=1
command_log = false

# Mark .knowledge/**/*.md as linguist-generated in .gitattributes (default: true)
# Set to false to show full diffs for .knowledge/**/*.md in GitHub PRs
gitattributes_generated = true

# Category templates: Place markdown files in .knowledge/templates/
# e.g., .knowledge/templates/decisions.md will be used as default content
# when running `lk add \"Title\" --category decisions` without --content
";

/// Global (user-scope) configuration loaded from `~/.config/lk/config.toml`.
///
/// This is the first global config file for lk. It governs the user-scope
/// markdown store used by `lk export --scope user` / `lk sync --scope user`.
/// (`~/.config/lk/config.json` remains install/update metadata only.)
pub struct GlobalConfig {
    /// Directory holding user-scope markdown (the source of truth for user knowledge).
    /// Default: `~/.config/lk/knowledge`. Can point at a dotfiles repo path.
    pub user_knowledge_dir: PathBuf,
    /// Detect potential secrets when exporting user-scope entries (default: true).
    pub secret_detection: bool,
}

impl GlobalConfig {
    fn default_with_home(home: &Path) -> Self {
        Self {
            user_knowledge_dir: home.join(".config").join("lk").join("knowledge"),
            secret_detection: true,
        }
    }

    /// Load config from `~/.config/lk/config.toml`. Returns defaults if the file
    /// doesn't exist. Paths are resolved to absolute (see [`resolve_path`]).
    pub fn load() -> Self {
        let home = crate::util::home_dir();
        Self::load_from(&home.join(".config").join("lk").join("config.toml"), &home)
    }

    /// Load from an explicit config path, resolving relative/`~` paths against `home`.
    /// Exposed for testing.
    pub fn load_from(config_path: &Path, home: &Path) -> Self {
        let mut config = Self::default_with_home(home);

        if let Ok(content) = std::fs::read_to_string(config_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    // Strip optional surrounding quotes from the value.
                    let value = value.trim().trim_matches('"').trim_matches('\'').trim();
                    match key {
                        "user_knowledge_dir" => {
                            if !value.is_empty() {
                                config.user_knowledge_dir = resolve_path(value, home);
                            }
                        }
                        "secret_detection" => {
                            config.secret_detection =
                                parse_bool(value, key, config.secret_detection);
                        }
                        _ => {} // Ignore unknown keys
                    }
                }
            }
        }

        config
    }
}

/// Resolve a config path value to an absolute path:
/// - absolute paths are used as-is
/// - `~` / `~/...` expands against `home`
/// - other relative paths are resolved against `home`
fn resolve_path(value: &str, home: &Path) -> PathBuf {
    // `..` components can escape HOME into unintended locations; warn (it's the
    // user's own config, so don't hard-fail) and recommend an absolute or `~/` path.
    if Path::new(value)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        eprintln!(
            "Warning: `user_knowledge_dir` contains `..` ({value:?}); \
             prefer an absolute path or one under `~/` to avoid writing outside your home."
        );
    }
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home.join(rest);
    }
    let p = Path::new(value);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        home.join(p)
    }
}

/// Default content for `~/.config/lk/config.toml`.
pub const DEFAULT_GLOBAL_CONFIG_CONTENT: &str = "\
# lk global configuration (user scope)
# Governs the user-scope markdown store (`lk export/sync --scope user`).

# Directory holding user-scope markdown — the source of truth for user knowledge.
# Default: ~/.config/lk/knowledge. Point this at a dotfiles repo to version it.
# Use an absolute path or one under ~/ (avoid `..`).
# e.g. user_knowledge_dir = ~/dotfiles/lk-knowledge
# user_knowledge_dir = ~/.config/lk/knowledge

# Detect potential secrets when exporting user-scope entries (default: true).
secret_detection = true
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.stale_threshold_days, 30);
        assert_eq!(config.local_stale_threshold_days, 7);
        assert_eq!(config.search_default_limit, 5);
        assert!(config.auto_sync);
        assert!(config.secret_detection);
        assert!(!config.command_log);
        assert!(config.gitattributes_generated);
    }

    #[test]
    fn test_load_missing_file() {
        let dir = TempDir::new().unwrap();
        let config = Config::load(dir.path());
        assert_eq!(config.stale_threshold_days, 30);
        assert_eq!(config.local_stale_threshold_days, 7);
    }

    #[test]
    fn test_load_custom_values() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "stale_threshold_days = 60\nlocal_stale_threshold_days = 7\nsearch_default_limit = 10\nauto_sync = false\ncommand_log = true\n",
        )
        .unwrap();
        let config = Config::load(dir.path());
        assert_eq!(config.stale_threshold_days, 60);
        assert_eq!(config.local_stale_threshold_days, 7);
        assert_eq!(config.search_default_limit, 10);
        assert!(!config.auto_sync);
        assert!(config.command_log);
    }

    #[test]
    fn test_load_with_comments() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "# comment\nstale_threshold_days = 60\n\n# another comment\n",
        )
        .unwrap();
        let config = Config::load(dir.path());
        assert_eq!(config.stale_threshold_days, 60);
        assert_eq!(config.local_stale_threshold_days, 7); // default
        assert_eq!(config.search_default_limit, 5); // default
    }

    #[test]
    fn test_invalid_values_use_defaults() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "stale_threshold_days = -1\nsearch_default_limit = 0\nlocal_stale_threshold_days = 0\n",
        )
        .unwrap();
        let config = Config::load(dir.path());
        assert_eq!(config.stale_threshold_days, 30); // default because -1 <= 0
        assert_eq!(config.local_stale_threshold_days, 7); // default because 0 <= 0
        assert_eq!(config.search_default_limit, 5); // default because 0 <= 0
    }

    #[test]
    fn test_stale_threshold_for_source() {
        let config = Config::default();
        assert_eq!(config.stale_threshold_for("local"), 7);
        assert_eq!(config.stale_threshold_for("shared"), 30);
        assert_eq!(config.stale_threshold_for("unknown"), 30);
    }

    #[test]
    fn test_global_config_defaults_when_missing() {
        let home = TempDir::new().unwrap();
        let cfg = GlobalConfig::load_from(&home.path().join("config.toml"), home.path());
        assert_eq!(
            cfg.user_knowledge_dir,
            home.path().join(".config").join("lk").join("knowledge")
        );
        assert!(cfg.secret_detection);
    }

    #[test]
    fn test_global_config_custom_values() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");
        std::fs::write(
            &config_path,
            "# global\nuser_knowledge_dir = /abs/path/knowledge\nsecret_detection = false\n",
        )
        .unwrap();
        let cfg = GlobalConfig::load_from(&config_path, home.path());
        assert_eq!(cfg.user_knowledge_dir, PathBuf::from("/abs/path/knowledge"));
        assert!(!cfg.secret_detection);
    }

    #[test]
    fn test_global_config_invalid_bool_keeps_default() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");
        // An invalid value must NOT silently flip secret_detection off (fail-open).
        std::fs::write(&config_path, "secret_detection = maybe\n").unwrap();
        let cfg = GlobalConfig::load_from(&config_path, home.path());
        assert!(
            cfg.secret_detection,
            "invalid bool should keep default true"
        );
    }

    #[test]
    fn test_global_config_bool_is_case_insensitive() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");
        std::fs::write(&config_path, "secret_detection = FALSE\n").unwrap();
        let cfg = GlobalConfig::load_from(&config_path, home.path());
        assert!(!cfg.secret_detection, "FALSE should parse as false");
    }

    #[test]
    fn test_parse_bool_accepts_common_forms() {
        assert!(parse_bool("yes", "k", false));
        assert!(parse_bool("1", "k", false));
        assert!(parse_bool("ON", "k", false));
        assert!(!parse_bool("no", "k", true));
        assert!(!parse_bool("0", "k", true));
        // Unknown keeps the provided current value.
        assert!(parse_bool("weird", "k", true));
        assert!(!parse_bool("weird", "k", false));
    }

    #[test]
    fn test_global_config_tilde_and_relative_expansion() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");

        std::fs::write(&config_path, "user_knowledge_dir = ~/dotfiles/lk\n").unwrap();
        let cfg = GlobalConfig::load_from(&config_path, home.path());
        assert_eq!(cfg.user_knowledge_dir, home.path().join("dotfiles/lk"));

        std::fs::write(&config_path, "user_knowledge_dir = \"rel/knowledge\"\n").unwrap();
        let cfg = GlobalConfig::load_from(&config_path, home.path());
        assert_eq!(cfg.user_knowledge_dir, home.path().join("rel/knowledge"));
    }
}
