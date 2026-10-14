//! Expression typing.

use sqlparser::ast::{
    AccessExpr, ArrayElemTypeDef, CaseWhen, DataType, Expr, Function, FunctionArguments, Ident,
    ObjectName, Subscript, UnaryOperator, Value,
};

use crate::catalog::{fold, FunctionReturn};
use crate::diagnostics::NameRule;

use super::dialect::SqlDialect;
use super::functions::{positional_args, Returns};
use super::literals::{constant, strip, value_set};
use super::memo::{first_column, Reporting};
use super::names::{split_access, table_key};
use super::operators::{category, common_of, common_type, operand_type, type_base};
use super::query::{Analyzer, Column};
use super::scope::Scope;
use super::typed::Typed;
use super::{decimal_type, Inferred};

/// `INT` from `ARRAY<INT>` or `ARRAY<INT>[3]`.
pub(super) fn array_element(sql_type: &str) -> Option<&str> {
    let inner = sql_type.strip_prefix("ARRAY<")?;
    Some(&inner[..inner.rfind('>')?])
}

/// The element type of a range type such as `TSTZRANGE`.
pub(super) fn range_element(sql_type: &str) -> Option<&'static str> {
    Some(match type_base(sql_type) {
        "INT4RANGE" => "INT",
        "INT8RANGE" => "BIGINT",
        "NUMRANGE" => "DECIMAL",
        "TSRANGE" => "TIMESTAMP",
        "TSTZRANGE" => "TIMESTAMPTZ",
        "DATERANGE" => "DATE",
        _ => return None,
    })
}

pub(super) fn known(python_type: &str) -> Inferred<Typed> {
    Ok(Typed::python_only(python_type))
}

pub(super) fn from_sql_type(sql_type: &str, dialect: SqlDialect) -> Typed {
    Typed::of_sql(sql_type, dialect)
}

impl<'a> Analyzer<'a> {
    /// The element type of an array type; an enum element keeps its `Literal`.
    pub(super) fn element_typed(&self, array: Option<&str>) -> Typed {
        let Some(elem) = array.and_then(array_element) else {
            return Typed::UNKNOWN;
        };
        match self.schema.user_type(&elem.to_lowercase()) {
            Some(user) if user.python_type.is_some() => {
                Typed::new(user.python_type.clone(), Some(elem.to_string()))
            }
            _ => from_sql_type(elem, self.dialect),
        }
    }

    /// The type of `field` in the composite type `composite`, when the schema declares it.
    pub(super) fn field_typed(&self, composite: Option<&str>, field: &Ident) -> Typed {
        composite
            .and_then(|name| self.schema.composite(&name.to_lowercase()))
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|col| col.name == fold(field, self.dialect))
            })
            .map_or(Typed::UNKNOWN, |col| col.typed(self.dialect))
    }

    /// Whether `data_type` is, or is an array of, a domain, enum or composite type the
    /// schema defines.
    fn defines_type(&self, data_type: &DataType) -> bool {
        match data_type {
            DataType::Custom(name, _) => self.schema.defines_type(&table_key(name, self.dialect)),
            DataType::Array(
                ArrayElemTypeDef::AngleBracket(inner)
                | ArrayElemTypeDef::Parenthesis(inner)
                | ArrayElemTypeDef::Qualified(inner, _)
                | ArrayElemTypeDef::SquareBracket(inner, _),
            ) => self.defines_type(inner),
            _ => false,
        }
    }

    /// The upper-case name the built-in function `name` is looked up by. In PostgreSQL a
    /// `pg_catalog.` qualifier names the same built-in, and a quoted name that is not lower
    /// case (`"LOWER"`) is no built-in at all.
    pub(super) fn builtin_name(&self, name: &ObjectName) -> String {
        if self.dialect != SqlDialect::Postgres {
            return name.to_string().to_uppercase();
        }
        let idents: Vec<&Ident> = name.0.iter().filter_map(|part| part.as_ident()).collect();
        let bare = match idents.as_slice() {
            [only] => only,
            [schema, last] if fold(schema, self.dialect) == "pg_catalog" => last,
            _ => return name.to_string().to_uppercase(),
        };
        let folded = fold(bare, self.dialect);
        if folded == folded.to_lowercase() {
            folded.to_uppercase()
        } else {
            format!("\"{folded}\"")
        }
    }

    /// A function name for messages: lower case, except that a PostgreSQL name keeping its
    /// case is quoted.
    fn display_name(&self, name: &ObjectName) -> String {
        if self.dialect != SqlDialect::Postgres {
            return name.to_string().to_lowercase();
        }
        let parts: Vec<String> = name
            .0
            .iter()
            .filter_map(|part| part.as_ident())
            .map(|ident| {
                let folded = fold(ident, self.dialect);
                if folded == folded.to_lowercase() {
                    folded
                } else {
                    format!("\"{folded}\"")
                }
            })
            .collect();
        parts.join(".")
    }

    /// The type of a call to a function the schema defines.
    fn user_function(&self, name: &ObjectName, args: &FunctionArguments) -> Typed {
        let count = match args {
            FunctionArguments::List(list) => list.args.len(),
            _ => 0,
        };
        match self.schema.function(&table_key(name, self.dialect), count) {
            Some(FunctionReturn::Scalar(typed) | FunctionReturn::SetOf(typed)) => {
                typed.clone().into()
            }
            _ => Typed::UNKNOWN,
        }
    }

    pub(super) fn expr(&self, expr: &Expr, scope: &Scope) -> Inferred<Typed> {
        match expr {
            Expr::Cast { data_type, .. } => Ok(self.cast_typed(data_type)),
            Expr::Nested(inner) => self.expr(inner, scope),
            Expr::Array(array) => self.array_typed(&array.elem, scope),
            Expr::CompoundFieldAccess { root, access_chain } => {
                self.access_typed(root, access_chain, scope)
            }
            Expr::UnaryOp {
                op: UnaryOperator::Minus | UnaryOperator::Plus,
                expr: inner,
            } => match crate::values::constant(expr) {
                Some(negative) => Ok(constant(negative)),
                None => Ok(strip(self.expr(inner, scope)?, self.dialect)),
            },
            Expr::UnaryOp {
                op: UnaryOperator::PGAbs | UnaryOperator::BitwiseNot,
                expr: inner,
            } if self.dialect == SqlDialect::Postgres => {
                Ok(strip(self.expr(inner, scope)?, self.dialect))
            }
            Expr::AtTimeZone { timestamp, .. } if self.dialect == SqlDialect::Postgres => {
                // `timestamp AT TIME ZONE zone` is a `timestamptz` and vice versa.
                let typed = self.expr(timestamp, scope)?;
                Ok(match operand_type(&typed) {
                    Some("TIMESTAMP") => from_sql_type("TIMESTAMPTZ", self.dialect),
                    Some("TIMESTAMPTZ") => from_sql_type("TIMESTAMP", self.dialect),
                    _ => Typed::UNKNOWN,
                })
            }
            Expr::Ceil { expr: inner, .. } | Expr::Floor { expr: inner, .. } => {
                self.function_type(Returns::Real, &[inner], scope)
            }
            Expr::Extract { .. } if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("DECIMAL", self.dialect))
            }
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => Ok(self.column_expr(expr, scope)),
            Expr::Value(value) => self.value_typed(&value.value),
            Expr::Exists { .. }
            | Expr::InSubquery { .. }
            | Expr::InList { .. }
            | Expr::InUnnest { .. }
            | Expr::Between { .. }
            | Expr::Like { .. }
            | Expr::ILike { .. }
            | Expr::SimilarTo { .. }
            | Expr::RLike { .. }
            | Expr::AnyOp { .. }
            | Expr::AllOp { .. }
            | Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::IsTrue(_)
            | Expr::IsNotTrue(_)
            | Expr::IsFalse(_)
            | Expr::IsNotFalse(_)
            | Expr::IsUnknown(_)
            | Expr::IsNotUnknown(_)
            | Expr::IsDistinctFrom(..)
            | Expr::IsNotDistinctFrom(..)
            | Expr::UnaryOp {
                op: UnaryOperator::Not,
                ..
            } => Ok(from_sql_type("BOOLEAN", self.dialect)),
            Expr::Interval(_) => Ok(from_sql_type("INTERVAL", self.dialect)),
            Expr::BinaryOp { left, op, right } => self.binary_op(left, op, right, scope),
            Expr::Trim { .. } | Expr::Substring { .. } if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("TEXT", self.dialect))
            }
            Expr::Trim { .. } | Expr::Substring { .. } => known("str"),
            Expr::Position { .. } => known("int"),
            Expr::Subquery(query) => {
                Ok(
                    first_column(&self.subquery_columns(query, scope, Reporting::Record))?
                        .map_or(Typed::UNKNOWN, Column::typed),
                )
            }
            Expr::Case {
                conditions,
                else_result,
                ..
            } => self.case_typed(conditions, else_result.as_deref(), scope),
            Expr::Function(func) => self.call_typed(func, scope),
            _ => Ok(Typed::UNKNOWN),
        }
    }

    /// `CAST(x AS type)`: the type as the schema resolves it, with the reason when it has no
    /// Python type.
    fn cast_typed(&self, data_type: &DataType) -> Typed {
        let (sql_type, python_type) = self.schema.resolve_type(data_type, self.dialect);
        let typed = match python_type {
            // `SELECT *` writes a MySQL enum back as `x::ENUM('X', 'Y')` with its
            // labels upper-cased, which are not the column's values.
            Some(_) if matches!(data_type, DataType::Enum(..)) => {
                from_sql_type(&sql_type, self.dialect)
            }
            Some(python_type) => Typed::new(Some(python_type), Some(sql_type)),
            None => from_sql_type(&sql_type, self.dialect),
        };
        if !typed.is_known() && self.diag.enabled() {
            if self.schema.is_empty() || self.defines_type(data_type) {
                self.diag.explain_silently();
            } else {
                self.diag.record(
                    NameRule::UnknownType,
                    format!("type `{data_type}` has no Python type"),
                );
            }
        }
        typed
    }

    /// `ARRAY[a, b, ...]`: an array of the common type of its elements.
    fn array_typed(&self, elems: &[Expr], scope: &Scope) -> Inferred<Typed> {
        let mut common: Option<Typed> = None;
        for elem in elems {
            if matches!(elem, Expr::Value(value) if matches!(value.value, Value::Null)) {
                continue;
            }
            let mut typed = self.expr(elem, scope)?;
            if let Some(inner) = typed.sql().and_then(array_element) {
                typed = from_sql_type(inner, self.dialect);
            }
            common = match common {
                _ if typed == Typed::UNKNOWN => None,
                Some(common) => common_type(common, typed, self.dialect),
                None => Some(typed),
            };
            if common.is_none() {
                return Ok(Typed::UNKNOWN);
            }
        }
        let elem =
            common.and_then(|typed| typed.sql().or_else(|| category(&typed)).map(String::from));
        Ok(elem.map_or(Typed::UNKNOWN, |elem| {
            from_sql_type(&format!("ARRAY<{elem}>"), self.dialect)
        }))
    }

    /// `col[1].field`: element and field types of the root's type, as the schema declares them.
    fn access_typed(
        &self,
        root: &Expr,
        access_chain: &[AccessExpr],
        scope: &Scope,
    ) -> Inferred<Typed> {
        let (root, chain) = split_access(root, access_chain);
        let mut typed = self.expr(&root, scope)?;
        for access in chain {
            typed = match access {
                AccessExpr::Subscript(Subscript::Index { .. }) => self.element_typed(typed.sql()),
                AccessExpr::Subscript(Subscript::Slice { .. }) => typed,
                AccessExpr::Dot(Expr::Identifier(field)) => self.field_typed(typed.sql(), field),
                AccessExpr::Dot(_) => typed,
            };
        }
        Ok(typed)
    }

    /// A literal: numbers by their spelling, strings as constants.
    fn value_typed(&self, value: &Value) -> Inferred<Typed> {
        match value {
            // PostgreSQL reads `1e3` as a `numeric`.
            Value::Number(number, _) if number.contains(['e', 'E']) => {
                known(if self.dialect == SqlDialect::Postgres {
                    decimal_type(self.dialect)
                } else {
                    "float"
                })
            }
            Value::Number(number, _) if number.contains('.') => known(decimal_type(self.dialect)),
            Value::Number(number, _) => Ok(number.parse().map_or_else(
                |_| Typed::python_only("int"),
                |n| constant(crate::values::Value::Int(n)),
            )),
            Value::Boolean(_) => known("bool"),
            Value::Null | Value::Placeholder(_) => Ok(Typed::UNKNOWN),
            Value::SingleQuotedString(text) => {
                Ok(constant(crate::values::Value::Str(text.clone())))
            }
            _ => known("str"),
        }
    }

    /// `CASE`: the common type of its results.
    fn case_typed(
        &self,
        conditions: &[CaseWhen],
        else_result: Option<&Expr>,
        scope: &Scope,
    ) -> Inferred<Typed> {
        let mut results = Vec::new();
        for expr in conditions
            .iter()
            .map(|when| &when.result)
            .chain(else_result)
        {
            results.push(self.expr(expr, scope)?);
        }
        if self.dialect == SqlDialect::Postgres {
            if let Some(common) = common_of(results.iter().cloned(), self.dialect) {
                return Ok(common);
            }
        }
        // Other dialects take the first branch's type, but a set of values
        // covers every branch.
        let first = results.first().cloned().unwrap_or(Typed::UNKNOWN);
        if value_set(&first).is_some() {
            return Ok(
                common_of(results, self.dialect).unwrap_or_else(|| strip(first, self.dialect))
            );
        }
        Ok(first)
    }

    /// A function call: a built-in by its [`Returns`] rule, else one the schema defines.
    fn call_typed(&self, func: &Function, scope: &Scope) -> Inferred<Typed> {
        let name = self.builtin_name(&func.name);
        let args = positional_args(&func.args);
        let arg = args.first().copied();
        match name.as_str() {
            "COUNT" => Ok(from_sql_type("BIGINT", self.dialect)),
            "SUM" | "MIN" | "MAX" => self.aggregate(&name, arg, scope),
            _ => match self.returns(&name) {
                Some(returns) => self.function_type(returns, &args, scope),
                None => Ok(self.user_call_typed(func)),
            },
        }
    }

    /// A call to a function sqlty has no rule for: the schema's, with the reason when the
    /// result stays untyped.
    fn user_call_typed(&self, func: &Function) -> Typed {
        let typed = self.user_function(&func.name, &func.args);
        if typed.python.is_none() && self.diag.enabled() {
            if self.schema.is_empty()
                || self
                    .schema
                    .defines_function(&table_key(&func.name, self.dialect))
            {
                self.diag.explain_silently();
            } else {
                self.diag.record(
                    NameRule::UnknownFunction,
                    format!(
                        "function `{}` is not known to sqlty, so its result is untyped",
                        self.display_name(&func.name)
                    ),
                );
            }
        }
        typed
    }
}
