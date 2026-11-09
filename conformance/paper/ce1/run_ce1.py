#!/usr/bin/env python3
"""Run the pinned CE1 cohort through the published GlassVM Python API."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import math
import os
import platform
import random
import resource
import re
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

RESULT_SCHEMA_VERSION = "1.0.0"
OBSERVATION_SCHEMA_VERSION = {"major": 2, "minor": 1, "patch": 0}
INPUT_SCHEDULE_SCHEMA = {
    "id": "glassvm.input_schedule",
    "version": {"major": 1, "minor": 0, "patch": 0},
}
CONFIGURATION_SCHEMAS = {
    "chip8": ("chip8.machine_configuration", 2),
    "pico8": ("pico8.machine_configuration", 1),
    "tic80": ("tic80.machine_configuration", 1),
}


def command_output(*command: str) -> str:
    try:
        return subprocess.check_output(
            command, text=True, stderr=subprocess.DEVNULL
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return "unavailable"


def git_metadata(path: Path) -> dict[str, object]:
    revision = command_output("git", "-C", str(path), "rev-parse", "HEAD")
    status = command_output("git", "-C", str(path), "status", "--porcelain")
    return {
        "revision": None if revision == "unavailable" else revision,
        "dirty": status != "",
    }


def package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def typed_value(value: object) -> dict[str, object]:
    if value is None:
        return {"Null": None}
    if isinstance(value, bool):
        return {"Bool": value}
    if isinstance(value, int):
        return {"Unsigned": value} if value >= 0 else {"Signed": value}
    if isinstance(value, float):
        return {"Float": value}
    if isinstance(value, str):
        return {"Text": value}
    if isinstance(value, list):
        return {"Array": [typed_value(item) for item in value]}
    if isinstance(value, dict):
        return {"Map": {key: typed_value(item) for key, item in value.items()}}
    raise TypeError(f"unsupported configuration value: {type(value).__name__}")


def event_kind(value: str) -> object:
    if value.startswith("extension:"):
        return {"extension": value.removeprefix("extension:")}
    return value


def observation_request(policy: dict, cohort: dict) -> dict:
    normalized = policy["normalized_events"]
    if normalized == "none":
        events: object = "none"
    elif normalized == "selected":
        events = {
            "kinds": [
                event_kind(item)
                for item in cohort["selected_normalized_event_kinds"]
            ]
        }
    elif normalized == "all":
        events = "all"
    else:
        raise ValueError(f"unknown normalized-event policy {normalized!r}")

    return {
        "schema_version": OBSERVATION_SCHEMA_VERSION,
        "guarantee": "summary",
        "normalized_events": {
            "events": events,
            "access_detail": "none",
            "state_diffs": False,
        },
        "capabilities": [],
        "native_evidence": {"enabled": False, "all": False, "kinds": []},
        "frames": {"capture": policy["frames"]},
        "snapshots": {"capture": "none"},
        "input_value_evidence": {"enabled": False},
        "budgets": {},
        "overflow": "fail",
    }


def machine_configuration(machine: str, values: dict) -> dict:
    schema_id, major = CONFIGURATION_SCHEMAS[machine]
    return {
        "schema": {
            "id": schema_id,
            "version": {"major": major, "minor": 0, "patch": 0},
        },
        "value": {"Map": {key: typed_value(value) for key, value in values.items()}},
    }


def host_metadata() -> dict[str, object]:
    kernel = platform.release()
    version_path = Path("/proc/version")
    proc_version = version_path.read_text(errors="replace") if version_path.exists() else ""
    virtualization = (
        "WSL2"
        if "microsoft" in (kernel + proc_version).lower()
        else "not identified"
    )
    cpu = None
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text(errors="replace").splitlines():
            if line.lower().startswith(("model name", "hardware")):
                cpu = line.partition(":")[2].strip()
                break
    return {
        "system": platform.system(),
        "release": kernel,
        "platform": platform.platform(),
        "architecture": platform.machine(),
        "processor": platform.processor() or cpu,
        "logical_cpu_count": os.cpu_count(),
        "virtualization": virtualization,
        "python": sys.version,
        "python_implementation": platform.python_implementation(),
        "uv": command_output("uv", "--version"),
        "rustc": command_output("rustc", "--version"),
    }


def trial(task: dict, repo_root: Path) -> dict:
    machine = task["machine_id"]
    workload = task["workload"]
    policy = task["policy"]
    artifact_path = (
        repo_root / machine / "fixtures" / workload["artifact_path"]
    )
    artifact = artifact_path.read_bytes()
    digest = hashlib.sha256(artifact).hexdigest()
    if digest != workload["sha256"]:
        raise ValueError(f"fixture digest mismatch for {artifact_path}: {digest}")

    started = datetime.now(timezone.utc).isoformat()
    process_started = time.perf_counter()
    rss_before = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    import glassvm

    runtime = glassvm.Runtime.discover()
    providers = json.loads(runtime.bundles())
    provider = next(
        (item for item in providers if item.get("machine_id") == machine), None
    )
    result: dict[str, object] = {
        "result_schema_version": RESULT_SCHEMA_VERSION,
        "trial": {
            "machine_id": machine,
            "workload_title": workload["title"],
            "policy_id": policy["id"],
            "repetition": task["repetition"],
            "order_index": task["order_index"],
            "started_utc": started,
        },
        "source": {
            "installed_package_sources": task["package_sources"],
            "cohort_checkout": git_metadata(repo_root),
            "provider_metadata": provider,
        },
        "host": {
            **task["host"],
            "packages": {
                "glassvm": package_version("glassvm"),
                f"glassvm-{machine}": package_version(f"glassvm-{machine}"),
            },
        },
        "workload": {
            "artifact_path": workload["artifact_path"],
            "artifact_sha256": digest,
            "artifact_bytes": len(artifact),
            "configuration": task["cohort"]["configuration"],
            "input_schedule": "empty",
            "frame_limit": task["frame_limit"],
        },
        "policy": policy,
        "request": None,
        "prepared_observation_identity": None,
        "outcomes": {},
        "execution_result": None,
        "evidence_receipt": None,
        "recorder_receipt": None,
        "evidence_logical_bytes": None,
        "recorder_logical_bytes": None,
        "recorder_encoded_bytes": None,
        "recorder_directory_bytes": None,
        "memory": {},
        "timing": {},
        "failure": None,
    }

    if provider is None:
        result["outcomes"] = {
            "machine_execution": "not_started",
            "evidence_fulfillment": "not_started",
            "python_sink_delivery": "not_started",
            "recorder_finalization": "not_started",
            "publication": "not_attempted",
        }
        result["failure"] = {"phase": "discovery", "message": "provider not found"}
        return result

    request = observation_request(policy, task["cohort"])
    input_schedule = {"schema": INPUT_SCHEDULE_SCHEMA, "entries": []}
    execution_controls = {
        "frame_limit": task["frame_limit"],
        "step_limit": None,
        "bundle_limits": [],
    }
    config = machine_configuration(machine, task["cohort"]["configuration"])
    result["request"] = {
        "configuration": config,
        "input_schedule": input_schedule,
        "execution_controls": execution_controls,
        "observation": request,
    }

    prepare_started = time.perf_counter()
    try:
        prepared = runtime.prepare(
            machine,
            artifact,
            configuration=config,
            input_schedule=input_schedule,
            execution_controls=execution_controls,
            observation=request,
        )
    except Exception as error:  # Keep preparation failures as raw trial results.
        result["outcomes"] = {
            "machine_execution": "not_started",
            "evidence_fulfillment": "not_started",
            "python_sink_delivery": "not_started",
            "recorder_finalization": "not_started",
            "publication": "not_attempted",
        }
        result["failure"] = {
            "phase": "preparation",
            "error_type": type(error).__name__,
            "message": str(error),
        }
        result["timing"] = {
            "preparation_seconds": time.perf_counter() - prepare_started,
            "process_wall_seconds": time.perf_counter() - process_started,
        }
        return result
    preparation_seconds = time.perf_counter() - prepare_started

    with tempfile.TemporaryDirectory(prefix=f"glassvm-ce1-{machine}-") as temporary:
        run_path = Path(temporary) / "published-run"
        execute_started = time.perf_counter()
        try:
            run_result = prepared.execute(
                file_recorder=glassvm.FileRecorder(str(run_path))
            )
        except Exception as error:  # Preserve exceptions without collapsing outcomes.
            result["outcomes"] = {
                "machine_execution": "unknown",
                "evidence_fulfillment": "unknown",
                "python_sink_delivery": "unknown",
                "recorder_finalization": "unknown",
                "publication": "unknown",
            }
            result["failure"] = {
                "phase": "execute_and_publish",
                "error_type": type(error).__name__,
                "message": str(error),
            }
            result["timing"] = {
                "preparation_seconds": preparation_seconds,
                "execute_and_publish_seconds": time.perf_counter() - execute_started,
                "process_wall_seconds": time.perf_counter() - process_started,
            }
            return result
        execute_seconds = time.perf_counter() - execute_started

        machine_outcome = run_result.get("machine_execution")
        evidence_outcome = run_result.get("evidence_fulfillment")
        delivery_outcome = run_result.get("python_sink_delivery")
        recorder_outcome = run_result.get("recorder_finalization")
        publication_outcome = run_result.get("publication")
        published = run_result.get("published_run")
        execution = (
            machine_outcome.get("result")
            if isinstance(machine_outcome, dict)
            and machine_outcome.get("status") == "completed"
            else None
        )
        evidence = evidence_outcome if isinstance(evidence_outcome, dict) else None
        recorder = (
            recorder_outcome.get("result")
            if isinstance(recorder_outcome, dict)
            and recorder_outcome.get("status") == "finalized"
            else None
        )
        publication = (
            publication_outcome.get("status")
            if isinstance(publication_outcome, dict)
            else None
        )
        channel_rows = evidence.get("channels", []) if evidence else []
        evidence_bytes = evidence.get("budgets", {}).get("observed_logical_bytes") if evidence else None
        if evidence_bytes is None and channel_rows:
            evidence_bytes = sum(
                row.get("logical_bytes", 0)
                for row in channel_rows
                if row.get("channel") != "envelope"
            )
        recorder_manifest = {}
        manifest_path = run_path / "manifest.json"
        if manifest_path.exists():
            recorder_manifest = json.loads(manifest_path.read_text())

        result.update(
            {
                "prepared_observation_identity": recorder_manifest.get(
                    "prepared_observation_id"
                ),
                "outcomes": {
                    "machine_execution": machine_outcome.get("status")
                    if isinstance(machine_outcome, dict)
                    else None,
                    "evidence_fulfillment": evidence.get("status") if evidence else None,
                    "python_sink_delivery": delivery_outcome.get("status")
                    if isinstance(delivery_outcome, dict)
                    else None,
                    "recorder_finalization": recorder_outcome.get("status")
                    if isinstance(recorder_outcome, dict)
                    else None,
                    "publication": publication,
                },
                "execution_result": execution,
                "evidence_receipt": evidence,
                "recorder_receipt": recorder,
                "evidence_logical_bytes": evidence_bytes,
                "recorder_logical_bytes": recorder.get("logical_bytes")
                if isinstance(recorder, dict)
                else None,
                "recorder_encoded_bytes": recorder.get("encoded_bytes")
                if isinstance(recorder, dict)
                else None,
                "recorder_directory_bytes": sum(
                    item.stat().st_size
                    for item in run_path.rglob("*")
                    if item.is_file()
                )
                if run_path.exists()
                else None,
                "memory": {
                    "peak_rss_bytes": int(
                        resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024
                    )
                    if platform.system() == "Linux"
                    else None,
                    "peak_rss_source": "process VmHWM/ru_maxrss; Python worker including extension and recorder",
                    "allocator_high_water_bytes": None,
                },
                "timing": {
                    "preparation_seconds": preparation_seconds,
                    "execute_and_publish_seconds": execute_seconds,
                    "process_wall_seconds": time.perf_counter() - process_started,
                    "rss_before_bytes": int(rss_before * 1024)
                    if platform.system() == "Linux"
                    else None,
                },
            }
        )
    return result


def percentile95(values: list[float]) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[math.ceil(0.95 * len(ordered)) - 1]


def summarize(raw_dir: Path, cases: list[dict]) -> dict:
    rows = [json.loads(path.read_text()) for path in sorted(raw_dir.glob("*.json"))]
    groups: dict[tuple[str, str], list[dict]] = {}
    for row in rows:
        key = (row["trial"]["machine_id"], row["trial"]["policy_id"])
        groups.setdefault(key, []).append(row)
    summary = {
        "result_schema_version": RESULT_SCHEMA_VERSION,
        "trial_count": len(rows),
        "expected_trial_count": len(cases),
        "groups": [],
    }
    for (machine, policy), group in sorted(groups.items()):
        accepted = {
            "machine_execution": {"completed"},
            "evidence_fulfillment": {"complete"},
            "python_sink_delivery": {"not_requested"},
            "recorder_finalization": {"finalized"},
            "publication": {"published"},
        }
        complete = [
            row
            for row in group
            if all(
                row.get("outcomes", {}).get(key) in statuses
                for key, statuses in accepted.items()
            )
        ]
        rss = [row["memory"]["peak_rss_bytes"] for row in complete if row.get("memory", {}).get("peak_rss_bytes") is not None]
        logical = [row["evidence_logical_bytes"] for row in complete if row.get("evidence_logical_bytes") is not None]
        recorder_logical = [row["recorder_logical_bytes"] for row in complete if row.get("recorder_logical_bytes") is not None]
        recorder_encoded = [row["recorder_encoded_bytes"] for row in complete if row.get("recorder_encoded_bytes") is not None]
        recorder_directory = [row["recorder_directory_bytes"] for row in complete if row.get("recorder_directory_bytes") is not None]
        wall = [row["timing"]["process_wall_seconds"] for row in complete if row.get("timing", {}).get("process_wall_seconds") is not None]
        summary["groups"].append(
            {
                "machine_id": machine,
                "policy_id": policy,
                "trial_count": len(group),
                "completed_count": len(complete),
                "outcome_tuples": [row["outcomes"] for row in group],
                "peak_rss_bytes": {
                    "median": statistics.median(rss) if rss else None,
                    "p95": percentile95(rss),
                    "max": max(rss) if rss else None,
                },
                "evidence_logical_bytes": {
                    "median": statistics.median(logical) if logical else None,
                    "p95": percentile95(logical),
                    "max": max(logical) if logical else None,
                    "total": sum(logical),
                },
                "recorder_logical_bytes": {
                    "median": statistics.median(recorder_logical) if recorder_logical else None,
                    "p95": percentile95(recorder_logical),
                    "max": max(recorder_logical) if recorder_logical else None,
                    "total": sum(recorder_logical),
                },
                "recorder_encoded_bytes": {
                    "median": statistics.median(recorder_encoded) if recorder_encoded else None,
                    "p95": percentile95(recorder_encoded),
                    "max": max(recorder_encoded) if recorder_encoded else None,
                    "total": sum(recorder_encoded),
                },
                "recorder_directory_bytes": {
                    "median": statistics.median(recorder_directory) if recorder_directory else None,
                    "p95": percentile95(recorder_directory),
                    "max": max(recorder_directory) if recorder_directory else None,
                    "total": sum(recorder_directory),
                },
                "process_wall_seconds": {
                    "median": statistics.median(wall) if wall else None,
                    "p95": percentile95(wall),
                    "max": max(wall) if wall else None,
                },
            }
        )
    return summary


def run_campaign(args: argparse.Namespace) -> None:
    repo_root = Path(args.machine_root).resolve()
    campaign_path = Path(args.campaign).resolve()
    campaign = tomllib.loads(campaign_path.read_text())
    expected_python = tuple(int(part) for part in campaign["python_version"].split(".")[:2])
    if sys.version_info[:2] != expected_python:
        raise SystemExit(
            f"campaign requires Python {campaign['python_version']}.x; "
            f"running {sys.version_info.major}.{sys.version_info.minor}"
        )
    expected_versions = {
        "glassvm": campaign["glassvm_distribution"].split("==", 1)[1],
        **{
            f"glassvm-{machine}": campaign["machine_distribution_version"]
            for machine in campaign["machine_ids"]
        },
    }
    for distribution, expected in expected_versions.items():
        actual = package_version(distribution)
        if actual != expected:
            raise SystemExit(
                f"expected installed {distribution}=={expected}, found {actual!r}"
            )
    output = Path(args.output_dir).resolve()
    if output.exists() and any(output.iterdir()):
        raise SystemExit(f"result directory must be empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    raw_dir = output / "raw"
    raw_dir.mkdir()

    cohorts = {}
    for machine in campaign["machine_ids"]:
        manifest_path = repo_root / machine / "fixtures/paper"
        if machine != "chip8":
            manifest_path = manifest_path / "ce1"
        cohort = tomllib.loads((manifest_path / "cohort.toml").read_text())
        if cohort["measurement_packages"]["glassvm"] != "0.3.2":
            raise SystemExit(f"unexpected GlassVM pin in {machine} cohort")
        bundle_name = "glassvm_chip8" if machine == "chip8" else f"glassvm_{machine}"
        if cohort["measurement_packages"][bundle_name] != "0.3.3":
            raise SystemExit(f"unexpected bundle pin in {machine} cohort")
        cohorts[machine] = (manifest_path, cohort)

    cases = []
    for machine in campaign["machine_ids"]:
        manifest_path, cohort = cohorts[machine]
        for workload in cohort["workload"]:
            artifact = (
                repo_root / machine / "fixtures" / workload["artifact_path"]
            ).resolve()
            if not artifact.is_file():
                raise SystemExit(f"missing cohort artifact: {artifact}")
            if hashlib.sha256(artifact.read_bytes()).hexdigest() != workload["sha256"]:
                raise SystemExit(f"cohort digest mismatch: {artifact}")
            for policy in campaign["policy"]:
                for repetition in range(1, campaign["repetitions"] + 1):
                    cases.append(
                        {
                            "machine_id": machine,
                            "workload": workload,
                            "cohort": cohort,
                            "cohort_path": str(manifest_path / "cohort.toml"),
                            "policy": policy,
                            "repetition": repetition,
                        }
                    )
    random.Random(campaign["run_order_seed"]).shuffle(cases)
    for index, case in enumerate(cases, start=1):
        case["order_index"] = index
        case["frame_limit"] = campaign["frame_limit"]
        case["host"] = host_metadata()
        case["package_sources"] = {
            "glassvm": campaign["glassvm_source_revision"],
            "glassvm-machines": campaign["machines_source_revision"],
            "glassvm_version": campaign["glassvm_distribution"],
            "bundle_version": campaign["machine_distribution_version"],
            "cohort_checkout_revision": git_metadata(repo_root)["revision"],
        }
        slug = re.sub(r"[^a-z0-9]+", "-", case["workload"]["title"].lower()).strip("-")[:48]
        result_path = raw_dir / (
            f"{case['machine_id']}__{slug}__{case['policy']['id']}__r{case['repetition']:02d}.json"
        )
        command = [
            sys.executable,
            "-I",
            str(Path(__file__).resolve()),
            "--worker",
            json.dumps(case, sort_keys=True),
            "--machine-root",
            str(repo_root),
            "--output",
            str(result_path),
        ]
        started = time.perf_counter()
        completed = subprocess.run(command, text=True, capture_output=True, check=False)
        elapsed = time.perf_counter() - started
        if completed.returncode != 0 or not result_path.exists():
            failure = {
                "result_schema_version": RESULT_SCHEMA_VERSION,
                "trial": {
                    "machine_id": case["machine_id"],
                    "workload_title": case["workload"]["title"],
                    "policy_id": case["policy"]["id"],
                    "repetition": case["repetition"],
                    "order_index": index,
                },
                "outcomes": {
                    "machine_execution": "unknown",
                    "evidence_fulfillment": "unknown",
                    "python_sink_delivery": "unknown",
                    "recorder_finalization": "unknown",
                    "publication": "unknown",
                },
                "failure": {
                    "phase": "worker_process",
                    "returncode": completed.returncode,
                    "stdout": completed.stdout,
                    "stderr": completed.stderr,
                },
                "timing": {"worker_process_seconds": elapsed},
            }
            result_path.write_text(json.dumps(failure, indent=2, sort_keys=True) + "\n")
        else:
            row = json.loads(result_path.read_text())
            row["timing"]["worker_process_seconds"] = elapsed
            result_path.write_text(json.dumps(row, indent=2, sort_keys=True) + "\n")
        row = json.loads(result_path.read_text())
        outcomes = row.get("outcomes", {})
        print(
            f"[{index}/{len(cases)}] {case['machine_id']}/{slug}/"
            f"{case['policy']['id']}/r{case['repetition']}: "
            f"{outcomes.get('machine_execution')} / {outcomes.get('evidence_fulfillment')}"
        , flush=True)

    manifest = {
        "campaign": campaign,
        "host": host_metadata(),
        "source": {
            "machine_checkout": git_metadata(repo_root),
            "installed_package_sources": {
                "glassvm": campaign["glassvm_source_revision"],
                "glassvm-machines": campaign["machines_source_revision"],
            },
        },
        "trial_count": len(cases),
        "trials": [
            {
                "path": str(path.relative_to(output)),
                "trial": json.loads(path.read_text()).get("trial"),
                "outcomes": json.loads(path.read_text()).get("outcomes"),
                "failure": json.loads(path.read_text()).get("failure"),
            }
            for path in sorted(raw_dir.glob("*.json"))
        ],
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    )
    summary = summarize(raw_dir, cases)
    (output / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n"
    )
    print(f"wrote {output / 'manifest.json'} and {output / 'summary.json'}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--campaign", type=Path, default=Path(__file__).with_name("experiment.toml"))
    parser.add_argument("--machine-root", type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--worker")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.worker:
        task = json.loads(args.worker)
        row = trial(task, Path(args.machine_root).resolve())
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(row, indent=2, sort_keys=True) + "\n")
    else:
        if args.output_dir is None:
            parser.error("--output-dir is required")
        run_campaign(args)


if __name__ == "__main__":
    main()
