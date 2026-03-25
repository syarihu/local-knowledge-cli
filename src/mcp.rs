use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::cmd::maybe_auto_sync_for;
use crate::config::Config;
use crate::db;
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
            let db_root = util::resolve_db_root(&canonical);
            let db_path = db_root.join(".knowledge").join("knowledge.db");
            if !db_path.exists() {
                return Err(format!(
                    "No knowledge DB found at {}. Run 'lk init' in that project first.",
                    canonical.display()
                )
                .into());
            }
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

fn tool_def_search(registry: &ProjectRegistry) -> Value {
    let mut def = json!({
        "name": "search_knowledge",
        "description": "Search the project's knowledge base for design decisions, architecture notes, feature specs, bug investigation records, and other institutional knowledge. Use this BEFORE making significant code changes to check if there are relevant decisions or context already documented. Supports full-text search and keyword-based search. Returns matching entries with relevance scores.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string"
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
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 5)",
                    "default": 5
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
        "description": "Save new knowledge to the project's knowledge base. Use this to record design decisions, architecture rationale, bug investigation findings, non-obvious implementation details, or any context that would be valuable for future development. Automatically checks for duplicates before adding.",
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
                    "description": "Keywords for the entry (auto-extracted if not provided)"
                },
                "category": {
                    "type": "string",
                    "description": "Category (e.g., 'features', 'architecture')",
                    "default": "general"
                },
                "status": {
                    "type": "string",
                    "description": "Initial status ('active', 'proposed', 'accepted', 'deprecated', 'superseded'). Default: 'active'. Use 'proposed' for design decisions awaiting review."
                },
                "force": {
                    "type": "boolean",
                    "description": "Skip duplicate check and force add (default: false)",
                    "default": false
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
        "description": "Browse all knowledge entries in the project's knowledge base. Use this to get an overview of what knowledge is available, or to find entries by source ('shared' = team knowledge from .knowledge/ markdown files, 'local' = entries added via CLI or MCP). Supports filtering by category and pagination.",
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
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 20)",
                    "default": 20
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip first N results (default: 0)",
                    "default": 0
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
        "description": "Retrieve the full content of a specific knowledge entry by ID. Use this after searching or listing to read the complete details of an entry, including its full markdown content, keywords, and metadata.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Entry ID"
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
                    "type": "integer",
                    "description": "Entry ID to update"
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
                    "description": "New keywords"
                },
                "status": {
                    "type": "string",
                    "description": "Set status ('active', 'deprecated', 'proposed', 'accepted', or 'superseded')"
                },
                "superseded_by": {
                    "type": "integer",
                    "description": "ID of the entry that supersedes this one (use 0 to clear)"
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
                    "type": "integer",
                    "description": "ID of the old entry being superseded"
                },
                "new_id": {
                    "type": "integer",
                    "description": "ID of the new entry that supersedes it"
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
        "description": "Get a quick overview of the knowledge base: total number of entries, shared vs local counts, and unique keyword count. Useful to check if a knowledge base exists and how much content is available before searching.",
        "inputSchema": {
            "type": "object",
            "properties": {}
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
        obj["supersedes"] = json!(ss.split(',').collect::<Vec<_>>());
    }
    obj
}

/// Resolve project, open DB, load config, run auto-sync.
fn resolve_project(
    params: &Value,
    registry: &ProjectRegistry,
) -> Result<(rusqlite::Connection, Config, PathBuf, Option<String>), String> {
    let project_param = params["project"].as_str();
    let project_root = registry.resolve(project_param)?;
    let knowledge_dir = project_root.join(".knowledge");
    let db_root = util::resolve_db_root(&project_root);
    let db_path = db_root.join(".knowledge").join("knowledge.db");

    maybe_auto_sync_for(&project_root);

    let (conn, _) = db::open_db(&db_path).map_err(|e| format!("DB error: {e}"))?;
    let config = Config::load(&knowledge_dir);

    // Project name for response decoration (only in multi-project mode)
    let project_name = if registry.legacy_mode {
        None
    } else {
        registry
            .projects
            .iter()
            .find(|(_, p)| *p == project_root)
            .map(|(n, _)| n.clone())
    };

    Ok((conn, config, knowledge_dir, project_name))
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
                })
            })
            .collect();
        return Ok(json!({
            "count": projects.len(),
            "projects": projects,
        }));
    }

    let (conn, config, knowledge_dir, project_name) = resolve_project(params, registry)?;

    match name {
        "search_knowledge" => {
            let query = params["query"]
                .as_str()
                .ok_or("missing required parameter: query")?;
            let keyword_only = params["keyword_only"].as_bool().unwrap_or(false);
            let category = params["category"].as_str();
            let source = params["source"].as_str();
            let limit = params["limit"].as_u64().unwrap_or(5) as usize;

            log_mcp_command("search", &[("query", query)], &knowledge_dir);

            let entries =
                db::search_entries(&conn, query, keyword_only, category, source, None, limit)
                    .map_err(|e| format!("search error: {e}"))?;

            let results: Vec<Value> = entries
                .iter()
                .map(|e| {
                    let kws = db::get_keywords(&conn, e.id).unwrap_or_default();
                    entry_to_json(e, &kws, &config)
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

            // Validate status if provided
            if let Some(st) = status {
                if !db::is_valid_status(st) {
                    return Err(format!(
                        "Invalid status: {st}. Must be one of: {}",
                        db::VALID_STATUSES.join(", ")
                    ));
                }
            }

            log_mcp_command("add", &[("title", title)], &knowledge_dir);

            // Apply category template if content is empty
            let template_content;
            let effective_content = if content.is_empty() {
                let template_dir = knowledge_dir.join("templates").join(format!("{category}.md"));
                template_content = std::fs::read_to_string(template_dir).unwrap_or_default();
                if template_content.is_empty() { content } else { &template_content }
            } else {
                content
            };

            // Duplicate check
            if !force {
                let similar = db::find_similar_entries(&conn, title, &keywords)
                    .map_err(|e| format!("duplicate check error: {e}"))?;
                if !similar.is_empty() {
                    let dupes: Vec<Value> = similar
                        .iter()
                        .map(|e| json!({"id": e.id, "title": e.title}))
                        .collect();
                    return Ok(decorate_result(
                        json!({
                            "added": false,
                            "reason": "Similar entries found. Use force=true to add anyway.",
                            "similar_entries": dupes,
                        }),
                        &project_name,
                    ));
                }
            }

            let id = db::add_entry_full(
                &conn, title, effective_content, &keywords, category, "local", None, None,
                None, status, None, None,
            )
            .map_err(|e| format!("add error: {e}"))?;

            Ok(decorate_result(
                json!({
                    "added": true,
                    "id": id,
                    "title": title,
                    "status": status.unwrap_or("active"),
                }),
                &project_name,
            ))
        }

        "list_knowledge" => {
            let source = params["source"].as_str();
            let category = params["category"].as_str();
            let limit = params["limit"].as_u64().unwrap_or(20) as usize;
            let offset = params["offset"].as_u64().unwrap_or(0) as usize;

            log_mcp_command("list", &[], &knowledge_dir);

            let entries = if let Some(src) = source {
                db::list_entries_by_source(&conn, src).map_err(|e| format!("list error: {e}"))?
            } else {
                db::list_entries(&conn, category).map_err(|e| format!("list error: {e}"))?
            };

            // Apply source + category filter when both specified
            let filtered: Vec<&db::Entry> = entries
                .iter()
                .filter(|e| {
                    if source.is_some() && category.is_some() {
                        category.is_none_or(|c| e.category == c)
                    } else {
                        true
                    }
                })
                .collect();

            let page: Vec<Value> = filtered
                .iter()
                .skip(offset)
                .take(limit)
                .map(|e| {
                    let kws = db::get_keywords(&conn, e.id).unwrap_or_default();
                    json!({
                        "id": e.id,
                        "title": e.title,
                        "category": e.category,
                        "source": e.source,
                        "status": e.status,
                        "keywords": kws,
                        "updated_at": e.updated_at,
                    })
                })
                .collect();

            Ok(decorate_result(
                json!({
                    "total": filtered.len(),
                    "offset": offset,
                    "count": page.len(),
                    "entries": page,
                }),
                &project_name,
            ))
        }

        "get_knowledge" => {
            let id = params["id"]
                .as_i64()
                .ok_or("missing required parameter: id")?;

            log_mcp_command("get", &[("id", &id.to_string())], &knowledge_dir);

            let entry = db::get_entry(&conn, id)
                .map_err(|e| format!("get error: {e}"))?
                .ok_or_else(|| format!("Entry not found: {id}"))?;
            let kws = db::get_keywords(&conn, id).unwrap_or_default();

            Ok(decorate_result(
                entry_to_json(&entry, &kws, &config),
                &project_name,
            ))
        }

        "update_knowledge" => {
            let id = params["id"]
                .as_i64()
                .ok_or("missing required parameter: id")?;

            // Verify entry exists
            db::get_entry(&conn, id)
                .map_err(|e| format!("get error: {e}"))?
                .ok_or_else(|| format!("Entry not found: {id}"))?;

            let title = params["title"].as_str();
            let content = params["content"].as_str();
            let keywords: Option<Vec<String>> = params["keywords"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
            let status = params["status"].as_str();

            log_mcp_command("update", &[("id", &id.to_string())], &knowledge_dir);

            let now = util::now_iso();
            db::update_entry(&conn, id, title, content, keywords.as_deref(), &now)
                .map_err(|e| format!("update error: {e}"))?;

            if let Some(st) = status {
                if !db::is_valid_status(st) {
                    return Err(format!(
                        "Invalid status: {st}. Must be one of: {}",
                        db::VALID_STATUSES.join(", ")
                    ));
                }
                let superseded_by_id = params["superseded_by"].as_i64();
                let superseded_by_uid: Option<String> = match superseded_by_id {
                    Some(0) => None,
                    Some(v) => {
                        let target = db::get_entry(&conn, v)
                            .map_err(|e| format!("get error: {e}"))?
                            .ok_or_else(|| format!("Entry #{v} not found for superseded_by"))?;
                        Some(target.uid.clone())
                    }
                    None => {
                        let current = db::get_entry(&conn, id)
                            .map_err(|e| format!("get error: {e}"))?
                            .unwrap();
                        current.superseded_by.clone()
                    }
                };
                db::update_entry_status(&conn, id, st, superseded_by_uid.as_deref())
                    .map_err(|e| format!("status update error: {e}"))?;
            } else if let Some(v) = params["superseded_by"].as_i64() {
                let current = db::get_entry(&conn, id)
                    .map_err(|e| format!("get error: {e}"))?
                    .unwrap();
                let uid = if v == 0 {
                    None
                } else {
                    let target = db::get_entry(&conn, v)
                        .map_err(|e| format!("get error: {e}"))?
                        .ok_or_else(|| format!("Entry #{v} not found for superseded_by"))?;
                    Some(target.uid.clone())
                };
                db::update_entry_status(&conn, id, &current.status, uid.as_deref())
                    .map_err(|e| format!("status update error: {e}"))?;
            }

            Ok(decorate_result(
                json!({
                    "updated": true,
                    "id": id,
                }),
                &project_name,
            ))
        }

        "supersede_knowledge" => {
            let old_id = params["old_id"]
                .as_i64()
                .ok_or("missing required parameter: old_id")?;
            let new_id = params["new_id"]
                .as_i64()
                .ok_or("missing required parameter: new_id")?;

            log_mcp_command(
                "supersede",
                &[("old_id", &old_id.to_string()), ("new_id", &new_id.to_string())],
                &knowledge_dir,
            );

            let old_entry = db::get_entry(&conn, old_id)
                .map_err(|e| format!("get error: {e}"))?
                .ok_or_else(|| format!("Entry #{old_id} not found"))?;
            let new_entry = db::get_entry(&conn, new_id)
                .map_err(|e| format!("get error: {e}"))?
                .ok_or_else(|| format!("Entry #{new_id} not found"))?;

            db::update_entry_status(&conn, old_id, "superseded", Some(&new_entry.uid))
                .map_err(|e| format!("supersede error: {e}"))?;

            let new_supersedes =
                db::append_supersedes(new_entry.supersedes.as_deref(), &old_entry.uid);
            db::update_entry_supersedes(&conn, new_id, Some(&new_supersedes))
                .map_err(|e| format!("supersede error: {e}"))?;

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
            log_mcp_command("stats", &[], &knowledge_dir);

            let stats = db::get_stats(&conn).map_err(|e| format!("stats error: {e}"))?;

            Ok(decorate_result(
                json!({
                    "total": stats.total,
                    "shared": stats.shared,
                    "local": stats.local,
                    "keywords": stats.keywords,
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
