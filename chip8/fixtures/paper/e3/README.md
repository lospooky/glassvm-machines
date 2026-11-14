# CHIP-8 E3 cohort

This pinned cohort contains 30 CHIP-8 programs selected from the pinned
John Earnest chip8Archive revision recorded in cohort.json. The selection
rule is license-based: all 30 screen-eligible entries marked CC0-1.0 were
included; the cohort was not ranked by observed behavior. The other seven of
37 screen-eligible entries were excluded because their metadata did not carry
the selected CC0 marker. Eight additional screened candidates stopped while
waiting for input and were ineligible under the empty-input protocol.

All 30 completed the 600-frame empty-input screen with complete requested
frame evidence and no fault. The screen is an eligibility check, not a
performance measurement or a claim that the cohort represents all CHIP-8
programs. The manifest preserves exact source URLs, authors as recorded by
the upstream registry, hashes, license basis, and per-member screen outcomes.

The upstream archive states that repository contents are released under CC0.
These ROMs remain third-party artifacts and are not covered by the machine
repository's BSD-3-Clause code license. See ATTRIBUTION.md and the
source-linked evidence in cohort.json.

Two programs also occur in the CE1 cohort. Their existing files under
../horseWorldOnline.ch8 and ../octojam2title.ch8 are reused; no additional
copies are made for E3. The other 28 ROMs are stored once under this e3/
directory.

Verify all 30 pinned artifacts and the cohort invariants offline with:

    python3 chip8/fixtures/verify_e3_cohort.py
