use serde_json::{json, Value};

use crate::run;

const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);";

fn ask(request: &Value) -> Value {
    serde_json::from_str(&run(&request.to_string())).unwrap()
}

#[test]
fn analyze_types_each_query_and_its_parameters() {
    let source = "q = \"--sql\\nSELECT id, email FROM users WHERE name = %s\"\n";
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(
        answer["queries"],
        json!([{
            "line": 1,
            "sql": "--sql\nSELECT id, email FROM users WHERE name = %s",
            "row_type": "tuple[int, str | None]",
            "columns": [
                {"name": "id", "type": "int", "sql_type": "INT"},
                {"name": "email", "type": "str | None", "sql_type": "TEXT"},
            ],
            "params": [{"index": 1, "name": null, "type": "str", "sql_type": "TEXT", "nullable": null}],
        }])
    );
    assert_eq!(
        answer["rewritten"],
        "from sqlty import SQL\nq = SQL[tuple[int, str | None]](\"--sql\\nSELECT id, email FROM users WHERE name = %s\")\n"
    );
    assert_eq!(answer["highlights"][0], json!([5, 10, "comment"]));
    assert_eq!(answer["highlights"][1], json!([12, 18, "keyword"]));
}

#[test]
fn analyze_reports_diagnostics_in_utf16_units() {
    let source = "é = SQL[tuple[str]](\"--sql\\nSELECT nme FROM users\")\n";
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(
        answer["diagnostics"],
        json!([{
            "rule": "unknown-column",
            "message": "column `nme` does not exist in `users`; did you mean `name`?",
            "severity": "error",
            "start": 35,
            "end": 38,
            "fix": [{"start": 35, "end": 38, "replacement": "name"}],
        }])
    );
}

#[test]
fn complete_and_hover_at_a_utf16_offset() {
    let source = "é = \"--sql\\nSELECT na FROM users\"";
    let cursor = source.find(" FROM").unwrap() - 1;
    let complete = ask(
        &json!({"op": "complete", "source": source, "schema": SCHEMA, "dialect": "postgres", "offset": cursor}),
    );
    assert_eq!(complete["start"], cursor - 2);
    assert_eq!(complete["items"][0]["label"], "name");
    let hover = ask(
        &json!({"op": "hover", "source": source, "schema": SCHEMA, "dialect": "postgres", "offset": cursor + 8}),
    );
    assert_eq!(
        hover["contents"],
        "Table `public.users`\n- `id` `INT NOT NULL`\n- `name` `TEXT NOT NULL`\n- `email` `TEXT NULL`"
    );
    assert_eq!(hover["start"], cursor + 6);
}

#[test]
fn outside_a_query_nothing_completes() {
    let answer =
        ask(&json!({"op": "complete", "source": "x = 1", "dialect": "sqlite", "offset": 2}));
    assert_eq!(answer, Value::Null);
}

#[test]
fn an_unknown_dialect_is_an_error() {
    let answer = ask(&json!({"op": "analyze", "source": "", "dialect": "postgress"}));
    assert!(answer["error"].as_str().unwrap().contains("postgress"));
}

#[test]
fn config_sets_the_dialect_import_style_and_rules() {
    let source = "q = \"--sql\\nSELECT * FROM users\"\n";
    let config = "dialect = \"sqlite\"\nimport-style = \"import sqlty\"\n\n[rules]\nstale-rewrite = \"error\"\n";
    let answer = ask(
        &json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "mysql", "config": config}),
    );
    assert_eq!(answer["diagnostics"][0]["rule"], "stale-rewrite");
    assert_eq!(answer["diagnostics"][0]["severity"], "error");
    assert_eq!(
        answer["rewritten"],
        "import sqlty\nq = sqlty.SQL[tuple[int, str, str | None]](\"--sql\\nSELECT * FROM users\")\n"
    );
    let source = "q = \"--sql\\nSELECT 1 = 1 AS b\"\n";
    for (dialect, row_type) in [("postgres", "tuple[bool]"), ("sqlite", "tuple[int]")] {
        let config = format!("dialect = \"{dialect}\"");
        let answer =
            ask(&json!({"op": "analyze", "source": source, "dialect": "mysql", "config": config}));
        assert_eq!(answer["queries"][0]["row_type"], row_type);
    }
}

#[test]
fn config_errors_are_reported_as_sqlty_check_reports_them() {
    for (config, message) in [
        (
            "dialet = \"mysql\"",
            "unknown key 'dialet' in [tool.sqlty] (did you mean 'dialect'?)",
        ),
        ("[rules]\nselect-star = \"loud\"", "loud"),
        (
            "schema = \"schema.sql\"",
            "the playground reads only dialect, import-style, rules, not 'schema'",
        ),
        ("import-style = \"from x\"", "Unsupported import-style"),
    ] {
        let answer =
            ask(&json!({"op": "analyze", "source": "", "dialect": "postgres", "config": config}));
        let error = answer["error"].as_str().unwrap();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn analyze_gives_quick_fixes_and_what_fix_and_strip_make() {
    let source = "é = \"--sql\\nSELECT id FROM users\"\n";
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(answer["diagnostics"][0]["rule"], "stale-rewrite");
    assert_eq!(
        answer["diagnostics"][0]["fix"],
        json!([
            {"start": 0, "end": 0, "replacement": "from sqlty import SQL\n"},
            {"start": 4, "end": 33, "replacement": "SQL[tuple[int]](\"--sql\\nSELECT id FROM users\")"},
        ])
    );
    let fixed = "from sqlty import SQL\né = SQL[tuple[int]](\"--sql\\nSELECT id FROM users\")\n";
    assert_eq!(answer["fixed"], fixed);
    let answer =
        ask(&json!({"op": "analyze", "source": fixed, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(answer["stripped"], source);
    assert_eq!(answer["diagnostics"], json!([]));
}
