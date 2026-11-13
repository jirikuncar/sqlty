//! The tables a query names, and whether it reads or writes each: CTEs, derived tables
//! and table functions are not tables.

use std::ops::ControlFlow;

use sqlparser::ast::{
    FromTable, ObjectName, Query, Statement, TableFactor, TableObject, TableWithJoins, Visit,
    Visitor,
};

use super::preprocess::parse_query;
use super::SqlDialect;

/// A table a query names, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    /// The table name, without its qualifier.
    pub name: String,
    /// The schema qualifier, when the query writes one (`analytics` of `analytics.events`).
    pub schema: Option<String>,
    /// Whether the statement writes the table: the target of an INSERT, UPDATE, DELETE or
    /// MERGE.
    pub write: bool,
}

/// The tables `sql` names, in source order, each once per access; empty when it does not
/// parse.
///
/// ```
/// let tables = sqlty_sql::analyzer::query_tables(
///     "WITH r AS (SELECT * FROM s.users) INSERT INTO log SELECT id FROM r",
///     "postgres",
/// );
/// let names: Vec<_> = tables.iter().map(|t| (t.name.as_str(), t.write)).collect();
/// assert_eq!(names, [("users", false), ("log", true)]);
/// ```
pub fn query_tables(sql: &str, dialect: &str) -> Vec<TableRef> {
    let Some(stmts) = parse_query(sql, SqlDialect::from_name(dialect)) else {
        return Vec::new();
    };
    let mut tables = Tables::default();
    let _ = stmts.visit(&mut tables);
    tables.found
}

#[derive(Default)]
struct Tables {
    /// The CTE names of each query being visited, outermost first.
    ctes: Vec<Vec<String>>,
    /// The `FROM` items an UPDATE, DELETE or MERGE writes.
    targets: Vec<*const TableFactor>,
    found: Vec<TableRef>,
}

impl Tables {
    fn add(&mut self, name: &ObjectName, write: bool) {
        let parts: Vec<&str> = name
            .0
            .iter()
            .filter_map(|part| part.as_ident())
            .map(|ident| ident.value.as_str())
            .collect();
        let Some((table, qualifiers)) = parts.split_last() else {
            return;
        };
        let cte = qualifiers.is_empty()
            && self
                .ctes
                .iter()
                .flatten()
                .any(|cte| cte.eq_ignore_ascii_case(table));
        let found = TableRef {
            name: table.to_string(),
            schema: qualifiers.last().map(ToString::to_string),
            write,
        };
        if !cte && !self.found.contains(&found) {
            self.found.push(found);
        }
    }
}

/// The items of `tables` that MySQL's `DELETE t1, t2 FROM ...` names, by alias or name.
fn deleted<'a>(names: &[ObjectName], tables: &'a [TableWithJoins]) -> Vec<&'a TableFactor> {
    let last = |name: &ObjectName| {
        name.0
            .last()
            .and_then(|part| part.as_ident())
            .map(|ident| ident.value.to_lowercase())
    };
    let names: Vec<String> = names.iter().filter_map(last).collect();
    let factors = tables.iter().flat_map(|table| {
        std::iter::once(&table.relation).chain(table.joins.iter().map(|join| &join.relation))
    });
    factors
        .filter(|factor| match factor {
            TableFactor::Table { name, alias, .. } => {
                let alias = alias.as_ref().map(|alias| alias.name.value.to_lowercase());
                names
                    .iter()
                    .any(|n| Some(n) == alias.as_ref() || Some(n) == last(name).as_ref())
            }
            _ => false,
        })
        .collect()
}

impl Visitor for Tables {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        let names = query.with.iter().flat_map(|with| &with.cte_tables);
        self.ctes
            .push(names.map(|cte| cte.alias.name.value.clone()).collect());
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<()> {
        self.ctes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, stmt: &Statement) -> ControlFlow<()> {
        let targets: Vec<&TableFactor> = match stmt {
            Statement::Insert(insert) => {
                if let TableObject::TableName(name) = &insert.table {
                    self.add(name, true);
                }
                Vec::new()
            }
            Statement::Update(update) => vec![&update.table.relation],
            Statement::Merge(merge) => vec![&merge.table],
            Statement::Delete(delete) => match &delete.from {
                FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => {
                    if delete.tables.is_empty() {
                        tables
                            .first()
                            .map(|table| &table.relation)
                            .into_iter()
                            .collect()
                    } else {
                        deleted(&delete.tables, tables)
                    }
                }
            },
            _ => Vec::new(),
        };
        self.targets.extend(
            targets
                .into_iter()
                .map(|factor| factor as *const TableFactor),
        );
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table {
            name, args: None, ..
        } = factor
        {
            let write = self.targets.contains(&(factor as *const TableFactor));
            self.add(name, write);
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::query_tables;

    fn tables(sql: &str, dialect: &str) -> Vec<String> {
        query_tables(sql, dialect)
            .into_iter()
            .map(|t| {
                let name = match t.schema {
                    Some(schema) => format!("{schema}.{}", t.name),
                    None => t.name,
                };
                format!("{name} {}", if t.write { "write" } else { "read" })
            })
            .collect()
    }

    #[test]
    fn reads_and_writes() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "SELECT * FROM a JOIN s.b ON true WHERE x IN (SELECT y FROM c)",
                "postgres",
                &["a read", "s.b read", "c read"],
            ),
            (
                "WITH r AS (SELECT * FROM a) SELECT * FROM r, (SELECT 1 FROM b) d, generate_series(1, 2)",
                "postgres",
                &["a read", "b read"],
            ),
            (
                "WITH RECURSIVE t AS (SELECT 1 UNION ALL SELECT n FROM t) SELECT * FROM t",
                "postgres",
                &[],
            ),
            (
                "SELECT * FROM r WHERE EXISTS (WITH r AS (SELECT 1) SELECT * FROM r)",
                "postgres",
                &["r read"],
            ),
            (
                "INSERT INTO log (id) SELECT id FROM users",
                "postgres",
                &["log write", "users read"],
            ),
            (
                "UPDATE users u SET n = o.n FROM orders o WHERE o.id = u.id",
                "postgres",
                &["users write", "orders read"],
            ),
            (
                "DELETE FROM users USING orders WHERE orders.user_id = users.id",
                "postgres",
                &["users write", "orders read"],
            ),
            (
                "DELETE u FROM users u JOIN orders o ON o.user_id = u.id",
                "mysql",
                &["users write", "orders read"],
            ),
            (
                "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE",
                "postgres",
                &["t write", "s read"],
            ),
            (
                "UPDATE users SET n = (SELECT count(*) FROM users)",
                "postgres",
                &["users write", "users read"],
            ),
            ("SELECT * FROM", "postgres", &[]),
        ];
        for (sql, dialect, expected) in cases {
            assert_eq!(tables(sql, dialect), *expected, "{sql}");
        }
    }
}
