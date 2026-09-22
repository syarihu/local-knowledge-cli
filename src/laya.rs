//! Client for the Laya MLX typed decision sidecar server.
//!
//! Connects via Unix domain socket (default: `~/.cache/lk/laya.sock`).
//! If the daemon is not running and Laya is enabled, it automatically spawns
//! the Python server (`scripts/laya_server.py`) using `uv run`.
//! If the server cannot be reached, times out, or errors, operations fail open
//! (returning `None` or failing over to standard heuristics) so core commands
//! never crash.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::LayaConfig;

#[allow(dead_code)]
pub const DEFAULT_SOCKET_NAME: &str = "laya.sock";

/// Embedded server script content so `lk` can unpack and run it even when
/// installed without the repository source directory.
#[allow(dead_code)]
pub const EMBEDDED_SERVER_SCRIPT: &str = include_str!("../scripts/laya_server.py");

#[derive(Debug, Clone, Deserialize)]
pub struct DuplicateResult {
    pub is_duplicate: bool,
    #[allow(dead_code)]
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerankCandidate {
    pub id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerankScore {
    pub id: String,
    pub score: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CategorizeResult {
    pub category: String,
    #[allow(dead_code)]
    pub confidence: f64,
    #[allow(dead_code)]
    pub probabilities: HashMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct KeywordItem {
    keyword: String,
    score: f64,
}

#[derive(Debug, Deserialize)]
struct FilterKeywordsResponse {
    ranked_keywords: Vec<KeywordItem>,
}

#[derive(Debug, Deserialize)]
struct RerankResponse {
    scores: Vec<RerankScore>,
}

pub struct LayaClient {
    #[cfg(unix)]
    writer: UnixStream,
    #[cfg(unix)]
    reader: BufReader<UnixStream>,
    #[allow(dead_code)]
    socket_path: PathBuf,
    #[allow(dead_code)]
    broken: bool,
}

#[allow(dead_code)]
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[allow(dead_code)]
impl LayaClient {
    /// Connect to an existing Laya daemon, or spawn one if enabled and missing.
    /// Returns `None` if disabled, unsupported, or if connection/spawning fails
    /// (providing transparent graceful degradation).
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub fn connect(config: &LayaConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        let socket_path = resolve_socket_path(config);

        // 1. Try connecting to an already-running daemon.
        if let Some((mut client, model_matches)) = Self::try_connect(&socket_path, &config.model) {
            if model_matches {
                return Some(client);
            }
            // Model mismatch: shut down the outdated daemon
            let _ = client.send_request("shutdown", json!({}));
            let pid_path = socket_path.with_extension("pid");
            if let Ok(content) = std::fs::read_to_string(&pid_path)
                && let Ok(pid) = content.trim().parse::<u32>()
            {
                let wait_start = Instant::now();
                while is_process_alive(pid) && wait_start.elapsed() < Duration::from_millis(1000) {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }

        // 2. Not running (or shut down): attempt to spawn the daemon in the background.
        if Self::spawn_daemon(config, &socket_path) {
            // Poll for socket readiness up to 5 seconds.
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(150));
                if let Some((client, model_matches)) =
                    Self::try_connect(&socket_path, &config.model)
                    && model_matches
                {
                    return Some(client);
                }
            }
        }

        None
    }

    /// On platforms other than macOS Apple Silicon, Laya is unsupported.
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    pub fn connect(_config: &LayaConfig) -> Option<Self> {
        None
    }

    /// Try connecting to the socket and verifying with a quick ping and model check.
    #[allow(dead_code)]
    fn try_connect(path: &Path, expected_model: &str) -> Option<(Self, bool)> {
        #[cfg(unix)]
        {
            let stream = UnixStream::connect(path).ok()?;
            let _ = stream.set_read_timeout(Some(Duration::from_millis(3000)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(3000)));
            let reader_stream = stream.try_clone().ok()?;
            let mut client = Self {
                writer: stream,
                reader: BufReader::new(reader_stream),
                socket_path: path.to_path_buf(),
                broken: false,
            };
            let ping_res = client.ping().ok()?;
            let model_matches = ping_res
                .get("model")
                .and_then(|m| m.as_str())
                .map(|m| m == expected_model)
                .unwrap_or(true);
            Some((client, model_matches))
        }
        #[cfg(not(unix))]
        {
            let _ = (path, expected_model);
            None
        }
    }

    /// Spawn the Python daemon process using `uv run`.
    #[allow(dead_code)]
    fn spawn_daemon(config: &LayaConfig, socket_path: &Path) -> bool {
        let pid_path = socket_path.with_extension("pid");
        if pid_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&pid_path)
                && let Ok(pid) = content.trim().parse::<u32>()
                && is_process_alive(pid)
            {
                // Daemon process is already alive; don't remove files or re-spawn
                return true;
            }
            let _ = std::fs::remove_file(&pid_path);
        }
        if socket_path.exists() {
            let _ = std::fs::remove_file(socket_path);
        }

        let script_path = match locate_or_extract_server_script() {
            Ok(p) => p,
            Err(_) => return false,
        };

        // Check if `uv` command is available
        if Command::new("uv")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let child = Command::new("uv")
                .args([
                    "run",
                    "--with",
                    "laya-mlx",
                    "python3",
                    script_path.to_str().unwrap_or(""),
                    "--socket-path",
                    socket_path.to_str().unwrap_or(""),
                    "--model",
                    &config.model,
                    "--idle-timeout",
                    &config.idle_timeout.to_string(),
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();

            child.is_ok()
        } else {
            false
        }
    }

    #[cfg(unix)]
    fn send_request(
        &mut self,
        task: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if self.broken {
            return Err("Laya socket is in a broken state from a previous error".to_string());
        }

        let req = json!({
            "id": 1,
            "task": task,
            "params": params,
        });
        let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        line.push('\n');

        if let Err(e) = self.writer.write_all(line.as_bytes()) {
            self.broken = true;
            return Err(format!("Failed to send to Laya socket: {e}"));
        }
        if let Err(e) = self.writer.flush() {
            self.broken = true;
            return Err(format!("Failed to flush Laya socket: {e}"));
        }

        let mut resp_line = String::new();
        if let Err(e) = self.reader.read_line(&mut resp_line) {
            self.broken = true;
            return Err(format!("Failed to read from Laya socket: {e}"));
        }
        if resp_line.is_empty() {
            self.broken = true;
            return Err("Laya socket closed by peer".to_string());
        }

        let resp: serde_json::Value = match serde_json::from_str(resp_line.trim()) {
            Ok(v) => v,
            Err(e) => {
                self.broken = true;
                return Err(format!("Failed to parse Laya response: {e}"));
            }
        };

        if let Some(err) = resp.get("error") {
            let msg = err.as_str().unwrap_or("Unknown server error");
            return Err(format!("Laya daemon error: {msg}"));
        }

        resp.get("result")
            .cloned()
            .ok_or_else(|| "Missing result in Laya response".to_string())
    }

    #[cfg(not(unix))]
    fn send_request(
        &mut self,
        _task: &str,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("Laya is only supported on Unix systems".to_string())
    }

    pub fn ping(&mut self) -> Result<serde_json::Value, String> {
        self.send_request("ping", json!({}))
    }

    pub fn check_duplicate(
        &mut self,
        entry_a_title: &str,
        entry_a_content: &str,
        entry_b_title: &str,
        entry_b_content: &str,
        threshold: f64,
    ) -> Result<DuplicateResult, String> {
        let params = json!({
            "entry_a": {
                "title": entry_a_title,
                "content": entry_a_content,
            },
            "entry_b": {
                "title": entry_b_title,
                "content": entry_b_content,
            },
            "threshold": threshold,
        });
        let res = self.send_request("duplicate", params)?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub fn filter_keywords(
        &mut self,
        title: &str,
        content: &str,
        candidates: &[String],
    ) -> Result<Vec<(String, f64)>, String> {
        let params = json!({
            "title": title,
            "content": content,
            "candidates": candidates,
        });
        let res = self.send_request("filter_keywords", params)?;
        let parsed: FilterKeywordsResponse =
            serde_json::from_value(res).map_err(|e| e.to_string())?;
        Ok(parsed
            .ranked_keywords
            .into_iter()
            .map(|item| (item.keyword, item.score))
            .collect())
    }

    pub fn rerank(
        &mut self,
        query: &str,
        candidates: &[RerankCandidate],
    ) -> Result<Vec<RerankScore>, String> {
        let params = json!({
            "query": query,
            "candidates": candidates,
        });
        let res = self.send_request("rerank", params)?;
        let parsed: RerankResponse = serde_json::from_value(res).map_err(|e| e.to_string())?;
        Ok(parsed.scores)
    }

    pub fn categorize(&mut self, title: &str, content: &str) -> Result<CategorizeResult, String> {
        let params = json!({
            "title": title,
            "content": content,
        });
        let res = self.send_request("categorize", params)?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }
}

/// Compute semantic rerank scores for candidates using Laya, if available.
/// Returns a map of candidate id -> semantic score (0.0 to 1.0).
pub fn compute_rerank_scores(
    query: &str,
    candidates: &[RerankCandidate],
    mut laya: Option<&mut LayaClient>,
) -> HashMap<String, f64> {
    if let Some(ref mut client) = laya
        && let Ok(scores) = client.rerank(query, candidates)
    {
        return scores.into_iter().map(|s| (s.id, s.score)).collect();
    }
    HashMap::new()
}

/// Report whether Laya is usable, for `lk stats` / MCP `get_stats`.
///
/// Only pings an already-running daemon — never spawns one — so checking the
/// status stays fast and does not load the model. A stopped daemon is normal:
/// it starts on the next command that needs it and exits after `idle_timeout`.
pub fn status(config: &LayaConfig) -> serde_json::Value {
    let supported = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let socket_path = resolve_socket_path(config);
    let running = if supported {
        LayaClient::try_connect(&socket_path, &config.model).map(|(mut client, _)| {
            client
                .ping()
                .ok()
                .and_then(|r| r.get("model").and_then(|m| m.as_str()).map(String::from))
        })
    } else {
        None
    };

    let mut obj = json!({
        "supported": supported,
        "enabled": config.enabled,
        "daemon": if running.is_some() { "running" } else { "stopped" },
        "model": config.model,
        "socket_path": socket_path.to_string_lossy(),
    });
    if let Some(Some(model)) = running {
        obj["daemon_model"] = json!(model);
    }
    obj
}

/// One-line human-readable summary of [`status`] for `lk stats`.
pub fn status_line(status: &serde_json::Value) -> String {
    if !status["supported"].as_bool().unwrap_or(false) {
        return "unsupported (macOS Apple Silicon only)".to_string();
    }
    let enabled = if status["enabled"].as_bool().unwrap_or(false) {
        "enabled"
    } else {
        "disabled"
    };
    match status["daemon"].as_str() {
        Some("running") => format!(
            "{enabled}, daemon running ({})",
            status["daemon_model"]
                .as_str()
                .or(status["model"].as_str())
                .unwrap_or("unknown model")
        ),
        _ if enabled == "enabled" => {
            format!("{enabled}, daemon stopped (starts on demand)")
        }
        _ => format!("{enabled}, daemon stopped"),
    }
}

/// Resolve the socket path to use, defaulting to `~/.cache/lk/laya.sock`.
#[allow(dead_code)]
pub fn resolve_socket_path(config: &LayaConfig) -> PathBuf {
    if let Some(ref p) = config.socket_path {
        p.clone()
    } else {
        crate::util::home_dir()
            .join(".cache")
            .join("lk")
            .join(DEFAULT_SOCKET_NAME)
    }
}

/// Locate server script via explicit environment override (`LK_LAYA_SERVER_SCRIPT`), or write
/// the embedded script to `~/.cache/lk/laya_server.py`.
#[allow(dead_code)]
fn locate_or_extract_server_script() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. Check explicit override via environment variable (e.g. for development)
    if let Ok(env_path) = std::env::var("LK_LAYA_SERVER_SCRIPT") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Extract embedded script into cache directory
    let cache_dir = crate::util::home_dir().join(".cache").join("lk");
    std::fs::create_dir_all(&cache_dir)?;
    let target = cache_dir.join("laya_server.py");
    let need_write = match std::fs::read_to_string(&target) {
        Ok(existing) => existing != EMBEDDED_SERVER_SCRIPT,
        Err(_) => true,
    };
    if need_write {
        std::fs::write(&target, EMBEDDED_SERVER_SCRIPT)?;
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_is_process_alive() {
        let my_pid = std::process::id();
        assert!(is_process_alive(my_pid));
        assert!(!is_process_alive(4_000_000));
    }

    #[test]
    #[cfg(not(unix))]
    fn test_is_process_alive_non_unix() {
        assert!(!is_process_alive(std::process::id()));
    }

    #[test]
    fn test_locate_or_extract_server_script() {
        let script = locate_or_extract_server_script().expect("script extracted");
        assert!(script.exists());
        let content = std::fs::read_to_string(&script).expect("readable script");
        assert_eq!(content, EMBEDDED_SERVER_SCRIPT);
    }

    #[test]
    fn test_status_does_not_spawn_when_socket_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = LayaConfig {
            enabled: true,
            socket_path: Some(dir.path().join("missing.sock")),
            ..LayaConfig::default()
        };
        let st = status(&cfg);
        assert_eq!(st["enabled"], json!(true));
        assert_eq!(st["daemon"], json!("stopped"));
        assert!(st.get("daemon_model").is_none());
        assert!(
            !dir.path().join("missing.pid").exists(),
            "status must not spawn the daemon"
        );
    }

    #[test]
    fn test_status_line() {
        let unsupported = json!({"supported": false, "enabled": true, "daemon": "stopped"});
        assert!(status_line(&unsupported).starts_with("unsupported"));

        let stopped = json!({"supported": true, "enabled": true, "daemon": "stopped"});
        assert_eq!(
            status_line(&stopped),
            "enabled, daemon stopped (starts on demand)"
        );

        let running = json!({
            "supported": true, "enabled": true, "daemon": "running",
            "model": "a/b", "daemon_model": "c/d",
        });
        assert_eq!(status_line(&running), "enabled, daemon running (c/d)");

        let disabled = json!({"supported": true, "enabled": false, "daemon": "stopped"});
        assert_eq!(status_line(&disabled), "disabled, daemon stopped");
    }

    #[test]
    fn test_resolve_socket_path() {
        let mut cfg = LayaConfig::default();
        let default_sock = resolve_socket_path(&cfg);
        assert!(default_sock.ends_with(DEFAULT_SOCKET_NAME));

        cfg.socket_path = Some(PathBuf::from("/tmp/custom.sock"));
        assert_eq!(resolve_socket_path(&cfg), PathBuf::from("/tmp/custom.sock"));
    }
}
