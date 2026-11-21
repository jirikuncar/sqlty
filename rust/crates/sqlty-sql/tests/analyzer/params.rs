use crate::support::*;
use sqlty_sql::analyzer::*;
const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, \
                      age INTEGER, born DATE, tags TEXT[], uid UUID NOT NULL);
                      CREATE TABLE orders (id INT, user_id INT, total NUMERIC(10, 2));";

/// The schema without the PostgreSQL-only types, for the other dialects.
fn schema_for(dialect: &str) -> String {
    match dialect {
        "postgres" => SCHEMA.to_string(),
        _ => SCHEMA.replace(", tags TEXT[]", "").replace("UUID", "TEXT"),
    }
}

/// `"index:name:type"`, with `!` for non-null and `?` for nullable.
fn params(sql: &str, dialect: &str) -> Vec<String> {
    let schema = load_schema(&schema_for(dialect), dialect);
    analyze_params(sql, dialect, Some(&schema))
        .unwrap_or_else(|| panic!("{sql} does not parse"))
        .iter()
        .map(|p| {
            let mark = match p.nullability {
                Nullability::NonNull => "!",
                Nullability::Nullable => "?",
                Nullability::Unknown => "",
            };
            format!("{}{mark}", p.python_type.as_deref().unwrap_or("-"))
        })
        .collect()
}

fn pg(sql: &str) -> Vec<String> {
    params(sql, "postgres")
}

#[test]
fn comparisons() {
    assert_eq!(
        pg("SELECT 1 FROM users WHERE id = %s AND name <> %s"),
        ["int", "str"]
    );
    assert_eq!(pg("SELECT 1 FROM users WHERE %s < age"), ["int"]);
    assert_eq!(
        pg("SELECT 1 FROM users u WHERE u.born >= $1"),
        ["datetime.date"]
    );
    assert_eq!(
        pg("SELECT 1 FROM users WHERE id BETWEEN %s AND %s"),
        ["int", "int"]
    );
    assert_eq!(pg("SELECT 1 FROM users WHERE name LIKE %s"), ["str"]);
    assert_eq!(
        pg("SELECT 1 FROM users WHERE id = %s OR id = %s"),
        ["int", "int"]
    );
    assert_eq!(pg("SELECT 1 FROM users WHERE 1 = %s"), ["int"]);
    assert_eq!(
        pg("SELECT 1 FROM users WHERE %s = ANY(ARRAY['a', 'b'])"),
        ["str"]
    );
}

#[test]
fn named_parameters_are_shared() {
    let schema = load_schema(SCHEMA, "postgres");
    let found = analyze_params(
        "SELECT 1 FROM users WHERE id = %(id)s OR age > %(id)s OR name = %(n)s",
        "postgres",
        Some(&schema),
    )
    .unwrap();
    let summary: Vec<_> = found
        .iter()
        .map(|p| (p.index, p.name.as_deref(), p.python_type.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [(1, Some("id"), Some("int")), (2, Some("n"), Some("str"))]
    );
}

#[test]
fn lists_and_arrays() {
    assert_eq!(
        pg("SELECT 1 FROM users WHERE id IN (%s, %s)"),
        ["int", "int"]
    );
    assert_eq!(pg("SELECT 1 FROM users WHERE name IN (%s, 'a')"), ["str"]);
    assert_eq!(pg("SELECT 1 FROM users WHERE id = ANY(%s)"), ["list[int]"]);
    assert_eq!(pg("SELECT 1 FROM users WHERE id = ANY($1)"), ["list[int]"]);
    assert_eq!(pg("SELECT 1 FROM users WHERE %s = ANY(tags)"), ["str"]);
    assert_eq!(
        pg("SELECT 1 FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > %s)"),
        ["decimal.Decimal"]
    );
    assert_eq!(
        pg("SELECT 1 FROM orders WHERE user_id IN (SELECT id FROM users WHERE name = %s)"),
        ["str"]
    );
    assert_eq!(
        pg("SELECT 1 FROM orders WHERE %s IN (SELECT id FROM users)"),
        ["int"]
    );
}

#[test]
fn inserts_and_updates() {
    assert_eq!(
        pg("INSERT INTO users (id, name, age) VALUES (%s, %s, %s)"),
        ["int!", "str!", "int?"]
    );
    assert_eq!(
        pg("INSERT INTO users VALUES (%s, %s, %s), (%s, %s, %s)"),
        ["int!", "str!", "int?", "int!", "str!", "int?"]
    );
    assert_eq!(
        pg("UPDATE users SET name = %s, age = %s WHERE id = %s"),
        ["str!", "int?", "int"]
    );
    assert_eq!(
        pg("INSERT INTO users (id, name) VALUES (%s, %s) ON CONFLICT (id) DO UPDATE SET name = %s WHERE users.age > %s"),
        ["int!", "str!", "str!", "int"]
    );
    assert_eq!(pg("DELETE FROM users WHERE id = %s LIMIT 1"), ["int"]);
    assert_eq!(
        params("INSERT INTO users (id, name) VALUES (?, ?)", "sqlite"),
        ["int!", "str!"]
    );
    assert_eq!(
        pg("INSERT INTO users (id, name) SELECT id, %s FROM users WHERE age = %s"),
        ["str!", "int"]
    );
}

#[test]
fn casts_limits_and_functions() {
    assert_eq!(
        pg("SELECT %s::int, CAST(%s AS date), %s::text[]"),
        ["int", "datetime.date", "list[str]"]
    );
    assert_eq!(pg("SELECT 1 FROM users LIMIT %s OFFSET %s"), ["int", "int"]);
    assert_eq!(
        pg("SELECT 1 FROM users OFFSET $1 ROWS FETCH FIRST $2 ROWS ONLY"),
        ["int", "int"]
    );
    assert_eq!(
        pg("SELECT 1 FROM users WHERE lower(name) = lower(%s)"),
        ["str",]
    );
    assert_eq!(pg("SELECT lpad(name, %s, %s) FROM users"), ["int", "str"]);
    assert_eq!(pg("SELECT coalesce(age, %s) FROM users"), ["int"]);
    assert_eq!(
        pg("SELECT 1 FROM users WHERE name = %s::text AND age = %s"),
        ["str", "int"]
    );
    assert_eq!(pg("SELECT %s, %s"), ["-", "-"]);
}

#[test]
fn scopes() {
    assert_eq!(
        pg("SELECT 1 FROM users u JOIN orders o ON o.user_id = u.id AND o.total > %s WHERE u.name = %s"),
        ["decimal.Decimal", "str"]
    );
    assert_eq!(
        pg("SELECT 1 FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND u.age > %s)"),
        ["int"]
    );
    assert_eq!(
        pg("WITH t AS (SELECT id AS k FROM users WHERE name = %s) SELECT 1 FROM t WHERE k = %s"),
        ["str", "int"]
    );
    assert_eq!(
        pg("SELECT 1 FROM (SELECT age AS a FROM users) s WHERE s.a = %s"),
        ["int"]
    );
    assert_eq!(pg("SELECT 1 FROM nosuch WHERE x = %s"), ["-"]);
    assert_eq!(
        pg("SELECT 1 UNION SELECT id FROM users WHERE name = %s"),
        ["str"]
    );
}

#[test]
fn spellings() {
    assert_eq!(
        pg("SELECT 1 FROM users WHERE id = $2 AND name = $1"),
        ["str", "int"]
    );
    assert_eq!(
        params("SELECT 1 FROM users WHERE id = ?1", "sqlite"),
        ["int"]
    );
    assert_eq!(
        params("SELECT 1 FROM users WHERE id = :id AND name = :n", "sqlite"),
        ["int", "str"]
    );
    assert_eq!(params("SELECT 1 FROM users WHERE id = ?", "mysql"), ["int"]);
    assert_eq!(
        params("SELECT 1 FROM users WHERE id = %s", "mysql"),
        ["int"]
    );
    // `?` is a JSONB operator in PostgreSQL unless the query doesn't parse with it.
    assert_eq!(pg("SELECT 1 FROM users WHERE id = ?"), ["int"]);
    assert_eq!(
        pg("SELECT '[]'::jsonb ? 'a' FROM users WHERE id = %s"),
        ["int"]
    );
}

#[test]
fn no_parameters_and_unparsed() {
    assert!(pg("SELECT 1").is_empty());
    let schema = load_schema(SCHEMA, "postgres");
    assert_eq!(
        analyze_params("SELEC 1 %s", "postgres", Some(&schema)),
        None
    );
}

#[test]
fn placeholder_numbers_past_max_params_are_not_parameters() {
    assert_eq!(
        params("SELECT ?65535 = id FROM users", "sqlite").len(),
        65535
    );
    assert!(params("SELECT ?99999999999 = id FROM users", "sqlite").is_empty());
    assert!(pg("SELECT $65536").is_empty());
}

/// Statements and expressions whose placeholders only some code reads.
#[test]
fn rarer_shapes() {
    const MORE: &str = "CREATE TABLE things (x mytype, ip INET);
        CREATE FUNCTION f(x mytype, y INT) RETURNS INT LANGUAGE sql AS 'SELECT 1';";
    let found = |dialect: &str, sql: &str| -> Vec<String> {
        let schema = load_schema(&format!("{}{MORE}", schema_for(dialect)), dialect);
        analyze_params(sql, dialect, Some(&schema))
            .unwrap()
            .iter()
            .map(|p| {
                let mark = match p.nullability {
                    Nullability::NonNull => "!",
                    Nullability::Nullable => "?",
                    Nullability::Unknown => "",
                };
                format!("{}{mark}", p.python_type.as_deref().unwrap_or("-"))
            })
            .collect()
    };
    let inet = "ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface";
    for (dialect, sql, expected) in [
        (
            "postgres",
            "UPDATE things SET x = %s + INTERVAL '1 day'",
            vec!["-"],
        ),
        (
            "postgres",
            "UPDATE users SET age = %s + INTERVAL '1 day'",
            vec!["-"],
        ),
        (
            "postgres",
            "SELECT 1 FROM users WHERE born = $1 AND born < $1 + INTERVAL '1 day'",
            vec!["datetime.date"],
        ),
        (
            "postgres",
            "UPDATE users SET name = $1, age = $1",
            vec!["str!"],
        ),
        (
            "mysql",
            "UPDATE (SELECT * FROM users) u SET u.name = %s",
            vec!["-"],
        ),
        ("postgres", "SET search_path = %s", vec!["-"]),
        ("postgres", "SELECT 1 FROM nope WHERE a = %s", vec!["-"]),
        (
            "mysql",
            "SELECT 1 FROM users LIMIT %s, %s",
            vec!["int", "int"],
        ),
        (
            "postgres",
            "(SELECT 1 FROM users WHERE id = %s)",
            vec!["int"],
        ),
        ("postgres", "TABLE users", vec![]),
        (
            "postgres",
            "SELECT * FROM generate_series(1, stop => %s)",
            vec!["-"],
        ),
        (
            "postgres",
            "INSERT INTO users (id) VALUES (%s, %s)",
            vec!["int!", "-"],
        ),
        (
            "postgres",
            "INSERT INTO users SELECT *, %s FROM users",
            vec!["-"],
        ),
        (
            "postgres",
            "INSERT INTO users (id) SELECT %s, %s",
            vec!["int!", "-"],
        ),
        (
            "postgres",
            "INSERT INTO users (id) SELECT 1 UNION SELECT %s",
            vec!["int!"],
        ),
        ("postgres", "INSERT INTO users DEFAULT VALUES", vec![]),
        ("mysql", "INSERT INTO users SET name = %s", vec!["str!"]),
        (
            "mysql",
            "INSERT INTO users (id) VALUES (1) ON DUPLICATE KEY UPDATE name = %s",
            vec!["str!"],
        ),
        (
            "postgres",
            "INSERT INTO users (id) VALUES (1) ON CONFLICT (id) DO UPDATE SET name = %s",
            vec!["str!"],
        ),
        (
            "postgres",
            "INSERT INTO users (id) VALUES (1) ON CONFLICT DO NOTHING",
            vec![],
        ),
        (
            "postgres",
            "UPDATE users SET (name, age) = (%s, %s)",
            vec!["str!", "int?"],
        ),
        (
            "postgres",
            "UPDATE users SET (name, age) = f(%s)",
            vec!["-"],
        ),
        (
            "postgres",
            "INSERT INTO users (id) SELECT %s UNION TABLE users",
            vec!["int!"],
        ),
        // Only PostgreSQL and MySQL queries read `TABLE name` as `SELECT * FROM name`.
        (
            "sqlite",
            "INSERT INTO users (id) SELECT ? UNION TABLE users",
            vec!["int!"],
        ),
        (
            "postgres",
            "UPDATE users SET nope = %s + INTERVAL '1 day'",
            vec!["-"],
        ),
        (
            "postgres",
            "SELECT tags || %s FROM users",
            vec!["list[str]"],
        ),
        (
            "postgres",
            "SELECT ip <<= %s, ip >>= %s, ip << %s FROM things",
            vec![inet, inet, inet],
        ),
        (
            "postgres",
            "SELECT 1 FROM users WHERE id = SOME(%s)",
            vec!["list[int]"],
        ),
        (
            "mysql",
            "SELECT 1 FROM users WHERE id = ANY(%s)",
            vec!["list[int]"],
        ),
        (
            "sqlite",
            "SELECT 1 FROM users WHERE id = ANY(%s)",
            vec!["list[int]"],
        ),
        (
            "postgres",
            "SELECT 1 FROM users WHERE %s = ANY(name)",
            vec!["-"],
        ),
        (
            "postgres",
            "SELECT born AT TIME ZONE 'UTC' FROM users",
            vec![],
        ),
        (
            "postgres",
            "SELECT CASE age WHEN %s THEN 1 END FROM users",
            vec!["int"],
        ),
        (
            "postgres",
            "SELECT 1 FROM users WHERE %s IN (SELECT id, name FROM users)",
            vec!["-"],
        ),
        (
            "postgres",
            "SELECT 1 FROM users WHERE %s IN (SELECT x FROM things)",
            vec!["-"],
        ),
        ("postgres", "SELECT %s::unknowntype", vec!["-"]),
        (
            "postgres",
            "SELECT SUBSTRING(%s FROM 1 FOR %s)",
            vec!["str", "int"],
        ),
        (
            "postgres",
            "SELECT generate_series(%s::date, %s::date, %s)",
            vec!["datetime.date", "datetime.date", "datetime.timedelta"],
        ),
        ("postgres", "SELECT generate_series(1, 10, %s)", vec!["int"]),
        (
            "postgres",
            "SELECT generate_series(%s::timestamp, %s::timestamp, '1 day')",
            vec!["datetime.datetime", "datetime.datetime"],
        ),
        ("postgres", "SELECT f(%s, %s)", vec!["-", "int"]),
    ] {
        assert_eq!(found(dialect, sql), expected, "{sql}");
    }
}

#[test]
fn parsed_params_equal_analyze_params() {
    const QUERIES: &[&str] = &[
        "",
        "SELEC 1 %s",
        "SELECT 1",
        "SELECT $1",
        "SELECT $2::int, $1",
        "SELECT %s::int",
        "SELECT %(a)s, %(b)s, %(a)s",
        "SELECT * FROM users WHERE id = %s",
        "SELECT * FROM users WHERE id = $1 AND name = $2",
        "SELECT * FROM users WHERE name = :name AND age > @age",
        "SELECT * FROM users WHERE id = ?",
        "SELECT * FROM users WHERE id IN (%s, %s)",
        "SELECT * FROM users WHERE tags @> %s",
        "SELECT * FROM users WHERE data ? 'a'",
        "SELECT * FROM users WHERE data ? 'a' AND id = ?",
        "SELECT * FROM users LIMIT %s OFFSET %s",
        "SELECT * FROM users WHERE born + INTERVAL '1 day' > %s",
        "SELECT %s + INTERVAL '1 day'",
        "SELECT SUBSTRING(%s FROM 1 FOR %s)",
        "SELECT generate_series(1, 10, %s)",
        "SELECT f(%s, %s)",
        "SELECT 1 FROM users WHERE %s IN (SELECT x FROM things)",
        "INSERT INTO users (id, name) VALUES (%s, %s)",
        "INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET name = $3",
        "UPDATE users SET name = %s WHERE id = %s",
        "UPDATE users SET name = :n WHERE id = :i",
        "DELETE FROM users WHERE id = %s LIMIT 1",
        "WITH c AS (SELECT id FROM users WHERE id = %s) SELECT * FROM c",
        "SELECT u.id FROM users u JOIN orders o ON o.user_id = u.id WHERE o.total > %s",
        "CREATE TABLE t (a INT)",
    ];
    for dialect in ["postgres", "mysql", "sqlite"] {
        let schema = load_schema(&schema_for(dialect), dialect);
        for sql in QUERIES {
            let parsed = parse(sql, dialect);
            for schema in [None, Some(&*schema)] {
                assert_eq!(
                    parsed.params(sql, schema),
                    analyze_params(sql, dialect, schema),
                    "{dialect}: {sql}"
                );
            }
        }
    }
}

#[test]
fn parsed_params_follow_the_database_a_query_names() {
    use sqlty_sql::analyzer::{route, Databases};
    let other = load_schema("CREATE TABLE events (n INTEGER NOT NULL)", "sqlite");
    let schema = SchemaData::with_databases(
        load_schema(&schema_for("postgres"), "postgres"),
        Databases {
            name: "default".to_string(),
            absent: false,
            others: vec![("other".to_string(), "sqlite", Some(other))],
        },
    );
    let schema = schema.as_deref();
    for sql in [
        "-- sqlty: db=other\nSELECT n FROM events WHERE n = ?",
        "-- sqlty: db=other\nSELECT $1::int",
        "-- sqlty: db=missing\nSELECT id FROM users WHERE id = %s",
    ] {
        let (routed, routed_schema) = route(sql, "postgres", schema);
        assert_eq!(
            parse(sql, routed).params(sql, routed_schema),
            analyze_params(sql, "postgres", schema),
            "{sql}"
        );
    }
}
