#!/usr/bin/env python3
"""Rebuild the pinned PICO-8 E3 cohort artifacts from checked-in sources."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).parent
MANIFEST = ROOT / "paper/e3/cohort.json"


def main() -> None:
    cohort = json.loads(MANIFEST.read_text(encoding="utf-8"))
    output_dir = ROOT / "paper/e3"
    output_dir.mkdir(parents=True, exist_ok=True)

    for entry in cohort["entries"]:
        source = ROOT / entry["source_path"]
        target = ROOT / entry["artifact_path"]
        target.parent.mkdir(parents=True, exist_ok=True)

        if entry["source_candidate_index"] == "replacement-fireintro":
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "build_paper_workload.py"),
                    "--output",
                    str(target),
                ],
                check=True,
            )
        elif source.resolve() != target.resolve():
            shutil.copyfile(source, target)

        digest = hashlib.sha256(target.read_bytes()).hexdigest()
        if digest != entry["sha256"]:
            raise SystemExit(
                f"digest mismatch for {entry['artifact_path']}: {digest}"
            )

    attribution = [
        "# PICO-8 E3 cohort attribution and licenses",
        "",
        "These third-party cartridges are separate from the repository's",
        "BSD-3-Clause code license. The Fraser-derived entries rely on the",
        "per-cartridge `CC4-BY-NC-SA` license markers in the pinned Fraser",
        "PICO-8 Games Dataset. They are reconstructed from dataset code and",
        "asset fields, not claimed to be byte-identical copies of BBS uploads.",
        f"Dataset attribution: {cohort['fraser_dataset_author']},",
        f"[{cohort['source_dataset_title']}]({cohort['source_dataset']}),",
        f"licensed under [CC BY-NC-SA 4.0]({cohort['fraser_dataset_license_url']}).",
        "The reconstructed files retain the per-cart CC BY-NC-SA 4.0 terms.",
        "Reuse must be noncommercial and attributed; indicate changes and",
        "distribute adaptations under the same or a compatible license.",
        "",
        "The Fire Intro adaptation is separately supplied under its upstream",
        "MIT license. Its copyright and permission notice are retained in",
        "`../../source/public/fire-intro-effect-MIT.txt`; only title escapes",
        "were normalized to ASCII in the adaptation.",
        "",
        "Attributions and artifact digests:",
        "",
    ]
    for entry in cohort["entries"]:
        attribution.extend(
            [
                f"{entry['cohort_index']}. **{entry['title']}** — "
                f"{entry['credited_author']}",
                f"   Source: <{entry['source_url']}>",
                f"   License: [{entry['license']}]({entry['license_url']})",
                f"   Artifact SHA-256: `{entry['sha256']}`",
            ]
        )
        if entry.get("origin_note"):
            attribution.append(f"   Changes/origin: {entry['origin_note']}")
        attribution.append("")
    (output_dir / "ATTRIBUTION.md").write_text("\n".join(attribution), encoding="utf-8")

    print(f"Verified {len(cohort['entries']) - 1} canonical carts and rebuilt Fire Intro.")


if __name__ == "__main__":
    main()
