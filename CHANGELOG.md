# Changelog

All notable changes to `copanion` will be recorded here.
## 0.1.3 - 2026-03-13
### Fixes
- Navigate inline annotation rows



### Other
- Merge remote-tracking branch 'origin/master'

## 0.1.2 - 2026-03-13
### Chores
- Enforce conventional commits and git-cliff changelog
- Use compilerla conventional pre-commit
- Prepare v0.1.1
- Prepare v0.1.2



### Ci
- Require manual release dispatch
- Auto-tag merged release prs



### Docs
- Inline copanion helper usage



### Fixes
- Expose ordered question turns
- Keep yanked question context focused
- Add readonly thread viewer



### Other
- Revert "ci(release): require manual release dispatch"

This reverts commit 13a44af56113a7772c6f85ea718726818296547f.
- Merge pull request #2 from inmzhang/release/v0.1.1

release: v0.1.1
- Merge pull request #3 from inmzhang/release/v0.1.2

release: v0.1.2

## 0.1.0 - 2026-03-12
### Chores
- Remove obsolete tui stub
- Add repo docs and release automation
- Sync default theme, CI branch, and status badges
- Raise MSRV to Rust 1.88 and refresh deps
- Format sources and refresh demo



### Ci
- Publish releases from version tags



### Docs
- Add VHS demo recording
- Refresh VHS demo with codex handoff
- Refresh README and demo assets
- Add Claude Code copanion setup
- Add repo agent guidance
- Add repository guidelines



### Features
- Bootstrap packet CLI and export flow
- Add inline note viewer
- Add fuzzy session management tools
- Refine editing flow and themes
- Add config file support
- Align themes and syntax highlighting with tuicr
- Add tuicr-style visual range selection
- Add per-project packets and threaded question workflow
- Add vim-style half-page scrolling
- Add git-backed diff review mode
- Refine diff review export UX



### Fixes
- Redraw after external editor exit
- Wrap annotation jumps at list boundaries



### Refactors
- Adopt a single global-session command
- Trim core flows and test overhead
