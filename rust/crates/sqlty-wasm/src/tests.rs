use serde_json::{json, Value};

use crate::run;

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

#[test]
fn project_asks_for_each_file_it_reads() {
    let mut files = serde_json::Map::new();
    let mut asked = Vec::new();
    let answer = loop {
        let answer = ask(&json!({"op": "project", "path": "/w/app", "files": files}));
        let Some(path) = answer["need"].as_str() else {
            break answer;
        };
        asked.push(path.to_string());
        let text = match path {
            "/w/app/pyproject.toml" => json!("[tool.sqlty]\nextend = \"../sqlty.toml\"\n"),
            "/w/sqlty.toml" => json!("schema = \"db/schema.sql\"\n"),
            _ => Value::Null,
        };
        files.insert(path.to_string(), text);
    };
    assert_eq!(
        asked,
        [
            "/w/app/sqlty.toml",
            "/w/app/pyproject.toml",
            "/w/sqlty.toml"
        ]
    );
    assert_eq!(
        answer,
        json!({"config": {"schema": ["../db/schema.sql"]}, "file": "/w/app/pyproject.toml"})
    );
    let files = json!({"/a/sqlty.toml": null, "/a/pyproject.toml": "[tool", "/a/Cargo.toml": null});
    let answer = ask(&json!({"op": "project", "path": "/a", "files": files}));
    assert!(answer["warning"]
        .as_str()
        .unwrap()
        .starts_with("/a/pyproject.toml: "));
    let files = json!({"/a/sqlty.toml": "nope = 1"});
    let answer = ask(&json!({"op": "project", "path": "/a", "files": files}));
    assert!(answer["error"]
        .as_str()
        .unwrap()
        .contains("unknown key 'nope'"));
    let mut files =
        json!({"/a/sqlty.toml": null, "/a/pyproject.toml": null, "/a/Cargo.toml": null});
    for name in ["sqlty.toml", "pyproject.toml", "Cargo.toml"] {
        files[format!("/{name}")] = Value::Null;
    }
    assert_eq!(
        ask(&json!({"op": "project", "path": "/a", "files": files})),
        Value::Null
    );
}
