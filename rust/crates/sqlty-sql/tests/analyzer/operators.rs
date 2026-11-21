use crate::support::*;
use sqlty_sql::analyzer::{analyze_select, Schema};
#[test]
fn binary_operators_in_queries() {
    let ddl = "CREATE TABLE t (i INT NOT NULL, s SMALLINT, b BIGINT, n NUMERIC(8, 2), \
               d DATE, ts TIMESTAMP, tz TIMESTAMPTZ, iv INTERVAL, txt TEXT, flag BOOLEAN)";
    let queries = [
        "SELECT i + s AS a, i * b AS b, s - n AS c, i / i AS d, i % s AS e FROM t",
        "SELECT i + 1.5 AS a, i + 1 AS b, n * 2 AS c FROM t",
        "SELECT d + 1 AS a, d - d AS b, d + iv AS c, ts - ts AS d, tz - iv AS e, iv * 2 AS f FROM t",
        "SELECT txt || 'x' AS a, txt || i AS b, 'a' || 'b' AS c FROM t",
        "SELECT i = 1 AS a, txt LIKE 'a%' AS b, i IS NULL AS c, NOT flag AS d, i BETWEEN 1 AND 2 AS e FROM t",
        "SELECT i IN (1, 2) AS a, i = ANY(ARRAY[1, 2]) AS b, flag AND true AS c, flag OR NULL AS d FROM t",
        "SELECT NULL + 1 AS a, 1 + NULL AS b, NULL AS c FROM t",
        "SELECT i + txt AS a FROM t",
        "SELECT 2 ^ 3 AS a, 7 & 3 AS b, 1 << 2 AS c, -i AS d, +n AS e, ~i AS f FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
}

#[test]
fn division_differs_between_postgres_and_mysql() {
    let ddl = "CREATE TABLE t (a INT NOT NULL, b INT NOT NULL)";
    let query = ["SELECT a / b AS q, a DIV b AS d, a % b AS m FROM t"];
    let postgres = reports(&query, "postgres", Some(ddl));
    let mysql = reports(&query, "mysql", Some(ddl));
    insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
}

/// The type of `left op right` over columns of the two types, as a query infers it.
fn arithmetic_type(left: &str, symbol: &str, right: &str, dialect: &str) -> Option<String> {
    thread_local! {
        static SCHEMAS: std::cell::RefCell<std::collections::HashMap<String, Schema>> =
            std::cell::RefCell::default();
    }
    const TYPES: [&str; 11] = [
        "SMALLINT",
        "INT",
        "BIGINT",
        "DECIMAL",
        "DOUBLE",
        "DATE",
        "TIME",
        "TIMESTAMP",
        "TIMESTAMPTZ",
        "INTERVAL",
        "TEXT",
    ];
    let column = |ty: &str| format!("c_{}", ty.to_lowercase());
    let ddl = TYPES
        .iter()
        .map(|ty| {
            let declared = if *ty == "DOUBLE" {
                "DOUBLE PRECISION"
            } else {
                ty
            };
            format!("{} {declared}", column(ty))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let schema = SCHEMAS.with_borrow_mut(|schemas| {
        schemas
            .entry(dialect.to_string())
            .or_insert_with(|| load_schema(&format!("CREATE TABLE t ({ddl})"), dialect))
            .clone()
    });
    analyze_select(
        &format!(
            "SELECT {} {symbol} {} AS x FROM t",
            column(left),
            column(right)
        ),
        dialect,
        Some(&schema),
    )
    .and_then(|columns| columns[0].sql_type.clone())
}

#[test]
fn arithmetic_promotes_numbers_and_types_date_math() {
    let operators = ["+", "-", "*", "/"];
    let types = [
        "SMALLINT",
        "INT",
        "BIGINT",
        "DECIMAL",
        "DOUBLE",
        "DATE",
        "TIME",
        "TIMESTAMP",
        "TIMESTAMPTZ",
        "INTERVAL",
        "TEXT",
    ];
    let mut lines = Vec::new();
    for symbol in operators {
        for left in types {
            for right in types {
                if let Some(result) = arithmetic_type(left, symbol, right, "postgres") {
                    lines.push(format!("{left} {symbol} {right} = {result}"));
                }
            }
        }
    }
    insta::assert_snapshot!(lines.join("\n"));
}

#[test]
fn mysql_division_is_at_least_decimal() {
    assert_eq!(
        arithmetic_type("INT", "/", "INT", "mysql").as_deref(),
        Some("DECIMAL")
    );
    assert_eq!(
        arithmetic_type("INT", "/", "INT", "postgres").as_deref(),
        Some("INT")
    );
    assert_eq!(
        arithmetic_type("INT", "/", "DOUBLE", "mysql").as_deref(),
        Some("DOUBLE")
    );
}

#[test]
fn arithmetic_with_an_unknown_operand_has_no_type() {
    assert_eq!(arithmetic_type("INT", "+", "TEXT", "postgres"), None);
    assert_eq!(arithmetic_type("DATE", "*", "INT", "postgres"), None);
}

/// What `left UNION right` makes of two operands: a column of each SQL type, a literal, or
/// `NULL`.
#[test]
fn a_union_follows_postgres_union_rules() {
    let columns = [
        "INT",
        "BIGINT",
        "SMALLINT",
        "DECIMAL",
        "DOUBLE PRECISION",
        "TEXT",
        "VARCHAR",
        "DATE",
        "TIMESTAMP",
        "TIMESTAMPTZ",
        "BOOLEAN",
        "TIME",
        "INT[]",
        "BIGINT[]",
        "TEXT[]",
    ];
    let name = |ty: &str| {
        format!(
            "c_{}",
            ty.replace(' ', "_").replace("[]", "_arr").to_lowercase()
        )
    };
    let ddl = columns
        .iter()
        .map(|ty| format!("{} {ty}", name(ty)))
        .collect::<Vec<_>>()
        .join(", ");
    let schema = load_schema(&format!("CREATE TABLE t ({ddl})"), "postgres");
    let union = |left: &str, right: &str| {
        let sql = format!("SELECT {left} FROM t UNION SELECT {right} FROM t");
        match analyze_select(&sql, "postgres", Some(&schema)) {
            Some(found) if found[0].python_type.is_some() => format!(
                "{} | {}",
                found[0].python_type.as_deref().unwrap_or("?"),
                found[0].sql_type.as_deref().unwrap_or("?")
            ),
            _ => "conflict".to_string(),
        }
    };
    let mut lines: Vec<String> = [
        ("INT", "INT"),
        ("INT", "BIGINT"),
        ("SMALLINT", "DECIMAL"),
        ("BIGINT", "DOUBLE PRECISION"),
        ("TEXT", "VARCHAR"),
        ("VARCHAR", "TEXT"),
        ("DATE", "TIMESTAMP"),
        ("TIMESTAMP", "TIMESTAMPTZ"),
        ("INT", "TEXT"),
        ("BOOLEAN", "INT"),
        ("DATE", "TIME"),
        ("INT[]", "BIGINT[]"),
        ("INT[]", "TEXT[]"),
        ("INT[]", "INT"),
    ]
    .iter()
    .map(|(left, right)| format!("{left} + {right} -> {}", union(&name(left), &name(right))))
    .collect();
    for (label, left, right) in [
        ("unknown + INT", "NULL", "c_int"),
        ("INT + unknown", "c_int", "NULL"),
        ("'str' + TEXT", "'str'", "c_text"),
        ("'str' + INT", "'str'", "c_int"),
        ("1 + SMALLINT", "1", "c_smallint"),
        ("1 + BIGINT", "1", "c_bigint"),
    ] {
        lines.push(format!("{label} -> {}", union(left, right)));
    }
    insta::assert_snapshot!(lines.join("\n"));
}
