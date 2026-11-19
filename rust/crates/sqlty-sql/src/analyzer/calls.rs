//! One evaluation of a function's inference rule per call node of a projected column.
//!
//! The type walk and the nullability walk of a column both ask which rule a call follows, and
//! finding it types the call's arguments. A frame holds the answers for one projected item, so
//! the second walk reads them instead of typing the arguments again.
//!
//! An answer is reused only where evaluating again could not give another: the same node of the
//! analyzed statement, under the same scope object, schema, dialect, diagnostics sink, outer
//! scope, merged columns, facts and virtual tables. An evaluation that recorded, rewound or
//! explained anything is not kept, so a second one still records what the first did.

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::dialect::SqlDialect;
use super::functions::RuleArguments;
use super::inference::FunctionRule;
use super::memo::Stable;
use super::nullability::Facts;
use super::query::Analyzer;
use super::scope::{Scope, Virtuals};
use crate::diagnostics::Diagnostics;

/// The rule found for one call node, and the call it was found for.
#[derive(Clone)]
pub(super) struct Cached {
    pub(super) name: String,
    pub(super) args: usize,
    pub(super) found: Option<(&'static FunctionRule, RuleArguments)>,
}

/// The context one projected item is analyzed in, and what was found in it.
pub(super) struct Frame {
    scope: usize,
    schema: usize,
    dialect: SqlDialect,
    diag: Diagnostics,
    outer: Rc<Scope>,
    merged: Rc<[String]>,
    facts: Facts,
    tables: Virtuals,
    stable: Rc<Stable>,
    pub(super) rules: FxHashMap<usize, Cached>,
}

/// Puts the frame that was current back when it drops.
pub(super) struct FrameGuard {
    cell: Rc<RefCell<Option<Frame>>>,
    previous: Option<Frame>,
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        *self.cell.borrow_mut() = self.previous.take();
    }
}

impl Analyzer<'_> {
    /// Start the frame of a projected item analyzed in `scope`, which must stay borrowed for
    /// as long as the guard lives.
    pub(super) fn enter_frame(&self, scope: &Scope) -> FrameGuard {
        let frame = Frame {
            scope: std::ptr::from_ref(scope) as usize,
            schema: std::ptr::from_ref(self.schema) as usize,
            dialect: self.dialect,
            diag: self.diag.clone(),
            outer: Rc::clone(&self.outer),
            merged: Rc::clone(&self.merged),
            facts: self.facts.clone(),
            tables: self.virtuals.clone(),
            stable: Rc::clone(&self.stable),
            rules: FxHashMap::default(),
        };
        FrameGuard {
            cell: Rc::clone(&self.calls),
            previous: self.calls.borrow_mut().replace(frame),
        }
    }

    /// Whether the current frame was made for this very context.
    pub(super) fn frame_matches(&self, scope: &Scope) -> bool {
        self.calls.borrow().as_ref().is_some_and(|frame| {
            std::ptr::from_ref(scope) as usize == frame.scope
                && std::ptr::from_ref(self.schema) as usize == frame.schema
                && self.dialect == frame.dialect
                && self.diag.same(&frame.diag)
                && Rc::ptr_eq(&self.outer, &frame.outer)
                && Rc::ptr_eq(&self.merged, &frame.merged)
                && Rc::ptr_eq(&self.stable, &frame.stable)
                && self.facts == frame.facts
                && self.virtuals.same(&frame.tables)
        })
    }

    /// The rule kept for the call node at `key`, when it is for this `name` and argument count.
    pub(super) fn cached_rule(&self, key: usize, name: &str, args: usize) -> Option<Cached> {
        let calls = self.calls.borrow();
        let cached = calls.as_ref()?.rules.get(&key)?;
        (cached.name == name && cached.args == args).then(|| cached.clone())
    }

    /// Keep the rule found for the call node at `key` in the current frame.
    pub(super) fn keep_rule(&self, key: usize, cached: Cached) {
        if let Some(frame) = self.calls.borrow_mut().as_mut() {
            frame.rules.insert(key, cached);
        }
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::ast::{Expr, Function, SelectItem, SetExpr, Statement};

    use super::super::functions::{function_name, positional_args, EVALUATIONS};
    use super::super::test_support::statement;
    use super::super::{explain_select, load_schema, SchemaData};
    use super::*;

    const DDL: &str = "CREATE TABLE t (name TEXT NOT NULL, a INT, b INT NOT NULL, x TEXT)";

    fn evaluations() -> usize {
        EVALUATIONS.with(std::cell::Cell::get)
    }

    fn reset() {
        EVALUATIONS.with(|count| count.set(0));
    }

    fn call_at(stmt: &Statement, n: usize) -> &Function {
        let Statement::Query(query) = stmt else {
            panic!("not a query");
        };
        let SetExpr::Select(select) = &*query.body else {
            panic!("not a select");
        };
        match &select.projection[n] {
            SelectItem::UnnamedExpr(Expr::Function(func)) => func,
            other => panic!("not a call: {other:?}"),
        }
    }

    /// The columns and diagnostics `sql` gives, and how many rules were evaluated for them.
    fn run(sql: &str, rooted: bool) -> (String, String, usize) {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement(sql, SqlDialect::Postgres);
        let mut analyzer = Analyzer::new(&schema, SqlDialect::Postgres);
        if rooted {
            analyzer = analyzer.rooted(&stmt);
        }
        analyzer.diag = Diagnostics::collecting();
        reset();
        let Ok(columns) = analyzer.statement_columns(&stmt);
        let found = format!("{:?}", analyzer.diag.located());
        (format!("{columns:?}"), found, evaluations())
    }

    /// The same answer as an analysis that finds no node by address, and the evaluations of both.
    fn same_as_uncached(sql: &str) -> (usize, usize) {
        let (columns, found, cached) = run(sql, true);
        let (expected, expected_found, uncached) = run(sql, false);
        assert_eq!((columns, found), (expected, expected_found), "{sql}");
        (cached, uncached)
    }

    #[test]
    fn a_rule_is_evaluated_once_per_call_node_with_arguments() {
        let (cached, uncached) =
            same_as_uncached("SELECT lower(upper(name)), coalesce(a, b) FROM t");
        assert_eq!(cached, 3);
        assert!(uncached > cached, "{uncached} without the frame");
        let (cached, uncached) = same_as_uncached("SELECT count(*) FROM t");
        assert_eq!(cached, uncached, "a call without arguments has no key");
    }

    #[test]
    fn explained_columns_evaluate_a_rule_once_too() {
        let schema = load_schema(DDL, "postgres");
        let sql = "SELECT lower(upper(name)), coalesce(a, b) FROM t";
        reset();
        let rules = explain_select(sql, "postgres", Some(&schema)).unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().all(|column| column.type_rule.is_some()));
        assert_eq!(evaluations(), 3);
    }

    #[test]
    fn a_call_that_recorded_is_evaluated_again() {
        for sql in [
            "SELECT coalesce(nope(x), 1) FROM t",
            "SELECT upper(unknown_col) FROM t",
        ] {
            let (cached, uncached) = same_as_uncached(sql);
            assert!(cached >= 2, "{sql}: {cached} evaluations");
            assert!(cached <= uncached, "{sql}");
            let (_, found, _) = run(sql, true);
            assert_ne!(found, "[]", "{sql}");
        }
    }

    #[test]
    fn calls_in_other_contexts_give_the_same_answer() {
        for sql in [
            "SELECT CASE lower(name) WHEN 'a' THEN upper(x) ELSE lower(x) END FROM t",
            "SELECT CASE WHEN a IS NOT NULL THEN abs(a) ELSE abs(b) END FROM t",
            "SELECT (SELECT lower(t.name) FROM t AS u LIMIT 1), upper(name) FROM t",
            "SELECT coalesce((SELECT max(lower(name)) FROM t), 'x'), lower(name) FROM t",
            "SELECT lower(name) FROM t WHERE upper(name) = 'A' AND abs(a) > 1",
        ] {
            same_as_uncached(sql);
        }
    }

    fn rooted<'a>(schema: &'a SchemaData, stmt: &'a Statement) -> Analyzer<'a> {
        let mut analyzer = Analyzer::new(schema, SqlDialect::Postgres).rooted(stmt);
        analyzer.diag = Diagnostics::collecting();
        analyzer
    }

    #[test]
    fn a_frame_serves_the_rule_of_a_call_node_again() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT lower('a'), lower('a')", SqlDialect::Postgres);
        let analyzer = rooted(&schema, &stmt);
        let scope = Scope::new();
        let func = call_at(&stmt, 0);
        let args = positional_args(&func.args);
        let name = function_name(func);

        assert!(!analyzer.frame_matches(&scope));
        reset();
        let before = analyzer.function_rule(&name, &args, &scope).unwrap();
        assert_eq!(evaluations(), 1);
        let _frame = analyzer.enter_frame(&scope);
        let first = analyzer.function_rule(&name, &args, &scope).unwrap();
        let again = analyzer.function_rule(&name, &args, &scope).unwrap();
        assert_eq!(evaluations(), 2);
        assert!(first.is_some() && again.is_some() && before.is_some());

        analyzer.function_rule("UPPER", &args, &scope).unwrap();
        assert_eq!(evaluations(), 3, "another name is evaluated");
        analyzer.function_rule(&name, &args, &scope).unwrap();
        assert_eq!(evaluations(), 4, "and replaces the entry");
        let other = positional_args(&call_at(&stmt, 1).args);
        analyzer.function_rule(&name, &other, &scope).unwrap();
        assert_eq!(evaluations(), 5, "another node is evaluated");
        analyzer.function_rule(&name, &[], &scope).unwrap();
        assert_eq!(evaluations(), 6, "no argument is no key");
        analyzer.function_rule(&name, &[], &scope).unwrap();
        assert_eq!(evaluations(), 7);
        let null = Expr::value(sqlparser::ast::Value::Null);
        analyzer.function_rule(&name, &[&null], &scope).unwrap();
        analyzer.function_rule(&name, &[&null], &scope).unwrap();
        assert_eq!(evaluations(), 9, "a temporary is no stable node");
    }

    #[test]
    fn a_rule_without_an_entry_for_its_argument_count_is_evaluated() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT round(1.5, 1)", SqlDialect::Postgres);
        let analyzer = rooted(&schema, &stmt);
        let scope = Scope::new();
        let func = call_at(&stmt, 0);
        let args = positional_args(&func.args);
        let _frame = analyzer.enter_frame(&scope);
        reset();
        analyzer.function_rule("ROUND", &args, &scope).unwrap();
        analyzer.function_rule("ROUND", &args[..1], &scope).unwrap();
        assert_eq!(evaluations(), 2);
    }

    #[test]
    fn a_name_without_rules_is_kept_as_no_rule() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT nope_fn(1)", SqlDialect::Postgres);
        let analyzer = rooted(&schema, &stmt);
        let scope = Scope::new();
        let func = call_at(&stmt, 0);
        let args = positional_args(&func.args);
        let _frame = analyzer.enter_frame(&scope);
        reset();
        assert!(analyzer
            .function_rule("NOPE_FN", &args, &scope)
            .unwrap()
            .is_none());
        assert!(analyzer
            .function_rule("NOPE_FN", &args, &scope)
            .unwrap()
            .is_none());
        assert_eq!(evaluations(), 1);
    }

    #[test]
    fn a_frame_is_for_one_context() {
        let schema = load_schema(DDL, "postgres");
        let other_schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT lower('a')", SqlDialect::Postgres);
        let analyzer = rooted(&schema, &stmt);
        let scope = Scope::new();
        let _frame = analyzer.enter_frame(&scope);
        assert!(analyzer.frame_matches(&scope));
        assert!(analyzer.clone().frame_matches(&scope));
        assert!(
            !analyzer.frame_matches(&Scope::new()),
            "another scope object"
        );

        let mut changed = analyzer.clone();
        changed.schema = &other_schema;
        assert!(!changed.frame_matches(&scope), "another schema");
        let mut changed = analyzer.clone();
        changed.dialect = SqlDialect::MySql;
        assert!(!changed.frame_matches(&scope), "another dialect");
        let mut changed = analyzer.clone();
        changed.diag = Diagnostics::default();
        assert!(!changed.frame_matches(&scope), "a silent analysis");
        let mut changed = analyzer.clone();
        changed.outer = Rc::default();
        assert!(!changed.frame_matches(&scope), "another outer scope");
        let mut changed = analyzer.clone();
        changed.merged = Rc::default();
        assert!(!changed.frame_matches(&scope), "other merged columns");
        let mut changed = analyzer.clone();
        changed.stable = Rc::default();
        assert!(!changed.frame_matches(&scope), "another statement");
        let mut changed = analyzer.clone();
        changed.facts.lockstep = true;
        assert!(!changed.frame_matches(&scope), "other facts");
        let mut changed = analyzer.clone();
        changed.virtuals.insert("v".to_string(), Vec::new());
        assert!(!changed.frame_matches(&scope), "other virtual tables");
    }

    #[test]
    fn a_nested_frame_gives_the_outer_one_back() {
        let schema = load_schema(DDL, "postgres");
        let stmt = statement("SELECT lower('a'), upper('b')", SqlDialect::Postgres);
        let analyzer = rooted(&schema, &stmt);
        let (outer, inner) = (Scope::new(), Scope::new());
        let func = call_at(&stmt, 0);
        let args = positional_args(&func.args);
        let name = function_name(func);

        let _frame = analyzer.enter_frame(&outer);
        analyzer.function_rule(&name, &args, &outer).unwrap();
        reset();
        {
            let _nested = analyzer.enter_frame(&inner);
            assert!(!analyzer.frame_matches(&outer));
            analyzer.function_rule(&name, &args, &outer).unwrap();
            assert_eq!(evaluations(), 1, "the nested frame has no entry for it");
        }
        assert!(analyzer.frame_matches(&outer));
        analyzer.function_rule(&name, &args, &outer).unwrap();
        assert_eq!(evaluations(), 1, "the outer frame serves it again");
        drop(_frame);
        assert!(
            !analyzer.frame_matches(&outer),
            "no frame after the last guard"
        );
    }
}
