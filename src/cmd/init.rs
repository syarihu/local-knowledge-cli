use crate::cmd::sync::sync_knowledge_dir;
use crate::cmd::update::install_embedded_commands;
use crate::db;
use crate::util::get_project_root;

pub fn cmd_init(global: bool) -> Result<(), Box<dyn std::error::Error>> {
    if global {
        return cmd_init_global();
    }
    let root = get_project_root();
    let db_path = root.join(".knowledge").join("knowledge.db");

    // 1. Initialize DB
    if db_path.exists() {
        println!("DB already exists at {}", db_path.display());
    } else {
        db::init_db(&db_path)?;
        println!("Created DB at {}", db_path.display());
    }

    // 2. Create .knowledge/ directory structure
    let knowledge_dir = root.join(".knowledge");
    std::fs::create_dir_all(&knowledge_dir)?;
    if !knowledge_dir.join("README.md").exists() {
        std::fs::write(
            knowledge_dir.join("README.md"),
            "# Project Knowledge Base\n\n\
             This directory contains shared knowledge files for the project.\n\
             These files are managed by `lk` (local-knowledge-cli) and synced to a local SQLite DB.\n\n\
             ## Structure\n\
             - `architecture/` - System design and architecture knowledge\n\
             - `features/` - Feature-specific knowledge\n\
             - `conventions/` - Coding conventions and patterns\n\n\
             ## Format\n\
             Each markdown file uses YAML frontmatter for metadata and `## Entry:` headings to delimit entries.\n",
        )?;
        for subdir in ["architecture", "features", "conventions"] {
            std::fs::create_dir_all(knowledge_dir.join(subdir))?;
        }
        println!(
            "Created .knowledge/ directory at {}",
            knowledge_dir.display()
        );
    }

    // 3. Import existing .knowledge/ files
    let (conn, _) = db::open_db(&db_path)?;
    let stats = sync_knowledge_dir(&conn, &knowledge_dir, &root)?;
    if stats.added > 0 {
        println!("Imported {} entries from .knowledge/", stats.added);
    }

    // 4. Update .gitignore
    let gitignore_path = root.join(".gitignore");
    let gitignore_entries = [
        ".knowledge/knowledge.db",
        // WAL mode leaves these two beside the DB whenever it is open; they are
        // machine state, and a stray `git add -A` picks them up otherwise.
        ".knowledge/knowledge.db-wal",
        ".knowledge/knowledge.db-shm",
        ".knowledge/knowledge.db.bak.*",
        ".knowledge/search.log",
        ".knowledge/command.log",
    ];
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        let mut added = Vec::new();
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            let mut needs_newline = !content.ends_with('\n');
            for entry in &gitignore_entries {
                if !content.contains(entry) {
                    if needs_newline {
                        writeln!(f)?;
                        needs_newline = false;
                    }
                    writeln!(f, "{entry}")?;
                    added.push(*entry);
                }
            }
        }
        for entry in &added {
            println!("Added {entry} to .gitignore");
        }
    } else {
        let content = gitignore_entries.join("\n") + "\n";
        std::fs::write(&gitignore_path, content)?;
        println!("Created .gitignore");
    }

    // 5. Update .gitattributes based on gitattributes_generated config
    let config = crate::config::Config::load(&knowledge_dir);
    let gitattributes_path = root.join(".gitattributes");
    let gitattributes_entry = ".knowledge/**/*.md linguist-generated=true";
    let legacy_gitattributes_entry = ".knowledge/*.md linguist-generated=true";

    // Migrate legacy pattern if present
    if gitattributes_path.exists() {
        let content = std::fs::read_to_string(&gitattributes_path)?;
        if content.contains(legacy_gitattributes_entry) && !content.contains(gitattributes_entry) {
            let new_content = content.replace(legacy_gitattributes_entry, gitattributes_entry);
            std::fs::write(&gitattributes_path, new_content)?;
            println!("Migrated .gitattributes pattern to {gitattributes_entry}");
        }
    }

    if config.gitattributes_generated {
        // Add the entry
        if gitattributes_path.exists() {
            let content = std::fs::read_to_string(&gitattributes_path)?;
            if !content.contains(gitattributes_entry) {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&gitattributes_path)?;
                if !content.ends_with('\n') {
                    writeln!(f)?;
                }
                writeln!(f, "{gitattributes_entry}")?;
                println!("Added {gitattributes_entry} to .gitattributes");
            }
        } else {
            std::fs::write(&gitattributes_path, format!("{gitattributes_entry}\n"))?;
            println!("Created .gitattributes");
        }
    } else if gitattributes_path.exists() {
        // Remove the entry (and legacy pattern) if they exist
        let content = std::fs::read_to_string(&gitattributes_path)?;
        if content.contains(gitattributes_entry) || content.contains(legacy_gitattributes_entry) {
            let new_content = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    trimmed != gitattributes_entry && trimmed != legacy_gitattributes_entry
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            std::fs::write(&gitattributes_path, new_content)?;
            println!("Removed linguist-generated entry from .gitattributes");
        }
    }

    // 6. Write instructions to .knowledge/lk-instructions.md and add import to CLAUDE.md
    let instructions_path = knowledge_dir.join("lk-instructions.md");
    let instructions_content = LK_INSTRUCTIONS_CONTENT;

    if instructions_path.exists() {
        let existing = std::fs::read_to_string(&instructions_path)?;
        if existing.trim() != instructions_content.trim() {
            std::fs::write(&instructions_path, instructions_content)?;
            println!("Updated {}", instructions_path.display());
        } else {
            println!("{} is already up-to-date", instructions_path.display());
        }
    } else {
        std::fs::write(&instructions_path, instructions_content)?;
        println!("Created {}", instructions_path.display());
    }

    // Add import line to CLAUDE.md
    // Priority: root CLAUDE.md > .claude/CLAUDE.md > create root CLAUDE.md
    let candidates = [
        root.join("CLAUDE.md"),
        root.join(".claude").join("CLAUDE.md"),
    ];

    let import_line = "@.knowledge/lk-instructions.md";
    let old_import_line = "@.claude/lk-instructions.md";
    let old_marker = "## Knowledge Base (local-knowledge-cli)";

    // Migrate from legacy .claude/lk-instructions.md if it exists
    let legacy_instructions_path = root.join(".claude").join("lk-instructions.md");
    if legacy_instructions_path.exists() {
        std::fs::remove_file(&legacy_instructions_path)?;
        println!("Migrated .claude/lk-instructions.md -> .knowledge/lk-instructions.md");
    }

    // Migrate: if AGENTS.md exists, remove any lk import or legacy marker from it
    let agents_md_path = root.join("AGENTS.md");
    if agents_md_path.exists() {
        let content = std::fs::read_to_string(&agents_md_path)?;
        let has_import = content.contains(import_line) || content.contains(old_import_line);
        let has_marker = content.contains(old_marker);

        if has_import || has_marker {
            let mut new_content = content.clone();
            if has_import {
                let lines: Vec<&str> = new_content
                    .lines()
                    .filter(|line| {
                        let trimmed = line.trim();
                        trimmed != import_line && trimmed != old_import_line
                    })
                    .collect();
                new_content = lines.join("\n");
                if !new_content.ends_with('\n') && !new_content.is_empty() {
                    new_content.push('\n');
                }
            }
            if let Some(section_start) = new_content.find(old_marker) {
                let rest = &new_content[section_start + old_marker.len()..];
                let section_end = rest
                    .match_indices("\n## ")
                    .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                    .map(|(i, _)| section_start + old_marker.len() + i)
                    .unwrap_or(new_content.len());

                let mut trimmed = new_content[..section_start].to_string();
                if section_end < new_content.len() {
                    trimmed.push_str(&new_content[section_end..]);
                }
                new_content = trimmed;
            }

            // Collapse excessive blank lines
            while new_content.contains("\n\n\n") {
                new_content = new_content.replace("\n\n\n", "\n\n");
            }

            if new_content.trim().is_empty() {
                std::fs::remove_file(&agents_md_path)?;
                println!("Migrated lk import from AGENTS.md and removed empty file");
            } else {
                std::fs::write(&agents_md_path, new_content)?;
                println!("Migrated lk import out of {}", agents_md_path.display());
            }
        }
    }

    // Migrate legacy import line in CLAUDE.md
    for candidate in &candidates {
        if candidate.exists() {
            let content = std::fs::read_to_string(candidate)?;
            if content.contains(old_import_line) {
                let new_content = content.replace(old_import_line, import_line);
                std::fs::write(candidate, new_content)?;
                println!("Updated import path in {}", candidate.display());
            }
        }
    }

    // Check if any candidate already contains the import line
    let already_imported = candidates.iter().any(|p| {
        p.exists()
            && std::fs::read_to_string(p)
                .map(|c| c.contains(import_line))
                .unwrap_or(false)
    });

    if already_imported {
        println!("lk import already exists in a project config file");
    } else {
        // Find the first existing file, or create CLAUDE.md
        let target_path = candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .unwrap_or_else(|| root.join("CLAUDE.md"));

        if target_path.exists() {
            let content = std::fs::read_to_string(&target_path)?;

            if content.contains(old_marker) {
                // Migrate: replace old inline section with import line
                let section_start = content.find(old_marker).unwrap();
                let rest = &content[section_start + old_marker.len()..];
                let section_end = rest
                    .match_indices("\n## ")
                    .find(|(i, _)| !rest[i + 4..].starts_with('#'))
                    .map(|(i, _)| section_start + old_marker.len() + i)
                    .unwrap_or(content.len());

                let mut new_content = content[..section_start].to_string();
                new_content.push_str(import_line);
                new_content.push('\n');
                if section_end < content.len() {
                    new_content.push_str(&content[section_end..]);
                }
                std::fs::write(&target_path, new_content)?;
                println!(
                    "Migrated inline instructions to import in {}",
                    target_path.display()
                );
            } else {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&target_path)?;
                if !content.ends_with('\n') {
                    writeln!(f)?;
                }
                writeln!(f, "{import_line}")?;
                println!("Added import to {}", target_path.display());
            }
        } else {
            std::fs::write(&target_path, format!("{import_line}\n"))?;
            println!("Created {} with lk import", target_path.display());
        }
    }

    // 7. Create config.toml if it doesn't exist
    let config_path = knowledge_dir.join("config.toml");
    if !config_path.exists() {
        std::fs::write(&config_path, crate::config::DEFAULT_CONFIG_CONTENT)?;
        println!("Created {}", config_path.display());
    }

    // 8. Write .knowledge/.lk-version
    let version_path = knowledge_dir.join(".lk-version");
    std::fs::write(&version_path, format!("{}\n", crate::util::VERSION))?;

    // 9. Install embedded Claude commands
    install_embedded_commands()?;

    println!("\nInitialization complete!");
    Ok(())
}

fn cmd_init_global() -> Result<(), Box<dyn std::error::Error>> {
    let claude_dir = crate::util::home_dir().join(".claude");
    std::fs::create_dir_all(&claude_dir)?;

    // 1. Write lk-instructions.md to ~/.claude/
    let instructions_path = claude_dir.join("lk-instructions.md");
    let instructions_content = LK_INSTRUCTIONS_CONTENT;

    if instructions_path.exists() {
        let existing = std::fs::read_to_string(&instructions_path)?;
        if existing.trim() != instructions_content.trim() {
            std::fs::write(&instructions_path, instructions_content)?;
            println!("Updated {}", instructions_path.display());
        } else {
            println!("{} is already up-to-date", instructions_path.display());
        }
    } else {
        std::fs::write(&instructions_path, instructions_content)?;
        println!("Created {}", instructions_path.display());
    }

    // 2. Add @lk-instructions.md to ~/.claude/CLAUDE.md
    let claude_md_path = claude_dir.join("CLAUDE.md");
    let import_line = "@lk-instructions.md";

    if claude_md_path.exists() {
        let content = std::fs::read_to_string(&claude_md_path)?;
        if content.contains(import_line) {
            println!("lk import already exists in {}", claude_md_path.display());
        } else {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&claude_md_path)?;
            if !content.ends_with('\n') {
                writeln!(f)?;
            }
            writeln!(f, "{import_line}")?;
            println!("Added import to {}", claude_md_path.display());
        }
    } else {
        std::fs::write(&claude_md_path, format!("{import_line}\n"))?;
        println!("Created {} with lk import", claude_md_path.display());
    }

    // 3. Install embedded commands
    install_embedded_commands()?;

    println!("\nGlobal initialization complete!");
    Ok(())
}

pub(crate) const LK_INSTRUCTIONS_CONTENT: &str =
    include_str!("../../.knowledge/lk-instructions.md");

/// Refresh an existing lk-instructions.md with the embedded content if it differs.
/// Does nothing when the file is absent, so it never imposes lk on a location that
/// hasn't opted in via `lk init`. lk-instructions.md is generated (not hand-edited),
/// so overwriting on change is safe. Returns `Ok(true)` when the file was rewritten.
pub fn refresh_instructions_if_exists(
    path: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !path.is_file() {
        return Ok(false);
    }
    let existing = std::fs::read_to_string(path)?;
    if existing.trim() != LK_INSTRUCTIONS_CONTENT.trim() {
        std::fs::write(path, LK_INSTRUCTIONS_CONTENT)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
