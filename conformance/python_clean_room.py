"""Check installed GlassVM bundle wheels through the public Python facade.

This script deliberately runs outside the machine workspace's Rust process. A
caller creates an isolated Python environment, installs the generic facade and
the selected bundle wheels, and invokes this script with the expected machine
set. It therefore checks installed entry points and native extensions rather
than importing source-tree modules.
"""

from __future__ import annotations

import argparse
import importlib
import json
from pathlib import Path
import tempfile

import glassvm


PROTOCOL = "glassvm.python_bundle"
PROTOCOL_VERSION = {"major": 2, "minor": 0, "patch": 0}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--machine-root",
        type=Path,
        required=True,
        help="checked-out glassvm-machines root containing smoke fixtures",
    )
    parser.add_argument(
        "--output-root",
        type=Path,
        required=True,
        help="temporary directory for published smoke runs",
    )
    parser.add_argument(
        "--expect",
        nargs="*",
        default=[],
        help="machine IDs expected to be installed",
    )
    parser.add_argument(
        "--reject",
        nargs="*",
        default=[],
        help="machine IDs that must be absent from discovery",
    )
    parser.add_argument(
        "--invalid-artifact",
        nargs="*",
        default=[],
        help="installed machine IDs that must reject malformed artifacts during preparation",
    )
    return parser.parse_args()


def assert_provider_metadata(metadata: list[dict[str, object]], expected: list[str]) -> None:
    machine_ids = [entry["machine_id"] for entry in metadata]
    assert machine_ids == sorted(expected), machine_ids

    for entry in metadata:
        machine = entry["machine_id"]
        assert entry == {
            "bundle_version": "0.2.0",
            "distribution": f"glassvm-{machine}",
            "entry_point": machine,
            "execute_function": "execute_prepared",
            "machine_id": machine,
            "module": f"glassvm_{machine}",
            "prepare_function": "prepare_run",
            "protocol": PROTOCOL,
            "protocol_version": PROTOCOL_VERSION,
        }, entry
        importlib.import_module(entry["module"])


def assert_absent(runtime: glassvm.Runtime, machine: str) -> None:
    try:
        runtime.prepare(machine, b"")
    except LookupError as error:
        assert "not installed" in str(error), error
    else:
        raise AssertionError(f"absent machine {machine!r} was accepted")


def assert_invalid_artifact_fails_during_preparation(
    runtime: glassvm.Runtime, machines: list[str]
) -> None:
    for machine in machines:
        try:
            runtime.prepare(machine, b"not a valid machine artifact")
        except ValueError:
            continue
        raise AssertionError(f"{machine!r} accepted an invalid artifact")


def assert_smoke_runs(
    runtime: glassvm.Runtime,
    machine_root: Path,
    output_root: Path,
    machines: list[str],
) -> None:
    controls = {"frame_limit": 1, "step_limit": None, "bundle_limits": []}
    output_root.mkdir(parents=True, exist_ok=True)

    class Consumer:
        def __init__(self) -> None:
            self.emissions: list[dict[str, object]] = []
            self.outputs: list[dict[str, object]] = []
            self.receipts: list[dict[str, object]] = []

        def emit_batch(self, emissions: list[dict[str, object]]) -> None:
            self.emissions.extend(emissions)

        def record_capability_outputs(self, outputs: list[dict[str, object]]) -> None:
            self.outputs.extend(outputs)

        def record_capability_receipts(self, receipts: list[dict[str, object]]) -> None:
            self.receipts.extend(receipts)

    for machine in machines:
        artifact = (machine_root / machine / "fixtures" / "smoke.rom").read_bytes()
        with tempfile.TemporaryDirectory(prefix=f"{machine}-", dir=output_root) as run_dir:
            output_path = Path(run_dir) / "run"
            consumer = Consumer()
            result = runtime.prepare(
                machine,
                artifact,
                execution_controls=controls,
            ).execute(
                sinks=[consumer],
                file_recorder=glassvm.FileRecorder(str(output_path)),
            )
            assert sorted(result) == [
                "evidence_fulfillment",
                "machine_execution",
                "publication",
                "published_run",
                "python_sink_delivery",
                "recorder_finalization",
            ], result
            assert result["machine_execution"]["status"] == "completed", result
            assert result["machine_execution"]["result"]["common"]["boot_success"] is True, result
            assert result["evidence_fulfillment"]["status"] == "complete", result
            assert result["python_sink_delivery"]["status"] == "complete", result
            assert result["python_sink_delivery"]["items_delivered"] > 0, result
            assert result["recorder_finalization"]["status"] == "finalized", result
            assert result["recorder_finalization"]["result"]["status"] == "complete", result
            assert result["publication"]["status"] == "published", result
            assert result["published_run"] == {
                "path": str(output_path),
                "published": True,
            }, result
            assert output_path.is_dir(), output_path
            kinds = {emission["kind"] for emission in consumer.emissions}
            assert {"run_started", "evidence_receipt", "run_finished"} <= kinds, kinds
            assert consumer.outputs == [], consumer.outputs
            assert consumer.receipts == [], consumer.receipts


def main() -> None:
    args = parse_args()
    expected = sorted(args.expect)
    runtime = glassvm.Runtime.discover()
    metadata = json.loads(runtime.bundles())
    assert_provider_metadata(metadata, expected)
    for machine in args.reject:
        assert_absent(runtime, machine)
    assert_invalid_artifact_fails_during_preparation(runtime, args.invalid_artifact)
    assert_smoke_runs(runtime, args.machine_root, args.output_root, expected)
    print(f"clean-room Python check passed for: {', '.join(expected) or 'no providers'}")


if __name__ == "__main__":
    main()
