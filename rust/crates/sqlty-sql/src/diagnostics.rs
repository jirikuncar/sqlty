//! Why a name, function, type or construct could not be resolved while inferring a query.
//!
//! The analyzer records a [`NameDiagnostic`] into a `Diagnostics` sink whenever a result
//! column ends up untyped, so every untyped column has a reason. The sink is shared by the
//! analyzers of subqueries and CTEs; a catalog-based schema loader can record into it too.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use sqlparser::tokenizer::Span;

pub use sqlty_rules::Rule;

/// One reason a table, column, function, type or construct is not typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameDiagnostic {
    /// The rule the diagnostic belongs to.
    pub rule: Rule,
    /// What could not be resolved and why, for the user.
    pub message: String,
    /// The bytes of the query it is about, when the analyzer knows where that is written:
    /// the name of an unknown table, column or function.
    pub span: Option<Range<usize>>,
    /// What to write at `span` to fix it: the closest column of an unknown one.
    pub replacement: Option<String>,
    /// The table it is about, as the query names it: the unknown table, or the one table an
    /// unknown column was looked up in.
    pub table: Option<String>,
    /// The column it is about: an unknown or ambiguous column, as the query spells it.
    pub column: Option<String>,
    /// A rule reporting the same mistake: where it is on, this diagnostic is not shown.
    pub shadowed_by: Option<Rule>,
}

impl NameDiagnostic {
    /// A diagnostic of `rule` saying `message`, about no name.
    pub(crate) fn new(rule: Rule, message: impl Into<String>) -> Self {
        found(rule, message.into())
    }
}

#[derive(Default)]
struct Inner {
    /// Whether anything is collected; the analyzer skips the work of explaining otherwise.
    enabled: bool,
    /// Each diagnostic with where sqlparser read its node, empty when unknown.
    items: Vec<(NameDiagnostic, Span)>,
    /// The rules of `items` by message, each pair recorded once.
    recorded: FxHashMap<String, Vec<Rule>>,
    /// The spans of the nodes being analyzed, innermost last; see [`Diagnostics::at`].
    at: Vec<Span>,
    /// Untyped results accounted for so far, recorded or deliberately silent.
    explained: usize,
    /// The columns the completion placeholder could name, once the analyzer reaches it.
    probe: Option<Vec<crate::analyzer::Candidate>>,
}

impl Inner {
    /// Drop the items from `start` on that `keep` rejects.
    fn drop_since(&mut self, start: usize, keep: impl Fn(Rule) -> bool) {
        if self
            .items
            .get(start..)
            .is_none_or(|tail| tail.iter().all(|(item, _)| keep(item.rule)))
        {
            return;
        }
        for (item, span) in self.items.split_off(start) {
            if keep(item.rule) {
                self.items.push((item, span));
            } else if self
                .recorded
                .get_mut(item.message.as_str())
                .is_none_or(|rules| {
                    rules.retain(|rule| *rule != item.rule);
                    rules.is_empty()
                })
            {
                self.recorded.remove(item.message.as_str());
            }
        }
    }
}

/// A shared, append-only list of diagnostics; clones record into the same list.
#[derive(Clone, Default)]
pub(crate) struct Diagnostics(Rc<RefCell<Inner>>);

impl Diagnostics {
    /// A sink that collects; the default one drops everything, and callers that only want
    /// types pay nothing for diagnostics.
    pub(crate) fn collecting() -> Self {
        Diagnostics(Rc::new(RefCell::new(Inner {
            enabled: true,
            ..Inner::default()
        })))
    }

    /// The columns [`Diagnostics::probe`] recorded, when the analyzer reached the placeholder.
    pub(crate) fn probed(&self) -> Option<Vec<crate::analyzer::Candidate>> {
        self.0.borrow_mut().probe.take()
    }

    /// Record what the completion placeholder could name; the first record wins.
    pub(crate) fn probe(&self, candidates: impl FnOnce() -> Vec<crate::analyzer::Candidate>) {
        let mut inner = self.0.borrow_mut();
        if inner.enabled && inner.probe.is_none() {
            inner.probe = Some(candidates());
        }
    }

    /// Whether diagnostics are collected. Guard the building of messages with it.
    pub(crate) fn enabled(&self) -> bool {
        self.0.borrow().enabled
    }

    /// Record a diagnostic (once per rule and message) at the node being analyzed.
    pub(crate) fn record(&self, rule: Rule, message: impl Into<String>) {
        self.record_about(rule, message, None, None);
    }

    /// [`Diagnostics::record`] naming the table and column it is about.
    pub(crate) fn record_about(
        &self,
        rule: Rule,
        message: impl Into<String>,
        table: Option<&str>,
        column: Option<&str>,
    ) {
        self.push(NameDiagnostic {
            table: table.map(str::to_string),
            column: column.map(str::to_string),
            ..found(rule, message.into())
        });
    }

    /// [`Diagnostics::record`], not shown where the rule `by` is on.
    pub(crate) fn record_shadowed(&self, rule: Rule, message: impl Into<String>, by: Rule) {
        self.push(NameDiagnostic {
            shadowed_by: Some(by),
            ..found(rule, message.into())
        });
    }

    /// Record `item` unless one of its rule and message is recorded already.
    pub(crate) fn push(&self, item: NameDiagnostic) {
        self.add(item, 1);
    }

    /// [`Diagnostics::push`] of a finding about a column that is typed, which explains
    /// no untyped result.
    pub(crate) fn warn(&self, item: NameDiagnostic) {
        self.add(item, 0);
    }

    fn add(&self, item: NameDiagnostic, explains: usize) {
        let mut inner = self.0.borrow_mut();
        if !inner.enabled {
            return;
        }
        inner.explained += explains;
        let new = match inner.recorded.get_mut(item.message.as_str()) {
            Some(rules) if rules.contains(&item.rule) => false,
            Some(rules) => {
                rules.push(item.rule);
                true
            }
            None => {
                inner.recorded.insert(item.message.clone(), vec![item.rule]);
                true
            }
        };
        if new {
            let at = inner.at.last().copied().unwrap_or(Span::empty());
            inner.items.push((item, at));
        }
    }

    /// Record at `span` until the guard drops: the name of the node being analyzed, or
    /// [`Span::empty`] for a node whose diagnostics are not about a name in it. `None` when
    /// nothing is collected.
    pub(crate) fn at(&self, span: Span) -> Option<At> {
        let mut inner = self.0.borrow_mut();
        inner.enabled.then(|| {
            inner.at.push(span);
            At(self.clone())
        })
    }

    /// Account for an untyped result whose reason is reported elsewhere or is not a
    /// problem to report (an unknown table already flagged, a missing schema).
    pub(crate) fn explain_silently(&self) {
        let mut inner = self.0.borrow_mut();
        if inner.enabled {
            inner.explained += 1;
        }
    }

    /// Account for `count` untyped results a memoized analysis explained when it first ran.
    pub(crate) fn add_explained(&self, count: usize) {
        let mut inner = self.0.borrow_mut();
        if inner.enabled {
            inner.explained += count;
        }
    }

    /// How many untyped results are accounted for; unchanged across an expression
    /// that went untyped means nothing explains it.
    pub(crate) fn explained(&self) -> usize {
        self.0.borrow().explained
    }

    /// Where the list stands, to [`Diagnostics::rewind`] to.
    pub(crate) fn mark(&self) -> (usize, usize) {
        let inner = self.0.borrow();
        (inner.items.len(), inner.explained)
    }

    /// Drop what was recorded after `mark`: an untyped part of an expression whose
    /// whole turned out typed needs no explanation.
    pub(crate) fn rewind(&self, (items, explained): (usize, usize)) {
        let mut inner = self.0.borrow_mut();
        inner.drop_since(items, |_| false);
        inner.explained = explained;
    }

    /// Drop what was recorded after `mark` under a rule `keep` rejects.
    pub(crate) fn retain_since(&self, (items, _): (usize, usize), keep: impl Fn(Rule) -> bool) {
        self.0.borrow_mut().drop_since(items, keep);
    }

    /// Everything recorded so far, in order, without spans.
    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn items(&self) -> Vec<NameDiagnostic> {
        self.located().into_iter().map(|(item, _)| item).collect()
    }

    /// Everything recorded so far, in order, with where sqlparser read it.
    pub(crate) fn located(&self) -> Vec<(NameDiagnostic, Span)> {
        self.0.borrow().items.clone()
    }
}

/// The span [`Diagnostics::at`] pushed, popped on drop.
pub(crate) struct At(Diagnostics);

impl Drop for At {
    fn drop(&mut self) {
        (self.0).0.borrow_mut().at.pop();
    }
}

/// A diagnostic of `rule` saying `message`, about no name.
fn found(rule: Rule, message: String) -> NameDiagnostic {
    NameDiagnostic {
        rule,
        message,
        span: None,
        replacement: None,
        table: None,
        column: None,
        shadowed_by: None,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn found(rule: Rule, message: &str) -> NameDiagnostic {
        super::found(rule, message.to_string())
    }

    #[test]
    fn identical_diagnostics_are_recorded_once() {
        let diag = Diagnostics::collecting();
        diag.record(Rule::UnknownTable, "unknown table t");
        diag.record(Rule::UnknownTable, "unknown table t");
        assert_eq!(diag.items(), [found(Rule::UnknownTable, "unknown table t")]);
    }

    #[test]
    fn same_message_under_another_rule_or_another_message_is_kept() {
        let diag = Diagnostics::collecting();
        diag.record(Rule::UnknownTable, "x");
        diag.record(Rule::UnknownColumn, "x");
        diag.record(Rule::UnknownTable, "y");
        assert_eq!(
            diag.items(),
            [
                found(Rule::UnknownTable, "x"),
                found(Rule::UnknownColumn, "x"),
                found(Rule::UnknownTable, "y"),
            ]
        );
    }

    #[test]
    fn items_keep_the_order_of_recording() {
        let diag = Diagnostics::collecting();
        diag.record(Rule::UnknownType, "b");
        diag.record(Rule::UnknownFunction, "a");
        let rules: Vec<_> = diag.items().into_iter().map(|d| d.rule).collect();
        assert_eq!(rules, [Rule::UnknownType, Rule::UnknownFunction]);
    }

    #[test]
    fn default_sink_is_disabled_and_collects_nothing() {
        let diag = Diagnostics::default();
        assert!(!diag.enabled());
        diag.record(Rule::UnknownTable, "t");
        diag.explain_silently();
        assert!(diag.items().is_empty());
        assert_eq!(diag.explained(), 0);
    }

    #[test]
    fn rewind_drops_what_was_recorded_after_the_mark() {
        let diag = Diagnostics::collecting();
        diag.record(Rule::UnknownType, "kept");
        let mark = diag.mark();
        diag.record(Rule::UnknownType, "dropped");
        diag.explain_silently();
        diag.rewind(mark);
        assert_eq!(diag.items(), [found(Rule::UnknownType, "kept")]);
        assert_eq!(diag.explained(), 1);
        diag.record(Rule::UnknownType, "dropped");
        let mark = diag.mark();
        diag.record(Rule::UnknownColumn, "dropped");
        diag.record(Rule::UnknownTable, "kept too");
        diag.retain_since(mark, |rule| rule == Rule::UnknownTable);
        diag.record(Rule::UnknownColumn, "dropped");
        let rules: Vec<_> = diag.items().into_iter().map(|d| d.rule).collect();
        assert_eq!(
            rules,
            [
                Rule::UnknownType,
                Rule::UnknownType,
                Rule::UnknownTable,
                Rule::UnknownColumn
            ],
            "what was dropped is recorded again"
        );
    }

    #[test]
    fn collecting_sink_is_enabled() {
        assert!(Diagnostics::collecting().enabled());
    }

    #[test]
    fn explained_counts_recorded_duplicates_and_silent_results() {
        let diag = Diagnostics::collecting();
        assert_eq!(diag.explained(), 0);
        diag.record(Rule::UnknownColumn, "c");
        assert_eq!(diag.explained(), 1);
        diag.record(Rule::UnknownColumn, "c");
        assert_eq!(diag.explained(), 2, "a duplicate still explains a result");
        diag.explain_silently();
        assert_eq!(diag.explained(), 3);
        assert_eq!(diag.items().len(), 1);
    }

    #[test]
    fn clones_record_into_the_same_list() {
        let diag = Diagnostics::collecting();
        let inner = diag.clone();
        inner.record(Rule::AmbiguousColumn, "id");
        inner.explain_silently();
        assert_eq!(diag.items(), [found(Rule::AmbiguousColumn, "id")]);
        assert_eq!(diag.explained(), 2);
    }

    #[test]
    fn sql_rules_list_every_name() {
        let names: Vec<_> = Rule::SQL.iter().map(|rule| rule.name()).collect();
        assert_eq!(
            names,
            [
                "unknown-table",
                "unknown-column",
                "deprecated-column",
                "ambiguous-column",
                "duplicate-alias",
                "unknown-function",
                "unknown-type",
                "unsupported-syntax",
                "insert-missing-column",
                "insert-value-count",
                "literal-write",
                "unmatched-literal",
                "ungrouped-column",
                "fanout-aggregate",
                "chasm-join",
                "unproven-join",
                "entity-mismatch",
                "foreign-key-mismatch",
                "tenant-scope",
                "rejected-query",
            ]
        );
    }
}
