//! One parse and one analysis of each query of a module, shared by the checks that ask.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sqlty_sql::analyzer::{parse, route, row_type, Column, Parsed};
use sqlty_sql::diagnostics::NameDiagnostic;

use crate::options::Options;

/// What the checks ask of one query.
pub(crate) struct Analysis {
    pub(crate) parsed: Parsed,
    /// The result columns; `None` when the query returns no rows or does not parse.
    pub(crate) columns: Option<Vec<Column>>,
    /// Why columns are untyped.
    pub(crate) diagnostics: Vec<NameDiagnostic>,
    pub(crate) row_type: Option<String>,
}

/// The analyses of a module's queries, by their text, each computed on first ask.
pub(crate) struct Analyses<'a> {
    options: &'a Options<'a>,
    known: RefCell<HashMap<String, Rc<Analysis>>>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static ANALYZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl<'a> Analyses<'a> {
    pub(crate) fn new(options: &'a Options<'a>) -> Self {
        Self {
            options,
            known: RefCell::default(),
        }
    }

    pub(crate) fn of(&self, query: &str) -> Rc<Analysis> {
        if let Some(analysis) = self.known.borrow().get(query) {
            return Rc::clone(analysis);
        }
        #[cfg(test)]
        ANALYZED.with(|count| count.set(count.get() + 1));
        let (dialect, schema) = route(query, self.options.dialect.name(), self.options.schema);
        let parsed = parse(query, dialect);
        let (columns, diagnostics) = parsed.analyze_with_diagnostics(schema);
        let analysis = Rc::new(Analysis {
            row_type: columns.as_deref().and_then(row_type),
            parsed,
            columns,
            diagnostics,
        });
        self.known
            .borrow_mut()
            .insert(query.to_string(), Rc::clone(&analysis));
        analysis
    }
}
