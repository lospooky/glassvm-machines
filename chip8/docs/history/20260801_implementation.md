# CHIP-8 bundle implementation

The plugin accepts raw ROM bytes loaded at `0x200`. The canonical
`plugin/tests/fixtures/smoke.rom` clears the display and loops at the entry
point. The public integration test loads that actual file, validates the
bundle, analyzes and verifies it, executes frames, and checks snapshot,
restore, and reset behavior.

Machine-specific source modules remain in the secondary `chip8_core` and
`chip8_verifier` crates. The primary plugin path and public test layout follow
the repository-wide machine-bundle filesystem contract.
