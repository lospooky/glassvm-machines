#!/usr/bin/env python3
"""Rebuild the 13 distributable TIC-80 E3 cartridges from pinned sources."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).parent
COHORT = ROOT / "paper/e3/cohort.json"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    cohort = json.loads(COHORT.read_text(encoding="utf-8"))
    members = cohort["members"]
    included = [member for member in members if member["availability"] == "included_artifact"]
    metadata_only = [member for member in members if member["availability"] == "metadata_only"]
    if len(members) != 30 or len(included) != 13 or len(metadata_only) != 17:
        raise SystemExit("E3 cohort must contain 13 artifacts and 17 metadata-only entries")

    expected_files: set[str] = set()
    for member in included:
        source = ROOT / member["source_path"]
        license_path = ROOT / member["license_path"]
        if not license_path.is_file():
            raise SystemExit(f"missing license notice for {member['name']}: {license_path}")
        target = ROOT / member["artifact_path"]
        source_bytes = source.read_bytes()
        if source.parts[-2] == "pakettic":
            if sha256(source_bytes) != member["source_sha256"]:
                raise SystemExit(f"source SHA-256 mismatch: {source}")
            if len(source_bytes) > 65_535:
                raise SystemExit(f"Lua source is too large for the TIC-80 code chunk: {source}")
            artifact = bytes([5]) + len(source_bytes).to_bytes(2, "little") + bytes([0]) + source_bytes
            expected_hash = member["tested_artifact_sha256"]
        else:
            artifact = source_bytes
            expected_hash = member["artifact_sha256"]

        actual_hash = sha256(artifact)
        if actual_hash != expected_hash:
            raise SystemExit(f"artifact SHA-256 mismatch for {member['name']}: {actual_hash}")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(artifact)
        expected_files.add(target.name)

    # Metadata-only members intentionally have no artifact/source path.
    for member in metadata_only:
        if set(member) != {
            "id", "name", "availability", "page_url", "download_url", "catalog_md5", "sha256"
        }:
            raise SystemExit(f"metadata-only entry contains fields beyond identity/URLs/hashes: {member['id']}")

    output = ROOT / "paper/e3"
    for stale in output.glob("*.tic"):
        if stale.name not in expected_files:
            stale.unlink()
    print(f"Rebuilt {len(included)} E3 carts; retained {len(metadata_only)} URL/hash-only records.")


if __name__ == "__main__":
    main()
