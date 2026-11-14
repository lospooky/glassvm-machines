#!/usr/bin/env python3
"""Verify the canonical CE1 TIC-80 cartridges against their cohort manifest."""

from __future__ import annotations

import hashlib
import tomllib
from pathlib import Path


ROOT = Path(__file__).parent
MANIFEST = ROOT / "paper/ce1/cohort.toml"


def main() -> None:
    cohort = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    workloads = cohort["workload"]
    for workload in workloads:
        artifact = ROOT / workload["artifact_path"]
        if not artifact.is_file():
            raise SystemExit(f"missing CE1 artifact: {artifact}")
        actual = hashlib.sha256(artifact.read_bytes()).hexdigest()
        if actual != workload["sha256"]:
            raise SystemExit(f"CE1 SHA-256 mismatch for {workload['title']}: {actual}")
    print(f"Verified {len(workloads)} canonical CE1 TIC-80 artifacts.")


if __name__ == "__main__":
    main()
