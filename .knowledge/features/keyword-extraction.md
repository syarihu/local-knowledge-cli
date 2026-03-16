---
keywords: [keywords, extraction, auto-extract, camelcase, snake_case]
category: features
---

# Keyword Auto-Extraction

## Entry: Extraction Strategy
keywords: [extract_keywords, stop-words, katakana]

The `extract_keywords()` function in `src/keywords.rs` uses 5 strategies: (1) file path extraction from `path/to/file.ext` patterns, (2) CamelCase splitting (e.g., `SessionManager` → "session", "manager"), (3) snake_case splitting, (4) katakana word extraction (4+ chars), and (5) general word extraction. The `STOP_WORDS` constant filters common English words, and only words of 3+ characters are kept.
