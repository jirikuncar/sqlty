//! The `query` op: one query typed and checked, as the ESLint plugin asks.

#![expect(clippy::expect_used, reason = "a bad answer fails the test")]

use serde_json::{json, Value};
use sqlty_wasm::run;

const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);";

fn ask(request: &Value) -> Value {
    serde_json::from_str(&run(&request.to_string())).expect("JSON")
}

#[test]
fn query_types_bare_sql_and_its_parameters() {
    let sql = "SELECT id, email FROM users WHERE name = $1";
    let answer = ask(&json!({"op": "query", "sql": sql, "schema": SCHEMA}));
    assert_eq!(
        answer,
        json!({
            "returns_rows": true,
            "columns": [
                {"name": "id", "sql_type": "INT", "nullable": false},
                {"name": "email", "sql_type": "TEXT", "nullable": true},
            ],
            "params": [{"index": 1, "name": null, "sql_type": "TEXT", "nullable": null}],
            "diagnostics": [],
        })
    );
}

#[test]
fn query_reports_unknown_names_in_utf16_units() {
    let sql = "SELECT 'é', nme FROM users";
    let answer = ask(&json!({"op": "query", "sql": sql, "schema": SCHEMA}));
    let found = &answer["diagnostics"][0];
    assert_eq!(found["rule"], "unknown-column");
    assert_eq!((&found["start"], &found["end"]), (&json!(12), &json!(15)));
    assert_eq!(found["fix"][0]["replacement"], "name");
}

#[test]
fn query_reports_what_does_not_parse_unless_ignored() {
    let sql = "SELEC 1";
    let answer = ask(&json!({"op": "query", "sql": sql}));
    assert_eq!(answer["diagnostics"][0]["rule"], "unparsed-query");
    assert_eq!(answer["diagnostics"][0]["end"], 7);
    let config = "[rules]\nunparsed-query = \"ignore\"\n";
    let answer = ask(&json!({"op": "query", "sql": sql, "config": config}));
    assert_eq!(answer["diagnostics"], json!([]));
}

#[test]
fn query_of_dml_returns_no_rows() {
    let answer =
        ask(&json!({"op": "query", "sql": "DELETE FROM users WHERE id = $1", "schema": SCHEMA}));
    assert_eq!(answer["returns_rows"], false);
    assert_eq!(answer["columns"], Value::Null);
    assert_eq!(answer["params"][0]["sql_type"], "INT");
}

#[test]
fn query_gives_constants_their_sql_type() {
    let sql = "SELECT 1 AS n, 'a' AS s, true AS b, 1.5 AS d, CASE WHEN true THEN 1 ELSE 2 END AS c";
    let answer = ask(&json!({"op": "query", "sql": sql}));
    let types: Vec<_> = answer["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .map(|c| c["sql_type"].clone())
        .collect();
    assert_eq!(types, ["INT", "TEXT", "BOOLEAN", "DECIMAL", "INT"]);
    let answer =
        ask(&json!({"op": "query", "sql": "SELECT 1.5 AS f, NULL AS z", "dialect": "clickhouse"}));
    assert_eq!(answer["columns"][0]["sql_type"], "DOUBLE");
    assert_eq!(answer["columns"][1]["sql_type"], Value::Null);
}

#[test]
fn query_reuses_a_schema_by_its_key() {
    let sql = "SELECT name FROM users";
    let answer = ask(&json!({"op": "query", "sql": sql, "schema_key": "a"}));
    assert_eq!(answer, json!({"need": "schema"}));
    let answer = ask(&json!({"op": "query", "sql": sql, "schema_key": "a", "schema": SCHEMA}));
    assert_eq!(answer["columns"][0]["sql_type"], "TEXT");
    let answer = ask(&json!({"op": "query", "sql": sql, "schema_key": "a"}));
    assert_eq!(answer["columns"][0]["nullable"], false);
}
