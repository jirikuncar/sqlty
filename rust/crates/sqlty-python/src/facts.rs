//! What one walk of a module finds, shared by every check that reads its calls and strings.

use std::cell::OnceCell;
use std::collections::HashSet;

use ruff_python_ast::{Expr, ExprCall};

use crate::arg_types::{Project, SourceTypes};
use crate::scopes::Scan;
use crate::types::symbols::Source;

/// A parsed module and, once something asks, the one [`Scan`] of its tree.
pub struct ModuleFacts<'a> {
    /// The module.
    pub source: &'a Source,
    scan: OnceCell<Scan<'a>>,
}

impl<'a> ModuleFacts<'a> {
    /// The facts of `source`, not yet walked.
    pub fn new(source: &'a Source) -> Self {
        Self {
            source,
            scan: OnceCell::new(),
        }
    }

    /// The walk of the module, made on first use.
    pub fn scan(&self) -> &Scan<'a> {
        self.scan.get_or_init(|| Scan::new(self.source.body()))
    }

    /// The types of the values the module passes, names of `project` included.
    pub fn arg_types<'s>(&'s self, project: Option<Project<'s>>) -> SourceTypes<'a, 's> {
        SourceTypes::new(
            Some(self.source),
            &self.source.imports,
            self.scan(),
            project,
        )
    }

    /// `ParsedSource.literals`: the string literals that are not an operand of `+`, and the
    /// constant parts of f-strings and t-strings.
    pub(crate) fn literals(&self) -> HashSet<String> {
        let text = &*self.source.text;
        let mut found = HashSet::new();
        for &(expr, operand) in &self.scan().strings {
            match expr {
                Expr::StringLiteral(literal) if !operand => {
                    found.insert(crate::parse::text(literal, text));
                }
                Expr::FString(_) | Expr::TString(_) => {
                    found.extend(crate::unparse::constants(expr, text));
                }
                _ => {}
            }
        }
        found
    }

    /// Every call of the module, in visit order.
    pub(crate) fn calls(&self) -> impl Iterator<Item = &'a ExprCall> + '_ {
        self.scan().calls.iter().map(|&(call, _)| call)
    }
}
