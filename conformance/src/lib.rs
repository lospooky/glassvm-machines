//! Cross-bundle conformance and publication-layout tests live in this package
//! because they intentionally span the generic GlassVM repository boundary.

use std::{collections::BTreeSet, fmt};

pub const PUBLICATION_MACHINES: [&str; 3] = ["chip8", "pico8", "tic80"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaturityDimension {
    DeclaredMachineSemantics,
    ArtifactAdmission,
    ConfigurationAndLimits,
    RuntimeInputs,
    AnalyzerVerifierServices,
    ObservationNegotiation,
    EmissionChannels,
    DerivedCapabilities,
    FramesAndContinuation,
    BoundednessAndFailureSeparation,
    DurablePublication,
    PythonProvider,
    PackagingAndProvenance,
    PaperEvidence,
}

impl MaturityDimension {
    pub const ALL: [Self; 14] = [
        Self::DeclaredMachineSemantics,
        Self::ArtifactAdmission,
        Self::ConfigurationAndLimits,
        Self::RuntimeInputs,
        Self::AnalyzerVerifierServices,
        Self::ObservationNegotiation,
        Self::EmissionChannels,
        Self::DerivedCapabilities,
        Self::FramesAndContinuation,
        Self::BoundednessAndFailureSeparation,
        Self::DurablePublication,
        Self::PythonProvider,
        Self::PackagingAndProvenance,
        Self::PaperEvidence,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeclaredMachineSemantics => "declared-machine-semantics",
            Self::ArtifactAdmission => "artifact-admission",
            Self::ConfigurationAndLimits => "configuration-and-limits",
            Self::RuntimeInputs => "runtime-inputs",
            Self::AnalyzerVerifierServices => "analyzer-verifier-services",
            Self::ObservationNegotiation => "observation-negotiation",
            Self::EmissionChannels => "emission-channels",
            Self::DerivedCapabilities => "derived-capabilities",
            Self::FramesAndContinuation => "frames-and-continuation",
            Self::BoundednessAndFailureSeparation => "boundedness-and-failure-separation",
            Self::DurablePublication => "durable-publication",
            Self::PythonProvider => "python-provider",
            Self::PackagingAndProvenance => "packaging-and-provenance",
            Self::PaperEvidence => "paper-evidence",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaturityStatus {
    Verified {
        evidence: &'static str,
    },
    Open {
        target_slice: &'static str,
        reason: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaturityEntry {
    pub machine: &'static str,
    pub dimension: MaturityDimension,
    pub status: MaturityStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaturityGap {
    pub machine: &'static str,
    pub dimension: MaturityDimension,
    pub target_slice: &'static str,
    pub reason: &'static str,
}

impl fmt::Display for MaturityGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} remains open for {}: {}",
            self.machine,
            self.dimension.as_str(),
            self.target_slice,
            self.reason
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaturityGateIssue {
    Open(MaturityGap),
    UnknownMachine {
        machine: &'static str,
    },
    MissingEntry {
        machine: &'static str,
        dimension: MaturityDimension,
    },
    DuplicateEntry {
        machine: &'static str,
        dimension: MaturityDimension,
    },
    EmptyEvidence {
        machine: &'static str,
        dimension: MaturityDimension,
    },
}

impl fmt::Display for MaturityGateIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(gap) => gap.fmt(formatter),
            Self::UnknownMachine { machine } => {
                write!(
                    formatter,
                    "{machine}: unknown publication bundle in maturity inventory"
                )
            }
            Self::MissingEntry { machine, dimension } => write!(
                formatter,
                "{machine}: missing maturity entry for {}",
                dimension.as_str()
            ),
            Self::DuplicateEntry { machine, dimension } => write!(
                formatter,
                "{machine}: duplicate maturity entry for {}",
                dimension.as_str()
            ),
            Self::EmptyEvidence { machine, dimension } => write!(
                formatter,
                "{machine}: verified maturity entry has no evidence for {}",
                dimension.as_str()
            ),
        }
    }
}

fn status(dimension: MaturityDimension) -> MaturityStatus {
    match dimension {
        MaturityDimension::DeclaredMachineSemantics => MaturityStatus::Verified {
            evidence: "versioned descriptor and bundle support documentation",
        },
        MaturityDimension::ArtifactAdmission => MaturityStatus::Verified {
            evidence: "three publication-bundle preparation tests cover fixtures, bounds, declared alignment, bundle schema binding, structured-format rejection, and total raw-byte domains",
        },
        MaturityDimension::ConfigurationAndLimits => MaturityStatus::Verified {
            evidence: "three publication-bundle tests cover canonical defaults, submitted seeds, schema/type/range failures, identity-bearing common controls, exact step-bound enforcement, and explicitly empty bundle-limit catalogs",
        },
        MaturityDimension::RuntimeInputs => MaturityStatus::Verified {
            evidence: "three publication-bundle tests cover canonical schedule ordering, pre-execution schedule rejection, InputApplied source identity, selectively negotiated input-value evidence, and live-input admission where advertised",
        },
        MaturityDimension::AnalyzerVerifierServices => MaturityStatus::Verified {
            evidence: "the public bundle exposes native non-executing static-analyzer and verifier services with accepted and rejected artifact coverage",
        },
        MaturityDimension::ObservationNegotiation => MaturityStatus::Verified {
            evidence: "three publication-bundle tests cover exact native schemas/kinds, normalized event kinds, frame prerequisites, required failure, optional downgrade, and shared/transitive dependency closure",
        },
        MaturityDimension::EmissionChannels => MaturityStatus::Verified {
            evidence: "three publication-bundle executions independently select and omit normalized, native, frame, snapshot, and input-value evidence channels without cross-channel leakage",
        },
        MaturityDimension::DerivedCapabilities => MaturityStatus::Verified {
            evidence: "three publication-bundle capability audits prove canonical outputs and receipts, exact reducer or native prerequisites, bounded online summaries, and hash/full visual-summary invariance",
        },
        MaturityDimension::FramesAndContinuation => MaturityStatus::Verified {
            evidence: "three publication-bundle frame/continuation tests prove independent frame artifacts, matching frame/step coordinates, history-free state snapshots, fresh-session continuation, and explicit incompatible-version rejection",
        },
        MaturityDimension::BoundednessAndFailureSeparation => MaturityStatus::Verified {
            evidence: "three publication-bundle budget gates prove hard normalized/native/frame/snapshot limits, oversized records are not forwarded, incomplete evidence remains separate from successful machine execution, and core capability-byte accounting is covered by regression test",
        },
        MaturityDimension::DurablePublication => MaturityStatus::Verified {
            evidence: "three publication-bundle FileRunSession tests prove selective normalized/frame/snapshot round trips, omitted channels remain absent, finalized recorder and evidence receipts agree, and publication appears only at the atomic published path",
        },
        MaturityDimension::PythonProvider => MaturityStatus::Verified {
            evidence: "the canonical facade protocol and clean-room harness cover entry-point discovery, opaque prepared state, structured four-domain results, missing-provider errors, and the identical FileRunSession lifecycle across the three publication providers",
        },
        MaturityDimension::PackagingAndProvenance => MaturityStatus::Open {
            target_slice: "CB-34/CB-35",
            reason: "Python bundle distributions remain unpublished to PyPI and empty-cache source-distribution and wheel publication gates remain open",
        },
        MaturityDimension::PaperEvidence => MaturityStatus::Open {
            target_slice: "CB-36/CB-37",
            reason: "three-machine documentation and reproducible paper measurements are not complete",
        },
    }
}

pub fn maturity_inventory() -> Vec<MaturityEntry> {
    PUBLICATION_MACHINES
        .into_iter()
        .flat_map(|machine| {
            MaturityDimension::ALL
                .into_iter()
                .map(move |dimension| MaturityEntry {
                    machine,
                    dimension,
                    status: status(dimension),
                })
        })
        .collect()
}

pub fn evaluate_release_gate(entries: &[MaturityEntry]) -> Result<(), Vec<MaturityGateIssue>> {
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in entries {
        if !PUBLICATION_MACHINES.contains(&entry.machine) {
            issues.push(MaturityGateIssue::UnknownMachine {
                machine: entry.machine,
            });
            continue;
        }
        if !seen.insert((entry.machine, entry.dimension)) {
            issues.push(MaturityGateIssue::DuplicateEntry {
                machine: entry.machine,
                dimension: entry.dimension,
            });
        }
        match entry.status {
            MaturityStatus::Verified { evidence } if evidence.trim().is_empty() => {
                issues.push(MaturityGateIssue::EmptyEvidence {
                    machine: entry.machine,
                    dimension: entry.dimension,
                });
            }
            MaturityStatus::Verified { .. } => {}
            MaturityStatus::Open {
                target_slice,
                reason,
            } => issues.push(MaturityGateIssue::Open(MaturityGap {
                machine: entry.machine,
                dimension: entry.dimension,
                target_slice,
                reason,
            })),
        }
    }

    for machine in PUBLICATION_MACHINES {
        for dimension in MaturityDimension::ALL {
            if !seen.contains(&(machine, dimension)) {
                issues.push(MaturityGateIssue::MissingEntry { machine, dimension });
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn release_gate() -> Result<(), Vec<MaturityGateIssue>> {
    evaluate_release_gate(&maturity_inventory())
}
