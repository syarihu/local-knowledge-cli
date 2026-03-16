use std::path::Path;

/// Project-level configuration loaded from `.knowledge/config.toml`.
pub struct Config {
    /// Days before a shared entry is considered stale (default: 30)
    pub stale_threshold_days: i64,
    /// Days before a local entry is considered stale (default: 14)
    pub local_stale_threshold_days: i64,
    /// Default limit for `lk search` results (default: 5)
    pub search_default_limit: usize,
    /// Auto-sync .knowledge/ markdown files before read commands (default: true)
    pub auto_sync: bool,
    /// Detect potential secrets in content (default: true)
    pub secret_detection: bool,
    /// Enable command logging to .knowledge/command.log (default: false)
    pub command_log: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stale_threshold_days: 30,
            local_stale_threshold_days: 14,
            search_default_limit: 5,
            auto_sync: true,
            secret_detection: true,
            command_log: false,
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
                            config.auto_sync = value == "true";
                        }
                        "secret_detection" => {
                            config.secret_detection = value == "true";
                        }
                        "command_log" => {
                            config.command_log = value == "true";
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

# Days before a local entry is considered stale (default: 14)
local_stale_threshold_days = 14

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
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.stale_threshold_days, 30);
        assert_eq!(config.local_stale_threshold_days, 14);
        assert_eq!(config.search_default_limit, 5);
        assert!(config.auto_sync);
        assert!(config.secret_detection);
        assert!(!config.command_log);
    }

    #[test]
    fn test_load_missing_file() {
        let dir = TempDir::new().unwrap();
        let config = Config::load(dir.path());
        assert_eq!(config.stale_threshold_days, 30);
        assert_eq!(config.local_stale_threshold_days, 14);
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
        assert_eq!(config.local_stale_threshold_days, 14); // default
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
        assert_eq!(config.local_stale_threshold_days, 14); // default because 0 <= 0
        assert_eq!(config.search_default_limit, 5); // default because 0 <= 0
    }

    #[test]
    fn test_stale_threshold_for_source() {
        let config = Config::default();
        assert_eq!(config.stale_threshold_for("local"), 14);
        assert_eq!(config.stale_threshold_for("shared"), 30);
        assert_eq!(config.stale_threshold_for("unknown"), 30);
    }
}
