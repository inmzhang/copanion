#!/usr/bin/env python3

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate CHANGELOG.md using git-cliff and normalize trailing newlines.",
    )
    parser.add_argument("--tag", help="Optional tag name for the generated release section.")
    parser.add_argument(
        "--output",
        default="CHANGELOG.md",
        help="Output path for the generated changelog. Defaults to CHANGELOG.md.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    command = ["git-cliff", "--output", "-"]
    if args.tag:
        command.extend(["--tag", args.tag])

    result = subprocess.run(command, check=True, capture_output=True, text=True)
    content = result.stdout.rstrip("\n") + "\n"
    Path(args.output).write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
