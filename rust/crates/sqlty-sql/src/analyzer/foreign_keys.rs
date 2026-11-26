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

use std::borrow::Cow;
use std::cell::LazyCell;
use std::ops::ControlFlow;

use rustc_hash::FxHashMap;

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

/// A name a comparison reads, borrowed from the query where it can be.
type Name<'n> = Cow<'n, str>;

/// A schema table column a comparison reads: names of the query (`'n`), and of the
/// catalog (`'s`).
#[derive(Clone)]
struct Column<'s, 'n> {
    /// The row it is read from: its source's reference, behind the `IN` subquery it is
    /// read in.
    row: Name<'n>,
    /// Its source's reference, for messages.
    reference: Name<'n>,
    /// The catalog key of its table.
    table: &'s str,
    name: &'s str,
    /// The foreign keys its table declares.
    keys: Vec<&'s ForeignKey>,
    /// Read from an enclosing query, which may compare the rest of a key.
    outer: bool,
}

impl<'s> Column<'s, '_> {
    fn qualified(&self) -> String {
        format!("{}.{}", short(self.table), self.name)
    }

    /// The `(table key, column)` each foreign key of the column references it to.
    fn targets(&self) -> impl Iterator<Item = (&'s str, &'s str)> + Clone + '_ {
        self.keys.iter().filter_map(|fk| {
            let at = fk.columns.iter().position(|c| *c == self.name)?;
            Some((fk.table.as_str(), fk.referred.get(at)?.as_str()))
        })
    }

    fn into_owned(self) -> Column<'s, 'static> {
        Column {
            row: owned(self.row),
            reference: owned(self.reference),
            table: self.table,
            name: self.name,
            keys: self.keys,
            outer: self.outer,
        }
    }
}

fn owned(name: Name<'_>) -> Name<'static> {
    Cow::Owned(name.into_owned())
}

/// A column located in its scope, before its table's keys are looked up.
struct Located<'s, 'n> {
    outer: bool,
    row: Name<'n>,
    reference: Name<'n>,
    table: &'s str,
    name: &'s str,
    foreign_key: bool,
}

impl<'s> Located<'s, '_> {
    fn into_owned(self) -> Located<'s, 'static> {
        Located {
            outer: self.outer,
            row: owned(self.row),
            reference: owned(self.reference),
            table: self.table,
            name: self.name,
            foreign_key: self.foreign_key,
        }
    }
}

/// Two columns compared, and where.
#[derive(Clone)]
struct Pair<'s, 'n> {
    left: Column<'s, 'n>,
    right: Column<'s, 'n>,
    span: Span,
}

/// A comparison of two schema table columns: the foreign keys and the entities it reads.
#[derive(Clone)]
struct Compared<'s, 'n> {
    keys: Option<Pair<'s, 'n>>,
    entities: Option<(Side<'s>, Side<'s>)>,
    span: Span,
}

impl<'s> Compared<'s, '_> {
    fn into_owned(self) -> Compared<'s, 'static> {
        Compared {
            keys: self.keys.map(|pair| Pair {
                left: pair.left.into_owned(),
                right: pair.right.into_owned(),
                span: pair.span,
            }),
            entities: self.entities,
            span: self.span,
        }
    }
}

/// A column of a row.
type RowColumn<'n> = (Name<'n>, Name<'n>);

/// Comparisons that hold together: the terms of one `AND`.
#[derive(Default)]
struct Group<'s, 'r> {
    pairs: Vec<Compared<'s, 'r>>,
    /// The pairs before it come from the enclosing `AND`, judged one by one there.
    seeded: usize,
    /// `(row, column, value)` of each column equal to a constant or parameter.
    pinned: Vec<(Name<'r>, Name<'r>, String)>,
    /// `(row, column)` of each column `IS NULL`.
    nulls: Vec<RowColumn<'r>>,
    /// `(row, column)` pairs of each column `=` another, keys or not, subqueries' included.
    equal: Vec<[RowColumn<'r>; 2]>,
    /// The rows each term that is no comparison reads: `lower(a.code) = lower(b.code)`.
    unread: Vec<Vec<Name<'r>>>,
    /// Split into alternatives (`OR` branches, `IN` list items), each judging the keys
    /// with this group's terms.
    split: bool,
}

impl<'s, 'r> Group<'s, 'r> {
    /// An alternative of this group: its terms, and more.
    fn alternative(&self) -> Group<'s, 'r> {
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

    /// This group, owning what it borrowed from the expressions it read.
    fn into_owned(self) -> Group<'s, 'static> {
        let pair = |(row, column): RowColumn<'_>| (owned(row), owned(column));
        Group {
            pairs: self.pairs.into_iter().map(Compared::into_owned).collect(),
            seeded: self.seeded,
            pinned: self
                .pinned
                .into_iter()
                .map(|(row, column, value)| (owned(row), owned(column), value))
                .collect(),
            nulls: self.nulls.into_iter().map(pair).collect(),
            equal: self
                .equal
                .into_iter()
                .map(|[a, b]| [pair(a), pair(b)])
                .collect(),
            unread: self
                .unread
                .into_iter()
                .map(|rows| rows.into_iter().map(owned).collect())
                .collect(),
            split: self.split,
        }
    }

    /// The columns and values `=` holds equal, each named by its class.
    fn classes(&self) -> Classes<'_> {
        let mut classes = Classes::default();
        for [a, b] in &self.equal {
            classes.union(Node::Column(&a.0, &a.1), Node::Column(&b.0, &b.1));
        }
        for (row, column, value) in &self.pinned {
            classes.union(Node::Column(row, column), Node::Value(value));
        }
        classes
    }
}

/// A column of a row, or a constant or parameter.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<'g> {
    Column(&'g str, &'g str),
    Value(&'g str),
}

/// Equality classes of [`Node`]s, a union-find.
#[derive(Default)]
struct Classes<'g> {
    parent: FxHashMap<Node<'g>, Node<'g>>,
}

impl<'g> Classes<'g> {
    fn root(&self, mut node: Node<'g>) -> Node<'g> {
        while let Some(&parent) = self.parent.get(&node) {
            node = parent;
        }
        node
    }

    fn union(&mut self, a: Node<'g>, b: Node<'g>) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.parent.insert(a, b);
        }
    }

    /// Whether `column` of `row` equals `other` of `other_row`.
    fn equal(&self, (row, column): (&str, &str), (other_row, other): (&str, &str)) -> bool {
        fn find<'x>(parent: &FxHashMap<Node<'x>, Node<'x>>, mut node: Node<'x>) -> Node<'x> {
            while let Some(&next) = parent.get(&node) {
                node = next;
            }
            node
        }
        find(&self.parent, Node::Column(row, column))
            == find(&self.parent, Node::Column(other_row, other))
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
        let projected = LazyCell::new(|| {
            select
                .projection
                .iter()
                .filter_map(|item| match item {
                    SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                        let column = self.locate(expr, scope, "")?;
                        Some((column.row, Cow::Borrowed(column.name)))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
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

    fn using_comparisons<'r>(
        &'r self,
        using: &[Ident],
        right: &TableFactor,
        scope: &'r Scope,
        into: &mut Group<'s, 'r>,
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
            let right = self
                .locate(&qualified(&scope[at].reference, ident), scope, "")
                .map(Located::into_owned);
            let left = scope[..at]
                .iter()
                .rev()
                .map(|source| qualified(&source.reference, ident))
                .find(|column| self.resolves(column, scope))
                .and_then(|column| self.locate(&column, scope, "").map(Located::into_owned));
            if let (Some(left), Some(right)) = (left, right) {
                into.pairs.extend(self.compared(left, right, ident.span));
            }
        }
    }

    /// The schema table column `expr` reads in `scope`, its row behind `subquery`.
    fn locate<'r>(
        &'r self,
        expr: &'r Expr,
        scope: &'r Scope,
        subquery: &str,
    ) -> Option<Located<'s, 'r>> {
        let expr = match strip(expr) {
            Expr::Cast { expr, .. } => strip(expr),
            expr => expr,
        };
        if !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
            return None;
        }
        let (found, column) = self.locate_table_column(expr, scope)?;
        let table = self.schema.catalog().table_id(&found.table)?;
        let inner = scope
            .iter()
            .any(|source| source.reference == *found.reference);
        Some(Located {
            outer: !inner,
            row: if inner && !subquery.is_empty() {
                Cow::Owned(format!("{subquery}{}", found.reference))
            } else {
                found.reference.clone()
            },
            reference: found.reference,
            table,
            name: &column.name,
            foreign_key: column.foreign_key,
        })
    }

    /// `left` compared with `right`, when either holds keys.
    fn compared<'n>(
        &self,
        left: Located<'s, 'n>,
        right: Located<'s, 'n>,
        span: Span,
    ) -> Option<Compared<'s, 'n>> {
        let entities = self
            .entity_side(left.table, left.name)
            .zip(self.entity_side(right.table, right.name));
        let keys = self.pair(left, right, span);
        (keys.is_some() || entities.is_some()).then_some(Compared {
            keys,
            entities,
            span,
        })
    }

    /// `left` compared with `right`, when either is part of a foreign key.
    fn pair<'n>(
        &self,
        left: Located<'s, 'n>,
        right: Located<'s, 'n>,
        span: Span,
    ) -> Option<Pair<'s, 'n>> {
        if !left.foreign_key && !right.foreign_key {
            return None;
        }
        let column = |located: Located<'s, 'n>| {
            let (table, keys) = self.schema.catalog().declared_foreign_keys(located.table)?;
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
    fn judge_pair(&self, pair: &Pair<'s, '_>) -> bool {
        let (left, right) = (&pair.left, &pair.right);
        let (to_left, to_right) = (left.targets(), right.targets());
        let (left_empty, right_empty) = (
            to_left.clone().next().is_none(),
            to_right.clone().next().is_none(),
        );
        let is = |column: &Column, (table, name): (&str, &str)| {
            column.table == table && column.name == name
        };
        let catalog = self.schema.catalog();
        let entity = |column: &Column<'s, '_>| catalog.entity(column.table, column.name);
        // A column of the same name that is no key, of a log or audit table say.
        let unkeyed = |column: &Column<'s, '_>, empty: bool| empty && entity(column).is_none();
        let allowed = (left.table == right.table && left.name == right.name)
            || to_left.clone().any(|target| is(right, target))
            || to_right.clone().any(|target| is(left, target))
            || to_left
                .clone()
                .any(|target| to_right.clone().any(|other| other == target))
            || entity(left).is_some_and(|root| Some(root) == entity(right))
            || left.name == right.name
                && (unkeyed(left, left_empty) || unkeyed(right, right_empty));
        if allowed || left_empty && right_empty {
            return false;
        }
        let (column, targets, other) = if left_empty {
            (right, to_right, left)
        } else {
            (left, to_left, right)
        };
        let targets: Vec<String> = targets
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
    fn judge_composites<'r>(
        &self,
        group: &Group<'s, 'r>,
        projected: &LazyCell<Vec<RowColumn<'r>>, impl FnOnce() -> Vec<RowColumn<'r>>>,
    ) {
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
                        || fk.referred.get(at).map(String::as_str) != Some(other.name)
                    {
                        continue;
                    }
                    let found = matches
                        .iter()
                        .position(|m| *m.row == *column.row && std::ptr::eq(m.fk, fk));
                    let at_match = found.unwrap_or_else(|| {
                        matches.push(Match {
                            row: column.row.clone(),
                            table: column.table,
                            siblings: &column.keys,
                            outer: false,
                            fk,
                            columns: Vec::new(),
                            span: pair.span,
                        });
                        matches.len() - 1
                    });
                    matches[at_match].outer |= column.outer || other.outer;
                    matches[at_match].columns.push((at, other, index));
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
                    let other = &*m.columns[0].1.row;
                    // `p.tree = n.tree` of a self-referencing key reads both ways: the other
                    // way is part of a complete match.
                    let within = matches.iter().zip(&complete).any(|(whole, complete)| {
                        !std::ptr::eq(*m, whole)
                            && *complete
                            && (m.columns.iter().all(|(_, _, index)| {
                                whole.columns.iter().any(|(_, _, i)| i == index)
                            }) || *whole.columns[0].1.row == *other)
                    });
                    // `a.tenant_id = b.tenant_id`, which every key of the tenant shares, a
                    // row of an enclosing query, which may compare the rest, or a term read as
                    // no comparison relating both rows.
                    !within
                        && !m.shared()
                        && !(m.outer && m.selects_rest(projected))
                        && !group.unread.iter().any(|rows| {
                            rows.contains(&m.row) && rows.iter().any(|row| *row == *other)
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
struct Match<'s, 'n, 'g> {
    row: Name<'n>,
    table: &'s str,
    /// The foreign keys of `table`, `fk` among them.
    siblings: &'g [&'s ForeignKey],
    /// A row of an enclosing query is compared, which may compare the rest of the key.
    outer: bool,
    fk: &'s ForeignKey,
    /// The position in the key of each column compared, what it is compared with, and
    /// the index of the pair comparing them.
    columns: Vec<(usize, &'g Column<'s, 'n>, usize)>,
    span: Span,
}

impl Match<'_, '_, '_> {
    fn rows(&self) -> Vec<&str> {
        let mut rows: Vec<&str> = self.columns.iter().map(|(_, c, _)| &*c.row).collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// Whether every column of the key is compared with its referenced column, or equal
    /// to it through other columns, constants or parameters, or one is NULL, so the key
    /// references no row.
    fn complete(&self, group: &Group, classes: &Classes) -> bool {
        let other = &*self.columns[0].1.row;
        let null = |column: &String| {
            group
                .nulls
                .iter()
                .any(|(r, c)| *r == self.row && **c == **column)
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
    fn selects_rest(&self, projected: &[RowColumn]) -> bool {
        let other = &self.columns[0].1.row;
        self.fk.referred.iter().enumerate().all(|(at, referred)| {
            self.columns.iter().any(|(i, _, _)| *i == at)
                || projected
                    .iter()
                    .any(|(row, column)| row == other && **column == **referred)
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
struct Reader<'r, 's> {
    analyzer: &'r Analyzer<'s>,
    scope: &'r Scope,
    subqueries: usize,
    /// The groups of `OR` branches and `IN` list items, each an alternative.
    groups: Vec<Group<'s, 'r>>,
}

impl<'r, 's> Reader<'r, 's> {
    /// Reads the terms of one `AND` into `group`, then each `OR` branch and `IN` list item
    /// into an alternative of it.
    fn conjunction(&mut self, terms: Vec<&'r Expr>, group: &mut Group<'s, 'r>) {
        let (mut branches, mut lists) = (Vec::new(), Vec::new());
        let mut alternatives = Vec::new();
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
                _ if self.comparison(term, group, &mut alternatives) => {
                    self.groups.append(&mut alternatives);
                }
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
            self.in_list(expr, list, group, &mut alternatives);
            self.groups.append(&mut alternatives);
        }
    }

    /// Reads `expr` into `group` when it is a comparison, and the alternatives of an `IN`
    /// list into `alternatives`.
    fn comparison<'g>(
        &mut self,
        expr: &'g Expr,
        group: &mut Group<'s, 'g>,
        alternatives: &mut Vec<Group<'s, 'g>>,
    ) -> bool
    where
        'r: 'g,
    {
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
                _ => self.in_list(left, list, group, alternatives),
            },
            Expr::In {
                expr: left,
                target: InTarget::Subquery(subquery),
                ..
            } => self.subquery(left, subquery, group),
            Expr::IsNull(column) => {
                if let Some(column) = self.analyzer.locate(column, self.scope, "") {
                    group.nulls.push((column.row, Cow::Borrowed(column.name)));
                }
            }
            _ => return false,
        }
        true
    }

    fn in_list<'g>(
        &self,
        left: &'g Expr,
        list: &'g [Expr],
        group: &Group<'s, 'g>,
        alternatives: &mut Vec<Group<'s, 'g>>,
    ) where
        'r: 'g,
    {
        for item in list {
            let mut alternative = group.alternative();
            let span = left.span().union(&item.span());
            self.compare(left, item, false, span, &mut alternative);
            alternatives.push(alternative);
        }
    }

    /// Reads `left = right` (`pin`) or `left <> right` into `group`, row values by position.
    fn compare<'g>(
        &self,
        left: &'g Expr,
        right: &'g Expr,
        pin: bool,
        span: Span,
        group: &mut Group<'s, 'g>,
    ) where
        'r: 'g,
    {
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
                        (left.row.clone(), Cow::Borrowed(left.name)),
                        (right.row.clone(), Cow::Borrowed(right.name)),
                    ]);
                }
                group.pairs.extend(analyzer.compared(left, right, span));
            }
            (a, b) => match constant_of(a, right).or_else(|| constant_of(b, left)) {
                Some((column, value)) => group.pinned.extend(
                    pinned(pin, value).map(|v| (column.row, Cow::Borrowed(column.name), v)),
                ),
                // `lower(w.code) = lower(s.code)`
                None => group.unread.push(self.rows(&[left, right])),
            },
        }
    }

    /// The rows the columns of `exprs` are read from, outside subqueries.
    fn rows<'g>(&self, exprs: &[&Expr]) -> Vec<Name<'g>> {
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
    fn subquery<'g>(&mut self, expr: &'g Expr, subquery: &Query, group: &mut Group<'s, 'g>)
    where
        'r: 'g,
    {
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
                analyzer
                    .locate(right, &inner_scope, &row)
                    .map(Located::into_owned),
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
            let located = |expr: &Expr| {
                analyzer
                    .locate(expr, &inner_scope, &row)
                    .map(Located::into_owned)
            };
            match (located(left), located(right)) {
                (Some(a), Some(b)) => group.equal.push([
                    (a.row, Cow::Borrowed(a.name)),
                    (b.row, Cow::Borrowed(b.name)),
                ]),
                (a, b) => group.pinned.extend(
                    constant_of(a, right)
                        .or_else(|| constant_of(b, left))
                        .and_then(|(column, value)| {
                            Some((column.row, Cow::Borrowed(column.name), pinned(true, value)?))
                        }),
                ),
            }
        }
    }
}

/// `column` compared with `expr`, when `expr` is a constant or parameter.
fn constant_of<'s, 'n, 'e>(
    column: Option<Located<'s, 'n>>,
    expr: &'e Expr,
) -> Option<(Located<'s, 'n>, &'e ValueWithSpan)> {
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
            let mut alternatives = Vec::new();
            if self.reader.comparison(expr, &mut group, &mut alternatives) {
                for group in alternatives.into_iter().chain([group]) {
                    self.reader.groups.push(group.into_owned());
                }
            }
        }
        ControlFlow::Continue(())
    }
}

/// Collects the rows of the columns an expression reads, outside subqueries.
struct Rows<'r, 'a, 's, 'g> {
    reader: &'r Reader<'a, 's>,
    queries: usize,
    rows: Vec<Name<'g>>,
}

impl Visitor for Rows<'_, '_, '_, '_> {
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
                self.rows.extend(
                    analyzer
                        .locate(expr, scope, "")
                        .map(|column| Cow::Owned(column.row.into_owned())),
                );
            }
        }
        ControlFlow::Continue(())
    }
}
