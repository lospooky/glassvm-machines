#!/usr/bin/env python3
"""Build the paper PICO-8 workload from its pinned public cartridge source."""

from argparse import ArgumentParser
from pathlib import Path


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    source = Path(__file__).parent / "source/public/fireintro.p8"
    cart = source.read_text(encoding="utf-8")

    # The Lua parser rejects the upstream P8SCII title escapes. Retain the fire
    # update and drawing logic, changing only this decorative title to ASCII.
    upstream_title = 'print("\\^w\\^tfire intro",25,30,7)'
    portable_title = 'print("fire intro",25,30,7)'
    if cart.count(upstream_title) != 1:
        raise ValueError("pinned fire-intro source title no longer matches the recorded adaptation")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        cart.replace(upstream_title, portable_title), encoding="utf-8", newline="\n"
    )


if __name__ == "__main__":
    main()
