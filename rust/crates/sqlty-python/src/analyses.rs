//! One parse and one analysis of each query of a module, shared by the checks that ask.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sqlty_sql::analyzer::SchemaData;
use sqlty_sql::analyzer::{parse, route, row_type, Column, Param, Parsed};
use sqlty_sql::diagnostics::NameDiagnostic;

use crate::options::Options;

/// What the checks ask of one query.
pub(crate) struct Analysis {
    pub(crate) parsed: Rc<Parsed>,
    /// The result columns; `None` when the query returns no rows or does not parse.
    pub(crate) columns: Option<Vec<Column>>,
    /// Why columns are untyped.
    pub(crate) diagnostics: Vec<NameDiagnostic>,
    pub(crate) row_type: Option<String>,
}

/// A query's parse, and what has been asked of it so far.
struct Slot {
    parsed: Rc<Parsed>,
    analysis: Option<Rc<Analysis>>,
    params: Option<Option<Vec<Param>>>,
}

/// The analyses of a module's queries, by their text, each computed on first ask.
pub(crate) struct Analyses<'a> {
    options: &'a Options<'a>,
    known: RefCell<HashMap<String, Slot>>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static ANALYZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static PARSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl<'a> Analyses<'a> {
    pub(crate) fn new(options: &'a Options<'a>) -> Self {
        Self {
            options,
            known: RefCell::default(),
        }
    }

    pub(crate) fn of(&self, query: &str) -> Rc<Analysis> {
        let parsed = self.parsed(query);
        if let Some(analysis) = self
            .known
            .borrow()
            .get(query)
            .and_then(|slot| slot.analysis.as_ref())
        {
            return Rc::clone(analysis);
        }
        #[cfg(test)]
        ANALYZED.with(|count| count.set(count.get() + 1));
        let (columns, diagnostics) = parsed.analyze_with_diagnostics(self.routed(query).1);
        let analysis = Rc::new(Analysis {
            row_type: columns.as_deref().and_then(row_type),
            parsed,
            columns,
            diagnostics,
        });
        if let Some(slot) = self.known.borrow_mut().get_mut(query) {
            slot.analysis = Some(Rc::clone(&analysis));
        }
        analysis
    }

    /// The parameters of `query`, found once; the query is not analyzed for its columns.
    pub(crate) fn params(&self, query: &str) -> Option<Vec<Param>> {
        let parsed = self.parsed(query);
        if let Some(params) = self
            .known
            .borrow()
            .get(query)
            .and_then(|slot| slot.params.as_ref())
        {
            return params.clone();
        }
        let params = parsed.params(self.routed(query).1);
        if let Some(slot) = self.known.borrow_mut().get_mut(query) {
            slot.params = Some(params.clone());
        }
        params
    }

    fn routed(&self, query: &str) -> (&'a str, Option<&'a SchemaData>) {
        route(query, self.options.dialect.name(), self.options.schema)
    }

    fn parsed(&self, query: &str) -> Rc<Parsed> {
        if let Some(slot) = self.known.borrow().get(query) {
            return Rc::clone(&slot.parsed);
        }
        #[cfg(test)]
        PARSED.with(|count| count.set(count.get() + 1));
        let parsed = Rc::new(parse(query, self.routed(query).0));
        self.known.borrow_mut().insert(
            query.to_string(),
            Slot {
                parsed: Rc::clone(&parsed),
                analysis: None,
                params: None,
            },
        );
        parsed
    }
}
