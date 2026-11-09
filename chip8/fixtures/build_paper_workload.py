#!/usr/bin/env python3
"""Copy a pinned public CHIP-8 ROM into the paper fixture directory."""

from argparse import ArgumentParser
from pathlib import Path


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument(
        "--source",
        type=Path,
        help="source ROM relative to this fixture directory (defaults to source/public/<output-name>)",
    )
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    fixture_root = Path(__file__).parent
    source = args.source or Path("source/public") / args.output.name
    if not source.is_absolute():
        source = fixture_root / source
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(source.read_bytes())


if __name__ == "__main__":
    main()
