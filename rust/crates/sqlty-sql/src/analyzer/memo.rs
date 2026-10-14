//! Memo of subquery analysis: each scalar subquery is analyzed once per context.
//!
//! The type, the nullability and the JSON shape of an expression are three walks over it, and
//! each of them needs the columns of a subquery the expression contains. Without a memo a
//! subquery nested `d` levels deep is analyzed `2^d` times.
//!
//! An entry is valid for one analysis context: the CTEs and derived tables in scope (the memo
//! belongs to a [`Virtuals`](super::scope::Virtuals), whose every change starts a fresh one),
//! the enclosing scopes, the merged columns and the facts. The query is part of the key by
//! value, not by address, because the analyzer builds and drops temporary expressions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use sqlparser::ast::Query;

use super::nullability::Facts;
use super::query::Analyzer;
use super::scope::Scope;
use super::{Column, Inferred};
use rustc_hash::FxHasher;

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
    fn same_context(&self, query: &Query, ctx: &Analyzer<'_>) -> bool {
        self.query == *query
            && self.merged == ctx.merged
            && (Rc::ptr_eq(&self.outer, &ctx.outer) || self.outer == ctx.outer)
            && self.facts == ctx.facts
    }
}

#[derive(Clone, Default)]
pub(super) struct SubqueryMemo(Rc<RefCell<HashMap<u64, Vec<Entry>>>>);

fn digest(query: &Query) -> u64 {
    let mut hasher = FxHasher::default();
    query.hash(&mut hasher);
    hasher.finish()
}

impl SubqueryMemo {
    fn get(&self, key: u64, query: &Query, ctx: &Analyzer<'_>) -> Option<(SubqueryColumns, usize)> {
        let wants_record = ctx.diag.enabled();
        let map = self.0.borrow();
        map.get(&key)?
            .iter()
            .find(|entry| (entry.recorded || !wants_record) && entry.same_context(query, ctx))
            .map(|entry| (Rc::clone(&entry.columns), entry.explained))
    }

    fn insert(&self, key: u64, query: &Query, ctx: &Analyzer<'_>, found: (SubqueryColumns, usize)) {
        let mut map = self.0.borrow_mut();
        let bucket = map.entry(key).or_default();
        // A recording entry replaces a silent one for the same context.
        bucket.retain(|old| old.recorded || !old.same_context(query, ctx));
        bucket.push(Entry {
            query: query.clone(),
            outer: Rc::clone(&ctx.outer),
            merged: Rc::clone(&ctx.merged),
            facts: ctx.facts.clone(),
            recorded: ctx.diag.enabled(),
            explained: found.1,
            columns: found.0,
        });
    }
}

impl<'a> Analyzer<'a> {
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
        let key = digest(query);
        if let Some((columns, explained)) = memo.get(key, query, &inner) {
            inner.diag.add_explained(explained);
            return columns;
        }
        let before = inner.diag.explained();
        let columns = Rc::new(inner.query_columns(query));
        let explained = inner.diag.explained() - before;
        memo.insert(key, query, &inner, (Rc::clone(&columns), explained));
        columns
    }
}

/// The first column of a subquery's result, when it has one.
pub(super) fn first_column(columns: &SubqueryColumns) -> Inferred<Option<&Column>> {
    match &**columns {
        Ok(columns) => Ok(columns.as_ref().and_then(|columns| columns.first())),
        Err(error) => Err(error.clone()),
    }
}
