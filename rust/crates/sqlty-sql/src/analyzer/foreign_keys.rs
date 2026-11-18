//! `foreign-key-mismatch`: a foreign key column compared with a column other than itself
//! or the one it references.
//!
//! `orders.customer_id REFERENCES customers (id)` may be compared with `customers.id`,
//! with `orders.customer_id` of another row, or with a column holding the same keys: one
//! referencing `customers.id` too, or reaching it through a chain of foreign keys. The
//! columns of a composite key are compared with their referenced columns of one row, all
//! of them.

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
    keys: Vec<&'s ForeignKey>,
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
#[derive(Clone)]
struct Pair<'s> {
    left: Column<'s>,
    right: Column<'s>,
    span: Span,
}

/// Comparisons that hold together: the terms of one `AND`.
#[derive(Default)]
struct Group<'s> {
    pairs: Vec<Pair<'s>>,
    /// The pairs before it come from the enclosing `AND`, judged one by one there.
    seeded: usize,
    /// `(row, column)` of each column equal to a constant or parameter.
    pinned: Vec<(String, String)>,
    /// `(row, column)` of each column `IS NULL`.
    nulls: Vec<(String, String)>,
    /// Split into alternatives (`OR` branches, `IN` list items), each judging the keys
    /// with this group's terms.
    split: bool,
}

impl<'s> Group<'s> {
    /// An alternative of this group: its terms, and more.
    fn alternative(&self) -> Group<'s> {
        Group {
            pairs: self.pairs.clone(),
            seeded: self.pairs.len(),
            pinned: self.pinned.clone(),
            nulls: self.nulls.clone(),
            split: false,
        }
    }

    fn pins(&self, row: &str, column: &str) -> bool {
        self.pinned.iter().any(|(r, c)| r == row && c == column)
    }
}

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
        let mut all = Group::default();
        for (using, right) in &usings {
            self.using_comparisons(using, right, scope, &mut all);
        }
        let terms = conditions.into_iter().flat_map(conjuncts).collect();
        reader.conjunction(terms, &mut all);
        reader.groups.push(all);
        for group in &reader.groups {
            for pair in &group.pairs[group.seeded..] {
                self.judge_pair(pair);
            }
            if !group.split {
                self.judge_composites(group);
            }
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
                into.pairs.extend(self.pair(left, right, ident.span));
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

    /// A foreign key column may be compared with itself, the column it references, or a
    /// column holding the same keys.
    fn judge_pair(&self, pair: &Pair) {
        let (left, right) = (&pair.left, &pair.right);
        let (to_left, to_right) = (left.targets(), right.targets());
        let is = |column: &Column, (table, name): (&str, &str)| {
            column.table == table && column.name == name
        };
        let entity = |column: &Column| self.schema.entity(column.table, &column.name);
        let allowed = (left.table == right.table && left.name == right.name)
            || to_left.iter().any(|&target| is(right, target))
            || to_right.iter().any(|&target| is(left, target))
            || to_left.iter().any(|target| to_right.contains(target))
            || entity(left).is_some_and(|root| Some(root) == entity(right));
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
    /// one row. A key column and its referenced column both equal to a constant or
    /// parameter count as compared.
    fn judge_composites(&self, group: &Group<'s>) {
        let mut matches: Vec<Match> = Vec::new();
        for (index, pair) in group.pairs.iter().enumerate() {
            for (column, other) in [(&pair.left, &pair.right), (&pair.right, &pair.left)] {
                for &fk in &column.keys {
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
                    matches[at_match].columns.push((at, other.clone(), index));
                }
            }
        }
        let partial: Vec<&Match> = matches
            .iter()
            .filter(|m| !self.across_rows(m) && !m.complete(group))
            .collect();
        // `p.tree = n.tree` of a self-referencing key reads both ways: the other way is
        // part of a complete match.
        let partial: Vec<&Match> = partial
            .into_iter()
            .filter(|m| {
                !matches.iter().any(|whole| {
                    !std::ptr::eq(*m, whole)
                        && whole.complete(group)
                        && m.columns
                            .iter()
                            .all(|(_, _, index)| whole.columns.iter().any(|(_, _, i)| i == index))
                })
            })
            .collect();
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
                        m.columns.iter().map(move |(at, other, _)| {
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
                .map(|(at, _, _)| m.fk.columns[*at].as_str())
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

    /// Records a composite key compared with columns of more than one row.
    fn across_rows(&self, m: &Match) -> bool {
        if m.rows().len() < 2 {
            return false;
        }
        let compared: Vec<String> = m
            .columns
            .iter()
            .map(|(_, other, _)| format!("{}.{}", other.reference, other.name))
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
        true
    }
}

/// The columns of a row's composite key compared with their referenced columns.
struct Match<'s> {
    row: String,
    table: &'s str,
    fk: &'s ForeignKey,
    /// The position in the key of each column compared, what it is compared with, and
    /// the index of the pair comparing them.
    columns: Vec<(usize, Column<'s>, usize)>,
    span: Span,
}

impl Match<'_> {
    fn rows(&self) -> Vec<&str> {
        let mut rows: Vec<&str> = self
            .columns
            .iter()
            .map(|(_, c, _)| c.row.as_str())
            .collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// Whether every column of the key is compared, or pinned with its referenced column,
    /// or one is NULL, so the key references no row.
    fn complete(&self, group: &Group) -> bool {
        let other = self.columns[0].1.row.as_str();
        let null = |column: &String| {
            group
                .nulls
                .iter()
                .any(|(r, c)| *r == self.row && c == column)
        };
        self.fk.columns.iter().any(null)
            || (0..self.fk.columns.len()).all(|at| {
                self.columns.iter().any(|(i, _, _)| *i == at)
                    || group.pins(&self.row, &self.fk.columns[at])
                        && self
                            .fk
                            .referred
                            .get(at)
                            .is_some_and(|referred| group.pins(other, referred))
            })
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
    /// Reads the terms of one `AND` into `group`, then each `OR` branch and `IN` list item
    /// into an alternative of it.
    fn conjunction(&mut self, terms: Vec<&Expr>, group: &mut Group<'s>) {
        let (mut branches, mut lists) = (Vec::new(), Vec::new());
        for term in terms {
            match term {
                Expr::BinaryOp {
                    left,
                    op: BinaryOperator::Or,
                    right,
                } => branches.extend([left, right]),
                Expr::In {
                    expr,
                    target: InTarget::List(list),
                    ..
                } => lists.push((expr, list, term.span())),
                _ if self.comparison(term, group) => {}
                _ => {
                    let mut nested = Nested {
                        reader: self,
                        queries: 0,
                    };
                    let _ = term.visit(&mut nested);
                }
            }
        }
        group.split |= !branches.is_empty() || !lists.is_empty();
        for branch in branches {
            let _at = self.analyzer.diag.at(branch.span());
            let mut alternative = group.alternative();
            self.conjunction(conjuncts(branch), &mut alternative);
            self.groups.push(alternative);
        }
        for (expr, list, span) in lists {
            let _at = self.analyzer.diag.at(span);
            self.in_list(expr, list, span, group);
        }
    }

    /// Reads `expr` into `group` when it is a comparison.
    fn comparison(&mut self, expr: &Expr, group: &mut Group<'s>) -> bool {
        let _at = self.analyzer.diag.at(expr.span());
        match expr {
            Expr::BinaryOp {
                left,
                op: op @ (BinaryOperator::Eq | BinaryOperator::NotEq),
                right,
            } => self.compare(left, right, *op == BinaryOperator::Eq, expr.span(), group),
            Expr::In {
                expr: left,
                target: InTarget::List(list),
                ..
            } => self.in_list(left, list, expr.span(), group),
            Expr::In {
                expr: left,
                target: InTarget::Subquery(subquery),
                ..
            } => self.subquery(left, subquery, expr.span(), group),
            Expr::IsNull(column) => {
                if let Some(column) = self.analyzer.locate(column, self.scope, "") {
                    group.nulls.push((column.row, column.name));
                }
            }
            _ => return false,
        }
        true
    }

    fn in_list(&mut self, left: &Expr, list: &[Expr], span: Span, group: &Group<'s>) {
        for item in list {
            let mut alternative = group.alternative();
            self.compare(left, item, false, span, &mut alternative);
            self.groups.push(alternative);
        }
    }

    /// Reads `left = right` (`pin`) or `left <> right` into `group`, row values by position.
    fn compare(&self, left: &Expr, right: &Expr, pin: bool, span: Span, group: &mut Group<'s>) {
        match (strip(left), strip(right)) {
            (Expr::Tuple(lefts), Expr::Tuple(rights)) if lefts.len() == rights.len() => {
                for (left, right) in lefts.iter().zip(rights) {
                    self.compare(left, right, pin, span, group);
                }
                return;
            }
            (Expr::Tuple(_), _) | (_, Expr::Tuple(_)) => return,
            _ => {}
        }
        let (analyzer, scope) = (self.analyzer, self.scope);
        match (
            analyzer.locate(left, scope, ""),
            analyzer.locate(right, scope, ""),
        ) {
            (Some(left), Some(right)) => group.pairs.extend(analyzer.pair(left, right, span)),
            (Some(column), None) if pin && is_value(right) => {
                group.pinned.push((column.row, column.name));
            }
            (None, Some(column)) if pin && is_value(left) => {
                group.pinned.push((column.row, column.name));
            }
            _ => {}
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
                group.pairs.extend(self.analyzer.pair(left, right, span));
            }
        }
    }
}

/// A constant or parameter.
fn is_value(expr: &Expr) -> bool {
    matches!(strip(expr), Expr::Value(_))
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
            let mut group = Group::default();
            if self.reader.comparison(expr, &mut group) {
                self.reader.groups.push(group);
            }
        }
        ControlFlow::Continue(())
    }
}
