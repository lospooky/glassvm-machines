#!/usr/bin/env python3
"""Spot-check bounded runtime-activity projections through installed providers.

Keeps fixed-family scalar summaries and diagnostic projection fingerprints.
Does not retain events, decode recorder storage, or run the full paper suite.
"""
import argparse
import hashlib
import importlib.metadata
import json
import subprocess
import sys
import tempfile
import tomllib
from collections import Counter
from pathlib import Path

EXECUTION = "glassvm.execution_activity"
RNG = "glassvm.rng_activity"


class Projection:
    def __init__(self):
        self.kinds = Counter()
        self.channels = Counter()
        self.totals = {EXECUTION: Counter(), RNG: Counter()}
        self.digests = {key: hashlib.sha256() for key in self.totals}
        self.previous_frame = {key: None for key in self.totals}

    def emit_batch(self, emissions):
        for emission in emissions:
            self.channels[emission["kind"]] += 1
            if emission["kind"] != "event":
                continue
            event = emission["record"]
            kind = event["kind"]
            name = kind if isinstance(kind, str) else "extension:" + kind["extension"]
            self.kinds[name] += 1
            if isinstance(kind, str) or kind["extension"] not in self.totals:
                continue
            family = kind["extension"]
            value = event["extensions"][family]
            assert value["schema"] == {"id": family, "version": {"major": 1, "minor": 0, "patch": 0}}
            frame = event["frame"]
            assert frame == event["step"]
            assert self.previous_frame[family] is None or frame > self.previous_frame[family]
            self.previous_frame[family] = frame
            totals = self.totals[family]
            totals["frames"] += 1
            if family == EXECUTION:
                assert set(value) == {"schema", "callbacks", "instruction_hook"}
                assert set(value["callbacks"]) == {"update", "draw", "main", "border"}
                totals.update(value["callbacks"])
                assert value["instruction_hook"]["quantum"] == 1000
                totals["hook_ticks"] += value["instruction_hook"]["ticks"]
            else:
                assert set(value) == {"schema", "random_calls", "reseed_calls"}
                assert value["random_calls"] + value["reseed_calls"] > 0
                totals["random_calls"] += value["random_calls"]
                totals["reseed_calls"] += value["reseed_calls"]
            # Intentional finite semantic projection, not a global sequence or
            # recorder-layout comparison and not a runtime identity algorithm.
            self.digests[family].update(json.dumps(
                [frame, event["step"], value], sort_keys=True, separators=(",", ":")
            ).encode() + b"\n")

    def record_capability_outputs(self, outputs):
        assert not outputs

    def record_capability_receipts(self, receipts):
        assert not receipts

    def summary(self):
        return {
            "event_kinds": dict(self.kinds), "channels": dict(self.channels),
            "totals": {key: dict(value) for key, value in self.totals.items()},
            "projection_sha256": {key: digest.hexdigest() for key, digest in self.digests.items()},
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--paper-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--frames", type=int, default=600)
    args = parser.parse_args()
    if args.frames < 1:
        parser.error("--frames must be positive")
    sys.path.insert(0, str(args.paper_root / "experiments"))
    from run_trial import machine_configuration, observation_request
    import glassvm

    machines_root = Path(__file__).resolve().parents[1]
    workloads = tomllib.loads((args.paper_root / "PAPER_WORKLOADS.toml").read_text())["workload"]
    recipes = tomllib.loads((args.paper_root / "experiments/OBSERVATION_RECIPES.toml").read_text())["recipe"]
    recipe = next(recipe for recipe in recipes if recipe["id"] == "selected_normalized")
    runtime = glassvm.Runtime.discover()
    results = []
    for workload in workloads:
        machine = workload["machine_id"]
        if machine not in ("pico8", "tic80"):
            continue
        artifact = (machines_root / workload["artifact_path"]).read_bytes()
        assert hashlib.sha256(artifact).hexdigest() == workload["artifact_sha256"]
        projections = {}
        for mode, family in (("selected_normalized", None), ("execution_only", EXECUTION),
                             ("rng_only", RNG), ("all_normalized", None)):
            observation = observation_request(recipe, workload)
            if family:
                observation["normalized_events"]["events"] = {"kinds": [{"extension": family}]}
            elif mode == "all_normalized":
                observation["normalized_events"]["events"] = "all"
            prepared = runtime.prepare(
                machine, artifact,
                configuration=machine_configuration(machine, workload["configuration"]),
                execution_controls={"frame_limit": args.frames, "step_limit": None, "bundle_limits": []},
                observation=observation,
            )
            projection = Projection()
            with tempfile.TemporaryDirectory(prefix="glassvm-activity-") as directory:
                result = prepared.execute(sinks=[projection],
                    file_recorder=glassvm.FileRecorder(str(Path(directory) / "run")))
            assert result["machine_execution"]["status"] == "completed", result
            assert result["evidence_fulfillment"]["status"] == "complete", result
            assert result["python_sink_delivery"]["status"] == "complete", result
            assert result["recorder_finalization"]["status"] == "finalized", result
            assert result["publication"]["status"] == "published", result
            assert projection.channels["native_evidence"] == 0
            assert projection.channels["frame"] == 0
            if family:
                assert set(projection.kinds) <= {"extension:" + family}
            summary = projection.summary()
            projections[mode] = summary
            results.append({"machine": machine, "mode": mode, "summary": summary,
                            "observation": observation, "result": result})
        for mode, family in (("execution_only", EXECUTION), ("rng_only", RNG)):
            assert projections[mode]["projection_sha256"][family] == projections["all_normalized"]["projection_sha256"][family]
            assert not projections["selected_normalized"]["totals"][family]

    tracked = set(subprocess.check_output(["git", "ls-files", "pico8", "tic80", "conformance"], cwd=machines_root, text=True).splitlines())
    tracked.update(("conformance/tests/runtime_activity.rs", "conformance/runtime_activity_probe.py"))
    report = {
        "note": "Unreleased local source builds; TIC-80 has corrected seeded semantics. Not published 0.3.0 evidence.",
        "machine_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=machines_root, text=True).strip(),
        "source_sha256": {path: hashlib.sha256((machines_root / path).read_bytes()).hexdigest()
                          for path in sorted(tracked) if path.endswith((".rs", ".toml", ".py"))},
        "installed_provider_sources": {name: json.loads(importlib.metadata.distribution(name).read_text("direct_url.json") or "null")
                                       for name in ("glassvm-pico8", "glassvm-tic80")},
        "frames": args.frames, "results": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {len(results)} bounded activity projections to {args.output}")


if __name__ == "__main__":
    main()
