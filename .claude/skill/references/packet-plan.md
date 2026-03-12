# Packet Plan Schema

Use this plan shape when an agent wants to stage a narrow deep-study pass into the single per-project packet that Copanion resolves for the current repo.

This is for bounded study work only. Do not use it for broad repo exploration.

## Intended Use

- Keep one durable packet per repo under Copanion's system data directory.
- Keep tracked files intentionally small.
- Discover the exact packet path through Copanion itself and write notes and follow-up questions directly into that file.

## Top-Level Fields

```json
{
  "title": "CLI and storage tour",
  "files": [
    "src/cli.rs",
    {
      "path": "src/storage.rs",
      "purpose": "Packet persistence and path handling"
    }
  ],
  "notes": [
    {
      "path": "src/cli.rs",
      "start_line": 41,
      "end_line": 92,
      "kind": "flow",
      "title": "CLI boot sequence",
      "body": "Argument parsing, packet loading, export mode, and TUI dispatch all meet here.",
      "tags": ["entrypoint", "packet"],
      "author": "agent"
    }
  ],
  "questions": [
    {
      "path": "src/cli.rs",
      "start_line": 62,
      "prompt": "Why is the packet written before export mode returns?",
      "why": "The ordering is visible, but the durability reason is not documented."
    }
  ]
}
```

## Packet Target

Apply the plan to the default per-project packet for the current repo.

- Discover or create the packet path by running `copanion --print-packet-path` from the repo root.
- Do not guess the system-data path and do not assume a repo-local `.copanion.toml`.
- Keep tracked file paths repo-relative.
- Reuse existing packet metadata when present instead of inventing a second packet file.

## File Entries

Each entry in `files` may be either:

- a string path like `"src/cli.rs"`
- an object with:
  - `path`
  - optional `label`
  - optional `purpose`

All paths should point inside the repo root. Prefer repo-relative paths.

## Note Entries

Each note object supports:

- `id` optional; when present and already in the packet, that note is updated in place
- `path` required
- `start_line` required
- `end_line` optional
- `kind` optional; one of `overview`, `flow`, `pitfall`, `reference`
- `title` required
- `body` required
- `tags` optional string array
- `author` optional string
- `source` optional; one of `agent`, `human`, `imported`

If `id` is omitted, create a new note id. Agent-authored notes should normally use `source = "agent"`.

## Question Entries

Each question object supports:

- `id` optional; when present and already in the packet, that question is updated in place
- `path` required
- `start_line` optional
- `end_line` optional
- `prompt` required
- `why` optional
- `related_note_ids` optional string array
- `status` optional; one of `open`, `answered`, `archived`

If `id` is omitted, create a new question id.

## Apply Rules

When materializing the plan into the Copanion-discovered packet:

1. Ensure every referenced file is present in `files`.
2. Append new notes and questions or update existing entries by `id`.
3. Preserve `created_at` for existing entries and bump `updated_at` for anything changed.
4. Keep note bodies factual and anchored. Put broad uncertainty into `questions`, not hand-wavy notes.
5. Do not create alternate packet files unless the user explicitly asks.
