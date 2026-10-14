//! DML statements: whether they return rows and the scope of their target tables.

use super::*;

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
