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
        let mut tracker = CodeFenceTracker::default();
        let has_import = content.lines().any(|line| {
            let inside = tracker.process_line(line);
            !inside
                && !tracker.is_in_code_block()
                && (is_matching_import(line, import_line)
                    || is_matching_import(line, old_import_line))
        });
        let has_marker = find_heading_pos(&content, old_marker).is_some();

        if has_import || has_marker {
            let newline = detect_newline(&content);
            let mut new_content = content.clone();
            if has_import {
                let mut filter_tracker = CodeFenceTracker::default();
                let lines: Vec<&str> = new_content
                    .lines()
                    .filter(|line| {
                        let inside = filter_tracker.process_line(line);
                        if !inside && !filter_tracker.is_in_code_block() {
                            !is_matching_import(line, import_line)
                                && !is_matching_import(line, old_import_line)
                        } else {
                            true
                        }
                    })
                    .collect();
                new_content = lines.join(newline);
                if content.ends_with('\n') && !new_content.is_empty() {
                    new_content.push_str(newline);
                }
            }
            if let Some(section_start) = find_heading_pos(&new_content, old_marker) {
                let rest = &new_content[section_start..];
                let heading_line_len = rest
                    .split_inclusive('\n')
                    .next()
                    .map(|l| l.len())
                    .unwrap_or(rest.len());
                let body = &rest[heading_line_len..];
                let section_end = find_next_h1_or_h2_pos(body)
                    .map(|offset| section_start + heading_line_len + offset)
                    .unwrap_or(new_content.len());

                let mut trimmed = new_content[..section_start].to_string();
                if section_end < new_content.len() {
                    trimmed.push_str(&new_content[section_end..]);
                }
                new_content = trimmed;
            }

            // Collapse excessive blank lines outside code fences
            new_content = collapse_blank_lines(&new_content);

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
            let mut check_tracker = CodeFenceTracker::default();
            let mut has_old_import = false;
            let mut has_new_import = false;
            for l in content.lines() {
                let inside = check_tracker.process_line(l);
                if !inside && !check_tracker.is_in_code_block() {
                    if is_matching_import(l, old_import_line) {
                        has_old_import = true;
                    }
                    if is_matching_import(l, import_line) {
                        has_new_import = true;
                    }
                }
            }
            if has_old_import {
                let newline = detect_newline(&content);
                let mut replace_tracker = CodeFenceTracker::default();
                let mut replaced_or_already_present = has_new_import;
                let mut lines: Vec<String> = Vec::new();
                for l in content.lines() {
                    let inside = replace_tracker.process_line(l);
                    if !inside
                        && !replace_tracker.is_in_code_block()
                        && is_matching_import(l, old_import_line)
                    {
                        if !replaced_or_already_present {
                            lines.push(import_line.to_string());
                            replaced_or_already_present = true;
                        }
                    } else {
                        lines.push(l.to_string());
                    }
                }
                let mut new_content = lines.join(newline);
                if content.ends_with('\n') && !new_content.is_empty() {
                    new_content.push_str(newline);
                }
                std::fs::write(candidate, new_content)?;
                println!("Updated import path in {}", candidate.display());
            }
        }
    }

    // Check if any candidate already contains the import line
    let already_imported = candidates.iter().any(|p| {
        p.exists()
            && std::fs::read_to_string(p)
                .map(|c| {
                    let mut tracker = CodeFenceTracker::default();
                    c.lines().any(|l| {
                        let inside = tracker.process_line(l);
                        !inside && !tracker.is_in_code_block() && is_matching_import(l, import_line)
                    })
                })
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

            if let Some(section_start) = find_heading_pos(&content, old_marker) {
                // Migrate: replace old inline section with import line
                let rest = &content[section_start..];
                let heading_line_len = rest
                    .split_inclusive('\n')
                    .next()
                    .map(|l| l.len())
                    .unwrap_or(rest.len());
                let body = &rest[heading_line_len..];
                let section_end = find_next_h1_or_h2_pos(body)
                    .map(|offset| section_start + heading_line_len + offset)
                    .unwrap_or(content.len());

                let newline = detect_newline(&content);
                let double_newline = if newline == "\r\n" {
                    "\r\n\r\n"
                } else {
                    "\n\n"
                };
                let mut new_content = content[..section_start].to_string();
                new_content.push_str(import_line);
                new_content.push_str(newline);
                if section_end < content.len() {
                    if !new_content.ends_with(double_newline) {
                        new_content.push_str(newline);
                    }
                    new_content.push_str(&content[section_end..]);
                }
                new_content = collapse_blank_lines(&new_content);
                std::fs::write(&target_path, new_content)?;
                println!(
                    "Migrated inline instructions to import in {}",
                    target_path.display()
                );
            } else {
                use std::io::Write;
                let newline = detect_newline(&content);
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&target_path)?;
                if !content.ends_with('\n') {
                    write!(f, "{newline}")?;
                }
                write!(f, "{import_line}{newline}")?;
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
        let mut tracker = CodeFenceTracker::default();
        let already_imported = content.lines().any(|l| {
            let inside = tracker.process_line(l);
            !inside && !tracker.is_in_code_block() && is_matching_import(l, import_line)
        });
        if already_imported {
            println!("lk import already exists in {}", claude_md_path.display());
        } else {
            use std::io::Write;
            let newline = detect_newline(&content);
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&claude_md_path)?;
            if !content.ends_with('\n') {
                write!(f, "{newline}")?;
            }
            write!(f, "{import_line}{newline}")?;
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

/// Returns "\r\n" if `content` uses CRLF newlines exclusively, otherwise defaults to "\n".
/// If the file has mixed line endings or lone LF characters, "\n" is returned to avoid churn.
fn detect_newline(content: &str) -> &'static str {
    if !content.contains("\r\n") {
        return "\n";
    }
    let bytes = content.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
            return "\n";
        }
    }
    "\r\n"
}

/// If `line` has up to 3 leading ASCII spaces (and no leading tab) followed by a non-whitespace
/// character, returns the remainder of the line starting from that character.
/// In CommonMark, block elements (fences, ATX headings) allow at most 3 spaces of indentation;
/// lines indented by 4 or more spaces, tabs, or whitespace-only lines (including empty lines) return `None`.
fn strip_indent_up_to_3(line: &str) -> Option<&str> {
    let mut space_count = 0;
    for (i, c) in line.char_indices() {
        if c == ' ' {
            space_count += 1;
            if space_count > 3 {
                return None;
            }
        } else if c == '\t' || c == '\r' || c == '\n' {
            return None;
        } else {
            return Some(&line[i..]);
        }
    }
    None
}

/// Returns true if `line` matches `target` after stripping up to 3 leading ASCII spaces
/// and trimming trailing whitespace. If the line is indented by 4+ spaces or tabs,
/// it is considered an indented code block in CommonMark and returns false.
fn is_matching_import(line: &str, target: &str) -> bool {
    strip_indent_up_to_3(line).is_some_and(|r| r.trim_end() == target)
}

/// Tracks CommonMark fenced code blocks (``` or ~~~) across lines.
#[derive(Default)]
struct CodeFenceTracker {
    active_fence: Option<(char, usize)>,
}

impl CodeFenceTracker {
    fn is_in_code_block(&self) -> bool {
        self.active_fence.is_some()
    }

    /// Process a line. If the line opens or closes a fence, updates state and returns false.
    /// Returns true if this line is content inside an active code block.
    fn process_line(&mut self, line: &str) -> bool {
        if let Some((fence_char, min_len)) = self.active_fence {
            if let Some(rest) = strip_indent_up_to_3(line) {
                let trimmed = rest.trim_end();
                let fence_count = trimmed.chars().take_while(|&c| c == fence_char).count();
                if fence_count >= min_len && trimmed[fence_count..].trim().is_empty() {
                    self.active_fence = None;
                    return false;
                }
            }
            true
        } else if let Some(rest) = strip_indent_up_to_3(line) {
            let trimmed = rest.trim_end();
            if let Some(first @ ('`' | '~')) = trimmed.chars().next() {
                let count = trimmed.chars().take_while(|&c| c == first).count();
                if count >= 3 {
                    let after_fence = &trimmed[count..];
                    if first == '`' && after_fence.contains('`') {
                        return false;
                    }
                    self.active_fence = Some((first, count));
                    return false;
                }
            }
            false
        } else {
            false
        }
    }
}

/// Find the byte offset where a markdown heading appears as a standalone line in content
/// outside of any fenced code block.
fn find_heading_pos(content: &str, heading: &str) -> Option<usize> {
    let mut tracker = CodeFenceTracker::default();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let inside_code = tracker.process_line(line);
        if !inside_code
            && !tracker.is_in_code_block()
            && strip_indent_up_to_3(line).is_some_and(|r| r.trim_end() == heading)
        {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Find the byte offset in `body` where the next H1 (`# ` or `#\t`) or H2 (`## ` or `##\t`) heading begins.
/// Skips any headings that occur inside code fences.
fn find_next_h1_or_h2_pos(body: &str) -> Option<usize> {
    let mut tracker = CodeFenceTracker::default();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let inside_code = tracker.process_line(line);
        if !inside_code
            && !tracker.is_in_code_block()
            && strip_indent_up_to_3(line).is_some_and(|r| {
                let trimmed = r.trim_end();
                trimmed.starts_with("# ")
                    || trimmed.starts_with("#\t")
                    || trimmed.starts_with("## ")
                    || trimmed.starts_with("##\t")
            })
        {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Collapses runs of multiple consecutive blank lines outside code fences down to a single blank line.
/// Blank lines inside fenced code blocks are preserved.
fn collapse_blank_lines(content: &str) -> String {
    let newline = detect_newline(content);
    let mut tracker = CodeFenceTracker::default();
    let mut consecutive_blank = 0;
    let mut result_lines = Vec::new();

    for line in content.lines() {
        let inside_code = tracker.process_line(line);
        if inside_code || tracker.is_in_code_block() {
            consecutive_blank = 0;
            result_lines.push(line);
        } else if line.trim().is_empty() {
            consecutive_blank += 1;
            if consecutive_blank <= 1 {
                result_lines.push(line);
            }
        } else {
            consecutive_blank = 0;
            result_lines.push(line);
        }
    }

    let mut result = result_lines.join(newline);
    if content.ends_with('\n') && !result.is_empty() {
        result.push_str(newline);
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_fence_tracker_backticks_and_tildes() {
        let mut tracker = CodeFenceTracker::default();
        assert!(!tracker.process_line("```markdown"));
        assert!(tracker.is_in_code_block());

        // ~~~ inside ``` does not close it
        assert!(tracker.process_line("~~~"));
        assert!(tracker.is_in_code_block());

        // Heading inside ``` is inside code block
        assert!(tracker.process_line("## Heading"));
        assert!(tracker.is_in_code_block());

        // Closing ``` closes it
        assert!(!tracker.process_line("```"));
        assert!(!tracker.is_in_code_block());

        // Outside fence again
        assert!(!tracker.process_line("## Heading"));
        assert!(!tracker.is_in_code_block());
    }

    #[test]
    fn test_code_fence_tracker_longer_fence_requires_matching_length() {
        let mut tracker = CodeFenceTracker::default();
        // 4 backticks opens a fence requiring at least 4 backticks to close
        assert!(!tracker.process_line("````"));
        assert!(tracker.is_in_code_block());

        // 3 backticks inside is just content
        assert!(tracker.process_line("```"));
        assert!(tracker.is_in_code_block());

        // 4 backticks closes it
        assert!(!tracker.process_line("````"));
        assert!(!tracker.is_in_code_block());
    }

    #[test]
    fn test_find_heading_pos_skips_code_fences() {
        let content = "\
# Documentation

Here is an example:
```markdown
## Knowledge Base (local-knowledge-cli)
inside code fence
```

## Knowledge Base (local-knowledge-cli)
real heading
";
        let pos = find_heading_pos(content, "## Knowledge Base (local-knowledge-cli)");
        assert!(pos.is_some());
        assert!(
            content[pos.unwrap()..]
                .starts_with("## Knowledge Base (local-knowledge-cli)\nreal heading")
        );
    }

    #[test]
    fn test_find_next_h1_or_h2_pos_skips_code_fences() {
        let body = "\
Some text
```markdown
# Fake H1 inside code block
## Fake H2 inside code block
```
More text
# Real H1
";
        let pos = find_next_h1_or_h2_pos(body);
        assert!(pos.is_some());
        assert!(body[pos.unwrap()..].starts_with("# Real H1"));
    }

    #[test]
    fn test_collapse_blank_lines_preserves_code_fences() {
        let content = "\
# Title



Outside text



```rust
fn foo() {



    bar();
}
```



# Next Section
";
        let collapsed = collapse_blank_lines(content);
        assert!(collapsed.contains("# Title\n\nOutside text"));
        assert!(collapsed.contains("```rust\nfn foo() {\n\n\n\n    bar();\n}\n```"));
        assert!(collapsed.contains("```\n\n# Next Section"));
    }

    #[test]
    fn test_code_fence_tracker_indentation_limit() {
        let mut tracker = CodeFenceTracker::default();
        // 1 to 3 spaces is valid fence
        assert!(!tracker.process_line("   ```markdown"));
        assert!(tracker.is_in_code_block());
        assert!(!tracker.process_line("   ```"));
        assert!(!tracker.is_in_code_block());

        // 4 spaces is indented block, NOT a fence
        assert!(!tracker.process_line("    ```markdown"));
        assert!(!tracker.is_in_code_block());

        // Tab is NOT a fence
        assert!(!tracker.process_line("\t```markdown"));
        assert!(!tracker.is_in_code_block());
    }

    #[test]
    fn test_find_heading_pos_rejects_4_space_indented_heading() {
        let content = "\
# Documentation

    ## Knowledge Base (local-knowledge-cli)
    This is an indented code block or list item, not an ATX heading.
";
        let pos = find_heading_pos(content, "## Knowledge Base (local-knowledge-cli)");
        assert!(pos.is_none());
    }

    #[test]
    fn test_is_matching_import() {
        let import = "@.knowledge/lk-instructions.md";
        assert!(is_matching_import("@.knowledge/lk-instructions.md", import));
        assert!(is_matching_import(
            "   @.knowledge/lk-instructions.md",
            import
        ));
        assert!(is_matching_import(
            "@.knowledge/lk-instructions.md   ",
            import
        ));
        assert!(is_matching_import(
            "   @.knowledge/lk-instructions.md   ",
            import
        ));

        // 4+ spaces is an indented code block, not an import
        assert!(!is_matching_import(
            "    @.knowledge/lk-instructions.md",
            import
        ));
        // Tab is an indented code block
        assert!(!is_matching_import(
            "\t@.knowledge/lk-instructions.md",
            import
        ));
        // Mismatched import line
        assert!(!is_matching_import("@.claude/lk-instructions.md", import));
        // Empty or whitespace lines
        assert!(!is_matching_import("", import));
        assert!(!is_matching_import("   ", import));
    }

    #[test]
    fn test_detect_newline() {
        assert_eq!(detect_newline("line1\r\nline2\r\n"), "\r\n");
        assert_eq!(detect_newline("line1\nline2\n"), "\n");
        assert_eq!(detect_newline("single line"), "\n");
        // Mixed line endings default to LF
        assert_eq!(detect_newline("line1\r\nline2\n"), "\n");
        assert_eq!(detect_newline("line1\nline2\r\n"), "\n");
    }

    #[test]
    fn test_collapse_blank_lines_preserves_crlf() {
        let content = "# Title\r\n\r\n\r\nText\r\n";
        let collapsed = collapse_blank_lines(content);
        assert_eq!(collapsed, "# Title\r\n\r\nText\r\n");
    }

    #[test]
    fn test_find_next_h1_or_h2_pos_recognizes_tabs() {
        let body = "Some text\n#\tHeading with tab\n";
        let pos = find_next_h1_or_h2_pos(body);
        assert!(pos.is_some());
        assert!(body[pos.unwrap()..].starts_with("#\tHeading with tab"));

        let body2 = "Some text\n##\tH2 with tab\n";
        let pos2 = find_next_h1_or_h2_pos(body2);
        assert!(pos2.is_some());
        assert!(body2[pos2.unwrap()..].starts_with("##\tH2 with tab"));
    }

    #[test]
    fn test_strip_indent_up_to_3() {
        assert_eq!(strip_indent_up_to_3("foo"), Some("foo"));
        assert_eq!(strip_indent_up_to_3("   foo"), Some("foo"));
        assert_eq!(strip_indent_up_to_3("    foo"), None);
        assert_eq!(strip_indent_up_to_3("\tfoo"), None);
        // Blank lines with \n or \r\n from split_inclusive
        assert_eq!(strip_indent_up_to_3("\n"), None);
        assert_eq!(strip_indent_up_to_3("\r\n"), None);
        assert_eq!(strip_indent_up_to_3("   \n"), None);
        assert_eq!(strip_indent_up_to_3("   \r\n"), None);
        assert_eq!(strip_indent_up_to_3(""), None);
        assert_eq!(strip_indent_up_to_3("   "), None);
    }
}
