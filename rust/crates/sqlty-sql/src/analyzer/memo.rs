//! Memo of subquery analysis: each scalar subquery is analyzed once per context.
//!
//! The type, the nullability and the JSON shape of an expression are three walks over it, and
//! each of them needs the columns of a subquery the expression contains. Without a memo a
//! subquery nested `d` levels deep is analyzed `2^d` times.
//!
//! An entry is valid for one analysis context: the CTEs and derived tables in scope (the memo
//! belongs to a [`Virtuals`](super::scope::Virtuals), whose every change starts a fresh one),
//! the enclosing scopes, the merged columns and the facts. A node of the analyzed statement is
//! found by address, which costs no hashing of the query; the temporary expressions the
//! analyzer builds and drops are found by value, as are equal queries at different nodes. The
//! addresses are only compared, never dereferenced, and an alias is matched only under the
//! [`Stable`] set of the statement it was recorded for, which it keeps alive.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::ControlFlow;
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use sqlparser::ast::{Query, Statement, Visit, Visitor};

use super::nullability::Facts;
use super::query::Analyzer;
use super::scope::Scope;
use super::{Column, Inferred};

/// Columns of a subquery, shared between the walks that read them.
pub(super) type SubqueryColumns = Rc<Inferred<Option<Vec<Column>>>>;

/// Whether an analysis reports diagnostics for what it finds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reporting {
    Record,
    Silent,
}

struct Entry {
    query: Query,
    outer: Rc<Scope>,
    merged: Rc<[String]>,
    facts: Facts,
    /// Whether the analysis recorded diagnostics; a silent one cannot stand in for it.
    recorded: bool,
    /// Untyped results the analysis accounted for, replayed on a hit.
    explained: usize,
    columns: SubqueryColumns,
}

impl Entry {
    fn same_scope(&self, ctx: &Analyzer<'_>) -> bool {
        self.merged == ctx.merged
            && (Rc::ptr_eq(&self.outer, &ctx.outer) || self.outer == ctx.outer)
            && self.facts == ctx.facts
    }

    fn same_context(&self, query: &Query, ctx: &Analyzer<'_>) -> bool {
        self.query == *query && self.same_scope(ctx)
    }
}

/// The addresses of the queries of one statement, which outlives the analyzer that holds this.
#[derive(Default)]
pub(super) struct Stable {
    queries: OnceCell<FxHashSet<usize>>,
}

struct Addresses(FxHashSet<usize>);

impl Visitor for Addresses {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        self.0.insert(std::ptr::from_ref(query) as usize);
        ControlFlow::Continue(())
    }
}

impl Stable {
    #[cfg(test)]
    pub(super) fn built(&self) -> bool {
        self.queries.get().is_some()
    }
}

/// An entry as one node of one statement reads it.
type Alias = (Rc<Stable>, Rc<Entry>);

#[derive(Default)]
struct Inner {
    by_value: HashMap<u64, Vec<Rc<Entry>>>,
    by_node: FxHashMap<usize, Vec<Alias>>,
}

#[derive(Clone, Default)]
pub(super) struct SubqueryMemo(Rc<RefCell<Inner>>);

fn digest(query: &Query) -> u64 {
    let mut hasher = FxHasher::default();
    query.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
thread_local! {
    /// Lookups answered by address, by value, and not at all.
    pub(super) static LOOKUPS: std::cell::Cell<[usize; 3]> = const { std::cell::Cell::new([0; 3]) };
}

#[cfg(test)]
fn count(kind: usize) {
    LOOKUPS.with(|lookups| {
        let mut now = lookups.get();
        now[kind] += 1;
        lookups.set(now);
    });
}

impl SubqueryMemo {
    /// The entry for `query`, or the digest of `query` when there is none.
    fn get(
        &self,
        query: &Query,
        addr: Option<usize>,
        ctx: &Analyzer<'_>,
    ) -> Result<(SubqueryColumns, usize), u64> {
        let wants_record = ctx.diag.enabled();
        let mut inner = self.0.borrow_mut();
        if let Some(addr) = addr {
            let hit = inner.by_node.get(&addr).and_then(|aliases| {
                aliases.iter().find(|(stable, entry)| {
                    Rc::ptr_eq(stable, &ctx.stable)
                        && (entry.recorded || !wants_record)
                        && entry.same_scope(ctx)
                })
            });
            if let Some((_, entry)) = hit {
                #[cfg(test)]
                count(0);
                return Ok((Rc::clone(&entry.columns), entry.explained));
            }
        }
        let key = digest(query);
        let found = inner.by_value.get(&key).and_then(|bucket| {
            bucket
                .iter()
                .find(|entry| (entry.recorded || !wants_record) && entry.same_context(query, ctx))
        });
        let Some(entry) = found.map(Rc::clone) else {
            #[cfg(test)]
            count(2);
            return Err(key);
        };
        #[cfg(test)]
        count(1);
        if let Some(addr) = addr {
            let alias = (Rc::clone(&ctx.stable), Rc::clone(&entry));
            inner.by_node.entry(addr).or_default().push(alias);
        }
        Ok((Rc::clone(&entry.columns), entry.explained))
    }

    fn insert(
        &self,
        key: u64,
        query: &Query,
        addr: Option<usize>,
        ctx: &Analyzer<'_>,
        found: (SubqueryColumns, usize),
    ) {
        let mut inner = self.0.borrow_mut();
        let entry = Rc::new(Entry {
            query: query.clone(),
            outer: Rc::clone(&ctx.outer),
            merged: Rc::clone(&ctx.merged),
            facts: ctx.facts.clone(),
            recorded: ctx.diag.enabled(),
            explained: found.1,
            columns: found.0,
        });
        let bucket = inner.by_value.entry(key).or_default();
        // A recording entry replaces a silent one for the same context.
        bucket.retain(|old| old.recorded || !old.same_context(query, ctx));
        bucket.push(Rc::clone(&entry));
        if let Some(addr) = addr {
            let aliases = inner.by_node.entry(addr).or_default();
            aliases.retain(|(stable, old)| {
                !Rc::ptr_eq(stable, &ctx.stable) || old.recorded || !old.same_scope(ctx)
            });
            aliases.push((Rc::clone(&ctx.stable), entry));
        }
    }
}

impl<'a> Analyzer<'a> {
    /// The address of `query` when it is a node of the statement this analyzer is rooted at.
    fn stable_query(&self, query: &Query) -> Option<usize> {
        let root = self.root?;
        let queries = self.stable.queries.get_or_init(|| {
            let mut addresses = Addresses(FxHashSet::default());
            let _ = root.visit(&mut addresses);
            addresses.0
        });
        let addr = std::ptr::from_ref(query) as usize;
        queries.contains(&addr).then_some(addr)
    }

    /// The analyzer, rooted at `stmt`: the subqueries inside it are memoized by address.
    pub(super) fn rooted(mut self, stmt: &'a Statement) -> Self {
        self.root = Some(stmt);
        self.stable = Rc::default();
        self
    }

    /// Columns of `query` read as a subquery of the expression being analyzed in `scope`,
    /// from the memo when this context has analyzed it before.
    pub(super) fn subquery_columns(
        &self,
        query: &Query,
        scope: &Scope,
        reporting: Reporting,
    ) -> SubqueryColumns {
        let mut inner = self.correlated(scope);
        if reporting == Reporting::Silent {
            inner.diag = Default::default();
        }
        let memo = &self.virtuals.memo;
        let addr = inner.stable_query(query);
        let key = match memo.get(query, addr, &inner) {
            Ok((columns, explained)) => {
                inner.diag.add_explained(explained);
                return columns;
            }
            Err(key) => key,
        };
        let before = inner.diag.explained();
        let columns = Rc::new(inner.query_columns(query));
        let explained = inner.diag.explained() - before;
        memo.insert(key, query, addr, &inner, (Rc::clone(&columns), explained));
        columns
    }
}

/// The first column of a subquery's result, when it has one.
pub(super) fn first_column(columns: &SubqueryColumns) -> Inferred<Option<&Column>> {
    let columns = (**columns).as_ref().unwrap_or_else(|never| match *never {});
    Ok(columns.as_ref().and_then(|columns| columns.first()))
}

#[cfg(test)]
mod tests {
    use sqlparser::ast::{Expr, Select, SelectItem, SetExpr};

    use super::super::api::view_columns;
    use super::super::test_support::statement;
    use super::super::SchemaData;
    use super::super::{load_schema, SqlDialect};
    use super::*;
    use crate::diagnostics::Diagnostics;

    const DDL: &str = "CREATE TABLE a (id INT NOT NULL, x TEXT NOT NULL); \
                       CREATE TABLE b (id INT NOT NULL, y TEXT)";

    fn lookups() -> [usize; 3] {
        LOOKUPS.with(std::cell::Cell::get)
    }

    fn reset() {
        LOOKUPS.with(|lookups| lookups.set([0; 3]));
    }

    fn select_of(stmt: &Statement) -> &Select {
        let Statement::Query(query) = stmt else {
            panic!("not a query");
        };
        let SetExpr::Select(select) = &*query.body else {
            panic!("not a select");
        };
        select
    }

    fn subquery(expr: &Expr) -> &Query {
        match expr {
            Expr::Subquery(query) => query,
            Expr::BinaryOp { left, .. } => subquery(left),
            other => panic!("no subquery in {other}"),
        }
    }

    fn nth_subquery(stmt: &Statement, n: usize) -> &Query {
        match &select_of(stmt).projection[n] {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                subquery(expr)
            }
            _ => panic!("not an expression"),
        }
    }

    /// The columns and diagnostics of `stmt`, and the analyzer that read it.
    fn run<'a>(
        stmt: &'a Statement,
        schema: &'a SchemaData,
        rooted: bool,
    ) -> (String, String, Analyzer<'a>) {
        let mut analyzer = Analyzer::new(schema, SqlDialect::Postgres);
        if rooted {
            analyzer = analyzer.rooted(stmt);
        }
        analyzer.diag = Diagnostics::collecting();
        let Ok(columns) = analyzer.statement_columns(stmt);
        let found = format!("{:?}", analyzer.diag.located());
        (format!("{columns:?}"), found, analyzer)
    }

    fn same_either_way(sql: &str) -> (Statement, [usize; 3]) {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement(sql, SqlDialect::Postgres);
        let (columns, found, _) = run(&stmt, &schema, false);
        reset();
        let (rooted_columns, rooted_found, analyzer) = run(&stmt, &schema, true);
        assert_eq!((columns, found), (rooted_columns, rooted_found));
        assert!(analyzer.stable.built());
        (stmt, lookups())
    }

    #[test]
    fn a_node_of_the_statement_is_found_by_address() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement(
            "SELECT (SELECT max(b.id) FROM b WHERE b.id = a.id) + 1 AS m FROM a",
            SqlDialect::Postgres,
        );
        let mut analyzer = Analyzer::new(&schema, SqlDialect::Postgres).rooted(&stmt);
        analyzer.diag = Diagnostics::collecting();
        reset();
        let _ = analyzer.statement_columns(&stmt);
        let [nodes, values, misses] = lookups();
        assert_eq!((values, misses), (0, 1));
        assert!(nodes >= 1);

        let query = nth_subquery(&stmt, 0);
        let scope = Scope::new();
        let first = analyzer.subquery_columns(query, &scope, Reporting::Record);
        let before = lookups();
        let again = analyzer.subquery_columns(query, &scope, Reporting::Record);
        let after = lookups();
        assert!(Rc::ptr_eq(&first, &again));
        assert_eq!(after[0], before[0] + 1);
        assert_eq!((after[1], after[2]), (before[1], before[2]));
    }

    #[test]
    fn equal_subqueries_at_two_nodes_share_one_entry() {
        let (stmt, [nodes, values, misses]) =
            same_either_way("SELECT (SELECT x FROM nope), (SELECT x FROM nope) FROM a");
        assert!(values >= 1 && misses >= 1 && nodes >= 1);
        let schema = load_schema(DDL, "postgres");
        let (_, _, analyzer) = run(&stmt, &schema, true);
        let inner = analyzer.virtuals.memo.0.borrow();
        assert_eq!(inner.by_node.len(), 2);
        assert_eq!(inner.by_value.values().map(Vec::len).sum::<usize>(), 1);
    }

    #[test]
    fn a_case_operand_clone_is_found_by_value() {
        let (_, [nodes, values, misses]) = same_either_way(
            "SELECT CASE a.x WHEN (SELECT y FROM b LIMIT 1) THEN 1 ELSE 2 END, \
                    CASE a.x WHEN (SELECT y FROM b LIMIT 1) THEN 1 ELSE 2 END FROM a",
        );
        assert_eq!((nodes, values, misses), (0, 1, 1));
    }

    #[test]
    fn a_recording_analysis_replaces_a_silent_one_in_both_maps() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT (SELECT y FROM b) FROM a", SqlDialect::Postgres);
        let mut analyzer = Analyzer::new(&schema, SqlDialect::Postgres).rooted(&stmt);
        analyzer.diag = Diagnostics::collecting();
        let query = nth_subquery(&stmt, 0);
        let addr = std::ptr::from_ref(query) as usize;
        let scope = Scope::new();
        let entries = |analyzer: &Analyzer<'_>| {
            let inner = analyzer.virtuals.memo.0.borrow();
            let by_value: Vec<bool> = inner
                .by_value
                .values()
                .flatten()
                .map(|e| e.recorded)
                .collect();
            let by_node: Vec<bool> = inner.by_node[&addr]
                .iter()
                .map(|(_, e)| e.recorded)
                .collect();
            (by_value, by_node)
        };
        reset();
        analyzer.subquery_columns(query, &scope, Reporting::Silent);
        assert_eq!(entries(&analyzer), (vec![false], vec![false]));
        analyzer.subquery_columns(query, &scope, Reporting::Record);
        assert_eq!(entries(&analyzer), (vec![true], vec![true]));
        analyzer.subquery_columns(query, &scope, Reporting::Silent);
        assert_eq!(lookups(), [1, 0, 2]);
    }

    #[test]
    fn a_by_value_hit_is_aliased_to_its_node() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement(
            "SELECT (SELECT y FROM b), (SELECT y FROM b) FROM a",
            SqlDialect::Postgres,
        );
        let analyzer = Analyzer::new(&schema, SqlDialect::Postgres).rooted(&stmt);
        let scope = Scope::new();
        reset();
        let first = analyzer.subquery_columns(nth_subquery(&stmt, 0), &scope, Reporting::Silent);
        let second = analyzer.subquery_columns(nth_subquery(&stmt, 1), &scope, Reporting::Silent);
        analyzer.subquery_columns(nth_subquery(&stmt, 1), &scope, Reporting::Silent);
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(lookups(), [1, 1, 1]);
    }

    #[test]
    fn an_unrooted_analyzer_looks_up_by_value() {
        let schema = load_schema(DDL, "postgres");
        let Statement::CreateView(view) =
            statement("CREATE VIEW v AS SELECT (SELECT 1)", SqlDialect::Postgres)
        else {
            panic!("not a view");
        };
        reset();
        let columns = view_columns(&schema, SqlDialect::Postgres, &view);
        assert_eq!(columns.len(), 1);
        let [nodes, _, misses] = lookups();
        assert_eq!((nodes, misses), (0, 1));
    }

    #[test]
    fn a_statement_without_subqueries_builds_no_set() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT id FROM a WHERE x = 'q'", SqlDialect::Postgres);
        let (_, _, analyzer) = run(&stmt, &schema, true);
        assert!(!analyzer.stable.built());
        assert_eq!(lookups().iter().sum::<usize>(), 0);
    }
}
