//! `*` over `USING` and `NATURAL` joins, which list one column per merged pair, and over
//! semi and anti joins, which list one side.
//!
//! PostgreSQL puts the merged columns first, in the order of the `USING` list (of the left
//! table for `NATURAL`). MySQL puts them first in the order of the first table, and reads a
//! `RIGHT JOIN` as a `LEFT JOIN` with the tables swapped. SQLite keeps the left table's
//! columns in place and drops the right table's copies.

use rustc_hash::FxHashMap;
use sqlparser::ast::{Join, JoinConstraint, JoinOperator, ObjectName, TableFactor, TableWithJoins};

use crate::catalog::fold;
use crate::MAX_COLUMNS;

use super::dialect::SqlDialect;
use super::nullability::Nullability;
use super::query::{Analyzer, Column};
use super::scope::{join_constraint, relation_name, Scope};

impl Analyzer<'_> {
    /// The columns of `*` over `from`, whose joins merge columns, or `None` when a source's
    /// columns are not known or it joins over [`MAX_JOINED`] tables. Past [`MAX_COLUMNS`],
    /// the rest are left out.
    pub(super) fn merged_star(
        &self,
        from: &[TableWithJoins],
        scope: &Scope,
    ) -> Option<Vec<Column>> {
        if joined(from) > MAX_JOINED {
            return None;
        }
        let mut columns = Vec::new();
        for table in from {
            if columns.len() > MAX_COLUMNS {
                break;
            }
            columns.extend(self.joined_columns(table, from, scope)?);
        }
        Some(columns.into_iter().map(|entry| entry.column).collect())
    }

    fn joined_columns(
        &self,
        table: &TableWithJoins,
        from: &[TableWithJoins],
        scope: &Scope,
    ) -> Option<Vec<Entry>> {
        let mut left = self.factor_columns(&table.relation, from, scope)?;
        for join in &table.joins {
            if left.len() > MAX_COLUMNS {
                break;
            }
            let right = || self.factor_columns(&join.relation, from, scope);
            left = match shape(join, self.dialect) {
                Shape::Both => [left, right()?].concat(),
                Shape::LeftOnly => left,
                Shape::RightOnly => right()?,
                Shape::Merge(side, using) => {
                    let right = right()?;
                    let names: Vec<String> = match using {
                        Some(names) => using_names(names, self.dialect)?,
                        None => {
                            let on_right = positions(&right);
                            left.iter()
                                .filter(|entry| position(&on_right, &entry.column.name).is_some())
                                .map(|entry| entry.column.name.clone())
                                .collect()
                        }
                    };
                    self.merge(&left, &right, &names, side)?
                }
            };
        }
        Some(left)
    }

    fn factor_columns(
        &self,
        factor: &TableFactor,
        from: &[TableWithJoins],
        scope: &Scope,
    ) -> Option<Vec<Entry>> {
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
        let columns = self
            .star_columns(Some(&name), scope)
            .filter(|columns| !columns.iter().any(Column::is_unknown_star))?;
        let declared = Self {
            facts: self.facts.without_joins(),
            ..self.clone()
        }
        .star_columns(Some(&name), scope)?;
        Some(
            columns
                .into_iter()
                .zip(declared)
                .map(|(column, declared)| Entry {
                    column,
                    origin: Some((name.clone(), declared.nullability)),
                })
                .collect(),
        )
    }

    /// `left` and `right` joined on the same-named columns `names`.
    fn merge(
        &self,
        left: &[Entry],
        right: &[Entry],
        names: &[String],
        side: Side,
    ) -> Option<Vec<Entry>> {
        let (on_left, on_right) = (positions(left), positions(right));
        let pairs = names
            .iter()
            .map(|name| Some((position(&on_left, name)?, position(&on_right, name)?)))
            .collect::<Option<Vec<(usize, usize)>>>()?;
        // A row of a full join has one side or both: COALESCE(l, r) is not NULL when neither
        // column is, unless another outer join fills their relation with NULLs too.
        let present = |entry: &Entry| {
            entry.column.nullability == Nullability::NonNull
                || entry.origin.as_ref().is_some_and(|(reference, declared)| {
                    *declared == Nullability::NonNull && self.facts.extensions(reference) == 1
                })
        };
        let merged = |(l, r): (usize, usize)| {
            let (from_left, from_right) = (&left[l], &right[r]);
            let nullability = match side {
                Side::Inner => Nullability::NonNull,
                Side::Left => from_left.column.nullability,
                Side::Right => from_right.column.nullability,
                Side::Full if present(from_left) && present(from_right) => Nullability::NonNull,
                Side::Full => Nullability::Nullable,
            };
            let kept = if side == Side::Right {
                from_right
            } else {
                from_left
            };
            Entry {
                column: Column {
                    nullability,
                    ..kept.column.clone()
                },
                origin: None,
            }
        };
        let rest = |columns: &[Entry], taken: &[usize]| -> Vec<Entry> {
            let mut kept = vec![true; columns.len()];
            for &index in taken {
                kept[index] = false;
            }
            columns
                .iter()
                .zip(kept)
                .filter(|&(_, kept)| kept)
                .map(|(column, _)| column.clone())
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

/// The most tables a merging join is expanded over: SQLite joins at most 64, MySQL 61.
/// Each merges up to [`MAX_COLUMNS`] columns.
const MAX_JOINED: usize = 64;

fn joined(from: &[TableWithJoins]) -> usize {
    let factor = |factor: &TableFactor| match factor {
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => joined(std::slice::from_ref(table_with_joins)),
        _ => 1,
    };
    from.iter()
        .map(|table| {
            factor(&table.relation)
                + table
                    .joins
                    .iter()
                    .map(|join| factor(&join.relation))
                    .sum::<usize>()
        })
        .sum()
}

/// What `*` lists of a join: both sides, both with the `USING` (or, for `None`, `NATURAL`)
/// columns merged, or one side only.
enum Shape<'a> {
    Both,
    Merge(Side, Option<&'a [ObjectName]>),
    LeftOnly,
    RightOnly,
}

/// A semi or anti join lists one side; ClickHouse lists both
/// (`semi_join_include_columns_from_both_sides` is on by default). An `ASOF` join pads the
/// right side with nulls, as a left join does.
fn shape(join: &Join, dialect: SqlDialect) -> Shape<'_> {
    use JoinOperator::*;
    let filters = dialect != SqlDialect::ClickHouse;
    let side = match &join.join_operator {
        Semi(_) | LeftSemi(_) | Anti(_) | LeftAnti(_) if filters => return Shape::LeftOnly,
        RightSemi(_) | RightAnti(_) if filters => return Shape::RightOnly,
        Join(_) | Inner(_) | StraightJoin(_) | CrossJoin(_) => Side::Inner,
        Left(_) | LeftOuter(_) | Semi(_) | LeftSemi(_) | Anti(_) | LeftAnti(_) | AsOf { .. } => {
            Side::Left
        }
        Right(_) | RightOuter(_) | RightSemi(_) | RightAnti(_) => Side::Right,
        FullOuter(_) => Side::Full,
        _ => return Shape::Both,
    };
    match join_constraint(join) {
        Some(JoinConstraint::Using(names)) => Shape::Merge(side, Some(names)),
        Some(JoinConstraint::Natural) => Shape::Merge(side, None),
        _ => Shape::Both,
    }
}

/// A column of a join, with the relation it reads and how that declares it, before any
/// outer join fills it with NULLs; a merged column has none.
#[derive(Clone)]
struct Entry {
    column: Column,
    origin: Option<(String, Nullability)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Inner,
    Left,
    Right,
    Full,
}

/// Where the column `name` is in `columns`, or `None` when it is not there exactly once.
/// Where each name is in `columns`, ignoring ASCII case: `None` for a name listed twice.
fn positions(columns: &[Entry]) -> FxHashMap<String, Option<usize>> {
    let mut positions = FxHashMap::default();
    for (index, entry) in columns.iter().enumerate() {
        positions
            .entry(entry.column.name.to_ascii_lowercase())
            .and_modify(|position| *position = None)
            .or_insert(Some(index));
    }
    positions
}

fn position(positions: &FxHashMap<String, Option<usize>>, name: &str) -> Option<usize> {
    positions.get(&name.to_ascii_lowercase()).copied().flatten()
}

fn using_names(names: &[ObjectName], dialect: SqlDialect) -> Option<Vec<String>> {
    names
        .iter()
        .map(|name| Some(fold(name.0.last()?.as_ident()?, dialect)))
        .collect()
}
