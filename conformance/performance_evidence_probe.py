#!/usr/bin/env python3
"""Compare short source-build runs using the existing paper requests.

This diagnostic hashes each caller-selected channel incrementally. It retains
counts, digests, and final receipts, rather than a history of emissions.
"""

import argparse
import hashlib
import json
import sys
import tempfile
import tomllib
from collections import Counter
from pathlib import Path


class Projection:
    def __init__(self):
        self.counts = Counter()
        self.digests = {}

    def record(self, channel, value):
        self.counts[channel] += 1
        digest = self.digests.setdefault(channel, hashlib.sha256())
        digest.update(json.dumps(value, sort_keys=True, separators=(",", ":")).encode())
        digest.update(b"\n")

    def emit_batch(self, emissions):
        for emission in emissions:
            self.record(emission["kind"], emission["record"])

    def record_capability_outputs(self, outputs):
        for output in outputs:
            self.record("capability_output", output)

    def record_capability_receipts(self, receipts):
        for receipt in receipts:
            self.record("capability_receipt", receipt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--paper-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--compare", type=Path, help="Fail if the logical projections or receipts differ from this baseline.")
    parser.add_argument("--frames", type=int, default=8)
    args = parser.parse_args()
    sys.path.insert(0, str(args.paper_root / "experiments"))
    from run_trial import machine_configuration, observation_request
    import glassvm

    machines_root = Path(__file__).resolve().parents[1]
    workloads = tomllib.loads((args.paper_root / "PAPER_WORKLOADS.toml").read_text())["workload"]
    recipes = tomllib.loads((args.paper_root / "experiments/OBSERVATION_RECIPES.toml").read_text())["recipe"]
    runtime = glassvm.Runtime.discover()
    results = []
    for workload in workloads:
        machine = workload["machine_id"]
        if machine not in ("pico8", "tic80"):
            continue
        artifact = (machines_root / workload["artifact_path"]).read_bytes()
        assert hashlib.sha256(artifact).hexdigest() == workload["artifact_sha256"]
        for recipe in recipes + [{**recipes[-1], "id": "all_evidence"}]:
            observation = observation_request(recipe, workload)
            if recipe["id"] == "all_evidence":
                observation["normalized_events"]["events"] = "all"
                observation["native_evidence"] = {"enabled": True, "all": True, "kinds": []}
                observation["frames"]["capture"] = "full"
                observation["snapshots"]["capture"] = "final"
            prepared = runtime.prepare(
                machine, artifact,
                configuration=machine_configuration(machine, workload["configuration"]),
                execution_controls={"frame_limit": args.frames, "step_limit": None, "bundle_limits": []},
                observation=observation,
            )
            projection = Projection()
            with tempfile.TemporaryDirectory(prefix="glassvm-performance-probe-") as directory:
                result = prepared.execute(
                    sinks=[projection], file_recorder=glassvm.FileRecorder(str(Path(directory) / "run")),
                )
                assert result["machine_execution"]["status"] == "completed", result
                assert result["python_sink_delivery"]["status"] == "complete", result
                assert result["evidence_fulfillment"]["status"] == "complete", result
                assert result["recorder_finalization"]["status"] == "finalized", result
                assert result["publication"]["status"] == "published", result
                # File locations vary between probes; compare the accounting itself.
                recorder = result["recorder_finalization"]["result"]
                results.append({
                    "machine": machine, "recipe": recipe["id"],
                    "counts": dict(projection.counts),
                    "channel_sha256": {key: value.hexdigest() for key, value in projection.digests.items()},
                    "execution": result["machine_execution"],
                    "evidence": result["evidence_fulfillment"],
                    "recorder": recorder,
                })
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2) + "\n")
    if args.compare:
        expected = json.loads(args.compare.read_text())
        if len(expected) != len(results):
            raise AssertionError("evidence probe case count changed")
        for before, after in zip(expected, results, strict=True):
            changed = [key for key in before.keys() | after.keys() if before.get(key) != after.get(key)]
            if changed:
                raise AssertionError(f"{after['machine']}/{after['recipe']}: changed {changed}")
    print(f"wrote {len(results)} short evidence projections to {args.output}")


if __name__ == "__main__":
    main()
