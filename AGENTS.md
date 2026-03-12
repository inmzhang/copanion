# Copanion Agents

## Project Shape

- `copanion` is a single-command Rust CLI/TUI for studying code and reviewing diffs with durable inline notes and question threads.
- Canonical per-project packets live under the user data directory, typically `~/.local/share/copanion/packets/` on Linux.
- `legacy_default_session_path(...)` is migration-only support for older `sessions/` storage. New writes should target `packets/`.

## Working Rules

- Preserve the single-command CLI shape. Do not introduce a subcommand tree unless the user asks for it explicitly.
- Keep packet anchors durable and repo-relative. Changes that make saved file paths or line-anchored annotations less stable are regressions.
- Notes stay local; exported/copied output should stay focused on open threads that are actually waiting for an agent reply.
- In diff mode, historical context must come from the selected revision, not the live worktree.

## Validation

Run these after meaningful code changes:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features --quiet
```

For CLI/doc checks, prefer the live binary surface:

```bash
cargo run -- --help
```

## Code Map

- `src/cli.rs`: CLI entrypoint, theme resolution, packet loading/building.
- `src/storage.rs`: project discovery and packet persistence.
- `src/diff.rs`: Git-backed diff loading and hidden-context expansion.
- `src/answer_plan.rs`: structured agent write-back validation and apply path.
- `src/tui/app.rs`: TUI state machine and mutation flows.
- `src/tui/render.rs`: TUI rendering and layout.

## Release Notes

- Keep README/help text aligned with the live CLI.
- Prefer targeted tests for packet persistence, diff correctness, and write-back validation over brittle UI-detail assertions.
