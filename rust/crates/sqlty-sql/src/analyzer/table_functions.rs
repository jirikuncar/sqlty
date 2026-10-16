//! Built-in set-returning functions read as row sources in `FROM`: `generate_series`,
//! `unnest`, the JSON `*_each` and `*_array_elements` families, SQLite's `json_each`.

use sqlparser::ast::{
    Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, SelectItem, TableAlias,
};

use crate::catalog::FunctionReturn;

use super::dialect::SqlDialect;
use super::expr::from_sql_type;
use super::functions::{function_name, positional_args};
use super::names::table_key;
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

/// PostgreSQL's built-in set-returning functions.
const SET_RETURNING: &[&str] = &[
    "GENERATE_SERIES",
    "GENERATE_SUBSCRIPTS",
    "UNNEST",
    "REGEXP_MATCHES",
    "REGEXP_SPLIT_TO_TABLE",
    "STRING_TO_TABLE",
    "JSON_ARRAY_ELEMENTS",
    "JSON_ARRAY_ELEMENTS_TEXT",
    "JSONB_ARRAY_ELEMENTS",
    "JSONB_ARRAY_ELEMENTS_TEXT",
    "JSON_EACH",
    "JSON_EACH_TEXT",
    "JSONB_EACH",
    "JSONB_EACH_TEXT",
    "JSON_OBJECT_KEYS",
    "JSONB_OBJECT_KEYS",
    "JSONB_PATH_QUERY",
];

impl Analyzer<'_> {
    /// Whether a call returns a set of rows: a built-in set-returning function, or a
    /// function the schema declares `SETOF` or `TABLE`.
    pub(super) fn is_set_returning(&self, func: &Function) -> bool {
        let count = match &func.args {
            FunctionArguments::List(list) => list.args.len(),
            _ => 0,
        };
        match self
            .schema
            .function(&table_key(&func.name, self.dialect), count)
        {
            Some(FunctionReturn::Scalar(_)) => false,
            Some(_) => true,
            None => SET_RETURNING.contains(&function_name(func).as_str()),
        }
    }

    /// How many of the select list's expressions call a set-returning function, outside
    /// subqueries.
    pub(super) fn set_returning_items(&self, items: &[SelectItem]) -> usize {
        items
            .iter()
            .filter(|item| match item {
                SelectItem::UnnamedExpr(expr)
                | SelectItem::ExprWithAlias { expr, .. }
                | SelectItem::ExprWithAliases { expr, .. } => self.calls_set_returning(expr),
                _ => false,
            })
            .count()
    }

    fn calls_set_returning(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Function(func) => {
                self.is_set_returning(func)
                    || positional_args(&func.args)
                        .into_iter()
                        .any(|arg| self.calls_set_returning(arg))
            }
            Expr::Nested(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::UnaryOp { expr: inner, .. }
            | Expr::Collate { expr: inner, .. } => self.calls_set_returning(inner),
            Expr::BinaryOp { left, right, .. } => {
                self.calls_set_returning(left) || self.calls_set_returning(right)
            }
            Expr::CompoundFieldAccess { root, .. } => self.calls_set_returning(root),
            _ => false,
        }
    }

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
