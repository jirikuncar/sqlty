//! Expression typing.

use sqlparser::ast::{
    AccessExpr, ArrayElemTypeDef, CaseWhen, DataType, Expr, Function, FunctionArguments, Ident,
    ObjectName, Subscript, UnaryOperator, Value,
};

use crate::catalog::{fold, FunctionReturn};
use crate::diagnostics::NameRule;

use super::dialect::SqlDialect;
use super::functions::{positional_args, Returns};
use super::inference::FunctionType;
use super::literals::{constant, strip, value_set};
use super::memo::{first_column, Reporting};
use super::names::{split_access, table_key};
use super::operators::{category, common_of, common_type, operand_type, type_base};
use super::query::{Analyzer, Column};
use super::scope::Scope;
use super::sql_type::is_unmapped_builtin;
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
            Some(user) if user.python_type.is_some() => Typed::with_values(
                user.python_type.clone(),
                Some(elem.to_string()),
                user.values.clone(),
            ),
            _ => from_sql_type(elem, self.dialect),
        }
    }

    /// The type of `field` in the composite type `composite`, when the schema declares it;
    /// the composite's name when it declares no such field.
    fn field_typed(&self, composite: Option<&str>, field: &Ident) -> Result<Typed, String> {
        let Some((name, fields)) = composite
            .map(str::to_lowercase)
            .and_then(|name| Some((name.clone(), self.schema.composite(&name)?)))
        else {
            return Ok(Typed::UNKNOWN);
        };
        let field = fold(field, self.dialect);
        match fields.iter().find(|col| col.name == field) {
            Some(col) => Ok(col.typed(self.dialect)),
            None => Err(format!("type `{name}` has no field `{field}`")),
        }
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
            Some(
                FunctionReturn::Scalar(typed)
                | FunctionReturn::SetOf(typed)
                | FunctionReturn::Row(typed, _),
            ) => typed.clone().into(),
            _ => Typed::UNKNOWN,
        }
    }

    pub(super) fn expr(&self, expr: &Expr, scope: &Scope) -> Inferred<Typed> {
        match expr {
            Expr::Cast { data_type, .. } => Ok(self.cast_typed(data_type)),
            Expr::TypedString(typed) => Ok(self.cast_typed(&typed.data_type)),
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
                let returns = match self.dialect {
                    SqlDialect::MySql => Returns::Integral,
                    _ => Returns::Real,
                };
                self.function_type(returns, &[inner], scope)
            }
            Expr::Extract { .. } if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("DECIMAL", self.dialect))
            }
            // MySQL reads a bare `CURRENT_USER`, and PostgreSQL `CURRENT_ROLE`, reserved
            // words both, as functions without parentheses.
            Expr::Identifier(ident)
                if ident.quote_style.is_none()
                    && matches!(
                        (self.dialect, ident.value.to_ascii_lowercase().as_str()),
                        (SqlDialect::MySql, "current_user")
                            | (SqlDialect::Postgres, "current_role")
                    ) =>
            {
                Ok(from_sql_type("TEXT", self.dialect))
            }
            // MySQL full-text relevance is a floating-point number.
            Expr::MatchAgainst { .. } => Ok(from_sql_type("DOUBLE", self.dialect)),
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
            Expr::Tuple(_) if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("RECORD", self.dialect))
            }
            Expr::BinaryOp { left, op, right } => self.binary_op(left, op, right, scope),
            Expr::Trim { .. } | Expr::Substring { .. } | Expr::Overlay { .. }
                if self.dialect == SqlDialect::Postgres =>
            {
                Ok(from_sql_type("TEXT", self.dialect))
            }
            Expr::Trim { .. } | Expr::Substring { .. } | Expr::Overlay { .. } => known("str"),
            Expr::Collate { expr: inner, .. } => self.expr(inner, scope),
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
        let (sql_type, python_type, values) = self.schema.resolve_type(data_type, self.dialect);
        let typed = match python_type {
            // `SELECT *` writes a MySQL enum back as `x::ENUM('X', 'Y')` with its
            // labels upper-cased, which are not the column's values.
            Some(_) if matches!(data_type, DataType::Enum(..)) => {
                from_sql_type(&sql_type, self.dialect)
            }
            Some(python_type) => Typed::with_values(Some(python_type), Some(sql_type), values),
            None => from_sql_type(&sql_type, self.dialect),
        };
        if !typed.is_known() && self.diag.enabled() {
            let builtin = is_unmapped_builtin(data_type, self.dialect);
            if (self.schema.is_empty() && !builtin) || self.defines_type(data_type) {
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
        let mark = self.diag.mark();
        let mut typed = self.expr(&root, scope)?;
        for access in chain {
            typed = match access {
                AccessExpr::Subscript(Subscript::Index { .. }) => self.element_typed(typed.sql()),
                AccessExpr::Subscript(Subscript::Slice { .. }) => typed,
                AccessExpr::Dot(Expr::Identifier(field)) => {
                    match self.field_typed(typed.sql(), field) {
                        Ok(typed) => typed,
                        Err(message) => {
                            self.diag.rewind(mark);
                            self.diag.record(NameRule::UnknownColumn, message);
                            return Ok(Typed::UNKNOWN);
                        }
                    }
                }
                AccessExpr::Dot(_) => typed,
            };
        }
        let mut column = root.as_ref();
        while let Expr::Nested(inner) = column {
            column = inner;
        }
        // A composite column with no Python type of its own, read through a typed field.
        if typed.python.is_some()
            && matches!(column, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
        {
            self.diag.rewind(mark);
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
            // PostgreSQL reads an integer too big for a `bigint` as a `numeric`.
            Value::Number(number, _) => Ok(number.parse().map_or_else(
                |_| match self.dialect {
                    SqlDialect::Postgres => Typed::python_only(decimal_type(self.dialect)),
                    _ => Typed::python_only("int"),
                },
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
        for when in conditions {
            // A branch reads its column where its `WHEN` holds: `WHEN s = 'a' THEN s` is 'a'.
            let learned = self.learn(&when.condition, true, scope);
            if learned.limits_values() {
                let mut inner = self.clone();
                inner.facts = self.facts.with(learned);
                results.push(inner.expr(&when.result, scope)?);
            } else {
                results.push(self.expr(&when.result, scope)?);
            }
        }
        if let Some(expr) = else_result {
            results.push(self.expr(expr, scope)?);
        }
        if self.dialect == SqlDialect::Postgres {
            // PostgreSQL resolves the `ELSE` result first: of `varchar` and `text`, which
            // convert either way, the first one wins. Value sets keep the source order.
            let mut ordered = results.clone();
            if else_result.is_some() {
                ordered.rotate_right(1);
            }
            let first = common_of(ordered, self.dialect);
            let common = match common_of(results.iter().cloned(), self.dialect) {
                Some(common) if first.as_ref().is_some_and(|first| first.sql != common.sql) => {
                    first
                }
                common => common,
            };
            if let Some(common) = common {
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
        if let (FunctionArguments::Subquery(query), "ARRAY", SqlDialect::Postgres) =
            (&func.args, name.as_str(), self.dialect)
        {
            let columns = self.subquery_columns(query, scope, Reporting::Record);
            let elem = first_column(&columns)?
                .map(Column::typed)
                .and_then(|typed| typed.sql().or_else(|| category(&typed)).map(String::from));
            return Ok(elem.map_or(Typed::UNKNOWN, |elem| {
                from_sql_type(&format!("ARRAY<{elem}>"), self.dialect)
            }));
        }
        let args = positional_args(&func.args);
        let arg = args.first().copied();
        if let Some((rule, typed)) = self.function_rule(&name, arg, scope)? {
            let argument = || match &typed {
                Some(typed) => Ok(typed.clone()),
                None => arg.map_or(Ok(Typed::UNKNOWN), |arg| self.expr(arg, scope)),
            };
            return Ok(match rule.result {
                FunctionType::Sql(sql) => from_sql_type(sql, self.dialect),
                FunctionType::Unknown => Typed::UNKNOWN,
                FunctionType::Argument => argument()?,
                FunctionType::ArgumentAs(sql) => {
                    let typed = argument()?;
                    Typed::with_values(
                        typed.python().map(String::from),
                        Some(sql.to_string()),
                        typed.values,
                    )
                }
                FunctionType::Code => self.sum(argument()?, arg, scope)?,
            });
        }
        match name.as_str() {
            // Ordered-set aggregates: the type of the `WITHIN GROUP (ORDER BY x)` value.
            "PERCENTILE_CONT" | "PERCENTILE_DISC" | "MODE"
                if self.dialect == SqlDialect::Postgres && !func.within_group.is_empty() =>
            {
                let ordered = self.expr(&func.within_group[0].expr, scope)?;
                let typed = match name.as_str() {
                    "PERCENTILE_CONT" if operand_type(&ordered) == Some("INTERVAL") => ordered,
                    "PERCENTILE_CONT" => from_sql_type("DOUBLE", self.dialect),
                    _ => strip(ordered, self.dialect),
                };
                // An array of fractions gives an array of percentiles.
                Ok(match (arg, typed.sql()) {
                    (Some(Expr::Array(_)), Some(sql)) if name != "MODE" => {
                        from_sql_type(&format!("ARRAY<{sql}>"), self.dialect)
                    }
                    (Some(Expr::Array(_)), _) if name != "MODE" => Typed::UNKNOWN,
                    _ => typed,
                })
            }
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

#[cfg(test)]
mod tests {
    use super::super::test_support::reports;
    use super::*;

    const DDL: &str = "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy'); \
                       CREATE TYPE pair AS (x INT, y TEXT); \
                       CREATE DOMAIN positive AS INT CHECK (VALUE > 0); \
                       CREATE TABLE t (i INT NOT NULL, n INT, txt TEXT, m mood NOT NULL, \
                       p pair, pos positive NOT NULL, arr INT[] NOT NULL, \
                       grid INT[][], ts TIMESTAMP NOT NULL, tz TIMESTAMPTZ, j JSONB)";

    #[test]
    fn array_element_reads_the_inner_type() {
        assert_eq!(array_element("ARRAY<INT>"), Some("INT"));
        assert_eq!(array_element("ARRAY<INT>[3]"), Some("INT"));
        assert_eq!(array_element("ARRAY<ARRAY<TEXT>>"), Some("ARRAY<TEXT>"));
        assert_eq!(array_element("INT"), None);
        assert_eq!(array_element("ARRAY<INT"), None);
    }

    #[test]
    fn range_element_names_the_bound_type() {
        assert_eq!(range_element("INT4RANGE"), Some("INT"));
        assert_eq!(range_element("INT8RANGE"), Some("BIGINT"));
        assert_eq!(range_element("NUMRANGE"), Some("DECIMAL"));
        assert_eq!(range_element("TSRANGE"), Some("TIMESTAMP"));
        assert_eq!(range_element("TSTZRANGE"), Some("TIMESTAMPTZ"));
        assert_eq!(range_element("DATERANGE"), Some("DATE"));
        assert_eq!(range_element("INT"), None);
    }

    #[test]
    fn known_is_a_python_only_type() {
        let typed = known("int").unwrap();
        assert_eq!(typed.python(), Some("int"));
        assert_eq!(typed.sql(), None);
    }

    #[test]
    fn from_sql_type_maps_the_python_type() {
        let typed = from_sql_type("BIGINT", SqlDialect::Postgres);
        assert_eq!(typed.python(), Some("int"));
        assert_eq!(typed.sql(), Some("BIGINT"));
        assert_eq!(from_sql_type("NOPE", SqlDialect::Postgres).python(), None);
    }

    #[test]
    fn casts() {
        let queries = [
            "SELECT i::BIGINT AS a, i::TEXT AS b, txt::INT AS c, n::NUMERIC(10, 2) AS d",
            "SELECT CAST(i AS DOUBLE PRECISION) AS a, CAST(txt AS VARCHAR(20)) AS b FROM t",
            "SELECT '1'::INT AS a, NULL::TEXT AS b, 1::BOOLEAN AS c",
            "SELECT i::INT[] AS a, txt::TEXT[] AS b, '{}'::INT[][] AS c",
            "SELECT m::TEXT AS a, txt::mood AS b, i::positive AS c",
            "SELECT i::pair AS a",
            "SELECT i::no_such_type AS a",
            "SELECT i::JSONB AS a, i::UUID AS b, i::BYTEA AS c, i::DATE AS d",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn casts_without_a_schema_do_not_complain() {
        let queries = ["SELECT 1::no_such_type AS a", "SELECT x::INT AS a"];
        insta::assert_snapshot!(reports(&queries, "postgres", None));
    }

    #[test]
    fn literals_and_unary_operators() {
        let queries = [
            "SELECT 1 AS a, -1 AS b, +1 AS c, 1.5 AS d, 'x' AS e, true AS f, NULL AS g",
            "SELECT -i AS a, @ i AS b, ~i AS c, NOT true AS d FROM t",
            "SELECT 1e3 AS a, 0x1F AS b, X'ff' AS c, E'a\\nb' AS d, $$dollar$$ AS e",
            "SELECT $1 AS a, $1::INT AS b",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn case_expressions() {
        let queries = [
            "SELECT CASE WHEN i > 1 THEN 1 ELSE 2 END AS a FROM t",
            "SELECT CASE WHEN i > 1 THEN 1 END AS a FROM t",
            "SELECT CASE WHEN i > 1 THEN i ELSE n END AS a FROM t",
            "SELECT CASE WHEN i > 1 THEN i ELSE 2.5 END AS a FROM t",
            "SELECT CASE WHEN i > 1 THEN i ELSE txt END AS a FROM t",
            "SELECT CASE i WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'many' END AS a FROM t",
            "SELECT CASE WHEN i > 1 THEN NULL ELSE NULL END AS a FROM t",
            "SELECT CASE WHEN i > 1 THEN m ELSE 'sad' END AS a FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn arrays_and_subscripts() {
        let queries = [
            "SELECT ARRAY[1, 2, 3] AS a, ARRAY[1, 2.5] AS b, ARRAY['a', 'b'] AS c",
            "SELECT ARRAY[i, n] AS a, ARRAY[]::INT[] AS b, ARRAY[ARRAY[1], ARRAY[2]] AS c FROM t",
            "SELECT ARRAY[1, 'a'] AS a",
            "SELECT arr[1] AS a, grid[1][2] AS b, arr[1:2] AS c, (arr)[1] AS d FROM t",
            "SELECT txt[1] AS a, i[1] AS b FROM t",
            "SELECT ARRAY(SELECT i FROM t) AS a",
            "SELECT i = ANY(arr) AS a, arr @> ARRAY[1] AS b, arr || 4 AS c FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn composite_fields_and_json_access() {
        let queries = [
            "SELECT (p).x AS a, (p).y AS b, (t.p).x AS c FROM t",
            "SELECT (p).nope AS a FROM t",
            "SELECT j -> 'a' AS a, j ->> 'a' AS b, j #> '{a,b}' AS c, j #>> '{a,b}' AS d FROM t",
            "SELECT j['a'] AS a, j @> '{}' AS b, j ? 'a' AS c FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn typed_string_literals_are_typed() {
        let report = super::super::test_support::report(
            "SELECT DATE '2020-01-01' AS d, TIMESTAMP '2020-01-01 00:00:00' AS ts",
            "postgres",
            None,
        );
        assert!(report.contains("d: datetime.date"), "{report}");
        assert!(report.contains("ts: datetime.datetime"), "{report}");
    }

    #[test]
    fn misc_expressions() {
        let queries = [
            "SELECT EXTRACT(YEAR FROM ts) AS a, POSITION('a' IN txt) AS b, TRIM(txt) AS c FROM t",
            "SELECT SUBSTRING(txt FROM 1 FOR 2) AS a, CEIL(1.5) AS b, FLOOR(i) AS c FROM t",
            "SELECT ts AT TIME ZONE 'UTC' AS a, tz AT TIME ZONE 'UTC' AS b, i AT TIME ZONE 'UTC' AS c FROM t",
            "SELECT EXISTS (SELECT 1) AS a, i IN (SELECT i FROM t) AS b, i BETWEEN 1 AND 3 AS c FROM t",
            "SELECT (SELECT max(i) FROM t) AS a, (SELECT i, n FROM t) AS b",
            "SELECT txt ILIKE 'a%' AS a, txt SIMILAR TO 'a' AS b, txt ~ 'a' AS c FROM t",
            "SELECT i IS DISTINCT FROM n AS a, n IS NOT NULL AS b, (i > 1) IS TRUE AS c FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn mysql_and_sqlite_expressions() {
        let ddl_mysql =
            "CREATE TABLE t (i INT NOT NULL, txt VARCHAR(10), e ENUM('a', 'b') NOT NULL)";
        let queries = [
            "SELECT CAST(i AS CHAR) AS a, CAST(i AS SIGNED) AS b, CAST(txt AS UNSIGNED) AS c FROM t",
            "SELECT TRIM(txt) AS a, SUBSTRING(txt, 1, 2) AS b, POSITION('a' IN txt) AS c FROM t",
            "SELECT e AS a, CAST(e AS CHAR) AS b, i DIV 2 AS c FROM t",
        ];
        let mysql = reports(&queries, "mysql", Some(ddl_mysql));
        let sqlite_queries = [
            "SELECT CAST(x AS INTEGER) AS a, CAST(x AS TEXT) AS b, CAST(x AS REAL) AS c FROM t",
            "SELECT 1 AS a, 'x' AS b, x'ff' AS c FROM t",
        ];
        let sqlite = reports(&sqlite_queries, "sqlite", Some("CREATE TABLE t (x)"));
        insta::assert_snapshot!(format!("mysql:\n{mysql}\nsqlite:\n{sqlite}"));
    }
}
