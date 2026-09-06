use serde_json::{Value, json};

pub struct PromptArgument {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

pub struct PromptDef {
    pub name: &'static str,
    pub filename: &'static str,
    pub description: &'static str,
    pub arguments: &'static [PromptArgument],
    pub raw_content: &'static str,
}

impl PromptDef {
    /// Extracts the prompt body after YAML frontmatter.
    pub fn body(&self) -> &str {
        strip_frontmatter(self.raw_content)
    }

    /// Renders the prompt body with argument substitution for `$ARGUMENTS`.
    pub fn render(&self, arguments: Option<&Value>) -> String {
        let arg_val = extract_argument_value(arguments, self.arguments);
        self.body().replace("$ARGUMENTS", &arg_val)
    }
}

pub fn strip_frontmatter(raw: &str) -> &str {
    let s = raw
        .strip_prefix("---\r\n")
        .or_else(|| raw.strip_prefix("---\n"));
    if let Some(rest) = s {
        if let Some(pos) = rest.find("\n---\r\n") {
            return rest[pos + 6..].trim_start_matches(['\r', '\n']);
        }
        if let Some(pos) = rest.find("\n---\n") {
            return rest[pos + 5..].trim_start_matches(['\r', '\n']);
        }
    }
    raw
}

fn extract_argument_value(args: Option<&Value>, defined_args: &[PromptArgument]) -> String {
    let Some(args) = args else {
        return String::new();
    };

    match args {
        Value::Object(map) => {
            // 1. Try defined argument names first
            for arg in defined_args {
                if let Some(val) = map.get(arg.name).and_then(|v| v.as_str()) {
                    return val.to_string();
                }
            }
            // 2. Try generic parameter names
            for generic in &["arguments", "input", "query", "value"] {
                if let Some(val) = map.get(*generic).and_then(|v| v.as_str()) {
                    return val.to_string();
                }
            }
            // 3. Try any string field in the map
            for val in map.values() {
                if let Some(s) = val.as_str() {
                    return s.to_string();
                }
            }
            String::new()
        }
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

pub const PROMPTS: &[PromptDef] = &[
    PromptDef {
        name: "lk-knowledge-search",
        filename: "lk-knowledge-search.md",
        description: "Search the local knowledge base for existing knowledge",
        arguments: &[PromptArgument {
            name: "query",
            description: "Search query or keywords to search for",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-search.md"),
    },
    PromptDef {
        name: "lk-knowledge-save-context",
        filename: "lk-knowledge-save-context.md",
        description: "Save conversation context to lk knowledge base",
        arguments: &[PromptArgument {
            name: "hint",
            description: "Optional hint about what to save, or empty to auto-extract from conversation",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-save-context.md"),
    },
    PromptDef {
        name: "lk-knowledge-plan",
        filename: "lk-knowledge-plan.md",
        description: "Save plans to tackle later and resume them from a working list",
        arguments: &[PromptArgument {
            name: "mode",
            description: "Mode: empty (auto-route), 'save [hint]', 'list', 'done <id>', or 'drop <id>'",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-plan.md"),
    },
    PromptDef {
        name: "lk-knowledge-discover",
        filename: "lk-knowledge-discover.md",
        description: "Explore the entire project and auto-generate knowledge markdown files for .knowledge/",
        arguments: &[PromptArgument {
            name: "focus",
            description: "Optional focus areas or depth (e.g., 'architecture only', 'focus on API layer', 'deep')",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-discover.md"),
    },
    PromptDef {
        name: "lk-knowledge-refresh",
        filename: "lk-knowledge-refresh.md",
        description: "Check all knowledge entries for staleness and update outdated ones",
        arguments: &[PromptArgument {
            name: "focus",
            description: "Optional focus area (e.g. 'architecture', 'features') or entry IDs to review",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-refresh.md"),
    },
    PromptDef {
        name: "lk-knowledge-add-db",
        filename: "lk-knowledge-add-db.md",
        description: "Add knowledge discovered in this conversation to the local DB",
        arguments: &[PromptArgument {
            name: "description",
            description: "Description of what knowledge to save, or empty to auto-extract from conversation",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-add-db.md"),
    },
    PromptDef {
        name: "lk-knowledge-from-branch",
        filename: "lk-knowledge-from-branch.md",
        description: "Extract knowledge entries from the current branch diff before merging",
        arguments: &[PromptArgument {
            name: "branch",
            description: "Optional base branch to diff against (e.g., 'main', 'develop')",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-from-branch.md"),
    },
    PromptDef {
        name: "lk-knowledge-write-md",
        filename: "lk-knowledge-write-md.md",
        description: "Help write well-structured knowledge markdown files from code or design info",
        arguments: &[PromptArgument {
            name: "target",
            description: "What knowledge to document (e.g., 'login authentication flow')",
            required: false,
        }],
        raw_content: include_str!("../commands/lk-knowledge-write-md.md"),
    },
    PromptDef {
        name: "lk-knowledge-agent-brief",
        filename: "lk-knowledge-agent-brief.md",
        description: "Canonical brief to prepend when delegating code investigation to Explore/general-purpose sub-agents",
        arguments: &[],
        raw_content: include_str!("../commands/lk-knowledge-agent-brief.md"),
    },
    PromptDef {
        name: "lk-knowledge-export",
        filename: "lk-knowledge-export.md",
        description: "Export local knowledge entries to shareable markdown files",
        arguments: &[],
        raw_content: include_str!("../commands/lk-knowledge-export.md"),
    },
    PromptDef {
        name: "lk-knowledge-sync",
        filename: "lk-knowledge-sync.md",
        description: "Sync shared knowledge markdown files into the local DB",
        arguments: &[],
        raw_content: include_str!("../commands/lk-knowledge-sync.md"),
    },
];

pub fn find_prompt(name: &str) -> Option<&'static PromptDef> {
    let normalized = name.trim().to_ascii_lowercase();
    let norm_stripped = normalized
        .strip_prefix("lk-knowledge-")
        .or_else(|| normalized.strip_prefix("lk-"))
        .unwrap_or(&normalized);

    PROMPTS.iter().find(|p| {
        if p.name == normalized {
            return true;
        }
        let p_short = p.name.strip_prefix("lk-knowledge-").unwrap_or(p.name);
        p_short == norm_stripped
    })
}

pub fn prompts_list_response() -> Value {
    let prompts: Vec<Value> = PROMPTS
        .iter()
        .map(|p| {
            let mut val = json!({
                "name": p.name,
                "description": p.description,
            });
            if !p.arguments.is_empty() {
                let args: Vec<Value> = p
                    .arguments
                    .iter()
                    .map(|a| {
                        json!({
                            "name": a.name,
                            "description": a.description,
                            "required": a.required,
                        })
                    })
                    .collect();
                val["arguments"] = Value::Array(args);
            }
            val
        })
        .collect();

    json!({
        "prompts": prompts
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_definitions_count() {
        assert_eq!(PROMPTS.len(), 11);
    }

    #[test]
    fn test_strip_frontmatter() {
        let raw = "---\ndescription: test\nallowed-tools: Bash\n---\n\nHello world";
        assert_eq!(strip_frontmatter(raw), "Hello world");
    }

    #[test]
    fn test_all_prompts_strip_frontmatter_successfully() {
        for p in PROMPTS {
            let body = p.body();
            assert!(
                !body.starts_with("---"),
                "Prompt {} body should not start with frontmatter",
                p.name
            );
            assert!(
                !body.is_empty(),
                "Prompt {} body should not be empty",
                p.name
            );
        }
    }

    #[test]
    fn test_find_prompt_canonical_and_alias() {
        assert!(find_prompt("lk-knowledge-search").is_some());
        assert!(find_prompt("search").is_some());
        assert!(find_prompt("SEARCH").is_some());
        assert!(find_prompt("lk-search").is_some());
        assert!(find_prompt("lk-knowledge-plan").is_some());
        assert!(find_prompt("plan").is_some());
        assert!(find_prompt("unknown").is_none());
    }

    #[test]
    fn test_prompt_render_substitution() {
        let p = find_prompt("search").unwrap();
        let rendered = p.render(Some(&json!({ "query": "my search query" })));
        assert!(rendered.contains("my search query"));
        assert!(!rendered.contains("$ARGUMENTS"));

        let rendered_generic = p.render(Some(&json!({ "arguments": "generic query" })));
        assert!(rendered_generic.contains("generic query"));

        let rendered_empty = p.render(None);
        assert!(!rendered_empty.contains("$ARGUMENTS"));
    }
}
