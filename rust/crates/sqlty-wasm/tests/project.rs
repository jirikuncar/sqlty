//! The `project` operation: the configuration of a directory, asking for each file it reads.

#![expect(
    clippy::unwrap_used,
    reason = "a malformed answer fails the test"
)]

use serde_json::{json, Value};
use sqlty_wasm::run;

fn ask(request: &Value) -> Value {
    serde_json::from_str(&run(&request.to_string())).unwrap()
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
