use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::cmd::maybe_auto_sync_for;
use crate::config::Config;
use crate::db;
use crate::similarity::Tier;
use crate::util;

// ── JSON-RPC 2.0 types ──────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ── ProjectRegistry ──────────────────────────────────────────────────

struct ProjectRegistry {
    projects: Vec<(String, PathBuf)>,
    legacy_mode: bool,
}

impl ProjectRegistry {
    fn from_paths(paths: Vec<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        if paths.is_empty() {
            return Ok(Self {
                projects: vec![],
                legacy_mode: true,
            });
        }

        let mut projects = Vec::new();
        let mut name_counts: HashMap<String, usize> = HashMap::new();

        for path in &paths {
            let canonical = std::fs::canonicalize(path)
                .map_err(|e| format!("Cannot resolve project path '{}': {e}", path.display()))?;
            // Note: we no longer require the project DB to exist here. Uninitialized
            // projects are allowed; reads/writes fall back to user scope, and an
            // explicit project operation surfaces the "run lk init" error at open time.
            let basename = canonical
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string();
            let count = name_counts.entry(basename.clone()).or_insert(0);
            *count += 1;
            let name = if *count > 1 {
                format!("{basename}-{count}")
            } else {
                basename
            };
            projects.push((name, canonical));
        }

        Ok(Self {
            projects,
            legacy_mode: false,
        })
    }

    fn resolve(&self, project_param: Option<&str>) -> Result<PathBuf, String> {
        if self.legacy_mode {
            return Ok(util::get_project_root());
        }

        match (self.projects.len(), project_param) {
            (1, None) => Ok(self.projects[0].1.clone()),
            (_, None) => {
                let names: Vec<&str> = self.projects.iter().map(|(n, _)| n.as_str()).collect();
                Err(format!(
                    "Multiple projects registered. Specify 'project' parameter. Available: {}",
                    names.join(", ")
                ))
            }
            (_, Some(name)) => self
                .projects
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, p)| p.clone())
                .ok_or_else(|| {
                    let names: Vec<&str> = self.projects.iter().map(|(n, _)| n.as_str()).collect();
                    format!("Unknown project: '{name}'. Available: {}", names.join(", "))
                }),
        }
    }

    fn project_names(&self) -> Vec<&str> {
        self.projects.iter().map(|(n, _)| n.as_str()).collect()
    }
}

// ── helpers ──────────────────────────────────────────────────────────

fn respond(id: Option<Value>, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn respond_err(id: Option<Value>, code: i64, msg: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: msg.to_string(),
        }),
    }
}

fn write_response(out: &mut impl Write, resp: &JsonRpcResponse) {
    if let Ok(json) = serde_json::to_string(resp) {
        let _ = writeln!(out, "{json}");
        let _ = out.flush();
    }
}

fn log_mcp_command(tool: &str, meta: &[(&str, &str)], knowledge_dir: &Path) {
    let config = Config::load(knowledge_dir);
    if !config.command_log {
        return;
    }
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write as _;
        let log_path = knowledge_dir.join("command.log");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let meta_str: Vec<String> = meta.iter().map(|(k, v)| format!("{k}={v}")).collect();
        writeln!(
            f,
            "[{}] cmd=mcp-{tool} {}",
            util::now_iso(),
            meta_str.join(" ")
        )?;
        Ok(())
    })();
}

// ── update check ─────────────────────────────────────────────────────

/// Check if a newer version of lk is available.
/// Checks at most once per day using a file-based cache.
/// Returns Some(latest_version) if update available, None otherwise.
/// Never fails — all errors return None.
fn check_update_available() -> Option<String> {
    (|| -> Option<String> {
        let config_dir = util::home_dir().join(".config").join("lk");
        let cache_path = config_dir.join("update_check.json");

        // Try reading cache
        if let Ok(content) = std::fs::read_to_string(&cache_path)
            && let Ok(cached) = serde_json::from_str::<Value>(&content)
        {
            let last_checked = cached["last_checked"].as_str()?;
            let latest = cached["latest_version"].as_str()?;

            // If checked today, use cache
            if util::days_since(last_checked) == Some(0) {
                return if is_newer(latest) {
                    Some(latest.to_string())
                } else {
                    None
                };
            }
        }

        // Fetch latest version via curl (no auth required)
        let latest = fetch_latest_tag_quiet()?;

        // Save cache
        let _ = std::fs::create_dir_all(&config_dir);
        let cache = json!({
            "last_checked": util::now_iso(),
            "latest_version": latest,
        });
        let _ = std::fs::write(&cache_path, serde_json::to_string(&cache).ok()?);

        if is_newer(&latest) {
            Some(latest)
        } else {
            None
        }
    })()
}

/// Fetch the latest release tag from GitHub using curl (no auth needed).
fn fetch_latest_tag_quiet() -> Option<String> {
    let url = format!("https://github.com/{}/releases/latest", util::DEFAULT_REPO);
    let output = std::process::Command::new("curl")
        .args(["-sL", "-o", "/dev/null", "-w", "%{url_effective}", &url])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // url_effective looks like: https://github.com/.../releases/tag/v0.10.2
    let effective_url = String::from_utf8_lossy(&output.stdout).to_string();
    let tag = effective_url.trim().rsplit('/').next()?.to_string();
    if tag.is_empty() {
        return None;
    }
    Some(tag)
}

/// Check if a version tag (e.g. "v0.11.0") is newer than current VERSION.
fn is_newer(tag: &str) -> bool {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    util::compare_versions(version, util::VERSION)
        .is_some_and(|ord| ord == std::cmp::Ordering::Greater)
}

/// Build the update_available field for MCP responses.
fn update_info() -> Option<Value> {
    let latest = check_update_available()?;
    let version = latest.strip_prefix('v').unwrap_or(&latest);
    Some(json!({
        "current": util::VERSION,
        "latest": version,
        "message": format!("A new version of lk is available ({} → {}). Run 'lk update' or 'brew upgrade syarihu/tap/lk' to update.", util::VERSION, version),
    }))
}

// ── tool definitions ─────────────────────────────────────────────────

fn tool_definitions(registry: &ProjectRegistry) -> Value {
    let mut tools: Vec<Value> = vec![
        tool_def_search(registry),
        tool_def_add(registry),
        tool_def_list(registry),
        tool_def_get(registry),
        tool_def_update(registry),
        tool_def_supersede(registry),
        tool_def_stats(registry),
    ];

    if !registry.legacy_mode {
        tools.push(tool_def_list_projects());
    }

    json!({ "tools": tools })
}

fn project_property(registry: &ProjectRegistry) -> Option<(String, Value)> {
    if registry.legacy_mode {
        return None;
    }
    let names = registry.project_names().join(", ");
    let desc = if registry.projects.len() == 1 {
        format!("Project name (default: '{}').", registry.projects[0].0)
    } else {
        format!("Project name to operate on. Available: {names}.")
    };
    Some((
        "project".to_string(),
        json!({
            "type": "string",
            "description": desc,
        }),
    ))
}

fn inject_project_prop(schema: &mut Value, registry: &ProjectRegistry) {
    if let Some((key, val)) = project_property(registry)
        && let Some(props) = schema
            .get_mut("inputSchema")
            .and_then(|s| s.get_mut("properties"))
            .and_then(|p| p.as_object_mut())
    {
        props.insert(key, val);
    }
}

/// The status enum for the MCP tool schemas, derived from the single source of
/// truth (`db::VALID_STATUSES`) so the schema can't drift from runtime validation.
fn status_enum() -> Value {
    Value::from(db::VALID_STATUSES)
}

fn tool_def_search(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "search_knowledge",
        "description": "Search the project's knowledge base for design decisions, architecture notes, feature specs, bug investigation records, and other institutional knowledge. Use this BEFORE making significant code changes to check if there are relevant decisions or context already documented. Supports full-text search and keyword-based search. Returns matching entries with relevance scores.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use 1-3 short content keywords separated by spaces. Do NOT paste the user's raw sentence/question — extract the key nouns and drop stopwords/particles. (Terms are OR-matched and ranked, so extra words mostly add noise.) Try both English and Japanese, since knowledge may be stored in either. If no hits, broaden by using fewer keywords."
                },
                "keyword_only": {
                    "type": "boolean",
                    "description": "Search keywords only (default: false)",
                    "default": false
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category (e.g., 'features', 'architecture')"
                },
                "source": {
                    "type": "string",
                    "description": "Filter by source ('local' or 'shared')"
                },
                "status": {
                    "type": "string",
                    "enum": status_enum(),
                    "description": "Filter by status ('active', 'proposed', 'accepted', 'deprecated', 'superseded'). Use 'proposed' to find open plan items."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 5)",
                    "default": 5
                },
                "scope": {
                    "type": "string",
                    "enum": ["project", "user", "all"],
                    "description": "Which knowledge store to search: 'project' (this repo), 'user' (global ~/.config/lk/knowledge.db, carried across projects), or 'all' (default, merged)."
                }
            },
            "required": ["query"]
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_add(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "add_knowledge",
        "description": "Save new knowledge to the project's knowledge base. Use this to record design decisions, architecture rationale, bug investigation findings, non-obvious implementation details, or any context that would be valuable for future development. Content rules: use stable identifiers (function/struct names), not line numbers; include the rationale ('why'), not just the 'what'; never store secrets. Duplicate handling: an entry whose title matches an existing one — or is all but identical to it — is rejected (`added: false` with `similar_entries`); update that entry instead, or pass force=true to add it anyway. Nothing else is rejected: the add succeeds (`added: true`) and any loosely related entries are listed under `possibly_related` for information only; do NOT call update_knowledge on those unless one genuinely covers the same topic.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Entry title"
                },
                "content": {
                    "type": "string",
                    "description": "Entry content (markdown supported)"
                },
                "keywords": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Curated keywords for the entry — ALWAYS provide 5-10 focused terms that best represent it (main components, concepts, proper nouns; lowercase-hyphenated; include both English and Japanese terms where useful). If omitted, keywords are auto-extracted by frequency as a fallback, which is noisier than a curated set."
                },
                "category": {
                    "type": "string",
                    "description": "Category (e.g., 'features', 'architecture')",
                    "default": "general"
                },
                "status": {
                    "type": "string",
                    "enum": status_enum(),
                    "description": "Initial status ('active', 'proposed', 'accepted', 'deprecated', 'superseded'). Default: 'active'. Use 'proposed' for design decisions awaiting review."
                },
                "force": {
                    "type": "boolean",
                    "description": "Add even if an entry with the same (or an all-but-identical) title already exists (default: false). Nothing else is ever rejected, so this is rarely needed — do not set it pre-emptively.",
                    "default": false
                },
                "scope": {
                    "type": "string",
                    "enum": ["auto", "project", "user"],
                    "default": "auto",
                    "description": "Where to save (default 'auto'): 'auto' = project's .knowledge DB if the project is initialized, otherwise the global user store; 'project' = this repo's .knowledge DB (errors if the project isn't initialized); 'user' = global ~/.config/lk/knowledge.db, persists across projects (good for cross-project context/preferences). The user DB is created on first use. On auto-fallback the result includes a 'note'."
                }
            },
            "required": ["title", "content"]
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_list(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "list_knowledge",
        "description": "Browse all knowledge entries in the project's knowledge base. Use this to get an overview of what knowledge is available, or to find entries by source ('shared' = team knowledge from .knowledge/ markdown files, 'local' = entries added via CLI or MCP). Supports filtering by category, status, and pagination.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Filter by source ('local' or 'shared')"
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category"
                },
                "status": {
                    "type": "string",
                    "enum": status_enum(),
                    "description": "Filter by status ('active', 'proposed', 'accepted', 'deprecated', 'superseded'). Use 'proposed' to list open plan items."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 20)",
                    "default": 20
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip first N results (default: 0)",
                    "default": 0
                },
                "scope": {
                    "type": "string",
                    "enum": ["project", "user", "all"],
                    "description": "Which knowledge store to list: 'project', 'user' (global), or 'all' (default, merged)."
                }
            }
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_get(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "get_knowledge",
        "description": "Retrieve the full content of a specific knowledge entry by id or uid. Use this after searching or listing to read the complete details of an entry, including its full markdown content, keywords, and metadata.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": ["integer", "string"],
                    "description": "Entry id (integer, project scope) or uid (string). A uid resolves across scopes (project then user)."
                },
                "scope": {
                    "type": "string",
                    "enum": ["project", "user"],
                    "description": "Optional: force the lookup scope. Omit to auto-resolve (numeric id = project; uid = project then user)."
                }
            },
            "required": ["id"]
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_update(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "update_knowledge",
        "description": "Update an existing knowledge entry by ID. Use this to correct outdated information, add details to existing entries, or mark entries as deprecated when they are no longer relevant. Only provided fields are updated.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": ["integer", "string"],
                    "description": "Entry id (integer, project scope) or uid (string; resolves project then user)."
                },
                "title": {
                    "type": "string",
                    "description": "New title"
                },
                "content": {
                    "type": "string",
                    "description": "New content"
                },
                "keywords": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "New keywords (replaces the existing set — pass the full curated list, 5-10 focused terms)"
                },
                "status": {
                    "type": "string",
                    "enum": status_enum(),
                    "description": "Set status ('active', 'deprecated', 'proposed', 'accepted', or 'superseded')"
                },
                "superseded_by": {
                    "type": ["integer", "string"],
                    "description": "id or uid of the entry that supersedes this one (must be in the same scope; use 0 to clear)"
                },
                "scope": {
                    "type": "string",
                    "enum": ["project", "user"],
                    "description": "Optional: force the lookup scope. Omit to auto-resolve (numeric id = project; uid = project then user)."
                }
            },
            "required": ["id"]
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_supersede(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "supersede_knowledge",
        "description": "Mark an entry as superseded by another entry. Creates bidirectional links: the old entry gets status 'superseded' with a reference to the new entry, and the new entry records that it supersedes the old one.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "old_id": {
                    "type": ["integer", "string"],
                    "description": "id or uid of the old entry being superseded"
                },
                "new_id": {
                    "type": ["integer", "string"],
                    "description": "id or uid of the new entry that supersedes it (must be in the same scope as old)"
                },
                "scope": {
                    "type": "string",
                    "enum": ["project", "user"],
                    "description": "Optional: force the scope. Both entries must live in the same scope. Omit to auto-resolve from the old entry."
                }
            },
            "required": ["old_id", "new_id"]
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_stats(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "get_stats",
        "description": "Get a quick overview of the knowledge base: total number of entries, shared vs local counts, and unique keyword count. Useful to check if a knowledge base exists and how much content is available before searching. Includes a per-scope breakdown.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["project", "user", "all"],
                    "description": "Which knowledge store to summarize: 'project', 'user' (global), or 'all' (default, combined)."
                }
            }
        }
    });
    inject_project_prop(&mut def, registry);
    def
}

fn tool_def_list_projects() -> Value {
    json!({
        "name": "list_projects",
        "description": "List all registered knowledge base projects with their names and paths. Use this to discover which projects are available before querying a specific one.",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    })
}

// ── tool execution ───────────────────────────────────────────────────

fn entry_to_json(e: &db::Entry, kws: &[String], config: &Config) -> Value {
    let days = util::days_since(&e.updated_at);
    let threshold = config.stale_threshold_for(&e.source);
    let stale = days.is_some_and(|d| d > threshold);
    let mut obj = json!({
        "id": e.id,
        "title": e.title,
        "content": e.content,
        "category": e.category,
        "source": e.source,
        "status": e.status,
        "uid": e.uid,
        "keywords": kws,
        "score": e.rank,
        "stale": stale,
        "created_at": e.created_at,
        "updated_at": e.updated_at,
    });
    if let Some(ref sb) = e.superseded_by {
        obj["superseded_by"] = json!(sb);
    }
    if let Some(ref ss) = e.supersedes {
        obj["supersedes"] = json!(
            ss.split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        );
    }
    obj
}

/// Add "project" and "update_available" keys to a result Value.
fn decorate_result(mut result: Value, project_name: &Option<String>) -> Value {
    if let Some(obj) = result.as_object_mut() {
        if let Some(name) = project_name {
            obj.insert("project".to_string(), json!(name));
        }
        if let Some(update) = update_info() {
            obj.insert("update_available".to_string(), update);
        }
    }
    result
}

fn project_name_for(registry: &ProjectRegistry, project_root: &Path) -> Option<String> {
    if registry.legacy_mode {
        None
    } else {
        registry
            .projects
            .iter()
            .find(|(_, p)| *p == project_root)
            .map(|(n, _)| n.clone())
    }
}

/// Whether the given project (registry project_root, not CWD) has an initialized DB.
/// Side-effect free; treats the legacy `.claude/knowledge.db` location as initialized.
fn project_db_exists_for(project_root: &Path) -> bool {
    let db_root = util::resolve_db_root(project_root);
    db_root.join(".knowledge").join("knowledge.db").is_file()
        || project_root.join(".claude").join("knowledge.db").is_file()
}

/// Open the project DB connection (runs auto-sync first, like the original flow).
/// Uses `get_db_path_for` so a legacy `.claude/knowledge.db` is migrated/opened too,
/// keeping it consistent with `project_db_exists_for`.
fn open_project_conn(project_root: &Path) -> Result<rusqlite::Connection, String> {
    let db_path = util::get_db_path_for(project_root);
    maybe_auto_sync_for(project_root);
    let (conn, _) = db::open_db(&db_path).map_err(|e| format!("DB error: {e}"))?;
    Ok(conn)
}

fn open_user_conn() -> Result<Option<rusqlite::Connection>, String> {
    util::open_user_db().map_err(|e| format!("user DB error: {e}"))
}

/// Open one scope's connection. `user` errors if no user DB exists yet.
fn open_scope_conn_mcp(scope: &str, project_root: &Path) -> Result<rusqlite::Connection, String> {
    match scope {
        "project" => open_project_conn(project_root),
        "user" => open_user_conn()?.ok_or_else(|| {
            "No user-scope knowledge DB exists yet (~/.config/lk/knowledge.db). \
             Add one with add_knowledge(scope=\"user\")."
                .to_string()
        }),
        o => Err(format!("Invalid scope '{o}' (expected: project, user)")),
    }
}

/// Connections to query for a read tool, honoring `scope` (project|user|all, default all).
fn read_scope_conns(
    scope: Option<&str>,
    project_root: &Path,
) -> Result<Vec<(rusqlite::Connection, &'static str)>, String> {
    let (want_project, project_required, want_user) = match scope {
        None | Some("all") => (true, false, true),
        Some("project") => (true, true, false),
        Some("user") => (false, false, true),
        Some(o) => {
            return Err(format!(
                "Invalid scope '{o}' (expected: project, user, all)"
            ));
        }
    };
    let mut conns: Vec<(rusqlite::Connection, &'static str)> = Vec::new();
    // Explicit project still errors on a missing DB; default `all` treats project as
    // best-effort and skips it (no open / no auto-sync) when not initialized.
    if want_project && (project_required || project_db_exists_for(project_root)) {
        conns.push((open_project_conn(project_root)?, "project"));
    }
    if want_user && let Some(c) = open_user_conn()? {
        conns.push((c, "user"));
    }
    Ok(conns)
}

/// Read an `id`/`old_id`/`new_id`/`superseded_by` param that may be an integer or a
/// UID string. `Ok(None)` = absent/null; `Err` = present but a wrong type (so a
/// malformed argument fails loudly instead of being silently dropped).
fn id_param(v: &Value) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        Value::Number(n) => n
            .as_i64()
            .map(|i| Some(i.to_string()))
            .ok_or_else(|| "id must be an integer or a uid string".to_string()),
        _ => Err("id must be an integer or a uid string".to_string()),
    }
}

/// Look up `<id-or-uid>` within a connection (None if absent).
fn lookup_in_conn(conn: &rusqlite::Connection, arg: &str) -> Result<Option<db::Entry>, String> {
    let r = if let Ok(id) = arg.parse::<i64>() {
        db::get_entry(conn, id)
    } else {
        db::get_entry_by_uid(conn, arg)
    };
    r.map_err(|e| format!("get error: {e}"))
}

/// Resolve `<id-or-uid>` across scopes. Numeric → named scope (default project);
/// UID → named scope, or project-then-user when scope is omitted. Returns the
/// owning connection so mutations run on the right DB.
fn mcp_resolve_target(
    arg: &str,
    scope: Option<&str>,
    project_root: &Path,
) -> Result<(rusqlite::Connection, db::Entry, &'static str), String> {
    let scope = match scope {
        Some(s) => Some(s),
        None if arg.parse::<i64>().is_ok() => Some("project"),
        None => None,
    };
    match scope {
        Some(s) => {
            let conn = open_scope_conn_mcp(s, project_root)?;
            let entry =
                lookup_in_conn(&conn, arg)?.ok_or_else(|| format!("Entry not found: {arg}"))?;
            let label = if s == "user" { "user" } else { "project" };
            Ok((conn, entry, label))
        }
        None => {
            // Auto-resolve a UID: look in project first only if it is initialized, so
            // an uninitialized project doesn't error before we fall back to user.
            if project_db_exists_for(project_root) {
                let pconn = open_project_conn(project_root)?;
                if let Some(e) = lookup_in_conn(&pconn, arg)? {
                    return Ok((pconn, e, "project"));
                }
            }
            if let Some(uconn) = open_user_conn()?
                && let Some(e) = lookup_in_conn(&uconn, arg)?
            {
                return Ok((uconn, e, "user"));
            }
            Err(format!("Entry not found: {arg}"))
        }
    }
}

fn call_tool(name: &str, params: &Value, registry: &ProjectRegistry) -> Result<Value, String> {
    // list_projects doesn't need a DB connection
    if name == "list_projects" {
        if registry.legacy_mode {
            return Err("list_projects is not available in single-project mode.".to_string());
        }
        let projects: Vec<Value> = registry
            .projects
            .iter()
            .map(|(name, path)| {
                json!({
                    "name": name,
                    "path": path.to_string_lossy(),
                    "initialized": project_db_exists_for(path),
                })
            })
            .collect();
        return Ok(json!({
            "count": projects.len(),
            "projects": projects,
        }));
    }

    // Project context — DBs are opened per-tool below based on scope, so user-scope
    // operations don't force a project DB open / auto-sync.
    let project_param = params["project"].as_str();
    let project_root = registry.resolve(project_param)?;
    let knowledge_dir = project_root.join(".knowledge");
    let project_name = project_name_for(registry, &project_root);
    let config = Config::load(&knowledge_dir);

    match name {
        "search_knowledge" => {
            let query = params["query"]
                .as_str()
                .ok_or("missing required parameter: query")?;
            let keyword_only = params["keyword_only"].as_bool().unwrap_or(false);
            let category = params["category"].as_str();
            let source = params["source"].as_str();
            let status = params["status"].as_str();
            let limit = params["limit"].as_u64().unwrap_or(5) as usize;
            let scope = params["scope"].as_str();

            // Validate status if provided
            if let Some(st) = status
                && !db::is_valid_status(st)
            {
                return Err(format!(
                    "Invalid status: {st}. Must be one of: {}",
                    db::VALID_STATUSES.join(", ")
                ));
            }

            log_mcp_command("search", &[("query", query)], &knowledge_dir);

            // Query each scope; keywords are fetched on the SAME conn the entry came
            // from (ids are per-DB). rank is 1/(1+|bm25|): smaller = better, so sort
            // ASCENDING to match per-DB order (None ranks sort last).
            let conns = read_scope_conns(scope, &project_root)?;
            let mut items: Vec<(f64, &'static str, db::Entry, Vec<String>)> = Vec::new();
            for (conn, label) in &conns {
                let entries = db::search_entries(
                    conn,
                    query,
                    keyword_only,
                    category,
                    source,
                    status,
                    None,
                    limit,
                )
                .map_err(|e| format!("search error: {e}"))?;
                for e in entries {
                    let kws = db::get_keywords(conn, e.id).unwrap_or_default();
                    let score = e.rank.unwrap_or(f64::MAX);
                    items.push((score, label, e, kws));
                }
            }
            // Score ASC (better match first), tie-break updated_at DESC — matching
            // the per-DB SQL order `ORDER BY rank, updated_at DESC`.
            items.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.2.updated_at.cmp(&a.2.updated_at))
            });
            items.truncate(limit);

            let results: Vec<Value> = items
                .iter()
                .map(|(_, label, e, kws)| {
                    let mut obj = entry_to_json(e, kws, &config);
                    obj["scope"] = json!(label);
                    obj
                })
                .collect();

            Ok(decorate_result(
                json!({
                    "count": results.len(),
                    "entries": results,
                }),
                &project_name,
            ))
        }

        "add_knowledge" => {
            let title = params["title"]
                .as_str()
                .ok_or("missing required parameter: title")?;
            let content = params["content"]
                .as_str()
                .ok_or("missing required parameter: content")?;
            let keywords: Vec<String> = params["keywords"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let category = params["category"].as_str().unwrap_or("general");
            let status = params["status"].as_str();
            let force = params["force"].as_bool().unwrap_or(false);
            let scope = params["scope"].as_str().unwrap_or("auto");

            // Validate status if provided
            if let Some(st) = status
                && !db::is_valid_status(st)
            {
                return Err(format!(
                    "Invalid status: {st}. Must be one of: {}",
                    db::VALID_STATUSES.join(", ")
                ));
            }

            // "auto" (default): project if initialized, else fall back to user.
            // Explicit "project" still errors on a missing DB (init prompt).
            let (effective_scope, fell_back) = match scope {
                "project" => ("project", false),
                "user" => ("user", false),
                "auto" => {
                    if project_db_exists_for(&project_root) {
                        ("project", false)
                    } else {
                        ("user", true)
                    }
                }
                o => {
                    return Err(format!(
                        "Invalid scope '{o}' (expected: project, user, auto)"
                    ));
                }
            };
            let conn = match effective_scope {
                "user" => {
                    util::open_or_create_user_db().map_err(|e| format!("user DB error: {e}"))?
                }
                _ => open_project_conn(&project_root)?,
            };

            log_mcp_command(
                "add",
                &[("title", title), ("scope", effective_scope)],
                &knowledge_dir,
            );

            // Apply category template if content is empty
            let template_content;
            let effective_content = if content.is_empty() {
                template_content =
                    util::load_category_template_from(&knowledge_dir, category).unwrap_or_default();
                if template_content.is_empty() {
                    content
                } else {
                    &template_content
                }
            } else {
                content
            };

            // Duplicate check. Only a Block-tier hit (a near-identical title)
            // refuses the add; weaker hits are reported alongside a successful add
            // as `possibly_related` further down.
            let similar = if force {
                Vec::new()
            } else {
                db::find_similar_entries(&conn, title, &keywords, category)
                    .map_err(|e| format!("duplicate check error: {e}"))?
            };
            // Shared with `lk add --json` so a hit looks identical on both surfaces.
            let describe = |s: &db::SimilarEntry| -> Value {
                util::similar_entry_json(&conn, s, effective_scope)
            };

            if similar.iter().any(|s| s.tier == Tier::Block) {
                let dupes: Vec<Value> = similar.iter().map(describe).collect();
                let mut out = json!({
                    "added": false,
                    "reason": "An entry with the same title — or one all but identical to it — \
                               already exists. Update it with update_knowledge if it covers the \
                               same topic, or pass force=true to add this one anyway. Check \
                               `match_reason`: `same-title` is an exact match after \
                               normalization, `similar-title` differs only marginally.",
                    "scope": effective_scope,
                    "similar_entries": dupes,
                });
                if fell_back {
                    out["note"] = json!(
                        "Project not initialized; checked the user scope. Run `lk init` for project scope."
                    );
                }
                return Ok(decorate_result(out, &project_name));
            }

            let id = db::add_entry_full(
                &conn,
                title,
                effective_content,
                &keywords,
                category,
                "local",
                None,
                None,
                None,
                status,
                None,
                None,
            )
            .map_err(|e| format!("add error: {e}"))?;

            // Return the uid too: ids collide across scopes, so uid is the unambiguous
            // handle for follow-up get/update calls (even without passing scope).
            let uid = db::get_entry(&conn, id)
                .ok()
                .flatten()
                .map(|e| e.uid)
                .unwrap_or_default();
            let mut out = json!({
                "added": true,
                "id": id,
                "uid": uid,
                "title": title,
                "status": status.unwrap_or("active"),
                "scope": effective_scope,
            });
            if fell_back {
                out["note"] = json!(
                    "Project not initialized; saved to user scope (global). Run `lk init` for project scope."
                );
            }
            // A different key from the block path's `similar_entries`, which means
            // "not added". Stating the outcome explicitly stops an agent from
            // reading a weak hit as a rejection and overwriting an unrelated entry.
            if !similar.is_empty() {
                out["possibly_related"] = json!(similar.iter().map(describe).collect::<Vec<_>>());
                out["possibly_related_note"] = json!(
                    "The entry WAS added successfully. These existing entries look related and \
                     are listed for information only. Call update_knowledge on one of them ONLY \
                     if it covers genuinely the same topic (in which case delete the new entry); \
                     otherwise ignore this list."
                );
            }
            Ok(decorate_result(out, &project_name))
        }

        "list_knowledge" => {
            let source = params["source"].as_str();
            let category = params["category"].as_str();
            let status = params["status"].as_str();
            let limit = params["limit"].as_u64().unwrap_or(20) as usize;
            let offset = params["offset"].as_u64().unwrap_or(0) as usize;
            let scope = params["scope"].as_str();

            // Validate status so a typo errors instead of silently returning [].
            if let Some(st) = status
                && !db::is_valid_status(st)
            {
                return Err(format!(
                    "Invalid status: {st}. Must be one of: {}",
                    db::VALID_STATUSES.join(", ")
                ));
            }

            log_mcp_command("list", &[], &knowledge_dir);

            // Merge entries across scopes, tagging each with (label, keywords).
            let conns = read_scope_conns(scope, &project_root)?;
            let mut tagged: Vec<(&'static str, db::Entry, Vec<String>)> = Vec::new();
            for (conn, label) in &conns {
                let entries = if let Some(src) = source {
                    db::list_entries_by_source(conn, src).map_err(|e| format!("list error: {e}"))?
                } else {
                    db::list_entries(conn, category).map_err(|e| format!("list error: {e}"))?
                };
                for e in entries {
                    // Apply category filter when source is also specified.
                    if source.is_some() && category.is_some_and(|c| e.category != c) {
                        continue;
                    }
                    // Apply status filter (e.g. proposed = open plan items).
                    if status.is_some_and(|st| e.status != st) {
                        continue;
                    }
                    let kws = db::get_keywords(conn, e.id).unwrap_or_default();
                    tagged.push((label, e, kws));
                }
            }

            // Re-sort merged set by updated_at DESC so pagination is globally correct.
            tagged.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
            let total = tagged.len();

            let page: Vec<Value> = tagged
                .iter()
                .skip(offset)
                .take(limit)
                .map(|(label, e, kws)| {
                    json!({
                        "id": e.id,
                        "uid": e.uid,
                        "title": e.title,
                        "category": e.category,
                        "source": e.source,
                        "scope": label,
                        "status": e.status,
                        "keywords": kws,
                        "updated_at": e.updated_at,
                    })
                })
                .collect();

            Ok(decorate_result(
                json!({
                    "total": total,
                    "offset": offset,
                    "count": page.len(),
                    "entries": page,
                }),
                &project_name,
            ))
        }

        "get_knowledge" => {
            let arg = id_param(&params["id"])?.ok_or("missing required parameter: id")?;
            let scope = params["scope"].as_str();

            log_mcp_command("get", &[("id", &arg)], &knowledge_dir);

            let (conn, entry, label) = mcp_resolve_target(&arg, scope, &project_root)?;
            let kws = db::get_keywords(&conn, entry.id).unwrap_or_default();
            let mut obj = entry_to_json(&entry, &kws, &config);
            obj["scope"] = json!(label);

            Ok(decorate_result(obj, &project_name))
        }

        "update_knowledge" => {
            let arg = id_param(&params["id"])?.ok_or("missing required parameter: id")?;
            let scope = params["scope"].as_str();
            let status = params["status"].as_str();

            // Validate status before resolving the target / opening the DB, so a bad
            // value errors consistently with add/search/list/CLI (an `Invalid status`
            // message) rather than a target/DB error for a nonexistent id.
            if let Some(st) = status
                && !db::is_valid_status(st)
            {
                return Err(format!(
                    "Invalid status: {st}. Must be one of: {}",
                    db::VALID_STATUSES.join(", ")
                ));
            }

            let (conn, entry, _label) = mcp_resolve_target(&arg, scope, &project_root)?;
            let local_id = entry.id;

            let title = params["title"].as_str();
            let content = params["content"].as_str();
            let keywords: Option<Vec<String>> = params["keywords"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
            // superseded_by may be an integer id or a uid string; "0" clears it.
            // Resolved within the SAME DB as the edited entry (no cross-scope refs).
            let sb_arg = id_param(&params["superseded_by"])?;

            log_mcp_command("update", &[("id", &arg)], &knowledge_dir);

            let resolve_sb = |s: &str| -> Result<Option<String>, String> {
                if s == "0" {
                    Ok(None)
                } else {
                    let target = lookup_in_conn(&conn, s)?.ok_or_else(|| {
                        format!("Entry '{s}' not found in the same scope for superseded_by")
                    })?;
                    Ok(Some(target.uid.clone()))
                }
            };

            // Resolve superseded_by BEFORE any write, so a bad value can't leave a
            // partial update. status is already validated above. status_update = (status, sb_uid).
            let status_update: Option<(String, Option<String>)> = if let Some(st) = status {
                let sb = match sb_arg.as_deref() {
                    Some(s) => resolve_sb(s)?,
                    None => entry.superseded_by.clone(),
                };
                Some((st.to_string(), sb))
            } else if let Some(s) = sb_arg.as_deref() {
                Some((entry.status.clone(), resolve_sb(s)?))
            } else {
                None
            };

            // Apply all writes atomically.
            let now = util::now_iso();
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| format!("update error: {e}"))?;
            let result = (|| -> Result<(), String> {
                db::update_entry(&conn, local_id, title, content, keywords.as_deref(), &now)
                    .map_err(|e| format!("update error: {e}"))?;
                if let Some((st, sb)) = &status_update {
                    db::update_entry_status(&conn, local_id, st, sb.as_deref())
                        .map_err(|e| format!("status update error: {e}"))?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => conn
                    .execute_batch("COMMIT")
                    .map_err(|e| format!("update error: {e}"))?,
                Err(e) => {
                    conn.execute_batch("ROLLBACK").ok();
                    return Err(e);
                }
            }

            Ok(decorate_result(
                json!({
                    "updated": true,
                    "id": local_id,
                }),
                &project_name,
            ))
        }

        "supersede_knowledge" => {
            let old = id_param(&params["old_id"])?.ok_or("missing required parameter: old_id")?;
            let new = id_param(&params["new_id"])?.ok_or("missing required parameter: new_id")?;
            let scope = params["scope"].as_str();

            log_mcp_command(
                "supersede",
                &[("old_id", &old), ("new_id", &new)],
                &knowledge_dir,
            );

            // Resolve `old` (which fixes the owning connection/scope), then resolve
            // `new` in the SAME connection so both updates share one transaction.
            // Cross-scope supersede is unsupported (new must live in old's DB).
            let (conn, old_entry, _label) = mcp_resolve_target(&old, scope, &project_root)?;
            let new_entry = lookup_in_conn(&conn, &new)?
                .ok_or_else(|| format!("Entry '{new}' not found in the same scope as '{old}'"))?;
            if old_entry.id == new_entry.id {
                return Err("old and new must be different entries".to_string());
            }
            let old_id = old_entry.id;
            let new_id = new_entry.id;

            // Atomic: both updates in a transaction
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| format!("supersede error: {e}"))?;
            let result = (|| -> Result<(), String> {
                db::update_entry_status(&conn, old_id, "superseded", Some(&new_entry.uid))
                    .map_err(|e| format!("supersede error: {e}"))?;
                let new_supersedes =
                    db::append_supersedes(new_entry.supersedes.as_deref(), &old_entry.uid);
                db::update_entry_supersedes(&conn, new_id, Some(&new_supersedes))
                    .map_err(|e| format!("supersede error: {e}"))?;
                Ok(())
            })();
            match result {
                Ok(()) => conn
                    .execute_batch("COMMIT")
                    .map_err(|e| format!("supersede error: {e}"))?,
                Err(e) => {
                    conn.execute_batch("ROLLBACK").ok();
                    return Err(e);
                }
            }

            let new_supersedes =
                db::append_supersedes(new_entry.supersedes.as_deref(), &old_entry.uid);

            Ok(decorate_result(
                json!({
                    "old_id": old_id,
                    "old_uid": old_entry.uid,
                    "new_id": new_id,
                    "new_uid": new_entry.uid,
                    "old_status": "superseded",
                    "old_superseded_by": new_entry.uid,
                    "new_supersedes": new_supersedes,
                }),
                &project_name,
            ))
        }

        "get_stats" => {
            let scope = params["scope"].as_str();
            log_mcp_command("stats", &[], &knowledge_dir);

            let conns = read_scope_conns(scope, &project_root)?;
            let mut total = 0i64;
            let mut shared = 0i64;
            let mut local = 0i64;
            // unique keywords is a UNION across DBs, not a sum of per-DB DISTINCT counts.
            let mut kw_union: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut scopes: Vec<Value> = Vec::new();
            for (conn, label) in &conns {
                let s = db::get_stats(conn).map_err(|e| format!("stats error: {e}"))?;
                total += s.total;
                shared += s.shared;
                local += s.local;
                for (kw, _) in db::keyword_counts(conn).map_err(|e| format!("stats error: {e}"))? {
                    kw_union.insert(kw);
                }
                scopes.push(json!({
                    "scope": label,
                    "total": s.total,
                    "shared": s.shared,
                    "local": s.local,
                    "keywords": s.keywords,
                }));
            }

            Ok(decorate_result(
                json!({
                    "total": total,
                    "shared": shared,
                    "local": local,
                    "keywords": kw_union.len(),
                    "scopes": scopes,
                }),
                &project_name,
            ))
        }

        _ => Err(format!("Unknown tool: {name}")),
    }
}

// ── main loop ────────────────────────────────────────────────────────

pub fn run_server(project_paths: Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let registry = ProjectRegistry::from_paths(project_paths)?;

    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    for line in stdin.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = respond_err(None, -32700, &format!("Parse error: {e}"));
                write_response(&mut stdout, &resp);
                continue;
            }
        };

        // Validate jsonrpc version
        if req.jsonrpc.as_deref() != Some("2.0") {
            if req.id.is_some() {
                let resp = respond_err(req.id, -32600, "Invalid Request: jsonrpc must be \"2.0\"");
                write_response(&mut stdout, &resp);
            }
            continue;
        }

        // Notifications (no id) — handle silently
        if req.id.is_none() {
            continue;
        }

        let resp = match req.method.as_str() {
            "initialize" => respond(
                req.id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "lk-knowledge",
                        "version": util::VERSION,
                    }
                }),
            ),

            "ping" => respond(req.id, json!({})),

            "tools/list" => respond(req.id, tool_definitions(&registry)),

            "tools/call" => {
                let tool_name = req.params["name"].as_str().unwrap_or("");
                let arguments = &req.params["arguments"];

                match call_tool(tool_name, arguments, &registry) {
                    Ok(result) => {
                        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
                        respond(
                            req.id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": text,
                                }]
                            }),
                        )
                    }
                    Err(e) => respond(
                        req.id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": e,
                            }],
                            "isError": true,
                        }),
                    ),
                }
            }

            _ => respond_err(req.id, -32601, &format!("Method not found: {}", req.method)),
        };

        write_response(&mut stdout, &resp);
    }

    Ok(())
}
