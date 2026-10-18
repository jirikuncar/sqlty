//! Column references outside the result columns: those of `WHERE`, `JOIN ... ON` and
//! the values and conditions of INSERT, UPDATE and DELETE. Typing a result column
//! resolves only the names it needs, so these are resolved on their own.

use std::ops::ControlFlow;

use sqlparser::ast::{Expr, FunctionArg, FunctionArguments, Query, Visit, Visitor};

use super::memo::Reporting;
use super::query::Analyzer;
use super::scope::Scope;

impl Analyzer<'_> {
    /// Resolves every column `expr` reads in `scope`, recording `unknown-column`,
    /// `unknown-table` and `ambiguous-column` where the rules apply. A subquery is
    /// analyzed in its own scope, correlated with `scope`.
    pub(super) fn check_references(&self, expr: &Expr, scope: &Scope) {
        self.check_references_besides(expr, scope, &[]);
    }

    /// [`Analyzer::check_references`], reading a bare name among `aliases` as the result
    /// column it names, as SQLite does in `WHERE` and MySQL in `GROUP BY` and `ORDER BY`.
    pub(super) fn check_references_besides(&self, expr: &Expr, scope: &Scope, aliases: &[&str]) {
        if !self.diag.enabled() {
            return;
        }
        let mut visitor = References {
            analyzer: self,
            scope,
            aliases,
            depth: 0,
            parameter_names: Vec::new(),
        };
        let _ = expr.visit(&mut visitor);
    }
}

struct References<'a, 's> {
    analyzer: &'a Analyzer<'s>,
    scope: &'a Scope,
    /// Result column aliases a bare name may refer to.
    aliases: &'a [&'a str],
    /// How many subqueries deep the visit is; only the outermost names are resolved here.
    depth: usize,
    /// The names of named arguments seen so far, as in `make_interval(days => 3)`: parameters,
    /// not columns.
    parameter_names: Vec<*const Expr>,
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
        if let Expr::Function(function) = expr {
            if let FunctionArguments::List(list) = &function.args {
                self.parameter_names
                    .extend(list.args.iter().filter_map(|arg| match arg {
                        FunctionArg::ExprNamed { name, .. } => Some(std::ptr::from_ref(name)),
                        _ => None,
                    }));
            }
        }
        if self
            .parameter_names
            .iter()
            .any(|&name| std::ptr::eq(name, expr))
        {
            return ControlFlow::Continue(());
        }
        let column = match expr {
            Expr::Identifier(ident) => {
                // `DEFAULT` in `VALUES` and `SET` is a keyword, not a column.
                (ident.quote_style.is_some() || !ident.value.eq_ignore_ascii_case("default"))
                    && !self
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(&ident.value))
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
