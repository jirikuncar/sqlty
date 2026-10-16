//! Why a name, function, type or construct could not be resolved while inferring a query.
//!
//! The analyzer records a [`NameDiagnostic`] into a `Diagnostics` sink whenever a result
//! column ends up untyped, so every untyped column has a reason. The sink is shared by the
//! analyzers of subqueries and CTEs; a catalog-based schema loader can record into it too.

use std::cell::RefCell;
use std::rc::Rc;

/// The lint rule a diagnostic belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameRule {
    /// A table the schema does not define.
    UnknownTable,
    /// A column the table does not have.
    UnknownColumn,
    /// An unqualified column more than one table has.
    AmbiguousColumn,
    /// A function whose result sqlty cannot type.
    UnknownFunction,
    /// A cast or column type without a Python type.
    UnknownType,
    /// A construct that parses but is not typed.
    UnsupportedSyntax,
}

impl NameRule {
    /// Every rule, the one list the checker and the Python side read.
    pub const ALL: [NameRule; 6] = [
        NameRule::UnknownTable,
        NameRule::UnknownColumn,
        NameRule::AmbiguousColumn,
        NameRule::UnknownFunction,
        NameRule::UnknownType,
        NameRule::UnsupportedSyntax,
    ];

    /// The kebab-case rule name, as in `[tool.sqlty.rules]`.
    pub fn name(self) -> &'static str {
        match self {
            NameRule::UnknownTable => "unknown-table",
            NameRule::UnknownColumn => "unknown-column",
            NameRule::AmbiguousColumn => "ambiguous-column",
            NameRule::UnknownFunction => "unknown-function",
            NameRule::UnknownType => "unknown-type",
            NameRule::UnsupportedSyntax => "unsupported-syntax",
        }
    }
}

/// One reason a table, column, function, type or construct is not typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameDiagnostic {
    /// The rule the diagnostic belongs to.
    pub rule: NameRule,
    /// What could not be resolved and why, for the user.
    pub message: String,
}

#[derive(Default)]
struct Inner {
    /// Whether anything is collected; the analyzer skips the work of explaining otherwise.
    enabled: bool,
    items: Vec<NameDiagnostic>,
    /// Untyped results accounted for so far, recorded or deliberately silent.
    explained: usize,
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

    /// Whether diagnostics are collected. Guard the building of messages with it.
    pub(crate) fn enabled(&self) -> bool {
        self.0.borrow().enabled
    }

    /// Record a diagnostic (once per rule and message).
    pub(crate) fn record(&self, rule: NameRule, message: impl Into<String>) {
        let mut inner = self.0.borrow_mut();
        if !inner.enabled {
            return;
        }
        let item = NameDiagnostic {
            rule,
            message: message.into(),
        };
        inner.explained += 1;
        if !inner.items.contains(&item) {
            inner.items.push(item);
        }
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
        inner.items.truncate(items);
        inner.explained = explained;
    }

    /// Everything recorded so far, in order.
    pub(crate) fn items(&self) -> Vec<NameDiagnostic> {
        self.0.borrow().items.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(rule: NameRule, message: &str) -> NameDiagnostic {
        NameDiagnostic {
            rule,
            message: message.to_string(),
        }
    }

    #[test]
    fn identical_diagnostics_are_recorded_once() {
        let diag = Diagnostics::collecting();
        diag.record(NameRule::UnknownTable, "unknown table t");
        diag.record(NameRule::UnknownTable, "unknown table t");
        assert_eq!(
            diag.items(),
            [found(NameRule::UnknownTable, "unknown table t")]
        );
    }

    #[test]
    fn same_message_under_another_rule_or_another_message_is_kept() {
        let diag = Diagnostics::collecting();
        diag.record(NameRule::UnknownTable, "x");
        diag.record(NameRule::UnknownColumn, "x");
        diag.record(NameRule::UnknownTable, "y");
        assert_eq!(
            diag.items(),
            [
                found(NameRule::UnknownTable, "x"),
                found(NameRule::UnknownColumn, "x"),
                found(NameRule::UnknownTable, "y"),
            ]
        );
    }

    #[test]
    fn items_keep_the_order_of_recording() {
        let diag = Diagnostics::collecting();
        diag.record(NameRule::UnknownType, "b");
        diag.record(NameRule::UnknownFunction, "a");
        let rules: Vec<_> = diag.items().into_iter().map(|d| d.rule).collect();
        assert_eq!(rules, [NameRule::UnknownType, NameRule::UnknownFunction]);
    }

    #[test]
    fn default_sink_is_disabled_and_collects_nothing() {
        let diag = Diagnostics::default();
        assert!(!diag.enabled());
        diag.record(NameRule::UnknownTable, "t");
        diag.explain_silently();
        assert!(diag.items().is_empty());
        assert_eq!(diag.explained(), 0);
    }

    #[test]
    fn rewind_drops_what_was_recorded_after_the_mark() {
        let diag = Diagnostics::collecting();
        diag.record(NameRule::UnknownType, "kept");
        let mark = diag.mark();
        diag.record(NameRule::UnknownType, "dropped");
        diag.explain_silently();
        diag.rewind(mark);
        assert_eq!(diag.items(), [found(NameRule::UnknownType, "kept")]);
        assert_eq!(diag.explained(), 1);
    }

    #[test]
    fn collecting_sink_is_enabled() {
        assert!(Diagnostics::collecting().enabled());
    }

    #[test]
    fn explained_counts_recorded_duplicates_and_silent_results() {
        let diag = Diagnostics::collecting();
        assert_eq!(diag.explained(), 0);
        diag.record(NameRule::UnknownColumn, "c");
        assert_eq!(diag.explained(), 1);
        diag.record(NameRule::UnknownColumn, "c");
        assert_eq!(diag.explained(), 2, "a duplicate still explains a result");
        diag.explain_silently();
        assert_eq!(diag.explained(), 3);
        assert_eq!(diag.items().len(), 1);
    }

    #[test]
    fn clones_record_into_the_same_list() {
        let diag = Diagnostics::collecting();
        let inner = diag.clone();
        inner.record(NameRule::AmbiguousColumn, "id");
        inner.explain_silently();
        assert_eq!(diag.items(), [found(NameRule::AmbiguousColumn, "id")]);
        assert_eq!(diag.explained(), 2);
    }

    #[test]
    fn all_lists_every_rule_with_its_name() {
        let names: Vec<_> = NameRule::ALL.iter().map(|rule| rule.name()).collect();
        assert_eq!(
            names,
            [
                "unknown-table",
                "unknown-column",
                "ambiguous-column",
                "unknown-function",
                "unknown-type",
                "unsupported-syntax",
            ]
        );
    }
}
