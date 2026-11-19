//! `foreign-key-mismatch`: a foreign key column compared with a column other than itself
//! or the one it references.
//!
//! `orders.customer_id REFERENCES customers (id)` may be compared with `customers.id`,
//! with `orders.customer_id` of another row, or with a column holding the same keys: one
//! referencing `customers.id` too, or reaching it through a chain of foreign keys. The
//! columns of a composite key are compared with their referenced columns of one row, all
//! of them.
//!
//! The comparisons read here are judged by `entity-mismatch` too (see `entities.rs`).

use std::collections::HashMap;
use std::ops::ControlFlow;

use sqlparser::ast::{
    BinaryOperator, Expr, Ident, InTarget, Query, Select, SelectItem, SetExpr, Spanned,
    TableFactor, Value, ValueWithSpan, Visit, Visitor,
};
use sqlparser::tokenizer::Span;

use super::entities::Side;
use super::fanout::{conjuncts, join_conditions, strip};
use super::query::Analyzer;
use super::scope::{relation_name, Scope};
use crate::catalog::ForeignKey;
use crate::diagnostics::{Diagnostics, Rule};

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
    /// Read from an enclosing query, which may compare the rest of a key.
    outer: bool,
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
    outer: bool,
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

/// A comparison of two schema table columns: the foreign keys and the entities it reads.
#[derive(Clone)]
struct Compared<'s> {
    keys: Option<Pair<'s>>,
    entities: Option<(Side, Side)>,
    span: Span,
}

/// Comparisons that hold together: the terms of one `AND`.
#[derive(Default)]
struct Group<'s> {
    pairs: Vec<Compared<'s>>,
    /// The pairs before it come from the enclosing `AND`, judged one by one there.
    seeded: usize,
    /// `(row, column, value)` of each column equal to a constant or parameter.
    pinned: Vec<(String, String, String)>,
    /// `(row, column)` of each column `IS NULL`.
    nulls: Vec<(String, String)>,
    /// `(row, column)` pairs of each column `=` another, keys or not, subqueries' included.
    equal: Vec<[(String, String); 2]>,
    /// The rows each term that is no comparison reads: `lower(a.code) = lower(b.code)`.
    unread: Vec<Vec<String>>,
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
            equal: self.equal.clone(),
            unread: self.unread.clone(),
            split: false,
        }
    }

    /// The columns and values `=` holds equal, each named by its class.
    fn classes(&self) -> Classes {
        let mut classes = Classes::default();
        for [a, b] in &self.equal {
            classes.union(
                &Node::Column(a.0.clone(), a.1.clone()),
                &Node::Column(b.0.clone(), b.1.clone()),
            );
        }
        for (row, column, value) in &self.pinned {
            classes.union(
                &Node::Column(row.clone(), column.clone()),
                &Node::Value(value.clone()),
            );
        }
        classes
    }
}

/// A column of a row, or a constant or parameter.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Node {
    Column(String, String),
    Value(String),
}

/// Equality classes of [`Node`]s, a union-find.
#[derive(Default)]
struct Classes {
    parent: HashMap<Node, Node>,
}

impl Classes {
    fn find(&self, node: &Node) -> Node {
        let mut node = node.clone();
        while let Some(parent) = self.parent.get(&node) {
            node = parent.clone();
        }
        node
    }

    fn union(&mut self, a: &Node, b: &Node) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent.insert(a, b);
        }
    }

    /// Whether `column` of `row` equals `other` of `other_row`.
    fn equal(&self, (row, column): (&str, &str), (other_row, other): (&str, &str)) -> bool {
        let node = |row: &str, column: &str| Node::Column(row.to_string(), column.to_string());
        self.find(&node(row, column)) == self.find(&node(other_row, other))
    }
}

impl<'s> Analyzer<'s> {
    /// Records, for the comparisons of `select` in `ON`, `USING`, `WHERE` and `HAVING`,
    /// `foreign-key-mismatch` for a foreign key column compared with a column it does not
    /// reference and for a composite key compared only in part or across rows, and
    /// `entity-mismatch` for columns holding keys of different entities, shown only where
    /// `foreign-key-mismatch` does not report that comparison.
    pub(super) fn check_comparisons(&self, select: &Select, scope: &Scope) {
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
        let projected: Vec<(String, String)> = select
            .projection
            .iter()
            .filter_map(|item| match item {
                SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                    let column = self.locate(expr, scope, "")?;
                    Some((column.row, column.name))
                }
                _ => None,
            })
            .collect();
        let mut all = Group::default();
        for (using, right) in &usings {
            self.using_comparisons(using, right, scope, &mut all);
        }
        let terms = conditions.into_iter().flat_map(conjuncts).collect();
        reader.conjunction(terms, &mut all);
        reader.groups.push(all);
        for group in &reader.groups {
            for compared in &group.pairs[group.seeded..] {
                let keys = compared
                    .keys
                    .as_ref()
                    .is_some_and(|pair| self.judge_pair(pair));
                if let Some((a, b)) = &compared.entities {
                    let _at = self.diag.at(compared.span);
                    self.report_entities(a, b, keys);
                }
            }
            if !group.split {
                self.judge_composites(group, &projected);
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
                .rev()
                .map(|source| qualified(&source.reference, ident))
                .find(|column| self.resolves(column, scope))
                .and_then(|column| self.locate(&column, scope, ""));
            if let (Some(left), Some(right)) = (left, right) {
                into.pairs.extend(self.compared(left, right, ident.span));
            }
        }
    }

    /// The schema table column `expr` reads in `scope`, its row behind `subquery`.
    fn locate(&self, expr: &Expr, scope: &Scope, subquery: &str) -> Option<Located> {
        let expr = match strip(expr) {
            Expr::Cast { expr, .. } => strip(expr),
            expr => expr,
        };
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return None;
        }
        let (reference, table, name, foreign_key) = self.locate_table_column(expr, scope)?;
        let inner = scope.iter().any(|source| source.reference == reference);
        Some(Located {
            outer: !inner,
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

    /// `left` compared with `right`, when either holds keys.
    fn compared(&self, left: Located, right: Located, span: Span) -> Option<Compared<'s>> {
        let entities = self
            .entity_side(&left.table, &left.name)
            .zip(self.entity_side(&right.table, &right.name));
        let keys = self.pair(left, right, span);
        (keys.is_some() || entities.is_some()).then_some(Compared {
            keys,
            entities,
            span,
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
                outer: located.outer,
            })
        };
        Some(Pair {
            left: column(left)?,
            right: column(right)?,
            span,
        })
    }

    /// A foreign key column may be compared with itself, the column it references, or a
    /// column holding the same keys. Returns whether it reports the pair.
    fn judge_pair(&self, pair: &Pair) -> bool {
        let (left, right) = (&pair.left, &pair.right);
        let (to_left, to_right) = (left.targets(), right.targets());
        let is = |column: &Column, (table, name): (&str, &str)| {
            column.table == table && column.name == name
        };
        let entity = |column: &Column| self.schema.entity(column.table, &column.name);
        // A column of the same name that is no key, of a log or audit table say.
        let unkeyed = |column: &Column, targets: &[(&str, &str)]| {
            targets.is_empty() && entity(column).is_none()
        };
        let allowed = (left.table == right.table && left.name == right.name)
            || to_left.iter().any(|&target| is(right, target))
            || to_right.iter().any(|&target| is(left, target))
            || to_left.iter().any(|target| to_right.contains(target))
            || entity(left).is_some_and(|root| Some(root) == entity(right))
            || left.name == right.name && (unkeyed(left, &to_left) || unkeyed(right, &to_right));
        if allowed || to_left.is_empty() && to_right.is_empty() {
            return false;
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
            Rule::ForeignKeyMismatch,
            format!(
                "{} references {}, not {}",
                column.qualified(),
                targets.join(" or "),
                other.qualified()
            ),
        );
        true
    }

    /// Each composite key of a row compared with its referenced columns: all of them, of
    /// one row. A key column and its referenced column both equal to a constant or
    /// parameter count as compared.
    fn judge_composites(&self, group: &Group<'s>, projected: &[(String, String)]) {
        let mut matches: Vec<Match> = Vec::new();
        let pairs = group.pairs.iter().enumerate();
        for (index, pair) in pairs.filter_map(|(i, c)| Some((i, c.keys.as_ref()?))) {
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
                            siblings: column.keys.clone(),
                            outer: false,
                            fk,
                            columns: Vec::new(),
                            span: pair.span,
                        });
                        matches.len() - 1
                    });
                    matches[at_match].outer |= column.outer || other.outer;
                    matches[at_match].columns.push((at, other.clone(), index));
                }
            }
        }
        let classes = group.classes();
        let complete: Vec<bool> = matches
            .iter()
            .map(|m| m.complete(group, &classes))
            .collect();
        let partial: Vec<&Match> =
            matches
                .iter()
                .zip(&complete)
                .filter(|(m, complete)| !self.across_rows(m) && !**complete)
                .map(|(m, _)| m)
                .filter(|m| {
                    let other = m.columns[0].1.row.as_str();
                    // `p.tree = n.tree` of a self-referencing key reads both ways: the other
                    // way is part of a complete match.
                    let within = matches.iter().zip(&complete).any(|(whole, complete)| {
                        !std::ptr::eq(*m, whole)
                            && *complete
                            && (m.columns.iter().all(|(_, _, index)| {
                                whole.columns.iter().any(|(_, _, i)| i == index)
                            }) || whole.columns[0].1.row == other)
                    });
                    // `a.tenant_id = b.tenant_id`, which every key of the tenant shares, a
                    // row of an enclosing query, which may compare the rest, or a term read as
                    // no comparison relating both rows.
                    !within
                        && !m.shared()
                        && !(m.outer && m.selects_rest(projected))
                        && !group.unread.iter().any(|rows| {
                            rows.contains(&m.row) && rows.iter().any(|row| row == other)
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
                    Rule::ForeignKeyMismatch,
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
                Rule::ForeignKeyMismatch,
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
            Rule::ForeignKeyMismatch,
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
    /// The foreign keys of `table`, `fk` among them.
    siblings: Vec<&'s ForeignKey>,
    /// A row of an enclosing query is compared, which may compare the rest of the key.
    outer: bool,
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

    /// Whether every column of the key is compared with its referenced column, or equal
    /// to it through other columns, constants or parameters, or one is NULL, so the key
    /// references no row.
    fn complete(&self, group: &Group, classes: &Classes) -> bool {
        let other = self.columns[0].1.row.as_str();
        let null = |column: &String| {
            group
                .nulls
                .iter()
                .any(|(r, c)| *r == self.row && c == column)
        };
        self.fk.columns.iter().any(null)
            || self.fk.columns.iter().enumerate().all(|(at, column)| {
                self.columns.iter().any(|(i, _, _)| *i == at)
                    || self.fk.referred.get(at).is_some_and(|referred| {
                        classes.equal((&self.row, column), (other, referred))
                    })
            })
    }

    /// Whether the query selects the referenced columns not compared, as `IN (SELECT ...)`
    /// compares them with the rest of the key of an enclosing query's row.
    fn selects_rest(&self, projected: &[(String, String)]) -> bool {
        let other = &self.columns[0].1.row;
        self.fk.referred.iter().enumerate().all(|(at, referred)| {
            self.columns.iter().any(|(i, _, _)| *i == at)
                || projected
                    .iter()
                    .any(|(row, column)| row == other && column == referred)
        })
    }

    /// Whether each column compared is part of another foreign key of the table too, as a
    /// tenant column every key of a tenant shares.
    fn shared(&self) -> bool {
        self.columns.iter().all(|(at, _, _)| {
            let column = &self.fk.columns[*at];
            self.siblings
                .iter()
                .any(|fk| !std::ptr::eq(*fk, self.fk) && fk.columns.contains(column))
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
                } if list.len() != 1 => lists.push((expr, list, term.span())),
                _ if self.comparison(term, group) => {}
                _ => {
                    group.unread.push(self.rows(&[term]));
                    let _ = term.visit(&mut Nested {
                        reader: self,
                        queries: 0,
                    });
                }
            }
        }
        group.split |= !branches.is_empty() || lists.iter().any(|(_, list, _)| !list.is_empty());
        for branch in branches {
            let _at = self.analyzer.diag.at(branch.span());
            let mut alternative = group.alternative();
            self.conjunction(conjuncts(branch), &mut alternative);
            self.groups.push(alternative);
        }
        for (expr, list, span) in lists {
            let _at = self.analyzer.diag.at(span);
            self.in_list(expr, list, group);
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
            Expr::IsNotDistinctFrom(left, right) => {
                self.compare(left, right, true, expr.span(), group);
            }
            Expr::IsDistinctFrom(left, right) => {
                self.compare(left, right, false, expr.span(), group);
            }
            Expr::In {
                expr: left,
                target: InTarget::List(list),
                negated,
                ..
            } => match list.as_slice() {
                [item] => self.compare(left, item, !negated, expr.span(), group),
                _ => self.in_list(left, list, group),
            },
            Expr::In {
                expr: left,
                target: InTarget::Subquery(subquery),
                ..
            } => self.subquery(left, subquery, group),
            Expr::IsNull(column) => {
                if let Some(column) = self.analyzer.locate(column, self.scope, "") {
                    group.nulls.push((column.row, column.name));
                }
            }
            _ => return false,
        }
        true
    }

    fn in_list(&mut self, left: &Expr, list: &[Expr], group: &Group<'s>) {
        for item in list {
            let mut alternative = group.alternative();
            let span = left.span().union(&item.span());
            self.compare(left, item, false, span, &mut alternative);
            self.groups.push(alternative);
        }
    }

    /// Reads `left = right` (`pin`) or `left <> right` into `group`, row values by position.
    fn compare(&self, left: &Expr, right: &Expr, pin: bool, span: Span, group: &mut Group<'s>) {
        match (strip(left), strip(right)) {
            (Expr::Tuple(lefts), Expr::Tuple(rights)) if lefts.len() == rights.len() => {
                for (left, right) in lefts.iter().zip(rights) {
                    let span = left.span().union(&right.span());
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
            (Some(left), Some(right)) => {
                if pin {
                    group.equal.push([
                        (left.row.clone(), left.name.clone()),
                        (right.row.clone(), right.name.clone()),
                    ]);
                }
                group.pairs.extend(analyzer.compared(left, right, span));
            }
            (a, b) => match constant_of(a, right).or_else(|| constant_of(b, left)) {
                Some((column, value)) => group
                    .pinned
                    .extend(pinned(pin, value).map(|v| (column.row, column.name, v))),
                // `lower(w.code) = lower(s.code)`
                None => group.unread.push(self.rows(&[left, right])),
            },
        }
    }

    /// The rows the columns of `exprs` are read from, outside subqueries.
    fn rows(&self, exprs: &[&Expr]) -> Vec<String> {
        let mut rows = Rows {
            reader: self,
            queries: 0,
            rows: Vec::new(),
        };
        for expr in exprs {
            let _ = expr.visit(&mut rows);
        }
        rows.rows
    }

    /// `expr IN (SELECT ...)`: each column of `expr` with the subquery's column at its
    /// position, read in the subquery's scope.
    fn subquery(&mut self, expr: &Expr, subquery: &Query, group: &mut Group<'s>) {
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
            let span = left.span().union(&right.span());
            if let (Some(left), Some(right)) = (
                self.analyzer.locate(left, self.scope, ""),
                analyzer.locate(right, &inner_scope, &row),
            ) {
                group
                    .pairs
                    .extend(self.analyzer.compared(left, right, span));
            }
        }
        // `WHERE u.tenant_id = t.tenant_id` or `= $1` of the subquery ties its row to the
        // outer one; it is judged where the subquery is analyzed.
        for term in inner.selection.iter().flat_map(conjuncts) {
            let (Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            }
            | Expr::IsNotDistinctFrom(left, right)) = term
            else {
                continue;
            };
            let located = |expr: &Expr| analyzer.locate(expr, &inner_scope, &row);
            match (located(left), located(right)) {
                (Some(a), Some(b)) => group.equal.push([(a.row, a.name), (b.row, b.name)]),
                (a, b) => group.pinned.extend(
                    constant_of(a, right)
                        .or_else(|| constant_of(b, left))
                        .and_then(|(column, value)| {
                            Some((column.row, column.name, pinned(true, value)?))
                        }),
                ),
            }
        }
    }
}

/// `column` compared with `expr`, when `expr` is a constant or parameter.
fn constant_of(column: Option<Located>, expr: &Expr) -> Option<(Located, &ValueWithSpan)> {
    match strip(expr) {
        Expr::Value(value) => Some((column?, value)),
        _ => None,
    }
}

/// The value a column `=` (`pin`) a constant or parameter holds. A `?` or `%s` is a
/// different parameter each time it is written.
fn pinned(pin: bool, value: &ValueWithSpan) -> Option<String> {
    match &value.value {
        Value::Null => None,
        Value::Placeholder(p) if p == "?" || p == "%s" => None,
        _ => pin.then(|| value.to_string()),
    }
}

/// Visits a term that is no comparison, reading each comparison in it, outside subqueries,
/// as a group of its own. Subqueries are judged where their columns are read.
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
            // Its rows need not hold for the query's rows: `CASE`, `NOT`, `COALESCE`.
            let mut group = Group {
                split: true,
                ..Group::default()
            };
            if self.reader.comparison(expr, &mut group) {
                self.reader.groups.push(group);
            }
        }
        ControlFlow::Continue(())
    }
}

/// Collects the rows of the columns an expression reads, outside subqueries.
struct Rows<'r, 'a, 's> {
    reader: &'r Reader<'a, 's>,
    queries: usize,
    rows: Vec<String>,
}

impl Visitor for Rows<'_, '_, '_> {
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
            if let Expr::Identifier(_) | Expr::CompoundIdentifier(_) = expr {
                let (analyzer, scope) = (self.reader.analyzer, self.reader.scope);
                self.rows
                    .extend(analyzer.locate(expr, scope, "").map(|column| column.row));
            }
        }
        ControlFlow::Continue(())
    }
}
