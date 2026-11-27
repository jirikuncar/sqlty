//! Requests the engine cannot answer: bad JSON, a panic, and an instance that crashed before.

#![expect(clippy::unwrap_used, reason = "a malformed answer fails the test")]

use serde_json::{json, Value};
use sqlty_wasm::run;

fn ask(request: &Value) -> Value {
    serde_json::from_str(&run(&request.to_string())).unwrap()
}

#[test]
fn a_request_that_is_not_json_is_an_error() {
    assert!(run("{").contains("\"error\""));
}

#[test]
fn a_request_asked_to_panic_panics_with_its_message() {
    let request = json!({"op": "analyze", "source": "", "dialect": "postgres", "panic": "boom"});
    let payload = std::panic::catch_unwind(|| run(&request.to_string())).unwrap_err();
    assert_eq!(payload.downcast_ref::<String>().unwrap(), "boom");
}

#[test]
fn a_crashed_request_reports_an_internal_error_in_place_of_its_answer() {
    let source = "q = \"--sql\\nSELECT 1\"";
    let request = |op: &str, config: &str| {
        let request = json!({"op": op, "source": source, "dialect": "postgres", "config": config,
            "crashed": "boom", "panic": "again"});
        ask(&request)
    };
    let link = sqlty_python::report::issue_url("boom");
    let message = format!("sqlty crashed checking this file: boom; please report it at {link}");
    assert_eq!(
        request("analyze", ""),
        json!({
            "diagnostics": [{"rule": "internal-error", "message": message, "severity": "error",
                "line": 1, "start": 0, "end": 0, "fix": []}],
            "queries": [],
            "highlights": [],
            "rewritten": source,
            "stripped": source,
            "fixed": source,
        })
    );
    let ignored = request("analyze", "[rules]\ninternal-error = \"ignore\"");
    assert_eq!(ignored["diagnostics"], json!([]));
    assert_eq!(
        request("hover", ""),
        json!({"error": format!("sqlty crashed: boom; please report it at {link}")})
    );
}
