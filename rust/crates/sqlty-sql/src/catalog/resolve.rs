//! Resolution: raw tables, views and types into the [`Catalog`] the analyzer reads.
//!
//! `Loader::build` resolves everything; `schema_column` and `resolve_rel` resolve
//! one relation when the loader updates the cached catalog in place.

use std::collections::HashMap;

use sqlparser::ast::{ArrayElemTypeDef, DataType, EnumMember};

use crate::analyzer::{canonical_type, typed, SqlDialect};
use crate::values::{self, Value};

use super::columns::{narrowed, Checks};
use super::loader::{Loader, RawReturn, Rel, TypeDef};
use super::names::{lookup_key, type_key};
use super::{Catalog, Function, FunctionReturn, Relation, SchemaColumn, UserType, BUILTIN_SCHEMAS};

impl Loader {
    /// The catalog as it stands now.
    pub(super) fn snapshot(&mut self) -> &Catalog {
        let catalog = self.cache.take().unwrap_or_else(|| self.build());
        self.cache.insert(catalog)
    }

    pub(super) fn build(&self) -> Catalog {
        let dialect = self.dialect;
        let path = self.search_path.clone();
        let mut user_types: HashMap<String, UserType> = HashMap::new();
        for key in self.types.keys() {
            if let Some(user) = self.resolve_user_type(key, &path, 0) {
                user_types.insert(key.clone(), user);
            }
        }
        for (old, new) in &self.type_aliases {
            if let Some(user) = user_types.get(new).cloned() {
                user_types.entry(old.clone()).or_insert(user);
            }
        }
        let lookup = |name: &str| -> Option<UserType> {
            lookup_key(&user_types, &path, name).and_then(|key| user_types.get(key).cloned())
        };
        let column =
            |name: &str, ident: &str, ty: &DataType, not_null: bool, checked: Checks<'_>| {
                let (sql_type, python_type) = resolve_with(ty, &lookup, dialect);
                let allowed_values = type_key(ty, dialect)
                    .and_then(|name| lookup(&name))
                    .and_then(|user| user.allowed_values);
                let python_type = checked.apply(python_type, &sql_type, allowed_values.is_some());
                SchemaColumn {
                    name: name.to_string(),
                    ident: ident.to_string(),
                    sql_type,
                    python_type,
                    not_null,
                    allowed_values,
                }
            };
        let mut relations = HashMap::new();
        let mut user_relations = 0;
        for (key, rel) in &self.relations {
            if !BUILTIN_SCHEMAS
                .iter()
                .any(|s| key.starts_with(&format!("{s}.")))
            {
                user_relations += 1;
            }
            relations.insert(
                key.clone(),
                match rel {
                    Rel::Table(table) if table.unknown => Relation::Unknown,
                    Rel::Table(table) => Relation::Table(
                        table
                            .columns
                            .iter()
                            .map(|c| column(&c.name, &c.ident, &c.ty, c.not_null, c.checks()))
                            .collect(),
                    ),
                    Rel::View(columns) => Relation::View(columns.clone()),
                },
            );
        }
        let mut composites: HashMap<String, Vec<SchemaColumn>> = self
            .types
            .iter()
            .filter_map(|(key, def)| match def {
                TypeDef::Composite(fields) => Some((
                    key.clone(),
                    fields
                        .iter()
                        .map(|(name, ty)| column(name, name, ty, false, Checks::default()))
                        .collect(),
                )),
                _ => None,
            })
            .collect();
        for (old, new) in &self.type_aliases {
            if let Some(fields) = composites.get(new).cloned() {
                composites.entry(old.clone()).or_insert(fields);
            }
        }
        let mut functions = HashMap::new();
        for (key, overloads) in &self.functions {
            if self.unknown_functions.contains(key) {
                continue;
            }
            let mut built = Vec::new();
            for f in overloads {
                let returns = match &f.returns {
                    RawReturn::Rows(columns) => FunctionReturn::Rows(
                        columns
                            .iter()
                            .map(|(name, ty)| column(name, name, ty, false, Checks::default()))
                            .collect(),
                    ),
                    RawReturn::Scalar(ty) | RawReturn::SetOf(ty) => {
                        let rows = type_key(ty, dialect)
                            .filter(|_| matches!(f.returns, RawReturn::SetOf(_)))
                            .and_then(|name| {
                                lookup_key(&relations, &path, &name).map(str::to_string)
                            })
                            .and_then(|key| match relations.get(&key) {
                                Some(Relation::Table(columns)) => Some(columns.clone()),
                                _ => None,
                            });
                        match rows {
                            Some(columns) => FunctionReturn::Rows(columns),
                            None => {
                                let (sql, python) = resolve_with(ty, &lookup, dialect);
                                let value = if sql == "VOID" {
                                    (Some("None".to_string()), Some(sql))
                                } else {
                                    typed(sql, python, dialect)
                                };
                                if matches!(f.returns, RawReturn::SetOf(_)) {
                                    FunctionReturn::SetOf(value)
                                } else {
                                    FunctionReturn::Scalar(value)
                                }
                            }
                        }
                    }
                };
                built.push(Function {
                    required: f.required,
                    max: if f.variadic { usize::MAX } else { f.args.len() },
                    returns,
                });
            }
            functions.insert(key.clone(), built);
        }
        Catalog {
            relations,
            types: user_types,
            composites,
            functions,
            unknown_functions: self
                .unknown_functions
                .iter()
                .map(|key| (key.clone(), ()))
                .collect(),
            type_names: self
                .types
                .keys()
                .chain(self.type_aliases.keys())
                .map(|key| (key.clone(), ()))
                .collect(),
            search_path: path,
            user_relations,
            postgres: self.postgres,
        }
    }

    /// The type a domain, enum or composite `key` gives its columns; domains over other
    /// types resolve through them, up to a fixed depth.
    pub(super) fn resolve_user_type(
        &self,
        key: &str,
        path: &[String],
        depth: usize,
    ) -> Option<UserType> {
        let short = key.rsplit('.').next().unwrap_or(key).to_uppercase();
        Some(match self.types.get(key)? {
            TypeDef::Enum(labels) => UserType {
                sql_type: short,
                python_type: (!labels.is_empty()).then(|| literal_of(labels)),
                allowed_values: Some(labels.clone()),
            },
            TypeDef::Composite(_) => UserType {
                sql_type: short,
                python_type: None,
                allowed_values: None,
            },
            TypeDef::Unknown => UserType {
                sql_type: short,
                python_type: None,
                allowed_values: None,
            },
            TypeDef::Domain(base, own) => {
                let inner = type_key(base, self.dialect)
                    .filter(|_| depth < 16)
                    .and_then(|name| lookup_key(&self.types, path, &name))
                    .and_then(|inner| self.resolve_user_type(inner, path, depth + 1));
                let mut user = inner.unwrap_or_else(|| UserType {
                    sql_type: canonical_type(base, self.dialect),
                    python_type: None,
                    allowed_values: None,
                });
                user.python_type = narrowed(user.python_type, &user.sql_type, own.as_deref());
                user
            }
        })
    }
}

/// `dt` as `(sql_type, python_type)`, user types looked up by `user`.
pub(super) fn resolve_with(
    dt: &DataType,
    user: &dyn Fn(&str) -> Option<UserType>,
    dialect: SqlDialect,
) -> (String, Option<String>) {
    if let DataType::Array(
        ArrayElemTypeDef::AngleBracket(inner)
        | ArrayElemTypeDef::Parenthesis(inner)
        | ArrayElemTypeDef::Qualified(inner, _)
        | ArrayElemTypeDef::SquareBracket(inner, _),
    ) = dt
    {
        let (sql, python) = resolve_with(inner, user, dialect);
        let size = match dt {
            DataType::Array(
                ArrayElemTypeDef::SquareBracket(_, Some(size))
                | ArrayElemTypeDef::Qualified(_, Some(size)),
            ) => format!("[{size}]"),
            _ => String::new(),
        };
        let python = python.or_else(|| typed(sql.clone(), None, dialect).0);
        return (
            format!("ARRAY<{sql}>{size}"),
            python.map(|ty| format!("list[{ty}]")),
        );
    }
    if let (DataType::Enum(members, _), false) = (dt, dialect == SqlDialect::Postgres) {
        // MySQL `ENUM('a', 'b')`.
        let labels: Vec<String> = members
            .iter()
            .map(|member| match member {
                EnumMember::Name(name) | EnumMember::NamedValue(name, _) => name.clone(),
            })
            .collect();
        return (
            canonical_type(dt, dialect),
            (!labels.is_empty()).then(|| literal_of(&labels)),
        );
    }
    match type_key(dt, dialect).and_then(|name| user(&name)) {
        Some(found) => (found.sql_type, found.python_type),
        None => (canonical_type(dt, dialect), None),
    }
}

/// A column declared `ty`, resolved against `catalog`.
pub(super) fn schema_column(
    catalog: &Catalog,
    dialect: SqlDialect,
    name: &str,
    ident: &str,
    ty: &DataType,
    not_null: bool,
    checked: Checks<'_>,
) -> SchemaColumn {
    let (sql_type, python_type) = catalog.resolve_type(ty, dialect);
    let allowed_values = type_key(ty, dialect)
        .and_then(|name| catalog.user_type(&name))
        .and_then(|user| user.allowed_values.clone());
    let python_type = checked.apply(python_type, &sql_type, allowed_values.is_some());
    SchemaColumn {
        name: name.to_string(),
        ident: ident.to_string(),
        sql_type,
        python_type,
        not_null,
        allowed_values,
    }
}

pub(super) fn resolve_rel(catalog: &Catalog, dialect: SqlDialect, rel: &Rel) -> Relation {
    match rel {
        Rel::Table(table) if table.unknown => Relation::Unknown,
        Rel::Table(table) => Relation::Table(
            table
                .columns
                .iter()
                .map(|c| {
                    schema_column(
                        catalog,
                        dialect,
                        &c.name,
                        &c.ident,
                        &c.ty,
                        c.not_null,
                        c.checks(),
                    )
                })
                .collect(),
        ),
        Rel::View(columns) => Relation::View(columns.clone()),
    }
}

/// The labels of an enum as a `typing.Literal`.
pub(super) fn literal_of(labels: &[String]) -> String {
    let labels: Vec<Value> = labels.iter().cloned().map(Value::Str).collect();
    values::render(&labels).unwrap_or_default()
}
