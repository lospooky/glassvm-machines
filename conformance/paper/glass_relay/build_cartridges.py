#!/usr/bin/env python3
"""Reproduce Glass Relay's nine purpose-built cartridges without dependencies."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
VARIANTS = ("reference", "delayed", "stuck")


def chip8(variant: str) -> tuple[bytes, str]:
    """Assemble a polling loop; DT supplies the delayed variant's frame delay."""
    words: list[tuple[str | None, int | str, str]] = []

    def op(value: int | str, note: str, label: str | None = None) -> None:
        words.append((label, value, note))

    op(0x00E0, "clear display")
    op(0x6000, "V0 = key 0")
    op(0x6100, "V1 = marker visible")
    op(0x6208, "V2 = x")
    op(0x6308, "V3 = y")
    op(0x6600, "V6 = pending response")
    op("i:sprite", "I = four-pixel marker")
    op(0xE09E, "skip next jump if key 0 held", "loop")
    op("jp:released", "key released")
    op(0x3100, "skip next jump if marker absent")
    op("jp:loop", "marker already visible")
    if variant == "delayed":
        op(0x3600, "skip next jump if response not pending")
        op("jp:wait", "poll delay timer")
        op(0x6403, "delay = three frame ticks")
        op(0xF415, "DT = delay")
        op(0x6601, "response pending")
        op("jp:loop", "wait for subsequent timer ticks")
        op(0xF407, "V4 = DT", "wait")
        op(0x3400, "skip next jump if delay elapsed")
        op("jp:loop", "keep waiting")
    op(0xD231, "draw marker once (avoid XOR flicker)")
    op(0x6101, "marker visible")
    op("jp:loop", "poll input")
    if variant == "stuck":
        op("jp:loop", "deliberate defect: retain marker after release", "released")
    else:
        op(0x6600, "cancel pending response", "released")
        op(0x3101, "skip next jump if marker visible")
        op("jp:loop", "display already clear")
        op(0x00E0, "restore original blank display")
        op(0x6100, "marker absent")
        op("jp:loop", "poll input")
    labels = {label: 0x200 + 2 * index for index, (label, _, _) in enumerate(words) if label}
    labels["sprite"] = 0x200 + 2 * len(words)
    data = bytearray()
    listing = [f"; Glass Relay {variant}; original repository-authored specimen"]
    for index, (label, value, note) in enumerate(words):
        if isinstance(value, str):
            kind, target = value.split(":")
            value = (0x1000 if kind == "jp" else 0xA000) | labels[target]
        data.extend(value.to_bytes(2, "big"))
        listing.append(f"{0x200 + index * 2:03x}  {value:04x}  ; {label + ': ' if label else ''}{note}")
    data.append(0xF0)
    listing.append(f"{labels['sprite']:03x}  f0    ; sprite")
    return bytes(data), "\n".join(listing) + "\n"


def lua(variant: str, machine: str) -> str:
    delay = 3 if variant == "delayed" else 0
    release = "-- deliberate defect: leave shown unchanged" if variant == "stuck" else "shown = false"
    update = f"""  local down = btn(0)
  if down then
    if not was_down then remaining = {delay} end
    if remaining > 0 then
      remaining = remaining - 1
    else
      shown = true
    end
  else
    {release}
    remaining = 0
  end
  was_down = down
"""
    header = f"-- Glass Relay {variant}; original repository-authored specimen\nshown = false\nwas_down = false\nremaining = 0\n"
    if machine == "pico8":
        return header + "function _update60()\n" + update + "end\nfunction _draw()\n  cls(0)\n  if shown then rectfill(8, 8, 11, 8, 7) end\nend\n"
    return "-- script: lua\n" + header + "function TIC()\n" + update + "  cls(0)\n  if shown then rect(8, 8, 4, 1, 15) end\nend\n"


def build_files() -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    artifacts = []
    for machine in ("chip8", "pico8", "tic80"):
        for variant in VARIANTS:
            prefix = f"{machine}/fixtures/relay/{variant}"
            if machine == "chip8":
                artifact, listing = chip8(variant)
                files[f"{prefix}.listing"] = listing.encode()
                path = f"{prefix}.ch8"
            elif machine == "pico8":
                source = lua(variant, machine)
                artifact = ("pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\n" + source).encode()
                path = f"{prefix}.p8"
            else:
                source = lua(variant, machine).encode()
                files[f"{prefix}.lua"] = source
                # TIC-80 native code chunk: type/bank, little-endian length, reserved.
                artifact = bytes([5]) + len(source).to_bytes(2, "little") + bytes([0]) + source
                path = f"{prefix}.tic"
            files[path] = artifact
            artifacts.append({"machine_id": machine, "variant": variant, "path": path,
                              "bytes": len(artifact), "sha256": hashlib.sha256(artifact).hexdigest()})
    manifest = {"schema": "glass-relay.artifacts", "version": 1, "license": "BSD-3-Clause",
                "variants": {"reference": "prompt response and restoration",
                             "delayed": "deliberate three-frame press response delay",
                             "stuck": "deliberate failure to clear after release"},
                "artifacts": artifacts}
    files["conformance/paper/glass_relay/artifacts.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-root", type=Path, default=ROOT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    for relative, data in build_files().items():
        path = args.output_root / relative
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                raise SystemExit(f"cartridge/source differs from builder: {path}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    print("Glass Relay cartridge sources and artifacts match" if args.check else "Built nine Glass Relay cartridges")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
