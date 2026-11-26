//! `entity-mismatch`: columns holding keys of different entities compared to each other.
//!
//! A primary or unique key column is a key of its table, and a foreign key column of the
//! key it references, followed to the end of the chain: `orders.customer_id` holds a
//! `customers` key. Joining `products.id` to `orders.customer_id` type-checks, as both
//! are integers, and returns plausible rows.

use super::query::Analyzer;
use crate::diagnostics::Rule;

impl<'s> Analyzer<'s> {
    /// The key `column` of `table` holds, when it is a key or references one.
    pub(super) fn entity_side(&self, table: &'s str, column: &'s str) -> Option<Side<'s>> {
        let entity = self.schema.catalog().entity(table, column)?;
        Some(Side {
            table: table.rsplit('.').next().unwrap_or(table),
            column,
            entity,
        })
    }

    /// Records `entity-mismatch` when `a` and `b` hold keys of different entities, not
    /// shown where `foreign-key-mismatch` is on when it reports them (`keys`).
    pub(super) fn report_entities(&self, a: &Side, b: &Side, keys: bool) {
        if a.entity == b.entity {
            return;
        }
        // Two columns of one composite key are told apart by the column.
        let named = |side: &Side| {
            let table = side.entity.0.rsplit('.').next().unwrap_or(&side.entity.0);
            if a.entity.0 == b.entity.0 {
                format!("{table}.{}", side.entity.1)
            } else {
                table.to_string()
            }
        };
        let message = format!(
            "{}.{} holds {} keys, {}.{} holds {} keys",
            a.table,
            a.column,
            named(a),
            b.table,
            b.column,
            named(b)
        );
        if keys {
            self.diag
                .record_shadowed(Rule::EntityMismatch, message, Rule::ForeignKeyMismatch);
        } else {
            self.diag.record(Rule::EntityMismatch, message);
        }
    }
}

/// A column and the key it holds.
#[derive(Clone)]
pub(super) struct Side<'r> {
    /// The table without its schema, and the column, for messages.
    table: &'r str,
    column: &'r str,
    /// The `(table key, column)` its foreign keys lead to.
    entity: (&'r str, &'r str),
}
