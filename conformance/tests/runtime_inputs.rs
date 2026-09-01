use std::collections::BTreeSet;
use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    CapabilityId, CapabilityRequest, Emission, EmissionSink, EventKind, EventSelection,
    ExecutionControls, ExecutionEvent, ExecutionRequest, InputApplied, InputCoordinate,
    InputDeliveryMode, InputId, InputInstanceId, InputSchedule, InputSource, InputValueEvidence,
    MachineBundle, MachineConfiguration, ObservationRequest, ScheduledInput, SchemaRef,
    SchemaVersion, SinkError, StructuredValue, TypedInputPayload,
};
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
    input_id: &'static str,
    input_schema: &'static str,
    first_value: StructuredValue,
    second_value: StructuredValue,
    input_summary: &'static str,
}

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
            input_id: "chip8.key.0",
            input_schema: "chip8.input.key",
            first_value: StructuredValue::Bool(true),
            second_value: StructuredValue::Bool(false),
            input_summary: "chip8.input_summary",
        },
        BundleCase {
            bundle: Arc::new(HexwellPlugin::new()),
            fixture: include_bytes!("../../hexwell/fixtures/smoke.rom"),
            input_id: "hexwell.tide",
            input_schema: "hexwell.input.tide",
            first_value: StructuredValue::Unsigned(0x41),
            second_value: StructuredValue::Unsigned(0x02),
            input_summary: "hexwell.input_summary",
        },
        BundleCase {
            bundle: Arc::new(Wyrd16Plugin::new()),
            fixture: include_bytes!("../../wyrd16/fixtures/smoke.rom"),
            input_id: "wyrd16.key.0",
            input_schema: "wyrd16.input.key",
            first_value: StructuredValue::Bool(true),
            second_value: StructuredValue::Bool(false),
            input_summary: "wyrd16.input_summary",
        },
        BundleCase {
            bundle: Arc::new(Pico8Plugin::new()),
            fixture: include_bytes!("../../pico8/fixtures/smoke.rom"),
            input_id: "pico8.button.0",
            input_schema: "pico8.input.button",
            first_value: StructuredValue::Bool(true),
            second_value: StructuredValue::Bool(false),
            input_summary: "pico8.input_summary",
        },
        BundleCase {
            bundle: Arc::new(Tic80Plugin::new()),
            fixture: include_bytes!("../../tic80/fixtures/smoke.rom"),
            input_id: "tic80.gamepad",
            input_schema: "tic80.input.gamepad",
            first_value: StructuredValue::Unsigned(1),
            second_value: StructuredValue::Unsigned(2),
            input_summary: "tic80.input_summary",
        },
    ]
}

fn payload(schema: &str, value: StructuredValue) -> TypedInputPayload {
    TypedInputPayload::new(SchemaRef::new(schema, SchemaVersion::V1), value)
        .expect("typed input payload")
}

fn entry(case: &BundleCase, ordinal: u64, value: StructuredValue) -> ScheduledInput {
    ScheduledInput {
        ordinal,
        coordinate: InputCoordinate::frame(0),
        input_id: InputId::new(case.input_id).unwrap(),
        payload: payload(case.input_schema, value),
    }
}

fn request(
    case: &BundleCase,
    schedule: InputSchedule,
    observation: ObservationRequest,
) -> ExecutionRequest {
    ExecutionRequest::new(
        format!("{}-runtime-inputs", case.bundle.descriptor().id),
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema()).unwrap(),
        schedule,
        observation,
        ExecutionControls {
            frame_limit: Some(2),
            step_limit: None,
            bundle_limits: Vec::new(),
        },
    )
    .expect("runtime-input conformance request")
}

fn input_observation(case: &BundleCase, with_values: bool) -> ObservationRequest {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events =
        EventSelection::Kinds(BTreeSet::from([EventKind::InputApplied]));
    observation.input_value_evidence.enabled = true;
    if with_values {
        observation.capabilities = vec![CapabilityRequest::required(
            CapabilityId::new(case.input_summary).unwrap(),
        )];
    }
    observation
}

#[derive(Default)]
struct InputSink {
    events: Vec<ExecutionEvent>,
    values: Vec<InputValueEvidence>,
    capability_ids: Vec<String>,
}

impl EmissionSink for InputSink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::Event(event) if event.kind == EventKind::InputApplied => {
                self.events.push(event.clone());
            }
            Emission::InputValueEvidence(evidence) => self.values.push(evidence.clone()),
            Emission::RunFinished(result) => {
                self.capability_ids.extend(
                    result
                        .capabilities
                        .iter()
                        .map(|output| output.schema.id.as_str().to_owned()),
                );
            }
            _ => {}
        }
        Ok(())
    }
}

fn execute(case: &BundleCase, schedule: InputSchedule, with_values: bool) -> InputSink {
    let mut request = request(case, schedule, input_observation(case, with_values));
    let prepared_observation = case
        .bundle
        .prepare_observation(&request.observation)
        .unwrap_or_else(|errors| panic!("{}: {}", request.machine_id, errors.join("; ")));
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = case.bundle.prepare_run(&request).unwrap();
    let mut session = case
        .bundle
        .emulator()
        .create_execution_with_prepared_run(
            case.fixture,
            request,
            prepared_run,
            prepared_observation,
        )
        .unwrap();
    let mut sink = InputSink::default();
    session.execute(&mut sink).unwrap();
    sink
}

#[test]
fn schedules_resolve_by_ordinal_independently_of_caller_order() {
    for case in cases() {
        let schedule = InputSchedule {
            schema: glassvm_core::input_schedule_schema(),
            entries: vec![
                entry(&case, 9, case.second_value.clone()),
                entry(&case, 3, case.first_value.clone()),
            ],
        };
        let prepared = case
            .bundle
            .prepare_run(&request(&case, schedule, ObservationRequest::summary()))
            .unwrap();
        assert_eq!(
            prepared
                .input_schedule
                .entries
                .iter()
                .map(|entry| entry.ordinal)
                .collect::<Vec<_>>(),
            vec![3, 9],
            "{}: caller order leaked into the prepared schedule",
            case.bundle.descriptor().id
        );
    }
}

#[test]
fn invalid_schedules_fail_during_preparation_for_every_bundle() {
    for case in cases() {
        let machine = &case.bundle.descriptor().id;
        let valid = entry(&case, 0, case.first_value.clone());
        let invalid = [
            (
                vec![ScheduledInput {
                    input_id: InputId::new("glassvm.test.undeclared").unwrap(),
                    ..valid.clone()
                }],
                "undeclared input",
            ),
            (
                vec![ScheduledInput {
                    payload: payload("glassvm.test.wrong-input", case.first_value.clone()),
                    ..valid.clone()
                }],
                "does not match declared schema",
            ),
            (vec![valid.clone(), valid.clone()], "repeats ordinal"),
            (
                vec![ScheduledInput {
                    coordinate: InputCoordinate::step(0),
                    ..valid
                }],
                "coordinate",
            ),
        ];
        for (entries, expected) in invalid {
            let mut invalid_request =
                request(&case, InputSchedule::empty(), ObservationRequest::summary());
            invalid_request.input_schedule = InputSchedule {
                schema: glassvm_core::input_schedule_schema(),
                entries,
            };
            let error = case
                .bundle
                .prepare_run(&invalid_request)
                .expect_err(&format!("{machine}: invalid schedule must fail"));
            assert!(error.contains(expected), "{machine}: {error}");
        }
    }
}

#[test]
fn application_events_and_selected_values_share_source_identity() {
    for case in cases() {
        let schedule = InputSchedule {
            schema: glassvm_core::input_schedule_schema(),
            entries: vec![
                entry(&case, 9, case.second_value.clone()),
                entry(&case, 3, case.first_value.clone()),
            ],
        };
        let sink = execute(&case, schedule, true);
        assert_eq!(sink.events.len(), 2, "{}", case.bundle.descriptor().id);
        assert_eq!(sink.values.len(), 2, "{}", case.bundle.descriptor().id);
        assert!(sink.capability_ids.contains(&case.input_summary.to_owned()));

        for (index, ordinal) in [3, 9].into_iter().enumerate() {
            let applied: InputApplied = serde_json::from_value(
                sink.events[index].extensions["glassvm.input_applied"].clone(),
            )
            .unwrap();
            let source = InputSource::Scheduled { ordinal };
            assert_eq!(applied.input_id.as_str(), case.input_id);
            assert_eq!(applied.input_schema.id, case.input_schema);
            assert_eq!(applied.source, source);
            assert_eq!(applied.application_coordinate, InputCoordinate::frame(0));
            assert_eq!(sink.values[index].source, source);
            assert_eq!(
                sink.values[index].application_coordinate,
                applied.application_coordinate
            );
            assert_eq!(sink.values[index].input_id, applied.input_id);
            assert_eq!(sink.values[index].input_schema, applied.input_schema);
        }
    }
}

#[test]
fn enabling_the_value_channel_without_a_selector_emits_no_values() {
    for case in cases() {
        let sink = execute(
            &case,
            InputSchedule {
                schema: glassvm_core::input_schedule_schema(),
                entries: vec![entry(&case, 0, case.first_value.clone())],
            },
            false,
        );
        assert_eq!(sink.events.len(), 1, "{}", case.bundle.descriptor().id);
        assert!(sink.values.is_empty(), "{}", case.bundle.descriptor().id);
    }
}

#[test]
fn live_delivery_is_advertised_and_scoped_only_by_chip8() {
    for case in cases() {
        let advertises_live = case
            .bundle
            .contract()
            .inputs
            .inputs
            .iter()
            .any(|input| input.delivery_modes.contains(&InputDeliveryMode::Live));
        assert_eq!(
            advertises_live,
            case.bundle.live_input_capability().is_some(),
            "{}: live declaration/service mismatch",
            case.bundle.descriptor().id
        );
        if case.bundle.descriptor().id.as_str() != "chip8" {
            assert!(!advertises_live);
        }
    }

    let case = cases().remove(0);
    let prepared = case
        .bundle
        .prepare_run(&request(
            &case,
            InputSchedule::empty(),
            ObservationRequest::summary(),
        ))
        .unwrap();
    let mut controller = case
        .bundle
        .live_input_capability()
        .unwrap()
        .create_controller(&prepared)
        .unwrap();
    let id = InputId::new(case.input_id).unwrap();
    let first = controller
        .apply(
            id.clone(),
            payload(case.input_schema, StructuredValue::Bool(true)),
        )
        .unwrap();
    assert_eq!(first.instance_id, InputInstanceId::new(0));
    assert_eq!(first.application_coordinate, InputCoordinate::frame(0));

    controller
        .apply(
            InputId::new("chip8.key.missing").unwrap(),
            payload(case.input_schema, StructuredValue::Bool(true)),
        )
        .expect_err("rejected input must not consume an instance ID");
    let second = controller
        .apply(
            id.clone(),
            payload(case.input_schema, StructuredValue::Bool(false)),
        )
        .unwrap();
    assert_eq!(second.instance_id, InputInstanceId::new(1));

    let applied = second.input_applied(
        id.clone(),
        SchemaRef::new(case.input_schema, SchemaVersion::V1),
    );
    let evidence = second.input_value_evidence(
        id,
        SchemaRef::new(case.input_schema, SchemaVersion::V1),
        payload(case.input_schema, StructuredValue::Bool(false)),
    );
    assert_eq!(applied.source, evidence.source);
    assert_eq!(
        applied.application_coordinate,
        evidence.application_coordinate
    );
}
