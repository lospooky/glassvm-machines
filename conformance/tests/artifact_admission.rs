use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    ArtifactEncoding, ExecutionControls, ExecutionRequest, InputSchedule, MachineBundle,
    MachineConfiguration, MachineId, ObservationRequest,
};
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
    malformed: Vec<u8>,
    malformed_fragment: &'static str,
    unsupported: Option<(Vec<u8>, &'static str)>,
}

fn tic80_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
}

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
            malformed: Vec::new(),
            malformed_fragment: "minimum is 1",
            unsupported: None,
        },
        BundleCase {
            bundle: Arc::new(Pico8Plugin::new()),
            fixture: include_bytes!("../../pico8/fixtures/smoke.rom"),
            malformed: b"\x89PNG\r\n\x1a\ntruncated".to_vec(),
            malformed_fragment: "invalid PICO-8 cartridge",
            unsupported: Some((
                b"not a PICO-8 cartridge".to_vec(),
                "unsupported PICO-8 artifact",
            )),
        },
        BundleCase {
            bundle: Arc::new(Tic80Plugin::new()),
            fixture: include_bytes!("../../tic80/fixtures/smoke.rom"),
            malformed: b"bad".to_vec(),
            malformed_fragment: "invalid TIC-80 cartridge",
            unsupported: Some((
                tic80_cart("// script: javascript\nfunction TIC() {}"),
                "unsupported TIC-80 cartridge language",
            )),
        },
    ]
}

fn request(case: &BundleCase, artifact: Vec<u8>, machine_id: MachineId) -> ExecutionRequest {
    ExecutionRequest::new(
        format!("{}-artifact-admission", case.bundle.descriptor().id),
        machine_id,
        artifact,
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema())
            .expect("bundle configuration defaults"),
        InputSchedule::empty(),
        ObservationRequest::summary(),
        ExecutionControls {
            frame_limit: Some(1),
            ..ExecutionControls::default()
        },
    )
    .expect("artifact-admission request envelope")
}

fn prepare(case: &BundleCase, artifact: Vec<u8>) -> Result<glassvm_core::PreparedRun, String> {
    let request = request(case, artifact, case.bundle.descriptor().id.clone());
    case.bundle.prepare_run(&request)
}

#[test]
fn named_fixtures_prepare_with_the_selected_bundle_schema() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let prepared = prepare(&case, case.fixture.to_vec())
            .unwrap_or_else(|error| panic!("{machine}: fixture admission failed: {error}"));
        assert_eq!(
            prepared.identity.artifact_identity.schema,
            case.bundle.contract().artifact.schema,
            "{machine}: preparation must bind the bundle artifact schema"
        );
        assert_eq!(
            prepared.identity.artifact_identity.encoding,
            case.bundle.contract().artifact.encoding,
            "{machine}: preparation must bind the bundle artifact encoding"
        );
        assert_eq!(
            prepared.identity.artifact_identity.canonical_value, case.fixture,
            "{machine}: prepared artifact bytes changed"
        );
    }
}

#[test]
fn malformed_and_below_minimum_artifacts_fail_during_preparation() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let error = prepare(&case, case.malformed.clone())
            .expect_err(&format!("{machine}: malformed artifact must fail"));
        assert!(
            error.contains(case.malformed_fragment),
            "{machine}: unexpected malformed-artifact error: {error}"
        );
    }
}

#[test]
fn every_bundle_rejects_artifacts_above_its_declared_hard_limit() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let oversized = vec![0; case.bundle.contract().artifact.max_bytes + 1];
        let error = prepare(&case, oversized)
            .expect_err(&format!("{machine}: oversized artifact must fail"));
        assert!(
            error.contains("maximum is"),
            "{machine}: oversized artifact bypassed declared bounds: {error}"
        );
    }
}

#[test]
fn structured_formats_reject_unsupported_artifacts_and_raw_domains_remain_total() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        if let Some((artifact, fragment)) = &case.unsupported {
            let error = prepare(&case, artifact.clone())
                .expect_err(&format!("{machine}: unsupported format must fail"));
            assert!(
                error.contains(fragment),
                "{machine}: unexpected unsupported-format error: {error}"
            );
            assert!(
                matches!(
                    case.bundle.contract().artifact.encoding,
                    ArtifactEncoding::Extension(_)
                ),
                "{machine}: structured rejection must have a declared extended encoding"
            );
        } else {
            assert_eq!(
                case.bundle.contract().artifact.encoding,
                ArtifactEncoding::RawBytes,
                "{machine}: only raw-byte domains may omit unsupported-format specimens"
            );
            let admissible = vec![0xff; case.bundle.contract().artifact.min_bytes];
            prepare(&case, admissible)
                .unwrap_or_else(|error| panic!("{machine}: raw-byte domain is not total: {error}"));
        }
    }
}

#[test]
fn declared_alignment_is_enforced_without_inventing_byte_alignment_failures() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let spec = &case.bundle.contract().artifact;
        if spec.alignment_bytes == 1 {
            continue;
        }
        let misaligned = vec![0; spec.min_bytes + 1];
        let error = prepare(&case, misaligned)
            .expect_err(&format!("{machine}: misaligned artifact must fail"));
        assert!(
            error.contains("not aligned"),
            "{machine}: unexpected alignment error: {error}"
        );
    }
}

#[test]
fn a_different_machine_contract_cannot_prepare_the_artifact_identity() {
    let cases = cases();
    for (index, case) in cases.iter().enumerate() {
        let machine = &case.bundle.descriptor().id;
        let other = cases[(index + 1) % cases.len()]
            .bundle
            .descriptor()
            .id
            .clone();
        let request = request(case, case.fixture.to_vec(), other.clone());
        let error = case
            .bundle
            .prepare_run(&request)
            .expect_err(&format!("{machine}: wrong machine contract must fail"));
        assert!(error.contains(other.as_str()), "{machine}: {error}");
        assert!(error.contains(machine.as_str()), "{machine}: {error}");
    }
}
