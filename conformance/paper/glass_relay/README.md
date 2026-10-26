# Glass Relay program specimens

These nine purpose-built cartridges implement the same press/hold/release
protocol on CHIP-8, PICO-8, and TIC-80. They are paper experiment assets; machine
implementations and their public contracts are not modified.

All programs are original repository-authored code under BSD-3-Clause.
[`build_cartridges.py`](build_cartridges.py) is their authoritative source. It
contains the small CHIP-8 assembler/label resolution and Lua source templates.
Outputs live under each machine's `fixtures/relay/` directory. CHIP-8 listings,
PICO-8 text cartridges, and TIC-80 Lua source make the programs inspectable.
TIC-80 binaries use the native uncompressed code-chunk representation.

```sh
python3 conformance/paper/glass_relay/build_cartridges.py
python3 conformance/paper/glass_relay/build_cartridges.py --check
python3 conformance/paper/glass_relay/build_cartridges.py --output-root /tmp/rebuilt-relay
```

`artifacts.json` records the generated paths, byte lengths, source license,
variant descriptions, and fixture digests. No network or third-party compiler
is needed. Paper assets have their own manifest rather than entering the
ordinary smoke-fixture inventories.

The reference programs draw a stable four-pixel marker while input 0 is held
and restore the blank display on release. The delayed programs wait three
semantic frame ticks on press; CHIP-8 uses DT and the Lua machines count
callbacks. The stuck programs intentionally omit restoration after release.
These are deliberate cartridge behaviors, not injected emulator defects.

The caller submits the current desired input state every frame. This supports
both the latched CHIP-8/PICO-8 key/button inputs and TIC-80's frame-scoped full
gamepad mask. The CHIP-8 configuration supplies 128 instructions per frame,
giving its small polling loop time to react within an acceptance frame.

The common protocol, exact configurations/input mappings, generic downstream
Python tester, and comparison reports live on GlassVM's `paper-publication`
branch under `docs/paper/experiments/glass_relay/` and `glass_relay.py`.
