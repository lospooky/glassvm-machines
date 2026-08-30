use hexwell_core::{Artifact, WELL_COUNT};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn checked_in_plate_loads() {
    assert_eq!(Artifact::parse(ROM).unwrap().bytes().len(), WELL_COUNT);
}
