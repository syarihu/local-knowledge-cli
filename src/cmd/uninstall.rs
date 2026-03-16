use crate::util::{confirm, get_project_root};

pub fn cmd_uninstall(yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = get_project_root();
    let knowledge_dir = root.join(".knowledge");
    let marker = "## Knowledge Base (local-knowledge-cli)";

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

    // 2. Remove section from CLAUDE.md
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
        if let Some(section_start) = content.find(marker) {
            let rest = &content[section_start + marker.len()..];
            let section_end = rest
                .match_indices("\n## ")
                .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                .map(|(i, _)| section_start + marker.len() + i)
                .unwrap_or(content.len());

            // Trim trailing whitespace before the section too
            let before = content[..section_start].trim_end_matches('\n');
            let after = &content[section_end..];
            let new_content = if after.trim().is_empty() {
                if before.is_empty() {
                    String::new()
                } else {
                    format!("{before}\n")
                }
            } else {
                format!("{before}\n{after}")
            };

            if new_content.trim().is_empty() {
                std::fs::remove_file(claude_md_path)?;
                println!(
                    "  Removed {} (was empty after section removal)",
                    claude_md_path.display()
                );
            } else {
                std::fs::write(claude_md_path, new_content)?;
                println!(
                    "  Removed knowledge section from {}",
                    claude_md_path.display()
                );
            }
        }
    }

    // 3. Remove lk entries from .gitignore
    let gitignore_path = root.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        let lk_entries = [
            ".knowledge/knowledge.db",
            ".knowledge/search.log",
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
