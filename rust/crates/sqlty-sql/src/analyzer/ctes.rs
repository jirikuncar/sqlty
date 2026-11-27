//! `unused-cte`: a CTE that neither a later CTE nor the statement reads.

use std::ops::ControlFlow;

use rustc_hash::FxHashMap;
use sqlparser::ast::{ObjectName, Query, SetExpr, Statement, TableFactor, Visit, Visitor};
use sqlparser::tokenizer::Span;

use super::dialect::SqlDialect;
use crate::catalog::fold;

/// The name and span of each CTE of `stmt` that nothing reads, innermost first. A
/// data-modifying CTE runs unread; a recursive one reading only itself never runs.
pub(super) fn unused_ctes(stmt: &Statement, dialect: SqlDialect) -> Vec<(String, Span)> {
    let mut walk = Walk {
        dialect,
        frames: Vec::new(),
        scope: FxHashMap::default(),
        function: false,
        unused: Vec::new(),
    };
    let _ = stmt.visit(&mut walk);
    walk.unused
}

struct Entry {
    name: String,
    /// The body, to know when the walk enters it.
    query: *const Query,
    span: Span,
    modifies: bool,
    read: bool,
}

struct Frame {
    ctes: Vec<Entry>,
    /// How many of `ctes` the query being walked sees.
    visible: usize,
    /// The CTE whose body the walk enters next, or is in.
    next: usize,
    /// Whether the walk is in the body of `next`.
    inside: bool,
}

struct Walk {
    dialect: SqlDialect,
    frames: Vec<Frame>,
    /// The visible CTEs of each name, innermost last, as (frame, CTE) indices.
    scope: FxHashMap<String, Vec<(usize, usize)>>,
    /// The relation visited next names a table function, not a CTE.
    function: bool,
    unused: Vec<(String, Span)>,
}

impl Walk {
    fn read(&mut self, name: &str) {
        if let Some(&(frame, cte)) = self.scope.get(name).and_then(|found| found.last()) {
            let frame = &mut self.frames[frame];
            // A recursive CTE reading itself runs only when something else reads it.
            if !(frame.inside && frame.next == cte) {
                frame.ctes[cte].read = true;
            }
        }
    }

    /// Makes the first `visible` CTEs of the innermost `WITH` visible.
    fn show(&mut self, visible: usize) {
        let at = self.frames.len() - 1;
        let frame = &mut self.frames[at];
        for cte in frame.visible..visible {
            let name = frame.ctes[cte].name.clone();
            self.scope.entry(name).or_default().push((at, cte));
        }
        frame.visible = frame.visible.max(visible);
    }

    /// Makes the CTEs of the enclosing `WITH` visible to `query` if it is the CTE body
    /// walked next, `after` it is walked or before.
    fn enter(&mut self, query: &Query, after: bool) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let next = frame.next;
        if frame
            .ctes
            .get(next)
            .is_some_and(|cte| std::ptr::eq(cte.query, query))
        {
            frame.inside = !after;
            frame.next += usize::from(after);
            self.show(next + usize::from(after));
        }
    }

    /// `TABLE name`, which sqlparser does not visit as a relation.
    fn tables(&mut self, body: &SetExpr) {
        match body {
            SetExpr::Table(table) if table.schema_name.is_none() => {
                if let Some(name) = &table.table_name {
                    let name = fold(name, self.dialect);
                    self.read(&name);
                }
            }
            SetExpr::SetOperation { left, right, .. } => {
                self.tables(left);
                self.tables(right);
            }
            _ => {}
        }
    }
}

impl Visitor for Walk {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        self.enter(query, false);
        if let Some(with) = &query.with {
            let ctes = with.cte_tables.iter().map(|cte| Entry {
                name: fold(&cte.alias.name, self.dialect),
                query: &*cte.query,
                span: cte.alias.name.span,
                modifies: matches!(
                    *cte.query.body,
                    SetExpr::Insert(_)
                        | SetExpr::Update(_)
                        | SetExpr::Delete(_)
                        | SetExpr::Merge(_)
                ),
                read: false,
            });
            let ctes: Vec<Entry> = ctes.collect();
            let all = ctes.len();
            self.frames.push(Frame {
                ctes,
                visible: 0,
                next: 0,
                inside: false,
            });
            // SQLite lets any CTE of a `WITH` read any other, `RECURSIVE` or not.
            if with.recursive || self.dialect == SqlDialect::Sqlite {
                self.show(all);
            }
        }
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        self.tables(&query.body);
        if query.with.is_some() {
            if let Some(frame) = self.frames.pop() {
                for cte in &frame.ctes[..frame.visible] {
                    if let Some(found) = self.scope.get_mut(&cte.name) {
                        found.pop();
                    }
                }
                let unused = frame
                    .ctes
                    .into_iter()
                    .filter(|cte| !cte.read && !cte.modifies);
                self.unused.extend(unused.map(|cte| (cte.name, cte.span)));
            }
        }
        self.enter(query, true);
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        self.function = matches!(factor, TableFactor::Table { args: Some(_), .. });
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
        if std::mem::take(&mut self.function) {
            return ControlFlow::Continue(());
        }
        if let [part] = relation.0.as_slice() {
            if let Some(ident) = part.as_ident() {
                let name = fold(ident, self.dialect);
                self.read(&name);
            }
        }
        ControlFlow::Continue(())
    }
}
