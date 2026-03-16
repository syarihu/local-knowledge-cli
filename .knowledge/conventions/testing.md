---
keywords: [testing, integration-test, tempdir]
category: conventions
---

# Testing Patterns

## Entry: Integration Test Structure
keywords: [test, integration_test.rs, tempdir, assert]

All tests in `tests/integration_test.rs` are integration tests that spawn the `lk` binary as a subprocess using `Command::cargo_bin("lk")`. Each test creates an isolated temp directory via `tempfile::tempdir()` with a `.git` directory for project root detection. Tests verify both exit codes and JSON output via `serde_json::from_str`. Key tests include init idempotency, add/get/delete CRUD, FTS search, markdown import/sync, export, keyword auto-extraction, and symlink traversal blocking (security test).
