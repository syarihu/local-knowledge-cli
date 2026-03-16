use crate::util::{confirm, get_project_root};

pub fn cmd_uninstall(yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = get_project_root();
    let knowledge_dir = root.join(".knowledge");
    let old_marker = "## Knowledge Base (local-knowledge-cli)";
    let import_line = "@.claude/lk-instructions.md";

    if !yes
        && !confirm(&format!(
            "Uninstall lk from project {}? This will remove .knowledge/ and all data.",
            root.display()
        ))
    {
        println!("Cancelled.");
        return Ok(());
    }

    println!("Uninstalling lk from project: {}", root.display());
    println!();

    // 1. Remove .knowledge/ directory
    if knowledge_dir.exists() {
        std::fs::remove_dir_all(&knowledge_dir)?;
        println!("  Removed .knowledge/");
    }

    // 2. Remove .claude/lk-instructions.md
    let instructions_path = root.join(".claude").join("lk-instructions.md");
    if instructions_path.exists() {
        std::fs::remove_file(&instructions_path)?;
        println!("  Removed .claude/lk-instructions.md");
    }

    // 3. Remove import line and old inline section from CLAUDE.md / AGENTS.md
    let candidates = [
        root.join("CLAUDE.md"),
        root.join("AGENTS.md"),
        root.join(".claude").join("CLAUDE.md"),
    ];
    for claude_md_path in &candidates {
        if !claude_md_path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(claude_md_path)?;
        let mut modified = false;
        let mut new_content = content.clone();

        // Remove @import line
        if new_content.contains(import_line) {
            let lines: Vec<&str> = new_content
                .lines()
                .filter(|line| line.trim() != import_line)
                .collect();
            new_content = lines.join("\n");
            if !new_content.ends_with('\n') && !new_content.is_empty() {
                new_content.push('\n');
            }
            modified = true;
        }

        // Remove old inline section (legacy format)
        if let Some(section_start) = new_content.find(old_marker) {
            let rest = &new_content[section_start + old_marker.len()..];
            let section_end = rest
                .match_indices("\n## ")
                .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                .map(|(i, _)| section_start + old_marker.len() + i)
                .unwrap_or(new_content.len());

            let before = new_content[..section_start].trim_end_matches('\n');
            let after = &new_content[section_end..];
            new_content = if after.trim().is_empty() {
                if before.is_empty() {
                    String::new()
                } else {
                    format!("{before}\n")
                }
            } else {
                format!("{before}\n{after}")
            };
            modified = true;
        }

        if modified {
            // Collapse excessive blank lines (3+ newlines → 2)
            while new_content.contains("\n\n\n") {
                new_content = new_content.replace("\n\n\n", "\n\n");
            }

            if new_content.trim().is_empty() {
                std::fs::remove_file(claude_md_path)?;
                println!(
                    "  Removed {} (was empty after cleanup)",
                    claude_md_path.display()
                );
            } else {
                std::fs::write(claude_md_path, &new_content)?;
                println!("  Cleaned up {}", claude_md_path.display());
            }
        }
    }

    // 4. Remove lk entries from .gitignore
    let gitignore_path = root.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        let lk_entries = [
            ".knowledge/knowledge.db",
            ".knowledge/knowledge.db.bak.*",
            ".knowledge/search.log",
            ".knowledge/command.log",
            // Legacy paths
            ".claude/knowledge.db",
            ".claude/search.log",
        ];
        let new_lines: Vec<&str> = content
            .lines()
            .filter(|line| !lk_entries.contains(&line.trim()))
            .collect();
        let new_content = new_lines.join("\n");
        if new_content.trim().is_empty() {
            std::fs::remove_file(&gitignore_path)?;
            println!("  Removed .gitignore (was empty after cleanup)");
        } else {
            let new_content = format!("{}\n", new_content.trim_end());
            std::fs::write(&gitignore_path, new_content)?;
            println!("  Removed lk entries from .gitignore");
        }
    }

    println!("\nProject cleanup complete!");
    println!();
    println!("To fully uninstall lk from your system, also run:");
    println!("  rm -rf ~/.claude/commands/lk-knowledge-*.md");
    println!("  rm -rf ~/.config/lk");
    if is_homebrew_install() {
        println!("  brew uninstall lk");
    } else {
        println!("  rm ~/.local/bin/lk");
    }

    Ok(())
}

/// Detect if the currently running binary was installed via Homebrew.
fn is_homebrew_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| {
            let s = p.to_string_lossy();
            s.contains("/homebrew/") || s.contains("/Cellar/") || s.contains("/linuxbrew/")
        })
        .unwrap_or(false)
}
