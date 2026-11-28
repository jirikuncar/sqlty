//! The function catalog through `sqlty_sql::signatures`.

use sqlty_sql::signatures::{signatures, with_prefix};
use sqlty_sql::Dialect;

#[test]
fn arity() {
    let now = &signatures(Dialect::Postgres, "now")[0];
    assert!(now.accepts(0) && !now.accepts(1));
    assert_eq!(now.parameter(0), None);
    assert_eq!(now.parameter_label(0), None);

    let left = &signatures(Dialect::Postgres, "left")[0];
    assert_eq!(left.parameter(1), Some(1));
    assert_eq!(left.parameter(2), None);

    let coalesce = &signatures(Dialect::Postgres, "Coalesce")[0];
    assert!(coalesce.accepts(1) && coalesce.accepts(9) && !coalesce.accepts(0));
    assert_eq!(coalesce.parameter(5), Some(0));
    assert_eq!(coalesce.returns, "anyelement");
}

#[test]
fn dialects() {
    let generic = signatures(Dialect::Other("generic"), "left");
    assert_eq!(generic, signatures(Dialect::Postgres, "left"));
    assert!(signatures(Dialect::ClickHouse, "left").is_empty());
    let duckdb = &signatures(Dialect::Other("duckdb"), "left")[0];
    assert_eq!(
        duckdb.label,
        "left(string: VARCHAR, count: BIGINT) -> VARCHAR"
    );
    assert_eq!(
        duckdb.url(),
        "https://duckdb.org/docs/2.0/sql/functions/text.html"
    );
    let mysql = &signatures(Dialect::MySql, "LEFT")[0];
    assert_eq!((mysql.label, mysql.returns), ("left(str, len)", ""));
    assert_eq!(
        mysql.url(),
        "https://dev.mysql.com/doc/refman/8.4/en/string-functions.html"
    );
    assert!(signatures(Dialect::Postgres, "no_such_function").is_empty());
}

#[test]
fn functions_by_prefix() {
    let names = |prefix| -> Vec<_> {
        with_prefix(Dialect::Postgres, prefix)
            .map(|overloads| (overloads[0].name, overloads.len()))
            .collect()
    };
    assert_eq!(names("COAL"), [("coalesce", 1)]);
    assert_eq!(names("ltri"), [("ltrim", 2)]);
    assert!(names("zzz").is_empty());
    assert!(names("é").is_empty());
    let all = names("");
    assert!(all.windows(2).all(|w| w[0].0 < w[1].0), "one entry per name, in order");
    assert_eq!(with_prefix(Dialect::ClickHouse, "").count(), 0);
}

#[test]
fn every_overload_documents_its_page() {
    let url = signatures(Dialect::Postgres, "date_trunc")
        .first()
        .expect("date_trunc")
        .url();
    assert_eq!(
        url,
        "https://www.postgresql.org/docs/17/functions-datetime.html#FUNCTIONS-DATETIME-TABLE"
    );
}

#[test]
fn signature_help_at_the_edges() {
    use sqlty_sql::analyzer::signature_help;

    let help = signature_help("(SELECT upper(", 14, "postgres", None).expect("upper");
    assert_eq!(help.name, 8..13);
    assert_eq!(
        signature_help("SELECT upper('é", 15, "postgres", None),
        None
    );
    let help = signature_help("upper(lower(", 6, "postgres", None).expect("upper");
    assert_eq!(help.name, 0..5);
}

/// The label of the active overload at the `|` of `sql`, its active parameter in `**`.
#[expect(clippy::expect_used, reason = "a case without a cursor fails the test")]
fn active(sql: &str) -> Option<String> {
    use sqlty_sql::analyzer::signature_help;

    let cursor = sql.find('|').expect("a cursor");
    let help = signature_help(&sql.replacen('|', "", 1), cursor, "postgres", None)?;
    let signature = &help.signatures[help.active_signature];
    Some(match help.active_parameter {
        Some(p) => {
            let (start, end) = signature.parameters[p].label;
            let label = signature.label;
            format!(
                "{}**{}**{}",
                &label[..start],
                &label[start..end],
                &label[end..]
            )
        }
        None => signature.label.to_string(),
    })
}

#[test]
#[rustfmt::skip]
fn signature_help_cases() {
    for (sql, expected) in [
        ("SELECT left(name, |", Some("left(string: text, **n: integer**) -> text")),
        ("SELECT left(name, 3, |", Some("left(string: text, n: integer) -> text")),
        ("SELECT date_trunc(|", Some("date_trunc(**text**, timestamp) -> timestamp")),
        ("SELECT coalesce(a, b, |", Some("coalesce(**value: anyelement**, ...) -> anyelement")),
        ("SELECT upper(left(name, |", Some("left(string: text, **n: integer**) -> text")),
        ("SELECT \"upper\"(|", Some("upper(**text**) -> text")),
        ("SELECT make_interval(days => |", Some("make_interval([years: int[, months: int[, weeks: int[, **days: int**[, hours: int[, mins: int[, secs: double precision]]]]]]]) -> interval")),
        ("SELECT make_interval(days => 1, |", Some("make_interval([years: int[, months: int[, weeks: int[, days: int[, hours: int[, mins: int[, secs: double precision]]]]]]]) -> interval")),
        ("SELECT make_interval(days := |", Some("make_interval([years: int[, months: int[, weeks: int[, **days: int**[, hours: int[, mins: int[, secs: double precision]]]]]]]) -> interval")),
        ("SELECT left(ARRAY['a', 'b'], |", Some("left(string: text, **n: integer**) -> text")),
        ("SELECT upper(ARRAY[|", None),
        ("SELECT make_interval(nope => |", Some("make_interval([years: int[, months: int[, weeks: int[, days: int[, hours: int[, mins: int[, secs: double precision]]]]]]]) -> interval")),
        ("SELECT \"UPPER\"(|", None),
        ("SELECT (a + |", None),
        ("SELECT upper((|", None),
        ("SELECT a FROM t WHERE a IN (|", None),
        ("SELECT upper('a, |", None),
        ("SELECT name|", None),
        ("SELECT no_such_function(|", None),
        ("INSERT INTO point (|", None),
        ("INSERT INTO public.point (|", None),
        ("INSERT INTO point VALUES (1, |", None),
        ("CREATE TABLE IF NOT EXISTS point (|", None),
        ("SELECT 1 + upper(|", Some("upper(**text**) -> text")),
    ] {
        assert_eq!(active(sql).as_deref(), expected, "{sql}");
    }
}
