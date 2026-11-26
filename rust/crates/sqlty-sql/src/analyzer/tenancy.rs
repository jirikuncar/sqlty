//! `tenant-scope`: a statement that reads, updates or deletes rows of a tenant table without
//! pinning its tenant key to a parameter, or inserts one whose tenant key is no parameter.
//!
//! `[tool.sqlty.tenancy]` names the tenant table; its primary key and every column of a
//! foreign key to it are tenant keys (see [`crate::catalog::Catalog::tenant_keys`]). A
//! source is pinned when one of its tenant keys equals a parameter, or a pinned tenant key of
//! another source, through `=` or `IN` in `WHERE` or a join condition that holds for its rows.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::rc::Rc;

use sqlparser::ast::{
    visit_expressions, Assignment, AssignmentTarget, BinaryOperator, Distinct, Expr, GroupByExpr,
    Ident, InTarget, Insert, JoinConstraint, JoinOperator, ObjectName, OnConflictAction, OnInsert,
    Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr, Spanned, Statement,
    TableFactor, TableObject, TableWithJoins, Value, WildcardAdditionalOptions,
};
use sqlparser::tokenizer::Span;

use super::dml::{assigned_names, written_rows};
use super::fanout::{conjuncts, strip};
use super::grouping::has_aggregate;
use super::names::{object_name, table_key};
use super::query::{Analyzer, Column};
use super::scope::{any_merging_join, relation_name, Scope};
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

/// What a CTE or derived table tells the query reading it: its outputs pinned to a
/// parameter, and the tenant tables it reads unpinned, by the outputs that carry each key
/// column.
#[derive(Debug)]
pub(super) struct Carried {
    pub(super) pinned: Vec<String>,
    pub(super) unpinned: Vec<Unpinned<(String, String)>>,
}

/// A tenant table read without a whole tenant key pinned: its message, where, and per
/// tenant key, the columns that would pin each of its columns.
#[derive(Clone, Debug)]
pub(super) struct Unpinned<N> {
    message: String,
    span: Span,
    alternatives: Vec<Vec<Vec<N>>>,
}

impl<N> Unpinned<N> {
    fn map<M>(self, f: impl Fn(N) -> M) -> Unpinned<M> {
        Unpinned {
            message: self.message,
            span: self.span,
            alternatives: (self.alternatives.into_iter())
                .map(|key| {
                    key.into_iter()
                        .map(|options| options.into_iter().map(&f).collect())
                        .collect()
                })
                .collect(),
        }
    }

    fn reported(self) -> Unpinned<Node> {
        self.map(|_| (String::new(), String::new()))
    }
}

/// The SELECTs a CTE or derived table returns, and what each leaves to the query reading
/// it, by address: their analysis meets them again under every clone of the analyzer.
pub(super) struct TenantSink {
    selects: HashSet<*const Select>,
    found: RefCell<HashMap<*const Select, Branch>>,
}

/// What one SELECT of a [`TenantSink`] leaves: its width, its outputs its pins reach and
/// its tenant tables by the positions of the outputs that carry their keys.
struct Branch {
    width: usize,
    pinned: Vec<usize>,
    unpinned: Vec<Unpinned<usize>>,
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
    /// Records `tenant-scope` for each tenant table `select` reads without pinning its tenant
    /// key. A SELECT that a CTE or derived table returns leaves those whose key it passes to
    /// an output column to the query reading it instead.
    pub(super) fn check_tenant_select(&self, select: &Select, scope: &Scope) {
        if !self.judges_tenancy() {
            return;
        }
        let mut pins = Pins::default();
        self.pin_carried(scope, &mut pins);
        for table in &select.from {
            self.read_joins(table, scope, &mut pins);
        }
        for term in select.selection.iter().flat_map(conjuncts) {
            self.read_term(term, scope, Holds::All, &mut pins);
        }
        let factors = select.from.iter().flat_map(factors);
        let unpinned = self.unpinned(factors, scope, &pins, |_| "read");
        match &self.tenant_sink {
            Some(sink) if sink.selects.contains(&(select as *const Select)) => {
                let branch = self.carry(select, scope, &pins, unpinned);
                sink.found
                    .borrow_mut()
                    .insert(select as *const Select, branch);
            }
            _ => self.report(unpinned),
        }
    }

    /// A sink for the SELECTs `query`, the body of a CTE or derived table, returns.
    pub(super) fn tenant_sink(&self, query: &Query) -> Option<Rc<TenantSink>> {
        let mut selects = HashSet::new();
        (self.judges_tenancy() && body_selects(query, &mut selects)).then(|| {
            Rc::new(TenantSink {
                selects,
                found: RefCell::default(),
            })
        })
    }

    /// What the SELECTs of [`Self::tenant_sink`] leave to the query reading `columns`: the
    /// outputs every one pins, and their unpinned tenant tables. Reported here instead
    /// when a SELECT was not judged or returns another width.
    pub(super) fn carried(&self, columns: &[Column]) -> Option<Carried> {
        let sink = self.tenant_sink.as_ref()?;
        let found = std::mem::take(&mut *sink.found.borrow_mut());
        let whole = found.len() == sink.selects.len()
            && found.values().all(|branch| branch.width == columns.len());
        if !whole {
            for branch in found.into_values() {
                self.report(
                    branch
                        .unpinned
                        .into_iter()
                        .map(Unpinned::reported)
                        .collect(),
                );
            }
            return None;
        }
        let name = |at: usize| self.column_key(&columns[at].name);
        let mut branches = found.into_values();
        let first = branches.next()?;
        let mut pinned = first.pinned.clone();
        let mut unpinned = first.unpinned;
        for branch in branches {
            pinned.retain(|at| branch.pinned.contains(at));
            unpinned.extend(branch.unpinned);
        }
        Some(Carried {
            pinned: pinned.into_iter().map(name).collect(),
            unpinned: (unpinned.into_iter())
                .map(|found| found.map(|at| (String::new(), name(at))))
                .collect(),
        })
    }

    /// The branch `select` leaves to the query reading it: the outputs its pins reach, and
    /// each tenant table whose key some output carries; the others are reported.
    fn carry(
        &self,
        select: &Select,
        scope: &Scope,
        pins: &Pins,
        unpinned: Vec<Unpinned<Node>>,
    ) -> Branch {
        let passed = self.passed(select, scope);
        let width = passed.as_ref().map_or(usize::MAX, Vec::len);
        let passed = passed.unwrap_or_default();
        let reached = pins.reached();
        let pinned = (0..passed.len())
            .filter(|&at| {
                passed[at]
                    .as_ref()
                    .is_some_and(|node| reached.contains(node))
            })
            .collect();
        let feeders = pins.feeders();
        let mut carried = Vec::new();
        let mut lost = Vec::new();
        for found in unpinned {
            let alternatives: Vec<Vec<Vec<usize>>> = (found.alternatives.iter())
                .map(|key| {
                    key.iter()
                        .map(|options| {
                            let sources = feeders.reaching(options);
                            (0..passed.len())
                                .filter(|&at| {
                                    passed[at].as_ref().is_some_and(|n| sources.contains(n))
                                })
                                .collect()
                        })
                        .collect()
                })
                .filter(|key: &Vec<Vec<usize>>| key.iter().all(|options| !options.is_empty()))
                .collect();
            match alternatives.is_empty() {
                true => lost.push(found),
                false => carried.push(Unpinned {
                    message: found.message,
                    span: found.span,
                    alternatives,
                }),
            }
        }
        self.report(lost);
        Branch {
            width,
            pinned,
            unpinned: carried,
        }
    }

    /// The column each output of `select` passes on unchanged, `None` for one it computes.
    /// `None` when a window or `DISTINCT ON` reads rows of every tenant, or a `*` is unknown.
    fn passed(&self, select: &Select, scope: &Scope) -> Option<Vec<Option<Node>>> {
        if matches!(select.distinct, Some(Distinct::On(_))) || select.qualify.is_some() {
            return None;
        }
        let keys: Vec<Node> = match &select.group_by {
            GroupByExpr::Expressions(keys, _) => keys,
            GroupByExpr::All(_) => return None,
        }
        .iter()
        .filter_map(|key| self.local_node(key, scope))
        .collect();
        let grouped = select.having.is_some()
            || !matches!(&select.group_by, GroupByExpr::Expressions(keys, _) if keys.is_empty());
        let mut passed = Vec::new();
        for item in &select.projection {
            let (reference, options) = match item {
                SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                    if has_window(expr) {
                        return None;
                    }
                    let grouped = grouped || has_aggregate(expr);
                    let node = self.local_node(expr, scope);
                    passed.push(node.filter(|node| !grouped || keys.contains(node)));
                    continue;
                }
                SelectItem::Wildcard(options) => (None, options),
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    options,
                ) => (Some(object_name(name)), options),
                _ => return None,
            };
            if *options != WildcardAdditionalOptions::default()
                || reference.is_none() && any_merging_join(&select.from)
            {
                return None;
            }
            for source in Self::star_sources(reference.as_deref(), scope) {
                let table = source.table.named()?;
                let names: Vec<String> = match self.virtual_columns(table) {
                    Some(columns) if columns.iter().any(Column::is_unknown_star) => return None,
                    Some(columns) => columns
                        .iter()
                        .map(|column| self.column_key(&column.name))
                        .collect(),
                    None => (self.schema.table(table)?.iter())
                        .filter(|column| !column.invisible)
                        .map(|column| column.name.clone())
                        .collect(),
                };
                for name in names {
                    passed.push((!grouped).then(|| (source.reference.clone(), name)));
                }
            }
        }
        Some(passed)
    }

    /// The column of a source of `scope` (not of an enclosing query) that `expr` reads.
    fn local_node(&self, expr: &Expr, scope: &Scope) -> Option<Node> {
        let (node, _) = self.tenant_node(expr, scope)?;
        scope
            .iter()
            .any(|source| source.reference == node.0)
            .then_some(node)
    }

    /// Pins the outputs a CTE or derived table of `scope` pins itself.
    fn pin_carried(&self, scope: &Scope, pins: &mut Pins) {
        for source in scope {
            let carried = source
                .table
                .named()
                .and_then(|table| self.virtuals.carried(table));
            for column in carried.iter().flat_map(|carried| &carried.pinned) {
                pins.pinned.push((source.reference.clone(), column.clone()));
            }
        }
    }

    fn report(&self, unpinned: Vec<Unpinned<Node>>) {
        for found in unpinned {
            let _at = self.diag.at(found.span);
            self.diag.record(Rule::TenantScope, found.message);
        }
    }

    /// Records `tenant-scope` for an UPDATE or DELETE that does not pin the tenant key of
    /// its target or a table it joins, and an INSERT that sets one from no parameter.
    pub(super) fn check_tenant_dml(&self, stmt: &Statement) {
        if !self.judges_tenancy() {
            return;
        }
        let (target, joined, selection, verb): (_, Vec<&TableWithJoins>, _, _) = match stmt {
            Statement::Insert(
                insert @ Insert {
                    table: TableObject::TableName(name),
                    ..
                },
            ) => return self.check_tenant_insert(insert, name),
            Statement::Update(update) => {
                self.check_tenant_update(update);
                let from = update.from.iter().flat_map(|from| match from {
                    sqlparser::ast::UpdateTableFromKind::BeforeSet(tables)
                    | sqlparser::ast::UpdateTableFromKind::AfterSet(tables) => tables,
                });
                let joined = std::iter::once(&update.table).chain(from).collect();
                (
                    Some(&update.table.relation),
                    joined,
                    update.selection.as_ref(),
                    "updated",
                )
            }
            Statement::Delete(delete) => {
                let (sqlparser::ast::FromTable::WithFromKeyword(tables)
                | sqlparser::ast::FromTable::WithoutKeyword(tables)) = &delete.from;
                let joined = tables.iter().chain(delete.using.iter().flatten()).collect();
                (
                    tables.first().map(|table| &table.relation),
                    joined,
                    delete.selection.as_ref(),
                    "deleted from",
                )
            }
            _ => return,
        };
        let tables: Vec<TableWithJoins> = joined.into_iter().cloned().collect();
        // Collects, so that derived tables leave their tenant tables, into a list dropped.
        let mut quiet = self.clone();
        quiet.diag = Diagnostics::collecting();
        let Ok((analyzer, scope)) = quiet.scope_of(&tables);
        let mut pins = Pins::default();
        analyzer.pin_carried(&scope, &mut pins);
        for table in &tables {
            analyzer.read_joins(table, &scope, &mut pins);
        }
        for term in selection.into_iter().flat_map(conjuncts) {
            analyzer.read_term(term, &scope, Holds::All, &mut pins);
        }
        let target = target.and_then(relation_name);
        let factors = tables.iter().flat_map(factors);
        let unpinned = analyzer.unpinned(factors, &scope, &pins, |reference| {
            match Some(reference) == target.as_deref() {
                true => verb,
                false => "read",
            }
        });
        self.report(unpinned);
    }

    fn judges_tenancy(&self) -> bool {
        self.diag.enabled() && self.judges_names() && self.schema.catalog().has_tenancy()
    }

    /// An INSERT into a tenant table other than the tenant table itself must set each
    /// tenant key column from a parameter: in every row of its `VALUES` or `SET`, or in the
    /// select list of its query, where a column also counts, as that query is judged itself.
    /// Its `ON CONFLICT DO UPDATE` must set one from a parameter or `EXCLUDED`.
    fn check_tenant_insert(&self, insert: &Insert, name: &ObjectName) {
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
        name: &ObjectName,
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
                        let node = |reference: &String| {
                            self.tenant_node(&qualified(reference, ident), scope)
                        };
                        let earlier = left
                            .iter()
                            .rev()
                            .find(|reference| self.resolves(&qualified(reference, ident), scope));
                        let a = earlier.and_then(node);
                        if let (Some(a), Some(b)) = (a, right.first().and_then(node)) {
                            pins.link(&a, &b, holds);
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
        let (left, right): (&Expr, &Expr) = match strip(term) {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            }
            | Expr::IsNotDistinctFrom(left, right) => (left, right),
            Expr::AnyOp {
                left,
                compare_op: BinaryOperator::Eq,
                right,
                ..
            } if is_parameter(right) => (left, right),
            Expr::In {
                expr,
                target: InTarget::List(list),
                negated: false,
                ..
            } => match list.as_slice() {
                [first, rest @ ..] if is_parameter(first) && rest.iter().all(is_parameter) => {
                    (expr, first)
                }
                _ => return,
            },
            _ => return,
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
    /// as pinned, and so does an output of a CTE or derived table that pins or carries one:
    /// that query is judged on its own.
    fn tenant_node(&self, expr: &Expr, scope: &Scope) -> Option<(Node, bool)> {
        let expr = match strip(expr) {
            Expr::Cast { expr, .. } => strip(expr),
            expr => expr,
        };
        let (reference, table, column) = self.locate_column(expr, scope)?;
        let outer = !scope.iter().any(|source| source.reference == reference);
        let (column, keyed) = match self.virtuals.carried(&table) {
            Some(carried) => {
                let column = self.column_key(&column);
                let keyed = carried.pinned.contains(&column)
                    || (carried.unpinned.iter())
                        .flat_map(|found| found.alternatives.iter().flatten().flatten())
                        .any(|(_, carrier)| *carrier == column);
                (column, keyed)
            }
            None if self.virtual_columns(&table).is_some() => (self.column_key(&column), false),
            None => {
                let keys = self.schema.catalog().tenant_keys(&table);
                let keyed =
                    keys.is_some_and(|keys| keys.iter().flatten().any(|key| *key == column));
                (column, keyed)
            }
        };
        Some(((reference, column), outer && keyed))
    }

    /// The tenant tables of `factors`, and those CTEs and derived tables of `scope` leave
    /// to it, that `pins` does not reach a whole tenant key of.
    fn unpinned<'f>(
        &self,
        factors: impl Iterator<Item = &'f TableFactor>,
        scope: &Scope,
        pins: &Pins,
        verb: impl Fn(&str) -> &'static str,
    ) -> Vec<Unpinned<Node>> {
        let reached = pins.reached();
        let mut found = Vec::new();
        for factor in factors {
            let TableFactor::Table { name, alias, .. } = factor else {
                continue;
            };
            let reference = alias.as_ref().map_or_else(
                || object_name(name),
                |alias| alias.name.value.to_lowercase(),
            );
            let keys = (scope.iter())
                .find(|source| source.reference == reference)
                .and_then(|source| source.table.named())
                .filter(|key| self.virtual_columns(key).is_none())
                .and_then(|key| self.schema.catalog().tenant_keys(key));
            let Some(keys) = keys else {
                continue;
            };
            let short = object_name(name);
            let pinned: Vec<String> = keys
                .iter()
                .map(|key| {
                    let pins: Vec<String> = key
                        .iter()
                        .map(|column| format!("`{short}.{column} = <parameter>`"))
                        .collect();
                    pins.join(" and ")
                })
                .collect();
            found.push(Unpinned {
                message: format!(
                    "`{short}` is {} without {}",
                    verb(&reference),
                    pinned.join(" or ")
                ),
                span: factor.span(),
                alternatives: (keys.iter())
                    .map(|key| {
                        (key.iter())
                            .map(|column| vec![(reference.clone(), column.clone())])
                            .collect()
                    })
                    .collect(),
            });
        }
        for source in scope {
            let carried = source
                .table
                .named()
                .and_then(|table| self.virtuals.carried(table));
            for inner in carried.iter().flat_map(|carried| &carried.unpinned) {
                found.push(
                    inner
                        .clone()
                        .map(|(_, column)| (source.reference.clone(), column)),
                );
            }
        }
        found.retain(|found| {
            !found.alternatives.iter().any(|key| {
                key.iter()
                    .all(|options| options.iter().any(|node| reached.contains(node)))
            })
        });
        found
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

    /// The arcs reversed: the columns each column takes its value from.
    fn feeders(&self) -> Feeders {
        let mut feeders: HashMap<Node, Vec<Node>> = HashMap::new();
        for (from, to) in &self.arcs {
            for to in to {
                feeders.entry(to.clone()).or_default().push(from.clone());
            }
        }
        Feeders(feeders)
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

struct Feeders(HashMap<Node, Vec<Node>>);

impl Feeders {
    /// The columns whose value reaches one of `nodes`, those included.
    fn reaching(&self, nodes: &[Node]) -> HashSet<Node> {
        let mut found: HashSet<Node> = HashSet::new();
        let mut queue = nodes.to_vec();
        while let Some(node) = queue.pop() {
            if let Some(next) = self.0.get(&node) {
                queue.extend(next.iter().filter(|n| !found.contains(*n)).cloned());
            }
            found.insert(node);
        }
        found
    }
}

/// Collects the SELECTs whose rows `query` returns as they are; `false` when a `LIMIT` or
/// a statement picks them.
fn body_selects(query: &Query, into: &mut HashSet<*const Select>) -> bool {
    fn set(body: &SetExpr, into: &mut HashSet<*const Select>) -> bool {
        match body {
            SetExpr::Select(select) => into.insert(&**select as *const Select) || true,
            SetExpr::SetOperation { left, right, .. } => set(left, into) && set(right, into),
            SetExpr::Query(query) => body_selects(query, into),
            _ => false,
        }
    }
    query.limit_clause.is_none() && query.fetch.is_none() && set(&query.body, into)
}

/// Whether `expr` calls a window function outside a subquery.
fn has_window(expr: &Expr) -> bool {
    let mut found = false;
    let _ = visit_expressions(expr, |expr| {
        found |= matches!(expr, Expr::Function(func) if func.over.is_some());
        ControlFlow::<()>::Continue(())
    });
    found
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
