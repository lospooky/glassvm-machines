#!/usr/bin/env python3
"""Verify the canonical, pinned CE1 artifacts in place."""
import hashlib
import re
from pathlib import Path

root = Path(__file__).parent
manifest = (root / "paper/ce1/cohort.toml").read_text()
blocks = re.split(r"(?m)^\[\[workload\]\]\s*$", manifest)[1:]
for block in blocks:
    fields = {}
    for key in ("source_path", "artifact_path", "sha256"):
        match = re.search(rf'(?m)^{key}\s*=\s*"([^"]+)"\s*$', block)
        if match is None:
            raise SystemExit(f"missing {key} in CE1 workload entry")
        fields[key] = match.group(1)
    source = root / fields["source_path"]
    artifact = root / fields["artifact_path"]
    if source.resolve() != artifact.resolve():
        raise SystemExit(
            f"CE1 source and artifact must use one canonical path: {fields['artifact_path']}"
        )
    digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
    if digest != fields["sha256"]:
        raise SystemExit(
            f"digest mismatch for {fields['artifact_path']}: {digest}"
        )
print(f"Verified {len(blocks)} canonical CE1 artifacts.")
