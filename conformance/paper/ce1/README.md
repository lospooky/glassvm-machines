# CE1 five-policy cohort

CE1 compares evidence selection over three pinned, zero-input workloads per
machine. The five policies and fixed run settings are in `experiment.toml`;
artifact provenance, machine configuration, and selected normalized event
kinds are in each machine's `fixtures/paper[/ce1]/cohort.toml`.

The selected-event policies reuse the established paper workload event set for
that machine. The all-event policies request every normalized event kind
declared by that bundle. Neither policy requests native evidence, capabilities,
snapshots, or input-value evidence. Frame hashes and full frame artifacts are
requested independently of normalized `FrameCompleted` selection.

Each trial runs in its own Python process using the published GlassVM 0.3.2
facade and machine bundle 0.3.3. The Rust reference recorder finalizes a run
for each trial; the runner records evidence and recorder receipts and measures
the temporary recording directory, then removes that directory. Raw JSON
results retain the five outcome domains separately. No recorder data is used
to reconstruct emissions.

From the repository root, create an isolated environment and install the
published selector:

```sh
uv venv --python 3.14 /tmp/glassvm-ce1-venv
uv pip install --python /tmp/glassvm-ce1-venv/bin/python 'glassvm-machines[all]==0.3.3'
/tmp/glassvm-ce1-venv/bin/python conformance/paper/ce1/run_ce1.py \
  --output-dir conformance/paper/ce1/results-0.3.3-20261109
```

The output directory must be absent or empty. The runner verifies every pinned
artifact digest before execution, randomizes trial order with the recorded
seed, and writes one raw record per trial plus a manifest and summary. The
three-repetition matrix contains 135 trials. A separate preflight record is
not part of those 135 trials.
