//! What one walk of a module finds, shared by every check that reads its calls and strings.

use std::cell::OnceCell;
use std::collections::HashSet;

use ruff_python_ast::{Expr, ExprCall};

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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn literals_skip_operands_of_plus_and_read_f_and_t_strings() {
        let text = "a = 'x' + 'y'\nb = 'z'\nc = f'p{b}q'\nd = t'r{b}s'\ne = ('u' + 'v') + 'w'\n";
        let source = Source::new(text, (3, 14)).unwrap();
        let facts = ModuleFacts::new(&source);
        let expected: HashSet<String> = ["z", "p", "q", "r", "s"].map(String::from).into();
        assert_eq!(facts.literals(), expected);
        assert_eq!(facts.calls().count(), 0);
    }
}
