//! `*` over `USING` and `NATURAL` joins, which list one column per merged pair.
//!
//! PostgreSQL puts the merged columns first, in the order of the `USING` list (of the left
//! table for `NATURAL`). MySQL puts them first in the order of the first table, and reads a
//! `RIGHT JOIN` as a `LEFT JOIN` with the tables swapped. SQLite keeps the left table's
//! columns in place and drops the right table's copies.

use sqlparser::ast::{JoinConstraint, JoinOperator, TableFactor, TableWithJoins};

use crate::catalog::fold;

use super::dialect::SqlDialect;
use super::nullability::Nullability;
use super::query::{Analyzer, Column};
use super::scope::{join_constraint, relation_name, Scope};

impl Analyzer<'_> {
    /// The columns of `*` over `from`, whose joins merge columns, or `None` when a source's
    /// columns are not known.
    pub(super) fn merged_star(
        &self,
        from: &[TableWithJoins],
        scope: &Scope,
    ) -> Option<Vec<Column>> {
        let mut columns = Vec::new();
        for table in from {
            columns.extend(self.joined_columns(table, from, scope)?);
        }
        Some(columns)
    }

    fn joined_columns(
        &self,
        table: &TableWithJoins,
        from: &[TableWithJoins],
        scope: &Scope,
    ) -> Option<Vec<Column>> {
        let mut left = self.factor_columns(&table.relation, from, scope)?;
        for join in &table.joins {
            let right = self.factor_columns(&join.relation, from, scope)?;
            let names: Vec<String> = match join_constraint(join) {
                Some(JoinConstraint::Using(names)) => names
                    .iter()
                    .map(|name| Some(fold(name.0.last()?.as_ident()?, self.dialect)))
                    .collect::<Option<_>>()?,
                Some(JoinConstraint::Natural) => left
                    .iter()
                    .filter(|column| position(&right, &column.name).is_some())
                    .map(|column| column.name.clone())
                    .collect(),
                _ => {
                    left.extend(right);
                    continue;
                }
            };
            left = self.merge(&left, &right, &names, &join.join_operator)?;
        }
        Some(left)
    }

    fn factor_columns(
        &self,
        factor: &TableFactor,
        from: &[TableWithJoins],
        scope: &Scope,
    ) -> Option<Vec<Column>> {
        if let TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } = factor
        {
            return self.joined_columns(table_with_joins, from, scope);
        }
        let name = relation_name(factor)?;
        if !self.star_resolvable(from, Some(&name)) {
            return None;
        }
        self.star_columns(Some(&name), scope)
            .filter(|columns| !columns.iter().any(Column::is_unknown_star))
    }

    /// `left` and `right` joined on the same-named columns `names`.
    fn merge(
        &self,
        left: &[Column],
        right: &[Column],
        names: &[String],
        operator: &JoinOperator,
    ) -> Option<Vec<Column>> {
        let side = match operator {
            JoinOperator::Join(_) | JoinOperator::Inner(_) | JoinOperator::StraightJoin(_) => {
                Side::Inner
            }
            JoinOperator::Left(_) | JoinOperator::LeftOuter(_) => Side::Left,
            JoinOperator::Right(_) | JoinOperator::RightOuter(_) => Side::Right,
            JoinOperator::FullOuter(_) => Side::Full,
            _ => return None,
        };
        let pairs = names
            .iter()
            .map(|name| Some((position(left, name)?, position(right, name)?)))
            .collect::<Option<Vec<(usize, usize)>>>()?;
        let merged = |(l, r): (usize, usize)| {
            let (from_left, from_right) = (&left[l], &right[r]);
            let nullability = match side {
                Side::Inner => Nullability::NonNull,
                Side::Left => from_left.nullability,
                Side::Right => from_right.nullability,
                Side::Full
                    if from_left.nullability == Nullability::NonNull
                        && from_right.nullability == Nullability::NonNull =>
                {
                    Nullability::NonNull
                }
                Side::Full => Nullability::Nullable,
            };
            let column = if side == Side::Right {
                from_right
            } else {
                from_left
            };
            Column {
                nullability,
                ..column.clone()
            }
        };
        let rest = |columns: &[Column], taken: &[usize]| -> Vec<Column> {
            (0..columns.len())
                .filter(|index| !taken.contains(index))
                .map(|index| columns[index].clone())
                .collect()
        };
        let (on_left, on_right): (Vec<usize>, Vec<usize>) = pairs.iter().copied().unzip();
        Some(match self.dialect {
            SqlDialect::Sqlite => {
                let mut columns = left.to_vec();
                for &pair in &pairs {
                    columns[pair.0] = merged(pair);
                }
                columns.extend(rest(right, &on_right));
                columns
            }
            _ => {
                let swap = self.dialect == SqlDialect::MySql && side == Side::Right;
                let mut ordered = pairs;
                if self.dialect == SqlDialect::MySql {
                    ordered.sort_by_key(|&(l, r)| if swap { r } else { l });
                }
                let (first, second) = if swap {
                    (rest(right, &on_right), rest(left, &on_left))
                } else {
                    (rest(left, &on_left), rest(right, &on_right))
                };
                ordered
                    .into_iter()
                    .map(merged)
                    .chain(first)
                    .chain(second)
                    .collect()
            }
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Inner,
    Left,
    Right,
    Full,
}

/// Where the column `name` is in `columns`, or `None` when it is not there exactly once.
fn position(columns: &[Column], name: &str) -> Option<usize> {
    let mut matches =
        (0..columns.len()).filter(|&index| columns[index].name.eq_ignore_ascii_case(name));
    matches.next().filter(|_| matches.next().is_none())
}
