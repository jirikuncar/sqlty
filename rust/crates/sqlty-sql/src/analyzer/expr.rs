//! Expression typing.

use sqlparser::ast::{
    AccessExpr, BinaryOperator, CaseWhen, DataType, Expr, Function, FunctionArgumentClause,
    FunctionArguments, Ident, ObjectName, Subscript, UnaryOperator, Value,
};

use crate::catalog::{fold, FunctionReturn, SchemaColumn};
use crate::diagnostics::Rule;
use sqlparser::ast::Spanned;
use sqlparser::tokenizer::Span;

use super::dialect::SqlDialect;
use super::functions::{arg_list, extract_type, positional_args, Returns};
use super::inference::FunctionType;
use super::literals::{constant, python_kind, strip, value_set};
use super::memo::{first_column, Reporting};
use super::names::{split_access, table_key};
use super::operators::{category, common_of, common_type, operand_type, type_base, unmatched};
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

/// The element type of a range or multirange type such as `TSTZRANGE`.
pub(super) fn range_element(sql_type: &str) -> Option<&'static str> {
    Some(match range_kind(sql_type)? {
        "INT4" => "INT",
        "INT8" => "BIGINT",
        "NUM" => "DECIMAL",
        "TS" => "TIMESTAMP",
        "TSTZ" => "TIMESTAMPTZ",
        "DATE" => "DATE",
        _ => return None,
    })
}

/// The prefix that names a range or multirange type: `INT4` for `INT4RANGE` and
/// `INT4MULTIRANGE`.
pub(super) fn range_kind(sql_type: &str) -> Option<&str> {
    let base = type_base(sql_type);
    base.strip_suffix("MULTIRANGE")
        .or_else(|| base.strip_suffix("RANGE"))
}

pub(super) fn known(python_type: &str) -> Inferred<Typed> {
    Ok(Typed::python_only(python_type))
}

pub(super) fn from_sql_type(sql_type: &str, dialect: SqlDialect) -> Typed {
    Typed::of_sql(sql_type, dialect)
}

impl<'a> Analyzer<'a> {
    /// The array of SQL type `elem`; an array of enums lists their `Literal`.
    pub(super) fn array_of(&self, elem: &str) -> Typed {
        let sql = format!("ARRAY<{elem}>");
        let typed = from_sql_type(&sql, self.dialect);
        match self.element_typed(Some(&sql)).python {
            Some(python) if typed.python.as_deref() == Some("list") => {
                Typed::new(Some(format!("list[{python}]")), Some(sql))
            }
            _ => typed,
        }
    }

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

    /// The type of the values PostgreSQL's `unnest` returns from an array of this SQL type:
    /// it flattens every dimension.
    pub(super) fn unnested_typed(&self, mut array: Option<&str>) -> Typed {
        while let Some(inner) = array
            .and_then(array_element)
            .filter(|e| e.starts_with("ARRAY<"))
        {
            array = Some(inner);
        }
        self.element_typed(array)
    }

    /// The type of `field` in the composite type `composite`, when the schema declares it;
    /// the composite's name when it declares no such field. In PostgreSQL each table is
    /// also the composite type of its rows.
    fn field_typed(&self, composite: Option<&str>, field: &Ident) -> Result<Typed, String> {
        let Some((name, fields)) = composite.and_then(|name| self.composite_fields(name)) else {
            return Ok(Typed::UNKNOWN);
        };
        if is_field_star(field) {
            return Ok(Typed::UNKNOWN);
        }
        let field = fold(field, self.dialect);
        match fields.iter().find(|col| col.name == field) {
            Some(col) => Ok(col.typed(self.dialect)),
            None => Err(format!("type `{name}` has no field `{field}`")),
        }
    }

    /// The lower-case name and the fields of the composite type `composite`, when the schema
    /// declares it. In PostgreSQL each table is also the composite type of its rows.
    pub(super) fn composite_fields(&self, composite: &str) -> Option<(String, &[SchemaColumn])> {
        let name = composite.to_lowercase();
        let fields = self.schema.composite(&name).or_else(|| {
            (self.dialect == SqlDialect::Postgres)
                .then(|| self.schema.table(&name))
                .flatten()
        })?;
        Some((name, fields))
    }

    /// Whether `data_type` is a domain, enum or composite type the schema defines; an array
    /// of any type is a `list`.
    fn defines_type(&self, data_type: &DataType) -> bool {
        matches!(data_type, DataType::Custom(name, _) if self.schema.defines_type(&table_key(name, self.dialect)))
    }

    /// The upper-case name the built-in function `name` is looked up by. In PostgreSQL a
    /// `pg_catalog.` qualifier names the same built-in, and a quoted name that is not lower
    /// case (`"LOWER"`) is no built-in at all. Elsewhere a quoted name is the built-in's.
    pub(super) fn builtin_name(&self, name: &ObjectName) -> String {
        let idents: Vec<&Ident> = name.0.iter().filter_map(|part| part.as_ident()).collect();
        if self.dialect != SqlDialect::Postgres {
            return match idents.as_slice() {
                [only] => only.value.to_uppercase(),
                _ => name.to_string().to_uppercase(),
            };
        }
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
        match self
            .schema
            .function(&table_key(name, self.dialect), arg_list(args).len())
        {
            Some(
                FunctionReturn::Scalar(typed)
                | FunctionReturn::SetOf(typed)
                | FunctionReturn::Row(typed, _),
            ) => typed.clone().into(),
            _ => Typed::UNKNOWN,
        }
    }

    #[recursive::recursive]
    pub(super) fn expr(&self, expr: &Expr, scope: &Scope) -> Inferred<Typed> {
        let typed = self.expr_typed(expr, scope)?;
        // ClickHouse gives a comparison, and a logical operator over one, as a `UInt8`.
        Ok(
            if self.dialect == SqlDialect::ClickHouse
                && typed.sql() == Some("BOOLEAN")
                && clickhouse_uint8(expr)
            {
                from_sql_type("UTINYINT", self.dialect)
            } else {
                typed
            },
        )
    }

    fn expr_typed(&self, expr: &Expr, scope: &Scope) -> Inferred<Typed> {
        let _at = self.diag.enabled().then(|| self.diag.at(name_span(expr)));
        match expr {
            Expr::Cast { data_type, .. } => Ok(self.cast_typed(data_type)),
            // MySQL's `CONVERT(x, type)`; `CONVERT(x USING charset)` is a binary string for
            // `binary`, else text.
            Expr::Convert {
                data_type: Some(data_type),
                is_try: false,
                ..
            } => Ok(self.cast_typed(data_type)),
            Expr::Convert {
                expr: inner,
                data_type: None,
                charset: Some(charset),
                ..
            } => {
                let typed = self.expr(inner, scope)?;
                let (kind, sql_type) = if is_binary_charset(charset) {
                    ("bytes", "VARBINARY")
                } else {
                    ("str", "TEXT")
                };
                Ok(if python_kind(&typed) == Some(kind) {
                    typed
                } else {
                    from_sql_type(sql_type, self.dialect)
                })
            }
            Expr::TypedString(typed) => Ok(self.cast_typed(&typed.data_type)),
            Expr::Nested(inner) => self.expr(inner, scope),
            Expr::Array(array) => self.array_typed(&array.elem, scope),
            Expr::CompoundFieldAccess { root, access_chain } => {
                self.access_typed(root, access_chain, scope)
            }
            Expr::UnaryOp {
                op: op @ (UnaryOperator::Minus | UnaryOperator::Plus),
                expr: inner,
            } => match crate::values::constant(expr) {
                Some(negative) => Ok(constant(negative)),
                None => {
                    let typed = self.expr(inner, scope)?;
                    Ok(match self.prefix_rule_of(op, &typed) {
                        Some(rule) => self.prefixed(rule, typed),
                        None => strip(typed, self.dialect),
                    })
                }
            },
            Expr::AtTimeZone { timestamp, .. } if self.dialect == SqlDialect::Postgres => {
                // `timestamp AT TIME ZONE zone` is a `timestamptz` and vice versa; a time
                // of day becomes a `timetz` either way. A date, a string literal and NULL
                // are read as a `timestamptz`, the preferred type of their category.
                let typed = self.expr(timestamp, scope)?;
                let untyped = matches!(
                    timestamp.as_ref(),
                    Expr::Value(value)
                        if matches!(value.value, Value::SingleQuotedString(_) | Value::Null)
                );
                Ok(match operand_type(&typed) {
                    _ if untyped => from_sql_type("TIMESTAMP", self.dialect),
                    Some("TIMESTAMP") => from_sql_type("TIMESTAMPTZ", self.dialect),
                    Some("TIMESTAMPTZ" | "DATE") => from_sql_type("TIMESTAMP", self.dialect),
                    Some("TIME" | "TIMETZ") => from_sql_type("TIMETZ", self.dialect),
                    _ => Typed::UNKNOWN,
                })
            }
            Expr::Ceil { expr: inner, .. } | Expr::Floor { expr: inner, .. } => {
                let typed = self.expr(inner, scope)?;
                let returns = match self.dialect {
                    SqlDialect::MySql => Returns::Integral,
                    // SQLite returns an integer argument as it is.
                    SqlDialect::Sqlite if python_kind(&typed) == Some("int") => Returns::Arg(0),
                    _ => Returns::Real,
                };
                self.function_type_of(returns, &[inner], &[Some(typed), None], scope)
            }
            Expr::Extract { .. } if self.dialect == SqlDialect::Sqlite => {
                let message = "SQLite has no EXTRACT; use strftime".to_string();
                self.diag.record(Rule::RejectedQuery, message);
                Ok(Typed::UNKNOWN)
            }
            Expr::Extract { field, .. } => Ok(extract_type(self.dialect, field)
                .map_or(Typed::UNKNOWN, |sql| from_sql_type(sql, self.dialect))),
            // A character set introducer: `_binary 'a'` is a binary string, `_utf8mb4 'a'` text.
            Expr::Prefixed { prefix, .. } if self.dialect == SqlDialect::MySql => {
                Ok(from_sql_type(
                    if prefix.value.eq_ignore_ascii_case("_binary") {
                        "VARBINARY"
                    } else {
                        "TEXT"
                    },
                    self.dialect,
                ))
            }
            // MySQL reads a bare `CURRENT_USER`, a reserved word, as a function.
            Expr::Identifier(ident)
                if self.dialect == SqlDialect::MySql
                    && ident.quote_style.is_none()
                    && ident.value.eq_ignore_ascii_case("current_user") =>
            {
                Ok(from_sql_type("TEXT", self.dialect))
            }
            // MySQL full-text relevance is a floating-point number.
            Expr::MatchAgainst { .. } => Ok(from_sql_type("DOUBLE", self.dialect)),
            Expr::Identifier(ident) => {
                Ok(self.column_expr(expr, std::slice::from_ref(ident), scope))
            }
            Expr::CompoundIdentifier(parts) => Ok(self.column_expr(expr, parts, scope)),
            Expr::Value(value) => self.value_typed(&value.value),
            Expr::Exists { .. }
            | Expr::In { .. }
            | Expr::MemberOf(_)
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
            | Expr::IsJson { .. }
            | Expr::IsNormalized { .. }
            | Expr::UnaryOp {
                op: UnaryOperator::Not,
                ..
            } => Ok(from_sql_type("BOOLEAN", self.dialect)),
            Expr::Interval(_) => Ok(from_sql_type("INTERVAL", self.dialect)),
            Expr::Tuple(_) if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("RECORD", self.dialect))
            }
            Expr::BinaryOp { left, op, right } => self.binary_op(left, op, right, scope),
            Expr::UnaryOp { op, expr: inner } => self.prefix_op(op, inner, scope),
            // SQLite's trim always returns text.
            Expr::Trim { .. } if self.dialect == SqlDialect::Sqlite => {
                Ok(from_sql_type("TEXT", self.dialect))
            }
            Expr::Trim { expr: inner, .. }
            | Expr::Substring { expr: inner, .. }
            | Expr::Overlay { expr: inner, .. } => {
                self.function_type(Returns::Text(0), &[inner.as_ref()], scope)
            }
            Expr::Collate { expr: inner, .. } => self.expr(inner, scope),
            Expr::Position { .. } => Ok(from_sql_type("INT", self.dialect)),
            Expr::Subquery(query) => {
                Ok(
                    first_column(&self.subquery_columns(query, scope, Reporting::Record))?
                        .map_or(Typed::UNKNOWN, Column::typed),
                )
            }
            Expr::Case {
                operand: None,
                conditions,
                else_result,
                ..
            } => self.case_typed(conditions, else_result.as_deref(), scope),
            // `CASE x WHEN v` is `CASE WHEN x = v`.
            Expr::Case {
                operand: Some(operand),
                conditions,
                else_result,
                ..
            } => {
                let searched: Vec<CaseWhen> = conditions
                    .iter()
                    .map(|when| CaseWhen {
                        condition: Expr::BinaryOp {
                            left: operand.clone(),
                            op: BinaryOperator::Eq,
                            right: Box::new(when.condition.clone()),
                        },
                        result: when.result.clone(),
                    })
                    .collect();
                self.case_typed(&searched, else_result.as_deref(), scope)
            }
            Expr::Function(func) => self.call_typed(func, scope),
            _ => Ok(Typed::UNKNOWN),
        }
    }

    /// `CAST(x AS type)`: the type as the schema resolves it, with the reason when it has no
    /// Python type.
    pub(super) fn cast_typed(&self, data_type: &DataType) -> Typed {
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
                    Rule::UnknownType,
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
            let typed = self.expr(elem, scope)?;
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
        Ok(elem.map_or(Typed::UNKNOWN, |elem| self.array_of(&elem)))
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
                AccessExpr::Subscript(Subscript::Index { .. })
                    if self.dialect == SqlDialect::Postgres
                        && operand_type(&typed) == Some("JSONB") =>
                {
                    from_sql_type("JSONB", self.dialect)
                }
                AccessExpr::Subscript(Subscript::Index { .. }) => self.element_typed(typed.sql()),
                AccessExpr::Subscript(Subscript::Slice { .. }) => typed,
                AccessExpr::Dot(Expr::Identifier(field)) => {
                    match self.field_typed(typed.sql(), field) {
                        Ok(typed) => typed,
                        Err(message) => {
                            self.diag.rewind(mark);
                            self.diag.record(Rule::UnknownColumn, message);
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
            // ClickHouse reads a decimal literal as a `Float64`.
            Value::Number(number, _) if number.contains('.') => known(match self.dialect {
                SqlDialect::ClickHouse => "float",
                dialect => decimal_type(dialect),
            }),
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
            Value::HexStringLiteral(hex) if hex.starts_with("0x") => {
                let message = format!("hex literal too big: {hex}");
                self.diag.record(Rule::RejectedQuery, message);
                Ok(Typed::UNKNOWN)
            }
            // `X'41'` and `B'101'` are bit strings in PostgreSQL, binary strings elsewhere.
            Value::HexStringLiteral(_) | Value::SingleQuotedByteStringLiteral(_) => {
                Ok(from_sql_type(
                    match self.dialect {
                        SqlDialect::Postgres => "BIT",
                        _ => "VARBINARY",
                    },
                    self.dialect,
                ))
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
        // Later branches read a column without the values earlier `WHEN`s took.
        let mut skipped = self.clone();
        for when in conditions {
            // A branch reads its column where its `WHEN` holds: `WHEN s = 'a' THEN s` is 'a'.
            let learned = skipped.learn(&when.condition, true, scope);
            if learned.limits_values() {
                let mut inner = skipped.clone();
                inner.facts = skipped.facts.with(learned);
                results.push(inner.expr(&when.result, scope)?);
            } else {
                results.push(skipped.expr(&when.result, scope)?);
            }
            let rest = skipped.learn_skipped(&when.condition, scope);
            if rest.limits_values() {
                skipped.facts = skipped.facts.with(rest);
            }
        }
        if let Some(expr) = else_result {
            results.push(skipped.expr(expr, scope)?);
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
        if let Some(common) = common_of(results.iter().cloned(), self.dialect) {
            return Ok(common);
        }
        if self.reject_unmatched("CASE", &results) {
            return Ok(Typed::UNKNOWN);
        }
        let first = results.first().cloned().unwrap_or(Typed::UNKNOWN);
        Ok(if value_set(&first).is_some() {
            strip(first, self.dialect)
        } else {
            first
        })
    }

    /// PostgreSQL rejects `what` over `types` of different categories: whether it does.
    pub(super) fn reject_unmatched(&self, what: &str, types: &[Typed]) -> bool {
        let Some((a, b)) = unmatched(types).filter(|_| self.dialect == SqlDialect::Postgres) else {
            return false;
        };
        let message = format!("{what} types {a} and {b} cannot be matched");
        self.diag.record(Rule::RejectedQuery, message);
        true
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
            return Ok(elem.map_or(Typed::UNKNOWN, |elem| self.array_of(&elem)));
        }
        if let FunctionArguments::List(list) = &func.args {
            let returning = list.clauses.iter().find_map(|clause| match clause {
                FunctionArgumentClause::JsonReturningClause(returning) => {
                    Some(&returning.data_type)
                }
                _ => None,
            });
            if let Some(data_type) = returning {
                return Ok(self.cast_typed(data_type));
            }
        }
        let args = positional_args(&func.args);
        // SQLite's scalar `min(a, b)` returns one of its arguments.
        if self.dialect == SqlDialect::Sqlite
            && matches!(name.as_str(), "MIN" | "MAX")
            && args.len() >= 2
        {
            return self.function_type(Returns::FirstKnown, &args, scope);
        }
        let arg = args.first().copied();
        if let Some((rule, typed)) = self.function_rule(&name, &args, scope)? {
            #[expect(clippy::expect_used, reason = "a rule reads the arguments it types")]
            let at = |position: usize| {
                let read: &Option<Typed> = &typed[position];
                read.clone()
                    .expect("a rule reads the argument it is typed by")
            };
            let argument = || at(0);
            return Ok(match rule.result {
                FunctionType::Sql(sql) => from_sql_type(sql, self.dialect),
                FunctionType::Any => Typed::python_only("typing.Any"),
                FunctionType::Unknown => Typed::UNKNOWN,
                FunctionType::Argument(position) => at(position),
                FunctionType::ArgumentAs(sql) => {
                    let typed = argument();
                    Typed::with_values(
                        typed.python().map(String::from),
                        Some(sql.to_string()),
                        typed.values,
                    )
                }
                FunctionType::Code => self.sum(argument(), arg, scope)?,
                FunctionType::Returns(returns) => {
                    self.function_type_of(returns, &args, &typed, scope)?
                }
            });
        }
        match name.as_str() {
            // Ordered-set aggregates: the type of the `WITHIN GROUP (ORDER BY x)` value.
            "PERCENTILE_CONT" | "PERCENTILE_DISC" | "MODE" if !func.within_group.is_empty() => {
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
            _ => Ok(self.user_call_typed(func)),
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
                    Rule::UnknownFunction,
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

/// Whether ClickHouse types `expr` as a `UInt8` truth value: a comparison or predicate, or
/// `AND`, `OR` or `NOT` over one or over a number; over `Bool` values only, they stay `Bool`.
fn clickhouse_uint8(expr: &Expr) -> bool {
    use BinaryOperator::{
        And, Eq, Gt, GtEq, Lt, LtEq, NotEq, Or, PGRegexIMatch, PGRegexMatch, PGRegexNotIMatch,
        PGRegexNotMatch, Xor,
    };
    match expr {
        Expr::Nested(inner) => clickhouse_uint8(inner),
        Expr::BinaryOp {
            op: And | Or | Xor,
            left,
            right,
        } => clickhouse_uint8(left) || clickhouse_uint8(right),
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr: inner,
        } => clickhouse_uint8(inner),
        Expr::BinaryOp {
            op:
                Eq | NotEq | Lt | LtEq | Gt | GtEq | PGRegexMatch | PGRegexIMatch | PGRegexNotMatch
                | PGRegexNotIMatch,
            ..
        }
        | Expr::In { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::ILike { .. }
        | Expr::IsNull(_)
        | Expr::IsNotNull(_)
        | Expr::IsDistinctFrom(..)
        | Expr::IsNotDistinctFrom(..)
        | Expr::Exists { .. } => true,
        Expr::Value(value) => matches!(value.value, Value::Number(..)),
        _ => false,
    }
}

/// Whether `CONVERT(x USING charset)` gives a binary string.
pub(super) fn is_binary_charset(charset: &ObjectName) -> bool {
    charset.to_string().eq_ignore_ascii_case("binary")
}

/// Whether `field` is the `"*"` that respell writes for PostgreSQL's `(composite).*`.
pub(super) fn is_field_star(field: &Ident) -> bool {
    field.value == "*" && field.quote_style == Some('"')
}

/// Where the name a diagnostic of `expr` is about is written: the column of an identifier,
/// the name of a function; empty for other nodes, whose diagnostics are about their parts.
fn name_span(expr: &Expr) -> Span {
    match expr {
        Expr::Identifier(ident) => ident.span,
        Expr::CompoundIdentifier(parts) => Span::union_iter(parts.iter().map(|part| part.span)),
        Expr::Function(function) => function.name.span(),
        _ => Span::empty(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

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
        assert_eq!(range_element("INT4MULTIRANGE"), Some("INT"));
        assert_eq!(range_element("DATEMULTIRANGE"), Some("DATE"));
        assert_eq!(range_element("INT"), None);
        assert_eq!(range_element("FOORANGE"), None);
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
}
