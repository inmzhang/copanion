#!/usr/bin/env python3

from __future__ import annotations

import re
import sys
from pathlib import Path

ALLOWED_TYPES = (
    "build",
    "chore",
    "ci",
    "docs",
    "feat",
    "fix",
    "perf",
    "refactor",
    "revert",
    "style",
    "test",
)

CONVENTIONAL_SUBJECT = re.compile(
    rf"^(?:{'|'.join(ALLOWED_TYPES)})(?:\([A-Za-z0-9._/-]+\))?(?:!)?: .+$"
)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_conventional_commit.py <commit-message-file>", file=sys.stderr)
        return 2

    message_path = Path(sys.argv[1])
    lines = message_path.read_text(encoding="utf-8").splitlines()
    visible_lines = [line for line in lines if not line.startswith("#")]
    subject = next((line.strip() for line in visible_lines if line.strip()), "")

    if not subject:
        print("empty commit message", file=sys.stderr)
        return 1

    if subject.startswith(("Merge ", "Revert ")):
        return 0

    if subject.startswith(("fixup! ", "squash! ")):
        subject = subject.split(" ", 1)[1]

    if CONVENTIONAL_SUBJECT.match(subject):
        return 0

    allowed = ", ".join(ALLOWED_TYPES)
    print("commit subject must use Conventional Commits", file=sys.stderr)
    print(f"allowed types: {allowed}", file=sys.stderr)
    print("expected: type(scope): description", file=sys.stderr)
    print("example: feat(tui): add fuzzy file picker shortcuts", file=sys.stderr)
    print("example: docs: refresh release workflow notes", file=sys.stderr)
    print(f"got: {subject}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
