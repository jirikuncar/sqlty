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
pub(super) fn dml_scope(stmt: &Statement) -> Scope {
    match stmt {
        Statement::Insert(insert) => match &insert.table {
            TableObject::TableName(name) => vec![(object_name(name), table_key(name))],
            _ => Vec::new(),
        },
        Statement::Update(update) => table_scope(&update.table.relation),
        Statement::Merge(merge) => table_scope(&merge.table),
        Statement::Delete(delete) => match &delete.from {
            FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables
                .first()
                .map(|table| table_scope(&table.relation))
                .unwrap_or_default(),
        },
        _ => Vec::new(),
    }
}
