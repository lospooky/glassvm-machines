use std::collections::BTreeSet;
use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    AccessDetail, CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityRequest,
    CostClass, Emission, EmissionSink, EventKind, EventSelection, ExecutionControls,
    ExecutionRequest, FrameCapture, InputCoordinate, InputId, InputSchedule, InputValueSelector,
    MachineBundle, MachineConfiguration, NormalizedAccessRequirement, NormalizerCatalog,
    ObservationRequest, PreparedCapabilityStatus, ScheduledInput, SchemaRef, SchemaVersion,
    SinkError, SnapshotCapture, StructuredValue, TypedInputPayload,
};
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
    native_capability: &'static str,
    native_kind: &'static str,
    native_schema: &'static str,
    semantic_capability: &'static str,
    semantic_event_kinds: Vec<(&'static str, EventKind)>,
    semantic_requires_frames: bool,
    input_id: &'static str,
    input_schema: &'static str,
    input_value: StructuredValue,
    input_summary: &'static str,
}

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
            native_capability: "chip8.execution_summary",
            native_kind: "chip8.execution_summary",
            native_schema: "chip8.execution_summary",
            semantic_capability: "chip8.visual_trajectory_motifs",
            semantic_event_kinds: vec![
                ("frame_completed", EventKind::FrameCompleted),
                ("display_write", EventKind::DisplayWrite),
                ("input_sampled", EventKind::InputSampled),
            ],
            semantic_requires_frames: true,
            input_id: "chip8.key.0",
            input_schema: "chip8.input.key",
            input_value: StructuredValue::Bool(true),
            input_summary: "chip8.input_summary",
        },
        BundleCase {
            bundle: Arc::new(HexwellPlugin::new()),
            fixture: include_bytes!("../../hexwell/fixtures/smoke.rom"),
            native_capability: "hexwell.reaction_dynamics",
            native_kind: "hexwell.reaction_dynamics",
            native_schema: "hexwell.observation",
            semantic_capability: "hexwell.reaction_field_motifs",
            semantic_event_kinds: vec![
                (
                    "extension:hexwell.sweep_committed",
                    EventKind::Extension("hexwell.sweep_committed".into()),
                ),
                (
                    "extension:hexwell.catalyst_fired",
                    EventKind::Extension("hexwell.catalyst_fired".into()),
                ),
                (
                    "extension:hexwell.tide_feed",
                    EventKind::Extension("hexwell.tide_feed".into()),
                ),
                ("input_sampled", EventKind::InputSampled),
                ("frame_completed", EventKind::FrameCompleted),
            ],
            semantic_requires_frames: false,
            input_id: "hexwell.tide",
            input_schema: "hexwell.input.tide",
            input_value: StructuredValue::Unsigned(0x41),
            input_summary: "hexwell.input_summary",
        },
        BundleCase {
            bundle: Arc::new(Wyrd16Plugin::new()),
            fixture: include_bytes!("../../wyrd16/fixtures/smoke.rom"),
            native_capability: "wyrd16.canvas",
            native_kind: "wyrd16.canvas",
            native_schema: "wyrd16.observation",
            semantic_capability: glassvm_core::standard_capabilities::CONTROL_FLOW_MOTIFS,
            semantic_event_kinds: vec![
                ("instruction_decoded", EventKind::InstructionDecoded),
                ("branch_taken", EventKind::BranchTaken),
                ("call", EventKind::Call),
                ("return", EventKind::Return),
                ("interrupt", EventKind::Interrupt),
                ("trap", EventKind::Trap),
            ],
            semantic_requires_frames: false,
            input_id: "wyrd16.key.0",
            input_schema: "wyrd16.input.key",
            input_value: StructuredValue::Bool(true),
            input_summary: "wyrd16.input_summary",
        },
        BundleCase {
            bundle: Arc::new(Pico8Plugin::new()),
            fixture: include_bytes!("../../pico8/fixtures/smoke.rom"),
            native_capability: "pico8.execution_summary",
            native_kind: "pico8.execution_summary",
            native_schema: "pico8.native_event",
            semantic_capability: "pico8.visual.motifs",
            semantic_event_kinds: vec![("frame_completed", EventKind::FrameCompleted)],
            semantic_requires_frames: true,
            input_id: "pico8.button.0",
            input_schema: "pico8.input.button",
            input_value: StructuredValue::Bool(true),
            input_summary: "pico8.input_summary",
        },
        BundleCase {
            bundle: Arc::new(Tic80Plugin::new()),
            fixture: include_bytes!("../../tic80/fixtures/smoke.rom"),
            native_capability: "tic80.execution_summary",
            native_kind: "tic80.execution_summary",
            native_schema: "tic80.native_event",
            semantic_capability: "tic80.visual.motifs",
            semantic_event_kinds: vec![
                ("frame_completed", EventKind::FrameCompleted),
                ("input_sampled", EventKind::InputSampled),
                (
                    "extension:tic80.trace",
                    EventKind::Extension("tic80.trace".into()),
                ),
            ],
            semantic_requires_frames: true,
            input_id: "tic80.gamepad",
            input_schema: "tic80.input.gamepad",
            input_value: StructuredValue::Unsigned(1),
            input_summary: "tic80.input_summary",
        },
    ]
}

fn typed_payload(case: &BundleCase) -> TypedInputPayload {
    TypedInputPayload::new(
        SchemaRef::new(case.input_schema, SchemaVersion::V1),
        case.input_value.clone(),
    )
    .expect("typed input payload")
}

fn one_input(case: &BundleCase) -> InputSchedule {
    InputSchedule {
        schema: glassvm_core::input_schedule_schema(),
        entries: vec![ScheduledInput {
            ordinal: 0,
            coordinate: InputCoordinate::frame(0),
            input_id: InputId::new(case.input_id).unwrap(),
            payload: typed_payload(case),
        }],
    }
}

fn request(
    case: &BundleCase,
    observation: ObservationRequest,
    input_schedule: InputSchedule,
) -> ExecutionRequest {
    ExecutionRequest::new(
        format!("{}-observation-channels", case.bundle.descriptor().id),
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema()).unwrap(),
        input_schedule,
        observation,
        ExecutionControls {
            frame_limit: Some(2),
            step_limit: None,
            bundle_limits: Vec::new(),
        },
    )
    .expect("observation conformance request")
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ChannelCounts {
    normalized: usize,
    native: usize,
    frames: usize,
    snapshots: usize,
    input_values: usize,
}

impl EmissionSink for ChannelCounts {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::Event(_) => self.normalized += 1,
            Emission::NativeEvidence(_) => self.native += 1,
            Emission::Frame(_) => self.frames += 1,
            Emission::Snapshot(_) => self.snapshots += 1,
            Emission::InputValueEvidence(_) => self.input_values += 1,
            _ => {}
        }
        Ok(())
    }
}

fn execute(
    case: &BundleCase,
    observation: ObservationRequest,
    schedule: InputSchedule,
) -> ChannelCounts {
    let mut request = request(case, observation, schedule);
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
    let mut counts = ChannelCounts::default();
    session.execute(&mut counts).unwrap();
    counts
}

fn assert_only(machine: &str, counts: &ChannelCounts, selected: &str) {
    let values = [
        ("normalized", counts.normalized),
        ("native", counts.native),
        ("frames", counts.frames),
        ("snapshots", counts.snapshots),
        ("input_values", counts.input_values),
    ];
    for (channel, count) in values {
        if channel == selected {
            assert!(count > 0, "{machine}: selected {channel} channel was empty");
        } else {
            assert_eq!(count, 0, "{machine}: omitted {channel} channel emitted");
        }
    }
}

#[test]
fn requested_and_omitted_channels_are_independent_for_every_bundle() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.to_string();
        let omitted = execute(&case, ObservationRequest::summary(), InputSchedule::empty());
        assert_eq!(omitted, ChannelCounts::default(), "{machine}");

        let mut normalized = ObservationRequest::summary();
        normalized.normalized_events.events = EventSelection::All;
        assert_only(
            &machine,
            &execute(&case, normalized, InputSchedule::empty()),
            "normalized",
        );

        let mut native = ObservationRequest::summary();
        native.native_evidence.enabled = true;
        native.native_evidence.all = true;
        assert_only(
            &machine,
            &execute(&case, native, InputSchedule::empty()),
            "native",
        );

        let mut frames = ObservationRequest::summary();
        frames.frames.capture = FrameCapture::Hashes;
        assert_only(
            &machine,
            &execute(&case, frames, InputSchedule::empty()),
            "frames",
        );

        let mut snapshots = ObservationRequest::summary();
        snapshots.snapshots.capture = SnapshotCapture::Final;
        assert_only(
            &machine,
            &execute(&case, snapshots, InputSchedule::empty()),
            "snapshots",
        );

        let mut input_values = ObservationRequest::summary();
        input_values.input_value_evidence.enabled = true;
        input_values.capabilities = vec![CapabilityRequest::required(
            CapabilityId::new(case.input_summary).unwrap(),
        )];
        assert_only(
            &machine,
            &execute(&case, input_values, one_input(&case)),
            "input_values",
        );
    }
}

#[test]
fn required_missing_prerequisites_fail_and_optional_roots_downgrade() {
    for case in cases() {
        let id = CapabilityId::new(case.input_summary).unwrap();
        let mut required = ObservationRequest::summary();
        required.capabilities = vec![CapabilityRequest::required(id.clone())];
        let errors = case.bundle.prepare_observation(&required).unwrap_err();
        assert!(
            errors.iter().any(|error| {
                error.contains(id.as_str()) && error.contains("input-value evidence channel")
            }),
            "{}: {errors:?}",
            case.bundle.descriptor().id
        );

        let mut optional = ObservationRequest::summary();
        optional.capabilities = vec![CapabilityRequest::optional(id.clone())];
        let prepared = case.bundle.prepare_observation(&optional).unwrap();
        let capability = prepared
            .capabilities
            .iter()
            .find(|capability| capability.id == id)
            .unwrap();
        assert!(capability.requested);
        assert!(!capability.required);
        assert_eq!(capability.status, PreparedCapabilityStatus::Unavailable);
        assert!(
            capability
                .message
                .as_deref()
                .is_some_and(|message| message.contains("input-value evidence channel"))
        );
        assert!(prepared.normalizer.requests.is_empty());
    }
}

#[test]
fn native_capabilities_require_their_exact_selected_kind_and_schema() {
    for case in cases() {
        let id = CapabilityId::new(case.native_capability).unwrap();
        let mut missing = ObservationRequest::summary();
        missing.capabilities = vec![CapabilityRequest::required(id.clone())];
        let errors = case.bundle.prepare_observation(&missing).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("native evidence channel")),
            "{}: {errors:?}",
            case.bundle.descriptor().id
        );

        let mut wrong_kind = missing.clone();
        wrong_kind.native_evidence.enabled = true;
        wrong_kind.native_evidence.kinds = vec!["glassvm.test.wrong-native-kind".into()];
        let errors = case.bundle.prepare_observation(&wrong_kind).unwrap_err();
        assert!(
            errors.iter().any(|error| error.contains(case.native_kind)),
            "{}: {errors:?}",
            case.bundle.descriptor().id
        );

        let mut complete = missing;
        complete.native_evidence.enabled = true;
        complete.native_evidence.kinds = vec![case.native_kind.into()];
        let prepared = case.bundle.prepare_observation(&complete).unwrap();
        assert_eq!(
            prepared.native_observation_schemas,
            vec![case.native_schema.to_owned()],
            "{}",
            case.bundle.descriptor().id
        );
    }
}

#[test]
fn semantic_capabilities_require_every_declared_event_kind_and_frame_mode() {
    for case in cases() {
        let id = CapabilityId::new(case.semantic_capability).unwrap();
        let mut complete = ObservationRequest::summary();
        complete.capabilities = vec![CapabilityRequest::required(id.clone())];
        complete.normalized_events.events = EventSelection::Kinds(
            case.semantic_event_kinds
                .iter()
                .map(|(_, kind)| kind.clone())
                .collect(),
        );
        if case.semantic_requires_frames {
            complete.frames.capture = FrameCapture::Hashes;
        }
        case.bundle
            .prepare_observation(&complete)
            .unwrap_or_else(|errors| {
                panic!("{}: {}", case.bundle.descriptor().id, errors.join("; "))
            });

        for (omitted_key, omitted) in &case.semantic_event_kinds {
            let mut selected = case
                .semantic_event_kinds
                .iter()
                .map(|(_, kind)| kind.clone())
                .collect::<BTreeSet<_>>();
            selected.remove(omitted);
            let mut missing_kind = complete.clone();
            missing_kind.normalized_events.events = EventSelection::Kinds(selected);
            let errors = case.bundle.prepare_observation(&missing_kind).unwrap_err();
            assert!(
                errors.iter().any(|error| error.contains(omitted_key)),
                "{} missing {}: {errors:?}",
                case.bundle.descriptor().id,
                omitted_key
            );
        }

        if case.semantic_requires_frames {
            let mut missing_frames = complete;
            missing_frames.frames.capture = FrameCapture::None;
            let errors = case
                .bundle
                .prepare_observation(&missing_frames)
                .unwrap_err();
            assert!(
                errors.iter().any(|error| error.contains("frame capture")),
                "{}: {errors:?}",
                case.bundle.descriptor().id
            );
        }
    }
}

#[test]
fn wyrd16_state_motifs_require_reads_writes_diffs_and_before_after_values() {
    let bundle = Wyrd16Plugin::new();
    let id = CapabilityId::new(glassvm_core::standard_capabilities::MEMORY_STATE_MOTIFS).unwrap();
    let descriptor = bundle.normalizer_catalog().find(&id).unwrap().clone();
    assert!(
        descriptor
            .dependencies
            .contains(&CapabilityDependency::NormalizedState {
                reads: true,
                writes: true,
                state_diffs: true,
                access: NormalizedAccessRequirement::BeforeAndAfter,
            })
    );

    let mut complete = ObservationRequest::summary();
    complete.capabilities = vec![CapabilityRequest::required(id)];
    complete.normalized_events.events = EventSelection::All;
    complete.normalized_events.access_detail = AccessDetail::BeforeAndAfter;
    complete.normalized_events.state_diffs = true;
    bundle.prepare_observation(&complete).unwrap();

    let mut no_reads_or_writes = complete.clone();
    no_reads_or_writes.normalized_events.access_detail = AccessDetail::None;
    no_reads_or_writes.normalized_events.state_diffs = false;
    assert!(bundle.prepare_observation(&no_reads_or_writes).is_err());

    let mut after_only = complete.clone();
    after_only.normalized_events.access_detail = AccessDetail::AfterOnly;
    assert!(bundle.prepare_observation(&after_only).is_err());

    let mut no_diffs = complete;
    no_diffs.normalized_events.state_diffs = false;
    assert!(bundle.prepare_observation(&no_diffs).is_err());
}

fn append_dependency_diamond(case: &BundleCase) -> (NormalizerCatalog, [CapabilityId; 4]) {
    let prefix = format!("glassvm.test.{}.observation", case.bundle.descriptor().id);
    let leaf = CapabilityId::new(format!("{prefix}.leaf")).unwrap();
    let left = CapabilityId::new(format!("{prefix}.left")).unwrap();
    let right = CapabilityId::new(format!("{prefix}.right")).unwrap();
    let root = CapabilityId::new(format!("{prefix}.root")).unwrap();
    let descriptor = |id: &CapabilityId, dependencies| CapabilityDescriptor {
        schema: id.schema(),
        output_type: "json.object".into(),
        dependencies,
        cost_class: CostClass::Bounded,
    };
    let mut catalog = case.bundle.normalizer_catalog();
    catalog.capabilities.extend([
        descriptor(
            &leaf,
            vec![CapabilityDependency::InputValueEvidence {
                selectors: vec![InputValueSelector::InputId(
                    InputId::new(case.input_id).unwrap(),
                )],
            }],
        ),
        descriptor(&left, vec![CapabilityDependency::Capability(leaf.clone())]),
        descriptor(&right, vec![CapabilityDependency::Capability(leaf.clone())]),
        descriptor(
            &root,
            vec![
                CapabilityDependency::Capability(left.clone()),
                CapabilityDependency::Capability(right.clone()),
            ],
        ),
    ]);
    (catalog, [leaf, left, right, root])
}

#[test]
fn shared_transitive_dependencies_resolve_once_without_optional_escalation() {
    for case in cases() {
        let (catalog, ids) = append_dependency_diamond(&case);
        let root = ids[3].clone();

        let mut optional = ObservationRequest::summary();
        optional.capabilities = vec![CapabilityRequest::optional(root.clone())];
        let prepared = glassvm_core::PreparedObservation::prepare_with_input_catalog(
            &catalog,
            &case.bundle.contract().inputs,
            &optional,
        )
        .unwrap();
        assert_eq!(prepared.capabilities.len(), 4);
        assert!(prepared.capabilities.iter().all(|capability| {
            capability.status == PreparedCapabilityStatus::Unavailable && !capability.required
        }));
        assert!(prepared.normalizer.requests.is_empty());

        let mut required = optional.clone();
        required.capabilities = vec![CapabilityRequest::required(root.clone())];
        assert!(
            glassvm_core::PreparedObservation::prepare_with_input_catalog(
                &catalog,
                &case.bundle.contract().inputs,
                &required,
            )
            .is_err()
        );

        let mut available = optional;
        available.input_value_evidence.enabled = true;
        let prepared = glassvm_core::PreparedObservation::prepare_with_input_catalog(
            &catalog,
            &case.bundle.contract().inputs,
            &available,
        )
        .unwrap();
        assert!(prepared.capabilities.iter().all(|capability| {
            capability.status == PreparedCapabilityStatus::Available && !capability.required
        }));
        assert_eq!(
            prepared.input_value_ids,
            vec![InputId::new(case.input_id).unwrap()]
        );
        assert_eq!(prepared.normalizer.requests.len(), 1);
        assert_eq!(prepared.normalizer.requests[0].id, root);
    }
}
