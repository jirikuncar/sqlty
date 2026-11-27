//! The configuration a request carries: its dialect, `sqlty.toml` settings, the settings a host
//! resolved, and the sqlty table of a file.

#![expect(clippy::unwrap_used, reason = "a malformed answer fails the test")]

use serde_json::{json, Value};
use sqlty_wasm::run;

const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);";

fn ask(request: &Value) -> Value {
    serde_json::from_str(&run(&request.to_string())).unwrap()
}

#[test]
fn the_dialect_is_postgres_by_default() {
    let answer = ask(&json!({"op": "analyze", "source": "q = \"--sql\\nSELECT 1::text AS n\""}));
    assert_eq!(answer["queries"][0]["row_type"], "tuple[str]");
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
            "the WebAssembly engine reads only deprecated-columns, dialect, driver, import-style, max-query-variants, param-types, preview, rules, tenancy, types, not 'schema'",
        ),
        ("import-style = \"from x\"", "Unsupported import-style"),
        ("param-types = 1", "param-types must be true or false"),
    ] {
        let answer =
            ask(&json!({"op": "analyze", "source": "", "dialect": "postgres", "config": config}));
        let error = answer["error"].as_str().unwrap();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn config_answers_the_sqlty_table_of_a_file() {
    let config = |path: &str, source: &str| {
        ask(&json!({"op": "config", "source": source, "path": path, "dialect": ""}))
    };
    let pyproject = "[project]\nname = \"app\"\n\n[tool.sqlty]\nschema = \"db/schema.sql\"\nextend = \"../base.toml\"\n";
    assert_eq!(
        config("/app/pyproject.toml", pyproject),
        json!({"config": {"schema": "db/schema.sql", "extend": "../base.toml"}})
    );
    assert_eq!(
        config("pyproject.toml", "[project]\nname = \"app\"\n"),
        Value::Null
    );
    let required = ask(&json!({
        "op": "config", "source": "", "path": "/base/pyproject.toml", "dialect": "", "required": true,
    }));
    assert_eq!(
        required["error"],
        "/base/pyproject.toml: no [tool.sqlty] table"
    );
    assert_eq!(
        config("/app/sqlty.toml", "dialet = \"sqlite\"")["error"],
        "/app/sqlty.toml: unknown key 'dialet' in [tool.sqlty] (did you mean 'dialect'?); expected one of: dialect, driver, schema, queries, query-format, rules, overrides, import-style, untyped, json-types, param-types, preview, max-query-variants, coerce-row-types, functions, types, deprecated-columns, tenancy, format-command, extend, databases, exclude, extend-exclude, respect-gitignore"
    );
}

#[test]
fn settings_resolved_by_the_host_apply_with_their_types() {
    let source = "q = \"--sql\\nSELECT d FROM t\"\n";
    let settings = json!({
        "dialect": "sqlite",
        "schema": "ignored by the engine.sql",
        "types": {"DATE": "datetime.date"},
        "deprecated-columns": {"t.d": {"use": "e"}},
        "tenancy": {"table": "t", "columns": ["t.d"]},
        "rules": {"stale-rewrite": {"severity": "error"}},
    });
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": "CREATE TABLE t (d DATE NOT NULL);",
        "dialect": "postgres", "settings": settings,
    }));
    assert_eq!(answer["queries"][0]["row_type"], "tuple[datetime.date]");
    assert_eq!(answer["diagnostics"][0]["severity"], "error");
    let rules: Vec<&str> = answer["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|found| found["rule"].as_str())
        .collect();
    assert!(rules.contains(&"deprecated-column"), "{answer}");
    assert!(rules.contains(&"tenant-scope"), "{answer}");
    let bad = ask(&json!({
        "op": "analyze", "source": source, "dialect": "postgres",
        "settings": {"types": {"NOT A TYPE": "str"}},
    }));
    assert_eq!(
        bad["error"],
        "[tool.sqlty.types]: `NOT A TYPE` is not a SQL type"
    );
    let bad = ask(&json!({
        "op": "analyze", "source": source, "dialect": "postgres",
        "settings": {"rules": {"stale-rewrite": "loud"}},
    }));
    assert!(bad["error"].as_str().unwrap().contains("loud"), "{bad}");
}

#[test]
fn settings_of_a_file_take_its_database_and_overrides() {
    let source = "q = \"--sql\\nSELECT d FROM t\"\n";
    let settings = json!({
        "rules": {"stale-rewrite": "warn"},
        "databases": {"lite": {"dialect": "sqlite", "include": ["lite"]}},
        "overrides": [{"include": ["lite/strict"], "exclude": [], "rules": {"stale-rewrite": "error"}}],
    });
    let at = |file: &str| {
        ask(&json!({
            "op": "analyze", "source": source, "schema": "CREATE TABLE t (d DATE NOT NULL);",
            "dialect": "postgres", "settings": settings, "base": "/w", "file": file,
        }))
    };
    assert_eq!(
        at("/w/app/q.py")["queries"][0]["row_type"],
        "tuple[datetime.date]"
    );
    assert_eq!(at("/w/lite/q.py")["queries"][0]["row_type"], "tuple[str]");
    assert_eq!(at("/w/lite/q.py")["diagnostics"][0]["severity"], "warn");
    assert_eq!(
        at("/w/lite/strict/q.py")["diagnostics"][0]["severity"],
        "error"
    );
    let untoml = ask(&json!({
        "op": "analyze", "source": source, "schema": "CREATE TABLE t (d DATE NOT NULL);",
        "dialect": "postgres", "settings": {"import-style": null}, "base": "/w", "file": "/w/q.py",
    }));
    assert_eq!(untoml["queries"][0]["row_type"], "tuple[datetime.date]");
}

#[test]
fn a_driver_types_ranges_under_the_projects_types() {
    let source = "q = \"--sql\\nSELECT during, days FROM b\"\n";
    let answer = ask(&json!({
        "op": "analyze", "source": source, "dialect": "postgres",
        "schema": "CREATE TABLE b (during tstzrange NOT NULL, days daterange NOT NULL);",
        "config": "driver = \"psycopg\"\n\n[types]\nDATERANGE = \"str\"\n",
    }));
    assert_eq!(
        answer["queries"][0]["row_type"],
        "tuple[psycopg.types.range.Range[datetime.datetime], str]"
    );
}
