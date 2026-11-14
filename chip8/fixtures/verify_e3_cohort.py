#!/usr/bin/env python3
"""Verify the pinned CHIP-8 E3 cohort files and manifest invariants."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).parent
MANIFEST = ROOT / "paper/e3/cohort.json"


def main() -> None:
    cohort = json.loads(MANIFEST.read_text(encoding="utf-8"))
    members = cohort["members"]
    if cohort["status"] != "pinned" or len(members) != 30:
        raise SystemExit("expected a pinned 30-member CHIP-8 E3 cohort")
    if len({entry["rom_id"] for entry in members}) != 30:
        raise SystemExit("duplicate ROM IDs in E3 cohort")
    if len({entry["sha256"] for entry in members}) != 30:
        raise SystemExit("duplicate artifact hashes in E3 cohort")
    for entry in members:
        if entry["license"] != "CC0-1.0" or not entry["screen"]["screen_pass"]:
            raise SystemExit(f"invalid cohort member: {entry['rom_id']}")
        if entry["screen"]["frame_boundaries"] != 600 or entry["screen"]["frame_artifacts"] != 600:
            raise SystemExit(f"incomplete 600-frame screen: {entry['rom_id']}")
        artifact = ROOT / entry["artifact_path"]
        if not artifact.is_file():
            raise SystemExit(f"missing artifact: {artifact}")
        digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
        if digest != entry["sha256"]:
            raise SystemExit(f"SHA-256 mismatch for {entry['rom_id']}: {digest}")
    print(f"Verified {len(members)} CHIP-8 E3 artifacts and unique hashes.")


if __name__ == "__main__":
    main()
