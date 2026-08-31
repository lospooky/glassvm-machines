use tic80_verifier::{analyze_bytes, verify_bytes};

fn lua_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
}

fn assert_missing_callback(source: &str) {
    let cart = lua_cart(source);
    assert!(
        !analyze_bytes(&cart)
            .expect("non-executing analysis")
            .has_tic_callback,
        "source must not supply callback evidence: {source:?}"
    );
    assert!(
        verify_bytes(&cart)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing_tic_callback"),
        "source must fail callback verification: {source:?}"
    );
}

#[test]
fn rejects_lua_that_names_tic_but_does_not_compile() {
    let cart = lua_cart("function TIC( end");
    assert!(
        !analyze_bytes(&cart)
            .expect("non-executing malformed analysis")
            .has_tic_callback
    );
    let report = verify_bytes(&cart);
    assert_eq!(report.severity_max(), "error");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lua_compile_error")
    );
}

#[test]
fn quoted_and_commented_near_matches_are_not_callback_evidence() {
    for source in [
        r#"message = "function TIC() end""#,
        "message = 'TIC = function() end'",
        "-- function TIC() end",
        "-- TIC = function() end\nvalue = 1",
        "--[[ function TIC() end ]]",
        "--[==[ TIC = function() end ]==]",
        "message = [[function TIC() end]]",
        "message = [=[TIC = function() end]=]",
        "message = [==[function TIC() end]==]",
    ] {
        assert_missing_callback(source);
    }
}

#[test]
fn local_nested_table_and_malformed_declarations_are_not_global_callback_evidence() {
    for source in [
        "function TICK() end",
        "local function TIC() end",
        "local\nfunction\nTIC() end",
        "local TIC = function() end",
        "local first,\nTIC\n= function() end",
        "local TIC; TIC = function() end",
        "local TIC\nTIC = function() end",
        "object.TIC = function() end",
        "object:TIC(function() end)",
        "function outer() function TIC() end end",
        "callbacks = {\nTIC = function() end\n}",
        "for TIC = function() end, 1 do end",
        "for TIC in pairs({}) do end",
        "function TIC end",
        "TIC = function end",
    ] {
        assert_missing_callback(source);
    }
}

#[test]
fn accepts_conventional_top_level_declaration_and_assignment_without_execution() {
    for source in [
        "function TIC() end",
        "function\nTIC\n() end",
        "TIC = function() end",
        "TIC\n=\nfunction\n() end",
        "local unrelated\nfunction TIC() end",
        "function outer() local TIC = function() end end\nTIC = function() end",
        "for i = 1, 1 do end\nfunction TIC() end",
        "while (function() return false end)() do end\nTIC = function() end",
    ] {
        let cart = lua_cart(source);
        assert!(
            analyze_bytes(&cart)
                .expect("non-executing analysis")
                .has_tic_callback,
            "expected bounded callback evidence: {source:?}"
        );
        assert_eq!(verify_bytes(&cart).severity_max(), "ok", "{source:?}");
    }
}

#[test]
fn rejects_non_lua_language_without_executing_the_cart() {
    let report = verify_bytes(&lua_cart("// script: javascript\nfunction TIC() {}"));
    assert_eq!(report.severity_max(), "error");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unsupported_language")
    );
}
