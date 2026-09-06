pub struct AgentSetupTarget {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub template: &'static str,
}

const AGY_TEMPLATE: &str = r#"# Connect local-knowledge (`lk`) with Antigravity (`agy`)

Pass the following instructions to Antigravity (or run within an Antigravity session):

---
Please configure my Antigravity environment to connect to the local-knowledge (`lk`) MCP server.

## 1. Prerequisites Check
1. Verify that `lk` is installed and executable in PATH:
   Run `command -v lk` or `which lk`. If not found, stop and notify me to install `lk` first.
2. Verify that `lk mcp` starts without fatal errors.

## 2. Configuration Setup
Locate the global MCP configuration file:
- Primary location: `~/.gemini/config/mcp_config.json`

Safety rules:
- **Preserve Symlinks**: If `~/.gemini/config/mcp_config.json` is a symlink (e.g. pointing to a dotfiles repo), resolve and edit the target file directly. Do not replace the symlink with a regular file.
- **Merge Existing Servers**: Do not overwrite `mcpServers`. Read the existing JSON (create `{ "mcpServers": {} }` if missing or empty), preserve all existing server definitions, and insert or update the `lk-knowledge` entry.

Entry definition:
```json
{
  "mcpServers": {
    "lk-knowledge": {
      "command": "lk",
      "args": ["mcp"]
    }
  }
}
```

## 3. Verification
Confirm the configuration is valid JSON and report back the updated configuration file path.
---
"#;

const CURSOR_TEMPLATE: &str = r#"# Connect local-knowledge (`lk`) with Cursor

Pass the following instructions to Cursor (or run within a Cursor session):

---
Please configure my Cursor environment to connect to the local-knowledge (`lk`) MCP server.

## 1. Prerequisites Check
Verify that `lk` is installed and accessible via PATH (`command -v lk`).

## 2. Configuration Setup
Cursor supports global MCP configuration in `~/.cursor/mcp.json` or per-workspace in `.cursor/mcp.json`.
Unless instructed otherwise, configure the global configuration at `~/.cursor/mcp.json`.

Safety rules:
- **Preserve Symlinks**: If `~/.cursor/mcp.json` is a symlink (e.g. pointing to a dotfiles repo), resolve and edit the target file directly. Do not replace the symlink with a regular file.
- **Merge Existing Servers**: Merge into the existing `mcpServers` object without removing other configured servers.

Entry definition:
```json
{
  "mcpServers": {
    "lk-knowledge": {
      "command": "lk",
      "args": ["mcp"]
    }
  }
}
```

## 3. Verification
Confirm the configuration is valid JSON and report back the updated configuration file path.
---
"#;

const CLAUDE_TEMPLATE: &str = r#"# Connect local-knowledge (`lk`) with Claude Code & Claude Desktop

### For Claude Code
Run the following command in your terminal:
```bash
claude mcp add --transport stdio lk-knowledge -- lk mcp
```
Or instruct Claude Code:
"Run `claude mcp add --transport stdio lk-knowledge -- lk mcp` to register the lk-knowledge MCP server."

### For Claude Desktop
Add `lk-knowledge` to `~/Library/Application Support/Claude/claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "lk-knowledge": {
      "command": "lk",
      "args": ["mcp"]
    }
  }
}
```
*(Alternatively, you can continue using `lk install-mcp --target claude-desktop`)*
"#;

const CODEX_TEMPLATE: &str = r#"# Connect local-knowledge (`lk`) with Codex

Pass the following instructions to Codex:

---
Please configure Codex to connect to the local-knowledge (`lk`) MCP server.

1. Ensure `lk` is executable in PATH (`command -v lk`).
2. In Codex's MCP configuration (`~/.codex/config.json` or active configuration file), register:
```json
{
  "mcpServers": {
    "lk-knowledge": {
      "command": "lk",
      "args": ["mcp"]
    }
  }
}
```
Ensure symlinks are preserved and existing servers remain intact.
---
"#;

pub const TARGETS: &[AgentSetupTarget] = &[
    AgentSetupTarget {
        id: "agy",
        aliases: &["antigravity", "gemini"],
        description: "Antigravity (agy) MCP configuration (~/.gemini/config/mcp_config.json)",
        template: AGY_TEMPLATE,
    },
    AgentSetupTarget {
        id: "cursor",
        aliases: &[],
        description: "Cursor MCP configuration (~/.cursor/mcp.json)",
        template: CURSOR_TEMPLATE,
    },
    AgentSetupTarget {
        id: "claude",
        aliases: &["claude-code", "claude-desktop"],
        description: "Claude Code (claude mcp add) and Claude Desktop",
        template: CLAUDE_TEMPLATE,
    },
    AgentSetupTarget {
        id: "codex",
        aliases: &[],
        description: "Codex MCP configuration",
        template: CODEX_TEMPLATE,
    },
];

pub fn cmd_setup(target: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    match target {
        None => {
            println!("{:<8} WHAT IT COVERS", "AGENT");
            for t in TARGETS {
                println!("{:<8} {}", t.id, t.description);
            }
            println!("{:<8} Output setup instructions for all agents", "all");
            println!("\nTo view instructions: lk setup <agent>");
            Ok(())
        }
        Some("all") => {
            for (i, t) in TARGETS.iter().enumerate() {
                if i > 0 {
                    println!("\n---\n");
                }
                print!("{}", t.template);
            }
            Ok(())
        }
        Some(name) => {
            let normalized = name.trim().to_ascii_lowercase();
            if let Some(target) = TARGETS
                .iter()
                .find(|t| t.id == normalized || t.aliases.contains(&normalized.as_str()))
            {
                print!("{}", target.template);
                Ok(())
            } else {
                let available: Vec<&str> = TARGETS.iter().map(|t| t.id).chain(["all"]).collect();
                Err(format!(
                    "Unknown agent: '{name}'. Available: {}.\n\
                     Run `lk setup` without arguments to see available agents.",
                    available.join(", ")
                )
                .into())
            }
        }
    }
}
