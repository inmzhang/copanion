---
name: "copanion"
description: "Use when a user wants durable Copanion updates in the canonical per-project packet under Copanion's system data directory. This includes two bounded workflows: saving line-anchored study notes or follow-up questions for a small explicit file set, and answering exported Copanion question threads by writing agent replies back into the same packet. Do not use for generic repo exploration, broad architecture tours, or implementation work that does not need packet writeback."
---

# Copanion

## Overview

Use this skill when the user wants durable Copanion state, not just a chat answer. The canonical packet under Copanion's system data directory is the source of truth for both study notes and question-thread conversations.

The portable Claude Code bundle lives at `../../skill/`. This repo-local mirror exists so Claude sessions inside this repository do not need to reach into `.agents/skills/`.

## Trigger Gate

Use this skill only when one of these is true:

1. The user wants detailed understanding of a small explicit file set and wants the notes/questions saved into Copanion.
2. The user wants exported Copanion follow-up questions answered and written back into Copanion.

Do not use this skill when the task is:

- generic repo exploration with no bounded file set
- bug fixing or implementation with no packet update requirement
- a broad codebase tour that would explode the tracked file set

## Workflow

### 1. Start from the canonical packet

- From the repo root, run `copanion --print-packet-path` and open the printed path.
- The packet lives under Copanion's system data directory, not inside the repo.
- Reuse the default per-project packet. Do not guess the path and do not create alternate packet files unless the user explicitly asks.
- Copanion exports are intentionally compact. Do not expect the copied follow-up text to restate the full write-back workflow; this skill is the workflow reference.

### 2. Pick the right mode

#### Study mode: add notes or open questions

Use this when the user wants durable learning notes on a small explicit file set.

- Keep the tracked file list intentionally small.
- Read only the target files and the immediate neighbors needed for a correct explanation.
- Plan notes/questions first, then materialize them in one pass.
- Read [packet-plan.md](../../skill/references/packet-plan.md) for the structured shape.
- Prefer the bundled helper from the shared Claude bundle:

```bash
python <skill-directory>/../../skill/scripts/apply_session_plan.py \
  --plan /tmp/copanion-plan.json
```

- Common helper flags:
  - `--plan PATH` required. Pass `-` to read the JSON plan from stdin.
  - `--repo-root PATH` optional. Defaults to the current directory.
  - `--title TEXT` optional. Overrides `plan.title` when both are present.
  - `--fresh` optional. Resets the packet first while preserving tracked files through Copanion.
- Minimal plan shape:

```json
{
  "title": "CLI tour",
  "files": ["src/cli.rs"],
  "notes": [
    {
      "path": "src/cli.rs",
      "start_line": 41,
      "kind": "flow",
      "title": "CLI boot sequence",
      "body": "Argument parsing, packet loading, export mode, and TUI dispatch meet here."
    }
  ],
  "questions": []
}
```

- Use `notes` for durable anchored explanations and `questions` for real unresolved uncertainty that should survive into a later answer pass.
- Reach for `python <skill-directory>/../../skill/scripts/apply_session_plan.py --help` only if the script changed or you need an uncommon path not covered here.
- Prefer `overview` notes for role/intent, `flow` notes for control or data movement, `pitfall` notes for sharp edges, and `reference` notes for schemas or reusable facts.
- Use open questions only for real uncertainty that should survive into a later answer pass.

#### Answer mode: write agent replies back into existing threads

Use this when the user pasted a Copanion export or wants open Copanion questions answered durably.

- Match target questions against the packet before editing; prefer question ids when available.
- Read only the files needed to answer the bounded question set well.
- Write agent answers back as question-thread replies, not just chat output.
- Answer the exported threads in order unless the user explicitly reprioritizes them.
- Prefer the built-in write-back path:

```bash
copanion --apply-agent-response - <<'JSON'
{
  "answers": [
    {
      "question_id": "question-...",
      "answer": "Concrete answer text"
    }
  ],
  "notes": []
}
JSON
```

- Leave answered threads `open` unless the user explicitly asks to resolve them immediately. Reopening Copanion should let the user continue the thread or resolve it from the TUI.
- Use `notes` only for durable line-anchored guidance that should outlive the conversation itself.

### 3. Hand off clearly

At the end, report:

- the packet path
- which files or question ids you updated
- whether any open threads still need an agent reply

## Output Standard

Good output from this skill leaves behind:

- the canonical per-project packet updated through Copanion's own path discovery
- a small, intentional tracked file list
- line-anchored notes/questions for study work, or agent reply cards for answer work
- only genuinely unresolved confusion left as open threads
