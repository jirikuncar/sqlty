//! Why a name, function, type or construct could not be resolved while inferring a query.
//!
//! The analyzer records a [`NameDiagnostic`] into a [`Diagnostics`] sink whenever a result
//! column ends up untyped, so every untyped column has a reason. The sink is shared by the
//! analyzers of subqueries and CTEs; a catalog-based schema loader can record into it too.

use std::cell::RefCell;
use std::rc::Rc;

/// The lint rule a diagnostic belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameRule {
    UnknownTable,
    UnknownColumn,
    AmbiguousColumn,
    UnknownFunction,
    UnknownType,
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
    pub rule: NameRule,
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
pub struct Diagnostics(Rc<RefCell<Inner>>);

impl Diagnostics {
    /// A sink that collects; the default one drops everything, and callers that only want
    /// types pay nothing for diagnostics.
    pub fn collecting() -> Self {
        Diagnostics(Rc::new(RefCell::new(Inner {
            enabled: true,
            ..Inner::default()
        })))
    }

    /// Whether diagnostics are collected. Guard the building of messages with it.
    pub fn enabled(&self) -> bool {
        self.0.borrow().enabled
    }

    /// Record a diagnostic (once per rule and message).
    pub fn record(&self, rule: NameRule, message: impl Into<String>) {
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
    pub fn explain_silently(&self) {
        let mut inner = self.0.borrow_mut();
        if inner.enabled {
            inner.explained += 1;
        }
    }

    /// How many untyped results are accounted for; unchanged across an expression
    /// that went untyped means nothing explains it.
    pub fn explained(&self) -> usize {
        self.0.borrow().explained
    }

    /// Everything recorded so far, in order.
    pub fn items(&self) -> Vec<NameDiagnostic> {
        self.0.borrow().items.clone()
    }
}
