//! Which inference rule typed each result column: `sqlty check --explain`.
//!
//! The analyzer records the rules of a projection's columns only when asked to, through
//! [`Analyzer::explain`]; otherwise it pays one branch per column.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sqlparser::ast::{BinaryOperator, Expr, Query, SelectItem, SetExpr, Statement};

use super::dialect::SqlDialect;
use super::dml::returning;
use super::functions::positional_args;
use super::inference::RuleRef;
use super::input::parser_input;
use super::operators::mysql_date_add;
use super::preprocess::parse_input;
use super::query::Analyzer;
use super::scope::Scope;
use super::SchemaData;

/// The rules of a result column: the one that gave its type and the one that gave its
/// nullability, when an inference rule did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnRules {
    /// The result column's name, as in `analyze_select`.
    pub name: String,
    /// The rule that gave the column's type.
    pub type_rule: Option<RuleRef>,
    /// The rule that decided whether the column can be NULL.
    pub nullability_rule: Option<RuleRef>,
}

/// The rules of each projection's columns, by the address of its select items.
pub(super) type Explained = Rc<RefCell<HashMap<usize, Vec<(Option<RuleRef>, Option<RuleRef>)>>>>;

/// The inference rules behind each result column of `sql`, in the order of
/// [`analyze_select`](super::analyze_select)'s columns; `None` when it returns no rows or
/// doesn't parse. A column the rules did not type (a column reference, a cast, a set
/// operation) has none.
///
/// `SELECT SUM(id::INT)` names `postgres.aggregate.sum-integers` for its type, and
/// `SELECT id` names no rule.
pub fn explain_select(
    sql: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Option<Vec<ColumnRules>> {
    let dialect = SqlDialect::from_name(dialect);
    let input = parser_input(sql, dialect);
    let stmt = parse_input(input.text(), dialect)?.into_iter().next()?;
    let mut analyzer = Analyzer::new(
        schema.unwrap_or_else(|| crate::catalog::empty(dialect)),
        dialect,
    );
    let explained = Explained::default();
    analyzer.explain = Some(explained.clone());
    analyzer.source = super::api::text_source(&input.source, dialect);
    let columns = analyzer.statement_columns(&stmt).ok()??;
    let items = match &stmt {
        Statement::Query(query) => top_projection(query),
        _ => returning(&stmt),
    };
    let explained = explained.borrow();
    let rules = items.and_then(|items| explained.get(&(items.as_ptr() as usize)));
    Some(
        columns
            .into_iter()
            .enumerate()
            .map(|(i, column)| {
                let (type_rule, nullability_rule) = rules
                    .and_then(|rules| rules.get(i).copied())
                    .unwrap_or_default();
                ColumnRules {
                    name: column.name,
                    type_rule,
                    nullability_rule,
                }
            })
            .collect(),
    )
}

/// The select items of a query's single `SELECT`; `None` for a set operation, whose
/// columns unify several.
fn top_projection(query: &Query) -> Option<&[SelectItem]> {
    match query.body.as_ref() {
        SetExpr::Select(select) => Some(&select.projection),
        SetExpr::Query(inner) => top_projection(inner),
        _ => None,
    }
}

impl Analyzer<'_> {
    /// The rules that typed `expr` and decided its nullability: those of its outermost
    /// operator or function call.
    pub(super) fn column_rules(
        &self,
        expr: &Expr,
        scope: &Scope,
    ) -> (Option<RuleRef>, Option<RuleRef>) {
        let rule = match expr {
            Expr::Nested(inner) => return self.column_rules(inner, scope),
            Expr::BinaryOp { left, op, right } => {
                let date_add = self.dialect == SqlDialect::MySql
                    && matches!(op, BinaryOperator::Plus | BinaryOperator::Minus)
                    && matches!(
                        (self.expr(left, scope), self.expr(right, scope)),
                        (Ok(l), Ok(r)) if mysql_date_add(op, (left, &l), (right, &r)).is_some()
                    );
                match self.binary_rule(left, op, right, scope) {
                    Ok(Some((rule, _))) if !date_add => Some(rule.explained()),
                    _ => None,
                }
            }
            Expr::UnaryOp { op, expr: inner } => match self.prefix_rule(op, inner, scope) {
                Ok(Some((rule, _))) => Some(rule.explained()),
                _ => None,
            },
            Expr::Function(func) => {
                let name = self.builtin_name(&func.name);
                let args = positional_args(&func.args);
                match self.function_rule(&name, &args, scope) {
                    Ok(Some((rule, _))) => Some(rule.explained()),
                    _ => None,
                }
            }
            _ => None,
        };
        (rule, rule)
    }
}
