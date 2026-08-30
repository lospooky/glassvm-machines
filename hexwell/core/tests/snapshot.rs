use hexwell_core::{Emulator, MachineConfiguration, NativeSnapshot};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn native_snapshot_round_trips() {
    let emulator = Emulator::new(ROM, MachineConfiguration::default()).unwrap();
    let snapshot = emulator.snapshot();
    let encoded = snapshot.to_bytes().unwrap();
    assert_eq!(NativeSnapshot::from_bytes(&encoded).unwrap(), snapshot);
    for path in ["wrapper", "state", "telemetry"] {
        let mut unknown: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        match path {
            "wrapper" => unknown["unknown"] = serde_json::json!(true),
            "state" => unknown["state"]["unknown"] = serde_json::json!(true),
            "telemetry" => unknown["telemetry"]["unknown"] = serde_json::json!(true),
            _ => unreachable!(),
        }
        assert!(NativeSnapshot::from_bytes(&serde_json::to_vec(&unknown).unwrap()).is_err());
    }
}
