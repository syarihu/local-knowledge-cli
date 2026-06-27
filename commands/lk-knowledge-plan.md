---
description: Save plans to tackle later and resume them from a working list
allowed-tools: Bash(lk *)
---

Manage "do it later" plans as first-class knowledge entries: save a plan now, list open plans, resume one, and close it when done. Plans use `category: plan` with `status` as the lifecycle: `proposed` = open (not started), `accepted` = done, `deprecated` = dropped.

## Arguments
$ARGUMENTS selects the mode:
- empty → **list** open plans and let the user pick one to resume
- `save [hint]` → **save** the current conversation/decision as a new plan
- `done <id-or-uid>` → **close** a plan (mark it done)
- `drop <id-or-uid>` → **abandon** a plan

## Procedure

### List (no arguments)
1. Run `lk list --category plan --status proposed --json` (reads merge project + user, so plans from any scope show up).
2. Present the open plans as a numbered list (title + id/uid + scope).
3. When the user picks one, run `lk get <id-or-uid> --json` and read the full content to resume the work. If the content references a plan file path (e.g. `~/.claude/plans/<name>.md`), read that too — but the entry itself should be self-contained.

### Save (`save [hint]`)
1. Review the conversation for the plan worth deferring: the decision reached, the approach, concrete identifiers (files/functions), rejected alternatives, and any dead-ends. Write it **dense** — enough to resume cold weeks later without the conversation.
2. Choose the scope: personal/cross-project work lists → `--scope user`; plans tied to this repo → default (`auto`).
3. Run:
   `lk add "<title>" --category plan --status proposed --keywords "plan,<kw1>,<kw2>" --content "<dense content>"`
4. Report the id/uid so it can be found later.

### Done (`done <id-or-uid>`)
- Run `lk edit <id-or-uid> --status accepted`. Keep the entry (don't delete) so it stays as a record of what was done.

### Drop (`drop <id-or-uid>`)
- Run `lk edit <id-or-uid> --status deprecated`.

## Guidelines
- category must be `plan` (distinct from `decisions`/ADR and `context`).
- Lifecycle: `proposed` (open) → `accepted` (done) / `deprecated` (dropped). Only `proposed` plans appear in the default list.
- Content must be self-contained and high-density — the same standard as `/lk-knowledge-save-context`: what/why, rejected options, dead-ends, concrete identifiers.
- Address user-scope plans by uid (numeric ids are project-only).
