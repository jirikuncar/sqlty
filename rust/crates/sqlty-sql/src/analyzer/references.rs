//! Column references outside the result columns: those of `WHERE`, `JOIN ... ON` and
//! the values and conditions of INSERT, UPDATE and DELETE. Typing a result column
//! resolves only the names it needs, so these are resolved on their own.

use std::ops::ControlFlow;

use sqlparser::ast::{Expr, Query, Visit, Visitor};

use super::memo::Reporting;
use super::query::Analyzer;
use super::scope::Scope;

impl Analyzer<'_> {
    /// Resolves every column `expr` reads in `scope`, recording `unknown-column`,
    /// `unknown-table` and `ambiguous-column` where the rules apply. A subquery is
    /// analyzed in its own scope, correlated with `scope`.
    pub(super) fn check_references(&self, expr: &Expr, scope: &Scope) {
        if !self.diag.enabled() {
            return;
        }
        let mut visitor = References {
            analyzer: self,
            scope,
            depth: 0,
        };
        let _ = expr.visit(&mut visitor);
    }
}

struct References<'a, 's> {
    analyzer: &'a Analyzer<'s>,
    scope: &'a Scope,
    /// How many subqueries deep the visit is; only the outermost names are resolved here.
    depth: usize,
}

impl Visitor for References<'_, '_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if self.depth == 0 {
            let _ = self
                .analyzer
                .subquery_columns(query, self.scope, Reporting::Record);
        }
        self.depth += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<()> {
        self.depth -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        let column = match expr {
            Expr::Identifier(ident) => {
                // `DEFAULT` in `VALUES` and `SET` is a keyword, not a column.
                ident.quote_style.is_some() || !ident.value.eq_ignore_ascii_case("default")
            }
            Expr::CompoundIdentifier(_) => true,
            _ => false,
        };
        if self.depth == 0 && column {
            let _ = self.analyzer.expr(expr, self.scope);
        }
        ControlFlow::Continue(())
    }
}
