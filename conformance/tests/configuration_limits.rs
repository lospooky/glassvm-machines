use std::collections::BTreeMap;
use std::sync::Arc;

use chip8_bundle::Chip8Plugin;
use glassvm_core::{
    BundleExecutionLimit, ExecutionControls, ExecutionLimitId, ExecutionRequest, InputSchedule,
    MachineBundle, MachineConfiguration, NullSink, ObservationRequest, SchemaRef, SchemaVersion,
    StepUnit, StructuredValue, TypedStructuredValue,
};
use pico8_bundle::Pico8Plugin;
use tic80_bundle::Tic80Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
}

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
        },
        BundleCase {
            bundle: Arc::new(Pico8Plugin::new()),
            fixture: include_bytes!("../../pico8/fixtures/smoke.rom"),
        },
        BundleCase {
            bundle: Arc::new(Tic80Plugin::new()),
            fixture: include_bytes!("../../tic80/fixtures/smoke.rom"),
        },
    ]
}

fn configuration(
    case: &BundleCase,
    values: BTreeMap<String, StructuredValue>,
) -> MachineConfiguration {
    MachineConfiguration::new(
        case.bundle.emulator().config_schema().schema,
        StructuredValue::Map(values),
    )
    .expect("configuration envelope")
}

fn request(
    case: &BundleCase,
    configuration: MachineConfiguration,
    execution_controls: ExecutionControls,
) -> ExecutionRequest {
    ExecutionRequest::new(
        format!("{}-configuration-limits", case.bundle.descriptor().id),
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        configuration,
        InputSchedule::empty(),
        ObservationRequest::summary(),
        execution_controls,
    )
    .expect("configuration conformance request")
}

fn prepare(
    case: &BundleCase,
    configuration: MachineConfiguration,
    execution_controls: ExecutionControls,
) -> Result<glassvm_core::PreparedRun, String> {
    case.bundle
        .prepare_run(&request(case, configuration, execution_controls))
}

fn execute(case: &BundleCase, execution_controls: ExecutionControls) -> glassvm_core::RunResult {
    let mut request = request(case, default_configuration(case), execution_controls);
    let prepared_run = case
        .bundle
        .prepare_run(&request)
        .unwrap_or_else(|error| panic!("{}: preparation failed: {error}", request.machine_id));
    let prepared_observation = case
        .bundle
        .prepare_observation(&request.observation)
        .unwrap_or_else(|errors| {
            panic!(
                "{}: observation preparation failed: {}",
                request.machine_id,
                errors.join("; ")
            )
        });
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let mut session = case
        .bundle
        .emulator()
        .create_execution_with_prepared_run(
            case.fixture,
            request,
            prepared_run,
            prepared_observation,
        )
        .unwrap_or_else(|error| panic!("session construction failed: {error}"));
    session
        .execute(&mut NullSink)
        .unwrap_or_else(|error| panic!("execution failed: {error}"))
}

fn default_configuration(case: &BundleCase) -> MachineConfiguration {
    MachineConfiguration::defaults(&case.bundle.emulator().config_schema())
        .expect("declared configuration defaults")
}

#[test]
fn defaults_and_partial_submissions_resolve_canonically_for_every_bundle() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let schema = case.bundle.emulator().config_schema();
        schema
            .validate()
            .unwrap_or_else(|error| panic!("{machine}: invalid config schema: {error}"));

        let defaults = prepare(
            &case,
            default_configuration(&case),
            ExecutionControls::default(),
        )
        .unwrap_or_else(|error| panic!("{machine}: defaults failed preparation: {error}"));
        let omitted = prepare(
            &case,
            configuration(&case, BTreeMap::new()),
            ExecutionControls::default(),
        )
        .unwrap_or_else(|error| panic!("{machine}: omitted defaults failed: {error}"));
        assert_eq!(
            defaults.configuration, omitted.configuration,
            "{machine}: explicit and resolved defaults diverged"
        );
        assert_eq!(
            defaults.configuration.schema, schema.schema,
            "{machine}: prepared configuration lost its declared schema"
        );
        assert_eq!(
            defaults.configuration.canonical_bytes, omitted.configuration.canonical_bytes,
            "{machine}: equal effective configuration has unequal canonical bytes"
        );

        let seeded = prepare(
            &case,
            configuration(
                &case,
                BTreeMap::from([("machine_seed".into(), StructuredValue::Unsigned(7))]),
            ),
            ExecutionControls::default(),
        )
        .unwrap_or_else(|error| panic!("{machine}: valid seed failed preparation: {error}"));
        let StructuredValue::Map(values) = &seeded.configuration.value else {
            panic!("{machine}: prepared configuration is not a map");
        };
        assert_eq!(
            values.get("machine_seed"),
            Some(&StructuredValue::Unsigned(7))
        );
        assert_eq!(values.len(), schema.fields.len());
        assert_ne!(
            defaults.identity.canonical_bytes(),
            seeded.identity.canonical_bytes(),
            "{machine}: machine configuration did not affect prepared identity"
        );
    }
}

#[test]
fn wrong_configuration_schema_fails_during_preparation_for_every_bundle() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let wrong = MachineConfiguration::new(
            SchemaRef::new(
                "glassvm.test.wrong-machine-configuration",
                SchemaVersion::V1,
            ),
            StructuredValue::Map(BTreeMap::new()),
        )
        .unwrap();
        let error = prepare(&case, wrong, ExecutionControls::default())
            .expect_err(&format!("{machine}: wrong configuration schema must fail"));
        assert!(
            error.contains("does not match declared schema"),
            "{machine}: {error}"
        );
    }
}

#[test]
fn unknown_fields_wrong_types_and_out_of_range_values_fail_during_preparation() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let invalid = [
            (
                BTreeMap::from([("unknown".into(), StructuredValue::Unsigned(1))]),
                "unknown field",
            ),
            (
                BTreeMap::from([("machine_seed".into(), StructuredValue::Bool(true))]),
                "machine_seed",
            ),
            (
                BTreeMap::from([("machine_seed".into(), StructuredValue::Signed(-1))]),
                "machine_seed",
            ),
        ];
        for (values, fragment) in invalid {
            let error = prepare(
                &case,
                configuration(&case, values),
                ExecutionControls::default(),
            )
            .expect_err(&format!("{machine}: invalid configuration must fail"));
            assert!(
                error.contains(fragment),
                "{machine}: unexpected configuration error: {error}"
            );
        }
    }
}

#[test]
fn common_execution_controls_are_preserved_and_identity_bearing() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let unbounded = prepare(
            &case,
            default_configuration(&case),
            ExecutionControls::default(),
        )
        .unwrap_or_else(|error| panic!("{machine}: omitted controls failed: {error}"));
        let bounded = prepare(
            &case,
            default_configuration(&case),
            ExecutionControls {
                frame_limit: Some(3),
                step_limit: Some(17),
                bundle_limits: Vec::new(),
            },
        )
        .unwrap_or_else(|error| panic!("{machine}: common controls failed: {error}"));

        assert_eq!(bounded.execution_controls.frame_limit, Some(3));
        assert_eq!(bounded.execution_controls.step_limit, Some(17));
        assert!(bounded.execution_controls.bundle_limits.is_empty());
        assert_ne!(
            unbounded.identity.canonical_bytes(),
            bounded.identity.canonical_bytes(),
            "{machine}: common controls did not affect prepared identity"
        );
    }
}

#[test]
fn declared_step_limits_bound_execution_in_each_bundle_step_domain() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let result = execute(
            &case,
            ExecutionControls {
                frame_limit: Some(10),
                step_limit: Some(1),
                bundle_limits: Vec::new(),
            },
        );
        let step = case
            .bundle
            .descriptor()
            .semantics
            .execution_coordinates
            .step
            .as_ref()
            .unwrap_or_else(|| panic!("{machine}: step_limit accepted without a step coordinate"));
        match &step.semantic_unit {
            StepUnit::Instruction | StepUnit::Action | StepUnit::BundleDefined(_) => {
                assert_eq!(
                    result.common.cycles, 1,
                    "{machine}: execution crossed or failed to reach its one-step bound"
                );
                assert_eq!(
                    result.common.frames, 0,
                    "{machine}: a partial machine-work frame was reported complete"
                );
            }
            StepUnit::Tick => {
                assert_eq!(
                    result.common.frames, 1,
                    "{machine}: one tick must complete exactly one callback frame"
                );
            }
            StepUnit::Sample => panic!("{machine}: unexpected sample step domain"),
        }
        assert!(
            result.common.termination.eq_ignore_ascii_case("step_limit"),
            "{machine}: unexpected bounded termination {:?}",
            result.common.termination
        );
    }
}

#[test]
fn bundle_limit_catalogs_are_explicitly_empty_and_reject_nonempty_requests() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let catalog = case.bundle.emulator().execution_limit_catalog();
        catalog
            .validate()
            .unwrap_or_else(|error| panic!("{machine}: invalid limit catalog: {error}"));
        assert!(
            catalog.limits.is_empty(),
            "{machine}: publication decision requires no bundle-specific limits"
        );

        prepare(
            &case,
            default_configuration(&case),
            ExecutionControls::default(),
        )
        .unwrap_or_else(|error| panic!("{machine}: empty bundle-limit set failed: {error}"));

        let limit_id = ExecutionLimitId::new(format!("{}.unsupported-limit", machine.as_str()))
            .expect("canonical unsupported limit ID");
        let controls = ExecutionControls {
            frame_limit: Some(1),
            step_limit: None,
            bundle_limits: vec![BundleExecutionLimit {
                limit_id: limit_id.clone(),
                value: TypedStructuredValue {
                    schema: SchemaRef::new("glassvm.test.limit-value", SchemaVersion::V1),
                    value: StructuredValue::Unsigned(1),
                },
            }],
        };
        let error = prepare(&case, default_configuration(&case), controls)
            .expect_err(&format!("{machine}: unsupported bundle limit must fail"));
        assert!(error.contains("unsupported limit"), "{machine}: {error}");
        assert!(error.contains(limit_id.as_str()), "{machine}: {error}");
    }
}
