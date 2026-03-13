# Repository Guidelines

## Project Structure & Module Organization

`copanion` is a single-command Rust CLI/TUI. Core code lives in `src/`: `main.rs` boots the app, `lib.rs` re-exports shared modules, `cli.rs` owns flag parsing and packet startup, `storage.rs` handles per-project packet persistence, `diff.rs` loads Git-backed review views, and `src/tui/` contains the interactive app state and rendering. Keep demo assets in `demo/`, CI definitions in `.github/workflows/`, and agent-skill assets in `.agents/` and `.claude/` when those flows change.

## Build, Test, and Development Commands

- `cargo run -- --help`: verify the live CLI surface before changing docs.
- `cargo run -- --diff`: smoke-test diff mode locally.
- `cargo fmt --all -- --check`: enforce formatting.
- `cargo clippy --all-targets --all-features -- -D warnings`: treat warnings as errors, matching CI and pre-push hooks.
- `cargo test --all-features`: run the full unit-test suite.
- `cargo publish --dry-run`: optional release sanity check used in CI.
- `python3 scripts/generate_changelog.py --output CHANGELOG.md`: regenerate the changelog from conventional-commit history.
- `pre-commit install --hook-type pre-commit --hook-type pre-push --hook-type commit-msg`: install the local formatting, test, and conventional-commit hooks.

## Coding Style & Naming Conventions

Target Rust `1.88+` and edition `2024` as declared in `Cargo.toml`. Let `rustfmt` decide layout; do not hand-format around it. Use `snake_case` for modules, functions, and test names, `CamelCase` for types, and keep small helper logic near the owning module instead of creating shallow files. Preserve the current single-command CLI shape and stable repo-relative packet anchors.

## Testing Guidelines

Tests are inline under `#[cfg(test)]` beside the code they verify; there is no separate `tests/` tree today. Add focused unit tests for packet persistence, export/write-back behavior, diff selection, and TUI state transitions. Prefer behavior checks over brittle rendering snapshots. Run `cargo test --all-features` before opening a PR, and rerun `cargo run -- --help` when flags, defaults, or help text change.

## Commit & Pull Request Guidelines

Use Conventional Commits for all new history. The required subject format is `type(scope): description` or `type: description`, with concise present-tense wording and an optional narrow scope such as `tui`, `diff`, `skills`, or `release`. Preferred types are `feat`, `fix`, `refactor`, `docs`, `ci`, `chore`, `test`, `build`, `perf`, `style`, and `revert`.

`git-cliff` release notes and `CHANGELOG.md` generation assume conventional commit subjects, so do not fall back to free-form imperative messages.

PRs should explain user-visible behavior changes, list validation commands run, and call out any README, demo GIF/tape, or agent-skill updates. Include screenshots or terminal captures for material TUI changes.
