//! `fanout-aggregate`, `chasm-join` and `unproven-join`: aggregates over rows a join repeats.
//!
//! A join is to-one toward a relation when its conditions equate, or fix to a constant,
//! every column of one of that relation's keys: a primary key, `UNIQUE` constraint or
//! unique index, or the `GROUP BY` and `DISTINCT` columns of a CTE or derived table.
//! A relation's rows repeat when a relation it reaches through to-one joins is joined to
//! another one to-many: the other side's key is matched, its own is not. A join whose
//! cardinality no key proves either way repeats nothing here, and is `unproven-join`.

use std::ops::ControlFlow;

use sqlparser::ast::Spanned;
use sqlparser::ast::{
    BinaryOperator, Distinct, DuplicateTreatment, Expr, FunctionArguments, GroupByExpr, Ident,
    JoinConstraint, Query, Select, SelectItem, SetExpr, TableFactor, TableWithJoins, Value, Visit,
    Visitor,
};
use sqlparser::tokenizer::Span;

use super::functions::function_name;
use super::grouping::{has_aggregate, is_aggregate_call};
use super::names::table_key;
use super::query::{Analyzer, Column};
use super::scope::{join_constraint, Scope};
use crate::catalog::fold;
use crate::diagnostics::NameRule;

/// Aggregates whose result changes when an input row is repeated.
const DUPLICATE_SENSITIVE: [&str; 10] = [
    "SUM",
    "AVG",
    "COUNT",
    "STRING_AGG",
    "ARRAY_AGG",
    "GROUP_CONCAT",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_GROUP_ARRAY",
    "LISTAGG",
];

impl Analyzer<'_> {
    /// Records the three rules for an aggregating `select` whose joins repeat rows.
    pub(super) fn check_fanout(&self, select: &Select, scope: &Scope) {
        if !self.diag.enabled() || !self.judges_names() || scope.len() < 2 {
            return;
        }
        let mut aggregates = Vec::new();
        for item in &select.projection {
            if let SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } = item {
                collect_aggregates(expr, &mut aggregates);
            }
        }
        if let Some(having) = &select.having {
            collect_aggregates(having, &mut aggregates);
        }
        aggregates.retain(|aggregate| aggregate.sensitive);
        if aggregates.is_empty() {
            return;
        }
        let tables: Vec<Option<&str>> = scope
            .iter()
            .map(|source| {
                let table = source.table.named()?;
                (!self.virtuals.contains_key(table))
                    .then(|| table.rsplit('.').next().unwrap_or(table))
            })
            .collect();
        let mut graph = Graph {
            keys: scope
                .iter()
                .map(|source| self.keys_of(source.table.named()))
                .collect(),
            // A table joined twice is called by its aliases.
            names: scope
                .iter()
                .zip(&tables)
                .map(|(source, table)| match table {
                    Some(table) if tables.iter().filter(|t| *t == &Some(*table)).count() == 1 => {
                        table.to_string()
                    }
                    _ => source.reference.clone(),
                })
                .collect(),
            pairs: Vec::new(),
            fixed: Vec::new(),
        };
        let mut conditions = Vec::new();
        join_conditions(&select.from, &mut conditions, &mut |using, right| {
            self.using_pairs(using, right, scope, &mut graph);
        });
        conditions.extend(&select.selection);
        for condition in conditions {
            for conjunct in conjuncts(condition) {
                self.add_equality(conjunct, scope, &mut graph);
            }
        }
        for aggregate in &aggregates {
            let mut nodes: Vec<usize> = Vec::new();
            for column in &aggregate.columns {
                if let Some((node, _)) = self.column_of(column, scope) {
                    if !nodes.contains(&node) {
                        nodes.push(node);
                    }
                }
            }
            // An aggregate reading any table at its own grain is not repeated.
            let repeated: Option<Vec<(usize, usize)>> = nodes
                .iter()
                .map(|&node| graph.multiplier(node).map(|by| (node, by)))
                .collect();
            let Some(&(node, by)) = repeated.as_ref().and_then(|found| found.first()) else {
                continue;
            };
            let _at = self.diag.at(aggregate.span);
            self.diag.record(
                NameRule::FanoutAggregate,
                format!(
                    "{} aggregates {}, whose rows repeat once per {} row",
                    aggregate.text, graph.names[node], graph.names[by]
                ),
            );
        }
        if let Some((from, a, b)) = graph.chasm() {
            self.diag.record(
                NameRule::ChasmJoin,
                format!(
                    "{} joins {} and {} one-to-many, so each row of one repeats once per row of the other",
                    graph.names[from], graph.names[a], graph.names[b]
                ),
            );
        }
        for a in 0..graph.keys.len() {
            for b in a + 1..graph.keys.len() {
                if !graph.one(a, b) && !graph.one(b, a) && graph.linked(a, b) {
                    self.diag.record(
                        NameRule::UnprovenJoin,
                        format!(
                            "the join of {} and {} matches no primary or unique key, so how often it repeats their rows is unknown",
                            graph.names[a], graph.names[b]
                        ),
                    );
                }
            }
        }
    }

    /// The keys of a source: `None` when they are not known (a view, a function).
    fn keys_of(&self, table: Option<&str>) -> Option<Vec<Vec<String>>> {
        let table = table?;
        if self.virtual_columns(table).is_some() {
            return self.virtuals.key(table).map(|key| vec![key.to_vec()]);
        }
        let keys = self.schema.unique_keys(table)?;
        Some(
            keys.into_iter()
                .map(|key| key.into_iter().map(str::to_string).collect())
                .collect(),
        )
    }

    /// The source in `scope` a column reference reads, and the column.
    fn column_of(&self, expr: &Expr, scope: &Scope) -> Option<(usize, String)> {
        let expr = strip(expr);
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return None;
        }
        let (reference, _, column) = self.locate_column(expr, scope)?;
        let node = scope
            .iter()
            .position(|source| source.reference == reference)?;
        Some((node, column))
    }

    /// Records `a = b` between two sources, or `a = constant`.
    fn add_equality(&self, conjunct: &Expr, scope: &Scope, graph: &mut Graph) {
        let Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } = conjunct
        else {
            return;
        };
        match (self.column_of(left, scope), self.column_of(right, scope)) {
            (Some(a), Some(b)) if a.0 != b.0 => graph.pairs.push((a, b)),
            (Some(column), None) if is_constant(right) => graph.fixed.push(column),
            (None, Some(column)) if is_constant(left) => graph.fixed.push(column),
            _ => {}
        }
    }

    /// Records the equalities `JOIN right USING (columns)` makes, each with the source
    /// before `right` that has the column.
    fn using_pairs(&self, using: &[Ident], right: &TableFactor, scope: &Scope, graph: &mut Graph) {
        let Some(right) = super::scope::relation_name(right)
            .and_then(|name| scope.iter().position(|source| source.reference == name))
        else {
            return;
        };
        for ident in using {
            let column = fold(ident, self.dialect);
            let left = scope[..right].iter().position(|source| {
                let qualified = Expr::CompoundIdentifier(vec![
                    Ident::new(source.reference.clone()),
                    ident.clone(),
                ]);
                self.locate_column(&qualified, scope).is_some()
            });
            if let Some(left) = left {
                graph.pairs.push(((left, column.clone()), (right, column)));
            }
        }
    }

    /// The names of the output columns of `query` no two rows share, when known: its
    /// `GROUP BY` or `DISTINCT` columns, none for an aggregate without `GROUP BY` (one
    /// row), or the key of the one table it reads when it selects that key.
    pub(super) fn output_key(&self, query: &Query, columns: &[Column]) -> Option<Vec<String>> {
        // A CTE of the subquery could shadow the table its key would be read from.
        if !self.diag.enabled() || query.with.is_some() {
            return None;
        }
        let SetExpr::Select(select) = query.body.as_ref() else {
            return None;
        };
        let positions = self.key_positions(select)?;
        positions
            .into_iter()
            .map(|at| columns.get(at).map(|column| column.name.clone()))
            .collect()
    }

    fn key_positions(&self, select: &Select) -> Option<Vec<usize>> {
        let items = &select.projection;
        let starred = items.iter().any(|item| {
            matches!(
                item,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..)
            )
        });
        let grouping = match &select.group_by {
            GroupByExpr::Expressions(keys, _) => keys.as_slice(),
            GroupByExpr::All(_) => return None,
        };
        let positions = |keys: &[Expr]| -> Option<Vec<usize>> {
            keys.iter().map(|key| position_of(key, items)).collect()
        };
        match &select.distinct {
            Some(Distinct::Distinct) if !starred => return Some((0..items.len()).collect()),
            Some(Distinct::On(keys)) if !starred => return positions(keys),
            _ => {}
        }
        if starred {
            return None;
        }
        if !grouping.is_empty() {
            if grouping.iter().any(|key| {
                matches!(
                    key,
                    Expr::Rollup(_) | Expr::Cube(_) | Expr::GroupingSets(_) | Expr::Tuple(_)
                )
            }) {
                return None;
            }
            return positions(grouping);
        }
        let aggregating = items.iter().any(|item| match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                has_aggregate(expr)
            }
            _ => false,
        });
        if aggregating {
            return Some(Vec::new());
        }
        let [from] = select.from.as_slice() else {
            return None;
        };
        let TableFactor::Table {
            name, args: None, ..
        } = &from.relation
        else {
            return None;
        };
        if !from.joins.is_empty() || self.is_virtual(name) {
            return None;
        }
        let keys = self.schema.unique_keys(&table_key(name, self.dialect))?;
        keys.into_iter().find_map(|key| {
            key.into_iter()
                .map(|column| {
                    items.iter().position(|item| match item {
                        SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                            fold(ident, self.dialect) == column
                        }
                        SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) => parts
                            .last()
                            .is_some_and(|ident| fold(ident, self.dialect) == column),
                        _ => false,
                    })
                })
                .collect()
        })
    }
}

/// The select item a grouping or `DISTINCT ON` key is: by position, alias or expression.
fn position_of(key: &Expr, items: &[SelectItem]) -> Option<usize> {
    if let Expr::Value(value) = key {
        if let Value::Number(n, _) = &value.value {
            return n
                .parse::<usize>()
                .ok()?
                .checked_sub(1)
                .filter(|&at| at < items.len());
        }
    }
    let key_name = match key {
        Expr::Identifier(ident) => Some(ident.value.to_lowercase()),
        _ => None,
    };
    items.iter().position(|item| match item {
        SelectItem::ExprWithAlias { alias, expr } => {
            key_name.as_deref() == Some(alias.value.to_lowercase().as_str()) || expr == key
        }
        SelectItem::UnnamedExpr(expr) => {
            expr == key
                || matches!((expr, &key_name), (Expr::CompoundIdentifier(parts), Some(name))
                    if parts.last().is_some_and(|last| last.value.to_lowercase() == *name))
        }
        _ => false,
    })
}

/// The sources of a SELECT, what joins them, and the multiplicity that follows.
struct Graph {
    /// Each source's keys, `None` when unknown.
    keys: Vec<Option<Vec<Vec<String>>>>,
    /// What messages call each source.
    names: Vec<String>,
    /// Columns of two sources an equality joins.
    pairs: Vec<((usize, String), (usize, String))>,
    /// Columns an equality fixes to one value.
    fixed: Vec<(usize, String)>,
}

impl Graph {
    /// Whether each row of `a` joins at most one row of `b`.
    fn one(&self, a: usize, b: usize) -> bool {
        let Some(keys) = &self.keys[b] else {
            return false;
        };
        keys.iter().any(|key| {
            key.iter().all(|column| {
                self.fixed.iter().any(|(n, c)| *n == b && c == column)
                    || self.pairs.iter().any(|((x, cx), (y, cy))| {
                        (*x == a && *y == b && cy == column) || (*y == a && *x == b && cx == column)
                    })
            })
        })
    }

    /// Whether a row of `a` may join several rows of `b`, each of which joins one of `a`.
    fn many(&self, a: usize, b: usize) -> bool {
        self.keys[b].is_some() && !self.one(a, b) && self.one(b, a)
    }

    fn linked(&self, a: usize, b: usize) -> bool {
        self.pairs
            .iter()
            .any(|((x, _), (y, _))| (*x == a && *y == b) || (*x == b && *y == a))
    }

    /// The sources `a` reaches without being repeated: to-one, or of unknown multiplicity.
    fn closure(&self, a: usize) -> Vec<usize> {
        let mut reached = vec![a];
        let mut at = 0;
        while let Some(&x) = reached.get(at) {
            at += 1;
            for y in 0..self.keys.len() {
                if !reached.contains(&y)
                    && !self.many(x, y)
                    && (self.one(x, y) || self.linked(x, y))
                {
                    reached.push(y);
                }
            }
        }
        reached
    }

    /// The source a one-to-many join repeats the rows of `a` by, if any.
    fn multiplier(&self, a: usize) -> Option<usize> {
        let reached = self.closure(a);
        reached
            .iter()
            .find_map(|&x| (0..self.keys.len()).find(|&y| !reached.contains(&y) && self.many(x, y)))
    }

    /// A source joined one-to-many to two others that don't reach each other one-to-one.
    fn chasm(&self) -> Option<(usize, usize, usize)> {
        let n = self.keys.len();
        (0..n).find_map(|x| {
            let fans: Vec<usize> = (0..n).filter(|&y| self.many(x, y)).collect();
            fans.iter().enumerate().find_map(|(i, &a)| {
                fans[i + 1..]
                    .iter()
                    .find(|&&b| !self.closure(a).contains(&b) && !self.closure(b).contains(&a))
                    .map(|&b| (x, a, b))
            })
        })
    }
}

/// A call of an aggregate function, outside subqueries and window functions.
struct Aggregate {
    text: String,
    span: Span,
    /// Whether repeating an input row changes the result.
    sensitive: bool,
    /// The column references among its arguments.
    columns: Vec<Expr>,
}

fn collect_aggregates(expr: &Expr, into: &mut Vec<Aggregate>) {
    struct Find<'a> {
        into: &'a mut Vec<Aggregate>,
        queries: usize,
        inside: usize,
    }
    impl Visitor for Find<'_> {
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
            if self.queries > 0 || self.inside > 0 {
                return ControlFlow::Continue(());
            }
            if let (true, Expr::Function(func)) = (is_aggregate_call(expr), expr) {
                self.inside += 1;
                let distinct = matches!(&func.args, FunctionArguments::List(list)
                    if list.duplicate_treatment == Some(DuplicateTreatment::Distinct));
                let mut columns = Vec::new();
                columns_in(&func.args, &mut columns);
                self.into.push(Aggregate {
                    text: expr.to_string(),
                    span: expr.span(),
                    sensitive: !distinct
                        && DUPLICATE_SENSITIVE.contains(&function_name(func).as_str()),
                    columns,
                });
            }
            ControlFlow::Continue(())
        }
        fn post_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
            if self.queries == 0 && self.inside > 0 && is_aggregate_call(expr) {
                self.inside -= 1;
            }
            ControlFlow::Continue(())
        }
    }
    let _ = expr.visit(&mut Find {
        into,
        queries: 0,
        inside: 0,
    });
}

/// The column references in `node`, outside subqueries.
fn columns_in<V: Visit>(node: &V, into: &mut Vec<Expr>) {
    struct Find<'a> {
        into: &'a mut Vec<Expr>,
        queries: usize,
    }
    impl Visitor for Find<'_> {
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
            if self.queries == 0
                && matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
            {
                self.into.push(expr.clone());
            }
            ControlFlow::Continue(())
        }
    }
    let _ = node.visit(&mut Find { into, queries: 0 });
}

/// The `ON` conditions of the joins in `from`, nested ones included; `using` gets each
/// `USING` list with the relation it joins.
pub(super) fn join_conditions<'a>(
    from: &'a [TableWithJoins],
    into: &mut Vec<&'a Expr>,
    using: &mut impl FnMut(&[Ident], &TableFactor),
) {
    for table in from {
        if let TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } = &table.relation
        {
            join_conditions(std::slice::from_ref(table_with_joins), into, using);
        }
        for join in &table.joins {
            if let TableFactor::NestedJoin {
                table_with_joins,
                alias: None,
            } = &join.relation
            {
                join_conditions(std::slice::from_ref(table_with_joins), into, using);
            }
            match join_constraint(join) {
                Some(JoinConstraint::On(on)) => into.push(on),
                Some(JoinConstraint::Using(names)) => {
                    let idents: Vec<Ident> = names
                        .iter()
                        .filter_map(|name| name.0.last()?.as_ident().cloned())
                        .collect();
                    using(&idents, &join.relation);
                }
                _ => {}
            }
        }
    }
}

/// The terms of a condition joined by `AND`.
pub(super) fn conjuncts(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let mut found = conjuncts(left);
            found.extend(conjuncts(right));
            found
        }
        Expr::Nested(inner) => conjuncts(inner),
        other => vec![other],
    }
}

pub(super) fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Nested(inner) => strip(inner),
        other => other,
    }
}

/// Whether `expr` has one value for the whole query: no column, subquery or function.
fn is_constant(expr: &Expr) -> bool {
    struct Find;
    impl Visitor for Find {
        type Break = ();
        fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
            match expr {
                Expr::Identifier(_)
                | Expr::CompoundIdentifier(_)
                | Expr::Function(_)
                | Expr::Subquery(_) => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            }
        }
    }
    expr.visit(&mut Find).is_continue()
}
