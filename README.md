# copanion

`copanion` is a Rust CLI/TUI for codebase study sessions.

It opens source files in a terminal UI, injects guidance cards directly below the exact lines they explain, and lets the reader stage follow-up questions that can be copied back to an agent in one structured export.

The interface stays intentionally simple:

- there is only one user-facing command: `copanion`
- sessions are always stored under the user data directory, usually `~/.local/share/copanion/sessions/`
- the saved session file is deterministic, so an agent can update it directly once you share the path

## Workflow

1. Start or reopen a study session for one or more files.
2. Let an agent add notes into the saved session TOML.
3. Open the TUI, read the file with inline note cards, and stage questions on unclear lines.
4. Save and quit to copy the follow-up prompt back to the clipboard, or print it to stdout with `--stdout`.

## Usage

Start a new session or merge files into an existing one:

```bash
copanion --session scheduler-tour src/main.rs src/lib.rs
```

Print the saved session path for an agent:

```bash
copanion --session scheduler-tour src/main.rs src/lib.rs --print-session-path
```

Reopen the saved session later:

```bash
copanion --session scheduler-tour
```

If the session has no tracked files yet, `copanion` now opens straight into the file picker instead of failing before the TUI starts.

Export staged questions without opening the TUI:

```bash
copanion --session scheduler-tour --export --stdout
```

Reset a session while keeping the tracked files if you do not pass new ones:

```bash
copanion --session scheduler-tour --fresh
```

## Session Format

Sessions are stored as TOML under the system data directory. A trimmed example looks like this:

```toml
version = 1
session_id = "scheduler-tour"
title = "Scheduler Tour"
workspace_root = "/home/inm/workspace/bloc"

[[files]]
path = "src/main.rs"

[[notes]]
id = "note-..."
path = "src/main.rs"
kind = "overview"
title = "Entry point"
body = "This is where the CLI hands off to the runtime."
source = "agent"

[notes.anchor]
start_line = 12

[[questions]]
id = "question-..."
path = "src/main.rs"
prompt = "Why does the parser branch return early here?"
status = "open"

[questions.anchor]
start_line = 18
```

An agent does not need a custom integration to add notes right away. It can edit this TOML directly once you hand it the path from `--print-session-path`.

## TUI Keys

- `Tab`: toggle focus between the file list and source view
- `j` / `k`: move in the focused pane
- `h` / `l`: switch files from the source pane
- `[` / `]`: jump between annotated lines
- `f`: open the fuzzy file picker and add a tracked file
- `/`: fuzzy-search notes and open questions, then jump to the match
- `a`: open the question composer at the current line
- `dd`: delete the selected tracked file, or the note/question at the current line
- `r`: reload the tracked source files from disk
- `s`: save the session
- `y`: export open questions without quitting
- `x`: save, export open questions, and quit
- `q`: quit, with a discard guard if there are unsaved changes
- `?`: open help

## Development

Run the full local verification bundle:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo publish --dry-run
```

Install the local git hooks with `pre-commit`:

```bash
pre-commit install --hook-type pre-commit --hook-type pre-push
```

## Release Flow

The GitHub workflows follow the same broad shape as `tuicr`:

- CI runs separate `check`, `fmt`, `clippy`, `test`, and `publish --dry-run` jobs
- the release workflow can open a release PR from a manual dispatch
- merging the release PR tags the version, publishes to crates.io, and creates the GitHub release

Before the publish path can succeed, set `CARGO_REGISTRY_TOKEN` in the GitHub repo secrets.
