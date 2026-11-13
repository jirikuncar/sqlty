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
                {"name": "id", "type": "int", "sql_type": "INT", "type_rule": null, "nullability_rule": null},
                {"name": "email", "type": "str | None", "sql_type": "TEXT", "type_rule": null, "nullability_rule": null},
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
            "line": 1,
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
fn param_types_are_written_by_rewrite_and_fix_all() {
    let source = "q = \"--sql\\nSELECT name FROM users WHERE id = %(id)s\"\n";
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres",
        "config": "param-types = true",
    }));
    let expected = "import typing\nfrom sqlty import SQL\n\n\n# sqlty: generated\n\
        class QParams(typing.TypedDict):\n    id: int | None\n\n\n\
        q = SQL[tuple[str], QParams](\"--sql\\nSELECT name FROM users WHERE id = %(id)s\")\n";
    assert_eq!(answer["rewritten"], expected);
    assert_eq!(answer["fixed"], expected);
    let answer =
        ask(&json!({"op": "analyze", "source": expected, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(answer["stripped"], source);
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
            "the playground reads only dialect, driver, import-style, max-query-variants, param-types, preview, rules, types, not 'schema'",
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

#[test]
fn format_lays_out_the_query_strings_in_the_configured_dialect() {
    let source = "q = \"\"\"--sql\nselect id,name from users where id=?\"\"\"\n";
    let answer = ask(
        &json!({"op": "format", "source": source, "dialect": "postgres", "config": "dialect = \"sqlite\""}),
    );
    assert_eq!(
        answer["formatted"],
        "q = \"\"\"--sql\nselect\n    id,\n    name\nfrom users\nwhere id = ?\"\"\"\n"
    );
}

#[test]
fn each_column_names_the_inference_rules_that_typed_it() {
    let source = "q = \"--sql\\nSELECT SUM(id) AS total FROM users\"\n";
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    let column = &answer["queries"][0]["columns"][0];
    assert_eq!(column["type_rule"]["id"], "postgres.aggregate.sum-integers");
    assert!(column["type_rule"]["reference"]
        .as_str()
        .unwrap()
        .contains("PostgreSQL"));
    assert!(!column["type_rule"]["reasoning"]
        .as_str()
        .unwrap()
        .is_empty());
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
fn analyze_checks_row_types_helpers_and_driver_calls() {
    let source = r#"import psycopg
from typing import Annotated, Literal, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    email: str

def run[T](q: Annotated[str, SQL], t: type[T], params=()) -> list[T]: ...

def find(conn: psycopg.Connection, order: Literal["id", "nme"]):
    SQL[User]("--sql\nSELECT id, email FROM users")
    run("SELECT id, email FROM users WHERE id = 1", User)
    conn.execute("SELECT id FROM users WHERE id = %s AND name = %s", (1,))
    run("SELECT id FROM users ORDER BY " + order, int)
"#;
    assert_eq!(
        rules_of(source),
        [
            "12:nullable-mismatch",
            "13:nullable-mismatch",
            "14:param-count-mismatch",
        ]
    );
}

#[test]
fn analyze_honours_ignore_comments() {
    let source = "q = SQL[tuple[str]](\"--sql\\nSELECT nme FROM users\")  # sqlty: ignore\n\
                  n = 1  # sqlty: ignore[unknown-column]\n";
    assert_eq!(rules_of(source), ["2:unused-ignore"]);
}

#[test]
fn analyze_reads_a_source_that_does_not_parse() {
    let source = "q = SQL[tuple[str]](\"--sql\\nSELECT nme FROM users\")\ndef broken(:\n";
    assert_eq!(rules_of(source), ["1:unknown-column"]);
}

#[test]
fn preview_checks_each_variant_of_a_query_built_in_branches() {
    let source = "from typing import Annotated, LiteralString\nfrom sqlty import SQL\n\n\
        def run[T](q: Annotated[LiteralString, SQL], t: type[T], /) -> list[T]: ...\n\n\
        def find(active: bool):\n    column = \"id\" if active else \"nme\"\n    \
        return run(\"SELECT \" + column + \" FROM users\", int)\n";
    let rules = |config: &str| -> Vec<String> {
        let answer = ask(&json!({
            "op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres",
            "config": config,
        }));
        let diagnostics = answer["diagnostics"].as_array().unwrap();
        let rule = |found: &Value| found["rule"].as_str().unwrap().to_string();
        diagnostics.iter().map(rule).collect()
    };
    assert_eq!(rules(""), Vec::<String>::new());
    assert_eq!(rules("preview = true"), ["unknown-column"]);
    let limited = rules("preview = true\nmax-query-variants = 1");
    assert_eq!(limited, Vec::<String>::new());
}
