use crate::support::*;
use sqlty_sql::analyzer::*;

/// The name PostgreSQL gives the one result column of `SELECT {select}`.
fn name_in(select: &str, dialect: &str, ddl: &str) -> String {
    let schema = load_schema(ddl, dialect);
    let columns = analyze_select(&format!("SELECT {select}"), dialect, Some(&schema))
        .unwrap_or_else(|| panic!("{select} has no columns"));
    columns[0].name.clone()
}

fn name(select: &str) -> String {
    name_in(select, "postgres", "CREATE TABLE t (x INT, y INT, col INT)")
}

#[test]
fn column_names_follow_postgres_figure_colname() {
    let exprs = [
        "x",
        "t.x",
        "\"Mixed\"",
        "x::int",
        "'a'::text",
        "'a'::text::int",
        "1::int",
        "1::bigint",
        "1::double precision",
        "1::varchar(10)",
        "1::timestamp with time zone",
        "count(*)",
        "pg_catalog.lower(x)",
        "case when x then 1 end",
        "case when x then 1 else y end",
        "exists (select 1)",
        "array[1, 2]",
        "row(1, 2)",
        "(select x from t)",
        "(select x as y from t)",
        "(t.col).field",
        "extract(year from x)",
        "trim(leading 'a' from x)",
        "trim(x)",
        "substring(x from 1)",
        "interval '1 day'",
        "x + 1",
        "-x",
        "1",
        "'literal'",
    ];
    let rendered: Vec<String> = exprs
        .iter()
        .map(|sql| {
            let columns = analyze_select(&format!("SELECT {sql} FROM t"), "postgres", None)
                .unwrap_or_else(|| panic!("{sql}"));
            format!("{sql} => {}", columns[0].name)
        })
        .collect();
    insta::assert_snapshot!(rendered.join("\n"));
}

#[test]
fn a_name_falls_back_to_question_column_where_postgres_has_none() {
    assert_eq!(name("x + 1"), "?column?");
    assert_eq!(name("x"), "x");
}

#[test]
fn identifiers_fold_in_postgres_unless_quoted() {
    assert_eq!(name("Id FROM (SELECT 1 AS id) s"), "id");
    assert_eq!(name("\"Id\" FROM (SELECT 1 AS \"Id\") s"), "Id");
}

#[test]
fn aliases_keep_their_case_outside_postgres() {
    assert_eq!(name_in("1 AS TotalCount", "postgres", ""), "totalcount");
    assert_eq!(name_in("1 AS TotalCount", "mysql", ""), "TotalCount");
    assert_eq!(name_in("1 AS \"TotalCount\"", "postgres", ""), "TotalCount");
}

#[test]
fn cast_target_names_are_postgres_type_names() {
    for (target, expected) in [
        ("integer", "int4"),
        ("smallint", "int2"),
        ("boolean", "bool"),
        ("real", "float4"),
        ("float(10)", "float4"),
        ("float(40)", "float8"),
        ("decimal(10, 2)", "numeric"),
        ("char(3)", "bpchar"),
        ("timestamp", "timestamp"),
        ("timestamptz", "timestamptz"),
        ("time with time zone", "timetz"),
        ("int[]", "int4"),
        ("public.mood", "mood"),
        ("text", "text"),
    ] {
        assert_eq!(name(&format!("1::{target}")), expected, "{target}");
    }
}

#[test]
fn rarer_column_names() {
    for (sql, expected) in [
        ("(1, 2)", "row"),
        ("ceil(x)", "ceil"),
        ("floor(x)", "floor"),
        ("overlay(x placing 'a' from 1)", "overlay"),
        ("(select * from t)", "?column?"),
        ("\"\"", "?column?"),
        ("1::character varying", "varchar"),
        ("1::timestamp(3) with time zone", "timestamptz"),
        ("1::time(3) with time zone", "timetz"),
        ("1::time without time zone", "time"),
    ] {
        assert_eq!(name(&format!("{sql} FROM t")), expected, "{sql}");
    }
}

#[test]
fn a_table_is_found_by_at_most_its_schema_and_name() {
    let schema = load_schema(
        "CREATE TABLE public.users (id INT NOT NULL); CREATE TABLE \"Users\" (n TEXT NOT NULL)",
        "postgres",
    );
    let infer = |sql: &str| infer_type(sql, "postgres", Some(&schema));
    assert_eq!(infer("SELECT id FROM users").as_deref(), Some("tuple[int]"));
    assert_eq!(
        infer("SELECT id FROM public.users").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT id FROM db.public.users").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer("SELECT id FROM Users").as_deref(), Some("tuple[int]"));
    assert_eq!(
        infer("SELECT n FROM \"Users\"").as_deref(),
        Some("tuple[str]")
    );
}

#[test]
fn table_names_fold_by_dialect() {
    for (dialect, quoted_users_is_found) in [("mysql", true), ("sqlite", true)] {
        let schema = load_schema("CREATE TABLE users (id INT NOT NULL)", dialect);
        let infer = |sql: &str| infer_type(sql, dialect, Some(&schema));
        assert_eq!(
            infer("SELECT id FROM Users").as_deref(),
            Some("tuple[int]"),
            "{dialect}"
        );
        assert_eq!(
            infer("SELECT id FROM `Users`").is_some(),
            quoted_users_is_found,
            "{dialect}"
        );
    }
    let schema = load_schema("CREATE TABLE users (id INT NOT NULL)", "postgres");
    assert_eq!(
        infer_type("SELECT id FROM \"Users\"", "postgres", Some(&schema)),
        None
    );
}

#[test]
fn a_field_chain_splits_after_the_table_and_column() {
    let schema = load_schema(
        "CREATE TYPE pair AS (a INT, b TEXT[]); CREATE TABLE t (col pair NOT NULL, arr INT[] NOT NULL)",
        "postgres",
    );
    let infer = |sql: &str| infer_type(sql, "postgres", Some(&schema));
    assert_eq!(
        infer("SELECT (t.col).b[1] FROM t").as_deref(),
        Some("tuple[str | None]")
    );
    assert_eq!(
        infer("SELECT (arr)[1] FROM t").as_deref(),
        Some("tuple[int | None]")
    );
}
