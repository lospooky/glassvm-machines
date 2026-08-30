#!/usr/bin/env python3
"""Rebuild the canonical CHIP-8 smoke ROM without network access."""

from argparse import ArgumentParser
from pathlib import Path


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    source = Path(__file__).parent / "source" / "smoke.hex"
    encoded = "".join(source.read_text(encoding="ascii").split())
    arguments.output.write_bytes(bytes.fromhex(encoded))


if __name__ == "__main__":
    main()
