# TIC-80 smoke cartridge

`smoke.rom` is the native binary `.tic` cartridge
`lincerely/gecko@f958fd19218510468a36f93b77449ca26a2d13cf:game.tic`, copied
unchanged. It is a playable Lua cartridge with code and graphics assets and is
used to exercise the real `.tic` parser, verifier, Lua runtime, framebuffer,
reset, and snapshot lifecycle. The `.rom` suffix is the repository-wide
canonical fixture name. The upstream repository is MIT-licensed;
`licenses/gecko-MIT.txt` preserves its license alongside the fixture.

`source/public/game.tic` is the byte-identical upstream input retained under
its upstream filename for an offline deterministic rebuild. Rebuild the
canonical conformance fixture with:

```sh
python3 build_fixture.py --output smoke.rom
```

The paper workload is separately stored as `paper/game.tic`, with its native
extension and upstream filename. It uses the same pinned no-input Gecko
cartridge; `.rom` remains only the existing conformance-fixture name.

Upstream: <https://github.com/lincerely/gecko>

```text
smoke.rom SHA-256:
f095a8ad534482a3aefbe0aa84c9b8bc47b74a89dd4202acbd5fc8b517995994

licenses/gecko-MIT.txt SHA-256:
9ed80fbc12842a1cf7e4dc03c6037437233e0df6257b94584bc8c566b44684bf
```

`source/upstream-luademo.lua` is copied verbatim from
`nesbox/TIC-80@4aba09c98f1e5028b82765be1647677b08d35942:demos/luademo.lua`.
It retains the upstream MIT notice and exercises the source-to-cart unit-test
path. The TIC-80 project license is also preserved in the machine reference
snapshot.
