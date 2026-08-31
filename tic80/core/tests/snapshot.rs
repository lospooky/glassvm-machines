use serde_json::{Value, json};
use tic80_core::{RuntimeSnapshot, Tic80Runtime};

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn native_snapshot_restores_hidden_lua_state_by_replay() {
    let mut runtime = Tic80Runtime::new(SMOKE, 7).expect("runtime");
    runtime.tick(1).expect("first frame");
    let checkpoint = runtime.snapshot().expect("checkpoint");
    runtime.tick(2).expect("second frame");
    let expected = runtime.snapshot().expect("expected continuation");

    runtime
        .restore_snapshot(&checkpoint)
        .expect("restore native snapshot");
    assert_eq!(runtime.snapshot().expect("round trip"), checkpoint);
    runtime.tick(2).expect("restored second frame");
    assert_eq!(runtime.snapshot().expect("restored continuation"), expected);
}

#[test]
fn directly_deserialized_native_snapshot_rejects_unknown_fields() {
    let runtime = Tic80Runtime::new(SMOKE, 7).expect("runtime");
    let snapshot = runtime.snapshot().expect("snapshot");
    let mut value: Value = serde_json::to_value(snapshot).expect("snapshot JSON");
    value
        .as_object_mut()
        .expect("snapshot object")
        .insert("unknown".into(), json!(true));
    assert!(
        serde_json::from_value::<RuntimeSnapshot>(value).is_err(),
        "TIC-80 RuntimeSnapshot accepted an unknown field"
    );
}
