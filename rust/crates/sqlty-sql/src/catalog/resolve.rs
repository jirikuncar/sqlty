//! Resolution: raw tables, views and types into the [`Catalog`] the analyzer reads.
//!
//! `Loader::build` resolves everything; `schema_column` and `resolve_rel` resolve
//! one relation when the loader updates the cached catalog in place.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use sqlparser::ast::{ArrayElemTypeDef, DataType, EnumMember};

use crate::analyzer::{canonical_type, typed, SqlDialect};
use crate::ddl::RowEvents;
use crate::values::{self, Value};

use super::columns::{narrowed, Checks};
use super::loader::{Loader, RawReturn, Rel, TypeDef};
use super::names::{lookup_key, type_key};
use super::{
    Catalog, Function, FunctionReturn, Relation, SchemaColumn, TypeOverrides, UserType,
    BUILTIN_SCHEMAS,
};

#[cfg(test)]
thread_local! {
    /// How many times `Loader::build` ran on this thread; read by the timing test.
    pub(super) static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Loader {
    /// The catalog as it stands now.
    pub(super) fn snapshot(&mut self) -> &Catalog {
        if self.functions_dirty {
            self.rebuild_functions();
        }
        let catalog = self.cache.take().unwrap_or_else(|| self.build());
        self.cache.insert(catalog)
    }

    pub(super) fn build(&self) -> Catalog {
        #[cfg(test)]
        BUILDS.with(|count| count.set(count.get() + 1));
        let dialect = self.dialect;
        let path = self.search_path.clone();
        let mut user_types: FxHashMap<String, UserType> = FxHashMap::default();
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
        let resolver = Resolver {
            types: &user_types,
            path: &path,
            dialect,
            overrides: &self.overrides,
        };
        let mut relations = FxHashMap::default();
        let mut user_relations = 0;
        for (key, rel) in &self.relations {
            if !BUILTIN_SCHEMAS
                .iter()
                .any(|s| key.starts_with(&format!("{s}.")))
            {
                user_relations += 1;
            }
            relations.insert(key.clone(), resolver.relation(key, rel));
        }
        let mut composites: FxHashMap<String, Vec<SchemaColumn>> = self
            .types
            .iter()
            .filter_map(|(key, def)| match def {
                TypeDef::Composite(fields) => Some((
                    key.clone(),
                    fields
                        .iter()
                        .map(|(name, ty)| {
                            resolver.column(name, name, ty, false, false, Checks::default())
                        })
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
        let (functions, unknown_functions) = self.build_functions(&resolver, &relations);
        Catalog {
            relations,
            types: user_types,
            composites,
            functions,
            unknown_functions,
            type_names: self
                .types
                .keys()
                .chain(self.type_aliases.keys())
                .map(|key| (key.clone(), ()))
                .collect(),
            search_path: path,
            user_relations,
            postgres: self.postgres,
            overrides: Arc::clone(&self.overrides),
        }
    }

    /// The function part of the catalog: overloads of known shape, and the keys of those
    /// of unknown shape. It reads user types (through `resolver`) and `SETOF table`
    /// functions read `relations`, so a change to either may need it again.
    pub(super) fn build_functions(
        &self,
        resolver: &Resolver<'_>,
        relations: &FxHashMap<String, Relation>,
    ) -> (FxHashMap<String, Vec<Function>>, FxHashMap<String, ()>) {
        let (path, dialect) = (resolver.path, resolver.dialect);
        let mut functions = FxHashMap::default();
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
                            .map(|(name, ty)| {
                                resolver.column(name, name, ty, false, false, Checks::default())
                            })
                            .collect(),
                    ),
                    RawReturn::Scalar(ty) | RawReturn::SetOf(ty) => {
                        let set_of = matches!(f.returns, RawReturn::SetOf(_));
                        let rows = type_key(ty, dialect)
                            .and_then(|name| lookup_key(relations, path, &name).map(str::to_string))
                            .and_then(|key| match relations.get(&key) {
                                Some(Relation::Table(columns, ..)) => Some(columns.clone()),
                                _ => None,
                            });
                        match rows {
                            Some(columns) if set_of => FunctionReturn::Rows(columns),
                            Some(columns) => {
                                let Resolved {
                                    sql_type: sql,
                                    python_type: python,
                                    ..
                                } = resolver.resolve(ty);
                                let columns = columns
                                    .into_iter()
                                    .map(|column| SchemaColumn {
                                        not_null: false,
                                        ..column
                                    })
                                    .collect();
                                FunctionReturn::Row(typed(sql, python, dialect), columns)
                            }
                            _ => {
                                let Resolved {
                                    sql_type: sql,
                                    python_type: python,
                                    ..
                                } = resolver.resolve(ty);
                                let value = if sql == "VOID" {
                                    (Some("None".to_string()), Some(sql))
                                } else {
                                    typed(sql, python, dialect)
                                };
                                if set_of {
                                    FunctionReturn::SetOf(value)
                                } else {
                                    FunctionReturn::Scalar(value)
                                }
                            }
                        }
                    }
                };
                let fixed = f.args.len() - usize::from(f.variadic);
                built.push(Function {
                    required: f.required,
                    max: if f.variadic { usize::MAX } else { f.args.len() },
                    args: f.args[..fixed]
                        .iter()
                        .map(|ty| {
                            let resolved = resolver.resolve(ty);
                            typed(resolved.sql_type, resolved.python_type, dialect)
                        })
                        .collect(),
                    returns,
                });
            }
            functions.insert(key.clone(), built);
        }
        let unknown = self
            .unknown_functions
            .iter()
            .map(|key| (key.clone(), ()))
            .collect();
        (functions, unknown)
    }

    /// Mark the functions of the cached catalog stale; the next `snapshot` resolves them
    /// again once, however many were changed in between.
    pub(super) fn refresh_functions(&mut self) {
        self.functions_dirty = self.cache.is_some();
    }

    /// Resolve the functions of the cached catalog again, leaving its relations and
    /// types as they are.
    fn rebuild_functions(&mut self) {
        self.functions_dirty = false;
        let Some(mut cache) = self.cache.take() else {
            return;
        };
        let (functions, unknown) =
            self.build_functions(&cache.resolver(self.dialect), &cache.relations);
        cache.functions = functions;
        cache.unknown_functions = unknown;
        self.cache = Some(cache);
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
                allowed_values: Some(labels.as_slice().into()),
                values: (!labels.is_empty())
                    .then(|| labels.iter().cloned().map(Value::Str).collect()),
            },
            TypeDef::Composite(_) => UserType {
                sql_type: short,
                python_type: None,
                allowed_values: None,
                values: None,
            },
            TypeDef::Unknown => UserType {
                sql_type: short,
                python_type: None,
                allowed_values: None,
                values: None,
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
                    values: None,
                });
                user.python_type = narrowed(user.python_type, &user.sql_type, own.as_deref());
                user.values = literal_values(user.python_type.as_deref());
                user
            }
        })
    }
}

/// Resolves declared column types against the user types of one catalog.
///
/// `build` and the in-place update of a cached catalog share it, so a column resolves
/// the same either way.
pub(super) struct Resolver<'a> {
    pub(super) types: &'a FxHashMap<String, UserType>,
    pub(super) path: &'a [String],
    pub(super) dialect: SqlDialect,
    pub(super) overrides: &'a TypeOverrides,
}

/// A declared type as columns show it.
pub(super) struct Resolved<'a> {
    pub(super) sql_type: String,
    pub(super) python_type: Option<String>,
    /// The labels of the enum the type is.
    pub(super) allowed_values: Option<&'a Arc<[String]>>,
    /// The values of a `Literal` `python_type`.
    pub(super) values: Option<Arc<[Value]>>,
}

impl<'a> Resolver<'a> {
    /// The user type a lookup name (such as `app.percent`) finds along the search path.
    pub(super) fn user_type(&self, name: &str) -> Option<&'a UserType> {
        let types = self.types;
        lookup_key(types, self.path, name).and_then(|key| types.get(key))
    }

    /// `dt` as SQL and Python types; user types are looked up once.
    /// The type `dt` declares, with the Python type the project sets for it, if any.
    pub(super) fn resolve(&self, dt: &DataType) -> Resolved<'a> {
        let mut resolved = self.resolve_declared(dt);
        if let Some(python) = self.overrides.for_type(&resolved.sql_type) {
            resolved.python_type = Some(python.to_string());
            resolved.allowed_values = None;
            resolved.values = None;
        }
        resolved
    }

    fn resolve_declared(&self, dt: &DataType) -> Resolved<'a> {
        let dialect = self.dialect;
        if let DataType::Array(
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::Parenthesis(inner)
            | ArrayElemTypeDef::Qualified(inner, _)
            | ArrayElemTypeDef::SquareBracket(inner, _),
        ) = dt
        {
            let Resolved {
                sql_type: sql,
                python_type: python,
                ..
            } = self.resolve(inner);
            let size = match dt {
                DataType::Array(
                    ArrayElemTypeDef::SquareBracket(_, Some(size))
                    | ArrayElemTypeDef::Qualified(_, Some(size)),
                ) => format!("[{size}]"),
                _ => String::new(),
            };
            let python = python.or_else(|| typed(sql.clone(), None, dialect).0);
            // DuckDB's fixed-size `ARRAY` comes back as a tuple (see `SqlType::python`).
            let fixed = !size.is_empty() && dialect == SqlDialect::Other("duckdb");
            return Resolved {
                sql_type: format!("ARRAY<{sql}>{size}"),
                python_type: python.map(|ty| {
                    if fixed {
                        format!("tuple[{ty}, ...]")
                    } else {
                        format!("list[{ty}]")
                    }
                }),
                allowed_values: None,
                values: None,
            };
        }
        if let (DataType::Enum(members, _), false) = (dt, dialect == SqlDialect::Postgres) {
            // MySQL `ENUM('a', 'b')`.
            let labels: Vec<String> = members
                .iter()
                .map(|member| match member {
                    EnumMember::Name(name) | EnumMember::NamedValue(name, _) => name.clone(),
                })
                .collect();
            return Resolved {
                sql_type: canonical_type(dt, dialect),
                python_type: (!labels.is_empty()).then(|| literal_of(&labels)),
                allowed_values: None,
                values: (!labels.is_empty()).then(|| labels.into_iter().map(Value::Str).collect()),
            };
        }
        match type_key(dt, dialect).and_then(|name| self.user_type(&name)) {
            Some(found) => Resolved {
                sql_type: found.sql_type.clone(),
                python_type: found.python_type.clone(),
                allowed_values: found.allowed_values.as_ref(),
                values: found.values.clone(),
            },
            None => Resolved {
                sql_type: canonical_type(dt, dialect),
                python_type: None,
                allowed_values: None,
                values: None,
            },
        }
    }

    /// A column declared `ty`.
    pub(super) fn column(
        &self,
        name: &str,
        ident: &str,
        ty: &DataType,
        not_null: bool,
        has_default: bool,
        checked: Checks<'_>,
    ) -> SchemaColumn {
        let resolved = self.resolve(ty);
        let python_type = checked.apply(
            resolved.python_type,
            &resolved.sql_type,
            resolved.allowed_values.is_some(),
        );
        SchemaColumn {
            name: name.to_string(),
            ident: ident.to_string(),
            sql_type: resolved.sql_type,
            values: literal_values(python_type.as_deref()),
            python_type,
            not_null,
            has_default,
            primary_key: false,
            allowed_values: resolved.allowed_values.cloned(),
        }
    }

    /// A raw relation as the catalog holds it.
    pub(super) fn relation(&self, key: &str, rel: &Rel) -> Relation {
        match rel {
            Rel::Table(table) if table.unknown => Relation::Unknown,
            Rel::Table(table) => Relation::Table(
                table
                    .columns
                    .iter()
                    .map(|c| {
                        let mut column = SchemaColumn {
                            primary_key: c.primary_key,
                            ..self.column(
                                &c.name,
                                &c.ident,
                                &c.ty,
                                c.not_null,
                                c.has_default,
                                c.checks(),
                            )
                        };
                        if let Some(python) = self.overrides.for_column(key, &c.name) {
                            column.python_type = Some(python.to_string());
                            column.values = None;
                            column.allowed_values = None;
                        }
                        column
                    })
                    .collect(),
                // A group naming a dropped or renamed column no longer holds.
                table
                    .nonnull_groups
                    .iter()
                    .filter(|group| {
                        group
                            .iter()
                            .all(|name| table.columns.iter().any(|c| c.name == *name))
                    })
                    .cloned()
                    .collect(),
                table
                    .row_triggers
                    .iter()
                    .fold(RowEvents::default(), |all, (_, events)| RowEvents {
                        insert: all.insert || events.insert,
                        update: all.update || events.update,
                    }),
            ),
            Rel::View(columns) => Relation::View(columns.clone()),
        }
    }
}

/// The values of a `Literal` Python type, read once when the catalog is built so the
/// analyzer gets them as data.
pub(super) fn literal_values(python: Option<&str>) -> Option<Arc<[Value]>> {
    python.and_then(values::parse).map(Arc::from)
}

/// The labels of an enum as a `typing.Literal`.
pub(super) fn literal_of(labels: &[String]) -> String {
    let labels: Vec<Value> = labels.iter().cloned().map(Value::Str).collect();
    values::render(&labels).unwrap_or_default()
}
