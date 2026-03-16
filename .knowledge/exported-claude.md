---
keywords: [claude, code, commands, embedded, fire, forget, include, include_str, install, install-commands, lk_search_log, log, logging, main, search, search-log, section, slash-command, インストール, クエリ・, コマンド, スラッシュコマンド, ディレクトリ, デフォルト, ファイル, ・ヒットタイトル]
category: exported
---

# Exported: claude

## Entry: Embedded Commandsへの新コマンド追加方法
keywords: [claude, code, commands, embedded, include, include_str, install, install-commands, main, section, slash-command, インストール, コマンド, スラッシュコマンド, ディレクトリ, ファイル]

新しいClaude Codeスラッシュコマンドを追加するには、(1) commands/ ディレクトリに `lk-knowledge-*.md` 形式のmdファイルを作成、(2) `src/cmd/update.rs` の `EMBEDDED_COMMANDS` 定数に `include_str!` で追加する。ビルド後 `lk install-commands` で `~/.claude/commands/` にインストールされる。

## Entry: Search Logging機能
keywords: [claude, fire, forget, lk_search_log, log, logging, main, search, search-log, クエリ・, コマンド, デフォルト, ・ヒットタイトル]

lkコマンド実行時に `.knowledge/command.log` へコマンド名・メタ情報を記録する機能。デフォルトは無効で、`.knowledge/config.toml` の `command_log = true` または環境変数 `LK_COMMAND_LOG=1` で有効化する。`log_command()` 関数（`src/cmd/mod.rs`）はfire-and-forget方式で、ログ書き込み失敗がコマンド結果に影響しない。`lk search-log` コマンドで直近のログを確認できる（デフォルト20件、-n で変更可）。
