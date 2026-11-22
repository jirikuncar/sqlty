//! The SQL lint rules the analyzer reports.

#![expect(
    clippy::expect_used,
    reason = "a test fails on the query it cannot read"
)]

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

mod inference {
    use sqlty_sql::analyzer::{explain_select, load_schema_with_skipped};

    /// `(type rule id, reference)` of the one column `sql` returns.
    fn type_rule(sql: &str, dialect: &str, ddl: &str) -> (String, String) {
        let (schema, _) = load_schema_with_skipped(ddl, dialect);
        let rules = explain_select(sql, dialect, Some(&schema)).expect("rows");
        let rule = rules[0].type_rule.expect("a rule types the column");
        (rule.id.to_string(), rule.reference.to_string())
    }

    #[test]
    fn a_reference_keeps_the_part_that_cites_the_dialect() {
        let reference = |dialect| type_rule("SELECT 1 = 1", dialect, "").1;
        assert_eq!(
            reference("postgres"),
            "PostgreSQL 17 documentation, 9.2, Table 9.1 Comparison Operators"
        );
        assert_eq!(
            reference("mysql"),
            "MySQL 8.4 Reference Manual, 14.4.2 Comparison Functions and Operators"
        );
        assert_eq!(
            reference("sqlite"),
            "SQLite documentation, SQL Language Expressions, 2 Operators"
        );
        // A dialect the rule does not cite gets every citation.
        for dialect in ["duckdb", "generic"] {
            assert_eq!(
                reference(dialect),
                "PostgreSQL 17 documentation, 9.2, Table 9.1 Comparison Operators; \
                 MySQL 8.4 Reference Manual, 14.4.2 Comparison Functions and Operators; \
                 SQLite documentation, SQL Language Expressions, 2 Operators",
                "{dialect}"
            );
        }
    }

    #[test]
    fn dialect_rules_come_first() {
        let id = |dialect| type_rule("SELECT x ->> 'a' FROM (SELECT '{}' AS x) q", dialect, "").0;
        assert_eq!(id("sqlite"), "sqlite.json.long-arrow");
        assert_eq!(id("postgres"), "common.json.long-arrow");
    }

    #[test]
    fn an_operand_predicate_admits_the_types_it_names_and_any_known_type_for_known() {
        let ddl = "CREATE TYPE mood AS ENUM ('a'); CREATE TABLE t (j jsonb, m mood, u nope);";
        let id = |sql: &str| type_rule(sql, "postgres", ddl).0;
        // A predicate on JSONB admits JSONB: neither an enum, nor a column of no known type.
        assert_eq!(id("SELECT j -> 'a' FROM t"), "postgres.json.arrow-jsonb");
        assert_eq!(id("SELECT m -> 'a' FROM t"), "postgres.json.arrow");
        assert_eq!(id("SELECT u -> 'a' FROM t"), "postgres.json.arrow");
        // `@known` admits any typed operand, an enum included, but not an untyped one.
        assert_eq!(id("SELECT AVG(m) FROM t"), "common.aggregate.avg");
        assert_eq!(
            id("SELECT AVG(unknown_fn(1)) FROM t"),
            "common.aggregate.avg-untyped"
        );
    }

    /// The dialects of `rules/dialects.toml` read the rules of others and are dialects of sqlparser.
    #[test]
    fn dialects_that_extend_others_are_dialects_sqlty_reads() {
        let toml = include_str!("../rules/dialects.toml");
        let extending: Vec<&str> = toml
            .lines()
            .filter_map(|line| line.strip_prefix('[')?.strip_suffix(']'))
            .collect();
        assert!(!extending.is_empty());
        for name in extending {
            assert_eq!(
                name.parse::<sqlty_sql::Dialect>(),
                Ok(sqlty_sql::Dialect::Other(name)),
                "{name}"
            );
            assert_eq!(
                type_rule("SELECT 1 + 1", name, "").0,
                "common.arithmetic.numbers",
                "{name}"
            );
        }
    }
}
