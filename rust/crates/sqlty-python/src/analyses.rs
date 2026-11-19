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

/// The analyses of a module's queries, by their text, each computed on first ask.
pub(crate) struct Analyses<'a> {
    options: &'a Options<'a>,
    known: RefCell<HashMap<String, Rc<Analysis>>>,
    params: RefCell<HashMap<String, Option<Vec<Param>>>>,
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
            params: RefCell::default(),
        }
    }

    pub(crate) fn of(&self, query: &str) -> Rc<Analysis> {
        if let Some(analysis) = self.known.borrow().get(query) {
            return Rc::clone(analysis);
        }
        #[cfg(test)]
        ANALYZED.with(|count| count.set(count.get() + 1));
        let (dialect, schema) = self.routed(query);
        let parsed = Rc::new(self.parse(query, dialect));
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

    /// The parameters of `query`, found once and not kept with a parse of their own: the
    /// tree of an analyzed query is reused, any other is dropped after use.
    pub(crate) fn params(&self, query: &str) -> Option<Vec<Param>> {
        if let Some(params) = self.params.borrow().get(query) {
            return params.clone();
        }
        let (dialect, schema) = self.routed(query);
        let analyzed = self.known.borrow().get(query).map(Rc::clone);
        let params = match analyzed {
            Some(analysis) => analysis.parsed.params(query, schema),
            None => self.parse(query, dialect).params(query, schema),
        };
        self.params
            .borrow_mut()
            .insert(query.to_string(), params.clone());
        params
    }

    fn routed(&self, query: &str) -> (&'a str, Option<&'a SchemaData>) {
        route(query, self.options.dialect.name(), self.options.schema)
    }

    fn parse(&self, query: &str, dialect: &str) -> Parsed {
        #[cfg(test)]
        PARSED.with(|count| count.set(count.get() + 1));
        parse(query, dialect)
    }
}
