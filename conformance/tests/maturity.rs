use std::collections::{BTreeMap, BTreeSet};

use glassvm_machine_conformance::{
    MaturityDimension, MaturityEntry, MaturityGateIssue, MaturityStatus, PUBLICATION_MACHINES,
    evaluate_release_gate, maturity_inventory, release_gate,
};

#[test]
fn maturity_inventory_names_every_bundle_and_dimension_once() {
    let inventory = maturity_inventory();
    assert_eq!(
        inventory.len(),
        PUBLICATION_MACHINES.len() * MaturityDimension::ALL.len()
    );

    let mut dimensions_by_machine = BTreeMap::<&str, BTreeSet<MaturityDimension>>::new();
    for entry in &inventory {
        assert!(
            PUBLICATION_MACHINES.contains(&entry.machine),
            "unknown publication bundle in maturity inventory: {}",
            entry.machine
        );
        assert!(
            dimensions_by_machine
                .entry(entry.machine)
                .or_default()
                .insert(entry.dimension),
            "{}: duplicate maturity dimension {}",
            entry.machine,
            entry.dimension.as_str()
        );
    }

    assert_eq!(
        dimensions_by_machine.keys().copied().collect::<Vec<_>>(),
        PUBLICATION_MACHINES
    );
    let expected = MaturityDimension::ALL.into_iter().collect::<BTreeSet<_>>();
    for (machine, dimensions) in dimensions_by_machine {
        assert_eq!(
            dimensions, expected,
            "{machine}: incomplete maturity inventory"
        );
    }
}

#[test]
fn verified_entries_always_name_their_evidence() {
    for entry in maturity_inventory() {
        if let MaturityStatus::Verified { evidence } = entry.status {
            assert!(
                !evidence.trim().is_empty(),
                "{}: {} has an empty verification claim",
                entry.machine,
                entry.dimension.as_str()
            );
        }
    }
}

#[test]
fn current_release_gate_fails_with_bundle_specific_open_gaps() {
    let issues = release_gate().expect_err("open maturity work must block release");
    assert_eq!(issues.len(), 59, "unexpected current maturity-issue count");

    for issue in &issues {
        let MaturityGateIssue::Open(gap) = issue else {
            panic!("current inventory has a structural maturity issue: {issue}");
        };
        let rendered = issue.to_string();
        assert!(rendered.contains(gap.machine), "{rendered}");
        assert!(rendered.contains(gap.dimension.as_str()), "{rendered}");
        assert!(rendered.contains(gap.target_slice), "{rendered}");
        assert!(!gap.reason.trim().is_empty(), "{rendered}");
    }

    assert!(!issues.iter().any(|issue| {
        matches!(
            issue,
            MaturityGateIssue::Open(gap)
                if gap.machine == "pico8"
                    && gap.dimension == MaturityDimension::AnalyzerVerifierServices
        )
    }));
    assert!(issues.iter().any(|issue| {
        matches!(
            issue,
            MaturityGateIssue::Open(gap)
                if gap.machine == "tic80"
                    && gap.dimension == MaturityDimension::AnalyzerVerifierServices
                    && gap.target_slice == "CB-24"
        )
    }));
}

#[test]
fn release_gate_rejects_empty_duplicate_unknown_and_evidence_free_inventories() {
    let empty_issues = evaluate_release_gate(&[]).expect_err("empty inventory must fail");
    assert_eq!(
        empty_issues.len(),
        PUBLICATION_MACHINES.len() * MaturityDimension::ALL.len()
    );
    assert!(
        empty_issues
            .iter()
            .all(|issue| matches!(issue, MaturityGateIssue::MissingEntry { .. }))
    );

    let malformed = vec![
        MaturityEntry {
            machine: "chip8",
            dimension: MaturityDimension::DeclaredMachineSemantics,
            status: MaturityStatus::Verified { evidence: "" },
        },
        MaturityEntry {
            machine: "chip8",
            dimension: MaturityDimension::DeclaredMachineSemantics,
            status: MaturityStatus::Verified {
                evidence: "duplicate",
            },
        },
        MaturityEntry {
            machine: "unknown",
            dimension: MaturityDimension::DeclaredMachineSemantics,
            status: MaturityStatus::Verified {
                evidence: "not accepted",
            },
        },
    ];
    let malformed_issues =
        evaluate_release_gate(&malformed).expect_err("malformed inventory must fail");
    assert!(
        malformed_issues
            .iter()
            .any(|issue| matches!(issue, MaturityGateIssue::EmptyEvidence { .. }))
    );
    assert!(
        malformed_issues
            .iter()
            .any(|issue| matches!(issue, MaturityGateIssue::DuplicateEntry { .. }))
    );
    assert!(
        malformed_issues
            .iter()
            .any(|issue| matches!(issue, MaturityGateIssue::UnknownMachine { .. }))
    );
}

#[test]
fn release_gate_accepts_only_an_inventory_without_open_entries() {
    let complete = PUBLICATION_MACHINES
        .into_iter()
        .flat_map(|machine| {
            MaturityDimension::ALL
                .into_iter()
                .map(move |dimension| MaturityEntry {
                    machine,
                    dimension,
                    status: MaturityStatus::Verified {
                        evidence: "test evidence",
                    },
                })
        })
        .collect::<Vec<_>>();

    evaluate_release_gate(&complete).expect("fully verified inventory must pass");
}
