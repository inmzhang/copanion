#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    import tomlkit
except ModuleNotFoundError as exc:
    raise SystemExit("tomlkit is required to edit copanion packets") from exc


NOTE_KINDS = {"overview", "flow", "pitfall", "reference"}
NOTE_SOURCES = {"agent", "human", "imported"}
QUESTION_STATUSES = {"open", "answered", "archived"}


@dataclass
class ApplyCounts:
    created_notes: int = 0
    updated_notes: int = 0
    created_questions: int = 0
    updated_questions: int = 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Apply a JSON note/question plan to the canonical copanion packet for a repo.",
    )
    parser.add_argument(
        "--plan",
        required=True,
        help="Path to the JSON plan file, or '-' to read the plan from stdin.",
    )
    parser.add_argument(
        "--repo-root",
        default=".",
        help="Repo root where copanion should be run. Defaults to the current directory.",
    )
    parser.add_argument(
        "--title",
        help="Optional packet title override. Falls back to plan.title when present.",
    )
    parser.add_argument(
        "--fresh",
        action="store_true",
        help="Reset the packet first while preserving tracked files through copanion.",
    )
    return parser.parse_args()


def read_plan(plan_arg: str) -> dict[str, Any]:
    if plan_arg == "-":
        raw = sys.stdin.read()
    else:
        raw = Path(plan_arg).read_text(encoding="utf-8")
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise ValueError("plan root must be a JSON object")
    return data


def iso_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def normalize_repo_path(raw_path: str, repo_root: Path) -> str:
    path = Path(raw_path)
    if path.is_absolute():
        try:
            relative = path.resolve().relative_to(repo_root.resolve())
        except ValueError as exc:
            raise ValueError(f"path must stay inside the repo root: {raw_path}") from exc
    else:
        relative = path

    normalized = relative.as_posix()
    if not normalized or normalized == ".":
        raise ValueError(f"invalid repo path: {raw_path}")

    target = repo_root / relative
    if not target.exists():
        raise ValueError(f"path does not exist in repo: {normalized}")

    return normalized


def validate_anchor(start_line: int | None, end_line: int | None) -> dict[str, int] | None:
    if start_line is None:
        if end_line is not None:
            raise ValueError("end_line requires start_line")
        return None
    if start_line < 1:
        raise ValueError("start_line must be >= 1")
    if end_line is not None and end_line < start_line:
        raise ValueError("end_line must be >= start_line")
    anchor = {"start_line": start_line}
    if end_line is not None:
        anchor["end_line"] = end_line
    return anchor


def ensure_array_of_tables(packet: Any, key: str) -> Any:
    if key not in packet:
        packet[key] = tomlkit.aot()
        return packet[key]

    value = packet[key]
    if isinstance(value, tomlkit.items.Array):
        if len(value) != 0:
            raise ValueError(f"{key} must be empty or an array-of-tables")
        packet[key] = tomlkit.aot()
        return packet[key]

    return value


def update_file_entry(existing: Any, payload: dict[str, Any]) -> None:
    if payload.get("label") is not None:
        existing["label"] = payload["label"]
    if payload.get("purpose") is not None:
        existing["purpose"] = payload["purpose"]


def ensure_file(packet: Any, payload: dict[str, Any]) -> None:
    files = ensure_array_of_tables(packet, "files")
    for existing in files:
        if existing.get("path") == payload["path"]:
            update_file_entry(existing, payload)
            return

    entry = tomlkit.table()
    entry["path"] = payload["path"]
    if payload.get("label") is not None:
        entry["label"] = payload["label"]
    if payload.get("purpose") is not None:
        entry["purpose"] = payload["purpose"]
    files.append(entry)


def ensure_tracked_files(packet: Any, repo_root: Path, plan: dict[str, Any]) -> None:
    file_payloads: list[dict[str, Any]] = []

    for raw_file in plan.get("files", []):
        if isinstance(raw_file, str):
            file_payloads.append({"path": normalize_repo_path(raw_file, repo_root)})
        elif isinstance(raw_file, dict):
            if "path" not in raw_file or not isinstance(raw_file["path"], str):
                raise ValueError("file objects require a string path")
            file_payloads.append(
                {
                    "path": normalize_repo_path(raw_file["path"], repo_root),
                    "label": raw_file.get("label"),
                    "purpose": raw_file.get("purpose"),
                }
            )
        else:
            raise ValueError("files entries must be strings or objects")

    for note in plan.get("notes", []):
        if not isinstance(note, dict):
            raise ValueError("notes entries must be objects")
        file_payloads.append({"path": normalize_repo_path(note["path"], repo_root)})

    for question in plan.get("questions", []):
        if not isinstance(question, dict):
            raise ValueError("questions entries must be objects")
        file_payloads.append({"path": normalize_repo_path(question["path"], repo_root)})

    seen: set[str] = set()
    for payload in file_payloads:
        path = payload["path"]
        if path in seen:
            continue
        seen.add(path)
        ensure_file(packet, payload)


def build_note_entry(payload: dict[str, Any], repo_root: Path, now: str) -> dict[str, Any]:
    path = normalize_repo_path(payload["path"], repo_root)
    kind = payload.get("kind", "overview")
    if kind not in NOTE_KINDS:
        raise ValueError(f"invalid note kind: {kind}")
    source = payload.get("source", "agent")
    if source not in NOTE_SOURCES:
        raise ValueError(f"invalid note source: {source}")
    anchor = validate_anchor(payload.get("start_line"), payload.get("end_line"))
    if anchor is None:
        raise ValueError("notes require start_line")
    title = payload.get("title")
    body = payload.get("body")
    if not isinstance(title, str) or not title.strip():
        raise ValueError("notes require a non-empty title")
    if not isinstance(body, str) or not body.strip():
        raise ValueError("notes require a non-empty body")
    tags = payload.get("tags", [])
    if not isinstance(tags, list) or not all(isinstance(tag, str) for tag in tags):
        raise ValueError("note tags must be a string array")

    entry = {
        "id": payload.get("id") or f"note-{uuid.uuid4().hex}",
        "path": path,
        "anchor": anchor,
        "kind": kind,
        "title": title,
        "body": body,
        "tags": tags,
        "source": source,
        "created_at": payload.get("created_at") or now,
        "updated_at": now,
    }
    if payload.get("author") is not None:
        entry["author"] = payload["author"]
    return entry


def build_question_entry(payload: dict[str, Any], repo_root: Path, now: str) -> dict[str, Any]:
    path = normalize_repo_path(payload["path"], repo_root)
    status = payload.get("status", "open")
    if status not in QUESTION_STATUSES:
        raise ValueError(f"invalid question status: {status}")
    anchor = validate_anchor(payload.get("start_line"), payload.get("end_line"))
    prompt = payload.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        raise ValueError("questions require a non-empty prompt")
    related = payload.get("related_note_ids", [])
    if not isinstance(related, list) or not all(isinstance(item, str) for item in related):
        raise ValueError("related_note_ids must be a string array")

    entry = {
        "id": payload.get("id") or f"question-{uuid.uuid4().hex}",
        "path": path,
        "prompt": prompt,
        "related_note_ids": related,
        "status": status,
        "created_at": payload.get("created_at") or now,
        "updated_at": now,
    }
    if anchor is not None:
        entry["anchor"] = anchor
    if payload.get("why") is not None:
        entry["why"] = payload["why"]
    return entry


def write_entry(target: Any, entry: dict[str, Any]) -> None:
    target["id"] = entry["id"]
    target["path"] = entry["path"]
    if "anchor" in entry:
        anchor_table = tomlkit.table()
        anchor_table["start_line"] = entry["anchor"]["start_line"]
        if "end_line" in entry["anchor"]:
            anchor_table["end_line"] = entry["anchor"]["end_line"]
        target["anchor"] = anchor_table
    elif "anchor" in target:
        del target["anchor"]

    for key in (
        "kind",
        "title",
        "body",
        "tags",
        "author",
        "source",
        "prompt",
        "why",
        "related_note_ids",
        "status",
        "created_at",
        "updated_at",
    ):
        if key in entry:
            target[key] = entry[key]
        elif key in target:
            del target[key]


def apply_notes(packet: Any, repo_root: Path, plan: dict[str, Any], now: str, counts: ApplyCounts) -> None:
    notes = ensure_array_of_tables(packet, "notes")
    index = {note.get("id"): note for note in notes if note.get("id")}

    for raw in plan.get("notes", []):
        entry = build_note_entry(raw, repo_root, now)
        existing = index.get(entry["id"])
        if existing is None:
            target = tomlkit.table()
            write_entry(target, entry)
            notes.append(target)
            counts.created_notes += 1
        else:
            write_entry(existing, entry)
            counts.updated_notes += 1


def apply_questions(
    packet: Any, repo_root: Path, plan: dict[str, Any], now: str, counts: ApplyCounts
) -> None:
    questions = ensure_array_of_tables(packet, "questions")
    index = {question.get("id"): question for question in questions if question.get("id")}

    for raw in plan.get("questions", []):
        entry = build_question_entry(raw, repo_root, now)
        existing = index.get(entry["id"])
        if existing is None:
            target = tomlkit.table()
            write_entry(target, entry)
            questions.append(target)
            counts.created_questions += 1
        else:
            write_entry(existing, entry)
            counts.updated_questions += 1


def packet_path_from_copanion(
    repo_root: Path,
    title: str | None,
    fresh: bool,
    plan: dict[str, Any],
) -> Path:
    cmd = ["copanion"]

    effective_title = title or plan.get("title")
    if effective_title is not None and not isinstance(effective_title, str):
        raise ValueError("title must be a string")

    file_args: list[str] = []
    for raw_file in plan.get("files", []):
        if isinstance(raw_file, str):
            file_args.append(raw_file)
        elif isinstance(raw_file, dict) and isinstance(raw_file.get("path"), str):
            file_args.append(raw_file["path"])
        else:
            raise ValueError("files entries must be strings or objects with path")

    if effective_title:
        cmd.extend(["--title", effective_title])
    if fresh:
        cmd.append("--fresh")
    cmd.extend(file_args)
    cmd.append("--print-packet-path")

    completed = subprocess.run(
        cmd,
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        stderr = completed.stderr.strip()
        raise RuntimeError(f"copanion command failed: {stderr or completed.stdout.strip()}")

    packet_path = completed.stdout.strip()
    if not packet_path:
        raise RuntimeError("copanion did not print a packet path")
    return Path(packet_path)


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    if not repo_root.is_dir():
        raise SystemExit(f"repo root is not a directory: {repo_root}")

    plan = read_plan(args.plan)
    packet_path = packet_path_from_copanion(
        repo_root=repo_root,
        title=args.title,
        fresh=args.fresh,
        plan=plan,
    )

    packet = tomlkit.parse(packet_path.read_text(encoding="utf-8"))
    now = iso_now()
    counts = ApplyCounts()

    ensure_tracked_files(packet, repo_root, plan)
    apply_notes(packet, repo_root, plan, now, counts)
    apply_questions(packet, repo_root, plan, now, counts)
    packet["updated_at"] = now

    packet_path.write_text(tomlkit.dumps(packet), encoding="utf-8")

    tracked_files = len(packet.get("files", []))
    print(f"Updated packet: {packet_path}")
    print(
        "Notes: "
        f"+{counts.created_notes} new, {counts.updated_notes} updated | "
        "Questions: "
        f"+{counts.created_questions} new, {counts.updated_questions} updated | "
        f"Tracked files: {tracked_files}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
