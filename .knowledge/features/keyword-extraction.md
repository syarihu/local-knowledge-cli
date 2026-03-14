---
keywords: [keywords, extraction, auto-extract, camelcase, snake_case]
category: features
---

# Keyword Auto-Extraction

## Entry: Extraction Strategy
keywords: [extract_keywords, stop-words, katakana]

The `extract_keywords()` function at `src/keywords.rs:20-52` uses 5 strategies: (1) file path extraction from `path/to/file.ext` patterns, (2) CamelCase splitting (e.g., `SessionManager` → "session", "manager"), (3) snake_case splitting, (4) katakana word extraction (4+ chars), and (5) general word extraction. A 46-word English stop word list (`STOP_WORDS` at `keywords.rs:4-16`) filters common words, and only words of 3+ characters are kept.
