//! `ungrouped-column`: a column a grouped query selects without grouping by it or
//! aggregating it, which PostgreSQL rejects.
//!
//! A column is grouped when a grouping key names it (`GROUP BY 1` and a result alias
//! name the select item), when a key is the expression it sits in, or when the keys
//! include its table's whole primary key, PostgreSQL's functional dependency. Only
//! columns that resolve to a schema table are judged.

use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, GroupByExpr, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, Value, Visit,
    Visitor,
};

use super::dialect::SqlDialect;
use super::functions::{function_name, positional_args};
use super::names::object_name;
use super::query::Analyzer;
use super::registry::lookup;
use super::scope::{any_merging_join, Scope, Source};
use crate::catalog::fold;
use crate::diagnostics::NameRule;

impl Analyzer<'_> {
    /// Records `ungrouped-column` for each select item column of a grouped `select`
    /// that is neither grouped nor aggregated.
    pub(super) fn check_grouping(&self, select: &Select, scope: &Scope) {
        if !self.diag.enabled() || self.dialect != SqlDialect::Postgres || !self.judges_names() {
            return;
        }
        let GroupByExpr::Expressions(keys, _) = &select.group_by else {
            return;
        };
        let items: Vec<&Expr> = select
            .projection
            .iter()
            .filter_map(|item| match item {
                SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                    Some(expr)
                }
                _ => None,
            })
            .collect();
        if keys.is_empty() && !items.iter().any(|expr| has_aggregate(expr)) {
            return;
        }
        let keys: Vec<&Expr> = keys
            .iter()
            .flat_map(|key| match key {
                Expr::Rollup(sets) | Expr::Cube(sets) | Expr::GroupingSets(sets) => {
                    sets.iter().flatten().collect()
                }
                Expr::Tuple(exprs) => exprs.iter().collect(),
                key => vec![key],
            })
            .collect();
        let mut grouped = Grouped::default();
        for key in &keys {
            match key {
                Expr::Value(value) if matches!(value.value, Value::Number(..)) => {
                    let position = value.value.to_string().parse::<usize>().ok();
                    if let Some(item) =
                        position.and_then(|n| select.projection.get(n.wrapping_sub(1)))
                    {
                        grouped.items.push(item as *const SelectItem);
                    }
                }
                key => match self.locate_column(key, scope) {
                    Some((reference, _, column)) => grouped.columns.push((reference, column)),
                    None => {
                        if let Expr::Identifier(alias) = key {
                            grouped.items.extend(select.projection.iter().filter_map(|item| {
                                matches!(item, SelectItem::ExprWithAlias { alias: a, .. } if a.value.eq_ignore_ascii_case(&alias.value))
                                    .then_some(item as *const SelectItem)
                            }));
                        }
                        grouped.expressions.push(key.to_string());
                    }
                },
            }
        }
        for item in &select.projection {
            let star = match item {
                // A merged column (`JOIN ... USING`) is either table's; not judged.
                SelectItem::Wildcard(_) if any_merging_join(&select.from) => None,
                SelectItem::Wildcard(_) => Some(None),
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    _,
                ) => Some(Some(object_name(name))),
                _ => None,
            };
            if let Some(reference) = star {
                self.check_star(&grouped, reference.as_deref(), scope);
                continue;
            }
            let (SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. }) = item
            else {
                continue;
            };
            if grouped.items.contains(&(item as *const SelectItem)) {
                continue;
            }
            let mut visitor = Ungrouped {
                analyzer: self,
                scope,
                grouped: &grouped,
                skipping: Vec::new(),
                queries: 0,
            };
            let _ = expr.visit(&mut visitor);
        }
    }

    /// Records `ungrouped-column` for each column `*` or `reference.*` selects that the
    /// keys don't cover.
    fn check_star(&self, grouped: &Grouped, reference: Option<&str>, scope: &Scope) {
        for Source { reference, table } in Self::star_sources(reference, scope) {
            let Some(table) = table.named() else {
                continue;
            };
            let Some(columns) = self.schema.table(table) else {
                continue;
            };
            if self.virtual_columns(table).is_some()
                || self.groups_primary_key(grouped, &reference, table)
            {
                continue;
            }
            for column in columns.iter().filter(|column| !column.invisible) {
                if !grouped.covers(&reference, &column.name) {
                    self.record_ungrouped(&reference, &column.name);
                }
            }
        }
    }

    fn record_ungrouped(&self, reference: &str, column: &str) {
        self.diag.record(
            NameRule::UngroupedColumn,
            format!(
                "column `{reference}.{column}` must appear in GROUP BY or be used in an aggregate function"
            ),
        );
    }

    /// Whether the keys include every primary key column of the schema table `table`,
    /// read by `reference`.
    fn groups_primary_key(&self, grouped: &Grouped, reference: &str, table: &str) -> bool {
        let Some(columns) = self.schema.table(table) else {
            return false;
        };
        let mut key = columns
            .iter()
            .filter(|column| column.primary_key)
            .peekable();
        key.peek().is_some() && key.all(|column| grouped.covers(reference, &column.name))
    }
}

impl Grouped {
    fn covers(&self, reference: &str, column: &str) -> bool {
        self.columns
            .iter()
            .any(|(r, c)| r == reference && c == column)
    }
}

/// What the grouping keys of a query cover.
#[derive(Default)]
struct Grouped {
    /// Columns, as `(reference, column)`.
    columns: Vec<(String, String)>,
    /// Other key expressions, as SQL text.
    expressions: Vec<String>,
    /// Select items a key names by position or alias.
    items: Vec<*const SelectItem>,
}

/// Visits a select item, reporting each column outside aggregates, window functions,
/// subqueries and grouped expressions that no key covers.
struct Ungrouped<'a, 's> {
    analyzer: &'a Analyzer<'s>,
    scope: &'a Scope,
    grouped: &'a Grouped,
    /// The expressions being skipped, outermost first.
    skipping: Vec<*const Expr>,
    queries: usize,
}

impl Visitor for Ungrouped<'_, '_> {
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
        if !self.skipping.is_empty() || self.queries > 0 {
            return ControlFlow::Continue(());
        }
        let skip = matches!(expr, Expr::Function(func) if func.over.is_some() || is_aggregate_call(expr))
            || self.grouped.expressions.contains(&expr.to_string());
        if skip {
            self.skipping.push(expr as *const Expr);
            return ControlFlow::Continue(());
        }
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return ControlFlow::Continue(());
        }
        let Some((reference, table, column)) = self.analyzer.locate_column(expr, self.scope) else {
            // A whole-row reference (`row_to_json(u)`) reads every column.
            if let (Expr::Identifier(ident), Some(table)) =
                (expr, self.analyzer.whole_row_table(expr, self.scope))
            {
                let reference = fold(ident, self.analyzer.dialect);
                if !self
                    .analyzer
                    .groups_primary_key(self.grouped, &reference, table)
                {
                    self.analyzer.record_ungrouped(&reference, "*");
                }
            }
            return ControlFlow::Continue(());
        };
        // Outer columns of a correlated query are grouped, or not, by their own query.
        if !self
            .scope
            .iter()
            .any(|source| source.reference == reference)
            || self.analyzer.schema.table(&table).is_none()
            || self.analyzer.virtual_columns(&table).is_some()
        {
            return ControlFlow::Continue(());
        }
        if !self.grouped.covers(&reference, &column)
            && !self
                .analyzer
                .groups_primary_key(self.grouped, &reference, &table)
        {
            self.analyzer.record_ungrouped(&reference, &column);
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if self.skipping.last() == Some(&(expr as *const Expr)) {
            self.skipping.pop();
        }
        ControlFlow::Continue(())
    }
}

/// Whether `expr` is a call of an aggregate function, not as a window function.
pub(super) fn is_aggregate_call(expr: &Expr) -> bool {
    let Expr::Function(func) = expr else {
        return false;
    };
    let name = function_name(func);
    let scalar_min_max =
        matches!(name.as_str(), "MIN" | "MAX") && positional_args(&func.args).len() >= 2;
    func.over.is_none()
        && !scalar_min_max
        && (matches!(name.as_str(), "COUNT" | "REGR_COUNT")
            || lookup(&name).is_some_and(super::registry::FunctionInfo::is_aggregate))
}

/// Whether `expr` calls an aggregate outside a subquery: the query is grouped then, as
/// one group of all rows.
fn has_aggregate(expr: &Expr) -> bool {
    struct Find {
        queries: usize,
    }
    impl Visitor for Find {
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
            if self.queries == 0 && is_aggregate_call(expr) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
    }
    expr.visit(&mut Find { queries: 0 }).is_break()
}
