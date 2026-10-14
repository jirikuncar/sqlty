//! Expression typing.

use super::*;

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

/// `(python_type, sql_type)` of an expression.
pub(super) type Typed = (Option<String>, Option<String>);

pub(super) fn known(python_type: &str) -> Inferred<Typed> {
    Ok((Some(python_type.to_string()), None))
}

pub(super) fn from_sql_type(sql_type: String, dialect: SqlDialect) -> Typed {
    (python_type(&sql_type, dialect), Some(sql_type))
}

impl<'a> Analyzer<'a> {
    /// The element type of an array type; an enum element keeps its `Literal`.
    pub(super) fn element_typed(&self, array: Option<&str>) -> Typed {
        let Some(elem) = array.and_then(array_element) else {
            return (None, None);
        };
        match self.schema.user_type(&elem.to_lowercase()) {
            Some(user) if user.python_type.is_some() => {
                (user.python_type.clone(), Some(elem.to_string()))
            }
            _ => from_sql_type(elem.to_string(), self.dialect),
        }
    }

    /// The type of `field` in the composite type `composite`, when the schema declares it.
    pub(super) fn field_typed(&self, composite: Option<&str>, field: &Ident) -> Typed {
        composite
            .and_then(|name| self.schema.composite(&name.to_lowercase()))
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|col| col.name == field.value.to_lowercase())
            })
            .map_or((None, None), |col| col.typed(self.dialect))
    }

    /// Whether `data_type` is, or is an array of, a domain, enum or composite type the
    /// schema defines.
    fn defines_type(&self, data_type: &DataType) -> bool {
        match data_type {
            DataType::Custom(name, _) => self.schema.defines_type(&table_key(name)),
            DataType::Array(
                ArrayElemTypeDef::AngleBracket(inner)
                | ArrayElemTypeDef::Parenthesis(inner)
                | ArrayElemTypeDef::Qualified(inner, _)
                | ArrayElemTypeDef::SquareBracket(inner, _),
            ) => self.defines_type(inner),
            _ => false,
        }
    }

    /// The type of a call to a function the schema defines.
    fn user_function(&self, name: &ObjectName, args: &FunctionArguments) -> Typed {
        let count = match args {
            FunctionArguments::List(list) => list.args.len(),
            _ => 0,
        };
        match self.schema.function(&table_key(name), count) {
            Some(FunctionReturn::Scalar(typed) | FunctionReturn::SetOf(typed)) => typed.clone(),
            _ => (None, None),
        }
    }

    pub(super) fn expr(&self, expr: &Expr, scope: &Scope) -> Inferred<Typed> {
        match expr {
            Expr::Cast { data_type, .. } => {
                let (sql_type, python_type) = self.schema.resolve_type(data_type, self.dialect);
                let typed = match python_type {
                    Some(python_type) => (Some(python_type), Some(sql_type)),
                    None => from_sql_type(sql_type, self.dialect),
                };
                if typed.0.is_none() && self.diag.enabled() {
                    if self.schema.is_empty() || self.defines_type(data_type) {
                        self.diag.explain_silently();
                    } else {
                        self.diag.record(
                            NameRule::UnknownType,
                            format!("type `{data_type}` has no Python type"),
                        );
                    }
                }
                Ok(typed)
            }
            Expr::Nested(inner) => self.expr(inner, scope),
            Expr::Array(array) => {
                let mut common: Option<Typed> = None;
                for elem in &array.elem {
                    if matches!(elem, Expr::Value(value) if matches!(value.value, Value::Null)) {
                        continue;
                    }
                    let mut typed = self.expr(elem, scope)?;
                    if let Some(inner) = typed.1.as_deref().and_then(array_element) {
                        typed = from_sql_type(inner.to_string(), self.dialect);
                    }
                    common = match common {
                        _ if typed == (None, None) => None,
                        Some(common) => common_type(common, typed, self.dialect),
                        None => Some(typed),
                    };
                    if common.is_none() {
                        return Ok((None, None));
                    }
                }
                let elem = common.and_then(|typed| {
                    typed
                        .1
                        .clone()
                        .or_else(|| category(&typed).map(String::from))
                });
                Ok(elem.map_or((None, None), |elem| {
                    from_sql_type(format!("ARRAY<{elem}>"), self.dialect)
                }))
            }
            Expr::CompoundFieldAccess { root, access_chain } => {
                let (mut typed, chain) = match (root.as_ref(), access_chain.as_slice()) {
                    (
                        Expr::Identifier(table),
                        [AccessExpr::Dot(Expr::Identifier(column)), rest @ ..],
                    ) => {
                        let column = Expr::CompoundIdentifier(vec![table.clone(), column.clone()]);
                        (self.expr(&column, scope)?, rest)
                    }
                    _ => (self.expr(root, scope)?, access_chain.as_slice()),
                };
                for access in chain {
                    typed = match access {
                        AccessExpr::Subscript(Subscript::Index { .. }) => {
                            self.element_typed(typed.1.as_deref())
                        }
                        AccessExpr::Subscript(Subscript::Slice { .. }) => typed,
                        AccessExpr::Dot(Expr::Identifier(field)) => {
                            self.field_typed(typed.1.as_deref(), field)
                        }
                        AccessExpr::Dot(_) => typed,
                    };
                }
                Ok(typed)
            }
            Expr::UnaryOp {
                op: UnaryOperator::Minus | UnaryOperator::Plus,
                expr: inner,
            } => self.expr(inner, scope),
            Expr::UnaryOp {
                op: UnaryOperator::PGAbs | UnaryOperator::BitwiseNot,
                expr: inner,
            } if self.dialect == SqlDialect::Postgres => self.expr(inner, scope),
            Expr::AtTimeZone { timestamp, .. } if self.dialect == SqlDialect::Postgres => {
                // `timestamp AT TIME ZONE zone` is a `timestamptz` and vice versa.
                let typed = self.expr(timestamp, scope)?;
                Ok(match operand_type(&typed) {
                    Some("TIMESTAMP") => from_sql_type("TIMESTAMPTZ".to_string(), self.dialect),
                    Some("TIMESTAMPTZ") => from_sql_type("TIMESTAMP".to_string(), self.dialect),
                    _ => (None, None),
                })
            }
            Expr::Ceil { expr: inner, .. } | Expr::Floor { expr: inner, .. } => {
                self.function_type(Returns::Real, &[inner], scope)
            }
            Expr::Extract { .. } if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("DECIMAL".to_string(), self.dialect))
            }
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => Ok(self.column_expr(expr, scope)),
            Expr::Value(value) => match &value.value {
                // PostgreSQL reads `1e3` as a `numeric`.
                Value::Number(number, _) if number.contains(['e', 'E']) => {
                    known(if self.dialect == SqlDialect::Postgres {
                        decimal_type(self.dialect)
                    } else {
                        "float"
                    })
                }
                Value::Number(number, _) if number.contains('.') => {
                    known(decimal_type(self.dialect))
                }
                Value::Number(..) => known("int"),
                Value::Boolean(_) => known("bool"),
                Value::Null | Value::Placeholder(_) => Ok((None, None)),
                _ => known("str"),
            },
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
            } => Ok(from_sql_type("BOOLEAN".to_string(), self.dialect)),
            Expr::Interval(_) => Ok(from_sql_type("INTERVAL".to_string(), self.dialect)),
            Expr::BinaryOp { left, op, right } => self.binary_op(left, op, right, scope),
            Expr::Trim { .. } | Expr::Substring { .. } if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("TEXT".to_string(), self.dialect))
            }
            Expr::Trim { .. } | Expr::Substring { .. } => known("str"),
            Expr::Position { .. } => known("int"),
            Expr::Subquery(query) => Ok(self
                .correlated(scope)
                .query_columns(query)?
                .and_then(|columns| columns.into_iter().next())
                .map_or((None, None), |col| (col.python_type, col.sql_type))),
            Expr::Case {
                conditions,
                else_result,
                ..
            } => {
                if self.dialect == SqlDialect::Postgres {
                    let mut results = Vec::new();
                    for expr in conditions
                        .iter()
                        .map(|when| &when.result)
                        .chain(else_result.as_deref())
                    {
                        results.push(self.expr(expr, scope)?);
                    }
                    if let Some(common) = common_of(results, self.dialect) {
                        return Ok(common);
                    }
                }
                match conditions
                    .first()
                    .map(|when| &when.result)
                    .or(else_result.as_deref())
                {
                    Some(branch) => self.expr(branch, scope),
                    None => Ok((None, None)),
                }
            }
            Expr::Function(func) => {
                let name = func.name.to_string().to_uppercase();
                let args = positional_args(&func.args);
                let arg = args.first().copied();
                match name.as_str() {
                    "COUNT" => Ok(from_sql_type("BIGINT".to_string(), self.dialect)),
                    "SUM" | "MIN" | "MAX" => self.aggregate(&name, arg, scope),
                    _ => match self.returns(&name) {
                        Some(returns) => self.function_type(returns, &args, scope),
                        None => {
                            let typed = self.user_function(&func.name, &func.args);
                            if typed.0.is_none() && self.diag.enabled() {
                                if self.schema.is_empty()
                                    || self.schema.defines_function(&table_key(&func.name))
                                {
                                    self.diag.explain_silently();
                                } else {
                                    self.diag.record(
                                        NameRule::UnknownFunction,
                                        format!(
                                            "function `{}` is not known to sqlty, so its result is untyped",
                                            func.name.to_string().to_lowercase()
                                        ),
                                    );
                                }
                            }
                            Ok(typed)
                        }
                    },
                }
            }
            _ => Ok((None, None)),
        }
    }
}
