---
keywords: [error-handling, conventions, result, json-output]
category: conventions
---

# Error Handling & Output Conventions

## Entry: Error Propagation Pattern
keywords: [error, Result, Box-dyn-Error]

All command handlers return `Result<(), Box<dyn std::error::Error>>` and use the `?` operator for propagation. The main function at `src/main.rs:132-135` catches errors with `eprintln!` and `std::process::exit(1)`. No custom error types are defined; the project relies on boxed trait objects for simplicity.

## Entry: JSON Output Pattern
keywords: [json, serde_json, output, --json]

Most commands support a `--json` flag for machine-readable output. JSON is constructed using `serde_json::json!()` macro and printed with `serde_json::to_string_pretty()`. Human-readable output uses simple `println!` formatting. The `Entry` struct derives `Serialize` for direct JSON serialization.
