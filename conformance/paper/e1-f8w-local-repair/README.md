# E1 F8-W: recorder write failure

Three independent processes per machine used the public Rust file-run
lifecycle with a one-byte recorder block limit. All nine sessions completed
execution and evidence delivery, reported recorder failure, and did not publish.
The runner uses the smoke fixtures and records the full request, receipt and
outcomes. Raw JSON files and the manifest contain fixture and binary hashes.
Temporary staged output was listed and then removed.

To reproduce one run from the pinned machine and GlassVM revisions:

```sh
cargo build -p glassvm_machine_conformance --example e1_recorder_write_failure --locked
mkdir -p /tmp/glassvm-f8w-example
target/debug/examples/e1_recorder_write_failure chip8 /tmp/glassvm-f8w-example/published
```

Use a fresh temporary directory for each machine and repetition.
