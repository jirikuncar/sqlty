//! Narrowing: what a condition proves about NULLs.
//!
//! A row that survives `WHERE x IS NOT NULL`, or `WHERE x = 1`, or an inner join's
//! `ON a.id = b.a_id`, has a non-null `x`. The same holds inside the conditions
//! themselves: the right side of `a AND b` is read where `a` is true, that of `a OR b`
//! where `a` is false, and a `CASE` branch where its `WHEN` holds and the earlier ones do
//! not.
//!
//! What a condition proves is a [`Learned`] set of columns and expressions. A
//! non-null column of a null-extended relation also proves the relation present, which
//! [`Facts`] reads as no longer null-extended.

use std::collections::HashSet;
use std::rc::Rc;

use sqlparser::ast::{
    BinaryOperator, Expr, Join, JoinConstraint, JoinOperator, Select, TableFactor, TableWithJoins,
    UnaryOperator,
};

use super::functions::{function_name, positional_args};
use super::hashing::{ExprKey, FxHashSet};
use super::nullability::{Facts, Nullability};
use super::query::Analyzer;
use super::registry::lookup;
use super::scope::Scope;

/// Columns and expressions proven not NULL.
#[derive(Clone, Default)]
pub(super) struct Learned {
    /// `(reference, column)` pairs.
    columns: HashSet<(String, String)>,
    /// References whose relation has a row, because a column of it is not NULL.
    present: HashSet<String>,
    /// Expressions such as `doc -> 'k'`.
    exprs: FxHashSet<ExprKey>,
}

impl Learned {
    fn union(mut self, other: Learned) -> Learned {
        self.columns.extend(other.columns);
        self.present.extend(other.present);
        self.exprs.extend(other.exprs);
        self
    }

    /// What both hold: facts that follow from either side of an `OR`.
    fn intersect(self, other: &Learned) -> Learned {
        Learned {
            columns: self
                .columns
                .into_iter()
                .filter(|pair| other.columns.contains(pair))
                .collect(),
            present: self
                .present
                .into_iter()
                .filter(|reference| other.present.contains(reference))
                .collect(),
            exprs: self
                .exprs
                .into_iter()
                .filter(|expr| other.exprs.contains(expr))
                .collect(),
        }
    }
}

impl Facts {
    /// These facts and what a condition proved.
    pub(super) fn with(&self, learned: Learned) -> Facts {
        let mut facts = self.clone();
        if !learned.columns.is_empty() {
            Rc::make_mut(&mut facts.non_null).extend(learned.columns);
        }
        if !learned.present.is_empty() {
            Rc::make_mut(&mut facts.present).extend(learned.present);
        }
        if !learned.exprs.is_empty() {
            Rc::make_mut(&mut facts.non_null_exprs).extend(learned.exprs);
        }
        facts
    }
}

/// The conditions every row of a SELECT satisfies: WHERE, HAVING, and the ON of inner
/// joins. An ON that an outer join null-extends does not count: it is satisfied by the
/// rows that are there, and the extended ones are not. That is the right side of a
/// `LEFT` or `FULL` join, and everything left of a `RIGHT` or `FULL` one.
pub(super) fn conditions(select: &Select) -> Vec<&Expr> {
    fn extends_left(join: &Join) -> bool {
        matches!(
            join.join_operator,
            JoinOperator::Right(_) | JoinOperator::RightOuter(_) | JoinOperator::FullOuter(_)
        )
    }
    fn extends_right(join: &Join) -> bool {
        matches!(
            join.join_operator,
            JoinOperator::Left(_)
                | JoinOperator::LeftOuter(_)
                | JoinOperator::FullOuter(_)
                | JoinOperator::OuterApply
        )
    }
    fn group<'s>(factor: &'s TableFactor, into: &mut Vec<&'s Expr>) {
        if let TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } = factor
        {
            chain(table_with_joins, into);
        }
    }
    fn chain<'s>(table: &'s TableWithJoins, into: &mut Vec<&'s Expr>) {
        // `later[k]`: whether a join after the k-th relation null-extends it.
        let mut later = vec![false; table.joins.len() + 1];
        for (k, join) in table.joins.iter().enumerate() {
            if extends_left(join) {
                later[..=k].iter_mut().for_each(|extended| *extended = true);
            }
        }
        if !later[0] {
            group(&table.relation, into);
        }
        for (k, join) in table.joins.iter().enumerate() {
            if later[k + 1] || extends_right(join) {
                continue;
            }
            group(&join.relation, into);
            if let JoinOperator::Join(JoinConstraint::On(on))
            | JoinOperator::Inner(JoinConstraint::On(on)) = &join.join_operator
            {
                into.push(on);
            }
        }
    }
    let mut found = Vec::new();
    for table in &select.from {
        chain(table, &mut found);
    }
    found.extend(select.selection.iter());
    found.extend(select.having.iter());
    found
}

impl<'a> Analyzer<'a> {
    /// The facts in force inside `select`, once its conditions are applied.
    pub(super) fn narrow_select(&self, select: &Select, scope: &Scope, mut facts: Facts) -> Facts {
        let learned = conditions(select)
            .into_iter()
            .map(|cond| self.learn(cond, true, scope))
            .fold(Learned::default(), Learned::union);
        facts = facts.with(learned);
        facts
    }

    /// What is proven not NULL when `cond` evaluates to `truth`.
    pub(super) fn learn(&self, cond: &Expr, truth: bool, scope: &Scope) -> Learned {
        use BinaryOperator::*;
        match cond {
            Expr::Nested(inner) => self.learn(inner, truth, scope),
            Expr::UnaryOp {
                op: UnaryOperator::Not,
                expr,
            } => self.learn(expr, !truth, scope),
            Expr::BinaryOp {
                left,
                op: And,
                right,
            } => {
                let (l, r) = (
                    self.learn(left, truth, scope),
                    self.learn(right, truth, scope),
                );
                if truth {
                    l.union(r)
                } else {
                    l.intersect(&r)
                }
            }
            Expr::BinaryOp {
                left,
                op: Or,
                right,
            } => {
                let (l, r) = (
                    self.learn(left, truth, scope),
                    self.learn(right, truth, scope),
                );
                if truth {
                    l.intersect(&r)
                } else {
                    l.union(r)
                }
            }
            Expr::IsNull(inner) if !truth => self.proven(inner, scope),
            Expr::IsNotNull(inner) if truth => self.proven(inner, scope),
            Expr::IsTrue(inner) if truth => self.learn(inner, true, scope),
            Expr::IsFalse(inner) if truth => self.learn(inner, false, scope),
            Expr::IsNotTrue(inner) if !truth => self.learn(inner, true, scope),
            Expr::IsNotFalse(inner) if !truth => self.learn(inner, false, scope),
            // `x IS NOT NULL` false, `x IS NULL` true and the like say nothing about `x`.
            Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::IsTrue(_)
            | Expr::IsFalse(_)
            | Expr::IsNotTrue(_)
            | Expr::IsNotFalse(_) => Learned::default(),
            // An empty subquery or array makes `x IN (...)` false and `x NOT IN (...)` true
            // for a NULL `x`: only `x IN (...)` being true proves `x` is there.
            Expr::InSubquery {
                expr: inner,
                negated,
                ..
            }
            | Expr::InUnnest {
                expr: inner,
                negated,
                ..
            } => {
                if truth != *negated {
                    self.proven(inner, scope)
                } else {
                    Learned::default()
                }
            }
            Expr::AnyOp { left, .. } => {
                if truth {
                    self.proven(left, scope)
                } else {
                    Learned::default()
                }
            }
            Expr::AllOp { .. } | Expr::IsNotDistinctFrom(..) | Expr::IsDistinctFrom(..) => {
                Learned::default()
            }
            Expr::Exists { .. } => Learned::default(),
            // A condition that is true, or false, is not NULL, and neither are the operands
            // whose NULL would have made it NULL.
            other => self.proven(other, scope),
        }
    }

    /// What is proven not NULL where `cond` is not true: false, or NULL. Only a condition
    /// that cannot be NULL tells anything, as a row that fails `a > 5` may hold a NULL `a`.
    pub(super) fn learn_not_true(&self, cond: &Expr, scope: &Scope, facts: &Facts) -> Learned {
        if self.nullability(cond, scope, facts) == Nullability::NonNull {
            self.learn(cond, false, scope)
        } else {
            Learned::default()
        }
    }

    /// What is not NULL when `expr` is not NULL.
    fn proven(&self, expr: &Expr, scope: &Scope) -> Learned {
        use BinaryOperator::*;
        let mut learned = Learned::default();
        match expr {
            Expr::Nested(inner) | Expr::Collate { expr: inner, .. } => {
                return self.proven(inner, scope)
            }
            Expr::Cast {
                expr: inner, kind, ..
            } if !matches!(
                kind,
                sqlparser::ast::CastKind::TryCast | sqlparser::ast::CastKind::SafeCast
            ) =>
            {
                return self.proven(inner, scope)
            }
            Expr::UnaryOp { expr: inner, .. } => return self.proven(inner, scope),
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                if let Some((reference, _, column)) = self.locate_column(expr, scope) {
                    learned.present.insert(reference.clone());
                    learned.columns.insert((reference, column));
                }
                return learned;
            }
            Expr::BinaryOp { left, op, right } => match op {
                And | Or | Xor | Spaceship => return learned,
                _ => {
                    learned.exprs.insert(ExprKey::of(expr));
                    return self
                        .proven(left, scope)
                        .union(self.proven(right, scope))
                        .union(learned);
                }
            },
            Expr::InList { expr: inner, .. } => return self.proven(inner, scope),
            // Over an empty set they are false or true, not NULL, whatever the left side is.
            Expr::InSubquery { .. } | Expr::InUnnest { .. } | Expr::AnyOp { .. } => return learned,
            Expr::Between {
                expr: inner,
                low,
                high,
                ..
            } => {
                return self
                    .proven(inner, scope)
                    .union(self.proven(low, scope))
                    .union(self.proven(high, scope))
            }
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
            } => return self.proven(inner, scope).union(self.proven(pattern, scope)),
            Expr::Function(func) => {
                learned.exprs.insert(ExprKey::of(expr));
                if func.over.is_none()
                    && lookup(&function_name(func)).is_some_and(|info| info.is_strict(self.dialect))
                {
                    for arg in positional_args(&func.args) {
                        learned = learned.union(self.proven(arg, scope));
                    }
                }
                return learned;
            }
            _ => {}
        }
        learned.exprs.insert(ExprKey::of(expr));
        learned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{analyze_select, load_schema};

    const SCHEMA: &str =
        "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b INTEGER, c TEXT, doc JSONB NOT NULL);
        CREATE TABLE u (id INTEGER PRIMARY KEY, t_id INTEGER NOT NULL, n INTEGER);";

    /// One mark per result column: `N` nullable, `-` non-null, `?` unknown.
    fn marks(sql: &str) -> String {
        let schema = Some(load_schema(SCHEMA, "postgres"));
        analyze_select(sql, "postgres", schema.as_deref())
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
    fn test_where_narrows_columns() {
        assert_eq!(marks("SELECT a, b FROM t"), "NN");
        assert_eq!(marks("SELECT a, b FROM t WHERE a IS NOT NULL"), "-N");
        assert_eq!(marks("SELECT a, b FROM t WHERE a = 1"), "-N");
        assert_eq!(marks("SELECT a, b FROM t WHERE 1 < a"), "-N");
        assert_eq!(marks("SELECT a, b FROM t WHERE a = b"), "--");
        assert_eq!(marks("SELECT a, b FROM t WHERE a IN (1, 2)"), "-N");
        assert_eq!(marks("SELECT a, b FROM t WHERE a BETWEEN 1 AND 2"), "-N");
        assert_eq!(marks("SELECT a, c FROM t WHERE c LIKE 'x%'"), "N-");
        assert_eq!(
            marks("SELECT a, b FROM t WHERE a > 1 AND b IS NOT NULL"),
            "--"
        );
        assert_eq!(marks("SELECT a, b FROM t WHERE NOT (a IS NULL)"), "-N");
        assert_eq!(marks("SELECT a FROM t WHERE (a IS NOT NULL)"), "-");
        assert_eq!(marks("SELECT a, b FROM t WHERE a IS TRUE"), "-N");
        assert_eq!(marks("SELECT a, b FROM t WHERE a > 1 IS TRUE"), "-N");
        assert_eq!(marks("SELECT * FROM t WHERE a IS NOT NULL"), "--NN-");
    }

    #[test]
    fn test_conditions_that_prove_nothing() {
        assert_eq!(marks("SELECT a FROM t WHERE a IS NULL"), "N");
        assert_eq!(marks("SELECT a FROM t WHERE a IS NOT DISTINCT FROM 1"), "N");
        assert_eq!(marks("SELECT a FROM t WHERE a IS NOT TRUE"), "N");
        assert_eq!(marks("SELECT a FROM t WHERE NOT (a > 1)"), "-");
        assert_eq!(marks("SELECT a FROM t WHERE a = ALL (ARRAY[1])"), "N");
        assert_eq!(marks("SELECT a FROM t WHERE a > 1 OR b > 1"), "N");
        assert_eq!(marks("SELECT a FROM t WHERE a > 1 OR a IS NOT NULL"), "-");
        assert_eq!(marks("SELECT a, b FROM t WHERE COALESCE(a, b) > 1"), "NN");
        assert_eq!(
            marks("SELECT a FROM t WHERE a IS NULL OR a > 1 AND false"),
            "N"
        );
    }

    #[test]
    fn test_narrowing_through_expressions() {
        assert_eq!(marks("SELECT a, b FROM t WHERE a + 1 > 1"), "-N");
        assert_eq!(marks("SELECT a, c FROM t WHERE lower(c) = 'x'"), "N-");
        assert_eq!(marks("SELECT a FROM t WHERE a::BIGINT = 1"), "-");
        assert_eq!(
            marks("SELECT lower(c) FROM t WHERE lower(c) IS NOT NULL"),
            "-"
        );
        // lower(c) is only NULL for a NULL c, so upper(c) is not NULL either.
        assert_eq!(
            marks("SELECT lower(c), upper(c), c FROM t WHERE lower(c) IS NOT NULL"),
            "---"
        );
    }

    #[test]
    fn test_json_key_test_does_not_narrow_the_accessor() {
        // `doc ? 'k'` is also true for an array holding the string 'k', where `doc -> 'k'`
        // is NULL.
        assert_eq!(
            marks("SELECT doc -> 'k', doc ->> 'k', doc -> 'j' FROM t WHERE doc ? 'k'"),
            "NNN"
        );
        assert_eq!(
            marks("SELECT doc -> 'k' FROM t WHERE doc -> 'k' IS NOT NULL"),
            "-"
        );
        assert_eq!(marks("SELECT doc -> 'k' FROM t WHERE NOT doc ? 'k'"), "N");
    }

    #[test]
    fn test_right_side_of_and_and_or() {
        assert_eq!(
            marks("SELECT a IS NOT NULL AND a > 1, a > 1 AND a IS NOT NULL, a IS NULL OR a > 1 FROM t"),
            "-N-"
        );
        assert_eq!(
            marks("SELECT a > 1 AND b > 1, a IS NULL AND b > 1 FROM t"),
            "NN"
        );
    }

    #[test]
    fn test_case_branches_see_their_condition() {
        assert_eq!(
            marks("SELECT CASE WHEN a IS NOT NULL THEN a ELSE 0 END, CASE WHEN a IS NULL THEN 0 ELSE a END FROM t"),
            "--"
        );
        assert_eq!(
            marks("SELECT CASE WHEN a IS NULL THEN a ELSE 0 END, CASE WHEN a > 1 THEN a ELSE 0 END FROM t"),
            "N-"
        );
        assert_eq!(
            marks("SELECT CASE a WHEN 1 THEN a ELSE 0 END, CASE WHEN a IS NULL THEN 1 WHEN b IS NULL THEN 2 ELSE a + b END FROM t"),
            "--"
        );
        assert_eq!(marks("SELECT CASE WHEN a IS NULL THEN 1 END FROM t"), "N");
    }

    #[test]
    fn test_inner_join_conditions_narrow() {
        // A condition on a column of an outer-joined relation drops the extended rows.
        assert_eq!(
            marks(
                "SELECT t.id, u.id, u.n FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.id IS NOT NULL"
            ),
            "--N"
        );
        assert_eq!(
            marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.n > 1"),
            "--"
        );
        assert_eq!(
            marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.id IS NULL"),
            "-N"
        );
        assert_eq!(
            marks(
                "SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE t.id = 1 OR u.id = 1"
            ),
            "-N"
        );
        assert_eq!(
            marks("SELECT t.id, u.id FROM t RIGHT JOIN u ON u.t_id = t.id WHERE t.id = 1"),
            "--"
        );
        assert_eq!(
            marks("SELECT t.id, u.id FROM t FULL JOIN u ON u.t_id = t.id WHERE t.id = 1"),
            "-N"
        );
        // The ON of an outer join filters nothing on the preserved side.
        assert_eq!(
            marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id AND u.n IS NOT NULL"),
            "-N"
        );
        // The ON of an inner join joined later narrows an earlier outer join.
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN u ON u.t_id = t.id JOIN t v ON v.id = u.n"),
            "---"
        );
        assert_eq!(
            marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.id IS NOT NULL AND u.n IS NULL"),
            "--"
        );
        // An inner join's ON holds for the rows of its own side only: a RIGHT JOIN after
        // it null-extends them, and a LEFT JOIN before it null-extends the group.
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t JOIN u ON u.t_id = t.id RIGHT JOIN t v ON v.id = u.n"),
            "NN-"
        );
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"),
            "-NN"
        );
        assert_eq!(
            marks(
                "SELECT t.id, u.id, v.id FROM t JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"
            ),
            "---"
        );
    }

    #[test]
    fn test_narrowing_stays_inside_the_select() {
        // The outer WHERE does not reach into a derived table or a CTE.
        assert_eq!(
            marks("SELECT d.a FROM (SELECT a FROM t) d WHERE d.a IS NOT NULL"),
            "-"
        );
        assert_eq!(
            marks("SELECT a, (SELECT a FROM t t2 WHERE t2.id = t.id) FROM t WHERE a IS NOT NULL"),
            "-N"
        );
        assert_eq!(
            marks("SELECT a, (SELECT max(a) FROM t t2) FROM t WHERE a IS NOT NULL"),
            "-N"
        );
        // A correlated subquery reads the outer row's facts.
        assert_eq!(
            marks("SELECT (SELECT t.a) FROM t WHERE t.a IS NOT NULL"),
            "-"
        );
        assert_eq!(marks("SELECT (SELECT t.a) FROM t"), "N");
    }

    #[test]
    fn test_having_narrows_groups() {
        assert_eq!(
            marks("SELECT a, max(b) FROM t GROUP BY a HAVING a IS NOT NULL AND max(b) IS NOT NULL"),
            "--"
        );
    }
}
