use std::path::Path;
use std::process::Command;

fn lk_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lk"))
}

/// `lk` run in `dir`, with `HOME` pointed at it too.
///
/// Reads default to both scopes merged, so a command run in a fixture project but
/// with the developer's real `HOME` answers from their user-scope DB as well, and an
/// assertion about how many entries the fixture holds stops being about the fixture.
/// CI never sees it — its `HOME` has no store — so this only ever failed locally.
fn lk_in(dir: &Path) -> Command {
    let mut cmd = lk_bin();
    cmd.current_dir(dir).env("HOME", dir);
    cmd
}

fn setup_temp_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    // Create a .git directory so lk recognizes it as a project root
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

#[test]
fn test_version() {
    let output = lk_bin().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("lk "));
}

#[test]
fn test_help() {
    let output = lk_bin().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Local knowledge base CLI"));
}

#[test]
fn test_init() {
    let dir = setup_temp_project();
    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // Verify DB was created
    assert!(dir.path().join(".knowledge/knowledge.db").exists());
    // Verify .knowledge/ was created
    assert!(dir.path().join(".knowledge").is_dir());
    assert!(dir.path().join(".knowledge/README.md").exists());
    // Verify .gitignore was created
    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".knowledge/knowledge.db"));
    // Verify CLAUDE.md was created with import line, and AGENTS.md was not created
    assert!(!dir.path().join("AGENTS.md").exists());
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
    // Verify .knowledge/lk-instructions.md was created with full instructions
    let instructions =
        std::fs::read_to_string(dir.path().join(".knowledge/lk-instructions.md")).unwrap();
    assert!(instructions.contains("Knowledge Base (local-knowledge-cli)"));
    // Verify .knowledge/.lk-version was created
    let version = std::fs::read_to_string(dir.path().join(".knowledge/.lk-version")).unwrap();
    assert!(!version.trim().is_empty());
}

#[test]
fn test_init_idempotent() {
    let dir = setup_temp_project();

    // Run init twice
    lk_in(dir.path()).arg("init").output().unwrap();
    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // CLAUDE.md should not have duplicate import lines, and AGENTS.md was not created
    assert!(!dir.path().join("AGENTS.md").exists());
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    let count = claude_md.matches("@.knowledge/lk-instructions.md").count();
    assert_eq!(count, 1, "CLAUDE.md should not have duplicate import lines");
}

#[test]
fn test_init_migrates_agents_md_to_claude_md() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    std::fs::write(&agents_md_path, "@.knowledge/lk-instructions.md\n").unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // AGENTS.md had only lk import, so it should be removed
    assert!(!agents_md_path.exists());
    // CLAUDE.md should now have the import
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_removes_import_from_agents_md_with_other_content() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    std::fs::write(
        &agents_md_path,
        "# Project Instructions\n\nFollow style guide.\n\n@.knowledge/lk-instructions.md\n",
    )
    .unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // AGENTS.md should still exist with user content, but without lk import
    assert!(agents_md_path.exists());
    let agents_md = std::fs::read_to_string(&agents_md_path).unwrap();
    assert!(agents_md.contains("# Project Instructions"));
    assert!(agents_md.contains("Follow style guide."));
    assert!(!agents_md.contains("@.knowledge/lk-instructions.md"));

    // CLAUDE.md should have the import
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_uses_existing_dot_claude_claude_md_when_root_absent() {
    let dir = setup_temp_project();
    let dot_claude = dir.path().join(".claude");
    std::fs::create_dir_all(&dot_claude).unwrap();
    let claude_md_path = dot_claude.join("CLAUDE.md");
    std::fs::write(&claude_md_path, "# Dot Claude\n").unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    assert!(!dir.path().join("AGENTS.md").exists());
    assert!(!dir.path().join("CLAUDE.md").exists());
    let claude_md = std::fs::read_to_string(&claude_md_path).unwrap();
    assert!(claude_md.starts_with("# Dot Claude\n"));
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_migrates_agents_md_when_claude_md_already_has_content() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    std::fs::write(&agents_md_path, "@.knowledge/lk-instructions.md\n").unwrap();
    let claude_md_path = dir.path().join("CLAUDE.md");
    std::fs::write(&claude_md_path, "# Existing Claude Rules\n").unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // AGENTS.md had only lk import, so it should be removed
    assert!(!agents_md_path.exists());
    // CLAUDE.md should keep existing content and have lk import appended
    let claude_md = std::fs::read_to_string(&claude_md_path).unwrap();
    assert!(claude_md.starts_with("# Existing Claude Rules\n"));
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_does_not_modify_agents_md_with_prose_mentions() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    let original_content = "\
# Agent Guidelines

Do not import @.knowledge/lk-instructions.md in this file.
See ## Knowledge Base (local-knowledge-cli) documentation elsewhere.


Extra blank lines that should remain untouched.
";
    std::fs::write(&agents_md_path, original_content).unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // AGENTS.md should be completely untouched
    assert!(agents_md_path.exists());
    let agents_md = std::fs::read_to_string(&agents_md_path).unwrap();
    assert_eq!(agents_md, original_content);

    // CLAUDE.md was created with import
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_migrates_agents_md_with_legacy_heading() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    let legacy_content = "\
# Project Rules

## Knowledge Base (local-knowledge-cli)
Old inline lk instructions.

### Subheading
Details here.

## Coding Style
Follow Rust 2021 conventions.
";
    std::fs::write(&agents_md_path, legacy_content).unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // Legacy lk section should be removed, but user sections kept
    assert!(agents_md_path.exists());
    let agents_md = std::fs::read_to_string(&agents_md_path).unwrap();
    assert!(agents_md.contains("# Project Rules"));
    assert!(agents_md.contains("## Coding Style"));
    assert!(agents_md.contains("Follow Rust 2021 conventions."));
    assert!(!agents_md.contains("Knowledge Base (local-knowledge-cli)"));
    assert!(!agents_md.contains("Old inline lk instructions."));

    // CLAUDE.md should have the import
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_migrates_agents_md_with_h1_following_legacy_section() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    let legacy_content = "\
## Knowledge Base (local-knowledge-cli)
Old inline lk instructions.

### Subheading
Details here.

# Next Chapter
Keep this H1 section.
";
    std::fs::write(&agents_md_path, legacy_content).unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // Legacy lk section should be removed, but H1 section kept
    assert!(agents_md_path.exists());
    let agents_md = std::fs::read_to_string(&agents_md_path).unwrap();
    assert!(agents_md.contains("# Next Chapter"));
    assert!(agents_md.contains("Keep this H1 section."));
    assert!(!agents_md.contains("Knowledge Base (local-knowledge-cli)"));
    assert!(!agents_md.contains("Old inline lk instructions."));

    // CLAUDE.md should have the import
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_init_migrates_claude_md_with_h1_following_legacy_section() {
    let dir = setup_temp_project();
    let claude_md_path = dir.path().join("CLAUDE.md");
    let legacy_content = "\
## Knowledge Base (local-knowledge-cli)
Old inline lk instructions.

### Subheading
Details here.

# Next Chapter
Keep this H1 section.
";
    std::fs::write(&claude_md_path, legacy_content).unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // Legacy lk section should be replaced with import, and H1 section kept
    let claude_md = std::fs::read_to_string(&claude_md_path).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
    assert!(claude_md.contains("# Next Chapter"));
    assert!(claude_md.contains("Keep this H1 section."));
    assert!(!claude_md.contains("Knowledge Base (local-knowledge-cli)"));
    assert!(!claude_md.contains("Old inline lk instructions."));
}

#[test]
fn test_init_does_not_modify_agents_md_with_marker_inside_code_block() {
    let dir = setup_temp_project();
    let agents_md_path = dir.path().join("AGENTS.md");
    let original_content = "\
# My Project

Here is an example:
```markdown
## Knowledge Base (local-knowledge-cli)
@.knowledge/lk-instructions.md
```

## Real Heading
Keep this heading.
";
    std::fs::write(&agents_md_path, original_content).unwrap();

    let output = lk_in(dir.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    // AGENTS.md should be completely untouched
    assert!(agents_md_path.exists());
    let agents_md = std::fs::read_to_string(&agents_md_path).unwrap();
    assert_eq!(agents_md, original_content);

    // CLAUDE.md was created with import
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.knowledge/lk-instructions.md"));
}

#[test]
fn test_add_and_get() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Add an entry
    let output = lk_bin()
        .args([
            "add",
            "Test Entry",
            "--keywords",
            "test,rust",
            "--content",
            "This is test content.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let add_result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = add_result["id"].as_i64().unwrap();
    assert!(id > 0);

    // Get the entry
    let output = lk_bin()
        .args(["get", &id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entry["title"], "Test Entry");
    assert_eq!(entry["content"], "This is test content.");
    assert!(
        entry["keywords"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "test")
    );
    assert!(
        entry["keywords"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "rust")
    );
}

#[test]
fn test_search() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    lk_bin()
        .args([
            "add",
            "OAuth Login",
            "--keywords",
            "oauth,login",
            "--content",
            "OAuth 2.0 with PKCE flow.",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Search by keyword
    let output = lk_bin()
        .args(["search", "oauth", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0]["title"], "OAuth Login");
}

#[test]
fn test_add_with_status() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Add an entry with an explicit initial status
    let output = lk_bin()
        .args([
            "add",
            "Deferred plan",
            "--category",
            "plan",
            "--status",
            "proposed",
            "--content",
            "Do this later.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = result["id"].as_i64().unwrap();

    // The stored entry carries the requested status
    let output = lk_bin()
        .args(["get", &id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entry["status"], "proposed");
}

#[test]
fn test_add_rejects_invalid_status() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = lk_bin()
        .args(["add", "Bad status", "--status", "bogus", "--content", "x"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid status"));
}

#[test]
fn test_search_status_filter() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Two entries sharing a keyword but with different statuses
    lk_bin()
        .args([
            "add",
            "Plan to migrate auth",
            "--category",
            "plan",
            "--status",
            "proposed",
            "--keywords",
            "auth,migrate",
            "--content",
            "Open plan item.",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    lk_bin()
        .args([
            "add",
            "Auth migration finished",
            "--category",
            "plan",
            "--status",
            "accepted",
            "--keywords",
            "auth,migrate",
            "--content",
            "Closed plan item.",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Filtering by status returns only the open plan
    let output = lk_bin()
        .args(["search", "auth", "--status", "proposed", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Plan to migrate auth");
    assert_eq!(results[0]["status"], "proposed");
}

#[test]
fn test_add_with_status_reports_status_in_json() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // The add --json response echoes the stored status (no follow-up get needed)
    let output = lk_bin()
        .args([
            "add",
            "Has status",
            "--status",
            "proposed",
            "--content",
            "x",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "proposed");

    // Default status (no --status) reports "active". An unrelated title needs no
    // --force: nothing short of a same or all-but-identical title is refused.
    let output = lk_bin()
        .args([
            "add",
            "Completely different topic",
            "--content",
            "unrelated body",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "active");
}

#[test]
fn test_search_status_combined_with_category() {
    let dir = setup_temp_project();
    lk_in(dir.path()).arg("init").output().unwrap();

    // Same status, different categories — the status+category filters must AND together
    for (title, category) in [("Plan item", "plan"), ("Decision item", "decisions")] {
        lk_in(dir.path())
            .args([
                "add",
                title,
                "--category",
                category,
                "--status",
                "proposed",
                "--keywords",
                "shared,kw",
                "--content",
                "shared kw body",
            ])
            .output()
            .unwrap();
    }

    let output = lk_in(dir.path())
        .args([
            "search",
            "shared",
            "--category",
            "plan",
            "--status",
            "proposed",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Plan item");
}

#[test]
fn test_search_rejects_invalid_status() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // A typo'd status must error loudly rather than silently returning 0 results
    let output = lk_bin()
        .args(["search", "anything", "--status", "bogus"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid status"));
}

#[test]
fn test_list_rejects_invalid_status() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // `lk list --status bogus` must error, not silently return an empty list
    let output = lk_bin()
        .args(["list", "--status", "bogus"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid status"));
}

#[test]
fn test_edit_invalid_status_validated_before_target() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Status is validated before the target is resolved: even for a nonexistent id,
    // a bad status reports "Invalid status" (not a "not found" error).
    let output = lk_bin()
        .args(["edit", "99999", "--status", "bogus"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid status"),
        "expected Invalid status, got: {stderr}"
    );
}

#[test]
fn test_search_status_filter_user_scope_merged() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    // One proposed plan in the project scope, one accepted plan in the user scope
    lk_bin()
        .args([
            "add",
            "Project open plan",
            "--category",
            "plan",
            "--status",
            "proposed",
            "--keywords",
            "merge,plan",
            "--content",
            "merge plan body",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    lk_bin()
        .args([
            "add",
            "User done plan",
            "--scope",
            "user",
            "--category",
            "plan",
            "--status",
            "accepted",
            "--keywords",
            "merge,plan",
            "--content",
            "merge plan body",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    // Default merged read (scope=all) filtered by status returns only the proposed one
    let output = lk_bin()
        .args(["search", "merge", "--status", "proposed", "--json"])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Project open plan");
    assert_eq!(results[0]["scope"], "project");
}

#[test]
fn test_delete() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = lk_bin()
        .args([
            "add",
            "To Delete",
            "--content",
            "Will be deleted.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = result["id"].as_i64().unwrap();

    // Delete (with -y to skip confirmation)
    let output = lk_bin()
        .args(["delete", &id.to_string(), "-y"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify it's gone
    let output = lk_bin()
        .args(["get", &id.to_string()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_import_and_sync() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create a knowledge file
    let arch_dir = dir.path().join(".knowledge/architecture");
    std::fs::create_dir_all(&arch_dir).unwrap();
    std::fs::write(
        arch_dir.join("test.md"),
        "---\nkeywords: [auth, login]\ncategory: architecture\n---\n\n\
         # Auth Flow\n\n\
         ## Entry: Token Management\n\
         keywords: [token, jwt]\n\n\
         JWT tokens expire after 15 minutes.\n",
    )
    .unwrap();

    // Sync
    let output = lk_bin()
        .args(["sync", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stats: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(stats["added"].as_i64().unwrap() > 0);

    // Verify entry is searchable
    let output = lk_bin()
        .args(["search", "token", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!results.is_empty());
}

#[test]
fn test_export() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Add local entries
    lk_bin()
        .args([
            "add",
            "Local Fact",
            "--keywords",
            "local,fact",
            "--content",
            "A locally discovered fact.",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Export
    let output = lk_bin()
        .args(["export"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // Check exported files exist
    let knowledge_dir = dir.path().join(".knowledge");
    let exported_files: Vec<_> = std::fs::read_dir(&knowledge_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("exported-"))
        .collect();
    assert!(!exported_files.is_empty());
}

/// Adding two unrelated entries back to back must just work. Duplicate detection
/// used to fire on shared auto-extracted keywords, so this was the common case
/// that got refused.
#[test]
fn test_add_does_not_flag_unrelated_entries() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    for (title, content) in [
        (
            "Homebrew formula release flow",
            "Bump the tap and update the bottle.",
        ),
        (
            "Screenshot test baseline update",
            "Regenerate baselines and review the diff.",
        ),
    ] {
        let output = lk_bin()
            .args(["add", title, "--content", content, "--json"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["added"], true, "{title} should be added: {result}");
        assert!(
            result.get("possibly_related").is_none(),
            "{title} is unrelated to anything already stored: {result}"
        );
    }
}

/// A same-title collision refuses the add and says so under `similar_entries`
/// with `added: false`. Covers the exact-match-after-normalization case only;
/// the near-identical band is pinned by
/// `similarity::tests::title_block_band_holds_only_all_but_identical_titles`.
#[test]
fn test_add_blocks_same_title() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    lk_bin()
        .args(["add", "Export group naming", "--content", "body", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Same title, differing only in case and spacing.
    let output = lk_bin()
        .args([
            "add",
            "export  group naming",
            "--content",
            "other",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["added"], false, "got {result}");
    assert_eq!(result["reason"], "duplicate");
    assert_eq!(result["similar_entries"][0]["match_reason"], "same-title");

    // --force still overrides it.
    let forced = lk_bin()
        .args([
            "add",
            "export  group naming",
            "--content",
            "other",
            "--force",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let forced: serde_json::Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(forced["added"], true, "got {forced}");
}

/// A refusal must name only the entries that caused it.
///
/// `find_similar_entries` returns blocking and non-blocking hits together, so a
/// keyword-only match can ride along with the title collision. Reporting it under
/// `similar_entries` — next to "edit that entry instead" — points the caller at
/// an entry that has nothing to do with the subject, which is how an unrelated
/// entry gets overwritten.
#[test]
fn test_add_block_reports_only_the_blocking_entries() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Filler so document frequencies are realistic; below ~4 entries the df cap
    // zeroes every keyword and no keyword-only hit can occur at all.
    for i in 0..6 {
        lk_bin()
            .args([
                "add",
                &format!("Filler topic {i}"),
                "--keywords",
                &format!("fill{i},pad{i}"),
                "--content",
                "x",
            ])
            .current_dir(dir.path())
            .output()
            .unwrap();
    }
    // The entry that will block, on title alone.
    lk_bin()
        .args([
            "add",
            "Alpha beta gamma delta",
            "--keywords",
            "unrelatedone,unrelatedtwo",
            "--content",
            "the blocking one",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // An unrelated entry that will match on keywords only.
    lk_bin()
        .args([
            "add",
            "Completely different subject here",
            "--keywords",
            "rareone,raretwo,rarethree",
            "--content",
            "shares only keywords",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = lk_bin()
        .args([
            "add",
            "Alpha beta gamma delta",
            "--keywords",
            "rareone,raretwo,rarethree",
            "--content",
            "dupe",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(result["added"], false, "got {result}");
    let entries = result["similar_entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "only the title collision blocked: {result}"
    );
    assert_eq!(entries[0]["title"], "Alpha beta gamma delta");
    assert_eq!(entries[0]["match_reason"], "same-title");
    assert!(
        !entries
            .iter()
            .any(|e| e["match_reason"] == "similar-keywords"),
        "a keyword-only hit did not cause the refusal and must not be listed: {result}"
    );
}

/// A follow-up on the same topic is reported but still added — the behavior change
/// that keeps a weak match from being mistaken for a rejection.
#[test]
fn test_add_reports_possibly_related_but_still_adds() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    lk_bin()
        .args([
            "add",
            "Export group name bug",
            "--keywords",
            "export,grouping,keywords",
            "--content",
            "The first user keyword is not used as the group name.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = lk_bin()
        .args([
            "add",
            "Export group name bug follow-up",
            "--keywords",
            "export,grouping,keywords",
            "--content",
            "Remaining work after the first fix.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(result["added"], true, "a follow-up must be added: {result}");
    assert!(
        result.get("similar_entries").is_none(),
        "`similar_entries` means 'not added' and must not appear here: {result}"
    );
    let related = result["possibly_related"]
        .as_array()
        .unwrap_or_else(|| panic!("expected possibly_related in {result}"));
    assert_eq!(related.len(), 1);
    assert_eq!(related[0]["title"], "Export group name bug");
    assert!(
        result["possibly_related_note"]
            .as_str()
            .unwrap_or_default()
            .contains("WAS added"),
        "the note must state the add succeeded: {result}"
    );
}

#[test]
fn test_export_keyword_cannot_write_outside_the_store() {
    // A keyword becomes the exported file's name, so `x/../../README` used to resolve
    // out of `.knowledge/` and overwrite whatever it landed on.
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    std::fs::write(dir.path().join("README.md"), "the project's own readme").unwrap();
    // The traversal needs this directory to exist for `..` to resolve through it.
    std::fs::create_dir_all(dir.path().join(".knowledge/exported-x")).unwrap();

    assert!(
        run(&[
            "add",
            "innocent looking entry",
            "-k",
            "x/../../README",
            "-c",
            "overwritten?",
        ])
        .status
        .success()
    );
    let out = run(&["export"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "the project's own readme",
        "export must not write outside .knowledge/"
    );
    // It still exported — into the store, under a flattened name.
    let written: Vec<String> = std::fs::read_dir(dir.path().join(".knowledge"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("exported-"))
        .collect();
    assert_eq!(written.len(), 1, "files: {written:?}");
    // Flattened, and inside the store — the `..` survives only as literal text.
    assert!(!written[0].contains('/'), "{written:?}");
}

// Gated on the whole function: on Windows there would be no link to refuse, and the
// assertions below would fail rather than skip.
#[cfg(unix)]
#[test]
fn test_export_refuses_to_write_through_a_symlink() {
    // `.knowledge/` travels with the repository, so a link committed as
    // `exported-auth.md -> ../README.md` would let an export truncate a file outside
    // the store — the lexical one-segment check cannot see that.
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    std::fs::write(dir.path().join("README.md"), "the project's own readme").unwrap();
    assert!(
        run(&["add", "auth entry", "-k", "auth", "-c", "how login works"])
            .status
            .success()
    );

    std::os::unix::fs::symlink(
        "../README.md",
        dir.path().join(".knowledge/exported-auth.md"),
    )
    .unwrap();

    let out = run(&["export"]);
    assert!(!out.status.success(), "export must refuse the symlink");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("symlink"),
        "and say why: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "the project's own readme",
        "the link target must be untouched"
    );
}

#[cfg(unix)]
#[test]
fn test_export_does_not_truncate_a_hard_linked_target() {
    // A hard link is not a symlink, so no check catches it: a plain write would
    // truncate the inode `README.md` also points at. Writing a fresh file and renaming
    // it over the entry replaces the directory entry instead.
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    std::fs::write(dir.path().join("README.md"), "the project's own readme").unwrap();
    assert!(
        run(&["add", "auth entry", "-k", "auth", "-c", "how login works"])
            .status
            .success()
    );
    std::fs::hard_link(
        dir.path().join("README.md"),
        dir.path().join(".knowledge/exported-auth.md"),
    )
    .unwrap();

    let out = run(&["export"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "the project's own readme",
        "the hard link's other name must be untouched"
    );
    let exported = std::fs::read_to_string(dir.path().join(".knowledge/exported-auth.md")).unwrap();
    assert!(exported.contains("how login works"), "{exported}");
}

#[cfg(unix)]
#[test]
fn test_export_keeps_the_files_permissions() {
    // The atomic replace writes a temp file (created 0600) and renames it, and a
    // rename keeps the source's mode — so without care, replacing a committed
    // `.knowledge/*.md` would quietly make it owner-only.
    use std::os::unix::fs::PermissionsExt;
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    assert!(
        run(&["add", "perm entry", "-k", "perm", "-c", "one"])
            .status
            .success()
    );
    assert!(run(&["export"]).status.success());

    let path = dir.path().join(".knowledge/exported-perm.md");
    // Compared against a sibling written the ordinary way rather than a fixed 0644: the
    // export is created `0666 & !umask` like any plain write, and the child `lk`
    // inherits this process's umask, so this holds however the suite is run.
    let probe = dir.path().join(".knowledge/.umask-probe");
    std::fs::write(&probe, "").unwrap();
    let expected = std::fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
    std::fs::remove_file(&probe).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, expected,
        "a fresh project-scope export should get the mode a plain write would"
    );

    // An existing file keeps whatever mode it had.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(
        run(&["add", "perm entry two", "-k", "perm", "-c", "two"])
            .status
            .success()
    );
    assert!(run(&["export"]).status.success());
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640, "replacing a file must not change its mode");
}

/// `feature/auth` is the keyword this whole change exists for — it used to make
/// `export` fail outright. Exporting it is only half the job: the markdown has to read
/// back as the same single keyword, or the next export names the file after a fragment
/// and abandons the one it wrote before.
#[test]
fn test_a_slashed_keyword_survives_an_export_sync_round_trip() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    assert!(
        run(&["add", "auth flow", "-k", "feature/auth", "-c", "body"])
            .status
            .success()
    );
    assert!(run(&["export"]).status.success());

    let exported = || -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.path().join(".knowledge"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("exported-"))
            .collect();
        names.sort();
        names
    };
    let first = exported();
    assert_eq!(first.len(), 1, "one exported file: {first:?}");

    // Edit the body so `sync` re-imports the file instead of matching on its hash.
    let md = dir.path().join(".knowledge").join(&first[0]);
    let text = std::fs::read_to_string(&md).unwrap();
    std::fs::write(&md, text.replace("body", "body edited")).unwrap();
    assert!(run(&["sync"]).status.success());

    // One keyword — not the three (`feature`, `auth`, `feature/auth`) that a frontmatter
    // parser splitting on word characters used to merge with the per-entry line.
    let kws: serde_json::Value =
        serde_json::from_slice(&run(&["keywords", "--json"]).stdout).unwrap();
    let names: Vec<&str> = kws
        .as_array()
        .unwrap()
        .iter()
        .map(|k| {
            k.get("keyword")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(names, vec!["feature/auth"], "keywords after sync: {kws:?}");

    // ...so a second export lands on the same file rather than orphaning the first.
    assert!(run(&["export"]).status.success());
    assert_eq!(exported(), first, "re-export must not rename the file");
}

/// Every entry in an exported file flips to `shared` together. If only some do, the next
/// export rebuilds the file from the ones still `local`, and `sync` then deletes the
/// `shared` rows pointing at it — losing the entries that did flip.
#[test]
fn test_the_flip_to_shared_is_all_or_nothing() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    // Same first keyword, so both land in one file.
    assert!(
        run(&["add", "first", "-k", "grp", "-c", "one"])
            .status
            .success()
    );
    assert!(
        run(&["add", "second", "-k", "grp", "-c", "two"])
            .status
            .success()
    );

    // Let the second entry's flip fail, after the first one's has already been applied.
    let db = dir.path().join(".knowledge/knowledge.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER refuse_second BEFORE UPDATE ON entries WHEN new.id = 2
             BEGIN SELECT RAISE(ABORT, 'no'); END;",
        )
        .unwrap();
    }

    let out = run(&["export"]);
    assert!(!out.status.success(), "export must report the failure");

    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TRIGGER refuse_second").unwrap();
        let sources: Vec<String> = conn
            .prepare("SELECT source FROM entries ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            sources,
            vec!["local", "local"],
            "a half-applied flip must roll back"
        );
    }
}

/// A refusal must happen before the first file is written. Exporting group by group and
/// failing part-way leaves the earlier groups written and flipped to `shared` while the
/// command reports failure — a state neither outcome asked for.
#[cfg(unix)]
#[test]
fn test_a_symlink_in_a_later_group_stops_the_whole_export() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    // Groups are visited in alphabetical order, so `aaa` would be written first.
    assert!(
        run(&["add", "first", "-k", "aaa", "-c", "one"])
            .status
            .success()
    );
    assert!(
        run(&["add", "second", "-k", "zzz", "-c", "two"])
            .status
            .success()
    );

    let victim = dir.path().join("victim.md");
    std::fs::write(&victim, "do not touch\n").unwrap();
    std::os::unix::fs::symlink(&victim, dir.path().join(".knowledge/exported-zzz.md")).unwrap();

    let out = run(&["export"]);
    assert!(!out.status.success(), "export must refuse");
    assert!(
        !dir.path().join(".knowledge/exported-aaa.md").exists(),
        "the earlier group must not have been written"
    );
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "do not touch\n",
        "the symlink target must be untouched"
    );
}

/// The file name comes from an entry's first keyword, and a re-imported entry's
/// keywords are seeded from the file-level list — so that list's head decides the name
/// on the next export. Sorted alphabetically, an entry keyworded `zebra, apple` came
/// back as `apple, zebra` and its file was renamed out from under `sync`.
#[test]
fn test_an_edited_export_keeps_its_file_name_after_a_sync() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    // `add` stores keywords sorted; `edit` keeps the order it is given, which is how a
    // store ends up with a first keyword that is not the alphabetically first one.
    // `apple` sorts first, so a sorted file-level list would put it at the head.
    assert!(
        run(&["add", "striped", "-k", "placeholder", "-c", "body"])
            .status
            .success()
    );
    assert!(run(&["edit", "1", "-k", "zebra,apple"]).status.success());
    assert!(run(&["export"]).status.success());

    let md = dir.path().join(".knowledge/exported-zebra.md");
    assert!(md.exists(), "named after the first keyword");
    let written = std::fs::read_to_string(&md).unwrap();
    assert!(
        written.contains("keywords: [zebra, apple]"),
        "the group keyword heads the file-level list: {written}"
    );

    // Edit the body so `sync` re-imports instead of matching on the hash.
    let text = std::fs::read_to_string(&md).unwrap();
    std::fs::write(&md, text.replace("body", "body edited")).unwrap();
    assert!(run(&["sync"]).status.success());

    // The entry is `shared` now, so there is nothing left to export; what decides the
    // next export's file name is the order the re-import stored. Read it straight from
    // the table, in insertion order — that is the order `get_keywords` returns and the
    // first of them is the group.
    let conn = rusqlite::Connection::open(dir.path().join(".knowledge/knowledge.db")).unwrap();
    let kws: Vec<String> = conn
        .prepare("SELECT keyword FROM keywords ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        kws.first().map(String::as_str),
        Some("zebra"),
        "the group keyword must stay first: {kws:?}"
    );
    assert!(
        kws.iter().any(|k| k == "apple"),
        "and the rest must survive: {kws:?}"
    );
}

/// A destination that is not a plain file has to be refused in the preflight, not by
/// the `rename` at the end: by then the earlier groups are written and flipped to
/// `shared`, and the command reports a failure it already half-applied.
#[test]
fn test_a_directory_in_a_later_group_stops_the_whole_export() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    // Groups are visited in alphabetical order, so `aaa` would be written first.
    assert!(
        run(&["add", "first", "-k", "aaa", "-c", "one"])
            .status
            .success()
    );
    assert!(
        run(&["add", "second", "-k", "zzz", "-c", "two"])
            .status
            .success()
    );
    std::fs::create_dir(dir.path().join(".knowledge/exported-zzz.md")).unwrap();

    let out = run(&["export"]);
    assert!(!out.status.success(), "export must refuse");
    assert!(
        !dir.path().join(".knowledge/exported-aaa.md").exists(),
        "the earlier group must not have been written"
    );
}

/// The store's existing files collide too. A pre-v7 export left `exported-AUTH.md`
/// owning its entries and migration 7 lowercased the keyword without renaming the file,
/// so an `auth` export plans the same directory entry on a file system that ignores
/// case — and replacing it makes the next `sync` delete everything that file owned.
#[test]
fn test_export_refuses_a_name_a_shared_file_already_owns() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());

    let legacy = dir.path().join(".knowledge/exported-AUTH.md");
    std::fs::write(
        &legacy,
        "---\nkeywords: [auth]\ncategory: exported\n---\n\n# Exported: AUTH\n\n## Entry: old one\nkeywords: [auth]\n\nkept\n",
    )
    .unwrap();
    assert!(run(&["sync"]).status.success());

    assert!(
        run(&["add", "new one", "-k", "auth", "-c", "fresh"])
            .status
            .success()
    );
    let out = run(&["export"]);
    assert!(
        !out.status.success(),
        "export must refuse: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("exported-AUTH.md"),
        "the message must name the file in the way: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        std::fs::read_to_string(&legacy).unwrap().contains("kept"),
        "the existing file must be untouched"
    );
}

#[test]
fn test_keyword_normalization_survives_every_path() {
    // Keywords are stored NFC and lowercased on every path, and the needles are
    // normalized the same way — so a decomposed query finds a composed keyword and
    // vice versa. Reverting the normalization in any one path fails this.
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());

    let composed = "が";
    let decomposed = "か\u{3099}";
    let stored = |id: &str| -> Vec<String> {
        let get: serde_json::Value =
            serde_json::from_slice(&run(&["get", id, "--json"]).stdout).unwrap();
        get["keywords"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect()
    };

    // `add` on its own, asserted before anything replaces the keywords — so reverting
    // the normalization in `add_entry_full` alone fails here.
    assert!(
        run(&["add", "nfc entry", "-k", decomposed, "-c", "body"])
            .status
            .success()
    );
    assert_eq!(stored("1"), vec![composed], "add stored: {:?}", stored("1"));

    // `edit` on a second entry, so the two paths are pinned independently.
    assert!(
        run(&[
            "add",
            "nfc entry two",
            "-k",
            "placeholder",
            "-c",
            "body two"
        ])
        .status
        .success()
    );
    assert!(
        run(&["edit", "2", "-k", &format!("{decomposed},GA,{composed}")])
            .status
            .success()
    );
    assert_eq!(
        stored("2"),
        vec![composed.to_string(), "ga".to_string()],
        "edit stored: {:?}",
        stored("2")
    );

    // Both spellings of the needle find it, through the keyword-only path.
    for needle in [composed, decomposed] {
        let out = run(&[
            "search",
            needle,
            "--keyword-only",
            "--scope",
            "project",
            "--json",
        ]);
        let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(hits.len(), 2, "needle {needle:?} found: {hits:?}");
    }
}

#[test]
fn test_export_round_trips_a_keyword_with_a_slash() {
    // `feature/auth` is an ordinary keyword to write, and it used to fail the whole
    // export with "No such file or directory" — the flattened name has to survive a
    // `sync` back as well, since that is how the md store stays the source of truth.
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    assert!(
        run(&[
            "add",
            "auth flow",
            "-k",
            "feature/auth",
            "-c",
            "how login works"
        ])
        .status
        .success()
    );

    let out = run(&["export"]);
    assert!(
        out.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sync = run(&["sync", "--json"]);
    assert!(
        sync.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&sync.stderr)
    );

    let search = run(&["search", "login", "--scope", "project", "--json"]);
    let hits: Vec<serde_json::Value> = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(
        hits.len(),
        1,
        "the entry must survive the round trip: {hits:?}"
    );
    assert_eq!(hits[0]["source"], "shared", "{hits:?}");
}

/// Pin `updated_at` on entries picked out by title.
///
/// Entries added moments apart share a timestamp (`updated_at` has second
/// granularity), which leaves `ORDER BY updated_at DESC` to fall back to scan order —
/// enough to make a ranking assertion pass for the wrong reason. Pinning beats
/// sleeping past the boundary: it is exact, and it costs no wall clock.
fn set_updated_at(db: &std::path::Path, rows: &[(&str, &str)]) {
    let conn = rusqlite::Connection::open(db).unwrap();
    for (title, when) in rows {
        let changed = conn
            .execute(
                "UPDATE entries SET updated_at = ?1 WHERE title = ?2",
                rusqlite::params![when, title],
            )
            .unwrap();
        assert_eq!(changed, 1, "expected exactly one entry titled {title:?}");
    }
}

/// Drive the MCP server over stdio with one JSON-RPC line and return the replies.
fn mcp_request(project: &std::path::Path, request: &str) -> Vec<serde_json::Value> {
    mcp_request_env(project, None, request)
}

/// [`mcp_request`] with `HOME` pointed at a scratch dir, so a test that touches the
/// user-scope store gets its own instead of the developer's.
fn mcp_request_with_home(
    project: &std::path::Path,
    home: &std::path::Path,
    request: &str,
) -> Vec<serde_json::Value> {
    mcp_request_env(project, Some(home), request)
}

fn mcp_request_env(
    project: &std::path::Path,
    home: Option<&std::path::Path>,
    request: &str,
) -> Vec<serde_json::Value> {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = lk_bin();
    if let Some(home) = home {
        cmd.env("HOME", home);
    }
    let mut child = cmd
        .args(["mcp", "--project", project.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let out = child.wait_with_output().unwrap();

    // Fail on the offending line rather than skipping it. Anything the server
    // prints to stdout is part of the JSON-RPC stream, so a line that does not
    // parse is the bug — dropping it would surface later as a confusing symptom
    // (an empty tool list) far from the cause.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .unwrap_or_else(|e| panic!("MCP wrote a non-JSON line to stdout ({e}): {l:?}"))
        })
        .collect()
}

#[test]
fn test_mcp_initialize_returns_instructions() {
    let dir = setup_temp_project();
    let replies = mcp_request(
        dir.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}"#,
    );
    assert_eq!(replies.len(), 1);
    let result = &replies[0]["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "lk-knowledge");
    let instructions = result["instructions"]
        .as_str()
        .expect("initialize response must contain instructions string");
    assert!(instructions.contains("Knowledge Base (local-knowledge-cli)"));
    assert!(instructions.contains("Search BEFORE investigating"));
}

/// Every MCP tool is named after the CLI subcommand it mirrors, so an agent that
/// learns one surface can transliterate to the other. `update_knowledge` was the
/// one exception, and it transliterated to `lk update` — the binary self-updater.
#[test]
fn test_mcp_tool_names_mirror_the_cli_subcommands() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let names: Vec<String> = mcp_request(
        dir.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
    )
    .into_iter()
    // Selecting the tools/list reply out of the stream, not discarding failures.
    .filter_map(|v| v["result"]["tools"].as_array().cloned())
    .flatten()
    .map(|t| {
        t["name"]
            .as_str()
            .unwrap_or_else(|| panic!("a tool in tools/list has no name: {t}"))
            .to_string()
    })
    .collect();

    assert!(!names.is_empty(), "tools/list returned nothing: {names:?}");
    assert!(
        names.iter().any(|n| n == "edit_knowledge"),
        "the entry-editing tool must be named after `lk edit`: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "update_knowledge"),
        "`update_knowledge` transliterates to `lk update`, which upgrades the binary: {names:?}"
    );
}

/// An edit that names no field changes nothing, so reporting success would leave
/// the caller believing the entry was updated. The CLI already refuses it; both
/// surfaces have to answer the same way.
#[test]
fn test_edit_with_no_fields_is_refused_on_both_surfaces() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    lk_bin()
        .args(["add", "Guard test", "--content", "original", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // CLI: already guarded.
    let cli = lk_bin()
        .args(["edit", "1", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!cli.status.success(), "the CLI must refuse an empty edit");

    // MCP: must not answer `updated: true` for an edit that did nothing.
    let replies = mcp_request(
        dir.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"edit_knowledge","arguments":{"id":1}}}"#,
    );
    let body = replies
        .iter()
        .find_map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{replies:?}"));
    assert!(
        !body.contains("\"updated\": true"),
        "an edit with no fields must not report success: {body}"
    );
    assert!(
        body.to_lowercase().contains("nothing to edit"),
        "and it must say why: {body}"
    );

    // The entry is untouched either way.
    let get = lk_bin()
        .args(["get", "1", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&get.stdout).unwrap();
    assert_eq!(entry["content"], "original");
}

/// `edit_knowledge` is the MCP tool for editing an entry, so an agent falling
/// back to the CLI reaches for `lk update` — which upgrades the binary. The reply
/// has to name `lk edit` outright; clap's "unexpected argument" would send the
/// caller through two more invocations to find it.
#[test]
fn test_update_with_entry_args_points_at_edit() {
    let output = lk_bin()
        .args(["update", "42", "--content", "new body"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "the mistake must not self-update");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lk edit 42 --content 'new body'"),
        "must suggest the equivalent edit, ready to paste: {stderr}"
    );
    assert!(
        !stderr.contains("unexpected argument"),
        "clap's default reply is what we are replacing: {stderr}"
    );
}

/// The same pointer has to be reachable from `--help`, since a caller who used
/// no positional lands there.
#[test]
fn test_update_help_disclaims_editing() {
    let output = lk_bin().args(["update", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("lk edit"), "got {stdout}");
    assert!(
        stdout.contains("--skip-verify"),
        "the real option must survive the hidden catch-all: {stdout}"
    );
    assert!(
        !stdout.contains("entry_args"),
        "the catch-all is an implementation detail and must stay hidden: {stdout}"
    );
}

#[test]
fn test_export_by_ids() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Add two entries with distinct titles/keywords to avoid auto-extraction collisions
    let out1 = lk_bin()
        .args([
            "add",
            "Authentication Flow",
            "--keywords",
            "oauth,auth",
            "--content",
            "OAuth 2.0 with PKCE flow.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let r1: serde_json::Value = serde_json::from_slice(&out1.stdout).unwrap();
    let id1 = r1["id"].as_i64().unwrap();

    lk_bin()
        .args([
            "add",
            "Database Migration",
            "--keywords",
            "database,migration",
            "--content",
            "SQLite schema versioning.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Export only the first entry by ID
    let output = lk_bin()
        .args(["export", "--ids", &id1.to_string()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 entries"));

    // First entry should now be shared
    let out_shared = lk_bin()
        .args(["list", "--source", "shared", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let shared: Vec<serde_json::Value> = serde_json::from_slice(&out_shared.stdout).unwrap();
    assert!(
        shared.iter().any(|e| e["title"] == "Authentication Flow"),
        "Auth entry should be shared after export"
    );

    // Second entry should still be local
    let out_local = lk_bin()
        .args(["list", "--source", "local", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let local: Vec<serde_json::Value> = serde_json::from_slice(&out_local.stdout).unwrap();
    assert!(
        local.iter().any(|e| e["title"] == "Database Migration"),
        "DB entry should still be local"
    );
}

#[test]
fn test_export_by_query() {
    let dir = setup_temp_project();
    lk_in(dir.path()).arg("init").output().unwrap();

    lk_in(dir.path())
        .args([
            "add",
            "OAuth Flow",
            "--keywords",
            "oauth",
            "--content",
            "OAuth 2.0 with PKCE.",
        ])
        .output()
        .unwrap();
    lk_in(dir.path())
        .args([
            "add",
            "Database Schema",
            "--keywords",
            "database",
            "--content",
            "SQLite with FTS5.",
        ])
        .output()
        .unwrap();

    // Export only OAuth-related entries
    let output = lk_in(dir.path())
        .args(["export", "--query", "OAuth"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 entries"));

    // Database entry should still be local
    let out = lk_in(dir.path())
        .args(["list", "--source", "local", "--json"])
        .output()
        .unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["title"], "Database Schema");
}

#[test]
fn test_stats() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = lk_bin()
        .args(["stats", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stats: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(stats["total_entries"].as_i64().is_some());
    assert!(stats["shared_entries"].as_i64().is_some());
    assert!(stats["local_entries"].as_i64().is_some());
    assert!(stats["unique_keywords"].as_i64().is_some());
}

#[test]
fn test_keywords_auto_extraction() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Add entry without explicit keywords - should auto-extract
    let output = lk_bin()
        .args([
            "add",
            "SessionManager Config",
            "--content",
            "The SessionManager in src/auth/session.ts handles tokens.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let keywords: Vec<String> = result["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    // Should have extracted CamelCase components and file path parts
    assert!(
        keywords.iter().any(|k| k == "session"),
        "Should extract 'session' from CamelCase/path"
    );
    assert!(
        keywords.iter().any(|k| k == "manager"),
        "Should extract 'manager' from CamelCase"
    );
    assert!(
        keywords.iter().any(|k| k == "auth"),
        "Should extract 'auth' from file path"
    );
}

#[test]
fn test_symlink_traversal_blocked() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create a file outside .knowledge/
    let secret_dir = dir.path().join("secrets");
    std::fs::create_dir(&secret_dir).unwrap();
    std::fs::write(
        secret_dir.join("secret.md"),
        "---\nkeywords: [secret]\n---\n\n# Secret\n\nThis should not be imported.\n",
    )
    .unwrap();

    // Create a symlink inside .knowledge/ pointing outside
    let knowledge_dir = dir.path().join(".knowledge");
    std::os::unix::fs::symlink(&secret_dir, knowledge_dir.join("evil-link")).unwrap();

    // Sync should skip the symlink
    let output = lk_bin()
        .args(["sync", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // The secret should NOT be in the database
    let output = lk_bin()
        .args(["search", "secret", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(results.is_empty(), "Symlink traversal should be blocked");
}

#[test]
fn test_supersede() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Add two entries
    let out1 = lk_bin()
        .args(["add", "Old Decision", "--content", "Use REST", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out1.status.success());
    let old_id = serde_json::from_slice::<serde_json::Value>(&out1.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    let out2 = lk_bin()
        .args(["add", "New Decision", "--content", "Use gRPC", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out2.status.success());
    let new_id = serde_json::from_slice::<serde_json::Value>(&out2.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Supersede old with new
    let output = lk_bin()
        .args([
            "supersede",
            &old_id.to_string(),
            &new_id.to_string(),
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["old_status"], "superseded");

    // Verify old entry is superseded
    let output = lk_bin()
        .args(["get", &old_id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let old_entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(old_entry["status"], "superseded");
    assert!(old_entry["superseded_by"].is_string());

    // Verify new entry has supersedes link
    let output = lk_bin()
        .args(["get", &new_id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let new_entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(new_entry["supersedes"].is_array());
    assert_eq!(new_entry["supersedes"].as_array().unwrap().len(), 1);

    // Verify UIDs match bidirectionally
    let old_uid = old_entry["uid"].as_str().unwrap();
    let new_uid = new_entry["uid"].as_str().unwrap();
    assert_eq!(old_entry["superseded_by"], new_uid);
    assert_eq!(new_entry["supersedes"][0], old_uid);
}

#[test]
fn test_supersede_same_id_rejected() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let out = lk_bin()
        .args(["add", "Entry", "--content", "content", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let id = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    let output = lk_bin()
        .args(["supersede", &id.to_string(), &id.to_string()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_status_extension() {
    let dir = setup_temp_project();
    lk_in(dir.path()).arg("init").output().unwrap();

    let out = lk_in(dir.path())
        .args([
            "add",
            "ADR Entry",
            "--content",
            "Decision content",
            "--category",
            "decisions",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Set status to proposed
    let output = lk_in(dir.path())
        .args(["edit", &id.to_string(), "--status", "proposed", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entry["status"], "proposed");

    // Set status to accepted
    let output = lk_in(dir.path())
        .args(["edit", &id.to_string(), "--status", "accepted", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entry["status"], "accepted");

    // Invalid status should fail
    let output = lk_in(dir.path())
        .args(["edit", &id.to_string(), "--status", "invalid"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    // List with --status filter
    let output = lk_in(dir.path())
        .args(["list", "--status", "accepted", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "accepted");
}

#[test]
fn test_uid_in_entries() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let out = lk_bin()
        .args(["add", "UID Test", "--content", "content", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    let output = lk_bin()
        .args(["get", &id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let uid = entry["uid"].as_str().unwrap();
    assert_eq!(uid.len(), 12); // 12 hex chars
    assert!(uid.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_init_global() {
    let home = tempfile::tempdir().unwrap();
    let output = lk_bin()
        .args(["init", "--global"])
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout: {stdout}");

    // Verify ~/.claude/lk-instructions.md was created
    let instructions_path = home.path().join(".claude/lk-instructions.md");
    assert!(instructions_path.exists());
    let instructions = std::fs::read_to_string(&instructions_path).unwrap();
    assert!(instructions.contains("Knowledge Base (local-knowledge-cli)"));

    // Verify ~/.claude/CLAUDE.md was created with import line
    let claude_md_path = home.path().join(".claude/CLAUDE.md");
    assert!(claude_md_path.exists());
    let claude_md = std::fs::read_to_string(&claude_md_path).unwrap();
    assert!(claude_md.contains("@lk-instructions.md"));
}

#[test]
fn test_init_global_idempotent() {
    let home = tempfile::tempdir().unwrap();

    // Run twice
    lk_bin()
        .args(["init", "--global"])
        .env("HOME", home.path())
        .output()
        .unwrap();
    let output = lk_bin()
        .args(["init", "--global"])
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // CLAUDE.md should not have duplicate import lines
    let claude_md = std::fs::read_to_string(home.path().join(".claude/CLAUDE.md")).unwrap();
    let count = claude_md.matches("@lk-instructions.md").count();
    assert_eq!(count, 1, "import line should appear exactly once");
}

#[test]
fn test_init_global_appends_to_existing_claude_md() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("CLAUDE.md"), "# My Config\n").unwrap();

    let output = lk_bin()
        .args(["init", "--global"])
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let claude_md = std::fs::read_to_string(claude_dir.join("CLAUDE.md")).unwrap();
    assert!(claude_md.starts_with("# My Config\n"));
    assert!(claude_md.contains("@lk-instructions.md"));
}

// ── user-scope fallback for uninitialized projects ───────────────────

/// An uninitialized project (no `lk init`) whose `add` (default scope) falls back
/// to the user store under a temp HOME, plus reads against it.
#[test]
fn test_uninit_add_falls_back_to_user() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project(); // has .git, but no `lk init`

    // add with default (auto) scope → should fall back to user
    let output = lk_bin()
        .args([
            "add",
            "global note",
            "--keywords",
            "fb",
            "--content",
            "saved via fallback",
            "--json",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "uninit add should succeed via fallback"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"scope\": \"user\""),
        "should save to user scope: {stdout}"
    );
    assert!(
        stdout.contains("\"fell_back_to_user\": true"),
        "should report fallback: {stdout}"
    );
    // user DB created under the temp HOME, never inside the project
    assert!(home.path().join(".config/lk/knowledge.db").is_file());
    assert!(!proj.path().join(".knowledge/knowledge.db").exists());

    // reads against an uninitialized project should not error, returning user entries
    let search = lk_bin()
        .args(["search", "fallback", "--json"])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(search.status.success(), "uninit search should not error");
    let s = String::from_utf8_lossy(&search.stdout);
    assert!(
        s.contains("\"scope\": \"user\""),
        "search should surface user entry: {s}"
    );
}

/// An explicit `--scope project` must still error (init prompt) when uninitialized.
#[test]
fn test_uninit_explicit_project_errors() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let output = lk_bin()
        .args(["add", "x", "--content", "c", "--scope", "project"])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "explicit --scope project must error when uninitialized"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lk init"),
        "error should prompt to run lk init: {stderr}"
    );
}

// ── user-scope markdown export/sync (ADR id:33) ──────────────────────

/// `lk add --scope user` → `lk export --scope user` writes markdown under
/// `~/.config/lk/knowledge/`, flips the entry to `shared`, and a default global
/// `config.toml` is scaffolded.
#[test]
fn test_user_scope_export_writes_markdown() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    // Seed a user-scope entry.
    let add = run(&[
        "add",
        "user pref",
        "--keywords",
        "prefs",
        "--content",
        "always use tabs",
        "--scope",
        "user",
    ]);
    assert!(add.status.success());

    // Export it to the user-scope markdown dir.
    let export = run(&["export", "--scope", "user"]);
    assert!(
        export.status.success(),
        "user export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    // Markdown landed under the user knowledge dir, never inside the project.
    let knowledge_dir = home.path().join(".config/lk/knowledge");
    let exported: Vec<_> = std::fs::read_dir(&knowledge_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("exported-"))
        .collect();
    assert_eq!(exported.len(), 1, "expected one exported-*.md file");
    assert!(!proj.path().join(".knowledge").exists());

    // Default global config.toml was scaffolded.
    assert!(home.path().join(".config/lk/config.toml").is_file());

    // The entry is now shared in the user DB.
    let list = run(&["list", "--scope", "user", "--source", "shared", "--json"]);
    let s = String::from_utf8_lossy(&list.stdout);
    assert!(s.contains("user pref"), "entry should be shared now: {s}");
}

#[test]
fn test_add_records_project_and_get_reports_it() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env_remove("LK_PROJECT")
            .output()
            .unwrap()
    };

    // An explicit owner/repo is stored verbatim, and `add --json` echoes it.
    let add = run(&[
        "add",
        "project note",
        "--keywords",
        "origin",
        "--content",
        "recorded from elsewhere",
        "--scope",
        "user",
        "--project",
        "syarihu/other-repo",
    ]);
    assert!(add.status.success());
    let added: serde_json::Value = serde_json::from_slice(
        &run(&[
            "add",
            "project note two",
            "--keywords",
            "origin",
            "--content",
            "second",
            "--scope",
            "user",
            "--project",
            "syarihu/other-repo",
            "--json",
        ])
        .stdout,
    )
    .unwrap();
    assert_eq!(added["project"], "syarihu/other-repo");

    // `get` reports the full slug; `search` shows the repo name on user-scope hits.
    let get = run(&["get", added["uid"].as_str().unwrap(), "--json"]);
    let entry: serde_json::Value = serde_json::from_slice(&get.stdout).unwrap();
    assert_eq!(entry["project"], "syarihu/other-repo");

    let search = run(&["search", "elsewhere", "--scope", "user"]);
    let text = String::from_utf8_lossy(&search.stdout);
    assert!(
        text.contains("@other-repo"),
        "search should badge user-scope hits with the repo name: {text}"
    );
}

#[test]
fn test_search_badges_only_other_projects() {
    // The badge marks knowledge carried in from elsewhere. An entry recorded against
    // the project you are standing in needs no badge — and one recorded against
    // another repo gets it even in project scope, where `--project` can put it.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env("LK_PROJECT", "syarihu/here")
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());

    assert!(
        run(&["add", "from here", "-k", "badge", "-c", "local knowledge"])
            .status
            .success()
    );
    assert!(
        run(&[
            "add",
            "from elsewhere",
            "-k",
            "badge",
            "-c",
            "imported knowledge",
            "--project",
            "syarihu/elsewhere",
        ])
        .status
        .success()
    );

    let out = run(&["search", "knowledge"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("@elsewhere"),
        "another project's entry must be badged: {text}"
    );
    assert!(
        !text.contains("@here"),
        "the current project must not be badged: {text}"
    );
}

/// Seed a user-scope store with one entry per project shape and return the runner.
fn setup_project_filter_fixture(
    home: &tempfile::TempDir,
    proj: &tempfile::TempDir,
) -> impl Fn(&[&str]) -> std::process::Output {
    let home = home.path().to_path_buf();
    let proj = proj.path().to_path_buf();
    let run = move |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(&proj)
            .env("HOME", &home)
            .env_remove("LK_PROJECT")
            .output()
            .unwrap()
    };
    for (title, project) in [
        ("filter hoge entry", "hoge/app"),
        ("filter fuga entry", "fuga/app"),
        ("filter lk entry", "syarihu/local-knowledge-cli"),
    ] {
        let out = run(&[
            "add",
            title,
            "-k",
            "filterkw",
            "-c",
            "shared body text",
            "--scope",
            "user",
            "--project",
            project,
        ]);
        assert!(out.status.success(), "seeding {title} failed");
    }
    run
}

#[test]
fn test_ties_prefer_the_current_project() {
    // `--keyword-only` takes the unranked path, where every hit scores the same, so
    // the tie-break is the only thing deciding the order. The entry from elsewhere is
    // added last (newer `updated_at`), which is what would otherwise win.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = |args: &[&str], env: Option<(&str, &str)>| {
        let mut cmd = lk_bin();
        cmd.args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env("LK_PROJECT", "syarihu/here");
        if let Some((k, v)) = env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };

    run(
        &[
            "add",
            "tie entry from here",
            "-k",
            "tiekw",
            "-c",
            "body",
            "--scope",
            "user",
        ],
        None,
    );
    run(
        &[
            "add",
            "tie entry from elsewhere",
            "-k",
            "tiekw",
            "-c",
            "body",
            "--scope",
            "user",
        ],
        Some(("LK_PROJECT", "syarihu/elsewhere")),
    );

    let out = run(
        &[
            "search",
            "tiekw",
            "--keyword-only",
            "--scope",
            "user",
            "--json",
        ],
        None,
    );
    let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0]["project"], "syarihu/here",
        "a tie must favour the project we are standing in: {hits:?}"
    );

    // Standing somewhere else flips it, which shows the order follows the current
    // project rather than anything baked into the entries.
    let out = run(
        &[
            "search",
            "tiekw",
            "--keyword-only",
            "--scope",
            "user",
            "--json",
        ],
        Some(("LK_PROJECT", "syarihu/elsewhere")),
    );
    let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(hits[0]["project"], "syarihu/elsewhere", "{hits:?}");
}

#[test]
fn test_limit_one_still_reaches_the_current_projects_entry() {
    // The boundary the preference exists for: with `--limit 1` the SQL hands back
    // only the newest row, so a preference applied after the merge could never
    // promote the current project's older entry. It has to happen inside the query.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = |args: &[&str], project: Option<&str>| {
        let mut cmd = lk_bin();
        cmd.args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env_remove("LK_PROJECT");
        if let Some(p) = project {
            cmd.env("LK_PROJECT", p);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };

    // Attributed to this directory's own project (no LK_PROJECT, so it is detected),
    // and made the OLDER of the two below, so `updated_at DESC` would drop it.
    let here = proj
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap()
        .to_string();
    run(
        &[
            "add",
            "limit one from here",
            "-k",
            "limitkw",
            "-c",
            "body",
            "--scope",
            "user",
        ],
        None,
    );
    run(
        &[
            "add",
            "limit one from elsewhere",
            "-k",
            "limitkw",
            "-c",
            "body",
            "--scope",
            "user",
        ],
        Some("syarihu/elsewhere"),
    );
    // Pinned rather than waited for: entries added moments apart share a timestamp
    // (second granularity), which would leave the ordering under test to scan order.
    set_updated_at(
        &home.path().join(".config/lk/knowledge.db"),
        &[
            ("limit one from here", "2020-01-01T00:00:00"),
            ("limit one from elsewhere", "2030-01-01T00:00:00"),
        ],
    );

    let expect_here = |out: std::process::Output, what: &str| {
        let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(hits.len(), 1, "{what}: {hits:?}");
        assert_eq!(
            hits[0]["project"], here,
            "{what}: the single slot must go to the current project's older entry: {hits:?}"
        );
    };
    expect_here(
        run(
            &[
                "search",
                "limitkw",
                "--keyword-only",
                "--scope",
                "user",
                "--limit",
                "1",
                "--json",
            ],
            None,
        ),
        "cli",
    );

    // The same through MCP, which resolves the project from the request's root.
    let replies = mcp_request_with_home(
        proj.path(),
        home.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search_knowledge","arguments":{"query":"limitkw","keyword_only":true,"scope":"user","limit":1}}}"#,
    );
    let body = replies
        .iter()
        .find_map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{replies:?}"));
    let out: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("search_knowledge did not return JSON ({e}): {body}"));
    let entries = out["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "body: {body}");
    assert_eq!(entries[0]["project"], here, "body: {body}");
}

#[test]
fn test_stats_by_project_counts_across_scopes() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env_remove("LK_PROJECT")
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());

    // Two entries for one project (one per scope), one for another, one unattributed.
    for (scope, project) in [
        ("user", "hoge/app"),
        ("project", "hoge/app"),
        ("user", "fuga/app"),
    ] {
        assert!(
            run(&[
                "add",
                &format!("stats entry {scope} {project}"),
                "-k",
                "statskw",
                "-c",
                "body",
                "--scope",
                scope,
                "--project",
                project,
            ])
            .status
            .success()
        );
    }
    let added: serde_json::Value = serde_json::from_slice(
        &run(&[
            "add",
            "stats entry unattributed",
            "-k",
            "statskw",
            "-c",
            "body",
            "--scope",
            "user",
            "--json",
        ])
        .stdout,
    )
    .unwrap();
    assert!(
        run(&["edit", added["uid"].as_str().unwrap(), "--project", ""])
            .status
            .success()
    );

    let out = run(&["stats", "--by-project", "--json"]);
    assert!(out.status.success());
    let stats: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = stats["by_project"].as_array().expect("by_project array");
    let count_for = |name: Option<&str>| -> i64 {
        rows.iter()
            .find(|r| r["project"].as_str() == name)
            .map(|r| r["entries"].as_i64().unwrap())
            .unwrap_or(0)
    };
    // The same project in both stores is one row, not two.
    assert_eq!(count_for(Some("hoge/app")), 2, "rows: {rows:?}");
    assert_eq!(count_for(Some("fuga/app")), 1);
    assert_eq!(count_for(None), 1, "unattributed entries must be visible");

    // The human-readable form names the unattributed group rather than dropping it.
    let text = String::from_utf8_lossy(&run(&["stats", "--by-project"]).stdout).to_string();
    assert!(
        text.contains("By project:") && text.contains("(unattributed)"),
        "{text}"
    );
}

#[test]
fn test_search_project_filter_exact_and_bare() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = setup_project_filter_fixture(&home, &proj);

    let projects = |args: &[&str]| -> Vec<String> {
        let out = run(args);
        assert!(
            out.status.success(),
            "{:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
        let mut got: Vec<String> = hits
            .iter()
            .filter_map(|h| h["project"].as_str().map(str::to_string))
            .collect();
        got.sort();
        got
    };

    // A full slug is exact: `hoge/app` must never answer with `fuga/app`.
    assert_eq!(
        projects(&[
            "search",
            "shared",
            "--scope",
            "user",
            "--project",
            "hoge/app",
            "--json"
        ]),
        vec!["hoge/app"]
    );
    // A bare name deliberately spans owners.
    assert_eq!(
        projects(&[
            "search",
            "shared",
            "--scope",
            "user",
            "--project",
            "app",
            "--json"
        ]),
        vec!["fuga/app", "hoge/app"]
    );
    // Unfiltered still returns everything.
    assert_eq!(
        projects(&["search", "shared", "--scope", "user", "--json"]).len(),
        3
    );
}

#[test]
fn test_bare_project_filter_warns_when_it_spans_owners() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = setup_project_filter_fixture(&home, &proj);

    let out = run(&["search", "shared", "--scope", "user", "--project", "app"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("matches 2 recorded project values") && stderr.contains("fuga/app"),
        "an ambiguous bare name must say so: {stderr}"
    );

    // The case the warning exists for: a page that can only show one project. The
    // check asks the store, so the limit cannot hide the ambiguity.
    let out = run(&[
        "search",
        "shared",
        "--scope",
        "user",
        "--project",
        "app",
        "--limit",
        "1",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("matches 2 recorded project values"),
        "--limit 1 must still warn: {stderr}"
    );

    // An exact slug is unambiguous by construction, so it stays quiet.
    let out = run(&[
        "search",
        "shared",
        "--scope",
        "user",
        "--project",
        "hoge/app",
    ]);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("matched"));
}

#[test]
fn test_list_project_filter_agrees_with_search() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = setup_project_filter_fixture(&home, &proj);

    let uids = |args: &[&str]| -> Vec<String> {
        let out = run(args);
        assert!(out.status.success());
        let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
        let mut got: Vec<String> = hits
            .iter()
            .map(|h| h["uid"].as_str().unwrap().to_string())
            .collect();
        got.sort();
        got
    };

    // `list` filters in Rust, `search` in SQL — they must answer the same question.
    assert_eq!(
        uids(&["list", "--scope", "user", "--project", "app", "--json"]),
        uids(&[
            "search",
            "shared",
            "--scope",
            "user",
            "--project",
            "app",
            "--json"
        ])
    );
}

#[test]
fn test_project_filter_dot_means_the_current_project() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = setup_project_filter_fixture(&home, &proj);

    // One more entry attributed to the project we are standing in.
    let here = proj
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        run(&[
            "add",
            "filter here entry",
            "-k",
            "filterkw",
            "-c",
            "shared body text",
            "--scope",
            "user"
        ])
        .status
        .success()
    );

    let out = run(&[
        "search",
        "shared",
        "--scope",
        "user",
        "--project",
        ".",
        "--json",
    ]);
    assert!(out.status.success());
    let hits: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(hits.len(), 1, "only the current project's entry");
    assert_eq!(hits[0]["project"], here);
}

#[test]
fn test_invalid_project_filter_errors_instead_of_returning_nothing() {
    // Silently matching nothing would read as "no knowledge here", which is worse
    // than an error: it invites re-recording something that already exists.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = setup_project_filter_fixture(&home, &proj);

    let out = run(&[
        "search",
        "shared",
        "--scope",
        "user",
        "--project",
        "///",
        "--json",
    ]);
    assert!(!out.status.success(), "an unusable filter must fail loudly");
    assert!(String::from_utf8_lossy(&out.stderr).contains("--project"));
}

#[test]
fn test_edit_project_sets_and_clears() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env_remove("LK_PROJECT")
            .output()
            .unwrap()
    };
    let added: serde_json::Value = serde_json::from_slice(
        &run(&[
            "add",
            "edit project entry",
            "-k",
            "editkw",
            "-c",
            "body",
            "--scope",
            "user",
            "--json",
        ])
        .stdout,
    )
    .unwrap();
    let uid = added["uid"].as_str().unwrap().to_string();

    assert!(
        run(&["edit", &uid, "--project", "syarihu/backfilled"])
            .status
            .success()
    );
    let entry: serde_json::Value =
        serde_json::from_slice(&run(&["get", &uid, "--json"]).stdout).unwrap();
    assert_eq!(entry["project"], "syarihu/backfilled");

    // An unusable value must not quietly replace the attribution with a detected
    // one: "set this value" is not "set whatever you can figure out".
    let out = run(&["edit", &uid, "--project", "///"]);
    assert!(!out.status.success(), "an unusable value must fail loudly");
    let entry: serde_json::Value =
        serde_json::from_slice(&run(&["get", &uid, "--json"]).stdout).unwrap();
    assert_eq!(
        entry["project"], "syarihu/backfilled",
        "the existing attribution must survive a rejected edit"
    );

    // An empty value clears it, which is how a wrong attribution gets removed.
    assert!(run(&["edit", &uid, "--project", ""]).status.success());
    let entry: serde_json::Value =
        serde_json::from_slice(&run(&["get", &uid, "--json"]).stdout).unwrap();
    assert!(
        entry["project"].is_null(),
        "project should be cleared: {entry}"
    );
}

#[test]
fn test_mcp_project_filter_dot_follows_the_requested_project() {
    // The MCP server serves registered projects from one process, so `.` must mean
    // the project the request targets — not the directory the server was started in
    // (here, this repository). Reverting to the CWD-based resolution fails this.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let targeted = proj
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap()
        .to_string();

    let add = |args: &[&str]| {
        let out = lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env_remove("LK_PROJECT")
            .output()
            .unwrap();
        assert!(out.status.success());
    };
    // One entry attributed to the targeted project (detected from its directory)...
    add(&[
        "add",
        "dot filter targeted",
        "-k",
        "dotkw",
        "-c",
        "shared body text",
        "--scope",
        "user",
    ]);
    // ...and one attributed to the repo the server process runs in.
    add(&[
        "add",
        "dot filter server cwd",
        "-k",
        "dotkw",
        "-c",
        "shared body text",
        "--scope",
        "user",
        "--project",
        "syarihu/local-knowledge-cli",
    ]);

    let replies = mcp_request_with_home(
        proj.path(),
        home.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search_knowledge","arguments":{"query":"shared","scope":"user","project_filter":"."}}}"#,
    );
    let body = replies
        .iter()
        .find_map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{replies:?}"));
    let out: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("search_knowledge did not return JSON ({e}): {body}"));
    let entries = out["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "body: {body}");
    assert_eq!(
        entries[0]["project"], targeted,
        "`.` must resolve to the requested project, not the server's directory: {body}"
    );
}

#[test]
fn test_mcp_project_filter_rejects_a_non_string_value() {
    // Reading a malformed filter as "absent" would widen the search to everything —
    // the opposite of what was asked.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let _run = setup_project_filter_fixture(&home, &proj);

    let replies = mcp_request_with_home(
        proj.path(),
        home.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search_knowledge","arguments":{"query":"shared","scope":"user","project_filter":123}}}"#,
    );
    let text = format!("{replies:?}");
    assert!(
        text.contains("project_filter"),
        "a non-string filter must be refused: {text}"
    );
    assert!(
        !text.contains("hoge/app") || text.contains("expected a string"),
        "it must not fall back to an unfiltered search: {text}"
    );
}

#[test]
fn test_mcp_project_filter() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = setup_project_filter_fixture(&home, &proj);
    let _ = run(&["list", "--scope", "user", "--json"]);

    let replies = mcp_request_with_home(
        proj.path(),
        home.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search_knowledge","arguments":{"query":"shared","scope":"user","project_filter":"hoge/app"}}}"#,
    );
    let body = replies
        .iter()
        .find_map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{replies:?}"));
    let out: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("search_knowledge did not return JSON ({e}): {body}"));
    let entries = out["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1, "body: {body}");
    assert_eq!(entries[0]["project"], "hoge/app");
}

#[test]
fn test_git_config_lk_project_overrides_the_remote() {
    // The lasting per-repo override: it beats the detected remote, and `LK_PROJECT`
    // still beats it (one invocation should always be able to say otherwise).
    let home = tempfile::tempdir().unwrap();
    let proj = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(proj.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    assert!(
        git(&[
            "remote",
            "add",
            "origin",
            "git@github.com:syarihu/detected.git"
        ])
        .status
        .success()
    );
    assert!(
        git(&["config", "lk.project", "syarihu/configured"])
            .status
            .success()
    );

    let add = |extra_env: Option<(&str, &str)>, title: &str| {
        let mut cmd = lk_bin();
        cmd.args([
            "add", title, "-k", "gitcfg", "-c", "body", "--scope", "user", "--json",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .env_remove("LK_PROJECT");
        if let Some((k, v)) = extra_env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success());
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };

    assert_eq!(
        add(None, "configured project")["project"],
        "syarihu/configured",
        "git config lk.project must win over the origin remote"
    );
    assert_eq!(
        add(Some(("LK_PROJECT", "syarihu/from-env")), "env beats config")["project"],
        "syarihu/from-env",
        "a one-off LK_PROJECT must still win"
    );
}

#[test]
fn test_lk_project_env_overrides_detection() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let add = lk_bin()
        .args([
            "add",
            "env note",
            "--keywords",
            "env",
            "--content",
            "from env",
            "--scope",
            "user",
            "--json",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .env("LK_PROJECT", "git@github.com:syarihu/from-env.git")
        .output()
        .unwrap();
    assert!(add.status.success());
    let out: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    // The remote URL is normalized to a slug before it is stored.
    assert_eq!(out["project"], "syarihu/from-env");
}

#[test]
fn test_mcp_add_records_the_project_it_was_called_for() {
    // The MCP write path resolves the project from the request's own project root
    // (not the server's cwd, and deliberately not `LK_PROJECT`). Without this test the
    // recording could be dropped from `add_knowledge` and everything still passed.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let expected = proj
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap()
        .to_string();

    let replies = mcp_request_with_home(
        proj.path(),
        home.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"add_knowledge","arguments":{"title":"mcp added note","content":"body","keywords":["mcpadd"],"scope":"user"}}}"#,
    );
    let body = replies
        .iter()
        .find_map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{replies:?}"));
    let out: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("add_knowledge did not return JSON ({e}): {body}"));
    assert_eq!(out["added"], true, "body: {body}");
    // No git remote in a temp project, so the key falls back to the directory name.
    assert_eq!(
        out["recorded_project"], expected,
        "the entry must be attributed to the requested project: {body}"
    );

    // And the value is really on the entry, not just in the reply.
    let uid = out["uid"].as_str().unwrap();
    let get = lk_bin()
        .args(["get", uid, "--json"])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&get.stdout).unwrap();
    assert_eq!(entry["project"], expected);
}

#[test]
fn test_project_flag_cannot_inject_markdown_metadata() {
    // A newline in the value would become a second metadata line once exported, and
    // the next sync would apply it (`status: deprecated` here). The value must be
    // refused, leaving detection to fill the field.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let add = lk_bin()
        .args([
            "add",
            "injection attempt",
            "-k",
            "inject",
            "-c",
            "body",
            "--scope",
            "user",
            "--project",
            "owner/repo\nstatus: deprecated",
            "--json",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .env_remove("LK_PROJECT")
        .output()
        .unwrap();
    assert!(add.status.success());
    let out: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    let recorded = out["project"].as_str().unwrap_or_default();
    assert!(
        !recorded.contains('\n') && !recorded.contains("status:"),
        "a control character must never reach the stored project: {recorded:?}"
    );
    assert_eq!(
        out["status"], "active",
        "the injected status must not apply"
    );
}

#[test]
fn test_mcp_get_keeps_the_entrys_project_distinct_from_the_target_project() {
    // `get_knowledge`'s result IS the entry object, and the server decorates every
    // result with a top-level `project` naming the project the request targeted.
    // The entry's own project must survive that (it is the point of the column).
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let add = lk_bin()
        .args([
            "add",
            "mcp origin note",
            "--keywords",
            "origin",
            "--content",
            "from another repo",
            "--scope",
            "user",
            "--project",
            "syarihu/other-repo",
            "--json",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(add.status.success());
    let added: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    let uid = added["uid"].as_str().unwrap();

    let replies = mcp_request_with_home(
        proj.path(),
        home.path(),
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"get_knowledge","arguments":{{"id":"{uid}"}}}}}}"#
        ),
    );
    let body = replies
        .iter()
        .find_map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{replies:?}"));
    let entry: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| {
        panic!("get_knowledge did not return JSON ({e}): {body}");
    });

    assert_eq!(
        entry["recorded_project"], "syarihu/other-repo",
        "the entry's own project must survive result decoration: {body}"
    );
    // And the decorated key still names the targeted project, not the entry's.
    assert_ne!(entry["project"], "syarihu/other-repo", "body: {body}");
}

#[test]
fn test_user_scope_sync_preserves_project() {
    // The regression this guards: `sync` deletes and re-inserts a file's entries, so
    // a project not carried in the markdown would silently become NULL.
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .env("LK_PROJECT", "syarihu/recorded-here")
            .output()
            .unwrap()
    };

    assert!(
        run(&[
            "add",
            "keep my project",
            "--keywords",
            "keep",
            "--content",
            "v1",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    assert!(run(&["export", "--scope", "user"]).status.success());

    let knowledge_dir = home.path().join(".config/lk/knowledge");
    let md = std::fs::read_dir(&knowledge_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("exported-"))
        })
        .expect("expected an exported-*.md file");

    let text = std::fs::read_to_string(&md).unwrap();
    assert!(
        text.contains("project: syarihu/recorded-here"),
        "export must write the project into md: {text}"
    );

    // Change the file so sync re-imports it (hash-gated), then confirm the project survived.
    std::fs::write(&md, text.replace("v1", "v2-edited")).unwrap();
    let sync = run(&["sync", "--scope", "user", "--json"]);
    assert!(
        sync.status.success(),
        "user sync failed: {}",
        String::from_utf8_lossy(&sync.stderr)
    );

    let search = run(&["search", "v2-edited", "--scope", "user", "--json"]);
    let results: Vec<serde_json::Value> = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(results.len(), 1, "expected the re-imported entry");
    assert_eq!(
        results[0]["project"], "syarihu/recorded-here",
        "sync dropped the project on re-import"
    );
}

/// Editing the exported markdown and running `lk sync --scope user` imports the
/// change back into the user DB (md is the source of truth). `--write-uids` bakes
/// the uid into the markdown.
#[test]
fn test_user_scope_sync_round_trip() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    assert!(
        run(&[
            "add",
            "rt note",
            "--keywords",
            "sync",
            "--content",
            "v1",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    assert!(run(&["export", "--scope", "user"]).status.success());

    // Locate the single exported markdown file (group name = first keyword).
    let knowledge_dir = home.path().join(".config/lk/knowledge");
    let md = std::fs::read_dir(&knowledge_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("exported-"))
        })
        .expect("expected an exported-*.md file");

    // Edit the markdown content, then sync it back.
    let text = std::fs::read_to_string(&md).unwrap();
    std::fs::write(&md, text.replace("v1", "v2-edited")).unwrap();

    let sync = run(&["sync", "--scope", "user", "--write-uids", "--json"]);
    assert!(
        sync.status.success(),
        "user sync failed: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let out = String::from_utf8_lossy(&sync.stdout);
    assert!(
        out.contains("\"updated\":") || out.contains("\"added\":"),
        "sync json should report stats: {out}"
    );

    // The DB now reflects the edited content.
    let search = run(&["search", "v2-edited", "--scope", "user", "--full", "--json"]);
    let s = String::from_utf8_lossy(&search.stdout);
    assert!(s.contains("v2-edited"), "edited content not synced: {s}");

    // UID was written back into the markdown.
    let md_after = std::fs::read_to_string(&md).unwrap();
    assert!(
        md_after.contains("uid:"),
        "write_uids should bake a uid into md: {md_after}"
    );
}

/// `user_knowledge_dir` in the global config.toml redirects the export target.
#[test]
fn test_user_scope_export_honors_config_dir() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    // Point the user knowledge dir at a custom location under HOME.
    let cfg_dir = home.path().join(".config/lk");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("config.toml"),
        "user_knowledge_dir = ~/custom-knowledge\n",
    )
    .unwrap();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    assert!(
        run(&[
            "add",
            "cfg note",
            "--keywords",
            "cfg",
            "--content",
            "c",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    assert!(run(&["export", "--scope", "user"]).status.success());

    // Exported to the configured dir, not the default ~/.config/lk/knowledge.
    let custom = home.path().join("custom-knowledge");
    let exported: Vec<_> = std::fs::read_dir(&custom)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("exported-"))
        .collect();
    assert_eq!(
        exported.len(),
        1,
        "export should honor user_knowledge_dir and write one file"
    );
    assert!(!home.path().join(".config/lk/knowledge").exists());
}

/// Two markdown files sharing a uid is an identity conflict: sync fails up front
/// with a clear, actionable message and leaves the DB untouched (no silent data loss).
#[test]
fn test_user_scope_sync_rejects_duplicate_uid() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    assert!(
        run(&[
            "add",
            "dup note",
            "--keywords",
            "dup",
            "--content",
            "body",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    assert!(run(&["export", "--scope", "user"]).status.success());

    // Duplicate the exported file (same uid baked in) under a different name.
    let kdir = home.path().join(".config/lk/knowledge");
    let orig = std::fs::read_dir(&kdir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("exported-"))
        })
        .unwrap();
    std::fs::copy(&orig, kdir.join("zzz-copy.md")).unwrap();

    // Sync must fail with a clear conflict message, not silently skip.
    let sync = run(&["sync", "--scope", "user", "--json"]);
    assert!(
        !sync.status.success(),
        "sync should reject the duplicate uid"
    );
    let stderr = String::from_utf8_lossy(&sync.stderr);
    assert!(
        stderr.contains("duplicate uid"),
        "should explain the duplicate uid conflict: {stderr}"
    );

    // The original entry is untouched — still findable after the rejected sync.
    let search = run(&["search", "dup note", "--scope", "user", "--json"]);
    assert!(
        String::from_utf8_lossy(&search.stdout).contains("dup note"),
        "rejected sync must not delete the existing entry"
    );
}

/// Exporting into a `--dir` that equals the configured user_knowledge_dir is a
/// normal managed export (flips to shared, syncable) — not a dump.
#[test]
fn test_user_scope_export_dir_equal_to_managed_is_managed() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let kdir = home.path().join(".config/lk/knowledge");

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };
    assert!(
        run(&[
            "add",
            "m note",
            "--keywords",
            "m",
            "--content",
            "c",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    // --dir explicitly set to the default managed dir → should behave as managed.
    let export = run(&["export", "--scope", "user", "--dir", kdir.to_str().unwrap()]);
    assert!(export.status.success());
    assert!(
        !String::from_utf8_lossy(&export.stderr).contains("one-off dump"),
        "exporting to the managed dir should not warn about a dump"
    );
    // Entry is now shared (managed), not left local.
    let shared = run(&["list", "--scope", "user", "--source", "shared", "--json"]);
    assert!(
        String::from_utf8_lossy(&shared.stdout).contains("m note"),
        "export to managed dir should flip entry to shared"
    );
}

/// `lk export --scope user --dir <X>` warns that sync won't read the custom dir.
#[test]
fn test_user_scope_export_dir_warns() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    assert!(
        run(&[
            "add",
            "d note",
            "--keywords",
            "d",
            "--content",
            "c",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    let dump = home.path().join("one-off");
    let export = run(&["export", "--scope", "user", "--dir", dump.to_str().unwrap()]);
    assert!(export.status.success());
    let stderr = String::from_utf8_lossy(&export.stderr);
    assert!(
        stderr.contains("one-off dump"),
        "should warn that --dir is a dump-only export: {stderr}"
    );

    // Dump-only: the entry must stay `local` (not flipped to `shared`), so a later
    // `sync --scope user` — which never sees the custom dir — cannot delete it.
    let list_local = run(&["list", "--scope", "user", "--source", "local", "--json"]);
    assert!(
        String::from_utf8_lossy(&list_local.stdout).contains("d note"),
        "dumped entry should remain local"
    );
    let sync = run(&["sync", "--scope", "user", "--json"]);
    assert!(sync.status.success());
    let still_there = run(&["search", "d note", "--scope", "user", "--json"]);
    assert!(
        String::from_utf8_lossy(&still_there.stdout).contains("d note"),
        "entry must survive a later user-scope sync (no data loss)"
    );
}

/// Secret detection blocks user-scope export of an entry containing a key.
#[test]
fn test_user_scope_export_blocks_secrets() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    // Add with --allow-secrets so the entry lands in the DB, then export without it.
    assert!(
        run(&[
            "add",
            "leak",
            "--keywords",
            "leak",
            "--content",
            "AKIAIOSFODNN7EXAMPLE is my key",
            "--scope",
            "user",
            "--allow-secrets",
        ])
        .status
        .success()
    );
    let export = run(&["export", "--scope", "user"]);
    assert!(
        !export.status.success(),
        "export should refuse to write a secret to the user store"
    );
    // The dir may be pre-created, but no markdown should have been written.
    let kdir = home.path().join(".config/lk/knowledge");
    let wrote_md = std::fs::read_dir(&kdir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("exported-"))
        })
        .unwrap_or(false);
    assert!(
        !wrote_md,
        "no markdown should be written when a secret is detected"
    );
}

/// A symlinked user_knowledge_dir (the dotfiles use case) still round-trips:
/// export → edit md → sync reflects the change (canonicalized rel-path agreement).
#[cfg(unix)]
#[test]
fn test_user_scope_symlinked_dir_round_trip() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    // Real knowledge store lives in a "dotfiles" dir; ~/.config/lk/knowledge → it.
    let real = home.path().join("dotfiles/lk-knowledge");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::create_dir_all(home.path().join(".config/lk")).unwrap();
    std::os::unix::fs::symlink(&real, home.path().join(".config/lk/knowledge")).unwrap();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    assert!(
        run(&[
            "add",
            "sym note",
            "--keywords",
            "sym",
            "--content",
            "before",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    assert!(run(&["export", "--scope", "user"]).status.success());

    // The md lands in the real dir (through the symlink).
    let md = std::fs::read_dir(&real)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("exported-"))
        })
        .expect("expected exported md in the symlink target");

    let text = std::fs::read_to_string(&md).unwrap();
    std::fs::write(&md, text.replace("before", "after")).unwrap();

    let sync = run(&["sync", "--scope", "user", "--json"]);
    assert!(
        sync.status.success(),
        "symlinked sync failed: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let out = String::from_utf8_lossy(&sync.stdout);
    assert!(
        out.contains("\"updated\":1"),
        "edit through symlink should sync as one update, not added/removed: {out}"
    );

    let search = run(&["search", "after", "--scope", "user", "--full", "--json"]);
    assert!(String::from_utf8_lossy(&search.stdout).contains("after"));
}

/// The auto-created user store is owner-only (not world-readable).
#[cfg(unix)]
#[test]
fn test_user_scope_store_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    let out = lk_bin()
        .args([
            "add",
            "p note",
            "--keywords",
            "p",
            "--content",
            "c",
            "--scope",
            "user",
        ])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode(&home.path().join(".config/lk")),
        0o700,
        "config dir should be 0700"
    );
    assert_eq!(
        mode(&home.path().join(".config/lk/knowledge.db")),
        0o600,
        "user DB should be 0600"
    );

    // Exported markdown itself must be owner-only too.
    let export = lk_bin()
        .args(["export", "--scope", "user"])
        .current_dir(proj.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(export.status.success());
    let kdir = home.path().join(".config/lk/knowledge");
    assert_eq!(mode(&kdir), 0o700, "auto-created md dir should be 0700");
    let md = std::fs::read_dir(&kdir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("exported-"))
        })
        .unwrap();
    assert_eq!(mode(&md), 0o600, "exported md should be 0600");
}

/// A pre-existing user_knowledge_dir keeps its own permissions — export must not
/// clobber a dir the user manages (e.g. a shared dotfiles location).
#[cfg(unix)]
#[test]
fn test_user_scope_export_preserves_existing_dir_perms() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    // Pre-create the default knowledge dir at a deliberately looser 0755.
    let kdir = home.path().join(".config/lk/knowledge");
    std::fs::create_dir_all(&kdir).unwrap();
    std::fs::set_permissions(&kdir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };
    assert!(
        run(&[
            "add",
            "k note",
            "--keywords",
            "k",
            "--content",
            "c",
            "--scope",
            "user",
        ])
        .status
        .success()
    );
    assert!(run(&["export", "--scope", "user"]).status.success());

    let mode = std::fs::metadata(&kdir).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o755,
        "export must not clobber a pre-existing dir's permissions"
    );
}

/// Bootstrap flow: a fresh machine with only a markdown store (no user DB yet) can
/// run `lk sync --scope user` to create and populate `~/.config/lk/knowledge.db`.
#[test]
fn test_user_scope_sync_bootstraps_db_from_markdown() {
    let home = tempfile::tempdir().unwrap();
    let proj = setup_temp_project();

    // Hand-place a markdown store (as if cloned from dotfiles); no user DB exists yet.
    let kdir = home.path().join(".config/lk/knowledge");
    std::fs::create_dir_all(&kdir).unwrap();
    std::fs::write(
        kdir.join("exported-prefs.md"),
        "---\nkeywords: [prefs]\ncategory: exported\n---\n\n\
         # Exported: prefs\n\n\
         ## Entry: editor choice\nkeywords: [editor]\nuid: bootstrapuid01\n\n\
         use neovim\n",
    )
    .unwrap();
    assert!(!home.path().join(".config/lk/knowledge.db").exists());

    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(proj.path())
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    let sync = run(&["sync", "--scope", "user", "--json"]);
    assert!(
        sync.status.success(),
        "bootstrap sync should create the DB, not error: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert!(
        home.path().join(".config/lk/knowledge.db").is_file(),
        "sync should have created the user DB"
    );
    let search = run(&["search", "neovim", "--scope", "user", "--full", "--json"]);
    assert!(
        String::from_utf8_lossy(&search.stdout).contains("use neovim"),
        "bootstrapped entry should be searchable"
    );
}

/// Project-scope `export --dir <X>` to a dir outside `.knowledge/` is a one-off dump:
/// entries stay `local` so a later `lk sync` (which only reads `.knowledge/`) can't
/// delete them. Without `--dir`, export flips to `shared` as before.
#[test]
fn test_project_export_dir_outside_knowledge_is_dump_only() {
    let dir = setup_temp_project();
    let run = |args: &[&str]| {
        lk_bin()
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    assert!(
        run(&["add", "p dump", "--keywords", "pd", "--content", "body"])
            .status
            .success()
    );

    let dump = dir.path().join("outside");
    let export = run(&["export", "--dir", dump.to_str().unwrap()]);
    assert!(export.status.success());
    assert!(
        String::from_utf8_lossy(&export.stderr).contains("one-off dump"),
        "project export outside .knowledge/ should warn it's a dump"
    );

    // Entry stays local (not flipped to shared) and survives a later sync.
    let local = run(&["list", "--source", "local", "--json"]);
    assert!(
        String::from_utf8_lossy(&local.stdout).contains("p dump"),
        "dumped entry should stay local"
    );
    assert!(run(&["sync", "--json"]).status.success());
    let after = run(&["search", "p dump", "--json"]);
    assert!(
        String::from_utf8_lossy(&after.stdout).contains("p dump"),
        "entry must survive sync (no data loss)"
    );
}

#[test]
fn test_add_manual_keywords_are_authoritative() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Content is full of extractable words, but the manual keywords must win.
    let output = lk_bin()
        .args([
            "add",
            "SessionManager token refresh",
            "--keywords",
            "session,token-refresh",
            "--content",
            "The SessionManager component refreshes access tokens through the gateway before expiry.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let add_result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let kws: Vec<String> = add_result["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kws,
        vec!["session".to_string(), "token-refresh".to_string()],
        "auto-extracted keywords must not be merged into a curated set"
    );
}

#[test]
fn test_auto_keywords_are_capped() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // 30 distinct candidate words, no manual keywords
    let content: String = (0..30)
        .map(|i| format!("uniqueword{i:02}"))
        .collect::<Vec<_>>()
        .join(" ");
    let output = lk_bin()
        .args(["add", "Capped entry", "--content", &content, "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let add_result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let count = add_result["keywords"].as_array().unwrap().len();
    assert!(
        count <= 15,
        "auto-extracted keywords must be capped at 15, got {count}"
    );
}

#[test]
fn test_keywords_regen() {
    let dir = setup_temp_project();
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    // A noisy entry: 20 keywords (> default threshold of 15)
    let noisy_kws: String = (0..20)
        .map(|i| format!("noisykeyword{i:02}"))
        .collect::<Vec<_>>()
        .join(",");
    let output = lk_bin()
        .args([
            "add",
            "Webhook retry design",
            "--keywords",
            &noisy_kws,
            "--content",
            "Webhook delivery retries use exponential backoff with a dead letter queue.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let noisy: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let noisy_id = noisy["id"].as_i64().unwrap();

    // A curated entry: few keywords, must be left alone
    let output = lk_bin()
        .args([
            "add",
            "Cache invalidation strategy",
            "--keywords",
            "cache,invalidation,ttl",
            "--content",
            "Entries expire via TTL; explicit invalidation happens on write.",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let curated: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let curated_id = curated["id"].as_i64().unwrap();

    // Dry run first: reports the noisy entry but does not write
    let output = lk_bin()
        .args(["keywords", "--regen", "--dry-run", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let dry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["regenerated"], 1);

    let output = lk_bin()
        .args(["get", &noisy_id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        entry["keywords"].as_array().unwrap().len(),
        20,
        "dry run must not modify keywords"
    );
    let updated_at_before = entry["updated_at"].as_str().unwrap().to_string();

    // Real run: noisy entry is regenerated, curated entry untouched
    let output = lk_bin()
        .args(["keywords", "--regen", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let regen: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(regen["regenerated"], 1);

    let output = lk_bin()
        .args(["get", &noisy_id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let kws: Vec<String> = entry["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    assert!(kws.len() <= 15, "regenerated keywords must be capped");
    assert!(
        kws.contains(&"webhook".to_string()),
        "regenerated keywords should reflect the entry text"
    );
    assert!(!kws.contains(&"noisykeyword00".to_string()));
    assert_eq!(
        entry["updated_at"].as_str().unwrap(),
        updated_at_before,
        "regen must not bump updated_at"
    );

    let output = lk_bin()
        .args(["get", &curated_id.to_string(), "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let kws: Vec<String> = entry["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kws,
        vec![
            "cache".to_string(),
            "invalidation".to_string(),
            "ttl".to_string()
        ],
        "curated entries at or below the threshold must be left alone"
    );
}
