# copanion

`copanion` is a Rust CLI/TUI for codebase study sessions.

It opens source files in a terminal UI, injects guidance cards directly below the exact lines they explain, and lets the reader stage notes or follow-up questions in place. Only open questions are exported back to the clipboard or stdout, so notes stay as local understanding aids while questions become the agent-facing follow-up.

The interface stays intentionally simple:

- there is only one user-facing command: `copanion`
- sessions are always stored under the user data directory, usually `~/.local/share/copanion/sessions/`
- a fresh empty session opens directly into the fuzzy file picker instead of failing before the TUI starts
- the saved session path is deterministic, so an agent can update it directly once you share the path

## Installation

### From crates.io

```bash
cargo install copanion
```

### From source

```bash
git clone https://github.com/inmzhang/copanion.git
cd copanion
cargo install --path .
```

## Usage

Run `copanion` in any repository you want to study:

```bash
cd /path/to/your/repo
copanion
```

That opens the default session for the current directory. If it has no tracked files yet, `copanion` opens straight into the fuzzy file picker so you can add them from inside the TUI.

## Options

| Flag | Description |
|------|-------------|
| `--session <NAME>` | Reuse a stable session name instead of the default per-directory session id |
| `--title <TITLE>` | Override the session title when creating or reopening a session |
| `--fresh` | Recreate the session metadata while keeping the tracked file list when no new files are supplied |
| `--print-session-path` | Print the saved session path and exit |
| `--export` | Export open questions without opening the TUI |
| `--stdout` | Print exports to stdout instead of copying them to the clipboard |
| `--theme <THEME>` | Select a built-in theme: `dark`, `light`, `onedark`, `gruvbox-dark`, `gruvbox-light`, `catppuccin-mocha`, `catppuccin-latte`, `ayu-light` |
| `<FILE>...` | Merge one or more source files into the current session before opening the TUI |

Examples:

```bash
copanion --session scheduler-tour src/main.rs src/lib.rs
copanion --session scheduler-tour --print-session-path
copanion --session scheduler-tour --export --stdout
copanion --session scheduler-tour --theme gruvbox-dark
```

## TUI Keys

| Key | Action |
|-----|--------|
| `Tab` | Toggle focus between the file list and source view |
| `j` / `k` | Move in the focused pane |
| `h` / `l` | Switch files from the source pane |
| `[` / `]` | Jump to the previous or next annotated line |
| `a` | Create a `QUESTION` draft at the current line |
| `n` | Create a `NOTE` draft at the current line |
| `i` | Edit the current question, or fall back to the current note |
| `I` | Edit the current note, or fall back to the current question |
| `f` | Open the fuzzy file picker and add a tracked file |
| `/` | Fuzzy-search notes and questions, then jump to the selected match |
| `dd` | Delete the selected tracked file, or the note/question at the current line |
| `Ctrl-o` | Open the current draft in `$VISUAL` or `$EDITOR` |
| `Ctrl-s` | Save the current draft from inside the prompt window |
| `Esc` | Close the current draft, with a save/discard prompt if the contents changed |
| `s` | Save the session |
| `y` | Export open questions without quitting |
| `x` | Save, export open questions, and quit |
| `q` | Quit, with a discard guard if there are unsaved changes |
| `?` | Toggle help |

## Development

Run the local verification bundle:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo publish --dry-run --allow-dirty
```

Install the local git hooks with `pre-commit`:

```bash
pre-commit install --hook-type pre-commit --hook-type pre-push
```
