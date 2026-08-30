#!/usr/bin/env python3
"""Rebuild the original Wyrd-16 smoke ROM without network access."""

from argparse import ArgumentParser
from pathlib import Path


PROGRAM = bytes.fromhex("100a1114b013c0100001")


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    arguments.output.write_bytes(PROGRAM)


if __name__ == "__main__":
    main()
