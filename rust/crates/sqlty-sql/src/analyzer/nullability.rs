//! Nullability: whether a result column can be NULL.
//!
//! Nullability is a field next to a column's type (see [`Column::nullability`]), never part
//! of the type string. It is three-valued: a column is [`Nullability::Nullable`] only where
//! the schema or the query's shape proves NULL can occur. Columns of an unresolved source and
//! expressions the analyzer does not read are [`Nullability::Unknown`], which row types treat
//! as non-optional.

use std::rc::Rc;

use sqlparser::ast::{
    AccessExpr, BinaryOperator, CastKind, DataType, Expr, FunctionArguments, Ident, Join,
    JoinConstraint, JoinOperator, Query, Select, SelectItem, SetExpr, Subscript, TableFactor,
    TableWithJoins, Value,
};

use super::dialect::SqlDialect;
use super::functions::{function_name, positional_args};
use super::hashing::ExprKey;
use super::inference::{binary_rules, function_rules, FunctionNullability, RuleNullability};
use super::literals::python_kind;
use super::memo::Reporting;
use super::names::{split_access, table_key};
use super::operators::operand_type;
use super::query::Analyzer;
use super::registry::{lookup, Behavior};
use super::scope::{table_scope, Scope};
use crate::catalog::fold;
use rustc_hash::FxHashSet;

/// Whether a column can be NULL.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nullability {
    /// NULL can occur.
    Nullable,
    /// NULL cannot occur.
    NonNull,
    /// Not known: treated as non-null when typing a row.
    #[default]
    Unknown,
}

impl Nullability {
    /// What a schema column's `NOT NULL` / `PRIMARY KEY` says.
    pub(super) fn declared(not_null: bool) -> Self {
        if not_null {
            Nullability::NonNull
        } else {
            Nullability::Nullable
        }
    }
}

/// What a SELECT's FROM and conditions tell about NULLs.
///
/// The lists are shared between the facts of a query and those of the expressions and
/// subqueries inside it, which mostly add nothing: cloning is a reference count, and a list
/// is copied only when something is added to a shared one.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Facts {
    /// References of relations an outer join null-extends: all their columns can be NULL.
    null_extended: Rc<Vec<String>>,
    /// `(reference, column)` pairs the conditions in force rule out as NULL.
    pub(super) non_null: Rc<Vec<(String, String)>>,
    /// References whose relation has a row in each result row: a null-extended relation
    /// with a column the conditions rule out as NULL cannot be the all-NULL row.
    pub(super) present: Rc<Vec<String>>,
    /// Expressions the conditions rule out as NULL, such as `doc -> 'k'` after `doc ? 'k'`.
    /// Compared structurally (ignoring spans), not by their printed form.
    pub(super) non_null_exprs: Rc<FxHashSet<ExprKey>>,
    /// Whether the SELECT has a plain GROUP BY, so every group holds at least one row.
    pub(super) grouped: bool,
    /// Whether the select list runs several set-returning functions, which PostgreSQL
    /// runs in lockstep, padding the shorter ones with NULL.
    pub(super) lockstep: bool,
    /// `(reference, column)` pairs that `ROLLUP`, `CUBE`, `GROUPING SETS` or `WITH ROLLUP`
    /// turn into NULL in their subtotal rows.
    rolled_up: Rc<Vec<(String, String)>>,
    /// Other grouping expressions such subtotal rows set to NULL.
    rolled_up_exprs: Rc<FxHashSet<ExprKey>>,
    /// The values the conditions in force leave a column (`WHERE status = 'paid'`).
    pub(super) values: Rc<Vec<super::narrowing::ColumnValues>>,
    /// `(reference, column)` pairs the conditions in force prove NULL (`col IS NULL`).
    pub(super) null: Rc<Vec<(String, String)>>,
}

impl Facts {
    /// Add the relations the outer joins of `from` may fill with NULLs.
    pub(super) fn join(&mut self, extended: Vec<String>) {
        Rc::make_mut(&mut self.null_extended).extend(extended);
    }

    /// Whether the relation `reference` can be a row of NULLs. A condition NULL cannot
    /// satisfy on one of its columns proves the row is there.
    pub(super) fn is_null_extended(&self, reference: &str) -> bool {
        self.null_extended.iter().any(|r| r == reference)
            && !self.present.iter().any(|r| r == reference)
    }

    fn is_non_null(&self, reference: &str, column: &str) -> bool {
        self.non_null
            .iter()
            .any(|(r, c)| r == reference && c == column)
    }

    /// Nullability of a column of `reference`, given what its source declares. `column` is
    /// the identity the resolver gives it (see [`Analyzer::column_key`]), never raw text.
    pub(super) fn column(
        &self,
        reference: &str,
        column: &str,
        declared: Nullability,
    ) -> Nullability {
        // A subtotal row is made after WHERE, so a condition cannot rule its NULL out.
        let rolled = self
            .rolled_up
            .iter()
            .any(|(r, c)| r == reference && c == column);
        if rolled {
            Nullability::Nullable
        } else if self.is_non_null(reference, column) {
            Nullability::NonNull
        } else if self.is_null_extended(reference) {
            Nullability::Nullable
        } else {
            declared
        }
    }

    /// Forget what is known about relations a nested SELECT defines itself.
    pub(super) fn without(&mut self, scope: &Scope) {
        let shadowed = |r: &String| scope.iter().any(|source| source.reference == *r);
        // Rebuilding a list only when it has something to forget keeps shared ones shared.
        fn forget<T: Clone>(list: &mut Rc<Vec<T>>, gone: impl Fn(&T) -> bool) {
            if list.iter().any(&gone) {
                Rc::make_mut(list).retain(|item| !gone(item));
            }
        }
        forget(&mut self.null_extended, |r| shadowed(r));
        forget(&mut self.non_null, |(r, _)| shadowed(r));
        forget(&mut self.present, |r| shadowed(r));
        forget(&mut self.rolled_up, |(r, _)| shadowed(r));
        forget(&mut self.values, |(r, _, _)| shadowed(r));
        // An expression does not say which relation its names belong to, and the nested
        // SELECT may bind them to its own: none of them carries over.
        self.non_null_exprs = Rc::default();
        self.rolled_up_exprs = Rc::default();
    }

    /// These facts without the grouping subtotals: what an aggregate reads is the rows of
    /// a group, not its subtotal row.
    fn ungrouped_rows(&self) -> Facts {
        Facts {
            rolled_up: Rc::default(),
            rolled_up_exprs: Rc::default(),
            ..self.clone()
        }
    }
}

impl Analyzer<'_> {
    /// References of the relations the outer joins of `from` may fill with NULLs.
    pub(super) fn null_extended(&self, from: &[TableWithJoins]) -> Vec<String> {
        let mut extended = Vec::new();
        for table in from {
            self.group(table, &mut extended);
        }
        extended
    }

    /// The relations of one `a JOIN b JOIN c` chain, by reference; null-extended ones go to
    /// `extended`.
    fn group<'f>(
        &self,
        table: &'f TableWithJoins,
        extended: &mut Vec<String>,
    ) -> Vec<(String, Option<&'f TableFactor>)> {
        let mut left = self.factor_references(&table.relation, extended);
        for join in &table.joins {
            let right = self.factor_references(&join.relation, extended);
            match &join.join_operator {
                JoinOperator::Left(_) | JoinOperator::LeftOuter(_) | JoinOperator::OuterApply
                    if !always_matches(join)
                        && !self.joins_a_reference(&left, &right, join, extended) =>
                {
                    extended.extend(right.iter().map(|(reference, _)| reference.clone()));
                }
                JoinOperator::Right(_) | JoinOperator::RightOuter(_)
                    if !self.joins_a_reference(&right, &left, join, extended) =>
                {
                    extended.extend(left.iter().map(|(reference, _)| reference.clone()));
                }
                JoinOperator::FullOuter(_) => {
                    extended.extend(left.iter().map(|(reference, _)| reference.clone()));
                    extended.extend(right.iter().map(|(reference, _)| reference.clone()));
                }
                _ => {}
            }
            left.extend(right);
        }
        left
    }

    fn factor_references<'f>(
        &self,
        factor: &'f TableFactor,
        extended: &mut Vec<String>,
    ) -> Vec<(String, Option<&'f TableFactor>)> {
        match factor {
            TableFactor::NestedJoin {
                table_with_joins,
                alias: None,
            } => self
                .group(table_with_joins, extended)
                .into_iter()
                .map(|(reference, _)| (reference, None))
                .collect(),
            // The reference the scope of the SELECT gives the relation.
            other => table_scope(other, self.dialect)
                .into_iter()
                .map(|source| (source.reference, Some(other)))
                .collect(),
        }
    }

    /// Whether `join` always finds a row of `outer`, the side it would fill with NULLs: a
    /// single base table that a `NOT NULL` foreign key of a table in `inner` references,
    /// joined on exactly the key's columns. SQLite does not enforce foreign keys by default.
    fn joins_a_reference(
        &self,
        inner: &[(String, Option<&TableFactor>)],
        outer: &[(String, Option<&TableFactor>)],
        join: &Join,
        extended: &[String],
    ) -> bool {
        let (JoinOperator::Left(JoinConstraint::On(on))
        | JoinOperator::LeftOuter(JoinConstraint::On(on))
        | JoinOperator::Right(JoinConstraint::On(on))
        | JoinOperator::RightOuter(JoinConstraint::On(on))) = &join.join_operator
        else {
            return false;
        };
        let ([(parent, Some(parent_factor))], false) = (outer, self.dialect == SqlDialect::Sqlite)
        else {
            return false;
        };
        let Some(parent_key) = self.base_table(parent_factor) else {
            return false;
        };
        // `(child reference, child column, parent column)` per equality of the ON.
        let mut pairs = Vec::new();
        for condition in conjuncts(on) {
            let Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            } = condition
            else {
                return false;
            };
            let (Some(a), Some(b)) = (self.qualified(left), self.qualified(right)) else {
                return false;
            };
            let ((child, column), (_, referred)) = match (a.0 == *parent, b.0 == *parent) {
                (false, true) => (a, b),
                (true, false) => (b, a),
                _ => return false,
            };
            pairs.push((child, column, referred));
        }
        let Some((child, _, _)) = pairs.first() else {
            return false;
        };
        let child = child.clone();
        if pairs.iter().any(|(reference, _, _)| *reference != child) || extended.contains(&child) {
            return false;
        }
        let Some(child_key) = inner
            .iter()
            .find(|(reference, _)| *reference == child)
            .and_then(|(_, factor)| self.base_table((*factor)?))
        else {
            return false;
        };
        let Some(columns) = self.schema.table(&child_key) else {
            return false;
        };
        let not_null = |name: &String| columns.iter().any(|c| c.name == *name && c.not_null);
        self.schema.foreign_keys(&child_key).any(|fk| {
            fk.table == parent_key
                && fk.columns.len() == pairs.len()
                && fk.columns.iter().all(not_null)
                && fk
                    .columns
                    .iter()
                    .zip(&fk.referred)
                    .all(|(column, referred)| {
                        pairs.iter().any(|(_, c, r)| c == column && r == referred)
                    })
                && pairs.iter().all(|(_, c, r)| {
                    fk.columns
                        .iter()
                        .zip(&fk.referred)
                        .any(|(column, referred)| c == column && r == referred)
                })
        })
    }

    /// The catalog key of a base table in FROM: not a CTE, a view or a function.
    fn base_table(&self, factor: &TableFactor) -> Option<String> {
        let TableFactor::Table {
            name, args: None, ..
        } = factor
        else {
            return None;
        };
        let key = table_key(name, self.dialect);
        if self.virtuals.get(&key).is_some() {
            return None;
        }
        self.schema.table_id(&key).map(str::to_string)
    }

    /// `(reference, column)` of `t.col`.
    fn qualified(&self, expr: &Expr) -> Option<(String, String)> {
        match expr {
            Expr::Nested(inner) => self.qualified(inner),
            Expr::CompoundIdentifier(parts) => match parts.as_slice() {
                [reference, column] => {
                    Some((reference.value.to_lowercase(), fold(column, self.dialect)))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

/// The operands of a chain of `AND`.
fn conjuncts(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Nested(inner) => conjuncts(inner),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let mut all = conjuncts(left);
            all.extend(conjuncts(right));
            all
        }
        other => vec![other],
    }
}

/// Whether an outer join always finds its one row, so it never null-extends it: a derived
/// table that returns exactly one row (an ungrouped aggregate), joined `ON TRUE` or applied.
fn always_matches(join: &Join) -> bool {
    let unconditional = match &join.join_operator {
        JoinOperator::Left(constraint) | JoinOperator::LeftOuter(constraint) => {
            matches!(constraint, JoinConstraint::On(on) if is_true(on))
        }
        JoinOperator::OuterApply => true,
        _ => false,
    };
    unconditional
        && matches!(&join.relation, TableFactor::Derived { subquery, .. } if returns_one_row(subquery))
}

fn is_true(expr: &Expr) -> bool {
    match expr {
        Expr::Nested(inner) => is_true(inner),
        Expr::Value(value) => matches!(value.value, Value::Boolean(true)),
        _ => false,
    }
}

/// Strictest of several parts: NULL if any part can be NULL, else unknown if any is unknown.
fn any_nullable(parts: impl IntoIterator<Item = Nullability>) -> Nullability {
    let mut result = Nullability::NonNull;
    for part in parts {
        match part {
            Nullability::Nullable => return Nullability::Nullable,
            Nullability::Unknown => result = Nullability::Unknown,
            Nullability::NonNull => {}
        }
    }
    result
}

/// NULL only if every part can be NULL (`COALESCE`): one non-null part settles it.
fn all_nullable(parts: impl IntoIterator<Item = Nullability>) -> Nullability {
    let mut result = Nullability::Nullable;
    for part in parts {
        match part {
            Nullability::NonNull => return Nullability::NonNull,
            Nullability::Unknown => result = Nullability::Unknown,
            Nullability::Nullable => {}
        }
    }
    result
}

/// Whether the frame of a window always holds the current row. The default frame does; a
/// spelled-out one must start at or before the current row and end at or after it.
fn frame_holds_current_row(over: Option<&sqlparser::ast::WindowType>) -> bool {
    use sqlparser::ast::{WindowFrameBound::*, WindowType};
    match over {
        Some(WindowType::WindowSpec(spec)) => spec.window_frame.as_ref().is_none_or(|frame| {
            !matches!(frame.start_bound, Following(_))
                && !matches!(frame.end_bound, Some(Preceding(_)))
        }),
        _ => false,
    }
}

/// Whether every row of `select` comes from rows its WHERE lets through: not so for an
/// aggregate without GROUP BY, which returns a row when none match, or a HAVING.
pub(super) fn rows_pass_where(select: &Select) -> bool {
    select.having.is_none()
        && (is_grouped(select)
            || !select.projection.iter().any(|item| match item {
                SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                    contains_aggregate(expr)
                }
                _ => false,
            }))
}

/// Whether `expr` holds an aggregate call outside any window or subquery.
fn contains_aggregate(expr: &Expr) -> bool {
    match expr {
        Expr::Function(func) => {
            let name = function_name(func);
            let args = positional_args(&func.args);
            let scalar_min_max = matches!(name.as_str(), "MIN" | "MAX") && args.len() >= 2;
            (func.over.is_none()
                && !scalar_min_max
                && (matches!(name.as_str(), "COUNT" | "REGR_COUNT")
                    || lookup(&name).is_some_and(super::registry::FunctionInfo::is_aggregate)))
                || args.into_iter().any(contains_aggregate)
        }
        Expr::Nested(inner)
        | Expr::UnaryOp { expr: inner, .. }
        | Expr::Cast { expr: inner, .. }
        | Expr::IsNull(inner)
        | Expr::IsNotNull(inner) => contains_aggregate(inner),
        Expr::BinaryOp { left, right, .. } => contains_aggregate(left) || contains_aggregate(right),
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            operand.as_deref().is_some_and(contains_aggregate)
                || conditions.iter().any(|when| {
                    contains_aggregate(&when.condition) || contains_aggregate(&when.result)
                })
                || else_result.as_deref().is_some_and(contains_aggregate)
        }
        _ => false,
    }
}

/// Whether `expr` is a number literal other than zero.
fn is_nonzero_number(expr: &Expr) -> bool {
    match expr {
        Expr::Nested(inner) => is_nonzero_number(inner),
        Expr::Value(value) => match &value.value {
            Value::Number(digits, _) => digits.parse::<f64>().is_ok_and(|n| n != 0.0),
            _ => false,
        },
        _ => false,
    }
}

/// Grouping expressions that a subtotal row sets to NULL: those inside `ROLLUP`, `CUBE` and
/// `GROUPING SETS`, and all of them under MySQL's `WITH ROLLUP`.
fn rolled_up(select: &Select) -> Vec<&Expr> {
    let sqlparser::ast::GroupByExpr::Expressions(by, modifiers) = &select.group_by else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for expr in by {
        match expr {
            Expr::Rollup(sets) | Expr::Cube(sets) | Expr::GroupingSets(sets) => {
                found.extend(sets.iter().flatten());
            }
            other if !modifiers.is_empty() => found.push(other),
            _ => {}
        }
    }
    found
}

/// Whether the SELECT has a plain GROUP BY, so that every group holds at least one row.
/// `GROUP BY ()`, `ROLLUP`, `CUBE` and `GROUPING SETS` also make a group of all rows, which
/// may be empty.
fn is_grouped(select: &Select) -> bool {
    match &select.group_by {
        sqlparser::ast::GroupByExpr::Expressions(by, modifiers) if modifiers.is_empty() => {
            let aggregate_all = |expr: &Expr| {
                matches!(expr, Expr::Tuple(items) if items.is_empty())
                    || matches!(
                        expr,
                        Expr::Rollup(_) | Expr::Cube(_) | Expr::GroupingSets(_)
                    )
            };
            !by.is_empty() && !by.iter().any(aggregate_all)
        }
        _ => false,
    }
}

/// Whether the query always returns exactly one row: no FROM, or an aggregate without GROUP BY.
fn returns_one_row(query: &Query) -> bool {
    let SetExpr::Select(select) = query.body.as_ref() else {
        return false;
    };
    if select.having.is_some() || query.limit_clause.is_some() {
        return false;
    }
    let ungrouped = !is_grouped(select);
    let aggregated = select.projection.iter().any(|item| match item {
        SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
            contains_aggregate(expr)
        }
        _ => false,
    });
    ungrouped && (aggregated || (select.from.is_empty() && select.selection.is_none()))
}

impl<'a> Analyzer<'a> {
    /// The facts in force inside `select`: those of enclosing queries, minus the names it
    /// redefines, plus its own joins.
    pub(super) fn select_facts(&self, select: &Select, scope: &Scope) -> Facts {
        let mut facts = self.facts.clone();
        facts.without(scope);
        facts.join(self.null_extended(&select.from));
        facts.grouped = is_grouped(select);
        for expr in rolled_up(select) {
            match self.locate_column(expr, scope) {
                Some((reference, _, column)) => {
                    Rc::make_mut(&mut facts.rolled_up).push((reference, column));
                }
                None => {
                    Rc::make_mut(&mut facts.rolled_up_exprs).insert(ExprKey::of(expr));
                }
            }
        }
        self.narrow_select(select, scope, facts)
    }

    /// The identity narrowing knows column `name` of the relation `reference` by: what
    /// [`Self::locate_column`] resolves a reference to it to, so that a condition on the
    /// column and a read of it agree whatever the folding of identifiers.
    pub(super) fn column_key(&self, reference: &str, name: &str, scope: &Scope) -> String {
        let quoted = |text: &str| Ident::with_quote('"', text);
        let expr = Expr::CompoundIdentifier(vec![quoted(reference), quoted(name)]);
        self.locate_column(&expr, scope)
            .map_or_else(|| name.to_string(), |(_, _, column)| column)
    }

    /// Whether `expr` can be NULL under `facts`.
    pub(super) fn nullability(&self, expr: &Expr, scope: &Scope, facts: &Facts) -> Nullability {
        if !facts.non_null_exprs.is_empty() || !facts.rolled_up_exprs.is_empty() {
            let key = ExprKey::of(expr);
            if facts.non_null_exprs.contains(&key) {
                return Nullability::NonNull;
            }
            if facts.rolled_up_exprs.contains(&key) {
                return Nullability::Nullable;
            }
        }
        let of = |inner: &Expr| self.nullability(inner, scope, facts);
        match expr {
            Expr::Nested(inner) | Expr::Collate { expr: inner, .. } => of(inner),
            Expr::Convert {
                expr: inner,
                is_try: false,
                data_type,
                ..
            } => match data_type {
                Some(data_type) if self.mysql_cast_can_fail(inner, data_type, scope) => {
                    Nullability::Nullable
                }
                _ => of(inner),
            },
            Expr::Cast {
                expr: inner,
                kind,
                data_type,
                ..
            } => match kind {
                CastKind::TryCast | CastKind::SafeCast => Nullability::Nullable,
                _ if self.mysql_cast_can_fail(inner, data_type, scope) => Nullability::Nullable,
                _ => of(inner),
            },
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                self.column_nullability(expr, scope, facts)
            }
            Expr::Value(value) => match value.value {
                Value::Null => Nullability::Nullable,
                Value::Placeholder(_) => Nullability::Unknown,
                _ => Nullability::NonNull,
            },
            Expr::TypedString(_)
            | Expr::Interval(_)
            | Expr::Array(_)
            | Expr::Tuple(_)
            | Expr::Exists { .. }
            | Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::IsTrue(_)
            | Expr::IsNotTrue(_)
            | Expr::IsFalse(_)
            | Expr::IsNotFalse(_)
            | Expr::IsUnknown(_)
            | Expr::IsNotUnknown(_)
            | Expr::IsDistinctFrom(..)
            | Expr::IsNotDistinctFrom(..) => Nullability::NonNull,
            Expr::UnaryOp { expr: inner, .. }
            | Expr::IsJson { expr: inner, .. }
            | Expr::IsNormalized { expr: inner, .. } => of(inner),
            Expr::BinaryOp { left, op, right } => match self.json_extract(expr, scope) {
                // A member every object has is there whenever the object is.
                Some(value) if !value.missing => of(left),
                _ => self.binary_nullability(left, op, right, scope, facts),
            },
            Expr::Between {
                expr: inner,
                low,
                high,
                ..
            } => {
                let of = |inner: &Expr| self.operand_nullability(inner, scope, facts);
                any_nullable([of(inner), of(low), of(high)])
            }
            Expr::InList {
                expr: inner, list, ..
            } => {
                let of = |inner: &Expr| self.operand_nullability(inner, scope, facts);
                any_nullable(std::iter::once(of(inner)).chain(list.iter().map(of)))
            }
            Expr::InSubquery {
                expr: inner,
                subquery,
                ..
            } => any_nullable([
                self.operand_nullability(inner, scope, facts),
                match &*self.subquery_columns(subquery, scope, Reporting::Silent) {
                    Ok(Some(columns)) => {
                        any_nullable(columns.iter().map(|column| column.nullability))
                    }
                    _ => Nullability::Unknown,
                },
            ]),
            Expr::InUnnest {
                expr: inner,
                array_expr,
                ..
            } => any_nullable([
                of(inner),
                self.element_nullability(array_expr, scope, facts),
            ]),
            Expr::Like {
                expr: inner,
                pattern,
                ..
            }
            | Expr::ILike {
                expr: inner,
                pattern,
                ..
            }
            | Expr::SimilarTo {
                expr: inner,
                pattern,
                ..
            }
            | Expr::RLike {
                expr: inner,
                pattern,
                ..
            } => any_nullable([of(inner), of(pattern)]),
            Expr::AnyOp { left, right, .. } | Expr::AllOp { left, right, .. } => {
                any_nullable([of(left), self.element_nullability(right, scope, facts)])
            }
            Expr::Prefixed { value: inner, .. }
            | Expr::Extract { expr: inner, .. }
            | Expr::Ceil { expr: inner, .. }
            | Expr::Floor { expr: inner, .. } => of(inner),
            Expr::Position { expr: inner, r#in } => any_nullable([of(inner), of(r#in)]),
            Expr::Overlay {
                expr: inner,
                overlay_what,
                overlay_from,
                overlay_for,
            } => any_nullable(
                [of(inner), of(overlay_what), of(overlay_from)]
                    .into_iter()
                    .chain(overlay_for.as_deref().map(of)),
            ),
            Expr::Substring {
                expr: inner,
                substring_from,
                substring_for,
                ..
            } => self.substring_nullability(
                inner,
                substring_from.as_deref(),
                substring_for.as_deref(),
                scope,
                facts,
            ),
            Expr::Trim {
                expr: inner,
                trim_what,
                trim_characters,
                ..
            } => any_nullable(
                std::iter::once(of(inner))
                    .chain(trim_what.as_deref().map(of))
                    .chain(trim_characters.iter().flatten().map(of)),
            ),
            Expr::Subquery(query) => {
                if returns_one_row(query) {
                    self.subquery_column(query, scope)
                } else {
                    // No row at all reads as NULL.
                    Nullability::Nullable
                }
            }
            Expr::Case {
                operand,
                conditions,
                else_result,
                ..
            } => self.case_nullability(
                operand.as_deref(),
                conditions,
                else_result.as_deref(),
                scope,
                facts,
            ),
            Expr::CompoundFieldAccess { root, access_chain } => {
                self.access_nullability(root, access_chain, scope, facts)
            }
            Expr::Function(func) => self.function_nullability(func, scope, facts),
            _ => Nullability::Unknown,
        }
    }

    /// A column reference: what its source declares, narrowed by `facts`. A column is
    /// also non-null where the others of a group a `CHECK` keeps one of are proven NULL
    /// (`CASE WHEN a IS NOT NULL THEN a ELSE b END`).
    fn column_nullability(&self, expr: &Expr, scope: &Scope, facts: &Facts) -> Nullability {
        let Some((reference, table, column)) = self.locate_column(expr, scope) else {
            return Nullability::Unknown;
        };
        let nullability = facts.column(
            &reference,
            &column,
            self.declared_nullability(&table, &column),
        );
        if nullability == Nullability::NonNull
            || facts.column(&reference, &column, Nullability::NonNull) != Nullability::NonNull
        {
            return nullability;
        }
        let mut columns: Vec<&str> = facts
            .null
            .iter()
            .filter(|(r, _)| *r == reference)
            .map(|(_, c)| c.as_str())
            .collect();
        if columns.is_empty() {
            return nullability;
        }
        columns.push(&column);
        if self.checked_non_null(&table, &columns) {
            Nullability::NonNull
        } else {
            nullability
        }
    }

    /// Whether `args` hold every column of a group a `CHECK` keeps at least one of non-null
    /// (`num_nonnulls(a, b) = 1`), from a relation that is not null-extended here.
    fn coalesces_checked_group(&self, args: &[&Expr], scope: &Scope, facts: &Facts) -> bool {
        let mut by_relation: Vec<(String, String, Vec<String>)> = Vec::new();
        for arg in args {
            if !matches!(arg, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
                continue;
            }
            let Some((reference, table, column)) = self.locate_column(arg, scope) else {
                continue;
            };
            // A subtotal row or a null-extended row makes every column NULL at once.
            if facts.column(&reference, &column, Nullability::NonNull) != Nullability::NonNull {
                continue;
            }
            match by_relation.iter_mut().find(|(r, _, _)| *r == reference) {
                Some((_, _, columns)) => columns.push(column),
                None => by_relation.push((reference, table, vec![column])),
            }
        }
        by_relation.iter().any(|(_, table, columns)| {
            let columns: Vec<&str> = columns.iter().map(String::as_str).collect();
            self.checked_non_null(table, &columns)
        })
    }

    /// MySQL casts a value that is not a date or time to one as NULL when it does not read
    /// as one, such as `CAST('abc' AS DATE)`. A value of unknown type keeps its nullability.
    fn mysql_cast_can_fail(&self, inner: &Expr, data_type: &DataType, scope: &Scope) -> bool {
        let target = data_type.to_string().to_uppercase();
        self.dialect == SqlDialect::MySql
            && (target.starts_with("DATE") || target.starts_with("TIME") || target == "YEAR")
            && self.expr(inner, scope).is_ok_and(|typed| {
                typed.is_known()
                    && !matches!(
                        operand_type(&typed),
                        Some("DATE" | "DATETIME" | "TIMESTAMP" | "TIME")
                    )
            })
    }

    /// A row is never NULL, but compared to another it is NULL when one of its fields is.
    fn operand_nullability(&self, expr: &Expr, scope: &Scope, facts: &Facts) -> Nullability {
        match expr {
            Expr::Tuple(items) => any_nullable(
                items
                    .iter()
                    .map(|item| self.operand_nullability(item, scope, facts)),
            ),
            Expr::Nested(inner) => self.operand_nullability(inner, scope, facts),
            _ => self.nullability(expr, scope, facts),
        }
    }

    fn binary_nullability(
        &self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        use BinaryOperator::*;
        let of = |inner: &Expr| self.operand_nullability(inner, scope, facts);
        // MySQL gives NULL for a date moved out of its range.
        if self.dialect == SqlDialect::MySql
            && matches!(op, Plus | Minus)
            && [left, right]
                .iter()
                .any(|side| matches!(side, Expr::Interval(_)))
        {
            return Nullability::Nullable;
        }
        let mut rules = binary_rules(op)
            .iter()
            .filter(|rule| rule.in_dialect(self.dialect));
        // Rules that agree on the nullability, the last for any operands, need no types.
        let agreed = rules.next().filter(|first| {
            let mut last = *first;
            rules.all(|rule| {
                last = rule;
                rule.nullability == first.nullability
            }) && !last.reads_left()
                && !last.reads_right()
        });
        let found = match agreed {
            Some(rule) => Some(rule),
            None => self
                .binary_rule(left, op, right, scope)
                .ok()
                .flatten()
                .map(|(rule, _)| rule),
        };
        if let Some(rule) = found {
            return match rule.nullability {
                RuleNullability::Operands => any_nullable([of(left), of(right)]),
                RuleNullability::Nullable => Nullability::Nullable,
                RuleNullability::NonNull => Nullability::NonNull,
                // The right side is read where the left one is true (AND) or not true (OR).
                RuleNullability::ShortCircuit => {
                    let known = if *op == And {
                        self.learn(left, true, scope)
                    } else {
                        self.learn_not_true(left, scope, facts)
                    };
                    any_nullable([of(left), self.nullability(right, scope, &facts.with(known))])
                }
                RuleNullability::NonzeroDivisor if !is_nonzero_number(right) => {
                    Nullability::Nullable
                }
                RuleNullability::NonzeroDivisor => any_nullable([of(left), of(right)]),
            };
        }
        any_nullable([of(left), of(right)])
    }

    fn substring_nullability(
        &self,
        inner: &Expr,
        from: Option<&Expr>,
        length: Option<&Expr>,
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        let of = |inner: &Expr| self.nullability(inner, scope, facts);
        // PostgreSQL's `substring(s FROM 'pattern')` is NULL when nothing matches.
        let by_pattern = self.dialect == SqlDialect::Postgres
            && from.is_some_and(|from| {
                !matches!(self.expr(from, scope), Ok(typed) if python_kind(&typed) == Some("int"))
            });
        let missed = if by_pattern {
            Nullability::Nullable
        } else {
            Nullability::NonNull
        };
        any_nullable(
            std::iter::once(of(inner))
                .chain([missed])
                .chain(from.map(of))
                .chain(length.map(of)),
        )
    }

    /// `col[1].field`: reading past an array's end or a row's field.
    fn access_nullability(
        &self,
        root: &Expr,
        access_chain: &[AccessExpr],
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        let (root, chain) = split_access(root, access_chain);
        let mut result = self.nullability(&root, scope, facts);
        for access in chain {
            result = match access {
                // An index past the end reads as NULL.
                AccessExpr::Subscript(Subscript::Index { .. }) => Nullability::Nullable,
                AccessExpr::Subscript(Subscript::Slice { .. }) => result,
                // A field of a row can hold NULL; a NULL row has only NULL fields.
                AccessExpr::Dot(_) if result == Nullability::Nullable => result,
                AccessExpr::Dot(_) => Nullability::Unknown,
            };
        }
        result
    }

    /// Whether the elements of an array, or the rows of a subquery, can be NULL: a non-null
    /// array still holds NULL elements, unless it is spelled out here.
    fn element_nullability(&self, set: &Expr, scope: &Scope, facts: &Facts) -> Nullability {
        match set {
            Expr::Nested(inner) => self.element_nullability(inner, scope, facts),
            Expr::Array(array) => any_nullable(
                array
                    .elem
                    .iter()
                    .map(|elem| self.nullability(elem, scope, facts)),
            ),
            Expr::Subquery(query) => self.subquery_column(query, scope),
            _ => Nullability::Nullable,
        }
    }

    /// Nullability of the first column of a subquery, as its own query reads it.
    fn subquery_column(&self, query: &Query, scope: &Scope) -> Nullability {
        match &*self.subquery_columns(query, scope, Reporting::Silent) {
            Ok(Some(columns)) => columns
                .first()
                .map_or(Nullability::Unknown, |col| col.nullability),
            _ => Nullability::Unknown,
        }
    }

    fn case_nullability(
        &self,
        operand: Option<&Expr>,
        conditions: &[sqlparser::ast::CaseWhen],
        else_result: Option<&Expr>,
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        // A branch is read where its WHEN holds and the earlier ones do not.
        let mut skipped = facts.clone();
        let mut results = Vec::new();
        for when in conditions {
            let condition = match operand {
                Some(operand) => Expr::BinaryOp {
                    left: Box::new(operand.clone()),
                    op: BinaryOperator::Eq,
                    right: Box::new(when.condition.clone()),
                },
                None => when.condition.clone(),
            };
            let taken = skipped.with(self.learn(&condition, true, scope));
            results.push(self.nullability(&when.result, scope, &taken));
            let unmatched = self.learn_not_true(&condition, scope, &skipped);
            skipped = skipped.with(unmatched);
        }
        // Without ELSE, a row no WHEN matches yields NULL.
        results.push(else_result.map_or(Nullability::Nullable, |expr| {
            self.nullability(expr, scope, &skipped)
        }));
        any_nullable(results)
    }

    fn function_nullability(
        &self,
        func: &sqlparser::ast::Function,
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        let name = function_name(func);
        if facts.lockstep && self.is_set_returning(func) {
            return Nullability::Nullable;
        }
        let info = lookup(&name);
        let args = positional_args(&func.args);
        let all_positional = match &func.args {
            FunctionArguments::List(list) => list.args.len() == args.len(),
            _ => true,
        };
        let of = |inner: &Expr| self.nullability(inner, scope, facts);
        let first = || args.first().map_or(Nullability::Unknown, |arg| of(arg));
        let postgres = self.dialect == SqlDialect::Postgres;

        let fixed = |nullability| match nullability {
            FunctionNullability::NonNull => Some(Nullability::NonNull),
            FunctionNullability::Nullable => Some(Nullability::Nullable),
            FunctionNullability::Aggregate | FunctionNullability::Strict => None,
        };
        let rules = function_rules(&name);
        if rules.iter().any(|rule| fixed(rule.nullability).is_some()) {
            if let Ok(Some((rule, _))) = self.function_rule(&name, &args, scope) {
                if let Some(nullability) = fixed(rule.nullability) {
                    return nullability;
                }
            }
        }
        // `ARRAY(SELECT ...)` over no rows is an empty array.
        if name == "REGR_COUNT"
            || (name == "ARRAY" && matches!(func.args, FunctionArguments::Subquery(_)))
        {
            return Nullability::NonNull;
        }
        // Over no rows MySQL's BIT_AND has every bit set, BIT_OR and BIT_XOR none.
        if self.dialect == SqlDialect::MySql
            && matches!(name.as_str(), "BIT_AND" | "BIT_OR" | "BIT_XOR")
        {
            return Nullability::NonNull;
        }
        // SQLite's `total` is 0.0 over no rows.
        if name == "TOTAL" && self.dialect == SqlDialect::Sqlite {
            return Nullability::NonNull;
        }
        // SQLite's two-argument MIN and MAX are scalar functions.
        if matches!(name.as_str(), "MIN" | "MAX") && args.len() >= 2 {
            return any_nullable(args.iter().map(|arg| of(arg)));
        }
        let behavior = info.map_or(Behavior::Scalar, |info| info.behavior);
        if behavior != Behavior::Scalar {
            // A window function over its frame sees the current row, unless the frame is
            // spelled out.
            if behavior == Behavior::NullOnFewRows {
                return Nullability::Nullable;
            }
            let framed = match &func.over {
                Some(sqlparser::ast::WindowType::WindowSpec(spec)) => spec.window_frame.is_some(),
                Some(_) => true,
                None => false,
            };
            let empty_possible = if func.over.is_some() {
                framed
            } else {
                !facts.grouped
            } || func.filter.is_some();
            let arg = if behavior == Behavior::AlwaysValued {
                Nullability::NonNull
            } else {
                let rows = facts.ungrouped_rows();
                args.first().map_or(Nullability::Unknown, |arg| {
                    self.nullability(arg, scope, &rows)
                })
            };
            return if empty_possible {
                Nullability::Nullable
            } else {
                arg
            };
        }
        // Without `missing_ok`, a setting that is not there is an error; with it, NULL.
        if name == "CURRENT_SETTING" && args.len() == 2 {
            return Nullability::Nullable;
        }
        if info.is_some_and(|info| info.always_nullable(self.dialect)) {
            return Nullability::Nullable;
        }
        match name.as_str() {
            "COALESCE" | "IFNULL" | "NVL" if self.coalesces_checked_group(&args, scope, facts) => {
                Nullability::NonNull
            }
            "COALESCE" | "IFNULL" | "NVL" => all_nullable(args.iter().map(|arg| of(arg))),
            "GREATEST" | "LEAST" if postgres => all_nullable(args.iter().map(|arg| of(arg))),
            "GREATEST" | "LEAST" => any_nullable(args.iter().map(|arg| of(arg))),
            "IF" | "IIF" if args.len() == 3 => any_nullable([of(args[1]), of(args[2])]),
            "LAG" | "LEAD" => match args.as_slice() {
                [value, _, default] => any_nullable([of(value), of(default)]),
                _ => Nullability::Nullable,
            },
            // A frame without the current row can be empty.
            // The remainder by zero is NULL in SQLite and MySQL.
            "MOD" if !postgres && args.get(1).is_some_and(|d| is_nonzero_number(d)) => {
                any_nullable(args.iter().map(|arg| of(arg)))
            }
            "MOD" if !postgres => Nullability::Nullable,
            "FIRST_VALUE" | "LAST_VALUE" if frame_holds_current_row(func.over.as_ref()) => first(),
            "FIRST_VALUE" | "LAST_VALUE" => Nullability::Nullable,
            // PostgreSQL's and SQLite's concat skip NULLs.
            "CONCAT" if self.dialect != SqlDialect::MySql => Nullability::NonNull,
            "CONCAT" => any_nullable(args.iter().map(|arg| of(arg))),
            // Only a NULL separator gives NULL.
            "CONCAT_WS" => first(),
            // PostgreSQL 17's `random(min, max)` is strict.
            "RANDOM" if !args.is_empty() => any_nullable(args.iter().map(|arg| of(arg))),
            _ if info.is_some_and(|info| info.never_null) => Nullability::NonNull,
            _ if info.is_some_and(|info| info.returns(self.dialect).is_some())
                && all_positional =>
            {
                any_nullable(args.iter().map(|arg| of(arg)))
            }
            _ => match self
                .schema
                .function_strict(&table_key(&func.name, self.dialect), args.len())
            {
                Some(true) => any_nullable(args.iter().map(|arg| of(arg))),
                Some(false) => Nullability::Nullable,
                None => Nullability::Unknown,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{analyze_select, load_schema};

    const SCHEMA: &str =
        "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b TEXT NOT NULL, c TEXT);
        CREATE TABLE u (id INTEGER PRIMARY KEY, t_id INTEGER NOT NULL, n INTEGER);";

    /// One mark per result column: `N` nullable, `-` non-null, `?` unknown.
    fn marks(sql: &str) -> String {
        marks_in(sql, "postgres")
    }

    fn marks_in(sql: &str, dialect: &str) -> String {
        marks_with(sql, dialect, SCHEMA)
    }

    fn marks_with(sql: &str, dialect: &str, schema: &str) -> String {
        let schema = Some(load_schema(schema, dialect));
        analyze_select(sql, dialect, schema.as_deref())
            .unwrap()
            .unwrap()
            .iter()
            .map(|col| match col.nullability {
                Nullability::Nullable => 'N',
                Nullability::NonNull => '-',
                Nullability::Unknown => '?',
            })
            .collect()
    }

    #[test]
    fn test_columns_follow_the_schema() {
        assert_eq!(marks("SELECT id, a, b, c FROM t"), "-N-N");
        assert_eq!(marks("SELECT * FROM t"), "-N-N");
        assert_eq!(marks("SELECT t.id, x.c FROM t, t AS x"), "-N");
        assert_eq!(marks("SELECT nope FROM t"), "?");
        assert_eq!(marks("SELECT id FROM missing"), "?");
    }

    #[test]
    fn test_outer_joins_extend_relations() {
        let join = |kind: &str| {
            marks(&format!(
                "SELECT t.id, u.id FROM t {kind} JOIN u ON u.t_id = t.id"
            ))
        };
        assert_eq!(join("INNER"), "--");
        assert_eq!(join("LEFT"), "-N");
        assert_eq!(join("RIGHT"), "N-");
        assert_eq!(join("FULL"), "NN");
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN u ON u.t_id = t.id LEFT JOIN t v ON v.id = u.n"),
            "-NN"
        );
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"),
            "-NN"
        );
        assert_eq!(
            marks("SELECT * FROM t LEFT JOIN u ON u.t_id = t.id"),
            "-N-NNNN"
        );
    }

    #[test]
    fn test_literals_casts_and_operators() {
        assert_eq!(marks("SELECT 1, 'x', true, NULL"), "---N");
        assert_eq!(
            marks("SELECT a::BIGINT, CAST(b AS TEXT), (a) FROM t"),
            "N-N"
        );
        assert_eq!(
            marks("SELECT a + 1, b || 'x', -a, NOT (a > 1) FROM t"),
            "N-NN"
        );
        assert_eq!(
            marks("SELECT a IS NULL, a IS NOT NULL, a IS DISTINCT FROM 1 FROM t"),
            "---"
        );
        assert_eq!(
            marks("SELECT a IN (1, 2), id IN (1, NULL), b LIKE 'x%' FROM t"),
            "NN-"
        );
        assert_eq!(marks("SELECT $1, nosuch(1) FROM t"), "??");
        assert_eq!(marks("SELECT EXISTS (SELECT 1)"), "-");
    }

    #[test]
    fn test_functions() {
        assert_eq!(
            marks("SELECT coalesce(a, 0), coalesce(a, c::INT), coalesce(a, id) FROM t"),
            "-N-"
        );
        assert_eq!(
            marks("SELECT nullif(id, 0), lower(c), lower(b), length(b) FROM t"),
            "NN--"
        );
        assert_eq!(marks("SELECT now(), random(), lower('x') FROM t"), "---");
        assert_eq!(marks("SELECT concat(c, b), concat_ws(',', c) FROM t"), "--");
        assert_eq!(
            marks_in("SELECT concat(c, b), concat_ws(',', c) FROM t", "mysql"),
            "N-"
        );
        assert_eq!(
            marks("SELECT greatest(a, 1), least(a, c::INT) FROM t"),
            "-N"
        );
        assert_eq!(
            marks_in("SELECT greatest(a, 1), ifnull(a, 1) FROM t", "mysql"),
            "N-"
        );
        assert_eq!(
            marks_in("SELECT if(a > 1, b, c), ifnull(c, 'x') FROM t", "mysql"),
            "N-"
        );
        assert_eq!(
            marks_in(
                "SELECT iif(a > 1, b, 'x'), coalesce(c, 'x') FROM t",
                "sqlite"
            ),
            "--"
        );
        assert_eq!(marks("SELECT user_defined(1) FROM t"), "?");
    }

    #[test]
    fn test_division_by_zero_is_null_outside_postgres() {
        let sql = "SELECT b / 2, b / b, id % id, id % 2, mod(id, id), mod(id, 2), id DIV id, id DIV 2, id <=> a FROM u2";
        let schema = "CREATE TABLE u2 (id INT NOT NULL, b INT NOT NULL, a INT);";
        assert_eq!(marks_with(sql, "mysql", schema), "-NN-N-N--");
        assert_eq!(
            marks_with(
                "SELECT b / 2, b / b, id % id, id % 2 FROM u2",
                "sqlite",
                schema
            ),
            "-NN-"
        );
        assert_eq!(
            marks_with(
                "SELECT b / b, id % id, id <=> a FROM u2",
                "postgres",
                schema
            ),
            "--N"
        );
    }

    #[test]
    fn test_functions_that_can_miss() {
        assert_eq!(
            marks("SELECT regexp_match(b, 'a'), regexp_substr(b, 'a'), regexp_replace(b, 'a', 'b'), substring(b FROM 'a'), substring(b FROM 2), substring(b FROM $1 FOR 2), current_setting('x', true), current_setting('x') FROM t"),
            "NN-N-NN-"
        );
    }

    #[test]
    fn test_array_elements_can_be_null() {
        let schema = "CREATE TABLE d (id INT NOT NULL, nums INT[] NOT NULL);";
        let run = |sql: &str| marks_with(sql, "postgres", schema);
        assert_eq!(
            run("SELECT id = ANY (nums), id = ALL (nums), id IN (SELECT unnest(nums)), unnest(nums) FROM d"),
            "NNNN"
        );
        assert_eq!(
            run("SELECT id = ANY (ARRAY[1, 2]), id = ANY (ARRAY[1, NULL]), id < ALL (ARRAY[id]) FROM d"),
            "-N-"
        );
    }

    #[test]
    fn test_json_and_array_accessors_can_be_null() {
        let schema = "CREATE TABLE d (doc JSONB NOT NULL, tags TEXT[] NOT NULL);";
        let run = |sql: &str| marks_with(sql, "postgres", schema);
        assert_eq!(
            run("SELECT doc -> 'k', doc ->> 'k', doc #>> '{a,b}' FROM d"),
            "NNN"
        );
        assert_eq!(
            run("SELECT tags[1], array_length(tags, 1), regexp_match('a', 'b') FROM d"),
            "NNN"
        );
        assert_eq!(run("SELECT doc, tags FROM d"), "--");
    }

    #[test]
    fn test_aggregates() {
        assert_eq!(
            marks("SELECT count(*), count(a), max(id), sum(b::INT), min(a) FROM t"),
            "--NNN"
        );
        assert_eq!(
            marks("SELECT count(*), max(id), avg(id), array_agg(a) FROM t"),
            "-NNN"
        );
        // Every group has a row, so only a nullable argument yields NULL.
        assert_eq!(
            marks("SELECT b, count(*), max(id), max(a), array_agg(a) FROM t GROUP BY b"),
            "---N-"
        );
        // A FILTER can leave a group empty.
        assert_eq!(
            marks("SELECT b, max(id) FILTER (WHERE a > 1) FROM t GROUP BY b"),
            "-N"
        );
        // Percentiles, correlations and sample statistics can be NULL for any group.
        assert_eq!(
            marks("SELECT b, percentile_cont(0.5) WITHIN GROUP (ORDER BY id), percentile_disc(0.5) WITHIN GROUP (ORDER BY id), corr(id, id), covar_samp(id, id), covar_pop(id, id), regr_slope(id, id), stddev(id), var_samp(id), regr_count(id, id), mode() WITHIN GROUP (ORDER BY id) FROM t GROUP BY b"),
            "-NNNNNNNN-N"
        );
        // GROUP BY () is a single group over possibly no rows.
        assert_eq!(marks("SELECT max(id) FROM t GROUP BY ()"), "N");
        assert_eq!(marks("SELECT max(id) FROM t GROUP BY ROLLUP (b)"), "N");
    }

    #[test]
    fn test_window_functions() {
        assert_eq!(
            marks("SELECT row_number() OVER (ORDER BY id), rank() OVER (ORDER BY id), count(a) OVER () FROM t"),
            "---"
        );
        assert_eq!(
            marks("SELECT sum(id) OVER (ORDER BY id), sum(a) OVER (), max(id) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND 1 PRECEDING) FROM t"),
            "-NN"
        );
        assert_eq!(
            marks("SELECT last_value(id) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING), last_value(id) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 2 FOLLOWING), first_value(id) OVER (ROWS BETWEEN 2 PRECEDING AND CURRENT ROW), first_value(id) OVER (ROWS BETWEEN 2 PRECEDING AND 1 PRECEDING), first_value(id) OVER (ROWS 2 PRECEDING), last_value(id) OVER w FROM t WINDOW w AS (ORDER BY id)"),
            "-N-N-N"
        );
        assert_eq!(
            marks("SELECT lag(id) OVER (ORDER BY id), lag(id, 1, 0) OVER (ORDER BY id), lead(a, 1, 0) OVER (ORDER BY id), first_value(id) OVER (ORDER BY id) FROM t"),
            "N-N-"
        );
    }

    #[test]
    fn test_case_without_else_can_be_null() {
        assert_eq!(
            marks("SELECT CASE WHEN a > 1 THEN b END, CASE WHEN a > 1 THEN b ELSE 'x' END, CASE WHEN a > 1 THEN c ELSE 'x' END FROM t"),
            "N-N"
        );
        assert_eq!(
            marks("SELECT CASE a WHEN 1 THEN 'x' ELSE NULL END FROM t"),
            "N"
        );
    }

    #[test]
    fn test_scalar_subqueries() {
        // Zero rows read as NULL.
        assert_eq!(
            marks("SELECT (SELECT b FROM u WHERE u.id = t.id) FROM t"),
            "N"
        );
        assert_eq!(marks("SELECT (SELECT id FROM t LIMIT 1) FROM t"), "N");
        // One row, whose column decides.
        assert_eq!(
            marks("SELECT (SELECT 1), (SELECT NULL), (SELECT count(*) FROM u) FROM t"),
            "-N-"
        );
        assert_eq!(
            marks("SELECT (SELECT max(id) FROM u), (SELECT count(*) + 1 FROM u) FROM t"),
            "N-"
        );
        assert_eq!(
            marks("SELECT (SELECT sum(id) FROM u WHERE u.t_id = t.id) FROM t"),
            "N"
        );
        assert_eq!(
            marks("SELECT id IN (SELECT t_id FROM u), id IN (SELECT n FROM u) FROM t"),
            "-N"
        );
    }
}
