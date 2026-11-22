//! How the statements of a schema change what a query reads: names it defines, the checks
//! and constraints of a column, `ALTER` and its limits, and the spelling of column types.

use sqlty_sql::analyzer::{
    analyze_select, analyze_with_diagnostics, load_schema_with_skipped, Nullability, Schema,
};
use sqlty_sql::diagnostics::Rule;

fn load(sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(sql, dialect).0
}

/// The rules `sql` reports against `schema`.
fn rules(schema: &Schema, dialect: &str, sql: &str) -> Vec<Rule> {
    let (_, diagnostics) = analyze_with_diagnostics(sql, dialect, Some(schema));
    diagnostics.into_iter().map(|d| d.rule).collect()
}

#[test]
fn defined_names_and_unmodelled_system_relations() {
    let c = load(
        "CREATE TABLE pg_orders (id int); CREATE TYPE mood AS ENUM ('a'); \
         CREATE TYPE pt AS (x int); CREATE DOMAIN pct AS int; \
         CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'SELECT 1'; \
         CREATE FUNCTION g() LANGUAGE sql AS 'SELECT 1';",
        "postgres",
    );
    let has = |sql: &str, rule: Rule| rules(&c, "postgres", sql).contains(&rule);
    for name in ["mood", "pt", "pct"] {
        assert!(
            !has(&format!("SELECT NULL::{name}"), Rule::UnknownType),
            "{name}"
        );
    }
    assert!(has("SELECT NULL::nope", Rule::UnknownType));
    assert!(!has("SELECT f(), g()", Rule::UnknownFunction));
    assert!(has("SELECT h()", Rule::UnknownFunction));
    // A table of the schema named like a system relation is read as the schema defines it.
    let star = |name: &str| analyze_select(&format!("SELECT * FROM {name}"), "postgres", Some(&c));
    assert_eq!(star("pg_orders").unwrap_or_default()[0].name, "id");
    assert_eq!(star("pg_class").unwrap_or_default()[0].name, "oid");
    for name in ["pg_trigger", "information_schema.sequences"] {
        let sql = format!("SELECT * FROM {name}");
        assert!(has(&sql, Rule::UnsupportedSyntax), "{name}");
        assert!(!has(&sql, Rule::UnknownTable), "{name}");
    }
    assert!(has("SELECT * FROM users", Rule::UnknownTable));
}

/// The column types as the schema spells them: the same in every dialect that parses them.
#[test]
fn canonical_type_of_common_types_is_the_same_in_every_dialect() {
    let cases = [
        ("INTEGER", "INT"),
        ("INT", "INT"),
        ("INT4", "INT"),
        ("BIGINT", "BIGINT"),
        ("SMALLINT", "SMALLINT"),
        ("TEXT", "TEXT"),
        ("BOOLEAN", "BOOLEAN"),
        ("BOOL", "BOOLEAN"),
        ("UUID", "UUID"),
        ("DATE", "DATE"),
        ("BYTEA", "VARBINARY"),
        ("JSON", "JSON"),
        ("JSONB", "JSONB"),
        ("REAL", "FLOAT"),
        ("DOUBLE PRECISION", "DOUBLE"),
        ("VARCHAR", "VARCHAR"),
        ("CHAR", "CHAR"),
        ("VARCHAR(20)", "VARCHAR(20)"),
        ("TIMESTAMP", "TIMESTAMP"),
        ("TEXT[]", "ARRAY<TEXT>"),
    ];
    for dialect in ["postgres", "mysql", "sqlite", "clickhouse", "bigquery"] {
        for (sql, expected) in cases {
            // Only PostgreSQL and SQLite read an array column.
            if sql == "TEXT[]" && !matches!(dialect, "postgres" | "sqlite") {
                continue;
            }
            let schema = load(&format!("CREATE TABLE t (a {sql})"), dialect);
            let found = analyze_select("SELECT a FROM t", dialect, Some(&schema));
            let got = found.unwrap_or_default().remove(0).sql_type;
            let want = match (dialect, sql) {
                ("mysql", "TIMESTAMP") => "TIMESTAMPTZ",
                ("mysql", "REAL") => "DOUBLE",
                _ => expected,
            };
            assert_eq!(got.as_deref(), Some(want), "{sql} in {dialect}");
        }
    }
}

#[test]
fn sqlite_autoincrement_on_a_column_that_is_not_the_rowid_is_a_default() {
    let schema = |autoincrement: &str| {
        load(
            &format!("CREATE TABLE t (a INT NOT NULL PRIMARY KEY {autoincrement}, b INT NOT NULL)"),
            "sqlite",
        )
    };
    let insert = "INSERT INTO t (b) VALUES (1)";
    assert!(!rules(&schema("AUTOINCREMENT"), "sqlite", insert).contains(&Rule::InsertMissingColumn));
    assert!(rules(&schema(""), "sqlite", insert).contains(&Rule::InsertMissingColumn));
}

#[test]
fn checks_of_a_column_and_a_repeated_column_narrow_each_other() {
    let c = load(
        "CREATE TABLE t (a INT CHECK (a IN (1, 2)) CHECK (a IN (2, 3)), a INT CHECK (a IN (2, 4)), b INT, PRIMARY KEY (a, c, (b)))",
        "postgres",
    );
    let found = analyze_select("SELECT * FROM t", "postgres", Some(&c)).unwrap_or_default();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].python_type.as_deref(), Some("typing.Literal[2]"));
    // The key names `a`, a column it lacks, and an expression: only `a` is a key.
    assert_eq!(found[0].nullability, Nullability::NonNull);
    assert_eq!(found[1].nullability, Nullability::Nullable);
}

#[test]
fn a_count_of_non_nulls_reads_either_way_round() {
    // `Some(true)`: `COALESCE(a, b)` is never NULL, as the check keeps one of them set;
    // `None`: `a` and `b` are each never NULL; `Some(false)`: the check says nothing.
    let cases = [
        ("0 < num_nonnulls(a, b)", Some(true)),
        ("1 <= num_nonnulls(a, b)", Some(true)),
        ("1 = num_nonnulls(a, b)", Some(true)),
        ("num_nonnulls(a, b) > 0", Some(true)),
        ("(b IS NOT NULL) OR (a IS NOT NULL)", Some(true)),
        ("num_nulls(a, b) = 0", None),
        ("1 > num_nonnulls(a, b)", Some(false)),
        ("1 >= num_nonnulls(a, b)", Some(false)),
        ("num_nonnulls(a, b) = a", Some(false)),
        ("num_nonnulls(*) = 1", Some(false)),
        ("current_date > 0", Some(false)),
    ];
    for (check, expected) in cases {
        let c = load(
            &format!("CREATE TABLE t (a int, b int, CHECK ({check}))"),
            "postgres",
        );
        let found = analyze_select("SELECT a, b, COALESCE(a, b) FROM t", "postgres", Some(&c))
            .unwrap_or_default();
        let nullability: Vec<_> = found.iter().map(|c| c.nullability).collect();
        let (nn, nul) = (Nullability::NonNull, Nullability::Nullable);
        let want = match expected {
            Some(true) => [nul, nul, nn],
            None => [nn, nn, nn],
            Some(false) => [nul, nul, nul],
        };
        assert_eq!(nullability, want, "{check}");
    }
}

#[test]
fn a_partition_column_without_a_type_adds_no_column() {
    let c = load(
        "CREATE TABLE q (a int, b int); CREATE TABLE p PARTITION OF q (c WITH OPTIONS NOT NULL) DEFAULT;",
        "postgres",
    );
    let found = analyze_select("SELECT * FROM p", "postgres", Some(&c)).unwrap_or_default();
    let names: Vec<_> = found.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["a", "b"]);
}

/// The labels of the enum `e` in `schema`, from the type of a column of it.
fn labels(schema: &Schema) -> Option<Vec<String>> {
    let found = analyze_select("SELECT c FROM t", "postgres", Some(schema))?;
    let python = found[0].python_type.as_deref()?;
    let inner = python.strip_prefix("typing.Literal[")?.strip_suffix(']')?;
    Some(
        inner
            .split(", ")
            .map(|l| l.trim_matches('\'').to_string())
            .collect(),
    )
}

/// A large enum is indexed so `ADD VALUE` does not scan its labels; its labels follow every
/// kind of change as a plain list would.
#[test]
fn adding_values_to_a_large_enum_follows_a_plain_list() {
    let labels_of = |labels: &[String]| {
        let quoted: Vec<_> = labels.iter().map(|l| format!("'{l}'")).collect();
        format!(
            "CREATE TYPE e AS ENUM ({}); CREATE TABLE t (c e);\n",
            quoted.join(", ")
        )
    };
    // The loader indexes an enum of more than 64 labels.
    let base: Vec<String> = (0..=64).map(|i| format!("l{i}")).collect();
    let mut model = base.clone();
    let mut sql = labels_of(&base);
    let mut alter = |statement: &str, change: &dyn Fn(&mut Vec<String>)| {
        sql += &format!("{statement};\n");
        change(&mut model);
        (sql.clone(), model.clone())
    };
    let at = |model: &[String], label: &str| model.iter().position(|l| l == label);
    let steps: Vec<(String, Vec<String>)> = vec![
        alter("ALTER TYPE e ADD VALUE 'x'", &|m| m.push("x".into())),
        alter("ALTER TYPE e ADD VALUE 'x'", &|_| ()),
        alter("ALTER TYPE e ADD VALUE 'l3'", &|_| ()),
        alter("ALTER TYPE e ADD VALUE 'y' BEFORE 'l0'", &|m| {
            m.insert(0, "y".into());
        }),
        alter("ALTER TYPE e ADD VALUE 'z' AFTER 'l1'", &|m| {
            m.insert(at(m, "l1").unwrap_or_default() + 1, "z".into());
        }),
        alter("ALTER TYPE e RENAME VALUE 'x' TO 'w'", &|m| {
            let i = at(m, "x").unwrap_or_default();
            m[i] = "w".into();
        }),
        alter("ALTER TYPE e ADD VALUE 'x'", &|m| m.push("x".into())),
        // Statements that do not write the type leave its labels as they are.
        alter("DROP INDEX ix", &|_| ()),
        alter("DROP SCHEMA s", &|_| ()),
        alter("DROP DOMAIN d", &|_| ()),
        alter("ALTER TYPE e ADD VALUE IF NOT EXISTS 'x'", &|_| ()),
    ];
    for (sql, model) in steps {
        assert_eq!(labels(&load(&sql, "postgres")), Some(model), "{sql}");
    }
}

#[test]
fn a_dropped_large_enum_forgets_its_labels() {
    let labels_of = |labels: &[&str]| {
        let quoted: Vec<_> = labels.iter().map(|l| format!("'{l}'")).collect();
        format!(
            "CREATE TYPE e AS ENUM ({}); CREATE TABLE t (c e);\n",
            quoted.join(", ")
        )
    };
    let many: Vec<String> = (0..=64).map(|i| format!("l{i}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    // The type is dropped and made again with other labels: `x` is new, not a duplicate.
    for drop in ["DROP TYPE e", "DROP DOMAIN e"] {
        let sql = format!(
            "{}ALTER TYPE e ADD VALUE 'x'; {drop}; {}ALTER TYPE e ADD VALUE IF NOT EXISTS 'x';",
            labels_of(&many),
            labels_of(&["a"]),
        );
        assert_eq!(
            labels(&load(&sql, "postgres")),
            Some(vec!["a".into(), "x".into()]),
            "{drop}"
        );
    }
}

#[test]
fn changing_a_table_that_is_not_there_does_nothing() {
    let c = load(
        "ALTER TABLE t ADD COLUMN a int; CREATE TABLE u (b int);",
        "postgres",
    );
    let found = analyze_select("SELECT * FROM t", "postgres", Some(&c)).unwrap_or_default();
    assert!(rules(&c, "postgres", "SELECT * FROM t").contains(&Rule::UnknownTable));
    assert_eq!(found[0].name, "*");
}

#[test]
fn an_operation_it_cannot_follow_leaves_the_table_unknown() {
    // Only dialects whose `ALTER` the loader does not read parse these.
    for dialect in ["postgres", "sqlite"] {
        let (c, skipped) = load_schema_with_skipped(
            "CREATE TABLE t (a INT); ALTER TABLE t SWAP WITH u;",
            dialect,
        );
        assert_eq!(skipped, [("table", "t".to_string())], "{dialect}");
        let found = analyze_select("SELECT * FROM t", dialect, Some(&c)).unwrap_or_default();
        assert_eq!(found[0].name, "*", "{dialect}");
    }
}

/// Whether `SELECT * FROM {name}` finds a table of `schema`.
fn finds(schema: &Schema, dialect: &str, name: &str) -> bool {
    let found = analyze_select(&format!("SELECT * FROM {name}"), dialect, Some(schema));
    found.is_some_and(|columns| columns.first().is_some_and(|c| c.name != "*"))
}

#[test]
fn a_name_is_found_by_its_folded_parts_in_every_dialect() {
    let postgres = load(
        "CREATE TABLE Users (id int); CREATE TABLE \"Mixed\" (id int); \
         CREATE SCHEMA \"Public2\"; CREATE TABLE \"Public2\".\"Users\" (id int);",
        "postgres",
    );
    for (name, found) in [
        ("users", true),
        ("USERS", true),
        ("public.Users", true),
        ("\"Mixed\"", true),
        ("mixed", false),
        ("\"Users\"", false),
        ("\"Public2\".\"Users\"", true),
        ("public2.users", false),
    ] {
        assert_eq!(finds(&postgres, "postgres", name), found, "postgres {name}");
    }
    for dialect in ["mysql", "sqlite"] {
        let schema = load(
            "CREATE TABLE Users (id INT); CREATE TABLE db.Orders (id INT);",
            dialect,
        );
        for name in ["Users", "users", "USERS", "Orders", "db.orders"] {
            assert!(finds(&schema, dialect, name), "{dialect} {name}");
        }
        assert!(!finds(&schema, dialect, "nobody"), "{dialect}");
    }
}

#[test]
fn a_bare_name_is_found_along_the_search_path() {
    let long = "x".repeat(200);
    let schema = load(
        &format!(
            "CREATE SCHEMA app; SET search_path = public, app; \
             CREATE TABLE app.users (id int); CREATE TABLE app.{long} (id int);"
        ),
        "postgres",
    );
    assert!(finds(&schema, "postgres", "users"));
    assert!(finds(&schema, "postgres", &long));
    assert!(!finds(&schema, "postgres", "missing"));
}

/// The Python type of the one column `sql` returns against `ddl`.
fn python_of(ddl: &str, sql: &str) -> String {
    let c = load(ddl, "postgres");
    let found = analyze_select(sql, "postgres", Some(&c)).unwrap_or_default();
    found[0].python_type.clone().unwrap_or_default()
}

#[test]
fn literal_values_round_trip_through_a_view() {
    let table = "CREATE TABLE t (c TEXT CHECK (c IN ('it''s', 'a\\b', 'line\nbreak', 'a\rb', 'c\td', '\u{1}')), \
                 d INT CHECK (d IN (1, -2)), e TEXT CHECK (e IN ('a', 'b')));";
    let view = format!("{table} CREATE VIEW v AS SELECT c, d, e FROM t;");
    let c = r"typing.Literal['it\'s', 'a\\b', 'line\nbreak', 'a\rb', 'c\td', '\x01']";
    assert_eq!(python_of(&view, "SELECT c FROM t"), c);
    assert_eq!(python_of(&view, "SELECT c FROM v"), c);
    assert_eq!(python_of(&view, "SELECT d FROM v"), "typing.Literal[1, -2]");
}

#[test]
fn only_a_set_of_one_kind_renders_as_a_literal() {
    let table = "CREATE TABLE t (d INT CHECK (d IN (1, -2)), e TEXT CHECK (e IN ('a', 'b'))); \
                 CREATE VIEW v AS SELECT CASE WHEN true THEN e ELSE d::text END AS mixed, \
                 ARRAY[e] AS arr, e::text AS cast, d + 1 AS plus FROM t;";
    for (column, python) in [
        ("mixed", "str"),
        ("arr", "list[str]"),
        ("cast", "str"),
        ("plus", "int"),
    ] {
        assert_eq!(
            python_of(table, &format!("SELECT {column} FROM v")),
            python,
            "{column}"
        );
    }
    // A set of integers and of strings together is no literal.
    assert_eq!(
        python_of(table, "SELECT CASE WHEN true THEN e ELSE d END FROM t"),
        ""
    );
}

#[test]
fn a_set_past_the_literal_limit_is_its_base_type() {
    let values = |n: usize| (0..n).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    let python = |n: usize| {
        python_of(
            &format!("CREATE TABLE t (c INT CHECK (c IN ({})))", values(n)),
            "SELECT c FROM t",
        )
    };
    assert!(python(32).starts_with("typing.Literal[0, 1, "));
    assert_eq!(python(33), "int");
}

/// `name:SQL_TYPE:nullability` of each column of `SELECT * FROM {name}`.
fn star(schema: &Schema, dialect: &str, name: &str) -> Vec<String> {
    let found = analyze_select(&format!("SELECT * FROM {name}"), dialect, Some(schema));
    found
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            format!(
                "{}:{}:{:?}",
                c.name,
                c.sql_type.unwrap_or_default(),
                c.nullability
            )
        })
        .collect()
}

fn nullability(schema: &Schema, dialect: &str, sql: &str) -> Vec<Nullability> {
    let found = analyze_select(sql, dialect, Some(schema)).unwrap_or_default();
    found.into_iter().map(|c| c.nullability).collect()
}

/// The statements a table takes in place change it as a copy would.
#[test]
fn statements_on_one_table_change_it_as_before() {
    let (c, skipped) = load_schema_with_skipped(
        "CREATE TABLE t (id int PRIMARY KEY, a int, b int);
        CREATE TABLE p (id int); CREATE TABLE ch () INHERITS (p);
        CREATE VIEW v0 AS SELECT * FROM t;
        CREATE VIEW w AS SELECT id FROM t;
        CREATE UNIQUE INDEX on_view ON w (id);
        CREATE TRIGGER on_view BEFORE INSERT ON w FOR EACH ROW EXECUTE FUNCTION f();
        ALTER TABLE t ADD COLUMN c int;
        ALTER TABLE t ADD COLUMN c text;
        ALTER TABLE t ADD COLUMN r int REFERENCES t;
        ALTER TABLE t ADD CONSTRAINT u UNIQUE (a, b);
        ALTER TABLE t ADD CONSTRAINT x UNIQUE (lower(b::text));
        ALTER TABLE t ADD CONSTRAINT f FOREIGN KEY (a) REFERENCES t (id);
        ALTER TABLE t ADD CONSTRAINT g FOREIGN KEY (b) REFERENCES missing (id);
        ALTER TABLE t ADD COLUMN m int, ADD CONSTRAINT h FOREIGN KEY (m) REFERENCES t (id);
        ALTER TABLE t ADD COLUMN n int, ADD CONSTRAINT i FOREIGN KEY (n) REFERENCES missing (id);
        ALTER TABLE t ADD CONSTRAINT k1 CHECK (b IS NOT NULL);
        ALTER TABLE t ADD CONSTRAINT k2 CHECK (a > 0);
        ALTER TABLE t ADD CONSTRAINT k3 CHECK (a IS NOT NULL OR c IS NOT NULL);
        CREATE TRIGGER g1 BEFORE INSERT ON t FOR EACH ROW EXECUTE FUNCTION f();
        DROP TRIGGER g1 ON t;
        DROP TRIGGER g1 ON t;
        ALTER TABLE p ADD COLUMN b int;
        ALTER TABLE ONLY p ADD COLUMN only_p int;
        ALTER TABLE w ADD COLUMN z int;
        DROP TABLE ch;
        CREATE VIEW v1 AS SELECT * FROM t;
        CREATE VIEW v2 AS SELECT * FROM p;",
        "postgres",
    );
    // Altering a view is not read: the view is left unknown.
    assert_eq!(skipped, [("table", "w".to_string())]);
    assert_eq!(
        star(&c, "postgres", "t"),
        [
            "id:INT:NonNull",
            "a:INT:Nullable",
            "b:INT:Nullable",
            "c:INT:Nullable",
            "r:INT:Nullable",
            "m:INT:Nullable",
            "n:INT:Nullable"
        ]
    );
    // A view holds the columns its table had when it was made.
    assert_eq!(star(&c, "postgres", "v0").len(), 3);
    assert_eq!(star(&c, "postgres", "v1").len(), 7);
    assert_eq!(star(&c, "postgres", "v2").len(), 3);
    assert_eq!(
        star(&c, "postgres", "p"),
        ["id:INT:Nullable", "b:INT:Nullable", "only_p:INT:Nullable"]
    );
    assert_eq!(star(&c, "postgres", "w"), ["*::Unknown"]);
    assert_eq!(star(&c, "postgres", "ch"), ["*::Unknown"]);
    // The keys: the primary key and `(a, b)`, not the expression.
    let unique = |columns: &[&str]| {
        let on = |left: &str, right: &str| {
            columns
                .iter()
                .map(|c| format!("{left}.{c} = {right}.{c}"))
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let sql = format!(
            "SELECT SUM(LENGTH(z.id::text)) FROM t p JOIN t q ON {} JOIN t z ON {}",
            on("q", "p"),
            on("z", "p")
        );
        let (_, found) = analyze_with_diagnostics(&sql, "postgres", Some(&c));
        !found.iter().any(|d| d.rule == Rule::UnprovenJoin)
    };
    assert!(unique(&["id"]) && unique(&["a", "b"]));
    assert!(!unique(&["a"]) && !unique(&["b"]));
    // The foreign keys of `t`, in the order they were added, then those that lead to it.
    let joins: Vec<_> = c
        .joins_with_prefix("t", "", 10)
        .iter()
        .map(|j| {
            (
                j.table.name.to_string(),
                j.pairs
                    .iter()
                    .map(|(a, b)| format!("{a}={b}"))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let t = |pair: &str| ("t".to_string(), vec![pair.to_string()]);
    assert_eq!(
        joins,
        [
            t("id=r"),
            t("id=a"),
            t("id=m"),
            t("r=id"),
            t("a=id"),
            t("m=id")
        ]
    );
    // Only the primary key and what the checks prove are NOT NULL: `b`, and `c` or `a`.
    assert_eq!(
        nullability(&c, "postgres", "SELECT a, b, COALESCE(c, a) FROM t"),
        [
            Nullability::Nullable,
            Nullability::NonNull,
            Nullability::NonNull
        ]
    );
    assert_eq!(
        nullability(&c, "postgres", "SELECT c, COALESCE(c, id) FROM t")[0],
        Nullability::Nullable
    );
    // The trigger was dropped: an insert returns what it writes.
    assert_eq!(
        nullability(&c, "postgres", "INSERT INTO t (a) VALUES (1) RETURNING a"),
        [Nullability::NonNull]
    );

    let m = load(
        "CREATE TABLE t (id INT, a INT);
        CREATE TRIGGER g1 BEFORE INSERT ON t FOR EACH ROW SET NEW.a = 1;
        CREATE TRIGGER g2 BEFORE UPDATE ON t FOR EACH ROW SET NEW.a = 1;
        ALTER TABLE t ADD COLUMN b INT AFTER id;
        ALTER TABLE t ADD COLUMN c INT AFTER missing;
        ALTER TABLE t ADD COLUMN d INT FIRST;
        DROP TRIGGER g1;",
        "mysql",
    );
    let names: Vec<_> = star(&m, "mysql", "t")
        .iter()
        .map(|c| c.split(':').next().unwrap_or_default().to_string())
        .collect();
    assert_eq!(names, ["d", "id", "b", "a"]);
    // `g1` was dropped, `g2` remains: an update may change the row, an insert may not.
    assert_eq!(
        nullability(&m, "mysql", "INSERT INTO t (a) VALUES (1) RETURNING a"),
        [Nullability::NonNull]
    );
    assert_eq!(
        nullability(&m, "mysql", "UPDATE t SET a = 1 RETURNING a"),
        [Nullability::Nullable]
    );
}
