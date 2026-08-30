use wyrd16_core::{Emulator, MachineConfiguration};

#[test]
fn native_snapshot_restores_exact_machine_state() {
    let rom = [0x20, 0x01, 0x70, 0x00];
    let mut emulator = Emulator::new(&rom, MachineConfiguration::default()).expect("emulator");
    emulator.step();
    let snapshot = emulator.snapshot();
    let encoded = snapshot.to_bytes().expect("encode");
    assert_eq!(
        wyrd16_core::NativeSnapshot::from_bytes(&encoded).expect("decode"),
        snapshot
    );
    for path in ["wrapper", "state"] {
        let mut unknown: serde_json::Value = serde_json::from_slice(&encoded).expect("JSON");
        match path {
            "wrapper" => unknown["unknown"] = serde_json::json!(true),
            "state" => unknown["state"]["unknown"] = serde_json::json!(true),
            _ => unreachable!(),
        }
        assert!(
            wyrd16_core::NativeSnapshot::from_bytes(&serde_json::to_vec(&unknown).unwrap())
                .is_err()
        );
    }
    emulator.step();
    emulator.restore(&snapshot);
    assert_eq!(emulator.snapshot(), snapshot);
    emulator.reset();
    assert_ne!(emulator.snapshot(), snapshot);
}
