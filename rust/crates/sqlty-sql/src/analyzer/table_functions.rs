//! Built-in set-returning functions read as row sources in `FROM`: `generate_series`,
//! `unnest`, the JSON `*_each` and `*_array_elements` families, SQLite's `json_each`.

use sqlparser::ast::{Expr, FunctionArg, FunctionArgExpr, TableAlias};

use super::dialect::SqlDialect;
use super::expr::from_sql_type;
use super::nullability::Nullability;
use super::operators::{common_of, operand_type};
use super::query::{rename_columns, Analyzer, Column};
use super::scope::Scope;
use super::typed::Typed;
use super::Inferred;

/// The rows of a built-in function in `FROM`, before the alias is applied.
pub(super) struct FunctionRows {
    pub(super) columns: Vec<Column>,
    /// One unnamed result column, which a bare table alias renames (`generate_series(1, 3)
    /// AS n`); a function with named result columns keeps them.
    pub(super) scalar: bool,
}

impl FunctionRows {
    /// The columns as the query sees them: the alias applied, and `ordinality` added.
    pub(super) fn named(
        mut self,
        alias: Option<&TableAlias>,
        ordinality: bool,
        dialect: SqlDialect,
    ) -> Vec<Column> {
        if ordinality {
            self.columns.push(Column::from_typed(
                "ordinality".to_string(),
                from_sql_type("BIGINT", dialect),
                Nullability::NonNull,
            ));
        }
        if let Some(alias) = alias {
            if self.scalar && alias.columns.is_empty() {
                self.columns[0].name = alias.name.value.to_lowercase();
            }
            rename_columns(&mut self.columns, alias, dialect);
        }
        self.columns
    }
}

impl Analyzer<'_> {
    /// Rows of the built-in function `name` (lower case) called with `args` in `FROM`;
    /// `None` when it is not one the analyzer knows in this dialect.
    pub(super) fn builtin_rows(
        &self,
        name: &str,
        args: &[&Expr],
        scope: &Scope,
    ) -> Inferred<Option<FunctionRows>> {
        let column = |name: &str, sql: &str, nullability| {
            Column::from_typed(
                name.to_string(),
                from_sql_type(sql, self.dialect),
                nullability,
            )
        };
        let scalar = |column: Column| FunctionRows {
            columns: vec![column],
            scalar: true,
        };
        let named = |columns: Vec<Column>| FunctionRows {
            columns,
            scalar: false,
        };
        let (non_null, nullable) = (Nullability::NonNull, Nullability::Nullable);
        if self.dialect == SqlDialect::Sqlite {
            return Ok(match name {
                "json_each" | "json_tree" => Some(named(vec![
                    Column::unknown("key"),
                    Column::unknown("value"),
                    column("type", "TEXT", non_null),
                    Column::unknown("atom"),
                    column("id", "BIGINT", non_null),
                    column("parent", "BIGINT", nullable),
                    column("fullkey", "TEXT", non_null),
                    column("path", "TEXT", non_null),
                ])),
                _ => None,
            });
        }
        if self.dialect != SqlDialect::Postgres {
            return Ok(None);
        }
        Ok(match name {
            "generate_series" => {
                let mut bounds = Vec::new();
                for arg in args.iter().take(2) {
                    bounds.push(self.expr(arg, scope)?);
                }
                let sql = match common_of(bounds, self.dialect)
                    .as_ref()
                    .and_then(operand_type)
                {
                    Some("SMALLINT" | "INT") => Some("INT"),
                    Some("BIGINT") => Some("BIGINT"),
                    Some("DECIMAL" | "NUMERIC") => Some("DECIMAL"),
                    Some("TIMESTAMP") => Some("TIMESTAMP"),
                    Some("TIMESTAMPTZ" | "DATE") => Some("TIMESTAMPTZ"),
                    _ => None,
                };
                let typed = sql.map_or(Typed::UNKNOWN, |sql| from_sql_type(sql, self.dialect));
                Some(scalar(Column::from_typed(
                    name.to_string(),
                    typed,
                    non_null,
                )))
            }
            "unnest" => {
                let mut columns = Vec::new();
                for arg in args {
                    let typed = self.element_typed(self.expr(arg, scope)?.sql());
                    columns.push(Column::from_typed(name.to_string(), typed, nullable));
                }
                Some(FunctionRows {
                    scalar: columns.len() == 1,
                    columns,
                })
            }
            "regexp_split_to_table" | "jsonb_object_keys" | "json_object_keys" => {
                Some(scalar(column(name, "TEXT", non_null)))
            }
            "jsonb_array_elements" => Some(named(vec![column("value", "JSONB", non_null)])),
            "json_array_elements" => Some(named(vec![column("value", "JSON", non_null)])),
            "jsonb_array_elements_text" | "json_array_elements_text" => {
                Some(named(vec![column("value", "TEXT", nullable)]))
            }
            "jsonb_each" => Some(named(vec![
                column("key", "TEXT", non_null),
                column("value", "JSONB", non_null),
            ])),
            "json_each" => Some(named(vec![
                column("key", "TEXT", non_null),
                column("value", "JSON", non_null),
            ])),
            "jsonb_each_text" | "json_each_text" => Some(named(vec![
                column("key", "TEXT", non_null),
                column("value", "TEXT", nullable),
            ])),
            _ => None,
        })
    }
}

/// The leading unnamed expression arguments of a table function call.
pub(super) fn table_args(args: &[FunctionArg]) -> Vec<&Expr> {
    args.iter()
        .map_while(|arg| match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => Some(expr),
            _ => None,
        })
        .collect()
}
