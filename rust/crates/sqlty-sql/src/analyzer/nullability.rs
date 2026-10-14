//! Nullability: whether a result column can be NULL.
//!
//! Nullability is a field next to a column's type (see [`Column::nullability`]), never part
//! of the type string. It is three-valued: a column is [`Nullability::Nullable`] only where
//! the schema or the query's shape proves NULL can occur. Columns of an unresolved source and
//! expressions the analyzer does not read are [`Nullability::Unknown`], which row types treat
//! as non-optional.

use super::*;

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
#[derive(Clone, Default)]
pub(super) struct Facts {
    /// References of relations an outer join null-extends: all their columns can be NULL.
    null_extended: Vec<String>,
}

impl Facts {
    /// Whether the relation `reference` can be a row of NULLs.
    pub(super) fn is_null_extended(&self, reference: &str) -> bool {
        self.null_extended.iter().any(|r| r == reference)
    }

    /// Nullability of a column of `reference`, given what its source declares.
    pub(super) fn column(
        &self,
        reference: &str,
        _column: &str,
        declared: Nullability,
    ) -> Nullability {
        if self.is_null_extended(reference) {
            Nullability::Nullable
        } else {
            declared
        }
    }

    /// Forget what is known about relations a nested SELECT defines itself.
    pub(super) fn without(&mut self, scope: &Scope) {
        self.null_extended
            .retain(|r| !scope.iter().any(|(s, _)| s == r));
    }
}

/// References of the relations in `from` that outer joins null-extend, nested joins included.
pub(super) fn null_extended(from: &[TableWithJoins]) -> Vec<String> {
    let mut extended = Vec::new();
    for table in from {
        group(table, &mut extended);
    }
    extended
}

/// References of the relations of one `a JOIN b JOIN c` chain; null-extended ones go to `extended`.
fn group(table: &TableWithJoins, extended: &mut Vec<String>) -> Vec<String> {
    let mut left = factor_references(&table.relation, extended);
    for join in &table.joins {
        let right = factor_references(&join.relation, extended);
        match &join.join_operator {
            JoinOperator::Left(_) | JoinOperator::LeftOuter(_) | JoinOperator::OuterApply => {
                extended.extend(right.iter().cloned())
            }
            JoinOperator::Right(_) | JoinOperator::RightOuter(_) => {
                extended.extend(left.iter().cloned())
            }
            JoinOperator::FullOuter(_) => {
                extended.extend(left.iter().cloned());
                extended.extend(right.iter().cloned());
            }
            _ => {}
        }
        left.extend(right);
    }
    left
}

fn factor_references(factor: &TableFactor, extended: &mut Vec<String>) -> Vec<String> {
    match factor {
        TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } => group(table_with_joins, extended),
        TableFactor::Table { name, alias, .. } => vec![alias
            .as_ref()
            .map_or_else(|| object_name(name), |a| a.name.value.to_lowercase())],
        other => vec![opaque_source(other).0],
    }
}

impl<'a> Analyzer<'a> {
    /// The facts in force inside `select`: those of enclosing queries, minus the names it
    /// redefines, plus its own joins.
    pub(super) fn select_facts(&self, select: &Select, scope: &Scope) -> Facts {
        let mut facts = self.facts.clone();
        facts.without(scope);
        facts.null_extended.extend(null_extended(&select.from));
        facts
    }

    /// Whether `expr` can be NULL under `facts`.
    pub(super) fn nullability(&self, expr: &Expr, scope: &Scope, facts: &Facts) -> Nullability {
        match expr {
            Expr::Nested(inner) => self.nullability(inner, scope, facts),
            Expr::Cast { expr: inner, .. } => self.nullability(inner, scope, facts),
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                match self.locate_column(expr, scope) {
                    Some((reference, table, column)) => facts.column(
                        &reference,
                        &column,
                        self.declared_nullability(&table, &column),
                    ),
                    None => Nullability::Unknown,
                }
            }
            _ => Nullability::Unknown,
        }
    }
}
