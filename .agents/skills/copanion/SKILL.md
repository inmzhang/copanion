---
name: "copanion"
description: "Use when a user wants durable Copanion updates in the canonical per-project packet under Copanion's system data directory for code study or change review. This includes two bounded workflows: saving line-anchored study notes or follow-up question/comment threads for a small explicit file set or diff review, and answering exported Copanion question/comment threads by writing agent replies back into the same packet. Do not use for generic repo exploration, broad architecture tours, or implementation work that does not need packet writeback."
---

# Copanion

## Overview

Use this skill when the user wants durable Copanion state for code study or change review, not just a chat answer. The canonical packet under Copanion's system data directory is the source of truth for study notes, source-mode question threads, and diff-review comment threads.

## Trigger Gate

Use this skill only when one of these is true:

1. The user wants detailed understanding of a small explicit file set, or a bounded diff review, and wants the notes/questions/comments saved into Copanion.
2. The user wants exported Copanion follow-up questions or diff-review comment threads answered and written back into Copanion.

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

#### Study mode: add notes or open threads

Use this when the user wants durable learning notes on a small explicit file set or a bounded diff review.

- Keep the tracked file list intentionally small.
- Read only the target files and the immediate neighbors needed for a correct explanation.
- Plan notes/questions first, then materialize them in one pass.
- Read [packet-plan.md](./references/packet-plan.md) for the structured shape.
- Prefer the bundled helper:

```bash
python <skill-directory>/scripts/apply_session_plan.py \
  --plan /tmp/copanion-plan.json
```

- Prefer `overview` notes for role/intent, `flow` notes for control or data movement, `pitfall` notes for sharp edges, and `reference` notes for schemas or reusable facts.
- In source mode, use open questions only for real uncertainty that should survive into a later answer pass.
- In diff mode, use comment threads for review comments and follow-up discussion; the packet model is the same even though the TUI wording changes.

#### Answer mode: write agent replies back into existing threads

Use this when the user pasted a Copanion export or wants open Copanion questions or diff-review comment threads answered durably.

- Match target questions/comments against the packet before editing; prefer question ids when available.
- Read only the files needed to answer the bounded thread set well.
- Write agent answers back as thread replies, not just chat output.
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

- Leave answered threads `open` unless the user explicitly asks to close them immediately. Reopening Copanion should let the user continue or close the thread from the TUI.
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
