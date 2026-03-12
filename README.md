# copanion: TUI for Code Study

Study a codebase in your terminal, keep notes on exact lines, and round-trip unresolved questions through coding agents without losing the conversation.

![demo](./demo/copanion-demo.gif)

## Why I built this

When I study an unfamiliar codebase, I struggle with the constant back-and-forth between the source and a separate explanation. A card-style companion for guidance and notes, anchored directly to the code, makes the learning flow much easier to hold onto.

Asking follow-up questions has the same problem. By the time I want help from an agent, I still have to gather the exact source location and package the right context. `copanion` removes that busywork: keep notes beside the code while you read, then export the open questions with their context when you are ready to ask.

## Overview

`copanion` is a Rust CLI/TUI for persistent source-learning packets.

It opens tracked files in a terminal UI, injects note cards directly below the lines they explain, and lets you stage follow-up question threads in place. Notes stay local. Only open threads that still need an agent reply are exported back to the clipboard or stdout.

Each project gets one canonical packet stored under Copanion's user data directory, usually `~/.local/share/copanion/packets/`. `copanion` discovers the project root, maps it to that packet path, and reopens the same packet every time you come back to the project.

If a packet has no tracked files yet, `copanion` opens directly into the fuzzy file picker so you can start from inside the TUI.

## Features

- **Inline notes and questions** - Attach notes or follow-up questions to exact lines or selected ranges.
- **Persistent per-project packets** - Save and reopen the same packet across runs from anywhere inside the project.
- **Fuzzy file picker** - Add tracked files from inside the TUI instead of preparing them up front.
- **Searchable annotations** - Jump through saved notes and questions with a fuzzy search prompt.
- **Question-thread export** - Export only the open threads that still need an agent reply.
- **Agent write-back** - Apply structured agent answers back into the packet so reopened threads show the conversation inline.
- **Clipboard or stdout output** - Copy exports to the clipboard by default, or print them with `--stdout`.
- **Canonical system storage** - Keep packet files in Copanion's data directory instead of scattering them through project trees.
- **Built-in themes** - Pick a theme from the CLI or config file.

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

### Codex Skill

This repo ships one repo-local Codex skill at `.agents/skills/copanion`.

#### Local to this repo

If you run Codex inside this repository, no extra installation step is needed. Codex will discover the repo-local skill automatically from `.agents/skills/copanion`.

If you want the same skill layout in another repository, copy or symlink the folder into that repo:

```bash
mkdir -p /path/to/other-repo/.agents/skills
ln -sfn /path/to/copanion/.agents/skills/copanion /path/to/other-repo/.agents/skills/copanion
```

#### Global install

To make the skill available in every Codex session on your machine, install it under `~/.codex/skills`:

```bash
mkdir -p ~/.codex/skills
ln -sfn "$(pwd)/.agents/skills/copanion" ~/.codex/skills/copanion
```

Restart Codex or open a new Codex session after adding the skill so it can be discovered cleanly.

## Usage

Run `copanion` in the repository you want to study:

```bash
cd /path/to/your/repo
copanion
```

That opens the canonical packet for the current project. If the packet is empty, `copanion` starts in the fuzzy file picker.

### Options

| Flag | Description |
|------|-------------|
| `--title <TITLE>` | Title to use when creating or reopening the canonical project packet. |
| `--fresh` | Recreate the saved packet while preserving tracked files when no new files are supplied. |
| `--print-packet-path` | Print the canonical packet path and exit. |
| `--export` | Export only the open question threads that still need an agent reply. |
| `--stdout` | Print exports to stdout instead of copying them to the clipboard. |
| `--apply-agent-response <PLAN>` | Apply a JSON response plan from a file or `-` for stdin, then exit. |
| `--theme <THEME>` | Built-in UI theme: `dark`, `light`, `one-dark`, `gruvbox-dark`, `gruvbox-light`, `catppuccin-mocha`, `catppuccin-latte`, `ayu-light`. |
| `<FILE>...` | Attach one or more files to the current packet before opening the TUI. |

Examples:

```bash
copanion src/main.rs src/lib.rs
copanion --print-packet-path
copanion --export --stdout
copanion --theme gruvbox-dark
```

## Configuration

Set a default theme in:

- Linux/macOS: `$XDG_CONFIG_HOME/copanion/config.toml` (default: `~/.config/copanion/config.toml`)
- Windows: `%APPDATA%\copanion\config.toml`

Example:

```toml
theme = "gruvbox-dark"
```

Theme resolution precedence:

1. `--theme <THEME>`
2. `theme` in the config file above
3. built-in default (`gruvbox-dark`)

Only `theme` is currently recognized. Unknown keys are ignored with a startup warning.

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `Tab` | Toggle focus between the file list and source view |
| `j` / `k` | Move in the focused pane |
| `h` / `l` | Switch files from the source pane |
| `g` / `G` | Jump to the first or last line |
| `[` / `]` | Jump to the previous or next annotated line |
| `PageUp` / `PageDown` | Scroll by page |
| `?` | Toggle help |

### Notes And Questions

| Key | Action |
|-----|--------|
| `v` / `V` | Start a visual selection for a ranged note or question |
| `a` | Create a `QUESTION` draft at the cursor or selected range |
| `n` | Create a `NOTE` draft at the cursor or selected range |
| `i` | Edit the question under the cursor, or fall back to the note |
| `I` | Edit the note under the cursor, or fall back to the question |
| `/` | Fuzzy-search notes and questions, then jump to the selected match |
| `dd` | Delete the selected tracked file or the annotation under the cursor |
| `Ctrl-o` | Open the current draft in `$VISUAL` or `$EDITOR` |
| `Ctrl-s` | Save the current draft from inside the prompt window |
| `Esc` | Leave visual mode or close the current popup |

### Session Actions

| Key | Action |
|-----|--------|
| `f` | Open the fuzzy file picker and add a tracked file |
| `r` | Resolve the open question thread under the cursor |
| `c` | Continue the open question thread under the cursor |
| `o` | Reopen the resolved question thread under the cursor |
| `R` | Reload tracked file contents from disk |
| `s` | Save the packet |
| `y` | Export open question threads that still need an agent reply without quitting |
| `x` | Save, export unresolved threads, and quit |
| `q` | Quit, with a discard guard if there are unsaved changes |

## Agent Round-Trip

Exported follow-ups now include:

- the canonical packet path
- stable question ids
- the current conversation thread for each question

That lets an agent answer the questions in chat and also write the same answers back into Copanion as structured conversation messages. When you reopen the TUI, you will see the agent reply cards inline under the original question and can either:

- press `c` to continue the same thread with another follow-up
- press `o` to reopen a previously resolved thread
- press `r` to mark the thread resolved so it stops appearing in future exports
