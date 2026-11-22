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
fn unquoted_table_names_fold_outside_postgres() {
    for dialect in ["mysql", "sqlite"] {
        let schema = load_schema("CREATE TABLE users (id INT NOT NULL)", dialect);
        let infer = |sql: &str| infer_type(sql, dialect, Some(&schema));
        assert_eq!(
            infer("SELECT id FROM Users").as_deref(),
            Some("tuple[int]"),
            "{dialect}"
        );
        assert!(infer("SELECT id FROM `Users`").is_some(), "{dialect}");
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
        infer("SELECT t.col.b[1] FROM t").as_deref(),
        Some("tuple[str | None]")
    );
    assert_eq!(
        infer("SELECT (arr)[1] FROM t").as_deref(),
        Some("tuple[int | None]")
    );
}

#[test]
fn a_scalar_subquery_is_named_by_the_leftmost_select_of_its_query() {
    assert_eq!(name("(SELECT 1 AS y UNION SELECT 2 AS z)"), "y");
    assert_eq!(name("((SELECT 1 AS y))"), "y");
    assert_eq!(name("(VALUES (1))"), "values");
}

#[test]
fn text_names_of_mysql_and_sqlite_items() {
    let ddl = "CREATE TABLE t (id INT, a INT, b INT, c INT)";
    // A column keeps the name the schema spells in SQLite, and as written in MySQL.
    assert_eq!(name_in("(T.Id) FROM t", "sqlite", ddl), "id");
    assert_eq!(name_in("(T.Id) FROM t", "mysql", ddl), "Id");
    // A MySQL string literal is its value, with or without a character set.
    assert_eq!(name_in("_utf8mb4'x'", "mysql", ddl), "x");
    let names = |select: &str| -> Vec<String> {
        let schema = load_schema(ddl, "mysql");
        let columns = analyze_select(select, "mysql", Some(&schema)).unwrap();
        columns.into_iter().map(|c| c.name).collect()
    };
    // The items are told apart by their text.
    assert_eq!(
        names("SELECT DISTINCT a IS DISTINCT FROM b, c FROM t"),
        ["a IS DISTINCT FROM b", "c"]
    );
    assert_eq!(names("SELECT a; SELECT b FROM t"), ["a"]);
}

#[test]
fn a_set_operation_is_named_by_its_leftmost_select() {
    let ddl = "CREATE TABLE t (a INT, id INT)";
    let names = |select: &str| -> Vec<String> {
        let schema = load_schema(ddl, "postgres");
        let columns = analyze_select(select, "postgres", Some(&schema)).unwrap();
        columns.into_iter().map(|c| c.name).collect()
    };
    assert_eq!(names("SELECT 1 AS a"), ["a"]);
    assert_eq!(names("SELECT 1 AS a UNION SELECT 2 AS b"), ["a"]);
    // A parenthesized branch is the leftmost one too.
    assert_eq!(names("(SELECT 3 AS c) UNION SELECT 2 AS b"), ["c"]);
    assert_eq!(
        names("((SELECT 3 AS c) UNION SELECT 2) UNION SELECT 4"),
        ["c"]
    );
    // `VALUES` has no select to name its columns.
    assert_eq!(names("VALUES (1)"), ["column1"]);
    assert_eq!(names("(VALUES (1)) UNION SELECT 2"), ["column1"]);
}

#[test]
fn a_star_over_joins_expands_by_the_name_of_each_table_without_quotes() {
    let schema = load_schema(
        "CREATE TABLE t (a INT NOT NULL, id INT); CREATE TABLE \"T2\" (a INT)",
        "postgres",
    );
    let expand = |sql: &str| expand_star(sql, "postgres", Some(&schema));
    // The alias, else the table without its schema.
    assert_eq!(
        expand("SELECT * FROM public.t JOIN t u ON true").as_deref(),
        Some("SELECT t.a::INT, t.id::INT, u.a::INT, u.id::INT FROM public.t JOIN t u ON true")
    );
    // A subquery has no name to qualify by, and a quoted one is not written out.
    assert_eq!(
        expand("SELECT * FROM public.t JOIN (SELECT 1 AS x) s ON true"),
        None
    );
    assert_eq!(expand("SELECT * FROM \"T2\" JOIN t ON true"), None);
}
