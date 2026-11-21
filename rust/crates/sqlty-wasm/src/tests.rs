use serde_json::{json, Value};

use crate::run;

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
            "the WebAssembly engine reads only dialect, driver, import-style, max-query-variants, param-types, preview, rules, types, not 'schema'",
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
        "/app/sqlty.toml: unknown key 'dialet' in [tool.sqlty] (did you mean 'dialect'?); expected one of: dialect, driver, schema, queries, query-format, rules, overrides, import-style, untyped, json-types, param-types, preview, max-query-variants, coerce-row-types, functions, types, format-command, extend, databases, exclude, extend-exclude, respect-gitignore"
    );
}

#[test]
fn settings_resolved_by_the_host_apply_with_their_types() {
    let source = "q = \"--sql\\nSELECT d FROM t\"\n";
    let settings = json!({
        "dialect": "sqlite",
        "schema": "ignored by the engine.sql",
        "types": {"DATE": "datetime.date"},
        "rules": {"stale-rewrite": "error"},
    });
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": "CREATE TABLE t (d DATE NOT NULL);",
        "dialect": "postgres", "settings": settings,
    }));
    assert_eq!(answer["queries"][0]["row_type"], "tuple[datetime.date]");
    assert_eq!(answer["diagnostics"][0]["severity"], "error");
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

fn rules_of(source: &str) -> Vec<String> {
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    let diagnostics = answer["diagnostics"].as_array().unwrap();
    diagnostics
        .iter()
        .map(|found| format!("{}:{}", found["line"], found["rule"].as_str().unwrap()))
        .collect()
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
