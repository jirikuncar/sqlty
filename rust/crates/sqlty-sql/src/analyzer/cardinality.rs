//! How many rows a statement returns, for calls that read only its first row.
//!
//! A relation of `FROM` returns at most one row when a condition every row passes fixes each
//! column of one of its keys (primary key, `UNIQUE` constraint or unique index) to a
//! constant, a parameter, or a column of a relation that already returns at most one row.

use std::collections::{HashMap, HashSet};

use sqlparser::ast::{
    Expr, GroupByExpr, Ident, Insert, JoinConstraint, JoinOperator, LimitClause, OnConflictAction,
    OnInsert, Query, Select, SelectItem, SetExpr, SqliteOnConflict, Statement, TableFactor,
    TableWithJoins, Value,
};

use super::dml::dml_scope;
use super::fanout::{conjuncts, pins, strip};
use super::grouping::has_aggregate;
use super::query::Analyzer;
use super::scope::Scope;
use super::Inferred;
use crate::catalog::fold;

/// How many rows a statement returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows {
    /// Exactly one: an aggregate without `GROUP BY`, `VALUES` of one row, `SELECT` without `FROM`.
    One,
    /// None or one: a key fixed by the conditions, or `LIMIT 1`.
    AtMostOne,
    /// Any number: the keys of every relation are known, and some relation's are not fixed.
    Many,
    /// Not known: a relation without known keys, a set operation, a CTE.
    Unknown,
}

impl Rows {
    fn at_most_one(self) -> Self {
        match self {
            Rows::One | Rows::AtMostOne => Rows::AtMostOne,
            _ => self,
        }
    }

    fn limited(self, limit: Option<u64>) -> Self {
        match limit {
            Some(0 | 1) => Rows::AtMostOne,
            _ => self,
        }
    }
}

/// Columns a condition fixes, and the relations they depend on.
#[derive(Default)]
struct Pins {
    /// Columns fixed to one value for the whole statement.
    fixed: Vec<(usize, String)>,
    /// `(from, (to, column))`: `column` of `to` has one value once `from` has one row.
    follows: Vec<(usize, (usize, String))>,
}

impl Analyzer<'_> {
    pub(super) fn statement_rows(&self, stmt: &Statement) -> Inferred<Rows> {
        Ok(match stmt {
            Statement::Query(query) => self.query_rows(query)?,
            Statement::Insert(insert) => self.insert_rows(insert)?,
            Statement::Update(update) if update.from.is_none() && update.table.joins.is_empty() => {
                self.target_rows(stmt, update.selection.as_ref())
            }
            Statement::Delete(delete) if delete.using.is_none() => {
                let rows = self.target_rows(stmt, delete.selection.as_ref());
                rows.limited(delete.limit.as_ref().and_then(count))
            }
            _ => Rows::Unknown,
        })
    }

    fn query_rows(&self, query: &Query) -> Inferred<Rows> {
        if query.with.is_some() || !query.pipe_operators.is_empty() {
            return Ok(Rows::Unknown);
        }
        let rows = match query.body.as_ref() {
            SetExpr::Select(select) => self.select_rows(select)?,
            SetExpr::Query(inner) => self.query_rows(inner)?,
            SetExpr::Values(values) if values.rows.len() == 1 => Rows::One,
            SetExpr::Values(_) => Rows::Many,
            _ => Rows::Unknown,
        };
        let limit = match &query.limit_clause {
            Some(LimitClause::LimitOffset { limit, .. }) => limit.as_ref().and_then(count),
            Some(LimitClause::OffsetCommaLimit { limit, .. }) => count(limit),
            None => None,
        };
        let fetch = query
            .fetch
            .as_ref()
            .filter(|fetch| !fetch.with_ties && !fetch.percent)
            .map(|fetch| fetch.quantity.as_ref().map_or(Some(1), count));
        let offset = matches!(
            &query.limit_clause,
            Some(
                LimitClause::LimitOffset {
                    offset: Some(_),
                    ..
                } | LimitClause::OffsetCommaLimit { .. }
            )
        );
        let rows = if offset { rows.at_most_one() } else { rows };
        Ok(rows.limited(limit).limited(fetch.flatten()))
    }

    fn select_rows(&self, select: &Select) -> Inferred<Rows> {
        if self.set_returning_items(&select.projection) > 0 {
            return Ok(Rows::Unknown);
        }
        let grouped = match &select.group_by {
            GroupByExpr::Expressions(keys, _) => !keys.is_empty(),
            GroupByExpr::All(_) => true,
        };
        let aggregating = select.projection.iter().any(|item| {
            matches!(item, SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. }
                if has_aggregate(expr))
        });
        if !grouped && (aggregating || select.having.is_some()) {
            return Ok(match select.having {
                Some(_) => Rows::AtMostOne,
                None => Rows::One,
            });
        }
        if select.from.is_empty() {
            return Ok(match select.selection {
                Some(_) => Rows::AtMostOne,
                None => Rows::One,
            });
        }
        if select.from.iter().any(|table| {
            std::iter::once(&table.relation)
                .chain(table.joins.iter().map(|join| &join.relation))
                .any(|factor| matches!(factor, TableFactor::NestedJoin { .. }))
        }) {
            return Ok(Rows::Unknown);
        }
        let (analyzer, scope) = self.scope_of(&select.from)?;
        Ok(analyzer.pinned(&select.from, select.selection.as_ref(), &scope))
    }

    /// Whether every relation of `from` returns at most one row under its joins and `selection`.
    fn pinned(&self, from: &[TableWithJoins], selection: Option<&Expr>, scope: &Scope) -> Rows {
        let mut pins = Pins::default();
        let mut at = 0;
        for table in from {
            at += 1;
            for join in &table.joins {
                let (constraint, outer) = match &join.join_operator {
                    JoinOperator::Join(c) | JoinOperator::Inner(c) | JoinOperator::CrossJoin(c) => {
                        (c, false)
                    }
                    JoinOperator::Left(c) | JoinOperator::LeftOuter(c) => (c, true),
                    _ => return Rows::Unknown,
                };
                match constraint {
                    JoinConstraint::On(on) => {
                        for conjunct in conjuncts(on) {
                            self.add_pin(conjunct, scope, outer.then_some(at), &mut pins);
                        }
                    }
                    JoinConstraint::Using(columns) => {
                        for name in columns {
                            let Some(ident) = name.0.last().and_then(|part| part.as_ident()) else {
                                return Rows::Unknown;
                            };
                            let column = fold(ident, self.dialect);
                            let Some(left) = (0..at).find(|&i| {
                                self.column_of(
                                    &Expr::CompoundIdentifier(vec![
                                        Ident::new(scope[i].reference.clone()),
                                        ident.clone(),
                                    ]),
                                    scope,
                                )
                                .is_some()
                            }) else {
                                return Rows::Unknown;
                            };
                            pins.follows.push((left, (at, column.clone())));
                            if !outer {
                                pins.follows.push((at, (left, column)));
                            }
                        }
                    }
                    JoinConstraint::None => {}
                    JoinConstraint::Natural | JoinConstraint::UsingAlias(..) => {
                        return Rows::Unknown
                    }
                }
                at += 1;
            }
        }
        if at != scope.len() {
            return Rows::Unknown;
        }
        for conjunct in selection.into_iter().flat_map(conjuncts) {
            self.add_pin(conjunct, scope, None, &mut pins);
        }
        self.resolve(scope, &pins)
    }

    /// Records what `conjunct` fixes; in the `ON` of an outer join only columns of its
    /// right side, `outer`, are fixed, as the rows of the left side are all kept.
    fn add_pin(&self, conjunct: &Expr, scope: &Scope, outer: Option<usize>, pins: &mut Pins) {
        let Expr::BinaryOp {
            left,
            op: sqlparser::ast::BinaryOperator::Eq,
            right,
        } = strip(conjunct)
        else {
            return;
        };
        let keeps = |node: usize| outer.is_none_or(|right| right == node);
        match (self.column_of(left, scope), self.column_of(right, scope)) {
            (Some(a), Some(b)) if a.0 != b.0 => {
                if keeps(b.0) {
                    pins.follows.push((a.0, b.clone()));
                }
                if keeps(a.0) {
                    pins.follows.push((b.0, a));
                }
            }
            (Some(column), None) if keeps(column.0) && pins_value(right, scope, column.0) => {
                pins.fixed.push(column);
            }
            (None, Some(column)) if keeps(column.0) && pins_value(left, scope, column.0) => {
                pins.fixed.push(column);
            }
            _ => {}
        }
    }

    /// The rows of `scope`: a source returns at most one row once every column of one of its
    /// keys is fixed, directly or by a source that returns at most one row.
    fn resolve(&self, scope: &Scope, pins: &Pins) -> Rows {
        let mut keys = Vec::with_capacity(scope.len());
        for source in scope {
            match self.keys_of(source.table.named()) {
                Some(found) => keys.push(found),
                None => return Rows::Unknown,
            }
        }
        let mut known: HashSet<(usize, &str)> =
            pins.fixed.iter().map(|(n, c)| (*n, c.as_str())).collect();
        let mut follows: HashMap<usize, Vec<(usize, &str)>> = HashMap::new();
        for (from, (n, c)) in &pins.follows {
            follows.entry(*from).or_default().push((*n, c.as_str()));
        }
        let fixes = |node: usize, known: &HashSet<(usize, &str)>| {
            keys[node]
                .iter()
                .any(|key| key.iter().all(|c| known.contains(&(node, c.as_str()))))
        };
        let mut one = vec![false; scope.len()];
        let mut queue: Vec<usize> = (0..scope.len()).filter(|&n| fixes(n, &known)).collect();
        for &node in &queue {
            one[node] = true;
        }
        while let Some(from) = queue.pop() {
            for &(node, column) in follows.get(&from).into_iter().flatten() {
                known.insert((node, column));
                if !one[node] && fixes(node, &known) {
                    one[node] = true;
                    queue.push(node);
                }
            }
        }
        if one.iter().all(|&known| known) {
            Rows::AtMostOne
        } else {
            Rows::Many
        }
    }

    fn insert_rows(&self, insert: &Insert) -> Inferred<Rows> {
        let rows = match &insert.source {
            None => Rows::One,
            Some(source) => self.query_rows(source)?,
        };
        let skips = insert.ignore
            || matches!(insert.or, Some(SqliteOnConflict::Ignore))
            || matches!(&insert.on, Some(OnInsert::OnConflict(conflict))
            if match &conflict.action {
                OnConflictAction::DoNothing => true,
                OnConflictAction::DoUpdate(update) => update.selection.is_some(),
            });
        Ok(if skips { rows.at_most_one() } else { rows })
    }

    /// The rows an `UPDATE` or `DELETE` of one table touches.
    fn target_rows(&self, stmt: &Statement, selection: Option<&Expr>) -> Rows {
        let scope = dml_scope(stmt, self.dialect);
        if scope.len() != 1 {
            return Rows::Unknown;
        }
        let mut pins = Pins::default();
        for conjunct in selection.into_iter().flat_map(conjuncts) {
            self.add_pin(conjunct, &scope, None, &mut pins);
        }
        self.resolve(&scope, &pins)
    }
}

/// Whether `value` has one value for every row of `scope[node]`.
fn pins_value(value: &Expr, scope: &Scope, node: usize) -> bool {
    pins(value, &scope[node].reference)
}

/// The number a `LIMIT` or `FETCH` spells out.
fn count(expr: &Expr) -> Option<u64> {
    match strip(expr) {
        Expr::Value(value) => match &value.value {
            Value::Number(n, _) => n.parse().ok(),
            _ => None,
        },
        _ => None,
    }
}
