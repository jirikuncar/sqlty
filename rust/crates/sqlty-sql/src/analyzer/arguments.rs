//! `argument-count-mismatch`: a call of a built-in function that none of its overloads takes.

use std::fmt::Write;
use std::ops::ControlFlow;

use sqlparser::ast::{
    visit_expressions, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, Spanned,
    Statement,
};

use super::names::table_key;
use super::query::Analyzer;
use crate::diagnostics::Rule;
use crate::signatures::{signatures, Signature};

impl Analyzer<'_> {
    /// Reports each call in `stmt` whose arguments fit no overload of its built-in.
    pub(super) fn check_argument_counts(&self, stmt: &Statement) {
        if !self.diag.enabled() {
            return;
        }
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
        let star = list.args.iter().any(|arg| {
            matches!(
                arg,
                FunctionArg::Unnamed(
                    FunctionArgExpr::Wildcard | FunctionArgExpr::QualifiedWildcard(_)
                )
            )
        });
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

/// How many arguments `overloads` take, from the fewest to the most: `1 or 2 arguments`.
fn takes(overloads: &[Signature]) -> String {
    let least = overloads.iter().map(|s| s.required).min().unwrap_or(0);
    let most = overloads
        .iter()
        .map(|s| (!s.variadic).then_some(s.parameters.len()))
        .try_fold(0, |most, len| Some(most.max(len?)));
    let counts = match most {
        None => format!("at least {least}"),
        Some(most) if most == least => most.to_string(),
        Some(most) if most == least + 1 => format!("{least} or {most}"),
        Some(most) => format!("{least} to {most}"),
    };
    let plural = if most.unwrap_or(least) == 1 { "" } else { "s" };
    format!("{counts} argument{plural}")
}
