#!/usr/bin/env python3
"""Check that every action in the workflows is pinned to a real commit.

A typo in a commit SHA is not caught by the YAML parser: the pipeline only
fails when GitHub tries to resolve the action, which is after the pull
request was merged. This script resolves every pin and fails when the
commit does not exist (or when an action is not pinned at all).

Usage:
    scripts/check-action-pins.py            # uses the GitHub API
    GITHUB_TOKEN=... scripts/check-action-pins.py

The token is optional; the unauthenticated API allows 60 requests per hour,
which is enough for the pins of this repository.
"""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parent.parent / ".github" / "workflows"
# `uses: owner/repo@<sha> # <tag>`; the tag is only a comment for the reader.
USE = re.compile(r"uses:\s*([\w.-]+/[\w.-]+)@([0-9a-fA-F]{40})\s*(?:#\s*(\S+))?")
UNPINNED = re.compile(r"uses:\s*(?![\w.-]+/[\w.-]+@)[^\s#]+")


def request(url: str, token: str | None) -> dict:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "termru-action-pin-check",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        return {"_error": error.code, "_message": error.reason}
    except urllib.error.URLError as error:
        return {"_error": "network", "_message": str(error.reason)}


def main() -> int:
    token = os.environ.get("GITHUB_TOKEN") or None
    failures: list[str] = []
    warnings: list[str] = []
    checked = 0
    # The same action is used by several jobs; one request per pin is enough.
    cache: dict[tuple[str, str], dict] = {}
    reported: set[tuple[str, str]] = set()

    for workflow in sorted(WORKFLOWS.glob("*.yml")):
        for number, line in enumerate(workflow.read_text().splitlines(), start=1):
            if not line.strip().startswith("- uses:") and "uses:" not in line:
                continue

            match = USE.search(line)
            if match is None:
                if UNPINNED.search(line):
                    failures.append(
                        f"{workflow.name}:{number}: the action is not pinned to a commit: {line.strip()}"
                    )
                continue

            repository, sha, tag = match.group(1), match.group(2), match.group(3)
            checked += 1
            key = (repository, sha)
            if key not in cache:
                cache[key] = request(
                    f"https://api.github.com/repos/{repository}/commits/{sha}", token
                )
            commit = cache[key]
            if "sha" in commit:
                continue
            detail = commit.get("_message", "the commit does not exist")
            if commit.get("_error") in (403, 429) or "rate limit" in str(detail).lower():
                if key not in reported:
                    reported.add(key)
                    warnings.append(f"{repository}@{sha[:12]}: {detail} (the check was skipped)")
            else:
                failures.append(f"{workflow.name}:{number}: {repository}@{sha[:12]} ({tag}): {detail}")

    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)

    if failures:
        print("the action pins are not valid:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    unique = len(cache)
    if warnings:
        print(f"{unique} action pins checked, {len(warnings)} could not be verified")
    else:
        print(f"all {unique} action pins resolve to a real commit ({checked} uses)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
