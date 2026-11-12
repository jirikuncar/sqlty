//! `entity-mismatch`: columns holding keys of different entities compared to each other.
//!
//! A primary or unique key column is a key of its table, and a foreign key column of the
//! key it references, followed to the end of the chain: `orders.customer_id` holds a
//! `customers` key. Joining `products.id` to `orders.customer_id` type-checks, as both
//! are integers, and returns plausible rows.

use std::ops::ControlFlow;

use sqlparser::ast::{
    BinaryOperator, Expr, Ident, Query, Select, SelectItem, SetExpr, Spanned, TableFactor, Visit,
    Visitor,
};

use super::fanout::{join_conditions, strip};
use super::query::Analyzer;
use super::scope::{relation_name, Scope};
use crate::diagnostics::{Diagnostics, NameRule};

impl Analyzer<'_> {
    /// Records `entity-mismatch` for each comparison of `select` between columns of
    /// different entities: in `ON`, `USING`, `WHERE`, `HAVING`, `IN` lists and `IN`
    /// subqueries that select one column.
    pub(super) fn check_entities(&self, select: &Select, scope: &Scope) {
        if !self.diag.enabled() || !self.judges_names() {
            return;
        }
        let mut conditions = Vec::new();
        join_conditions(&select.from, &mut conditions, &mut |using, right| {
            self.check_using(using, right, scope);
        });
        conditions.extend(&select.selection);
        conditions.extend(&select.having);
        for condition in conditions {
            let _ = condition.visit(&mut Comparisons {
                analyzer: self,
                scope,
                queries: 0,
            });
        }
    }

    fn check_using(&self, using: &[Ident], right: &TableFactor, scope: &Scope) {
        let Some(at) = relation_name(right)
            .and_then(|name| scope.iter().position(|source| source.reference == name))
        else {
            return;
        };
        let qualified = |reference: &str, ident: &Ident| {
            Expr::CompoundIdentifier(vec![Ident::new(reference), ident.clone()])
        };
        for ident in using {
            let right = qualified(&scope[at].reference, ident);
            let left = scope[..at]
                .iter()
                .map(|source| qualified(&source.reference, ident))
                .find(|left| self.locate_column(left, scope).is_some());
            if let Some(left) = left {
                let _at = self.diag.at(ident.span);
                self.compare(&left, &right, scope);
            }
        }
    }

    /// Records `entity-mismatch` when `left` and `right`, read in `scope`, hold keys of
    /// different entities.
    fn compare(&self, left: &Expr, right: &Expr, scope: &Scope) {
        if let (Some(a), Some(b)) = (self.entity_of(left, scope), self.entity_of(right, scope)) {
            self.report(&a, &b);
        }
    }

    fn report(&self, a: &Side, b: &Side) {
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
        self.diag.record(
            NameRule::EntityMismatch,
            format!(
                "{} holds {} keys, {} holds {} keys",
                a.column,
                named(a),
                b.column,
                named(b)
            ),
        );
    }

    fn entity_of(&self, expr: &Expr, scope: &Scope) -> Option<Side> {
        let expr = strip(expr);
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return None;
        }
        let (_, table, column) = self.locate_column(expr, scope)?;
        if self.virtual_columns(&table).is_some() {
            return None;
        }
        let (root, key) = self.schema.entity(&table, &column)?;
        let name = table.rsplit('.').next().unwrap_or(&table);
        Some(Side {
            column: format!("{name}.{column}"),
            entity: (root.to_string(), key),
        })
    }

    /// `expr IN (SELECT column FROM ...)`: the subquery's column, read in its own scope.
    fn check_in_subquery(&self, expr: &Expr, subquery: &Query, scope: &Scope) {
        let SetExpr::Select(inner) = subquery.body.as_ref() else {
            return;
        };
        let [SelectItem::UnnamedExpr(column) | SelectItem::ExprWithAlias { expr: column, .. }] =
            inner.projection.as_slice()
        else {
            return;
        };
        let mut quiet = self.correlated(scope);
        quiet.diag = Diagnostics::default();
        let Ok((analyzer, inner_scope)) = quiet.scope_of(&inner.from);
        let (Some(a), Some(b)) = (
            self.entity_of(expr, scope),
            analyzer.entity_of(column, &inner_scope),
        ) else {
            return;
        };
        let _at = self.diag.at(column.span());
        self.report(&a, &b);
    }
}

/// A column and the key it holds.
struct Side {
    /// `table.column`, for messages.
    column: String,
    /// The `(table key, column)` its foreign keys lead to.
    entity: (String, String),
}

/// Visits a condition, comparing the columns of each `=`, `<>`, `IN` list and `IN`
/// subquery outside subqueries.
struct Comparisons<'a, 's> {
    analyzer: &'a Analyzer<'s>,
    scope: &'a Scope,
    queries: usize,
}

impl Visitor for Comparisons<'_, '_> {
    type Break = ();

    fn pre_visit_query(&mut self, _query: &Query) -> ControlFlow<()> {
        self.queries += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<()> {
        self.queries -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if self.queries > 0 {
            return ControlFlow::Continue(());
        }
        let (analyzer, scope) = (self.analyzer, self.scope);
        let _at = analyzer.diag.at(expr.span());
        match expr {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq | BinaryOperator::NotEq,
                right,
            } => analyzer.compare(left, right, scope),
            Expr::InList { expr, list, .. } => {
                for item in list {
                    analyzer.compare(expr, item, scope);
                }
            }
            Expr::InSubquery { expr, subquery, .. } => {
                analyzer.check_in_subquery(expr, subquery, scope);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}
