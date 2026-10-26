#!/usr/bin/env python3
"""Copy the pinned public CHIP-8 cartridge to the paper artifact location."""

from argparse import ArgumentParser
from pathlib import Path


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    source = Path(__file__).parent / "source/public/octojam2title.ch8"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(source.read_bytes())


if __name__ == "__main__":
    main()
