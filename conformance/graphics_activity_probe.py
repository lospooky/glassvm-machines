#!/usr/bin/env python3
"""Spot-check normalized graphics selection on installed local provider wheels.

This is not the paper suite. The consumer keeps scalar summaries and a digest
of an explicitly selected semantic projection, never a run/event history.
"""

import argparse
import hashlib
import importlib.metadata
import json
import subprocess
import sys
import tomllib
from collections import Counter
from pathlib import Path

GRAPHICS = "glassvm.graphics_activity"
SCHEMA = {"id": GRAPHICS, "version": {"major": 1, "minor": 0, "patch": 0}}


class Projection:
    def __init__(self):
        self.channels = Counter()
        self.event_kinds = Counter()
        self.frames = 0
        self.calls = 0
        self.previous_frame = None
        self.minimum = None
        self.maximum = 0
        self.graphics = hashlib.sha256()

    def emit_batch(self, emissions):
        for emission in emissions:
            self.channels[emission["kind"]] += 1
            if emission["kind"] != "event":
                continue
            record = emission["record"]
            kind = record["kind"]
            name = kind if isinstance(kind, str) else "extension:" + kind["extension"]
            self.event_kinds[name] += 1
            if kind != {"extension": GRAPHICS}:
                continue
            payload = record["extensions"][GRAPHICS]
            assert payload["schema"] == SCHEMA, payload
            assert set(payload) == {"schema", "draw_calls"}, payload
            calls = payload["draw_calls"]
            assert isinstance(calls, int) and calls > 0
            frame = record["frame"]
            assert frame == record["step"]
            assert self.previous_frame is None or frame > self.previous_frame
            self.previous_frame = frame
            self.frames += 1
            self.calls += calls
            self.minimum = calls if self.minimum is None else min(self.minimum, calls)
            self.maximum = max(self.maximum, calls)
            # Compare frame/step/payload semantics, not cross-channel sequence
            # allocation or recorder layout. This digest is diagnostic only.
            self.graphics.update(json.dumps(
                [frame, record["step"], payload], sort_keys=True, separators=(",", ":")
            ).encode())
            self.graphics.update(b"\n")

    def record_capability_outputs(self, outputs):
        assert not outputs

    def record_capability_receipts(self, receipts):
        assert not receipts

    def summary(self):
        return {
            "channels": dict(self.channels), "event_kinds": dict(self.event_kinds),
            "graphics_frames": self.frames, "draw_calls": self.calls,
            "min_calls_per_nonempty_frame": self.minimum,
            "max_calls_per_nonempty_frame": self.maximum,
            "graphics_projection_sha256": self.graphics.hexdigest(),
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
        for mode in ("selected_normalized", "graphics_only", "all_normalized"):
            observation = observation_request(recipe, workload)
            if mode == "graphics_only":
                observation["normalized_events"]["events"] = {"kinds": [{"extension": GRAPHICS}]}
            elif mode == "all_normalized":
                observation["normalized_events"]["events"] = "all"
            prepared = runtime.prepare(
                machine, artifact,
                configuration=machine_configuration(machine, workload["configuration"]),
                execution_controls={"frame_limit": args.frames, "step_limit": None, "bundle_limits": []},
                observation=observation,
            )
            projection = Projection()
            result = prepared.execute(sinks=[projection])
            assert result["machine_execution"]["status"] == "completed", result
            assert result["evidence_fulfillment"]["status"] == "complete", result
            assert result["python_sink_delivery"]["status"] == "complete", result
            assert projection.channels["native_evidence"] == 0
            assert projection.channels["frame"] == 0
            summary = projection.summary()
            projections[mode] = summary
            results.append({"machine": machine, "mode": mode, "summary": summary,
                            "request": observation, "result": result})
        assert projections["selected_normalized"]["graphics_frames"] == 0
        assert projections["graphics_only"]["graphics_frames"] > 0
        assert projections["graphics_only"]["graphics_projection_sha256"] == projections["all_normalized"]["graphics_projection_sha256"]
        assert set(projections["graphics_only"]["event_kinds"]) == {"extension:" + GRAPHICS}

    tracked = set(subprocess.check_output(["git", "ls-files", "pico8", "tic80", "conformance"], cwd=machines_root, text=True).splitlines())
    tracked.update(("conformance/tests/graphics_activity.rs", "conformance/graphics_activity_probe.py"))
    report = {
        "note": "Unreleased local source builds; distribution version alone is not source provenance.",
        "machine_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=machines_root, text=True).strip(),
        "source_sha256": {path: hashlib.sha256((machines_root / path).read_bytes()).hexdigest()
                          for path in sorted(tracked) if path.endswith((".rs", ".toml", ".py"))},
        "packages": {name: importlib.metadata.version(name) for name in ("glassvm", "glassvm-pico8", "glassvm-tic80")},
        "installed_provider_sources": {
            name: json.loads(importlib.metadata.distribution(name).read_text("direct_url.json") or "null")
            for name in ("glassvm-pico8", "glassvm-tic80")
        },
        "frames": args.frames, "results": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {len(results)} bounded projection checks to {args.output}")


if __name__ == "__main__":
    main()
