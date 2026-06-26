use std::process::Command;

fn lk_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lk"))
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
    let output = lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify DB was created
    assert!(dir.path().join(".knowledge/knowledge.db").exists());
    // Verify .knowledge/ was created
    assert!(dir.path().join(".knowledge").is_dir());
    assert!(dir.path().join(".knowledge/README.md").exists());
    // Verify .gitignore was created
    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".knowledge/knowledge.db"));
    // Verify AGENTS.md was created with import line
    let claude_md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
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
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let output = lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // AGENTS.md should not have duplicate import lines
    let agents_md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    let count = agents_md.matches("@.knowledge/lk-instructions.md").count();
    assert_eq!(count, 1, "AGENTS.md should not have duplicate import lines");
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
            "--force",
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
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    lk_bin()
        .args([
            "add",
            "OAuth Flow",
            "--keywords",
            "oauth",
            "--content",
            "OAuth 2.0 with PKCE.",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    lk_bin()
        .args([
            "add",
            "Database Schema",
            "--keywords",
            "database",
            "--content",
            "SQLite with FTS5.",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Export only OAuth-related entries
    let output = lk_bin()
        .args(["export", "--query", "OAuth"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 entries"));

    // Database entry should still be local
    let out = lk_bin()
        .args(["list", "--source", "local", "--json"])
        .current_dir(dir.path())
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
        .args([
            "add",
            "Old Decision",
            "--content",
            "Use REST",
            "--force",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out1.status.success());
    let old_id = serde_json::from_slice::<serde_json::Value>(&out1.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    let out2 = lk_bin()
        .args([
            "add",
            "New Decision",
            "--content",
            "Use gRPC",
            "--force",
            "--json",
        ])
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
    lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let out = lk_bin()
        .args([
            "add",
            "ADR Entry",
            "--content",
            "Decision content",
            "--category",
            "decisions",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Set status to proposed
    let output = lk_bin()
        .args(["edit", &id.to_string(), "--status", "proposed", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entry["status"], "proposed");

    // Set status to accepted
    let output = lk_bin()
        .args(["edit", &id.to_string(), "--status", "accepted", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entry["status"], "accepted");

    // Invalid status should fail
    let output = lk_bin()
        .args(["edit", &id.to_string(), "--status", "invalid"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());

    // List with --status filter
    let output = lk_bin()
        .args(["list", "--status", "accepted", "--json"])
        .current_dir(dir.path())
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
