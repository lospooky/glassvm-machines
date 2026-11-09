#!/usr/bin/env python3
"""Copy the pinned CE1 source artifacts to their canonical paper paths."""
from pathlib import Path

root = Path(__file__).parent
for source in sorted((root / "source/ce1").glob("*.tic")):
    target = root / "paper/ce1" / source.name
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(source.read_bytes())
