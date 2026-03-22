use std::path::PathBuf;

use crate::util;

/// Build the args list for `lk mcp` with optional `--project` flags.
fn build_mcp_args(projects: &[PathBuf]) -> Vec<String> {
    let mut args = vec!["mcp".to_string()];
    for p in projects {
        args.push("--project".to_string());
        args.push(p.to_string_lossy().to_string());
    }
    args
}

/// Resolve project paths: use explicit --project flags, or auto-detect CWD if it has .knowledge.
fn resolve_projects(projects: &[PathBuf]) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if !projects.is_empty() {
        // Validate all paths
        for p in projects {
            let canonical = std::fs::canonicalize(p)
                .map_err(|e| format!("Cannot resolve path '{}': {e}", p.display()))?;
            if !canonical.join(".knowledge").join("knowledge.db").exists() {
                return Err(format!(
                    "No knowledge DB found at {}. Run 'lk init' in that project first.",
                    canonical.display()
                )
                .into());
            }
        }
        return Ok(projects
            .iter()
            .map(|p| std::fs::canonicalize(p).unwrap())
            .collect());
    }

    // Auto-detect from CWD
    let cwd = std::env::current_dir()?;
    if cwd.join(".knowledge").join("knowledge.db").exists() {
        eprintln!("Auto-detected project: {}", cwd.display());
        return Ok(vec![cwd]);
    }

    Err(
        "No --project specified and current directory has no knowledge base. \
         Use --project /path/to/project to specify project directories."
            .into(),
    )
}

pub fn cmd_install_mcp(
    target: &str,
    projects: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let do_claude_code = target == "all" || target == "claude-code";
    let do_claude_desktop = target == "all" || target == "claude-desktop";

    if !do_claude_code && !do_claude_desktop {
        return Err(format!(
            "Unknown target: {target}. Use 'claude-code', 'claude-desktop', or 'all'."
        )
        .into());
    }

    if do_claude_code {
        install_claude_code(projects)?;
    }

    if do_claude_desktop {
        let resolved = resolve_projects(projects)?;
        install_claude_desktop(&resolved)?;
    }

    Ok(())
}

fn install_claude_code(projects: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Installing MCP server for Claude Code...");

    let mut args = vec![
        "mcp".to_string(),
        "add".to_string(),
        "--transport".to_string(),
        "stdio".to_string(),
        "lk-knowledge".to_string(),
        "--".to_string(),
    ];
    args.extend(build_mcp_args(projects));

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let status = std::process::Command::new("claude")
        .args(&args_ref)
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("Successfully registered lk-knowledge MCP server with Claude Code.");
            Ok(())
        }
        Ok(s) => Err(format!(
            "claude mcp add exited with status: {}",
            s.code().unwrap_or(-1)
        )
        .into()),
        Err(e) => Err(format!(
            "Failed to run 'claude' command. Is Claude Code installed? Error: {e}"
        )
        .into()),
    }
}

fn install_claude_desktop(projects: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Installing MCP server for Claude Desktop...");

    let config_path = get_claude_desktop_config_path();
    let config_dir = config_path
        .parent()
        .ok_or("Cannot determine config directory")?;

    // Ensure directory exists
    std::fs::create_dir_all(config_dir)?;

    // Read existing config or start fresh
    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content)?
    } else {
        serde_json::json!({})
    };

    // Ensure mcpServers object exists
    if config.get("mcpServers").is_none() {
        config["mcpServers"] = serde_json::json!({});
    }

    // Build args with --project flags
    let mcp_args = build_mcp_args(projects);
    let args_json: Vec<serde_json::Value> = mcp_args
        .iter()
        .map(|s| serde_json::Value::String(s.clone()))
        .collect();

    // Add/update lk-knowledge server
    config["mcpServers"]["lk-knowledge"] = serde_json::json!({
        "command": "lk",
        "args": args_json,
    });

    // Write back
    let output = serde_json::to_string_pretty(&config)?;
    std::fs::write(&config_path, output + "\n")?;

    eprintln!(
        "Successfully configured lk-knowledge MCP server in {}",
        config_path.display()
    );
    if !projects.is_empty() {
        for p in projects {
            eprintln!("  Project: {}", p.display());
        }
    }
    Ok(())
}

pub fn cmd_uninstall_mcp(target: &str) -> Result<(), Box<dyn std::error::Error>> {
    let do_claude_code = target == "all" || target == "claude-code";
    let do_claude_desktop = target == "all" || target == "claude-desktop";

    if !do_claude_code && !do_claude_desktop {
        return Err(format!(
            "Unknown target: {target}. Use 'claude-code', 'claude-desktop', or 'all'."
        )
        .into());
    }

    if do_claude_code {
        uninstall_claude_code()?;
    }

    if do_claude_desktop {
        uninstall_claude_desktop()?;
    }

    Ok(())
}

fn uninstall_claude_code() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Removing MCP server from Claude Code...");

    let status = std::process::Command::new("claude")
        .args(["mcp", "remove", "lk-knowledge"])
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("Successfully removed lk-knowledge MCP server from Claude Code.");
            Ok(())
        }
        Ok(s) => Err(format!(
            "claude mcp remove exited with status: {}",
            s.code().unwrap_or(-1)
        )
        .into()),
        Err(e) => Err(format!(
            "Failed to run 'claude' command. Is Claude Code installed? Error: {e}"
        )
        .into()),
    }
}

fn uninstall_claude_desktop() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Removing MCP server from Claude Desktop...");

    let config_path = get_claude_desktop_config_path();
    if !config_path.exists() {
        eprintln!("Config file not found, nothing to remove.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&config_path)?;
    let mut config: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        if servers.remove("lk-knowledge").is_some() {
            let output = serde_json::to_string_pretty(&config)?;
            std::fs::write(&config_path, output + "\n")?;
            eprintln!("Successfully removed lk-knowledge MCP server from Claude Desktop.");
        } else {
            eprintln!("lk-knowledge was not found in Claude Desktop config.");
        }
    } else {
        eprintln!("No mcpServers found in Claude Desktop config.");
    }

    Ok(())
}

fn get_claude_desktop_config_path() -> PathBuf {
    let home = util::home_dir();
    home.join("Library")
        .join("Application Support")
        .join("Claude")
        .join("claude_desktop_config.json")
}
