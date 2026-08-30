#!/usr/bin/env python3
"""Build the complete deterministic Hexwell catalyst plate offline."""

import argparse
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.write_bytes(bytes(range(256)))


if __name__ == "__main__":
    main()
