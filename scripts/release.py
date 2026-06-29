#!/usr/bin/env python3
"""Repo-level release automation for Cachebox.

The Rust workspace has multiple public packages, but Cachebox releases are
repo-level releases. This script deliberately reads Conventional Commit
messages across the whole repository instead of asking Cargo which package
changed.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO_URL = "https://github.com/smarzola/cachebox"
VERSION_FILES = [
    ROOT / "Cargo.toml",
    ROOT / "crates/cachebox-protocol/Cargo.toml",
    ROOT / "clients/rust/cachebox-client/Cargo.toml",
    ROOT / "clients/python/pyproject.toml",
]
PYTHON_VERSION_FILES = [
    ROOT / "clients/python/python/cachebox/__init__.py",
    ROOT / "clients/python/tests/test_package.py",
]
CONVENTIONAL_RE = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]+)\))?(?P<breaking>!)?: (?P<description>.+)$"
)
SEMVER_TAG_RE = re.compile(r"^v(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)$")
RELEASE_TYPES = {"feat": "minor", "fix": "patch", "perf": "patch"}
GROUP_TITLES = {
    "breaking": "Breaking Changes",
    "feat": "Added",
    "fix": "Fixed",
    "perf": "Performance",
}


def run_git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def latest_repo_tag() -> str:
    tags = run_git("tag", "--list", "v[0-9]*.[0-9]*.[0-9]*", "--sort=-v:refname").splitlines()
    for tag in tags:
        if SEMVER_TAG_RE.match(tag):
            return tag
    return "v0.0.0"


def parse_version(tag: str) -> tuple[int, int, int]:
    match = SEMVER_TAG_RE.match(tag)
    if not match:
        raise ValueError(f"invalid SemVer tag: {tag}")
    return int(match["major"]), int(match["minor"]), int(match["patch"])


def bump_version(tag: str, bump: str) -> str:
    major, minor, patch = parse_version(tag)
    if bump == "major":
        major += 1
        minor = 0
        patch = 0
    elif bump == "minor":
        minor += 1
        patch = 0
    elif bump == "patch":
        patch += 1
    else:
        raise ValueError(f"unknown bump: {bump}")
    return f"{major}.{minor}.{patch}"


def commit_records(tag: str) -> list[dict[str, str]]:
    if tag == "v0.0.0":
        revision_range = "HEAD"
    else:
        revision_range = f"{tag}..HEAD"
    output = run_git("log", "--format=%H%x00%s%x00%b%x1e", revision_range)
    records: list[dict[str, str]] = []
    for raw in output.split("\x1e"):
        raw = raw.strip()
        if not raw:
            continue
        commit_hash, subject, body = raw.split("\x00", 2)
        records.append({"hash": commit_hash, "subject": subject, "body": body})
    return records


def conventional_line(record: dict[str, str]) -> tuple[re.Match[str], str] | None:
    lines = [record["subject"], *record["body"].splitlines()]
    for line in lines:
        line = line.strip()
        if not line or line.startswith("Merge pull request "):
            continue
        match = CONVENTIONAL_RE.match(line)
        if match:
            return match, line
    return None


def analyze_commits(tag: str) -> tuple[str | None, dict[str, list[str]]]:
    bump: str | None = None
    groups: dict[str, list[str]] = {"breaking": [], "feat": [], "fix": [], "perf": []}
    seen_lines: set[tuple[str, str]] = set()

    for record in commit_records(tag):
        found = conventional_line(record)
        if not found:
            continue
        match, line = found
        commit_type = match["type"]
        description = match["description"]
        short_hash = record["hash"][:7]
        breaking = bool(match["breaking"]) or "BREAKING CHANGE:" in record["body"]

        if breaking:
            bump = "major"
            group = "breaking"
        elif commit_type in RELEASE_TYPES:
            next_bump = RELEASE_TYPES[commit_type]
            if bump != "major" and (bump != "minor" or next_bump == "minor"):
                bump = next_bump
            group = commit_type
        else:
            continue

        scope = match["scope"]
        note = f"{scope}: {description}" if scope else description
        entry = f"- {note} ({short_hash})"
        key = (group, line)
        if key not in seen_lines:
            groups[group].append(entry)
            seen_lines.add(key)

    return bump, groups


def replace_first_package_version(text: str, version: str) -> str:
    return re.sub(r'(?m)^version = "[^"]+"$', f'version = "{version}"', text, count=1)


def update_versions(version: str) -> None:
    for path in VERSION_FILES:
        text = path.read_text()
        text = replace_first_package_version(text, version)
        text = re.sub(
            r'(cachebox(?:-protocol|-client)? = \{ version = ")[^"]+(")',
            rf"\g<1>{version}\2",
            text,
        )
        path.write_text(text)

    for path in PYTHON_VERSION_FILES:
        text = path.read_text()
        text = re.sub(r'__version__ = "[^"]+"', f'__version__ = "{version}"', text)
        text = re.sub(r'== "[0-9]+\.[0-9]+\.[0-9]+"', f'== "{version}"', text)
        path.write_text(text)


def changelog_header() -> str:
    return (
        "# Changelog\n\n"
        "All notable changes to this project will be documented in this file.\n\n"
        "The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),\n"
        "and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n"
    )


def release_section(version: str, previous_tag: str, groups: dict[str, list[str]]) -> str:
    today = dt.date.today().isoformat()
    compare_url = f"{REPO_URL}/compare/{previous_tag}...v{version}"
    parts = [f"## [{version}]({compare_url}) - {today}\n"]
    for key in ("breaking", "feat", "fix", "perf"):
        entries = groups[key]
        if not entries:
            continue
        parts.append(f"### {GROUP_TITLES[key]}\n")
        parts.extend(entries)
        parts.append("")
    return "\n".join(parts).rstrip() + "\n\n"


def update_changelog(version: str, previous_tag: str, groups: dict[str, list[str]]) -> None:
    path = ROOT / "CHANGELOG.md"
    existing = path.read_text() if path.exists() else ""
    section_marker = f"## [{version}]"
    existing_section = existing.find(section_marker)
    if existing_section != -1:
        next_section = existing.find("\n## [", existing_section + 1)
        existing = existing[:existing_section] if next_section == -1 else existing[:existing_section] + existing[next_section + 1 :]

    unreleased_marker = "## [Unreleased]"
    unreleased_index = existing.find(unreleased_marker)
    if unreleased_index == -1:
        rest_index = existing.find("## [")
        rest = existing[rest_index:] if rest_index != -1 else ""
        body = "## [Unreleased]\n\n" + release_section(version, previous_tag, groups) + rest
    else:
        next_section = existing.find("\n## [", unreleased_index + 1)
        if next_section == -1:
            body = "## [Unreleased]\n\n" + release_section(version, previous_tag, groups)
        else:
            rest = existing[next_section + 1 :]
            body = "## [Unreleased]\n\n" + release_section(version, previous_tag, groups) + rest

    path.write_text(changelog_header() + body)


def write_release_info(needed: bool, version: str = "", previous_tag: str = "") -> None:
    lines = [
        f"release_needed={'true' if needed else 'false'}",
        f"version={version}",
        f"previous_tag={previous_tag}",
    ]
    (ROOT / "release-info.env").write_text("\n".join(lines) + "\n")


def prepare() -> int:
    previous_tag = latest_repo_tag()
    bump, groups = analyze_commits(previous_tag)
    if bump is None:
        write_release_info(False)
        print(f"No release-producing commits since {previous_tag}.")
        return 0

    version = bump_version(previous_tag, bump)
    update_versions(version)
    update_changelog(version, previous_tag, groups)
    write_release_info(True, version, previous_tag)
    print(f"Prepared release v{version} from {previous_tag} with a {bump} bump.")
    return 0


def current_version() -> int:
    manifest = (ROOT / "Cargo.toml").read_text()
    match = re.search(r'(?m)^version = "([^"]+)"$', manifest)
    if not match:
        print("Cargo.toml does not contain a package version", file=sys.stderr)
        return 1
    print(match.group(1))
    return 0


def notes(version: str) -> int:
    changelog = (ROOT / "CHANGELOG.md").read_text()
    start_marker = f"## [{version}]"
    start = changelog.find(start_marker)
    if start == -1:
        print(f"CHANGELOG.md does not contain {start_marker}", file=sys.stderr)
        return 1
    next_start = changelog.find("\n## [", start + 1)
    section = changelog[start:] if next_start == -1 else changelog[start:next_start]
    print(section.strip())
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("prepare")
    subcommands.add_parser("current-version")
    notes_parser = subcommands.add_parser("notes")
    notes_parser.add_argument("--version", required=True)
    args = parser.parse_args()

    if args.command == "prepare":
        return prepare()
    if args.command == "current-version":
        return current_version()
    if args.command == "notes":
        return notes(args.version)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
