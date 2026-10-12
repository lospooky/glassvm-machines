#!/usr/bin/env python3
"""Print local distribution files not yet present for a PyPI project version."""

from __future__ import annotations

import json
from pathlib import Path
import sys
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def existing_filenames(project: str, version: str) -> set[str]:
    url = f"https://pypi.org/pypi/{project}/{version}/json"
    request = Request(url, headers={"User-Agent": "glassvm-machines-release/1.0"})
    try:
        with urlopen(request, timeout=20) as response:
            release = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return set()
        raise
    return {file["filename"] for file in release["urls"]}


def main() -> int:
    if len(sys.argv) < 4:
        raise SystemExit("usage: publish_missing_pypi_files.py PROJECT VERSION FILE...")
    project, version, *filenames = sys.argv[1:]
    existing = existing_filenames(project, version)
    candidates = sorted({Path(filename) for filename in filenames}, key=str)
    for candidate in candidates:
        if candidate.name not in existing:
            print(candidate)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
