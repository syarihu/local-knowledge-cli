use crate::cmd::sync::sync_knowledge_dir;
use crate::cmd::update::install_embedded_commands;
use crate::db;
use crate::util::get_project_root;

pub fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
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
    let gitattributes_entry = ".knowledge/*.md linguist-generated=true";
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
        // Remove the entry if it exists
        let content = std::fs::read_to_string(&gitattributes_path)?;
        if content.contains(gitattributes_entry) {
            let new_content = content
                .lines()
                .filter(|line| line.trim() != gitattributes_entry)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            std::fs::write(&gitattributes_path, new_content)?;
            println!("Removed {gitattributes_entry} from .gitattributes");
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

    // Add import line to AGENTS.md or CLAUDE.md
    // Priority: root AGENTS.md > root CLAUDE.md > .claude/CLAUDE.md > create root AGENTS.md
    let candidates = [
        root.join("AGENTS.md"),
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

    // Migrate legacy import line in AGENTS.md / CLAUDE.md
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
        // Find the first existing file, or create AGENTS.md
        let target_path = candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .unwrap_or_else(|| root.join("AGENTS.md"));

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

const LK_INSTRUCTIONS_CONTENT: &str = include_str!("../../.knowledge/lk-instructions.md");
