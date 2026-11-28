//! `argument-count-mismatch`: a call of a built-in function that none of its overloads takes.

use std::fmt::Write;
use std::ops::ControlFlow;

use sqlparser::ast::{
    visit_expressions, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, Spanned,
    Statement,
};

use super::dialect::SqlDialect;
use super::names::table_key;
use super::query::Analyzer;
use crate::diagnostics::Rule;
use crate::signatures::{signatures, Signature};

impl Analyzer<'_> {
    /// Reports each call in `stmt` whose arguments fit no overload of its built-in.
    pub(super) fn check_argument_counts(&self, stmt: &Statement) {
        let _ = visit_expressions(stmt, |expr| {
            if let Expr::Function(func) = expr {
                self.check_arguments(func);
            }
            ControlFlow::<()>::Continue(())
        });
    }

    /// Reports `func` when its arguments fit no overload of its built-in.
    pub(super) fn check_arguments(&self, func: &Function) {
        let FunctionArguments::List(list) = &func.args else {
            return;
        };
        let name = self.builtin_name(&func.name);
        let overloads = signatures(self.dialect, &name);
        // `rank(x) WITHIN GROUP (ORDER BY y)` is the hypothetical-set form, which takes arguments.
        if overloads.is_empty()
            || name == "COUNT"
            || !func.within_group.is_empty()
            || list
                .args
                .iter()
                .any(|arg| !matches!(arg, FunctionArg::Unnamed(_)))
            || self
                .schema
                .defines_function(&table_key(&func.name, self.dialect))
        {
            return;
        }
        let star = |arg: &FunctionArg| match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => Some(true),
            FunctionArg::Unnamed(FunctionArgExpr::QualifiedWildcard(_)) => Some(false),
            _ => None,
        };
        // DuckDB expands a star into the columns it reads (`struct_pack(*)`); elsewhere `t.*` is
        // one row and `*` an argument of `count` only.
        if self.dialect == SqlDialect::Other("duckdb")
            && list.args.iter().any(|a| star(a).is_some())
        {
            return;
        }
        let star = list.args.iter().any(|arg| star(arg) == Some(true));
        let count = list.args.len();
        if !star && overloads.iter().any(|s| s.accepts(count)) {
            return;
        }
        let name = overloads[0].name;
        let mut message = if star {
            format!("`{name}` takes no `*`, only `count` does")
        } else {
            format!("`{name}` takes {}, got {count}", takes(overloads))
        };
        for (i, overload) in overloads.iter().enumerate() {
            let _ = write!(
                message,
                "{}{}",
                if i == 0 { ": " } else { "; " },
                overload.label
            );
        }
        let _at = self.diag.at(func.name.span());
        self.diag.record(Rule::ArgumentCountMismatch, message);
    }
}

/// How many arguments `overloads` take: `1 or 2 arguments`, `2 to 4 arguments`, `at least 1
/// argument`, `1, 3 or 6 arguments`.
fn takes(overloads: &[Signature]) -> String {
    let repeated = overloads
        .iter()
        .filter(|s| s.variadic)
        .map(|s| s.required)
        .min();
    let most = overloads
        .iter()
        .filter(|s| !s.variadic)
        .map(|s| s.parameters.len())
        .max()
        .unwrap_or(0);
    let counts: Vec<usize> = (0..=most)
        .filter(|&n| repeated.is_none_or(|least| n < least))
        .filter(|&n| overloads.iter().any(|s| s.accepts(n)))
        .collect();
    let mut words: Vec<String> = match counts.as_slice() {
        [first, .., last] if counts.len() > 2 && last - first + 1 == counts.len() => {
            vec![format!("{first} to {last}")]
        }
        _ => counts.iter().map(ToString::to_string).collect(),
    };
    words.extend(repeated.map(|least| format!("at least {least}")));
    let plural = if words == ["1"] || words == ["at least 1"] {
        ""
    } else {
        "s"
    };
    let text = match words.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    };
    format!("{text} argument{plural}")
}
