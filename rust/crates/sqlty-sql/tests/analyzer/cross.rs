use crate::support::*;
use sqlty_sql::analyzer::*;

fn infer(sql: &str) -> Option<String> {
    infer_type(sql, "postgres", None)
}

#[test]
fn sum_of_a_column_missing_from_the_schema_is_untyped() {
    let (schema, _) = load_schema_with_skipped("CREATE TABLE users (id INT)", "postgres");
    let (columns, diagnostics) =
        analyze_with_diagnostics("SELECT SUM(nope) FROM users", "postgres", Some(&schema));
    assert_eq!(columns.unwrap()[0].python_type, None);
    assert_eq!(diagnostics[0].rule.name(), "unknown-column");
}

#[test]
fn unparsed_statements_that_set_keys_or_types_are_skipped() {
    let long = format!(
        "ALTER TABLE t ADD CONSTRAINT {} UNIQUE (id) FOO",
        "c".repeat(80)
    );
    let schema = format!(
        "CREATE TABLE t (id INT, meta TEXT);
         ALTER TABLE t ALTER COLUMN meta SET NOT NULL FOO;
         ALTER TABLE t ALTER COLUMN meta SET STATISTICS 200;
         CREATE TYPE mood AS ENUM ('a') FOO;
         GRANT SELECT ON t TO someone FOO;
         {long};"
    );
    let (_, skipped) = load_schema_with_skipped(&schema, "postgres");
    assert_eq!(
        skipped,
        [
            (
                "statement",
                "ALTER TABLE t ALTER COLUMN meta SET NOT NULL FOO".to_string()
            ),
            (
                "statement",
                "CREATE TYPE mood AS ENUM ('a') FOO".to_string()
            ),
            ("statement", format!("{}...", &long[..80])),
        ]
    );
}

#[test]
fn test_dml_returning() {
    assert_eq!(
        infer("DELETE FROM t RETURNING id::INTEGER").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("WITH x AS (SELECT 1) UPDATE t SET a = 1 RETURNING id::INTEGER").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer("INSERT INTO t SELECT a::INTEGER FROM u"), None);
    assert_eq!(
        infer(
            "INSERT INTO t (a) VALUES (1) ON CONFLICT (a) WHERE b = 'x' DO NOTHING RETURNING a::INTEGER"
        )
        .as_deref(),
        Some("tuple[int]")
    );
}

#[test]
fn test_empty_target_list() {
    assert_eq!(
        infer("SELECT FROM t FOR UPDATE").as_deref(),
        Some("tuple[()]")
    );
    assert_eq!(infer("SELECT FROM t").as_deref(), Some("tuple[()]"));
}

#[test]
fn test_schema_order_and_canonical_types() {
    let schema = Some(load_schema(
        "CREATE TABLE users (id INTEGER NOT NULL, score NUMERIC(10,2) NOT NULL, tags TEXT[] NOT NULL)",
        "postgres",
    ));
    assert_eq!(
        expand_star("SELECT * FROM users", "postgres", schema.as_deref()).as_deref(),
        Some("SELECT id::INT, score::DECIMAL(10, 2), tags::ARRAY<TEXT> FROM users")
    );
    assert_eq!(
        infer_type("SELECT * FROM users", "postgres", schema.as_deref()).as_deref(),
        Some("tuple[int, decimal.Decimal, list[str]]")
    );
}

fn names(sql: &str) -> Vec<String> {
    names_in(sql, "postgres")
}

#[test]
fn test_unaliased_names() {
    assert_eq!(
        names("SELECT NOT active, -n, -1, n::INT, COUNT(*), lower(x), f(x) FROM t"),
        ["?column?", "?column?", "?column?", "n", "count", "lower", "f"]
    );
}

#[test]
fn test_mysql_column_types() {
    let schema = Some(load_schema(
        "CREATE TABLE t (a INT3 ZEROFILL, b INT ZEROFILL UNSIGNED, \
         c INT SIGNED ZEROFILL, d MIDDLEINT(8) UNSIGNED)",
        "mysql",
    ));
    let types: Vec<_> = analyze_select("SELECT * FROM t", "mysql", schema.as_deref())
        .unwrap()
        .into_iter()
        .map(|col| col.sql_type.unwrap())
        .collect();
    assert_eq!(types, ["UMEDIUMINT", "UINT", "UINT", "UMEDIUMINT(8)"]);
    assert_eq!(
        expand_star("SELECT * FROM t", "mysql", schema.as_deref()).as_deref(),
        Some("SELECT a, b, c, d FROM t"),
        "MySQL has no `::`"
    );
    assert_eq!(
        names_in("SELECT 1 zerofill", "mysql"),
        ["zerofill"],
        "queries are not respelled"
    );
}

fn names_in(sql: &str, dialect: &str) -> Vec<String> {
    analyze_select(sql, dialect, None)
        .unwrap()
        .into_iter()
        .map(|col| col.name)
        .collect()
}

#[test]
fn test_question_mark_placeholders() {
    assert_eq!(
        infer("SELECT a::INT FROM t WHERE id = ? AND b ?| array['x']").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT a::INT FROM t WHERE b ? 'x'").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer("SELECT a::INT FROM t WHERE b ? 'x' AND id = ?"), None);
}

#[test]
fn test_dialect_type_aliases() {
    let types = |columns: &str, dialect: &str| -> Vec<String> {
        let schema = Some(load_schema(&format!("CREATE TABLE t ({columns})"), dialect));
        analyze_select("SELECT * FROM t", dialect, schema.as_deref())
            .unwrap()
            .into_iter()
            .map(|col| col.sql_type.unwrap())
            .collect()
    };
    assert_eq!(
        types("a INT64, b FLOAT64", "bigquery"),
        ["BIGINT", "DOUBLE"]
    );
    assert_eq!(
        types("a Int8, b INT8", "clickhouse"),
        ["TINYINT", "TINYINT"]
    );
    assert_eq!(types("a INT8", "postgres"), ["BIGINT"]);
    assert_eq!(types("a UNIQUEIDENTIFIER", "tsql"), ["UUID"]);
}

#[test]
fn star_expansion_casts_only_where_the_dialect_has_a_cast_operator() {
    let expand = |dialect: &str| {
        let schema = Some(load_schema("CREATE TABLE t (a INTEGER, b TEXT)", dialect));
        expand_star("SELECT * FROM t", dialect, schema.as_deref())
    };
    for dialect in ["postgres", "duckdb", "snowflake"] {
        assert_eq!(
            expand(dialect).as_deref(),
            Some("SELECT a::INT, b::TEXT FROM t"),
            "{dialect}"
        );
    }
    for dialect in ["mysql", "sqlite", "bigquery", "tsql"] {
        assert_eq!(
            expand(dialect).as_deref(),
            Some("SELECT a, b FROM t"),
            "{dialect}"
        );
    }
}

#[test]
fn test_sum_of_unknown_schema_column_is_untyped() {
    let schema = Some(load_schema("CREATE TABLE users (id INT)", "postgres"));
    assert_eq!(
        infer_type("SELECT SUM(nope) FROM users", "postgres", schema.as_deref()),
        None
    );
}

#[test]
fn test_ctes_and_derived_tables() {
    let schema = Some(load_schema(
        "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, born DATE NOT NULL)",
        "postgres",
    ));
    let infer = |sql: &str| infer_type(sql, "postgres", schema.as_deref());
    assert_eq!(
        infer("WITH r AS (SELECT id, name FROM users) SELECT id, name FROM r").as_deref(),
        Some("tuple[int, str]")
    );
    assert_eq!(
        infer("WITH r(a, b) AS (SELECT id, name FROM users) SELECT b, a FROM r").as_deref(),
        Some("tuple[str, int]")
    );
    assert_eq!(
        infer("WITH a AS (SELECT id FROM users), b AS (SELECT id FROM a) SELECT * FROM b")
            .as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT t.* FROM (SELECT name, born FROM users) t").as_deref(),
        Some("tuple[str, datetime.date]")
    );
    assert_eq!(
        infer("SELECT x FROM (SELECT id FROM users) t(x)").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT u.id, o.name FROM users u JOIN LATERAL (SELECT name FROM users WHERE id = u.id) o ON true")
            .as_deref(),
        Some("tuple[int, str]")
    );
    assert_eq!(
        infer("WITH n AS (INSERT INTO users (name) VALUES ('a') RETURNING id, name) SELECT id, name FROM n")
            .as_deref(),
        Some("tuple[int, str]")
    );
    assert_eq!(
        infer("WITH RECURSIVE c AS (SELECT id FROM users UNION ALL SELECT id FROM c) SELECT id FROM c")
            .as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("WITH users AS (SELECT 1 AS other) SELECT id FROM users"),
        None
    );
}

/// `(rule, message)` pairs for a query against a two-table schema.
fn diagnose(sql: &str) -> Vec<(&'static str, String)> {
    let schema = Some(load_schema(
        "CREATE TABLE a (id INT, n INT, g GEOMETRY); CREATE TABLE b (id INT, t TEXT);",
        "postgres",
    ));
    analyze_with_diagnostics(sql, "postgres", schema.as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name(), found.message))
        .collect()
}

fn rules(sql: &str) -> Vec<&'static str> {
    diagnose(sql).into_iter().map(|(rule, _)| rule).collect()
}

/// `(rule, table, column)` of each diagnostic of a query against a users/orders schema.
fn subjects(sql: &str) -> Vec<(&'static str, Option<String>, Option<String>)> {
    let schema = Some(load_schema(
        "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);\
         CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT NOT NULL REFERENCES users (id),\
         status TEXT NOT NULL)",
        "postgres",
    ));
    analyze_with_diagnostics(sql, "postgres", schema.as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name(), found.table, found.column))
        .collect()
}

#[test]
fn test_name_diagnostic_subjects() {
    let some = |name: &str| Some(name.to_string());
    let cases = [
        (
            "SELECT u.nope FROM users u",
            vec![("unknown-column", some("users"), some("nope"))],
        ),
        (
            "SELECT naïve FROM users",
            vec![("unknown-column", some("users"), some("naïve"))],
        ),
        (
            "SELECT x.id FROM users u",
            vec![("unknown-table", some("x"), some("id"))],
        ),
        (
            "SELECT id FROM missing",
            vec![("unknown-table", some("missing"), None)],
        ),
        (
            "SELECT id FROM users u JOIN orders o ON o.user_id = u.id",
            vec![("ambiguous-column", None, some("id"))],
        ),
        (
            "INSERT INTO orders (id, user_id) VALUES (1, 2)",
            vec![("insert-missing-column", some("orders"), some("status"))],
        ),
        (
            "SELECT my_udf(id) FROM users",
            vec![("unknown-function", None, None)],
        ),
    ];
    for (sql, expected) in cases {
        assert_eq!(subjects(sql), expected, "{sql}");
    }
}

#[test]
fn test_name_diagnostics() {
    assert_eq!(rules("SELECT nope FROM a"), ["unknown-column"]);
    assert_eq!(rules("SELECT x.id FROM a"), ["unknown-table"]);
    assert_eq!(rules("SELECT id FROM a, b"), ["ambiguous-column"]);
    assert_eq!(rules("SELECT id FROM missing"), ["unknown-table"]);
    assert_eq!(rules("SELECT frob(n) FROM a"), ["unknown-function"]);
    assert_eq!(rules("SELECT n::mood FROM a"), ["unknown-type"]);
    assert_eq!(rules("SELECT g FROM a"), ["unknown-type"]);
    assert!(rules("SELECT id, t FROM b UNION TABLE b").is_empty());
}

#[test]
fn test_name_diagnostics_stay_quiet() {
    for sql in [
        "SELECT id FROM a JOIN b USING (id)",
        "SELECT id FROM a NATURAL JOIN b",
        "WITH c AS (SELECT n AS k FROM a) SELECT k FROM c",
        "SELECT s.k FROM (SELECT n AS k FROM a) s",
        "SELECT (SELECT max(b.id) FROM b WHERE b.id = a.id) FROM a",
        "SELECT a.id, l.m FROM a, LATERAL (SELECT max(b.id) AS m FROM b WHERE b.id = a.id) l",
        "SELECT A.ID, \"n\" FROM A",
        "SELECT NULL, $1",
    ] {
        assert!(rules(sql).is_empty(), "{sql}: {:?}", diagnose(sql));
    }
    // A table function's columns are not read, which is reported, not guessed.
    assert_eq!(
        rules("SELECT path FROM ST_Dump(NULL) AS d"),
        ["unsupported-syntax"]
    );
}

/// What each diagnostic's span covers in `sql`.
fn underlined(sql: &str) -> Vec<(&'static str, Option<&str>)> {
    let schema = Some(load_schema(
        "CREATE TABLE a (id INT, n INT, g GEOMETRY); CREATE TABLE b (id INT, t TEXT);",
        "postgres",
    ));
    analyze_with_diagnostics(sql, "postgres", schema.as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name(), found.span.map(|span| &sql[span])))
        .collect()
}

#[test]
fn test_name_diagnostics_underline_the_name() {
    assert_eq!(
        underlined("--sql\nSELECT nope FROM a"),
        [("unknown-column", Some("nope"))]
    );
    assert_eq!(
        underlined("SELECT a.nope FROM a"),
        [("unknown-column", Some("a.nope"))]
    );
    assert_eq!(
        underlined("SELECT \"nopé\" FROM a"),
        [("unknown-column", Some("\"nopé\""))]
    );
    assert_eq!(
        underlined("SELECT id FROM a, b"),
        [("ambiguous-column", Some("id"))]
    );
    assert_eq!(
        underlined("SELECT id\nFROM a JOIN missing ON true"),
        [("unknown-table", Some("missing"))]
    );
    assert_eq!(
        underlined("SELECT upper(frob(n)) FROM a"),
        [("unknown-function", Some("frob"))]
    );
    // Inside a cast, about the cast, not the column it reads.
    assert_eq!(
        underlined("SELECT n::mood FROM a"),
        [("unknown-type", None)]
    );
    // Through the edits of the query before parsing: `%(who)s` and `%s` become `$00`.
    assert_eq!(
        underlined("SELECT %(who)s, nope FROM a"),
        [("unknown-column", Some("nope"))]
    );
    assert_eq!(
        underlined("SELECT %s, %s, nope FROM a"),
        [("unknown-column", Some("nope"))]
    );
    // PostgreSQL reads `?` placeholders only after renumbering them, which moves the names.
    assert_eq!(
        underlined("SELECT ?, nope FROM a"),
        [("unknown-column", None)]
    );
}

#[test]
fn test_name_rules_need_a_schema() {
    let found = analyze_with_diagnostics("SELECT nope, id FROM a, b", "postgres", None).1;
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn rarer_star_expansions_and_result_stars() {
    let schema = Some(load_schema(
        "CREATE TABLE t (a INT); CREATE TABLE u (b INT)",
        "mysql",
    ));
    let expand = |sql: &str| expand_star(sql, "mysql", schema.as_deref());
    assert_eq!(expand("WITH c AS (SELECT 1 AS x) SELECT * FROM c, t"), None);
    assert_eq!(expand("SELECT * FROM t, (SELECT 1 AS x) s"), None);
    assert_eq!(expand("SELECT 1"), None);
    assert_eq!(
        expand("SELECT * FROM t, u").as_deref(),
        Some("SELECT t.a, u.b FROM t, u")
    );
    let stars = |sql: &str| parse(sql, "postgres").result_stars();
    assert_eq!(stars("CREATE TABLE x (a INT)"), Some(vec![]));
    assert_eq!(
        stars("(SELECT * FROM t) UNION SELECT a FROM t"),
        Some(vec!["SELECT *".to_string()])
    );
}

#[test]
fn mysql_full_text_relevance_is_a_double() {
    let (schema, _) = load_schema_with_skipped(
        "CREATE TABLE t (body TEXT NOT NULL, FULLTEXT (body))",
        "mysql",
    );
    let (columns, _) = analyze_with_diagnostics(
        "SELECT MATCH(body) AGAINST('x') AS a FROM t",
        "mysql",
        Some(&schema),
    );
    assert_eq!(columns.unwrap()[0].sql_type.as_deref(), Some("DOUBLE"));
}

#[test]
fn a_star_over_more_than_64_merging_joins_is_untyped() {
    let schema = load_schema_with_skipped("CREATE TABLE t (a int NOT NULL);", "postgres").0;
    let natural = |n: usize| {
        let joins: String = (1..n).map(|i| format!(" NATURAL JOIN t t{i}")).collect();
        format!("SELECT * FROM (t t0{joins})")
    };
    assert_eq!(
        infer_type(&natural(64), "postgres", Some(&schema)).as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer_type(&natural(65), "postgres", Some(&schema)), None);
}

#[test]
fn update_narrowing_folds_unquoted_set_columns() {
    let schema = load_schema_with_skipped("CREATE TABLE t (name TEXT);", "postgres").0;
    let infer = |sql: &str| infer_type(sql, "postgres", Some(&schema));
    assert_eq!(
        infer("UPDATE t SET Name = NULL WHERE name IS NOT NULL RETURNING name").as_deref(),
        Some("tuple[str | None]")
    );
    assert_eq!(
        infer("UPDATE t SET NAME = 'x' RETURNING name").as_deref(),
        Some("tuple[str]")
    );
}

/// `sqlty_sql`'s limit on result columns (`MAX_COLUMNS`); the message below names it.
const MAX_COLUMNS: usize = 4096;

#[test]
fn a_result_wider_than_max_columns_is_untyped() {
    // Each CTE joins the one before to itself: 2^i + 1 columns, past the cap at c12.
    let ctes: Vec<String> = (1..=20)
        .map(|i| {
            format!(
                "c{i} AS (SELECT * FROM c{0} x JOIN c{0} y USING (a))",
                i - 1
            )
        })
        .collect();
    let sql = format!(
        "WITH c0 AS (SELECT 1 AS a, 2 AS b), {} SELECT * FROM c20",
        ctes.join(", ")
    );
    assert_eq!(names(&sql), ["*"]);
    let diagnostics = analyze_with_diagnostics(&sql, "postgres", None).1;
    let message = format!("over {MAX_COLUMNS} result columns are not typed");
    assert!(diagnostics.iter().any(|d| d.message == message));
    assert_eq!(infer_type(&sql, "postgres", None), None);
}

#[test]
fn values_and_functions_wider_than_max_columns_are_untyped() {
    let ones = vec!["1"; MAX_COLUMNS + 1].join(", ");
    let sql = format!("SELECT column1 FROM (VALUES ({ones})) v");
    assert_eq!(infer_type(&sql, "postgres", None), None);
    let columns: Vec<String> = (0..=MAX_COLUMNS).map(|i| format!("c{i} int")).collect();
    let (schema, _) = load_schema_with_skipped(
        &format!(
            "CREATE FUNCTION f() RETURNS TABLE ({}) AS 'SELECT 1' LANGUAGE sql;",
            columns.join(", ")
        ),
        "postgres",
    );
    assert_eq!(
        infer_type("SELECT c0 FROM f()", "postgres", Some(&schema)),
        None
    );
}

#[test]
fn stars_over_wide_tables_stop_past_max_columns() {
    let columns: Vec<String> = (0..MAX_COLUMNS).map(|i| format!("c{i} int")).collect();
    let schema = format!("CREATE TABLE big ({});", columns.join(", "));
    let (schema, _) = load_schema_with_skipped(&schema, "postgres");
    let message = format!("over {MAX_COLUMNS} result columns are not typed");
    for sql in [
        "SELECT * FROM big b0, big b1, big b2",
        "SELECT b0.*, b1.*, b2.* FROM big b0, big b1, big b2",
        "SELECT * FROM big b0 JOIN big b1 USING (c0) JOIN big b2 USING (c0), big b3",
        "UPDATE big SET c0 = 1 FROM big b0, big b1 RETURNING *",
    ] {
        assert_eq!(infer_type(sql, "postgres", Some(&schema)), None, "{sql}");
        let diagnostics = analyze_with_diagnostics(sql, "postgres", Some(&schema)).1;
        assert!(diagnostics.iter().any(|d| d.message == message), "{sql}");
    }
}

#[test]
fn test_set_operation_types() {
    let schema = Some(load_schema(
        "CREATE TABLE a (n INTEGER, t TEXT, d DATE);\
         CREATE TABLE b (n BIGINT, t VARCHAR(5), d TIMESTAMP);\
         CREATE TABLE c (g GEOMETRY);",
        "postgres",
    ));
    let sql_types = |sql: &str| -> Vec<Option<String>> {
        analyze_select(sql, "postgres", schema.as_deref())
            .unwrap()
            .into_iter()
            .map(|col| col.sql_type)
            .collect()
    };
    let types = |names: &[&str]| -> Vec<Option<String>> {
        names
            .iter()
            .map(|name| (!name.is_empty()).then(|| name.to_string()))
            .collect()
    };
    assert_eq!(
        sql_types("SELECT n, t, d FROM a UNION SELECT n, t, d FROM b"),
        types(&["BIGINT", "TEXT", "TIMESTAMP"])
    );
    assert_eq!(
        sql_types("SELECT NULL, $1, n FROM a INTERSECT SELECT n, t, n::REAL FROM b"),
        types(&["BIGINT", "VARCHAR(5)", "FLOAT"])
    );
    assert_eq!(
        sql_types("SELECT n, t FROM a EXCEPT (SELECT t, n FROM b UNION SELECT n, t FROM b)"),
        types(&["", ""])
    );
    assert_eq!(
        sql_types("SELECT (SELECT n FROM a UNION ALL SELECT n FROM b)"),
        types(&["BIGINT"])
    );
    assert_eq!(
        sql_types(
            "WITH RECURSIVE r AS (SELECT n FROM a UNION ALL SELECT n * 2::BIGINT FROM r) \
             SELECT n FROM r"
        ),
        types(&["BIGINT"])
    );
    assert_eq!(
        sql_types("SELECT g, ARRAY[g] FROM c UNION SELECT n, ARRAY[g] FROM a, c"),
        types(&["", "ARRAY<GEOMETRY>"])
    );
    assert_eq!(
        sql_types("SELECT ARRAY[n], n * 1.5 FROM a UNION SELECT ARRAY[n], n FROM b"),
        types(&["ARRAY<BIGINT>", "DECIMAL"])
    );
}

#[test]
fn rarer_parse_errors_highlights_and_placeholders() {
    assert_eq!(
        parse_error("SELECT cast(a, b)", "postgres").as_deref(),
        Some("unsupported in this dialect")
    );
    for sql in [
        "UPDATE t SET (a, b) = (1, 2)",
        "WITH x AS (SELECT 1) UPDATE t SET (a, b) = (1, 2)",
        "INSERT INTO t SET (a, b) = (1, 2)",
        "INSERT INTO t (a) VALUES (1) ON DUPLICATE KEY UPDATE (a, b) = (1, 2)",
    ] {
        assert_eq!(
            parse_error(sql, "mysql").as_deref(),
            Some("unsupported in this dialect"),
            "{sql}"
        );
    }
    assert_eq!(
        parse_error(
            "INSERT INTO t SET a = 1 ON DUPLICATE KEY UPDATE b = 2",
            "mysql"
        ),
        None
    );
    assert_eq!(
        parse_error("UPDATE t SET (a, b) = (1, 2)", "postgres"),
        None
    );
    let operators: Vec<_> = sqlty_sql::highlight::highlight("SELECT a ~~~ b", "postgres")
        .into_iter()
        .map(|(range, _)| range)
        .collect();
    assert_eq!(operators, [0..6, 9..12]);
    assert_eq!(
        sqlty_sql::placeholders::percent_placeholders("SELECT 5 %% 2, %s"),
        vec![15..17]
    );
}

#[test]
fn sqlite_multi_word_type_names_are_joined() {
    let (schema, skipped) = load_schema_with_skipped(
        "CREATE TABLE t (a UNSIGNED BIG INT NOT NULL, \"b c\" VARYING CHARACTER(9), \
         d INTEGER PRIMARY KEY, CONSTRAINT k PRIMARY KEY (d), e)",
        "sqlite",
    );
    assert!(skipped.is_empty(), "{skipped:?}");
    let columns =
        analyze_select("SELECT a, \"b c\", d, e FROM t", "sqlite", Some(&schema)).unwrap();
    let described: Vec<_> = columns
        .iter()
        .map(|c| {
            (
                c.python_type.as_deref(),
                c.sql_type.as_deref(),
                c.nullability,
            )
        })
        .collect();
    assert_eq!(
        described,
        [
            (Some("int"), Some("UNSIGNED_BIG_INT"), Nullability::NonNull),
            (
                Some("str"),
                Some("VARYING_CHARACTER(9)"),
                Nullability::Nullable
            ),
            (Some("int"), Some("INT"), Nullability::NonNull),
            (None, Some(""), Nullability::Nullable),
        ]
    );
    let (schema, skipped) = load_schema_with_skipped("CREATE TABLE t (a INT NOT NULL)", "sqlite");
    assert!(skipped.is_empty());
    assert_eq!(
        infer_type("SELECT a FROM t", "sqlite", Some(&schema)).as_deref(),
        Some("tuple[int]")
    );
}

#[test]
fn the_type_mapping_starts_over_past_its_size() {
    // Every spelling of a cast type is remembered per thread, up to a bound.
    for n in 0..=4096 {
        analyze_select(&format!("SELECT 1::VARCHAR({n})"), "postgres", None);
    }
    let columns = analyze_select("SELECT 1::VARCHAR(1)", "postgres", None).unwrap();
    assert_eq!(columns[0].python_type.as_deref(), Some("str"));
}

#[test]
fn rarer_text_names_and_items() {
    let names = |sql: &str, dialect: &str| -> Option<Vec<String>> {
        analyze_select(sql, dialect, None)
            .map(|columns| columns.into_iter().map(|c| c.name).collect())
    };
    let some = |names: &[&str]| Some(names.iter().map(ToString::to_string).collect::<Vec<_>>());
    assert_eq!(names("SELECT (T.Id)", "sqlite"), some(&["Id"]));
    assert_eq!(names("SELECT _utf8mb4'x'", "mysql"), some(&["x"]));
    assert_eq!(
        names("SELECT DISTINCT a IS DISTINCT FROM b, c FROM t", "mysql"),
        some(&["a IS DISTINCT FROM b", "c"])
    );
    assert_eq!(
        names("SELECT a, (b + 1) FROM t", "mysql"),
        some(&["a", "(b + 1)"])
    );
}

#[test]
fn only_a_table_has_an_unquoted_reference() {
    let schema = load_schema(
        "CREATE TABLE t (id INT NOT NULL); CREATE TABLE u (n INT)",
        "postgres",
    );
    assert_eq!(
        expand_star("SELECT * FROM public.t, u", "postgres", Some(&schema)).as_deref(),
        Some("SELECT t.id::INT, u.n::INT FROM public.t, u")
    );
    assert_eq!(
        expand_star("SELECT * FROM t, (SELECT 1) s", "postgres", Some(&schema)),
        None
    );
}
