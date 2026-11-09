//! E1 F2-D: exercise a shared dependency diamond against each bundle's input catalog.
use glassvm_core::{
    CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityRequest, CostClass,
    InputId, InputValueSelector, MachineBundle, ObservationRequest, PreparedCapabilityStatus,
    PreparedObservation,
};
use serde_json::json;
use std::sync::Arc;

fn main() {
    let machine = std::env::args().nth(1).expect("machine id");
    let bundle: Arc<dyn MachineBundle> = match machine.as_str() {
        "chip8" => Arc::new(chip8_bundle::Chip8Plugin::new()),
        "pico8" => Arc::new(pico8_bundle::Pico8Plugin::new()),
        "tic80" => Arc::new(tic80_bundle::Tic80Plugin::new()),
        _ => panic!("unknown machine"),
    };
    let input = match machine.as_str() {
        "chip8" => "chip8.key.0",
        "pico8" => "pico8.button.0",
        _ => "tic80.gamepad",
    };
    let id = |suffix: &str| CapabilityId::new(format!("glassvm.test.{machine}.{suffix}")).unwrap();
    let (leaf, left, right, root) = (id("leaf"), id("left"), id("right"), id("root"));
    let descriptor = |id: &CapabilityId, dependencies| CapabilityDescriptor {
        schema: id.schema(),
        output_type: "json.object".into(),
        dependencies,
        cost_class: CostClass::Bounded,
    };
    let mut catalog = bundle.normalizer_catalog();
    catalog.capabilities.extend([
        descriptor(
            &leaf,
            vec![CapabilityDependency::InputValueEvidence {
                selectors: vec![InputValueSelector::InputId(InputId::new(input).unwrap())],
            }],
        ),
        descriptor(&left, vec![CapabilityDependency::Capability(leaf.clone())]),
        descriptor(&right, vec![CapabilityDependency::Capability(leaf.clone())]),
        descriptor(
            &root,
            vec![
                CapabilityDependency::Capability(left),
                CapabilityDependency::Capability(right),
            ],
        ),
    ]);
    let mut optional = ObservationRequest::summary();
    optional.capabilities = vec![CapabilityRequest::optional(root.clone())];
    let missing = PreparedObservation::prepare_with_input_catalog(
        &catalog,
        &bundle.contract().inputs,
        &optional,
    )
    .expect("optional missing prerequisite must prepare");
    assert_eq!(missing.capabilities.len(), 4);
    assert!(missing.capabilities.iter().all(|c| !c.required
        && c.status == PreparedCapabilityStatus::Unavailable
        && c.message.is_some()));
    let mut required = optional.clone();
    required.capabilities = vec![CapabilityRequest::required(root.clone())];
    let errors = PreparedObservation::prepare_with_input_catalog(
        &catalog,
        &bundle.contract().inputs,
        &required,
    )
    .expect_err("required missing prerequisite must fail");
    let mut complete = optional.clone();
    complete.input_value_evidence.enabled = true;
    let available = PreparedObservation::prepare_with_input_catalog(
        &catalog,
        &bundle.contract().inputs,
        &complete,
    )
    .expect("complete prerequisite must prepare");
    assert_eq!(available.capabilities.len(), 4);
    assert!(
        available
            .capabilities
            .iter()
            .all(|c| !c.required && c.status == PreparedCapabilityStatus::Available)
    );
    assert_eq!(
        available.input_value_ids,
        vec![InputId::new(input).unwrap()]
    );
    assert_eq!(available.normalizer.requests.len(), 1);
    println!("{}",serde_json::to_string(&json!({"case":"F2-D","machine":machine,"scope":"synthetic capability diamond over the real bundle input catalog; preparation only", "optional_request":optional,"optional_prepared":missing,"required_request":required,"required_errors":errors,"complete_request":complete,"complete_prepared":available})).unwrap());
}
