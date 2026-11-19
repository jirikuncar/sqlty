//! The SQL assertions of the mdtest suite (`-- columns:`, `-- inferred:`, ...), through the
//! analyzer's public API. `MDTEST_UPDATE=1` writes the actual values.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "a test reads the suite's files and fails on a malformed one"
)]
#![allow(dead_code, reason = "the shared runner serves every crate's kinds")]

#[path = "../../../mdtest/harness.rs"]
mod harness;
#[path = "../../../mdtest/parser.rs"]
mod parser;

use parser::{Section, Unit};
use sqlty_sql::analyzer::{self, Databases, Nullability, Schema, TypeOverrides};
use sqlty_sql::json::{JsonShape, JsonValue};

const KINDS: [&str; 6] = ["inferred", "expanded", "columns", "json", "params", "rules"];

struct Sql;

impl harness::Runner for Sql {
    /// A driver's types come from sqlty-python's configuration: its sections run there.
    fn owns(&self, section: &Section, unit: &Unit, kind: &str) -> bool {
        unit.lang == "sql" && KINDS.contains(&kind) && section.setting("driver").is_none()
    }

    fn compute(&self, section: &Section, unit: &Unit, kind: &str) -> Result<String, String> {
        let dialect = dialect(section.str_setting("dialect").unwrap_or("postgres"));
        let schema = schema(section, dialect)?;
        let schema = schema.as_deref();
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
            "rules" => {
                analyzer::explain_select(&text, dialect, schema).map_or_else(none, |columns| {
                    let id = |rule: Option<analyzer::RuleRef>| rule.map_or("-", |rule| rule.id);
                    lines(columns.into_iter().map(|c| {
                        format!(
                            "{}: {} / {}",
                            c.name,
                            id(c.type_rule),
                            id(c.nullability_rule)
                        )
                    }))
                })
            }
            "params" => {
                analyzer::analyze_params(&text, dialect, schema).map_or_else(none, |params| {
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
                })
            }
            _ => unreachable!("{kind}"),
        })
    }
}

fn lines(lines: impl Iterator<Item = String>) -> String {
    lines.collect::<Vec<_>>().join("\n")
}

fn or_none(value: Option<String>) -> String {
    value.unwrap_or_else(|| "None".to_string())
}

fn dialect(name: &str) -> &'static str {
    sqlty_sql::Dialect::NAMES
        .iter()
        .find(|known| **known == name)
        .copied()
        .unwrap_or("postgres")
}

/// The section's schema with its `types` and other `databases`, as the Python runner loads it.
fn schema(section: &Section, dialect: &'static str) -> Result<Option<Schema>, String> {
    let sql = section.schema_sql();
    let types: Vec<(String, String)> = section
        .setting("types")
        .and_then(toml::Value::as_table)
        .map(|types| {
            types
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let databases = section.setting("databases").and_then(toml::Value::as_table);
    if sql.is_none() && types.is_empty() && databases.is_none() {
        return Ok(None);
    }
    let overrides = TypeOverrides::new(&types, dialect)?;
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

#[test]
fn mdtest() {
    harness::run(&Sql);
}
