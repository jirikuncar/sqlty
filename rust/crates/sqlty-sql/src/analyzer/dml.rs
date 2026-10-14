//! DML statements: whether they return rows and the scope of their target tables.

use sqlparser::ast::{FromTable, OutputClause, SelectItem, SetExpr, Statement, TableObject};

use super::dialect::SqlDialect;
use super::names::{object_name, table_key};
use super::scope::{table_scope, Scope, Source, Table};

pub(super) fn statement_returns_rows(stmt: &Statement) -> bool {
    match stmt {
        Statement::Query(query) => match query.body.as_ref() {
            SetExpr::Insert(inner)
            | SetExpr::Update(inner)
            | SetExpr::Delete(inner)
            | SetExpr::Merge(inner) => statement_returns_rows(inner),
            _ => true,
        },
        _ => returning(stmt).is_some(),
    }
}

/// The non-empty top-level RETURNING list of INSERT/UPDATE/DELETE/MERGE.
pub(super) fn returning(stmt: &Statement) -> Option<&[SelectItem]> {
    let items = match stmt {
        Statement::Insert(insert) => &insert.returning,
        Statement::Update(update) => &update.returning,
        Statement::Delete(delete) => &delete.returning,
        Statement::Merge(merge) => {
            return match &merge.output {
                Some(OutputClause::Returning { select_items, .. }) if !select_items.is_empty() => {
                    Some(select_items)
                }
                _ => None,
            };
        }
        _ => return None,
    };
    items.as_deref().filter(|items| !items.is_empty())
}

/// Tables a DML statement writes to, for resolving RETURNING columns.
pub(super) fn dml_scope(stmt: &Statement, dialect: SqlDialect) -> Scope {
    match stmt {
        Statement::Insert(insert) => match &insert.table {
            TableObject::TableName(name) => {
                vec![Source::named(object_name(name), table_key(name, dialect))]
            }
            _ => Vec::new(),
        },
        Statement::Update(update) => {
            let mut scope = table_scope(&update.table.relation, dialect);
            if update.from.is_some() || !update.table.joins.is_empty() {
                scope.push(other_sources());
            }
            scope
        }
        Statement::Merge(merge) => {
            let mut scope = table_scope(&merge.table, dialect);
            scope.push(other_sources());
            scope
        }
        Statement::Delete(delete) => {
            let mut scope = match &delete.from {
                FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables
                    .first()
                    .map(|table| table_scope(&table.relation, dialect))
                    .unwrap_or_default(),
            };
            if delete.using.is_some() {
                scope.push(other_sources());
            }
            scope
        }
        _ => Vec::new(),
    }
}

/// RETURNING may read the tables of `FROM`, `USING` and `MERGE ... USING`, which are not
/// resolved here.
fn other_sources() -> Source {
    Source {
        reference: String::new(),
        table: Table::ExtraSources,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{reports, statement};
    use super::*;

    fn returns_rows(sql: &str) -> bool {
        statement_returns_rows(&statement(sql, SqlDialect::Postgres))
    }

    fn scope_of(sql: &str, dialect: SqlDialect) -> Vec<String> {
        dml_scope(&statement(sql, dialect), dialect)
            .iter()
            .map(|source| format!("{}={:?}", source.reference, source.table))
            .collect()
    }

    #[test]
    fn queries_return_rows() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("VALUES (1)"));
        assert!(returns_rows("WITH c AS (SELECT 1) SELECT * FROM c"));
    }

    #[test]
    fn dml_returns_rows_only_with_returning() {
        assert!(!returns_rows("INSERT INTO t (a) VALUES (1)"));
        assert!(returns_rows("INSERT INTO t (a) VALUES (1) RETURNING a"));
        assert!(!returns_rows("UPDATE t SET a = 1"));
        assert!(returns_rows("UPDATE t SET a = 1 RETURNING *"));
        assert!(!returns_rows("DELETE FROM t"));
        assert!(returns_rows("DELETE FROM t RETURNING a"));
    }

    #[test]
    fn a_cte_around_dml_follows_the_dml() {
        assert!(!returns_rows(
            "WITH d AS (SELECT 1) DELETE FROM t WHERE a IN (SELECT * FROM d)"
        ));
        assert!(returns_rows(
            "WITH d AS (SELECT 1) DELETE FROM t WHERE a IN (SELECT * FROM d) RETURNING a"
        ));
    }

    #[test]
    fn ddl_and_transaction_control_return_no_rows() {
        assert!(!returns_rows("CREATE TABLE t (a INT)"));
        assert!(!returns_rows("BEGIN"));
        assert!(!returns_rows("COMMIT"));
    }

    #[test]
    fn merge_returns_rows_only_with_returning() {
        let merge = "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE";
        assert!(!returns_rows(merge));
        assert!(returns_rows(&format!("{merge} RETURNING t.id")));
    }

    #[test]
    fn returning_lists_the_items_of_the_statement() {
        let items =
            |sql: &str| returning(&statement(sql, SqlDialect::Postgres)).map(<[SelectItem]>::len);
        assert_eq!(
            items("INSERT INTO t (a) VALUES (1) RETURNING a, b"),
            Some(2)
        );
        assert_eq!(items("UPDATE t SET a = 1 RETURNING *"), Some(1));
        assert_eq!(items("DELETE FROM t RETURNING a"), Some(1));
        assert_eq!(items("DELETE FROM t"), None);
        assert_eq!(items("SELECT 1"), None);
    }

    #[test]
    fn scope_of_an_insert_is_its_target() {
        assert_eq!(
            scope_of(
                "INSERT INTO Public.Users (a) VALUES (1)",
                SqlDialect::Postgres
            ),
            ["users=Named(\"public.users\")"]
        );
    }

    #[test]
    fn scope_of_an_update_adds_extra_sources_for_from_and_joins() {
        assert_eq!(
            scope_of("UPDATE t SET a = 1", SqlDialect::Postgres),
            ["t=Named(\"t\")"]
        );
        assert_eq!(
            scope_of("UPDATE t SET a = u.a FROM u", SqlDialect::Postgres),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
        assert_eq!(
            scope_of(
                "UPDATE t JOIN u ON t.id = u.id SET t.a = 1",
                SqlDialect::MySql
            ),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
    }

    #[test]
    fn scope_of_a_delete_adds_extra_sources_for_using() {
        assert_eq!(
            scope_of("DELETE FROM t AS x", SqlDialect::Postgres),
            ["x=Named(\"t\")"]
        );
        assert_eq!(
            scope_of(
                "DELETE FROM t USING u WHERE t.id = u.id",
                SqlDialect::Postgres
            ),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
    }

    #[test]
    fn scope_of_a_merge_always_has_extra_sources() {
        assert_eq!(
            scope_of(
                "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE",
                SqlDialect::Postgres
            ),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
    }

    #[test]
    fn other_statements_have_no_scope() {
        assert!(scope_of("SELECT 1", SqlDialect::Postgres).is_empty());
    }

    #[test]
    fn returning_columns_in_queries() {
        let ddl = "CREATE TABLE t (id SERIAL PRIMARY KEY, name TEXT NOT NULL, note TEXT); \
                   CREATE TABLE u (id INT, extra INT)";
        let queries = [
            "INSERT INTO t (name) VALUES ('a') RETURNING id, name, note",
            "INSERT INTO t (name) VALUES ('a') RETURNING *",
            "INSERT INTO t (name) VALUES ('a') RETURNING id AS new_id, upper(name) AS shout",
            "UPDATE t SET note = 'x' RETURNING *",
            "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING *",
            "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING t.id, extra",
            "DELETE FROM t USING u WHERE t.id = u.id RETURNING t.*",
            "DELETE FROM t WHERE id = 1 RETURNING nope",
            "INSERT INTO t (name) VALUES ('a')",
            "INSERT INTO missing (a) VALUES (1) RETURNING a",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
    }
}
