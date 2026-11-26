//! `tenant-scope`: a statement that reads, updates or deletes rows of a tenant table without
//! pinning its tenant key to a parameter, or inserts one whose tenant key is no parameter.
//!
//! `[tool.sqlty.tenancy]` names the tenant table; its primary key and every column of a
//! foreign key to it are tenant keys (see [`crate::catalog::Catalog::tenant_keys`]). A
//! source is pinned when one of its tenant keys equals a parameter, or a pinned tenant key of
//! another source, through `=` or `IN` in `WHERE` or a join condition that holds for its rows.

use std::collections::{HashMap, HashSet};

use sqlparser::ast::{
    Assignment, AssignmentTarget, BinaryOperator, Expr, Ident, InTarget, JoinConstraint,
    JoinOperator, OnConflictAction, OnInsert, SetExpr, Spanned, Statement, TableFactor,
    TableObject, TableWithJoins, Value,
};

use super::dml::{assigned_names, written_rows};
use super::fanout::{conjuncts, strip};
use super::names::{object_name, table_key};
use super::query::Analyzer;
use super::scope::{relation_name, Scope};
use crate::diagnostics::{Diagnostics, Rule};

/// A column of a source: its reference and name.
type Node = (String, String);

/// What the conditions of a statement hold: the columns equal to a parameter, and the
/// columns each column's value passes to.
#[derive(Default)]
struct Pins {
    pinned: Vec<Node>,
    arcs: HashMap<Node, Vec<Node>>,
}

/// The sources a condition holds for: all of them, those of a join's right side, or all
/// but those (a `RIGHT JOIN` keeps every row of its right side).
#[derive(Clone, Copy)]
enum Holds<'a> {
    All,
    Only(&'a [String]),
    Except(&'a [String]),
}

impl Holds<'_> {
    fn covers(&self, reference: &str) -> bool {
        match self {
            Holds::All => true,
            Holds::Only(sources) => sources.iter().any(|source| source == reference),
            Holds::Except(sources) => !sources.iter().any(|source| source == reference),
        }
    }
}

impl Analyzer<'_> {
    /// Records `tenant-scope` for each tenant table `from` and `selection` read without
    /// pinning its tenant key.
    pub(super) fn check_tenant_select(
        &self,
        from: &[TableWithJoins],
        selection: Option<&Expr>,
        scope: &Scope,
    ) {
        if !self.judges_tenancy() {
            return;
        }
        let mut pins = Pins::default();
        for table in from {
            self.read_joins(table, scope, &mut pins);
        }
        for term in selection.into_iter().flat_map(conjuncts) {
            self.read_term(term, scope, Holds::All, &mut pins);
        }
        let factors = from.iter().flat_map(factors);
        self.report_unpinned(factors, scope, &pins, |_| "read");
    }

    /// Records `tenant-scope` for an UPDATE or DELETE that does not pin the tenant key of
    /// its target or a table it joins, and an INSERT that sets one from no parameter.
    pub(super) fn check_tenant_dml(&self, stmt: &Statement) {
        if !self.judges_tenancy() {
            return;
        }
        let (target, joined, selection, verb): (_, Vec<&TableWithJoins>, _, _) = match stmt {
            Statement::Insert(insert) => return self.check_tenant_insert(insert),
            Statement::Update(update) => {
                self.check_tenant_update(update);
                let from = update.from.iter().flat_map(|from| match from {
                    sqlparser::ast::UpdateTableFromKind::BeforeSet(tables)
                    | sqlparser::ast::UpdateTableFromKind::AfterSet(tables) => tables,
                });
                let joined = std::iter::once(&update.table).chain(from).collect();
                (
                    &update.table.relation,
                    joined,
                    update.selection.as_ref(),
                    "updated",
                )
            }
            Statement::Delete(delete) => {
                let (sqlparser::ast::FromTable::WithFromKeyword(tables)
                | sqlparser::ast::FromTable::WithoutKeyword(tables)) = &delete.from;
                let Some(first) = tables.first() else {
                    return;
                };
                let joined = tables.iter().chain(delete.using.iter().flatten()).collect();
                (
                    &first.relation,
                    joined,
                    delete.selection.as_ref(),
                    "deleted from",
                )
            }
            _ => return,
        };
        let tables: Vec<TableWithJoins> = joined.into_iter().cloned().collect();
        let mut quiet = self.clone();
        quiet.diag = Diagnostics::default();
        let Ok((analyzer, scope)) = quiet.scope_of(&tables);
        let mut pins = Pins::default();
        for table in &tables {
            analyzer.read_joins(table, &scope, &mut pins);
        }
        for term in selection.into_iter().flat_map(conjuncts) {
            analyzer.read_term(term, &scope, Holds::All, &mut pins);
        }
        let target = relation_name(target);
        let factors = tables.iter().flat_map(factors);
        self.report_unpinned(factors, &scope, &pins, |reference| {
            match Some(reference) == target.as_deref() {
                true => verb,
                false => "read",
            }
        });
    }

    fn judges_tenancy(&self) -> bool {
        self.diag.enabled() && self.judges_names() && self.schema.catalog().has_tenancy()
    }

    /// An INSERT into a tenant table other than the tenant table itself must set each
    /// tenant key column from a parameter: in every row of its `VALUES` or `SET`, or in the
    /// select list of its query, where a column also counts, as that query is judged itself.
    /// Its `ON CONFLICT DO UPDATE` must set one from a parameter or `EXCLUDED`.
    fn check_tenant_insert(&self, insert: &sqlparser::ast::Insert) {
        let TableObject::TableName(name) = &insert.table else {
            return;
        };
        let key = table_key(name, self.dialect);
        let catalog = self.schema.catalog();
        let Some(keys) = catalog.tenant_keys(&key) else {
            return;
        };
        let short = object_name(name);
        let mut tenant_columns: Vec<&str> = keys.iter().flatten().map(String::as_str).collect();
        tenant_columns.sort_unstable();
        tenant_columns.dedup();
        if let Some(OnInsert::OnConflict(on_conflict)) = &insert.on {
            if let OnConflictAction::DoUpdate(update) = &on_conflict.action {
                let excluded = |value: &Expr| {
                    matches!(strip(value), Expr::CompoundIdentifier(parts)
                        if parts.len() == 2 && parts[0].value.eq_ignore_ascii_case("excluded"))
                };
                self.check_tenant_assignments(
                    name,
                    &short,
                    &tenant_columns,
                    &update.assignments,
                    &excluded,
                );
            }
        }
        if catalog.is_tenant_table(&key) {
            return;
        }
        let assigned = assigned_names(&insert.assignments);
        let (columns, rows): (Vec<String>, Vec<Vec<&Expr>>) = match &insert.source {
            None => (
                assigned,
                vec![insert.assignments.iter().map(|a| &a.value).collect()],
            ),
            Some(source) => {
                let columns = match insert.columns.is_empty() {
                    false => insert.columns.iter().map(object_name).collect(),
                    true => (self.schema.table(&key).into_iter().flatten())
                        .filter(|column| !column.invisible)
                        .map(|column| column.name.to_lowercase())
                        .collect(),
                };
                let mut rows = Vec::new();
                written_rows(&source.body, &mut rows);
                (columns, rows)
            }
        };
        let query = !matches!(
            insert.source.as_deref().map(|q| q.body.as_ref()),
            None | Some(SetExpr::Values(_))
        );
        let tenant_value = |value: &Expr| {
            is_parameter(value)
                || query
                    && matches!(
                        strip(value),
                        Expr::Identifier(_) | Expr::CompoundIdentifier(_)
                    )
        };
        for tenant in tenant_columns {
            let at = columns
                .iter()
                .position(|column| column.eq_ignore_ascii_case(tenant));
            let from_parameter = at.is_some_and(|at| {
                rows.iter()
                    .all(|row| row.get(at).is_some_and(|value| tenant_value(value)))
            });
            if !from_parameter {
                let _at = self.diag.at(name.span());
                self.diag.record(
                    Rule::TenantScope,
                    format!("INSERT into `{short}` does not set `{tenant}` from a parameter"),
                );
            }
        }
    }

    /// An UPDATE of a tenant table that sets a tenant key column must set it from a
    /// parameter, or the row moves to a tenant the caller did not name.
    fn check_tenant_update(&self, update: &sqlparser::ast::Update) {
        let TableFactor::Table { name, .. } = &update.table.relation else {
            return;
        };
        let key = table_key(name, self.dialect);
        let Some(keys) = self.schema.catalog().tenant_keys(&key) else {
            return;
        };
        let mut tenant_columns: Vec<&str> = keys.iter().flatten().map(String::as_str).collect();
        tenant_columns.sort_unstable();
        tenant_columns.dedup();
        let short = object_name(name);
        self.check_tenant_assignments(name, &short, &tenant_columns, &update.assignments, &|_| {
            false
        });
    }

    fn check_tenant_assignments(
        &self,
        name: &sqlparser::ast::ObjectName,
        short: &str,
        tenant_columns: &[&str],
        assignments: &[Assignment],
        also: &dyn Fn(&Expr) -> bool,
    ) {
        for assignment in assignments {
            let AssignmentTarget::ColumnName(column) = &assignment.target else {
                continue;
            };
            let column = object_name(column);
            let tenant = tenant_columns
                .iter()
                .find(|key| key.eq_ignore_ascii_case(&column));
            if let Some(tenant) = tenant {
                if !is_parameter(&assignment.value) && !also(&assignment.value) {
                    let _at = self.diag.at(name.span());
                    self.diag.record(
                        Rule::TenantScope,
                        format!("UPDATE of `{short}` does not set `{tenant}` from a parameter"),
                    );
                }
            }
        }
    }

    /// Reads the join conditions of `table` into `pins`, each for the sources it holds for.
    fn read_joins(&self, table: &TableWithJoins, scope: &Scope, pins: &mut Pins) {
        if let TableFactor::NestedJoin {
            table_with_joins, ..
        } = &table.relation
        {
            self.read_joins(table_with_joins, scope, pins);
        }
        let mut left: Vec<String> = references(&table.relation);
        for join in &table.joins {
            if let TableFactor::NestedJoin {
                table_with_joins, ..
            } = &join.relation
            {
                self.read_joins(table_with_joins, scope, pins);
            }
            let right = references(&join.relation);
            let (constraint, holds) = match &join.join_operator {
                JoinOperator::Join(c)
                | JoinOperator::Inner(c)
                | JoinOperator::CrossJoin(c)
                | JoinOperator::StraightJoin(c) => (c, Holds::All),
                JoinOperator::Left(c) | JoinOperator::LeftOuter(c) => (c, Holds::Only(&right)),
                JoinOperator::Right(c) | JoinOperator::RightOuter(c) => (c, Holds::Except(&right)),
                _ => {
                    left.extend(right);
                    continue;
                }
            };
            match constraint {
                JoinConstraint::On(on) => {
                    for term in conjuncts(on) {
                        self.read_term(term, scope, holds, pins);
                    }
                }
                JoinConstraint::Using(names) => {
                    for ident in names.iter().filter_map(|name| name.0.last()?.as_ident()) {
                        let joined = right.first();
                        let earlier = left
                            .iter()
                            .rev()
                            .find(|reference| self.resolves(&qualified(reference, ident), scope));
                        if let (Some(a), Some(b)) = (earlier, joined) {
                            let a = self.tenant_node(&qualified(a, ident), scope);
                            let b = self.tenant_node(&qualified(b, ident), scope);
                            if let (Some(a), Some(b)) = (a, b) {
                                pins.link(&a, &b, holds);
                            }
                        }
                    }
                }
                _ => {}
            }
            left.extend(right);
        }
    }

    /// Reads one conjunct: `column = parameter`, `column IN (parameters)`, `column = ANY
    /// (parameter)` and `column = column`.
    fn read_term(&self, term: &Expr, scope: &Scope, holds: Holds, pins: &mut Pins) {
        let (left, rights): (&Expr, Vec<&Expr>) = match strip(term) {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            }
            | Expr::IsNotDistinctFrom(left, right) => (left, vec![right]),
            Expr::AnyOp {
                left,
                compare_op: BinaryOperator::Eq,
                right,
                ..
            } if is_parameter(right) => (left, vec![right]),
            Expr::In {
                expr,
                target: InTarget::List(list),
                negated: false,
                ..
            } if !list.is_empty() && list.iter().all(is_parameter) => {
                (expr, list.iter().take(1).collect())
            }
            _ => return,
        };
        let [right] = rights.as_slice() else {
            return;
        };
        let node = |expr: &Expr| self.tenant_node(expr, scope);
        match (node(left), node(right)) {
            (Some(a), Some(b)) => pins.link(&a, &b, holds),
            (Some(a), None) if is_parameter(right) => pins.pin(a, holds),
            (None, Some(b)) if is_parameter(left) => pins.pin(b, holds),
            _ => {}
        }
    }

    /// The column `expr` reads, as a node. A tenant key of an enclosing query's row counts
    /// as pinned: that query is judged on its own.
    fn tenant_node(&self, expr: &Expr, scope: &Scope) -> Option<(Node, bool)> {
        let expr = match strip(expr) {
            Expr::Cast { expr, .. } => strip(expr),
            expr => expr,
        };
        let (reference, table, column, _) = self.locate_table_column(expr, scope)?;
        let outer = !scope.iter().any(|source| source.reference == reference);
        let keyed = outer
            && (self.schema.catalog().tenant_keys(&table))
                .is_some_and(|keys| keys.iter().flatten().any(|key| *key == column));
        Some(((reference, column), keyed))
    }

    fn report_unpinned<'f>(
        &self,
        factors: impl Iterator<Item = &'f TableFactor>,
        scope: &Scope,
        pins: &Pins,
        verb: impl Fn(&str) -> &'static str,
    ) {
        let reached = pins.reached();
        for factor in factors {
            let (TableFactor::Table { name, .. }, Some(reference)) =
                (factor, relation_name(factor))
            else {
                continue;
            };
            let Some(source) = scope.iter().find(|source| source.reference == reference) else {
                continue;
            };
            let Some(key) = source.table.named() else {
                continue;
            };
            if self.virtual_columns(key).is_some() {
                continue;
            }
            let Some(keys) = self.schema.catalog().tenant_keys(key) else {
                continue;
            };
            let pinned = keys.iter().any(|key| {
                key.iter()
                    .all(|column| reached.contains(&(reference.clone(), column.clone())))
            });
            if pinned {
                continue;
            }
            let short = object_name(name);
            let keys: Vec<String> = keys
                .iter()
                .map(|key| {
                    let pins: Vec<String> = key
                        .iter()
                        .map(|column| format!("`{short}.{column} = <parameter>`"))
                        .collect();
                    pins.join(" and ")
                })
                .collect();
            let _at = self.diag.at(factor.span());
            self.diag.record(
                Rule::TenantScope,
                format!(
                    "`{short}` is {} without {}",
                    verb(&reference),
                    keys.join(" or ")
                ),
            );
        }
    }
}

impl Pins {
    fn pin(&mut self, (node, _): (Node, bool), holds: Holds) {
        if holds.covers(&node.0) {
            self.pinned.push(node);
        }
    }

    /// `a = b`: a pinned `a` pins `b` and the reverse, where the condition holds for `b`'s
    /// (or `a`'s) source. An enclosing query's tenant key is pinned already.
    fn link(&mut self, a: &(Node, bool), b: &(Node, bool), holds: Holds) {
        for (from, to) in [(a, b), (b, a)] {
            if holds.covers(&to.0 .0) {
                if from.1 {
                    self.pinned.push(to.0.clone());
                }
                let arcs = self.arcs.entry(from.0.clone()).or_default();
                arcs.push(to.0.clone());
            }
        }
    }

    /// Every column a pinned column passes its value to, itself included.
    fn reached(&self) -> HashSet<Node> {
        let mut reached: HashSet<Node> = HashSet::new();
        let mut queue = self.pinned.clone();
        while let Some(node) = queue.pop() {
            if let Some(next) = self.arcs.get(&node) {
                queue.extend(next.iter().filter(|n| !reached.contains(*n)).cloned());
            }
            reached.insert(node);
        }
        reached
    }
}

/// A parameter, cast or not: `%s`, `$1`, `:name`, `?`.
fn is_parameter(expr: &Expr) -> bool {
    match strip(expr) {
        Expr::Cast { expr, .. } => is_parameter(expr),
        Expr::Value(value) => matches!(value.value, Value::Placeholder(_)),
        _ => false,
    }
}

/// The references of the sources `factor` names, those of a nested join included.
fn references(factor: &TableFactor) -> Vec<String> {
    match factor {
        TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } => factors(table_with_joins)
            .filter_map(relation_name)
            .collect(),
        factor => relation_name(factor).into_iter().collect(),
    }
}

/// The table factors of `table`, nested joins flattened.
fn factors(table: &TableWithJoins) -> Box<dyn Iterator<Item = &TableFactor> + '_> {
    let all = std::iter::once(&table.relation).chain(table.joins.iter().map(|j| &j.relation));
    Box::new(all.flat_map(|factor| match factor {
        TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } => factors(table_with_joins),
        factor => Box::new(std::iter::once(factor)),
    }))
}

fn qualified(reference: &str, ident: &Ident) -> Expr {
    Expr::CompoundIdentifier(vec![Ident::new(reference), ident.clone()])
}
