#!/usr/bin/env python3
"""Compare finite hook probes built with upstream and inlined dispatch."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    inputs = {}
    for label in ("reference", "candidate"):
        binary = getattr(args, label).resolve(strict=True)
        result = json.loads(subprocess.check_output([str(binary)], text=True))
        inputs[label] = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                         "projection": result}
    reference = inputs["reference"]["projection"]
    candidate = inputs["candidate"]["projection"]
    assert len(reference["cases"]) == len(candidate["cases"])
    for before, after in zip(reference["cases"], candidate["cases"], strict=True):
        assert before == after, f"hook mismatch: mask={before['mask']} quantum={before['quantum']}"
    assert reference == candidate, "quota exhaustion or yield/resume behavior differs"
    summary = {
        "cases": [{"mask": case["mask"], "quantum": case["quantum"],
                   "event_count": len(case["events"]), "sha256": digest(case)} for case in reference["cases"]],
        "exhaustion": reference["exhaustion"], "yielding": reference["yielding"],
    }
    report = {"schema": "glassvm.lua_hook_differential", "equal": True, "summary": summary,
              "binaries": {label: {key: value for key, value in info.items() if key != "projection"}
                           for label, info in inputs.items()}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Matched {len(summary['cases'])} hook cases, quota exhaustion, and yield/resume")


if __name__ == "__main__":
    main()
