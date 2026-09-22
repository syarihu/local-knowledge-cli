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
    stream: UnixStream,
    #[allow(dead_code)]
    socket_path: PathBuf,
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
        if let Some(client) = Self::try_connect(&socket_path) {
            return Some(client);
        }

        // 2. Not running: attempt to spawn the daemon in the background.
        if Self::spawn_daemon(config, &socket_path) {
            // Poll for socket readiness up to 5 seconds.
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(150));
                if let Some(client) = Self::try_connect(&socket_path) {
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

    /// Try connecting to the socket and verifying with a quick ping.
    fn try_connect(path: &Path) -> Option<Self> {
        #[cfg(unix)]
        {
            let stream = UnixStream::connect(path).ok()?;
            let _ = stream.set_read_timeout(Some(Duration::from_millis(3000)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(3000)));
            let mut client = Self {
                stream,
                socket_path: path.to_path_buf(),
            };
            if client.ping().is_ok() {
                Some(client)
            } else {
                None
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Spawn the Python daemon process using `uv run`.
    fn spawn_daemon(config: &LayaConfig, socket_path: &Path) -> bool {
        // Clean up any dead socket or pid file before starting
        if socket_path.exists() {
            let _ = std::fs::remove_file(socket_path);
        }
        let pid_path = socket_path.with_extension("pid");
        if pid_path.exists() {
            let _ = std::fs::remove_file(&pid_path);
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
        let req = json!({
            "id": 1,
            "task": task,
            "params": params,
        });
        let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        line.push('\n');

        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| format!("Failed to send to Laya socket: {e}"))?;
        self.stream
            .flush()
            .map_err(|e| format!("Failed to flush Laya socket: {e}"))?;

        let mut reader = BufReader::new(&self.stream);
        let mut resp_line = String::new();
        reader
            .read_line(&mut resp_line)
            .map_err(|e| format!("Failed to read from Laya socket: {e}"))?;

        let resp: serde_json::Value = serde_json::from_str(resp_line.trim())
            .map_err(|e| format!("Failed to parse Laya response: {e}"))?;

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

/// Locate `scripts/laya_server.py` in the workspace, or write the embedded script
/// to `~/.cache/lk/laya_server.py`.
#[allow(dead_code)]
fn locate_or_extract_server_script() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. Check relative to workspace or current dir
    let dev_script = PathBuf::from("scripts/laya_server.py");
    if dev_script.exists() {
        return Ok(dev_script);
    }

    // 2. Extract embedded script into cache directory
    let cache_dir = crate::util::home_dir().join(".cache").join("lk");
    std::fs::create_dir_all(&cache_dir)?;
    let target = cache_dir.join("laya_server.py");
    std::fs::write(&target, EMBEDDED_SERVER_SCRIPT)?;
    Ok(target)
}
