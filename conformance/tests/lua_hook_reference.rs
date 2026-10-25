//! The expected projection was generated with unmodified upstream dispatch.
//! Updating it requires running lua_hook_compare.py against a fresh reference
//! build; it must not be regenerated from the optimized implementation alone.

#[allow(dead_code)]
#[path = "../examples/lua_hook_probe.rs"]
mod probe;

use glassvm_core::ContentDigest;
use serde_json::json;

#[test]
fn countdown_dispatch_matches_upstream_hook_semantics() {
    let actual = probe::projection();
    let cases: Vec<_> = actual["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            let bytes = serde_json::to_vec(case).unwrap();
            let sha256 = ContentDigest::sha256(&bytes)
                .value
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            json!({"mask": case["mask"], "quantum": case["quantum"],
               "event_count": case["events"].as_array().unwrap().len(), "sha256": sha256})
        })
        .collect();
    let summary =
        json!({"cases": cases, "exhaustion": actual["exhaustion"], "yielding": actual["yielding"]});
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/lua_hook_reference.json")).unwrap();
    assert_eq!(summary, expected);
}
