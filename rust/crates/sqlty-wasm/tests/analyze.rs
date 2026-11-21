//! The analysis of a module: its queries, diagnostics, highlights, fixes, formatting,
//! and the breaking changes a schema change makes.

use serde_json::{json, Value};
use sqlty_wasm::run;

const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);";

fn ask(request: &Value) -> Value {
    serde_json::from_str(&run(&request.to_string())).unwrap()
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
        "from sqlty import SQL\nq = SQL[tuple[int, str | None], tuple[str | None]](\"--sql\\nSELECT id, email FROM users WHERE name = %s\")\n"
    );
    assert_eq!(answer["highlights"][0], json!([5, 10, "comment"]));
    assert_eq!(answer["highlights"][1], json!([12, 18, "keyword"]));
}

#[test]
fn analyze_highlights_a_long_line_in_utf16_units() {
    // Each offset used to be counted from the start of the source: quadratic in its length.
    let source = format!(
        "é = \"--sql\\nSELECT 'ü', {}\"\n",
        vec!["1"; 50_000].join(", ")
    );
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    let highlights = answer["highlights"].as_array().unwrap();
    assert_eq!(highlights.len(), 3 + 50_000);
    assert_eq!(highlights[2], json!([19, 22, "string"]));
    assert_eq!(highlights[3], json!([24, 25, "number"]));
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
fn param_types_are_written_by_rewrite_and_fix_all() {
    let source = "q = \"--sql\\nSELECT name FROM users WHERE id = %(id)s\"\n";
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres",
    }));
    let expected = "import typing\nfrom sqlty import SQL\n\n\n# sqlty: generated\n\
        class QParams(typing.TypedDict):\n    id: int | None\n\n\n\
        q = SQL[tuple[str], QParams](\"--sql\\nSELECT name FROM users WHERE id = %(id)s\")\n";
    assert_eq!(answer["rewritten"], expected);
    assert_eq!(answer["fixed"], expected);
    let answer =
        ask(&json!({"op": "analyze", "source": expected, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(answer["stripped"], source);
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres",
        "config": "param-types = false",
    }));
    let expected =
        "from sqlty import SQL\nq = SQL[tuple[str]](\"--sql\\nSELECT name FROM users WHERE id = %(id)s\")\n";
    assert_eq!(answer["rewritten"], expected);
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

#[test]
fn hover_outside_a_query_is_null() {
    let answer =
        ask(&json!({"op": "hover", "source": "x = 1", "offset": 99, "dialect": "postgres"}));
    assert_eq!(answer, Value::Null);
}

#[test]
fn sqruff_findings_are_reported_when_enabled() {
    let source = "q = SQL[tuple[int]](\"--sql\\nSELECT id FROM users WHERE name = NULL\")\n";
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres",
        "config": "[rules]\nsqruff = \"warn\"",
    }));
    let rules: Vec<_> = answer["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| &found["rule"])
        .collect();
    assert_eq!(rules, [&json!("sqruff")]);
}

#[test]
fn a_field_one_query_needs_optional_is_not_redundant() {
    let schema = "CREATE TABLE coupons (id INTEGER PRIMARY KEY, owner_id INTEGER);";
    let source = "from typing import Annotated, LiteralString, TypedDict\nfrom sqlty import SQL\n\n\
        def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...\n\n\
        class Coupon(TypedDict):\n    id: int\n    owner_id: int | None\n\n\
        mine = fetch_all(\"SELECT id, owner_id FROM coupons WHERE owner_id = %(owner)s\", Coupon)\n\
        any_ = fetch_all(\"SELECT id, owner_id FROM coupons\", Coupon)\n";
    let answer = ask(&json!({
        "op": "analyze", "source": source, "schema": schema, "dialect": "postgres",
        "config": "[rules]\nredundant-optional = \"error\"",
    }));
    assert_eq!(answer["diagnostics"], json!([]));
}

#[test]
fn parameters_stored_in_a_column_know_if_they_take_none() {
    let source = "q = \"--sql\\nINSERT INTO users (id, name, email) VALUES (%s, %s, %s)\"\n";
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    let nullable: Vec<_> = answer["queries"][0]["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|param| &param["nullable"])
        .collect();
    assert_eq!(nullable, [&json!(false), &json!(false), &json!(true)]);
}

#[test]
fn an_offset_past_the_end_is_the_end() {
    // "é" is one UTF-16 unit but two bytes: an offset past the end clamps to the end, not a panic.
    let source = "é = \"--sql\\nSELECT na FROM users\"";
    let end = source.encode_utf16().count();
    for op in ["complete", "hover"] {
        let at = |offset: usize| {
            ask(&json!({
                "op": op, "source": source, "schema": SCHEMA, "dialect": "postgres", "offset": offset,
            }))
        };
        assert_eq!(at(end + 10_000), at(end));
        assert_eq!(at(end + 10_000), Value::Null);
    }
}

#[test]
fn every_highlight_and_candidate_kind_has_a_name() {
    let source = "q = \"--sql\\nWITH c AS (SELECT 1) SELECT 'a', 1 + $1 FROM \"";
    let answer = ask(&json!({
        "op": "analyze", "source": source,
        "schema": format!("{SCHEMA}\nCREATE VIEW v AS SELECT id FROM users;"), "dialect": "postgres",
    }));
    let kinds: std::collections::BTreeSet<_> = answer["highlights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found[2].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "comment",
            "keyword",
            "number",
            "operator",
            "parameter",
            "string"
        ]
        .into()
    );
    let complete = ask(&json!({
        "op": "complete", "source": source, "offset": source.len() - 1,
        "schema": format!("{SCHEMA}\nCREATE VIEW v AS SELECT id FROM users;"), "dialect": "postgres",
    }));
    let kinds: std::collections::BTreeSet<_> = complete["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found["kind"].as_str().unwrap())
        .collect();
    assert!(
        ["table", "view", "cte"]
            .iter()
            .all(|kind| kinds.contains(kind)),
        "{kinds:?}"
    );
    let source = "q = \"--sql\\nSELECT 1 FROM users JOIN \"";
    let schema =
        format!("{SCHEMA}\nCREATE TABLE posts (id INT, user_id INT REFERENCES users (id));");
    let complete = ask(&json!({
        "op": "complete", "source": source, "offset": source.len() - 1, "schema": schema, "dialect": "postgres",
    }));
    assert_eq!(complete["items"][0]["kind"], "join");
}

#[test]
fn analyze_keeps_what_ignore_comments_suppress() {
    let source = "q = \"--sql\\nSELECT * FROM users\"  # sqlty: ignore\n";
    let answer =
        ask(&json!({"op": "analyze", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(answer["diagnostics"], json!([]));
    assert_eq!(answer["fixed"], source);

    // A source that does not parse is read by its strings, and its comments by the lexer.
    let broken = "q = \"--sql\\nSELECT 1 FROM\"{}\ndef f(:\n";
    let rules = |source: String| {
        let answer = ask(&json!({"op": "analyze", "source": source, "dialect": "postgres"}));
        answer["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|found| found["rule"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(rules(broken.replace("{}", "")), ["unparsed-query"]);
    assert!(rules(broken.replace("{}", "  # sqlty: ignore")).is_empty());
}

#[test]
fn breaking_reports_what_the_schema_change_adds() {
    let source = "a = \"--sql\\nSELECT email FROM users\"\nb = SQL[tuple[str | None]](\"--sql\\nSELECT name FROM users\")\nc = \"--sql\\nSELECT nme FROM users\"\n";
    let after = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);";
    let answer = ask(&json!({
        "op": "breaking", "source": source, "schema": after, "before": SCHEMA, "dialect": "postgres",
    }));
    let found: Vec<_> = answer["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| {
            (
                found["line"].as_u64().unwrap(),
                found["rule"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(found, [(1, "unknown-column")]);
}

#[test]
fn breaking_without_a_before_schema_reports_every_schema_error() {
    let source = "q = \"--sql\\nSELECT nme FROM users\"\n";
    let answer =
        ask(&json!({"op": "breaking", "source": source, "schema": SCHEMA, "dialect": "postgres"}));
    assert_eq!(answer["diagnostics"][0]["rule"], "unknown-column");
}
