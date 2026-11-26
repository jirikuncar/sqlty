//! The SQL assertions (`-- columns:`, `-- inferred:`, ...) through the analyzer's public API,
//! for sqlty-sql and for the `driver` sections sqlty-python runs.

use super::parser::{Section, Unit};
use sqlty_sql::analyzer::{self, Databases, Nullability, Schema, TypeOverrides};
use sqlty_sql::json::{JsonShape, JsonValue};

pub const KINDS: [&str; 6] = ["inferred", "expanded", "columns", "json", "params", "rules"];

pub fn compute(
    section: &Section,
    unit: &Unit,
    kind: &str,
    schema: Option<&Schema>,
) -> Result<String, String> {
    let dialect = dialect(section.str_setting("dialect").unwrap_or("postgres"));
    let schema = schema.map(|schema| &**schema);
    let text = unit.text();
    let none = || "None".to_string();
    Ok(match kind {
        "inferred" => analyzer::infer_type(&text, dialect, schema).unwrap_or_else(none),
        "expanded" => analyzer::expand_star(&text, dialect, schema).unwrap_or_else(none),
        "columns" => {
            analyzer::analyze_select(&text, dialect, schema).map_or_else(none, |columns| {
                lines(columns.iter().map(|c| {
                    format!(
                        "{}: {} ({})",
                        c.name,
                        or_none(c.display_type()),
                        or_none(c.sql_type.clone())
                    )
                }))
            })
        }
        "json" => {
            let columns = analyzer::analyze_select(&text, dialect, schema).unwrap_or_default();
            let shapes = lines(columns.iter().filter_map(|c| {
                c.json
                    .as_ref()
                    .map(|shape| format!("{}: {}", c.name, render(shape)))
            }));
            if shapes.is_empty() {
                none()
            } else {
                shapes
            }
        }
        "rules" => analyzer::explain_select(&text, dialect, schema).map_or_else(none, |columns| {
            let id = |rule: Option<analyzer::RuleRef>| rule.map_or("-", |rule| rule.id);
            lines(columns.into_iter().map(|c| {
                format!(
                    "{}: {} / {}",
                    c.name,
                    id(c.type_rule),
                    id(c.nullability_rule)
                )
            }))
        }),
        "params" => analyzer::analyze_params(&text, dialect, schema).map_or_else(none, |params| {
            lines(params.into_iter().map(|p| {
                let null = match p.nullability {
                    Nullability::Nullable => " NULL",
                    Nullability::NonNull => " NOT NULL",
                    Nullability::Unknown => "",
                };
                let name = p.name.unwrap_or_else(|| p.index.to_string());
                format!(
                    "{name}: {} ({}){null}",
                    or_none(p.python_type),
                    or_none(p.sql_type)
                )
            }))
        }),
        _ => return Err(format!("not a SQL assertion: {kind}")),
    })
}

fn lines(lines: impl Iterator<Item = String>) -> String {
    lines.collect::<Vec<_>>().join("\n")
}

fn or_none(value: Option<String>) -> String {
    value.unwrap_or_else(|| "None".to_string())
}

pub fn dialect(name: &str) -> &'static str {
    sqlty_sql::Dialect::NAMES
        .iter()
        .find(|known| **known == name)
        .copied()
        .unwrap_or("postgres")
}

/// The section's `types`, under `defaults` as a Python dict merge puts them.
pub fn types(section: &Section, defaults: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut types = defaults;
    let own = section.setting("types").and_then(toml::Value::as_table);
    for (key, value) in own.into_iter().flatten() {
        let value = value.as_str().unwrap_or_default().to_string();
        match types.iter_mut().find(|(known, _)| known == key) {
            Some(known) => known.1 = value,
            None => types.push((key.clone(), value)),
        }
    }
    let deprecated = section
        .setting("deprecated-columns")
        .and_then(toml::Value::as_table);
    for (column, options) in deprecated.into_iter().flatten() {
        let part = |name: &str| options.get(name).and_then(toml::Value::as_str);
        types.push(analyzer::deprecated_type(
            column,
            part("use"),
            part("remove-after"),
        ));
    }
    if let Some(tenancy) = section.setting("tenancy").and_then(toml::Value::as_table) {
        let list = |key: &str| -> Vec<&str> {
            let listed = tenancy.get(key).and_then(toml::Value::as_array);
            listed
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .collect()
        };
        types.push(analyzer::tenancy_type(
            tenancy
                .get("table")
                .and_then(toml::Value::as_str)
                .unwrap_or_default(),
            &list("exclude"),
            &list("columns"),
            &list("session-functions"),
        ));
    }
    types
}

/// The section's schema with `types` and its other `databases`, as the Python runner loads it.
pub fn schema(section: &Section, types: &[(String, String)]) -> Result<Option<Schema>, String> {
    let dialect = dialect(section.str_setting("dialect").unwrap_or("postgres"));
    let sql = section.schema_sql();
    let databases = section.setting("databases").and_then(toml::Value::as_table);
    if sql.is_none() && types.is_empty() && databases.is_none() {
        return Ok(None);
    }
    let overrides = TypeOverrides::new(types, dialect)?;
    let text = sql.as_deref().unwrap_or("");
    let (schema, _) = if overrides.is_empty() {
        analyzer::load_schema_with_skipped(text, dialect)
    } else {
        analyzer::load_schema_with_overrides(text, dialect, overrides)
    };
    let Some(databases) = databases else {
        return Ok(Some(schema));
    };
    let others = databases
        .iter()
        .map(|(name, database)| {
            let other = self::dialect(
                database
                    .get("dialect")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(dialect),
            );
            let sql = section.joined(database.get("schema"));
            let schema = sql.map(|sql| analyzer::load_schema_with_skipped(&sql, other).0);
            (name.clone(), other, schema)
        })
        .collect();
    let databases = Databases {
        name: analyzer::DEFAULT.to_string(),
        absent: sql.is_none(),
        others,
    };
    Ok(analyzer::SchemaData::with_databases(schema, databases))
}

/// A JSON shape as `sqlty._json_shape.render` writes it.
fn render(shape: &JsonShape) -> String {
    let value = |value: &JsonValue| {
        let text = render(&value.shape);
        if value.nullable {
            format!("{text} | null")
        } else {
            text
        }
    };
    match shape {
        JsonShape::Any => "any".into(),
        JsonShape::Never => "never".into(),
        JsonShape::Null => "null".into(),
        JsonShape::Boolean => "boolean".into(),
        JsonShape::Integer => "integer".into(),
        JsonShape::Number => "number".into(),
        JsonShape::String => "string".into(),
        JsonShape::Array(item) => format!("[{}]", value(item)),
        JsonShape::Map(item) => format!("{{[key]: {}}}", value(item)),
        JsonShape::Object(fields) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|f| {
                    format!(
                        "{}{}: {}",
                        f.name,
                        if f.optional { "?" } else { "" },
                        value(&f.value)
                    )
                })
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
    }
}
