# copanion: TUI for Code Study

Study a codebase in your terminal, keep notes on exact lines, and keep coding agents inside the same learning loop instead of bouncing between disconnected chats and scratch notes.

![demo](./demo/copanion-demo.gif)

## Why I built this

When I study an unfamiliar codebase, I struggle with the constant back-and-forth between the source and a separate explanation. A card-style companion for guidance and notes, anchored directly to the code, makes the learning flow much easier to hold onto.

Asking follow-up questions has the same problem. By the time I want help from an agent, I still have to gather the exact source location and package the right context, and once the agent replies the answer usually lives somewhere else. `copanion` removes that busywork: keep notes beside the code while you read, export the exact question threads that still need help, write the agent replies back into the same packet, then continue or close the thread inside the TUI.

## Overview

`copanion` is a Rust CLI/TUI for persistent source-learning packets with an explicit agent loop.

It opens tracked files in a terminal UI, injects note cards directly below the lines they explain, and lets you stage follow-up question threads in place. Notes stay local. Only open threads that still need an agent reply are exported back to the clipboard or stdout.

Each project gets one canonical packet stored under Copanion's user data directory, usually `~/.local/share/copanion/packets/`. `copanion` discovers the project root, maps it to that packet path, and reopens the same packet every time you come back to the project.

If a packet has no tracked files yet, `copanion` opens directly into the fuzzy file picker so you can start from inside the TUI.

## Learning Loop

`copanion` is built around a repeatable loop with agents in the middle, not outside the tool:

1. Read code in the TUI and attach notes or open questions to exact lines.
2. Export only the question threads that still need an agent reply.
3. Ask an agent to answer those threads and write the answers back into the same packet.
4. Reopen Copanion and keep reading with the full thread visible inline beside the source.
5. Continue the thread, close it, or reopen it later if new confusion appears.

The important point is that the thread stays durable. The root question, the agent answer, and later follow-up questions all live in one place and remain attached to the same code anchor.

## Features

- **Inline notes and question threads** - Attach notes or threaded follow-up questions to exact lines or selected ranges.
- **Persistent per-project packets** - Save and reopen the same packet across runs from anywhere inside the project.
- **Fuzzy file picker** - Add tracked files from inside the TUI instead of preparing them up front.
- **Searchable annotations** - Jump through saved notes and threads with a fuzzy search prompt.
- **Question-thread export** - Export only the open threads that still need an agent reply.
- **Agent write-back** - Apply structured agent answers back into the packet so reopened threads show the conversation inline.
- **Thread lifecycle control** - Continue, close, or reopen a question thread from inside the TUI.
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

### Agent Skills

This repo ships the same Copanion workflow for both Codex and Claude Code.

#### Codex

To install the skill for Codex:

```bash
cp -r "$(pwd)/.agents/skills/copanion" ~/.codex/skills/copanion
```

#### Claude Code

To install the skill for Claude Code:

```bash
# Option 1:
cp -r "$(pwd)/.claude/skill" ~/.claude/skills/copanion

# Option 2:
claude skill add "$(pwd)/.claude/skill"
```

## Usage

Run `copanion` in the repository you want to study:

```bash
cd /path/to/your/repo
copanion
```

That opens the canonical packet for the current project. If the packet is empty, `copanion` starts in the fuzzy file picker.

To browse recent commits or uncommitted changes instead of the tracked-source view, start in diff mode:

```bash
copanion --diff
```

Diff mode is Git-backed. It opens a commit selector first, lets you choose either the working tree or a contiguous commit range, and then shows a unified patch view with the same Copanion packet loaded beside note/comment counts.

### Options

| Flag | Description |
|------|-------------|
| `--title <TITLE>` | Title to use when creating or reopening the canonical project packet. |
| `--fresh` | Recreate the saved packet while preserving tracked files when no new files are supplied. |
| `--print-packet-path` | Print the canonical packet path and exit. |
| `--export` | Export only the open question threads that still need an agent reply. |
| `--diff` | Open directly in Git diff mode with uncommitted-change and commit-range selection. |
| `--stdout` | Print exports to stdout instead of copying them to the clipboard. |
| `--apply-agent-response <PLAN>` | Apply a JSON response plan from a file or `-` for stdin, then exit. |
| `--theme <THEME>` | Built-in UI theme: `dark`, `light`, `one-dark`, `gruvbox-dark`, `gruvbox-light`, `catppuccin-mocha`, `catppuccin-latte`, `ayu-light`. |
| `<FILE>...` | Attach one or more files to the current packet before opening the TUI. |

Examples:

```bash
copanion src/main.rs src/lib.rs
copanion --print-packet-path
copanion --diff
copanion --export --stdout
copanion --theme gruvbox-dark
```

Typical agent-assisted flow:

```bash
cd /path/to/your/repo
copanion
# read code, add notes, stage question threads
copanion --export --stdout
# ask an agent to answer the exported threads
copanion --apply-agent-response replies.json
# reopen the TUI and continue or close the thread
copanion
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

### Shared Keys

These work in both source mode and diff mode. The nouns change by mode: source mode works with question threads, while diff mode treats the same thread keys as review comments.

| Key | Action |
|-----|--------|
| `Tab` | Toggle focus between the sidebar and the main pane |
| `j` / `k` | Move in the focused pane |
| `h` / `l` | Switch files from the main pane |
| `g` / `G` | Jump to the first or last row |
| `[` / `]` | Jump to the previous or next note/thread |
| `PageUp` / `PageDown` | Scroll by page |
| `Ctrl-u` / `Ctrl-d` | Scroll by half a page |
| `v` / `V` | Start a visual selection on anchorable rows |
| `a` | Start a new thread, or reply to the open thread under the cursor |
| `c` | Close the open thread under the cursor |
| `o` | Reopen the closed thread under the cursor |
| `n` | Create a `NOTE` draft at the cursor or selected range |
| `i` | Edit the thread under the cursor, or fall back to the note |
| `I` | Edit the note under the cursor, or fall back to the thread |
| `/` | Fuzzy-search notes and threads, then jump to the selected match |
| `dd` | Delete the annotation under the cursor |
| `Ctrl-o` | Open the current draft in `$VISUAL` or `$EDITOR` |
| `Ctrl-s` | Save the current draft from inside the prompt window |
| `s` | Save the packet |
| `q` | Quit, with a discard guard if there are unsaved changes |
| `Esc` | Leave visual mode or close the current popup |
| `?` | Toggle help |

### Source Mode Differences

Source mode uses the tracked-file browser and question-thread wording.

| Key | Action |
|-----|--------|
| `f` | Open the fuzzy file picker and add a tracked file |
| `R` | Reload tracked file contents from disk |
| `dd` | In the file list, remove the selected tracked file from the packet |
| `y` | Export open question threads that still need an agent reply without quitting |
| `x` | Save, export open question threads, and quit |

### Diff Mode Differences

Start diff mode with `copanion --diff`. In this mode, `a` creates a review comment or reply, `c` closes a comment thread, and exports include the selected commits or working-tree scope plus your review comments.

| Key | Action |
|-----|--------|
| `Space` | In the commit selector, toggle or extend the contiguous selection range; in the diff view, expand or collapse hidden context |
| `Enter` | In the commit selector, open the selected working-tree or commit-range diff; in the diff view, expand or collapse hidden context |
| `m` | Reopen the commit selector from the diff view |
| `r` | Mark the current diff file reviewed and jump to the next file |
| `R` | Reload the active diff selection |
| `y` | Copy a diff-review export with the selected commits and your review comments |
| `x` | Save, export the diff review, and quit |

Review comments and notes in diff mode attach to added lines, unchanged context lines, or expanded hidden context. Deleted-only rows do not have a durable source anchor, so they are not directly annotatable yet.
