#!/usr/bin/env python3
"""Rebuild the canonical PICO-8 smoke artifact without network access."""

from argparse import ArgumentParser
from pathlib import Path


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    source = Path(__file__).parent / "source" / "smoke.p8"
    arguments.output.write_bytes(source.read_bytes())


if __name__ == "__main__":
    main()
