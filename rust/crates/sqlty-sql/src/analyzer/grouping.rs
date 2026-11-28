//! `ungrouped-column`: a column a grouped query selects without grouping by it or
//! aggregating it, which PostgreSQL rejects.
//!
//! A column is grouped when a grouping key names it (`GROUP BY 1` and a result alias
//! name the select item), when a key is the expression it sits in, or when the keys
//! include its table's whole primary key, PostgreSQL's functional dependency. Columns
//! of schema tables, CTEs and derived tables are judged.

use std::ops::ControlFlow;

use rustc_hash::{FxHashMap, FxHashSet};
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
use crate::catalog::{fold, SchemaColumn};
use crate::diagnostics::Rule;

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
        let keys = grouping_keys(keys);
        // A position naming no select item is `position-out-of-range`.
        let wildcard = select.projection.iter().any(|item| {
            matches!(
                item,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..)
            )
        });
        if !wildcard
            && keys.iter().any(|key| {
                super::query::position(key, self.dialect).is_some_and(|n| {
                    !usize::try_from(n).is_ok_and(|n| (1..=select.projection.len()).contains(&n))
                })
            })
        {
            return;
        }
        let mut grouped = Grouped::default();
        for key in &keys {
            match key {
                Expr::Value(value) if matches!(value.value, Value::Number(..)) => {
                    let position = value.value.to_string().parse::<usize>().ok();
                    if let Some(item) =
                        position.and_then(|n| select.projection.get(n.wrapping_sub(1)))
                    {
                        grouped.items.insert(item as *const SelectItem);
                    }
                }
                key => match self.resolve_reference(key, scope) {
                    Some(found) => {
                        grouped
                            .columns
                            .entry(found.reference.into_owned())
                            .or_default()
                            .insert(found.column.into_owned());
                    }
                    None => {
                        if let Expr::Identifier(alias) = key {
                            grouped.items.extend(select.projection.iter().filter_map(|item| {
                                matches!(item, SelectItem::ExprWithAlias { alias: a, .. } if a.value.eq_ignore_ascii_case(&alias.value))
                                    .then_some(item as *const SelectItem)
                            }));
                        }
                        grouped.expressions.insert(key.to_string());
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
            // A CTE or derived table has no primary key to group by.
            let names: Vec<&str> = match self.virtual_columns(table) {
                Some(columns) => columns.iter().map(|column| column.name.as_str()).collect(),
                None => {
                    let columns = self.schema.table(table).unwrap_or_default();
                    if Self::groups_primary_key(grouped, &reference, columns) {
                        continue;
                    }
                    columns
                        .iter()
                        .filter(|column| !column.invisible)
                        .map(|column| column.name.as_str())
                        .collect()
                }
            };
            for column in names {
                if !grouped.covers(&reference, column) {
                    self.record_ungrouped(&reference, column);
                }
            }
        }
    }

    fn record_ungrouped(&self, reference: &str, column: &str) {
        // PostgreSQL calls an unaliased subquery `unnamed_subquery`.
        let reference = Some(reference)
            .filter(|reference| !reference.is_empty())
            .unwrap_or("unnamed_subquery");
        self.diag.record(
            Rule::UngroupedColumn,
            format!(
                "column `{reference}.{column}` must appear in GROUP BY or be used in an aggregate function"
            ),
        );
    }

    /// Whether the keys include every primary key column of `columns`, read by `reference`.
    fn groups_primary_key(grouped: &Grouped, reference: &str, columns: &[SchemaColumn]) -> bool {
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
            .get(reference)
            .is_some_and(|columns| columns.contains(column))
    }
}

/// What the grouping keys of a query cover.
#[derive(Default)]
struct Grouped {
    /// Columns, by reference.
    columns: FxHashMap<String, FxHashSet<String>>,
    /// Other key expressions, as SQL text.
    expressions: FxHashSet<String>,
    /// Select items a key names by position or alias.
    items: FxHashSet<*const SelectItem>,
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
            || (!self.grouped.expressions.is_empty()
                && self.grouped.expressions.contains(&expr.to_string()));
        if skip {
            self.skipping.push(expr as *const Expr);
            return ControlFlow::Continue(());
        }
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return ControlFlow::Continue(());
        }
        let Some(found) = self.analyzer.resolve_reference(expr, self.scope) else {
            // A whole-row reference (`row_to_json(u)`) reads every column.
            if let (Expr::Identifier(ident), Some(table)) =
                (expr, self.analyzer.whole_row_table(expr, self.scope))
            {
                let reference = fold(ident, self.analyzer.dialect);
                let columns = self.analyzer.schema.table(table).unwrap_or_default();
                if !Analyzer::groups_primary_key(self.grouped, &reference, columns) {
                    self.analyzer.record_ungrouped(&reference, "*");
                }
            }
            return ControlFlow::Continue(());
        };
        let (reference, table, column) = (&*found.reference, &*found.table, &*found.column);
        // Outer columns of a correlated query are grouped, or not, by their own query.
        let read = self.scope.reads(reference);
        let columns = match self.analyzer.virtual_columns(table) {
            Some(_) => Some(&[][..]),
            None => self.analyzer.schema.table(table),
        };
        let Some(columns) = columns.filter(|_| read) else {
            return ControlFlow::Continue(());
        };
        if !self.grouped.covers(reference, column)
            && !Analyzer::groups_primary_key(self.grouped, reference, columns)
        {
            self.analyzer.record_ungrouped(reference, column);
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
pub(super) fn has_aggregate(expr: &Expr) -> bool {
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

/// The expressions of `GROUP BY` keys, those of `ROLLUP`, `CUBE`, `GROUPING SETS` and
/// tuples included.
pub(super) fn grouping_keys(keys: &[Expr]) -> Vec<&Expr> {
    keys.iter()
        .flat_map(|key| match key {
            Expr::Rollup(sets) | Expr::Cube(sets) | Expr::GroupingSets(sets) => {
                sets.iter().flatten().collect()
            }
            Expr::Tuple(exprs) => exprs.iter().collect(),
            key => vec![key],
        })
        .collect()
}
