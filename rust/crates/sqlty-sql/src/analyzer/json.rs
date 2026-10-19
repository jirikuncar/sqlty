//! Shapes of the JSON a query builds: `jsonb_build_object()`, `jsonb_agg()`, `to_jsonb()`,
//! `->` and friends (PostgreSQL). The column type stays `Any`; the shape is kept next to it
//! (see [`Column::json`]) so row types can be checked against it.

use std::rc::Rc;

use sqlparser::ast::{BinaryOperator, DataType, Expr, UnaryOperator, Value};

use crate::json::{JsonField, JsonShape, JsonValue};

use super::dialect::SqlDialect;
use super::expr::array_element;
use super::functions::positional_args;
use super::memo::{first_column, Reporting};
use super::names::object_name;
use super::nullability::Nullability;
use super::operators::type_base;
use super::query::{Analyzer, Column};
use super::scope::Scope;
use super::typed::Typed;
use super::{Inferred, ANY, DECIMAL};

/// Deepest composite type or array nesting `to_jsonb()` follows.
const MAX_DEPTH: usize = 8;

fn is_json_type(data_type: &DataType) -> bool {
    matches!(data_type, DataType::JSON | DataType::JSONB)
}

/// The string literal `expr` is, looking through parentheses and casts to text.
fn string_literal(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Nested(inner) => string_literal(inner),
        Expr::Cast { expr: inner, .. } => string_literal(inner),
        Expr::Value(value) => match &value.value {
            Value::SingleQuotedString(text) => Some(text),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn is_null_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Nested(inner) => is_null_literal(inner),
        Expr::Value(value) => matches!(value.value, Value::Null),
        _ => false,
    }
}

/// The index `expr` is, such as `0` or `-1`.
fn index_literal(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Nested(inner) => index_literal(inner),
        Expr::Value(value) => match &value.value {
            Value::Number(number, _) => number.parse().ok(),
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr: inner,
        } => index_literal(inner).map(|index| -index),
        _ => None,
    }
}

/// The keys of a text path: `'{a,b}'` or `ARRAY['a', 'b']`.
fn path_keys(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Nested(inner) => path_keys(inner),
        Expr::Array(array) => array
            .elem
            .iter()
            .map(|elem| string_literal(elem).map(str::to_string))
            .collect(),
        _ => {
            let text = string_literal(expr)?.trim();
            let inner = text.strip_prefix('{')?.strip_suffix('}')?;
            if inner.trim().is_empty() {
                return Some(Vec::new());
            }
            Some(
                inner
                    .split(',')
                    .map(|key| key.trim().trim_matches('"').to_string())
                    .collect(),
            )
        }
    }
}

/// What `->` or `#>` extracts: its shape, whether it can be missing (SQL `NULL`) and
/// whether it can be a JSON `null`.
#[derive(Clone, Debug)]
pub(super) struct Extracted {
    pub(super) shape: JsonShape,
    pub(super) missing: bool,
    pub(super) null: bool,
}

impl Extracted {
    const UNKNOWN: Extracted = Extracted {
        shape: JsonShape::Any,
        missing: true,
        null: true,
    };
}

/// The member `key` of an object or map.
fn member(base: &JsonShape, key: &str) -> Extracted {
    match base {
        JsonShape::Object(fields) => {
            fields
                .iter()
                .find(|field| field.name == key)
                .map_or(Extracted::UNKNOWN, |field| Extracted {
                    shape: field.value.shape.clone(),
                    missing: field.optional,
                    null: field.value.nullable,
                })
        }
        JsonShape::Map(value) => Extracted {
            shape: value.shape.clone(),
            missing: true,
            null: value.nullable,
        },
        _ => Extracted::UNKNOWN,
    }
}

/// An element of an array, missing when the index is out of range.
fn element(base: &JsonShape) -> Extracted {
    match base {
        JsonShape::Array(value) => Extracted {
            shape: value.shape.clone(),
            missing: true,
            null: value.nullable,
        },
        _ => Extracted::UNKNOWN,
    }
}

/// `base` followed along the path keys, an array being indexed by a number. A JSON
/// `null` on the way makes the rest of the path missing.
fn follow(base: &JsonShape, keys: &[String]) -> Extracted {
    let start = Extracted {
        shape: base.clone(),
        missing: false,
        null: false,
    };
    keys.iter().fold(start, |seen, key| {
        let next = match seen.shape {
            JsonShape::Array(_) if key.parse::<i64>().is_ok() => element(&seen.shape),
            JsonShape::Object(_) | JsonShape::Map(_) => member(&seen.shape, key),
            _ => Extracted::UNKNOWN,
        };
        Extracted {
            shape: next.shape,
            missing: seen.missing || seen.null || next.missing,
            null: next.null,
        }
    })
}

/// The value that is any of `values`; unknown (`Any`) when there are none.
fn unify_all(values: impl IntoIterator<Item = JsonShape>) -> JsonShape {
    values
        .into_iter()
        .reduce(|a, b| a.unify(&b))
        .unwrap_or(JsonShape::Any)
}

impl<'a> Analyzer<'a> {
    /// The shape of the JSON a projected expression builds, when it is JSON of a known
    /// shape; `sql_type` is the type of the expression.
    pub(super) fn column_json(
        &self,
        expr: &Expr,
        sql_type: Option<&str>,
        scope: &Scope,
    ) -> Inferred<Option<JsonShape>> {
        if !matches!(self.dialect, SqlDialect::Postgres | SqlDialect::MySql)
            || !matches!(sql_type.map(type_base), Some("JSON" | "JSONB"))
        {
            return Ok(None);
        }
        let shape = self.json_shape(expr, scope)?;
        Ok((shape != JsonShape::Any).then_some(shape))
    }

    /// The shape of a JSON expression; `Any` when it is not known.
    #[recursive::recursive]
    fn json_shape(&self, expr: &Expr, scope: &Scope) -> Inferred<JsonShape> {
        match expr {
            Expr::Nested(inner) => self.json_shape(inner, scope),
            Expr::Cast {
                expr: inner,
                data_type,
                ..
            } if is_json_type(data_type) => self.json_operand(inner, scope),
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => Ok(self.column_shape(expr, scope)),
            Expr::Subquery(query) => {
                Ok(
                    first_column(&self.subquery_columns(query, scope, Reporting::Record))?
                        .and_then(|col| col.json.clone())
                        .unwrap_or(JsonShape::Any),
                )
            }
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Arrow | BinaryOperator::HashArrow,
                right,
            } => Ok(self.extracted(left, right, expr, scope)?.shape),
            Expr::Case {
                operand,
                conditions,
                else_result,
                ..
            } => {
                // A branch is read where its WHEN holds and the earlier ones do not.
                let mut skipped = self.facts.clone();
                let mut shapes = Vec::new();
                for when in conditions {
                    let condition = match operand.as_deref() {
                        Some(operand) => Expr::BinaryOp {
                            left: Box::new(operand.clone()),
                            op: BinaryOperator::Eq,
                            right: Box::new(when.condition.clone()),
                        },
                        None => when.condition.clone(),
                    };
                    if !is_null_literal(&when.result) {
                        let mut inner = self.clone();
                        inner.facts = skipped.with(self.learn(&condition, true, scope));
                        Rc::make_mut(&mut inner.facts.present)
                            .extend(self.row_guards(&condition, scope));
                        shapes.push(inner.json_operand(&when.result, scope)?);
                    }
                    let unmatched = self.learn_not_true(&condition, scope, &skipped);
                    skipped = skipped.with(unmatched);
                }
                if let Some(result) = else_result.as_deref().filter(|e| !is_null_literal(e)) {
                    let mut inner = self.clone();
                    inner.facts = skipped;
                    shapes.push(inner.json_operand(result, scope)?);
                }
                Ok(unify_all(shapes))
            }
            Expr::Function(func) if func.over.is_none() => {
                let name = self.builtin_name(&func.name);
                let args = positional_args(&func.args);
                match &func.filter {
                    // The rows of `FILTER (WHERE ...)` satisfy the condition.
                    Some(filter) => {
                        let mut inner = self.clone();
                        inner.facts = self.facts.with(self.learn(filter, true, scope));
                        Rc::make_mut(&mut inner.facts.present)
                            .extend(self.row_guards(filter, scope));
                        inner.json_function(&name, &args, scope)
                    }
                    None => self.json_function(&name, &args, scope),
                }
            }
            _ => Ok(JsonShape::Any),
        }
    }

    /// What `left -> right` or `left #> right` (`op` in `expr`) extracts from a JSON value
    /// of known shape.
    fn extracted(
        &self,
        left: &Expr,
        right: &Expr,
        expr: &Expr,
        scope: &Scope,
    ) -> Inferred<Extracted> {
        let base = self.json_shape(left, scope)?;
        let unknown = Extracted::UNKNOWN;
        Ok(match expr {
            Expr::BinaryOp {
                op: BinaryOperator::HashArrow,
                ..
            } => path_keys(right).map_or(unknown, |keys| follow(&base, &keys)),
            _ => match (string_literal(right), index_literal(right)) {
                (Some(key), _) => member(&base, key),
                (None, Some(_)) => element(&base),
                _ => unknown,
            },
        })
    }

    /// What PostgreSQL's `->` or `#>` in `expr` extracts from a JSON value; `None` for any
    /// other expression.
    pub(super) fn json_extract(&self, expr: &Expr, scope: &Scope) -> Option<Extracted> {
        if let Expr::Function(func) = expr {
            let name = self.builtin_name(&func.name);
            let args = positional_args(&func.args);
            return match (name.as_str(), args.split_first()) {
                ("JSONB_EXTRACT_PATH" | "JSON_EXTRACT_PATH", Some((base, keys)))
                    if self.dialect == SqlDialect::Postgres && func.over.is_none() =>
                {
                    self.extract_path(base, keys, scope).ok()
                }
                _ => None,
            };
        }
        let Expr::BinaryOp {
            left,
            op: BinaryOperator::Arrow | BinaryOperator::HashArrow,
            right,
        } = expr
        else {
            return None;
        };
        if self.dialect != SqlDialect::Postgres
            || !matches!(
                self.expr(left, scope).ok()?.sql().map(type_base),
                Some("JSON" | "JSONB")
            )
        {
            return None;
        }
        self.extracted(left, right, expr, scope).ok()
    }

    /// `jsonb_extract_path(base, key, ...)`: `base #> '{key, ...}'` when every key is a
    /// literal.
    fn extract_path(&self, base: &Expr, keys: &[&Expr], scope: &Scope) -> Inferred<Extracted> {
        let shape = self.json_shape(base, scope)?;
        let keys: Option<Vec<String>> = keys
            .iter()
            .map(|key| string_literal(key).map(str::to_string))
            .collect();
        Ok(keys.map_or(Extracted::UNKNOWN, |keys| follow(&shape, &keys)))
    }

    /// References of the relations a condition proves present with `r IS NOT NULL` or
    /// `r.* IS NOT NULL`, which `learn` does not read.
    fn row_guards(&self, cond: &Expr, scope: &Scope) -> Vec<String> {
        match cond {
            Expr::Nested(inner) => self.row_guards(inner, scope),
            Expr::BinaryOp {
                left,
                op: BinaryOperator::And,
                right,
            } => {
                let mut found = self.row_guards(left, scope);
                found.extend(self.row_guards(right, scope));
                found
            }
            Expr::IsNotNull(inner) => {
                let reference = match inner.as_ref() {
                    Expr::Identifier(ident) if self.whole_row(inner, scope).is_some() => {
                        ident.value.to_lowercase()
                    }
                    Expr::QualifiedWildcard(name, _) => object_name(name).to_lowercase(),
                    _ => return Vec::new(),
                };
                if scope.iter().any(|source| source.reference == reference) {
                    vec![reference]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// A JSON operand: a string literal is JSON text where JSON is expected, as in
    /// `COALESCE(jsonb_agg(x), '[]')`.
    fn json_operand(&self, expr: &Expr, scope: &Scope) -> Inferred<JsonShape> {
        match expr {
            Expr::Value(_) => Ok(string_literal(expr)
                .and_then(JsonShape::of_literal)
                .unwrap_or(JsonShape::Any)),
            _ => self.json_shape(expr, scope),
        }
    }

    fn json_function(&self, name: &str, args: &[&Expr], scope: &Scope) -> Inferred<JsonShape> {
        Ok(match (name, args) {
            ("JSON_BUILD_OBJECT" | "JSONB_BUILD_OBJECT", _) => self.build_object(args, scope)?,
            ("JSON_OBJECT", _) if self.dialect == SqlDialect::MySql => {
                self.build_object(args, scope)?
            }
            ("JSON_ARRAYAGG", [arg]) if self.dialect == SqlDialect::MySql => {
                JsonShape::Array(Box::new(self.json_value(arg, scope)?))
            }
            ("JSON_OBJECTAGG", [_, value]) if self.dialect == SqlDialect::MySql => {
                JsonShape::Map(Box::new(self.json_value(value, scope)?))
            }
            ("JSON_BUILD_ARRAY" | "JSONB_BUILD_ARRAY" | "JSON_ARRAY", _)
                if name != "JSON_ARRAY" || self.dialect == SqlDialect::MySql =>
            {
                let mut element: Option<JsonValue> = None;
                for arg in args {
                    let value = self.json_value(arg, scope)?;
                    element =
                        Some(element.map_or(value.clone(), |seen| JsonValue::unify(&seen, &value)));
                }
                let element = element.unwrap_or_else(|| JsonValue::new(JsonShape::Never, false));
                JsonShape::Array(Box::new(element))
            }
            ("JSON_AGG" | "JSONB_AGG", [arg]) => {
                JsonShape::Array(Box::new(self.json_value(arg, scope)?))
            }
            (
                "JSON_OBJECT_AGG"
                | "JSONB_OBJECT_AGG"
                | "JSON_OBJECT_AGG_STRICT"
                | "JSONB_OBJECT_AGG_STRICT"
                | "JSON_OBJECT_AGG_UNIQUE"
                | "JSONB_OBJECT_AGG_UNIQUE"
                | "JSON_OBJECT_AGG_UNIQUE_STRICT"
                | "JSONB_OBJECT_AGG_UNIQUE_STRICT",
                [_, value],
            ) => JsonShape::Map(Box::new(self.json_value(value, scope)?)),
            ("TO_JSON" | "TO_JSONB", [arg]) => self.json_value(arg, scope)?.shape,
            ("ROW_TO_JSON", [arg]) => self
                .whole_row(arg, scope)
                .map_or(JsonShape::Any, |(_, row)| row.shape),
            ("COALESCE", _) => {
                let mut shapes = Vec::new();
                for arg in args.iter().filter(|arg| !is_null_literal(arg)) {
                    shapes.push(self.json_operand(arg, scope)?);
                }
                unify_all(shapes)
            }
            ("NULLIF", [first, _]) => self.json_operand(first, scope)?,
            ("JSONB_EXTRACT_PATH" | "JSON_EXTRACT_PATH", [base, keys @ ..]) => {
                self.extract_path(base, keys, scope)?.shape
            }
            _ => JsonShape::Any,
        })
    }

    /// `jsonb_build_object(k1, v1, k2, v2, ...)`: an object when every key is a literal,
    /// else a map of the values.
    fn build_object(&self, args: &[&Expr], scope: &Scope) -> Inferred<JsonShape> {
        if !args.len().is_multiple_of(2) {
            return Ok(JsonShape::Any);
        }
        let mut fields: Vec<JsonField> = Vec::new();
        let mut values: Vec<JsonValue> = Vec::new();
        let mut literal_keys = true;
        for pair in args.as_chunks::<2>().0 {
            let value = self.json_value(pair[1], scope)?;
            match string_literal(pair[0]) {
                // The last of repeated keys wins, as in `jsonb`.
                Some(key) => {
                    fields.retain(|field| field.name != key);
                    fields.push(JsonField {
                        name: key.to_string(),
                        value: value.clone(),
                        optional: false,
                    });
                }
                None => literal_keys = false,
            }
            values.push(value);
        }
        if literal_keys {
            return Ok(JsonShape::Object(fields));
        }
        let value = values
            .into_iter()
            .reduce(|a, b| JsonValue::unify(&a, &b))
            .unwrap_or_else(|| JsonValue::new(JsonShape::Never, false));
        Ok(JsonShape::Map(Box::new(value)))
    }

    /// The JSON a SQL value converts to, and whether it can be `null`.
    fn json_value(&self, expr: &Expr, scope: &Scope) -> Inferred<JsonValue> {
        if let Some((reference, row)) = self.whole_row(expr, scope) {
            return Ok(JsonValue::new(
                row.shape,
                row.nullable || self.facts.is_null_extended(&reference),
            ));
        }
        // psycopg loads a JSON `null` as `None`, like a SQL `NULL`.
        let nullable = self.nullability(expr, scope, &self.facts) == Nullability::Nullable
            || self
                .json_extract(expr, scope)
                .is_some_and(|value| value.null);
        let typed = self.expr(expr, scope)?;
        let shape = if matches!(typed.sql().map(type_base), Some("JSON" | "JSONB")) {
            self.json_shape(expr, scope)?
        } else {
            self.shape_of_type(&typed, 0)
        };
        // MySQL has no boolean type: a BOOLEAN column is a TINYINT, which JSON gets as a
        // number; only TRUE and FALSE are JSON booleans.
        let shape = match (self.dialect, &shape, expr) {
            (SqlDialect::MySql, JsonShape::Boolean, Expr::Value(_)) => shape,
            (
                SqlDialect::MySql,
                JsonShape::Boolean,
                Expr::Identifier(_) | Expr::CompoundIdentifier(_),
            ) => JsonShape::Integer,
            (SqlDialect::MySql, JsonShape::Boolean, _) => JsonShape::Any,
            _ => shape,
        };
        Ok(JsonValue::new(shape, nullable))
    }

    /// A reference to a whole row (`to_jsonb(t)`, `jsonb_agg(t)`): the source's columns as
    /// an object, and the reference. A column called like the source wins, as in PostgreSQL.
    fn whole_row(&self, expr: &Expr, scope: &Scope) -> Option<(String, JsonValue)> {
        let Expr::Identifier(ident) = expr else {
            return None;
        };
        let reference = ident.value.to_lowercase();
        if !scope.iter().any(|source| source.reference == reference)
            || self.locate_column(expr, scope).is_some()
        {
            return None;
        }
        let columns = self.star_columns(Some(&reference), scope)?;
        if columns.iter().any(Column::is_unknown_star) {
            return None;
        }
        let fields = columns
            .iter()
            .map(|col| JsonField {
                name: col.name.clone(),
                value: JsonValue::new(self.column_shape_of(col), col.nullable()),
                optional: false,
            })
            .collect();
        Some((reference, JsonValue::new(JsonShape::Object(fields), false)))
    }

    /// The shape of the JSON a column of a CTE, view or derived table holds.
    fn column_shape(&self, expr: &Expr, scope: &Scope) -> JsonShape {
        let Some((_, table, column)) = self.locate_column(expr, scope) else {
            return JsonShape::Any;
        };
        self.virtual_columns(&table)
            .and_then(|columns| {
                columns
                    .iter()
                    .find(|col| self.same_column(&col.name, &column))
            })
            .and_then(|col| col.json.clone())
            .unwrap_or(JsonShape::Any)
    }

    /// The JSON a column converts to.
    fn column_shape_of(&self, col: &Column) -> JsonShape {
        match (&col.json, col.sql_type.as_deref().map(type_base)) {
            (Some(shape), _) => shape.clone(),
            (None, Some("JSON" | "JSONB")) => JsonShape::Any,
            _ => self.shape_of_type(&col.typed(), 0),
        }
    }

    /// The shape of the JSON a SQL value of this type converts to (`to_jsonb()`).
    fn shape_of_type(&self, typed: &Typed, depth: usize) -> JsonShape {
        if depth > MAX_DEPTH {
            return JsonShape::Any;
        }
        if let Some(sql_type) = typed.sql() {
            if array_element(sql_type).is_some() {
                let elem = self.element_typed(Some(sql_type));
                let value = JsonValue::new(self.shape_of_type(&elem, depth + 1), false);
                return JsonShape::Array(Box::new(value));
            }
            if let Some(fields) = self.schema.composite(&sql_type.to_lowercase()) {
                return JsonShape::Object(
                    fields
                        .iter()
                        .map(|field| JsonField {
                            name: field.name.clone(),
                            value: JsonValue::new(
                                self.shape_of_type(&field.typed(self.dialect), depth + 1),
                                !field.not_null,
                            ),
                            optional: false,
                        })
                        .collect(),
                );
            }
        }
        match typed.python() {
            None | Some(ANY) => JsonShape::Any,
            Some("bool") => JsonShape::Boolean,
            Some("int") => JsonShape::Integer,
            // A value set is its `str` or `int` base.
            Some(python) if crate::values::base(python) == Some("int") => JsonShape::Integer,
            Some("float" | DECIMAL) => JsonShape::Number,
            Some(python) if is_container(python) => JsonShape::Any,
            // Text, dates, UUIDs, enums, ...: converted to strings.
            Some(_) => JsonShape::String,
        }
    }
}

/// Whether a Python type is a builtin container, which converts to no known JSON.
fn is_container(python_type: &str) -> bool {
    ["list[", "dict[", "tuple[", "set["]
        .iter()
        .any(|prefix| python_type.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{analyze_select, load_schema};

    const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, bio TEXT);
         CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total NUMERIC NOT NULL);";

    fn shape(sql: &str) -> Option<String> {
        let schema = load_schema(SCHEMA, "postgres");
        let columns = analyze_select(sql, "postgres", Some(&schema))
            .unwrap()
            .unwrap();
        columns[0].json.as_ref().map(JsonShape::to_json)
    }

    #[test]
    fn build_object_with_literal_keys_is_an_object() {
        assert_eq!(
            shape("SELECT jsonb_build_object('id', id, 'bio', bio) FROM users").unwrap(),
            r#"{"object":[{"name":"id","value":{"shape":"integer","nullable":false},"optional":false},{"name":"bio","value":{"shape":"string","nullable":true},"optional":false}]}"#
        );
    }

    #[test]
    fn build_object_with_computed_keys_is_a_map() {
        assert_eq!(
            shape("SELECT jsonb_build_object(name, id) FROM users").unwrap(),
            r#"{"map":{"shape":"integer","nullable":false}}"#
        );
    }

    #[test]
    fn coalesce_of_agg_and_empty_array_keeps_the_element() {
        assert_eq!(
            shape("SELECT coalesce(jsonb_agg(id), '[]') FROM users").unwrap(),
            r#"{"array":{"shape":"integer","nullable":false}}"#
        );
    }

    #[test]
    fn row_to_json_of_a_table_lists_its_columns() {
        assert!(shape("SELECT row_to_json(u) FROM users u")
            .unwrap()
            .starts_with(r#"{"object":[{"name":"id""#));
    }

    #[test]
    fn arrow_follows_a_member() {
        assert_eq!(
            shape("SELECT jsonb_build_object('a', jsonb_build_object('b', 1)) -> 'a' FROM users")
                .unwrap(),
            r#"{"object":[{"name":"b","value":{"shape":"integer","nullable":false},"optional":false}]}"#
        );
    }

    #[test]
    fn json_columns_of_unknown_shape_have_none() {
        assert_eq!(
            shape("SELECT to_jsonb(bio) FROM users").as_deref(),
            Some("\"string\"")
        );
        assert_eq!(shape("SELECT '{}'::jsonb || '{}'::jsonb FROM users"), None);
    }

    #[test]
    fn filter_keeps_only_the_matched_rows_of_an_outer_join() {
        let sql =
            "SELECT jsonb_agg(jsonb_build_object('t', o.total)) FILTER (WHERE o.id IS NOT NULL)
                   FROM users u LEFT JOIN orders o ON o.user_id = u.id";
        assert_eq!(
            shape(sql).unwrap(),
            r#"{"array":{"shape":{"object":[{"name":"t","value":{"shape":"number","nullable":false},"optional":false}]},"nullable":false}}"#
        );
    }

    #[test]
    fn row_guards_in_filter_count_as_null_guards() {
        for guard in ["o IS NOT NULL", "o.* IS NOT NULL", "o.user_id IS NOT NULL"] {
            let sql = format!(
                "SELECT jsonb_agg(jsonb_build_object('t', o.total)) FILTER (WHERE {guard})
                 FROM users u LEFT JOIN orders o ON o.user_id = u.id"
            );
            assert!(
                shape(&sql).unwrap().contains(r#""nullable":false}"#),
                "{guard}"
            );
            assert!(
                !shape(&sql).unwrap().contains(r#""nullable":true"#),
                "{guard}"
            );
        }
    }

    #[test]
    fn union_of_objects_makes_missing_members_optional() {
        let sql = "SELECT jsonb_build_object('a', 1) FROM users
                   UNION ALL SELECT jsonb_build_object('b', 1) FROM users";
        assert!(shape(sql).unwrap().contains(r#""optional":true"#));
    }

    #[test]
    fn union_with_a_branch_of_unknown_shape_has_none() {
        let sql = "SELECT jsonb_build_object('a', 1) FROM users UNION ALL SELECT '{}'::jsonb || '{}'::jsonb FROM users";
        assert_eq!(shape(sql), None);
    }

    #[test]
    fn other_dialects_have_no_shapes() {
        let columns = analyze_select("SELECT json_object('a', 1)", "sqlite", None)
            .unwrap()
            .unwrap();
        assert_eq!(columns[0].json, None);
    }

    #[test]
    fn mysql_json_object_is_an_object() {
        let columns = analyze_select("SELECT json_object('a', 1, 'b', TRUE)", "mysql", None)
            .unwrap()
            .unwrap();
        assert_eq!(
            columns[0].json.as_ref().map(JsonShape::to_json),
            analyze_select(
                "SELECT jsonb_build_object('a', 1, 'b', TRUE)",
                "postgres",
                None
            )
            .unwrap()
            .unwrap()[0]
                .json
                .as_ref()
                .map(JsonShape::to_json)
        );
    }
}
