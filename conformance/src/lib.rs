//! Cross-bundle conformance and publication-layout tests live in this package
//! because they intentionally span the generic GlassVM repository boundary.

use std::{collections::BTreeSet, fmt};

pub const PUBLICATION_MACHINES: [&str; 5] = ["chip8", "hexwell", "pico8", "tic80", "wyrd16"];

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
            evidence: "all-five preparation tests cover fixtures, bounds, declared alignment, bundle schema binding, structured-format rejection, and total raw-byte domains",
        },
        MaturityDimension::ConfigurationAndLimits => MaturityStatus::Verified {
            evidence: "all-five tests cover canonical defaults, submitted seeds, schema/type/range failures, identity-bearing common controls, exact step-bound enforcement, and explicitly empty bundle-limit catalogs",
        },
        MaturityDimension::RuntimeInputs => MaturityStatus::Open {
            target_slice: "CB-27",
            reason: "schedule admission, InputApplied, and selective input-value evidence are not yet proven uniformly",
        },
        MaturityDimension::AnalyzerVerifierServices => MaturityStatus::Verified {
            evidence: "the public bundle exposes native non-executing static-analyzer and verifier services with accepted and rejected artifact coverage",
        },
        MaturityDimension::ObservationNegotiation => MaturityStatus::Open {
            target_slice: "CB-28",
            reason: "exact per-bundle prerequisite and optional-downgrade coverage is incomplete",
        },
        MaturityDimension::EmissionChannels => MaturityStatus::Open {
            target_slice: "CB-28",
            reason: "requested and omitted channel behavior is not yet proven for every channel and bundle",
        },
        MaturityDimension::DerivedCapabilities => MaturityStatus::Open {
            target_slice: "CB-29",
            reason: "capability depth, exact prerequisites, and bounded reducer evidence are not yet uniform",
        },
        MaturityDimension::FramesAndContinuation => MaturityStatus::Open {
            target_slice: "CB-30",
            reason: "fresh-session continuation and frame evidence invariants need one named all-five gate",
        },
        MaturityDimension::BoundednessAndFailureSeparation => MaturityStatus::Open {
            target_slice: "CB-31",
            reason: "hard-limit and independent failure-domain evidence is incomplete per bundle",
        },
        MaturityDimension::DurablePublication => MaturityStatus::Open {
            target_slice: "CB-32",
            reason: "selective file-backed round trips and atomic publication are not yet named for every bundle",
        },
        MaturityDimension::PythonProvider => MaturityStatus::Open {
            target_slice: "CB-33",
            reason: "the full invalid-preparation and prepare-exactly-once matrix is not yet proven for every provider",
        },
        MaturityDimension::PackagingAndProvenance => MaturityStatus::Open {
            target_slice: "CB-34/CB-35",
            reason: "Rust source coordinates are stale and empty-cache sdist and wheel publication gates remain open",
        },
        MaturityDimension::PaperEvidence => MaturityStatus::Open {
            target_slice: "CB-36/CB-37",
            reason: "all-five documentation and reproducible paper measurements are not complete",
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
