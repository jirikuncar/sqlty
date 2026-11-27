//! The function catalog through `sqlty_sql::signatures`.

use sqlty_sql::signatures::signatures;
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
    assert!(signatures(Dialect::Postgres, "no_such_function").is_empty());
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
