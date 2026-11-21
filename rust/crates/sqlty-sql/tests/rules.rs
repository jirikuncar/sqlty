//! The SQL lint rules the analyzer reports.

use sqlty_sql::diagnostics::Rule;

#[test]
fn sql_rules_list_every_name() {
    let names: Vec<_> = Rule::SQL.iter().map(|rule| rule.name()).collect();
    assert_eq!(
        names,
        [
            "unknown-table",
            "unknown-column",
            "ambiguous-column",
            "duplicate-alias",
            "unknown-function",
            "unknown-type",
            "unsupported-syntax",
            "insert-missing-column",
            "insert-value-count",
            "ungrouped-column",
            "fanout-aggregate",
            "chasm-join",
            "unproven-join",
            "entity-mismatch",
            "foreign-key-mismatch",
            "rejected-query",
        ]
    );
}
