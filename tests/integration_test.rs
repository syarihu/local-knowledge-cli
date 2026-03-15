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
    // Verify CLAUDE.md was created (at root, not .claude/)
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("Knowledge Base (local-knowledge-cli)"));
}

#[test]
fn test_init_idempotent() {
    let dir = setup_temp_project();

    // Run init twice
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();
    let output = lk_bin()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // CLAUDE.md should not have duplicate sections
    let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    let count = claude_md.matches("Knowledge Base (local-knowledge-cli)").count();
    assert_eq!(count, 1, "CLAUDE.md should not have duplicate sections");
}

#[test]
fn test_add_and_get() {
    let dir = setup_temp_project();
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

    // Add an entry
    let output = lk_bin()
        .args(["add", "Test Entry", "--keywords", "test,rust", "--content", "This is test content.", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let add_result: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap();
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
    assert!(entry["keywords"].as_array().unwrap().iter().any(|k| k == "test"));
    assert!(entry["keywords"].as_array().unwrap().iter().any(|k| k == "rust"));
}

#[test]
fn test_search() {
    let dir = setup_temp_project();
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

    lk_bin()
        .args(["add", "OAuth Login", "--keywords", "oauth,login", "--content", "OAuth 2.0 with PKCE flow."])
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

    let results: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0]["title"], "OAuth Login");
}

#[test]
fn test_delete() {
    let dir = setup_temp_project();
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

    let output = lk_bin()
        .args(["add", "To Delete", "--content", "Will be deleted.", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = result["id"].as_i64().unwrap();

    // Delete
    let output = lk_bin()
        .args(["delete", &id.to_string()])
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
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

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
    let results: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap();
    assert!(!results.is_empty());
}

#[test]
fn test_export() {
    let dir = setup_temp_project();
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

    // Add local entries
    lk_bin()
        .args(["add", "Local Fact", "--keywords", "local,fact", "--content", "A locally discovered fact."])
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
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("exported-")
        })
        .collect();
    assert!(!exported_files.is_empty());
}

#[test]
fn test_stats() {
    let dir = setup_temp_project();
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

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
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

    // Add entry without explicit keywords - should auto-extract
    let output = lk_bin()
        .args(["add", "SessionManager Config", "--content", "The SessionManager in src/auth/session.ts handles tokens.", "--json"])
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
    assert!(keywords.iter().any(|k| k == "session"), "Should extract 'session' from CamelCase/path");
    assert!(keywords.iter().any(|k| k == "manager"), "Should extract 'manager' from CamelCase");
    assert!(keywords.iter().any(|k| k == "auth"), "Should extract 'auth' from file path");
}

#[test]
fn test_symlink_traversal_blocked() {
    let dir = setup_temp_project();
    lk_bin().arg("init").current_dir(dir.path()).output().unwrap();

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
    let results: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap();
    assert!(results.is_empty(), "Symlink traversal should be blocked");
}
