//! `foreign-key-mismatch`: a foreign key column compared with a column other than itself
//! or the one it references.
//!
//! `orders.customer_id REFERENCES customers (id)` may be compared with `customers.id`,
//! with `orders.customer_id` of another row, or with a column referencing `customers.id`
//! too. The columns of a composite key are compared with their referenced columns of one
//! row, all of them.

use std::ops::ControlFlow;

use sqlparser::ast::{
    BinaryOperator, Expr, Ident, InTarget, Query, Select, SelectItem, SetExpr, Spanned,
    TableFactor, Visit, Visitor,
};
use sqlparser::tokenizer::Span;

use super::fanout::{conjuncts, join_conditions, strip};
use super::query::Analyzer;
use super::scope::{relation_name, Scope};
use crate::catalog::ForeignKey;
use crate::diagnostics::{Diagnostics, NameRule};

/// A schema table column a comparison reads.
#[derive(Clone)]
struct Column<'s> {
    /// The row it is read from: its source's reference, behind the `IN` subquery it is
    /// read in.
    row: String,
    /// Its source's reference, for messages.
    reference: String,
    /// The catalog key of its table.
    table: &'s str,
    name: String,
    /// The foreign keys its table declares.
    keys: &'s [ForeignKey],
}

impl Column<'_> {
    fn qualified(&self) -> String {
        format!("{}.{}", short(self.table), self.name)
    }

    /// The `(table key, column)` each foreign key of the column references it to.
    fn targets(&self) -> Vec<(&str, &str)> {
        self.keys
            .iter()
            .filter_map(|fk| {
                let at = fk.columns.iter().position(|c| *c == self.name)?;
                Some((fk.table.as_str(), fk.referred.get(at)?.as_str()))
            })
            .collect()
    }
}

/// A column located in its scope, before its table's keys are looked up.
struct Located {
    row: String,
    reference: String,
    table: String,
    name: String,
    foreign_key: bool,
}

/// Two columns compared, and where.
struct Pair<'s> {
    left: Column<'s>,
    right: Column<'s>,
    span: Span,
}

/// Comparisons that hold together: the terms of one `AND`.
type Group<'s> = Vec<Pair<'s>>;

impl<'s> Analyzer<'s> {
    /// Records `foreign-key-mismatch` for each comparison of `select` in `ON`, `USING`,
    /// `WHERE` and `HAVING` between a foreign key column and a column it does not
    /// reference, and for each composite key compared with its referenced columns only in
    /// part or across rows.
    pub(super) fn check_foreign_keys(&self, select: &Select, scope: &Scope) {
        if !self.diag.enabled() || !self.judges_names() {
            return;
        }
        let mut conditions = Vec::new();
        let mut usings = Vec::new();
        join_conditions(&select.from, &mut conditions, &mut |using, right| {
            usings.push((using.to_vec(), right.clone()));
        });
        conditions.extend(&select.selection);
        conditions.extend(&select.having);
        let mut reader = Reader {
            analyzer: self,
            scope,
            subqueries: 0,
            groups: Vec::new(),
        };
        let mut all = Group::new();
        for (using, right) in &usings {
            self.using_comparisons(using, right, scope, &mut all);
        }
        for condition in conditions {
            reader.conjunction(condition, &mut all);
        }
        reader.groups.push(all);
        for group in &reader.groups {
            self.judge(group);
        }
    }

    fn using_comparisons(
        &self,
        using: &[Ident],
        right: &TableFactor,
        scope: &Scope,
        into: &mut Group<'s>,
    ) {
        let Some(at) = relation_name(right)
            .and_then(|name| scope.iter().position(|source| source.reference == name))
        else {
            return;
        };
        let qualified = |reference: &str, ident: &Ident| {
            Expr::CompoundIdentifier(vec![Ident::new(reference), ident.clone()])
        };
        for ident in using {
            let right = self.locate(&qualified(&scope[at].reference, ident), scope, "");
            let left = scope[..at]
                .iter()
                .find_map(|source| self.locate(&qualified(&source.reference, ident), scope, ""));
            if let (Some(left), Some(right)) = (left, right) {
                into.extend(self.pair(left, right, ident.span));
            }
        }
    }

    /// The schema table column `expr` reads in `scope`, its row behind `subquery`.
    fn locate(&self, expr: &Expr, scope: &Scope, subquery: &str) -> Option<Located> {
        let expr = strip(expr);
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return None;
        }
        let (reference, table, name, foreign_key) = self.locate_table_column(expr, scope)?;
        let inner = scope.iter().any(|source| source.reference == reference);
        Some(Located {
            row: if inner {
                format!("{subquery}{reference}")
            } else {
                reference.clone()
            },
            reference,
            table,
            name,
            foreign_key,
        })
    }

    /// `left` compared with `right`, when either is part of a foreign key.
    fn pair(&self, left: Located, right: Located, span: Span) -> Option<Pair<'s>> {
        if !left.foreign_key && !right.foreign_key {
            return None;
        }
        let column = |located: Located| {
            let (table, keys) = self.schema.declared_foreign_keys(&located.table)?;
            Some(Column {
                row: located.row,
                reference: located.reference,
                table,
                name: located.name,
                keys,
            })
        };
        Some(Pair {
            left: column(left)?,
            right: column(right)?,
            span,
        })
    }

    fn judge(&self, group: &Group<'s>) {
        for pair in group {
            self.judge_pair(pair);
        }
        self.judge_composites(group);
    }

    /// A foreign key column may be compared with itself, the column it references, or a
    /// column referencing that one too.
    fn judge_pair(&self, pair: &Pair) {
        let (left, right) = (&pair.left, &pair.right);
        let (to_left, to_right) = (left.targets(), right.targets());
        let is = |column: &Column, (table, name): (&str, &str)| {
            column.table == table && column.name == name
        };
        let allowed = (left.table == right.table && left.name == right.name)
            || to_left.iter().any(|&target| is(right, target))
            || to_right.iter().any(|&target| is(left, target))
            || to_left.iter().any(|target| to_right.contains(target));
        if allowed || to_left.is_empty() && to_right.is_empty() {
            return;
        }
        let (column, targets, other) = if to_left.is_empty() {
            (right, to_right, left)
        } else {
            (left, to_left, right)
        };
        let targets: Vec<String> = targets
            .iter()
            .map(|(table, name)| format!("{}.{name}", short(table)))
            .collect();
        let _at = self.diag.at(pair.span);
        self.diag.record(
            NameRule::ForeignKeyMismatch,
            format!(
                "{} references {}, not {}",
                column.qualified(),
                targets.join(" or "),
                other.qualified()
            ),
        );
    }

    /// Each composite key of a row compared with its referenced columns: all of them, of
    /// one row.
    fn judge_composites(&self, group: &Group<'s>) {
        let mut matches: Vec<Match> = Vec::new();
        for pair in group {
            for (column, other) in [(&pair.left, &pair.right), (&pair.right, &pair.left)] {
                for fk in column.keys {
                    let Some(at) = fk.columns.iter().position(|c| *c == column.name) else {
                        continue;
                    };
                    if fk.columns.len() < 2
                        || other.table != fk.table
                        || fk.referred.get(at) != Some(&other.name)
                    {
                        continue;
                    }
                    let found = matches
                        .iter()
                        .position(|m| m.row == column.row && std::ptr::eq(m.fk, fk));
                    let at_match = found.unwrap_or_else(|| {
                        matches.push(Match {
                            row: column.row.clone(),
                            table: column.table,
                            fk,
                            columns: Vec::new(),
                            span: pair.span,
                        });
                        matches.len() - 1
                    });
                    matches[at_match].columns.push((at, other.clone()));
                }
            }
        }
        let mut partial = Vec::new();
        for m in &matches {
            let rows = m.rows();
            if rows.len() > 1 {
                let compared: Vec<String> = m
                    .columns
                    .iter()
                    .map(|(_, other)| format!("{}.{}", other.reference, other.name))
                    .collect();
                let _at = self.diag.at(m.span);
                self.diag.record(
                    NameRule::ForeignKeyMismatch,
                    format!(
                        "{} {} references one {} row, compared with {}",
                        short(m.table),
                        list(&m.fk.columns),
                        short(&m.fk.table),
                        compared.join(" and ")
                    ),
                );
            } else if m.covered() < m.fk.columns.len() {
                partial.push(m);
            }
        }
        let mut reported = vec![false; partial.len()];
        for (i, m) in partial.iter().enumerate() {
            if reported[i] {
                continue;
            }
            let mixed: Vec<usize> = (i..partial.len())
                .filter(|&j| {
                    let other = partial[j];
                    other.row == m.row && other.fk.table == m.fk.table && other.rows() == m.rows()
                })
                .collect();
            let _at = self.diag.at(m.span);
            if mixed.len() > 1 {
                let keys: Vec<String> = mixed
                    .iter()
                    .map(|&j| list(&partial[j].fk.columns))
                    .collect();
                let compared: Vec<String> = mixed
                    .iter()
                    .flat_map(|&j| {
                        let m = partial[j];
                        m.columns.iter().map(move |(at, other)| {
                            format!("{} = {}.{}", m.fk.columns[*at], other.reference, other.name)
                        })
                    })
                    .collect();
                self.diag.record(
                    NameRule::ForeignKeyMismatch,
                    format!(
                        "{} {} mixed: {}",
                        short(m.table),
                        keys.join(" and "),
                        compared.join(", ")
                    ),
                );
                for j in mixed {
                    reported[j] = true;
                }
                continue;
            }
            let mut compared: Vec<&str> = m
                .columns
                .iter()
                .map(|(at, _)| m.fk.columns[*at].as_str())
                .collect();
            compared.sort_unstable();
            compared.dedup();
            self.diag.record(
                NameRule::ForeignKeyMismatch,
                format!(
                    "{} {} references {} {}, compared with {} on {} only",
                    short(m.table),
                    list(&m.fk.columns),
                    short(&m.fk.table),
                    list(&m.fk.referred),
                    m.columns[0].1.reference,
                    compared.join(", ")
                ),
            );
        }
    }
}

/// The columns of a row's composite key compared with their referenced columns.
struct Match<'s> {
    row: String,
    table: &'s str,
    fk: &'s ForeignKey,
    /// The position in the key of each column compared, and what it is compared with.
    columns: Vec<(usize, Column<'s>)>,
    span: Span,
}

impl Match<'_> {
    fn rows(&self) -> Vec<&str> {
        let mut rows: Vec<&str> = self.columns.iter().map(|(_, c)| c.row.as_str()).collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    fn covered(&self) -> usize {
        let mut at: Vec<usize> = self.columns.iter().map(|(at, _)| *at).collect();
        at.sort_unstable();
        at.dedup();
        at.len()
    }
}

/// A table's name without its schema.
fn short(table: &str) -> &str {
    table.rsplit('.').next().unwrap_or(table)
}

fn list(columns: &[String]) -> String {
    format!("({})", columns.join(", "))
}

/// Reads the comparisons of conditions into groups of terms that hold together.
struct Reader<'a, 's> {
    analyzer: &'a Analyzer<'s>,
    scope: &'a Scope,
    subqueries: usize,
    /// The groups of `OR` branches and `IN` list items, each an alternative.
    groups: Vec<Group<'s>>,
}

impl<'s> Reader<'_, 's> {
    fn conjunction(&mut self, condition: &Expr, group: &mut Group<'s>) {
        for term in conjuncts(condition) {
            if let Expr::BinaryOp {
                left,
                op: BinaryOperator::Or,
                right,
            } = term
            {
                for branch in [left, right] {
                    let mut alternative = Group::new();
                    self.conjunction(branch, &mut alternative);
                    self.groups.push(alternative);
                }
            } else if !self.comparison(term, group) {
                let mut nested = Nested {
                    reader: self,
                    queries: 0,
                };
                let _ = term.visit(&mut nested);
            }
        }
    }

    /// Reads `expr` into `group` when it is a comparison.
    fn comparison(&mut self, expr: &Expr, group: &mut Group<'s>) -> bool {
        let _at = self.analyzer.diag.at(expr.span());
        match expr {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq | BinaryOperator::NotEq,
                right,
            } => self.compare(left, right, expr.span(), group),
            Expr::In {
                expr: left,
                target: InTarget::List(list),
                ..
            } => {
                for item in list {
                    let mut alternative = Group::new();
                    self.compare(left, item, expr.span(), &mut alternative);
                    self.groups.push(alternative);
                }
            }
            Expr::In {
                expr: left,
                target: InTarget::Subquery(subquery),
                ..
            } => self.subquery(left, subquery, expr.span(), group),
            _ => return false,
        }
        true
    }

    fn compare(&self, left: &Expr, right: &Expr, span: Span, group: &mut Group<'s>) {
        match (strip(left), strip(right)) {
            (Expr::Tuple(lefts), Expr::Tuple(rights)) if lefts.len() == rights.len() => {
                for (left, right) in lefts.iter().zip(rights) {
                    self.compare(left, right, span, group);
                }
                return;
            }
            (Expr::Tuple(_), _) | (_, Expr::Tuple(_)) => return,
            _ => {}
        }
        let (analyzer, scope) = (self.analyzer, self.scope);
        if let (Some(left), Some(right)) = (
            analyzer.locate(left, scope, ""),
            analyzer.locate(right, scope, ""),
        ) {
            group.extend(analyzer.pair(left, right, span));
        }
    }

    /// `expr IN (SELECT ...)`: each column of `expr` with the subquery's column at its
    /// position, read in the subquery's scope.
    fn subquery(&mut self, expr: &Expr, subquery: &Query, span: Span, group: &mut Group<'s>) {
        let SetExpr::Select(inner) = subquery.body.as_ref() else {
            return;
        };
        let Some(selected) = inner
            .projection
            .iter()
            .map(|item| match item {
                SelectItem::UnnamedExpr(column)
                | SelectItem::ExprWithAlias { expr: column, .. } => Some(column),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let outer: Vec<&Expr> = match strip(expr) {
            Expr::Tuple(columns) => columns.iter().collect(),
            column => vec![column],
        };
        if outer.len() != selected.len() {
            return;
        }
        let mut quiet = self.analyzer.correlated(self.scope);
        quiet.diag = Diagnostics::default();
        let Ok((analyzer, inner_scope)) = quiet.scope_of(&inner.from);
        self.subqueries += 1;
        let row = format!("{}#", self.subqueries);
        for (left, right) in outer.into_iter().zip(selected) {
            if let (Some(left), Some(right)) = (
                self.analyzer.locate(left, self.scope, ""),
                analyzer.locate(right, &inner_scope, &row),
            ) {
                group.extend(self.analyzer.pair(left, right, span));
            }
        }
    }
}

/// Visits a term that is no comparison, reading each comparison in it, outside subqueries,
/// as a group of its own.
struct Nested<'r, 'a, 's> {
    reader: &'r mut Reader<'a, 's>,
    queries: usize,
}

impl Visitor for Nested<'_, '_, '_> {
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
        if self.queries == 0 {
            let mut group = Group::new();
            if self.reader.comparison(expr, &mut group) {
                self.reader.groups.push(group);
            }
        }
        ControlFlow::Continue(())
    }
}
