use std::path::{Path, PathBuf};

/// Parse a boolean config value leniently. Accepts `true/false`, `yes/no`, `1/0`,
/// `on/off` (case-insensitive, so `TRUE` and `False` are valid). An unrecognized
/// value (e.g. `secret_detection = treu`) keeps `current` and emits a warning, so a
/// typo can't silently flip a safety setting to its non-default (fail-open) state.
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
    /// Add the lk-instructions import to CLAUDE.md (default: true).
    /// Set to false when agents receive the instructions over the lk-knowledge MCP
    /// server instead, which serves the same content in its `initialize` response.
    pub claude_md_import: bool,
    /// Laya MLX integration settings.
    pub laya: LayaConfig,
}

/// Configuration for Laya MLX integration (typed decisions on Apple Silicon).
#[derive(Debug, Clone)]
pub struct LayaConfig {
    /// Whether Laya integration is enabled (default: false, opt-in).
    pub enabled: bool,
    /// Path to the Unix domain socket. Defaults to `~/.cache/lk/laya.sock`.
    pub socket_path: Option<PathBuf>,
    /// Seconds of inactivity before the background daemon auto-shuts down (default: 600).
    pub idle_timeout: u64,
    /// Hugging Face model repository or local path.
    pub model: String,
    /// Semantic duplicate detection probability threshold (default: 0.85).
    pub duplicate_threshold: f64,
}

impl Default for LayaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            socket_path: None,
            idle_timeout: 600,
            model: "aac6fef/laya-multilingual-mlx".to_string(),
            duplicate_threshold: 0.85,
        }
    }
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
            claude_md_import: true,
            laya: LayaConfig::default(),
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
        let global = GlobalConfig::load();
        Self::load_with_global(knowledge_dir, &global)
    }

    /// Load config from `.knowledge/config.toml`, inheriting `laya` from `global` as default.
    pub fn load_with_global(knowledge_dir: &Path, global: &GlobalConfig) -> Self {
        let mut config = Self {
            laya: global.laya.clone(),
            ..Self::default()
        };
        let config_path = knowledge_dir.join("config.toml");

        if let Ok(content) = std::fs::read_to_string(&config_path) {
            let mut section = String::new();
            for line in content.lines() {
                let line = line.trim();
                // Skip comments and empty lines
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with('[') && line.ends_with(']') {
                    section = line[1..line.len() - 1].trim().to_ascii_lowercase();
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"').trim_matches('\'').trim();
                    let full_key = if section.is_empty() {
                        key.to_string()
                    } else {
                        format!("{section}.{key}")
                    };
                    match full_key.as_str() {
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
                        "claude_md_import" => {
                            config.claude_md_import =
                                parse_bool(value, key, config.claude_md_import);
                        }
                        "laya.enabled" | "laya_enabled" => {
                            // Project config may only disable Laya (opt-out); it cannot
                            // enable Laya if it is disabled in the global user configuration.
                            if !parse_bool(value, key, true) {
                                config.laya.enabled = false;
                            }
                        }
                        "laya.duplicate_threshold" | "laya_duplicate_threshold" => {
                            if let Ok(v) = value.parse::<f64>()
                                && (0.0..=1.0).contains(&v)
                            {
                                config.laya.duplicate_threshold = v;
                            }
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
        if std::env::var("LK_NO_LAYA").unwrap_or_default() == "1"
            || std::env::var("LK_LAYA").unwrap_or_default() == "0"
        {
            config.laya.enabled = false;
        } else if std::env::var("LK_LAYA").unwrap_or_default() == "1" {
            config.laya.enabled = true;
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

# Add `@.knowledge/lk-instructions.md` to CLAUDE.md (default: true)
# Set to false when agents read the instructions from the lk-knowledge MCP server,
# which returns the same content in its `initialize` response; `lk init` then removes
# the import line instead of adding it. `lk init --no-import` sets this for you.
claude_md_import = true

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
    /// Add the lk-instructions import to `~/.claude/CLAUDE.md` (default: true).
    /// The user-scope counterpart of [`Config::claude_md_import`].
    pub claude_md_import: bool,
    /// Laya MLX integration settings.
    pub laya: LayaConfig,
}

impl GlobalConfig {
    fn default_with_home(home: &Path) -> Self {
        Self {
            user_knowledge_dir: home.join(".config").join("lk").join("knowledge"),
            secret_detection: true,
            claude_md_import: true,
            laya: LayaConfig::default(),
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
            let mut section = String::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with('[') && line.ends_with(']') {
                    section = line[1..line.len() - 1].trim().to_ascii_lowercase();
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    // Strip optional surrounding quotes from the value.
                    let value = value.trim().trim_matches('"').trim_matches('\'').trim();
                    let full_key = if section.is_empty() {
                        key.to_string()
                    } else {
                        format!("{section}.{key}")
                    };
                    match full_key.as_str() {
                        "user_knowledge_dir" => {
                            if !value.is_empty() {
                                config.user_knowledge_dir = resolve_path(value, home);
                            }
                        }
                        "secret_detection" => {
                            config.secret_detection =
                                parse_bool(value, key, config.secret_detection);
                        }
                        "claude_md_import" => {
                            config.claude_md_import =
                                parse_bool(value, key, config.claude_md_import);
                        }
                        "laya.enabled" | "laya_enabled" => {
                            config.laya.enabled = parse_bool(value, key, config.laya.enabled);
                        }
                        "laya.idle_timeout" | "laya_idle_timeout" => {
                            if let Ok(v) = value.parse::<u64>()
                                && v > 0
                            {
                                config.laya.idle_timeout = v;
                            }
                        }
                        "laya.model" | "laya_model" => {
                            if !value.is_empty() {
                                config.laya.model = value.to_string();
                            }
                        }
                        "laya.socket_path" | "laya_socket_path" => {
                            if !value.is_empty() {
                                config.laya.socket_path = Some(resolve_path(value, home));
                            }
                        }
                        "laya.duplicate_threshold" | "laya_duplicate_threshold" => {
                            if let Ok(v) = value.parse::<f64>()
                                && (0.0..=1.0).contains(&v)
                            {
                                config.laya.duplicate_threshold = v;
                            }
                        }
                        _ => {} // Ignore unknown keys
                    }
                }
            }
        }

        if std::env::var("LK_NO_LAYA").unwrap_or_default() == "1"
            || std::env::var("LK_LAYA").unwrap_or_default() == "0"
        {
            config.laya.enabled = false;
        } else if std::env::var("LK_LAYA").unwrap_or_default() == "1" {
            config.laya.enabled = true;
        }

        config
    }

    /// Path of the global config file (`~/.config/lk/config.toml`).
    pub fn path() -> PathBuf {
        crate::util::home_dir()
            .join(".config")
            .join("lk")
            .join("config.toml")
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
            "Warning: config path contains `..` ({value:?}); \
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

# Add `@lk-instructions.md` to ~/.claude/CLAUDE.md (default: true)
# Set to false when agents read the instructions from the lk-knowledge MCP server.
# `lk init --global --no-import` sets this for you.
claude_md_import = true
";

/// Set a boolean key in one of lk's line-based config files, rewriting the line when
/// the key is already present and appending it otherwise. `default_content` seeds the
/// file when it does not exist yet, so a `--no-import` run on a fresh project still
/// leaves a fully commented config behind.
///
/// Key matching mirrors [`Config::load`]: lines are trimmed, `#` comments are skipped,
/// and the key is whatever precedes the first `=`. Duplicate assignments of the same
/// key are dropped rather than left in place — the loader takes the last one it sees,
/// so a stale duplicate below the rewritten line would silently win.
pub fn set_config_bool(
    path: &Path,
    key: &str,
    value: bool,
    default_content: &str,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => default_content.to_string(),
        Err(e) => return Err(e),
    };

    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let is_assignment = !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key);
        if is_assignment {
            if !replaced {
                lines.push(format!("{key} = {value}"));
                replaced = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("{key} = {value}"));
    }

    // Keep whatever line endings the file already had: config.toml is git-tracked, so a
    // CRLF checkout rewritten to LF would turn one changed setting into a whole-file diff.
    // Same reason for the trailing newline — only re-add it when the original ended with
    // one, which is the rule the markdown rewrites in `cmd/init.rs` already follow.
    let newline = crate::util::detect_newline(&content);
    let mut out = lines.join(newline);
    if content.ends_with('\n') {
        out.push_str(newline);
    }
    std::fs::write(path, out)
}

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
        assert!(config.claude_md_import);
    }

    #[test]
    fn test_load_claude_md_import() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "claude_md_import = false\n").unwrap();
        assert!(!Config::load(dir.path()).claude_md_import);

        // An unparseable value must not flip the default off by accident.
        std::fs::write(dir.path().join("config.toml"), "claude_md_import = maybe\n").unwrap();
        assert!(Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_global_config_claude_md_import() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");
        assert!(GlobalConfig::load_from(&config_path, home.path()).claude_md_import);

        std::fs::write(&config_path, "claude_md_import = no\n").unwrap();
        assert!(!GlobalConfig::load_from(&config_path, home.path()).claude_md_import);
    }

    #[test]
    fn test_config_inherits_global_laya_config() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");
        std::fs::write(&config_path, "[laya]\nenabled = true\n").unwrap();
        let global = GlobalConfig::load_from(&config_path, home.path());
        assert!(global.laya.enabled);

        let project_dir = TempDir::new().unwrap();
        // Project without laya config inherits global laya
        let config = Config::load_with_global(project_dir.path(), &global);
        assert!(config.laya.enabled);

        // Project with laya.enabled = false overrides global laya
        std::fs::write(
            project_dir.path().join("config.toml"),
            "[laya]\nenabled = false\n",
        )
        .unwrap();
        let config = Config::load_with_global(project_dir.path(), &global);
        assert!(!config.laya.enabled);

        // Project config cannot force-enable laya if disabled globally,
        // and cannot override socket_path or model
        let disabled_global =
            GlobalConfig::load_from(&home.path().join("nonexistent.toml"), home.path());
        assert!(!disabled_global.laya.enabled);
        let untrusted_dir = TempDir::new().unwrap();
        std::fs::write(
            untrusted_dir.path().join("config.toml"),
            "[laya]\nenabled = true\nsocket_path = \"/tmp/attacker.sock\"\nmodel = \"attacker/model\"\n",
        )
        .unwrap();
        let config = Config::load_with_global(untrusted_dir.path(), &disabled_global);
        assert!(
            !config.laya.enabled,
            "project cannot force-enable Laya when globally disabled"
        );
        assert_eq!(
            config.laya.socket_path, None,
            "project cannot override socket_path"
        );
        assert_eq!(
            config.laya.model, disabled_global.laya.model,
            "project cannot override model"
        );
    }

    #[test]
    fn test_global_config_laya_socket_path_expansion() {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join("config.toml");
        std::fs::write(
            &config_path,
            "[laya]\nsocket_path = \"~/.cache/custom.sock\"\n",
        )
        .unwrap();
        let global = GlobalConfig::load_from(&config_path, home.path());
        assert_eq!(
            global.laya.socket_path,
            Some(home.path().join(".cache").join("custom.sock"))
        );
    }

    #[test]
    fn test_set_config_bool_rewrites_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# lk config\nauto_sync = true\nclaude_md_import = true\n",
        )
        .unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content, "# lk config\nauto_sync = true\nclaude_md_import = false\n",
            "only the target key changes"
        );
        assert!(!Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_set_config_bool_appends_missing_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "auto_sync = true\n").unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        assert!(!Config::load(dir.path()).claude_md_import);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("auto_sync = true"));
        assert!(content.ends_with("claude_md_import = false\n"));
    }

    #[test]
    fn test_set_config_bool_seeds_file_from_default_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        set_config_bool(&path, "claude_md_import", false, DEFAULT_CONFIG_CONTENT).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        // The rest of the default config survives, so the user still gets the comments.
        assert!(content.contains("stale_threshold_days = 30"));
        assert!(!Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_set_config_bool_drops_duplicate_assignments() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        // `Config::load` takes the last assignment it sees, so a leftover duplicate
        // below the rewritten line would win and undo the change.
        std::fs::write(
            &path,
            "claude_md_import = true\nauto_sync = true\nclaude_md_import = true\n",
        )
        .unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("claude_md_import").count(), 1);
        assert!(!Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_set_config_bool_preserves_missing_trailing_newline() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        // No trailing newline: adding one is churn unrelated to the setting being changed.
        std::fs::write(&path, "auto_sync = true\nclaude_md_import = true").unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "auto_sync = true\nclaude_md_import = false");
        assert!(!Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_set_config_bool_keeps_trailing_newline_when_present() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "auto_sync = true\nclaude_md_import = true\n").unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "auto_sync = true\nclaude_md_import = false\n");
    }

    #[test]
    fn test_set_config_bool_preserves_crlf() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        // config.toml is git-tracked, so a CRLF checkout must not come back as LF.
        std::fs::write(
            &path,
            "# lk config\r\nauto_sync = true\r\nclaude_md_import = true\r\n",
        )
        .unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content, "# lk config\r\nauto_sync = true\r\nclaude_md_import = false\r\n",
            "line endings must survive the rewrite"
        );
        assert!(!Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_set_config_bool_appends_with_crlf_when_file_uses_crlf() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "auto_sync = true\r\n").unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains('\n')
                || content.matches("\r\n").count() == content.matches('\n').count(),
            "appended lines must use CRLF too, got: {content:?}"
        );
        assert!(!Config::load(dir.path()).claude_md_import);
    }

    #[test]
    fn test_set_config_bool_ignores_commented_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# claude_md_import = true\n").unwrap();

        set_config_bool(&path, "claude_md_import", false, "").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("# claude_md_import = true"),
            "the comment is left as documentation, got:\n{content}"
        );
        assert!(content.contains("\nclaude_md_import = false\n"));
        assert!(!Config::load(dir.path()).claude_md_import);
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
