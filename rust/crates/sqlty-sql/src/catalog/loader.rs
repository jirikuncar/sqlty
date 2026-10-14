//! The loader: schema statements as ordered operations over raw tables, views, types and
//! functions.
//!
//! [`Loader::run`] applies operations until nothing changes, so statements may appear in
//! any order. It keeps the resolved [`super::Catalog`] current between operations.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::rc::Rc;

use sqlparser::ast::{
    AlterTable, AlterType, ArgMode, ColumnOption, CreateFunction, CreateTable, CreateTableLikeKind,
    CreateView, DataType, Expr, FunctionReturnType, ObjectName, ObjectType, Query, Statement,
    TableConstraint, Visit, Visitor,
};

use crate::analyzer::{canonical_type, typed, view_columns, Column, SkippedObject, SqlDialect};
use crate::values::{self, Value};

use super::columns::{
    apply_check, column_named, is_rowid_type, mark_not_null, merge_column, raw_column, RawColumn,
};
use super::names::{fold, key_of, lookup_key, parts, schema_for_new};
use super::resolve::resolve_rel;
use super::{Catalog, BUILTIN_SCHEMAS};

#[derive(Clone, Default)]
pub(super) struct RawTable {
    pub(super) columns: Vec<RawColumn>,
    /// Tables it inherits from or is a partition of.
    pub(super) parents: Vec<String>,
    /// The shape is not known: nothing may be inferred from it.
    pub(super) unknown: bool,
}

pub(super) enum Rel {
    Table(RawTable),
    View(Vec<Column>),
}

#[derive(Clone)]
pub(super) enum TypeDef {
    /// The base type, and the values a `CHECK (VALUE IN (...))` limits it to.
    Domain(DataType, Option<Vec<Value>>),
    Enum(Vec<String>),
    Composite(Vec<(String, DataType)>),
    /// Altered in a way this module cannot follow.
    Unknown,
}

pub(super) struct RawFunction {
    pub(super) args: Vec<DataType>,
    pub(super) required: usize,
    pub(super) variadic: bool,
    pub(super) returns: RawReturn,
}

pub(super) enum RawReturn {
    Scalar(DataType),
    SetOf(DataType),
    Rows(Vec<(String, DataType)>),
}

pub(super) enum OpKind {
    Table(Box<CreateTable>, Vec<Vec<String>>),
    View(Box<CreateView>),
    Domain(ObjectName, DataType, Option<Vec<Value>>),
    Type(ObjectName, TypeDef),
    AlterTable(Box<AlterTable>),
    AlterType(AlterType),
    Drop(Box<Statement>),
    Function(Box<CreateFunction>),
    /// A statement that changes the named relation, type or function in a way this
    /// module cannot follow.
    Taint(Taint, Vec<String>),
}

#[derive(Clone, Copy)]
pub(super) enum Taint {
    Relation,
    Type,
    Function,
}

/// A statement and the `search_path` it ran under.
pub(super) struct Op {
    pub(super) path: Rc<Vec<String>>,
    pub(super) kind: OpKind,
}

/// Why an `ALTER TABLE` operation did not apply.
pub(super) enum AlterFail {
    /// A column it names does not exist (yet).
    Missing,
    /// The operation may change columns in a way this module does not follow.
    Unsupported,
}

pub(super) struct Loader {
    pub(super) dialect: SqlDialect,
    pub(super) postgres: bool,
    pub(super) relations: HashMap<String, Rel>,
    pub(super) types: HashMap<String, TypeDef>,
    pub(super) functions: HashMap<String, Vec<RawFunction>>,
    /// Functions whose latest definition could not be read.
    pub(super) unknown_functions: HashSet<String>,
    pub(super) search_path: Vec<String>,
    /// The resolved catalog; a change to one relation updates it in place, any other
    /// change drops it to be built again.
    pub(super) cache: Option<Catalog>,
    /// Old name -> current name of a renamed type: columns declared with the old name
    /// have the type under its new one.
    pub(super) type_aliases: HashMap<String, String>,
    /// Objects whose columns or types the loader could not follow.
    pub(super) skipped: Vec<SkippedObject>,
    /// Some function returns `SETOF table`, so its rows follow the table.
    pub(super) setof_functions: bool,
}

impl Loader {
    pub(super) fn new(dialect: SqlDialect) -> Self {
        Loader {
            dialect,
            postgres: dialect == SqlDialect::Postgres,
            relations: HashMap::new(),
            types: HashMap::new(),
            functions: HashMap::new(),
            unknown_functions: HashSet::new(),
            search_path: vec!["public".to_string()],
            cache: None,
            skipped: Vec::new(),
            type_aliases: HashMap::new(),
            setof_functions: false,
        }
    }

    /// The key under which a statement under `path` creates `name`.
    pub(super) fn new_key(&self, name: &ObjectName, path: &[String]) -> String {
        let parts = parts(name, self.dialect);
        match parts.as_slice() {
            [] => String::new(),
            [only] => format!("{}.{only}", self.default_schema(path)),
            _ if self.postgres => key_of(&parts, self.dialect),
            [.., last] => format!("public.{last}"),
        }
    }

    pub(super) fn default_schema<'a>(&self, path: &'a [String]) -> &'a str {
        if self.postgres {
            schema_for_new(path)
        } else {
            "public"
        }
    }

    pub(super) fn lookup<T>(
        &self,
        map: &HashMap<String, T>,
        parts: &[String],
        path: &[String],
    ) -> Option<String> {
        if parts.is_empty() {
            return None;
        }
        lookup_key(map, path, &key_of(parts, self.dialect)).map(str::to_string)
    }

    pub(super) fn table_key(&self, parts: &[String], path: &[String]) -> Option<String> {
        let key = self.lookup(&self.relations, parts, path)?;
        matches!(self.relations.get(&key), Some(Rel::Table(_))).then_some(key)
    }

    /// Apply every op, retrying the ones whose dependencies are missing until a pass
    /// changes nothing; then give up on them, views last.
    pub(super) fn run(&mut self, mut pending: Vec<Op>) {
        loop {
            let mut progress = false;
            let mut next = Vec::new();
            for op in pending {
                if self.apply(&op, false) {
                    progress = true;
                } else {
                    next.push(op);
                }
            }
            pending = next;
            if pending.is_empty() {
                return;
            }
            if progress {
                continue;
            }
            // Stuck: nothing waits on anything that will appear. Give up on tables first
            // (an unknown table lets the statements on it through), then on the rest, and
            // only then read the views against what is there. Each step takes every
            // statement of its kind at once, so a pass is never repeated per statement.
            let is_table = |op: &Op| matches!(op.kind, OpKind::Table(..));
            if pending.iter().any(is_table) {
                let (tables, rest): (Vec<Op>, Vec<Op>) = pending.into_iter().partition(is_table);
                for op in &tables {
                    self.give_up(op);
                }
                pending = rest;
            } else if pending.iter().any(|op| !matches!(op.kind, OpKind::View(_))) {
                pending.retain(|op| matches!(op.kind, OpKind::View(_)));
            } else {
                for op in &pending {
                    if !self.apply(op, false) {
                        self.apply(op, true);
                    }
                }
                return;
            }
        }
    }

    /// Apply `op`; `false` when what it needs is not defined yet. `force` applies a view
    /// whatever it reads.
    pub(super) fn apply(&mut self, op: &Op, force: bool) -> bool {
        let path = op.path.clone();
        let blocked = match &op.kind {
            OpKind::Table(create, likes) => !self.create_table(create, likes, &path),
            OpKind::View(view) => !self.create_view(view, &path, force),
            OpKind::Domain(name, base, values) => {
                self.cache = None;
                let key = self.new_key(name, &path);
                self.type_aliases.remove(&key);
                self.types
                    .insert(key, TypeDef::Domain(base.clone(), values.clone()));
                false
            }
            OpKind::Type(name, def) => {
                self.cache = None;
                let key = self.new_key(name, &path);
                self.type_aliases.remove(&key);
                self.types.insert(key, def.clone());
                false
            }
            OpKind::AlterTable(alter) => !self.alter_table(alter, &path),
            OpKind::AlterType(alter) => !self.alter_type(alter, &path),
            OpKind::Drop(stmt) => {
                self.drop_objects(stmt, &path);
                false
            }
            OpKind::Function(create) => {
                self.create_function(create, &path);
                false
            }
            OpKind::Taint(what, name) => !self.taint(*what, name, &path),
        };
        !blocked
    }

    /// Stop waiting for what `op` needs: its target keeps an unknown shape.
    pub(super) fn give_up(&mut self, op: &Op) {
        let path = op.path.clone();
        if let OpKind::Table(create, _) = &op.kind {
            // A table built from one that never appears: it exists, shape unknown.
            let key = self.new_key(&create.name, &path);
            self.skipped.push(("table", create.name.to_string()));
            self.set_relation(
                key,
                Some(Rel::Table(RawTable {
                    unknown: true,
                    ..RawTable::default()
                })),
            );
        }
    }

    // ---- CREATE TABLE ---------------------------------------------------------------

    /// Returns whether the table was created; `false` waits for a table it builds on.
    pub(super) fn create_table(
        &mut self,
        create: &CreateTable,
        likes: &[Vec<String>],
        path: &[String],
    ) -> bool {
        let key = self.new_key(&create.name, path);
        if create.if_not_exists && self.relations.contains_key(&key) {
            return true;
        }
        let mut table = RawTable::default();
        if self.postgres {
            let mut sources: Vec<Vec<String>> = create
                .inherits
                .iter()
                .flatten()
                .map(|name| parts(name, self.dialect))
                .collect();
            sources.extend(
                create
                    .partition_of
                    .as_ref()
                    .map(|name| parts(name, self.dialect)),
            );
            for source in &sources {
                let Some(parent) = self.table_key(source, path) else {
                    return false;
                };
                let Some(Rel::Table(parent_table)) = self.relations.get(&parent) else {
                    return false;
                };
                table.unknown |= parent_table.unknown;
                for column in parent_table.columns.clone() {
                    merge_column(&mut table.columns, column);
                }
                table.parents.push(parent);
            }
            let mut like_sources: Vec<Vec<String>> = likes.to_vec();
            if let Some(
                CreateTableLikeKind::Parenthesized(like) | CreateTableLikeKind::Plain(like),
            ) = &create.like
            {
                like_sources.push(parts(&like.name, self.dialect));
            }
            for source in &like_sources {
                let Some(from) = self.table_key(source, path) else {
                    return false;
                };
                let Some(Rel::Table(from_table)) = self.relations.get(&from) else {
                    return false;
                };
                table.unknown |= from_table.unknown;
                for column in from_table.columns.clone() {
                    merge_column(&mut table.columns, column);
                }
            }
        }
        for column in &create.columns {
            if column.data_type != DataType::Unspecified {
                merge_column(&mut table.columns, raw_column(column, self.dialect));
            }
        }
        // SQLite lets a PRIMARY KEY hold NULL, except in a WITHOUT ROWID or STRICT table or
        // as the sole `INTEGER` column, which is the rowid.
        let loose_keys =
            self.dialect == SqlDialect::Sqlite && !create.without_rowid && !create.strict;
        if loose_keys {
            for def in &create.columns {
                let keyed = def
                    .options
                    .iter()
                    .any(|o| matches!(o.option, ColumnOption::PrimaryKey(_)));
                let declared = def
                    .options
                    .iter()
                    .any(|o| matches!(o.option, ColumnOption::NotNull));
                let name = fold(&def.name, self.dialect);
                if let Some(column) = table.columns.iter_mut().find(|c| c.name == name) {
                    if keyed && !declared && !is_rowid_type(&def.data_type) {
                        column.not_null = false;
                    }
                }
            }
        }
        for constraint in &create.constraints {
            if let TableConstraint::Check(check) = constraint {
                apply_check(&mut table.columns, &check.expr, self.dialect);
            }
            if let TableConstraint::PrimaryKey(pk) = constraint {
                if loose_keys
                    && !(pk.columns.len() == 1
                        && column_named(&table.columns, &pk.columns[0].column.expr, self.dialect)
                            .is_some_and(|c| is_rowid_type(&c.ty)))
                {
                    continue;
                }
                for index_column in &pk.columns {
                    mark_not_null(&mut table.columns, &index_column.column.expr, self.dialect);
                }
            }
        }
        if create.query.is_none() && !table.columns.is_empty() {
            self.set_relation(key, Some(Rel::Table(table)));
        }
        true
    }

    // ---- CREATE VIEW ----------------------------------------------------------------

    pub(super) fn create_view(&mut self, view: &CreateView, path: &[String], force: bool) -> bool {
        if view.if_not_exists && self.relations.contains_key(&self.new_key(&view.name, path)) {
            return true;
        }
        let (relations_defined, undefined_calls) = self.view_sources(&view.query, path);
        if !force && !relations_defined {
            return false;
        }
        let key = self.new_key(&view.name, path);
        let dialect = self.dialect;
        let columns = view_columns(self.snapshot(), dialect, view);
        // A call to a function defined further down may be why a column is untyped.
        if !force && undefined_calls && columns.iter().any(|c| c.python_type.is_none()) {
            return false;
        }
        self.set_relation(key, Some(Rel::View(columns)));
        true
    }

    /// Whether every table and view `query` reads is defined, and whether it calls a
    /// function that is not (it may be built in).
    pub(super) fn view_sources(&self, query: &Query, path: &[String]) -> (bool, bool) {
        struct Sources {
            relations: Vec<ObjectName>,
            calls: Vec<ObjectName>,
            ctes: HashSet<String>,
            dialect: SqlDialect,
        }
        impl Visitor for Sources {
            type Break = ();
            fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
                if let Some(with) = &query.with {
                    for cte in &with.cte_tables {
                        self.ctes.insert(fold(&cte.alias.name, self.dialect));
                    }
                }
                ControlFlow::Continue(())
            }
            fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
                self.relations.push(relation.clone());
                ControlFlow::Continue(())
            }
            fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
                if let Expr::Function(func) = expr {
                    self.calls.push(func.name.clone());
                }
                ControlFlow::Continue(())
            }
        }
        let mut sources = Sources {
            relations: Vec::new(),
            calls: Vec::new(),
            ctes: HashSet::new(),
            dialect: self.dialect,
        };
        let _ = query.visit(&mut sources);
        let relations_defined = sources.relations.iter().all(|name| {
            let parts = parts(name, self.dialect);
            (parts.len() == 1 && sources.ctes.contains(&parts[0]))
                || self.lookup(&self.relations, &parts, path).is_some()
                || self.lookup(&self.functions, &parts, path).is_some()
        });
        let undefined_calls = sources.calls.iter().any(|name| {
            self.lookup(&self.functions, &parts(name, self.dialect), path)
                .is_none()
        });
        (relations_defined, undefined_calls)
    }

    /// Widen the `Literal` columns that views took from text and integer columns to
    /// `str` / `int`: a dropped `CHECK` may have bounded them. Enum labels and constants
    /// stay.
    pub(super) fn forget_view_values(&mut self) {
        self.cache = None;
        let dialect = self.dialect;
        for rel in self.relations.values_mut() {
            let Rel::View(columns) = rel else { continue };
            for column in columns {
                let (Some(python), Some(sql)) = (&column.python_type, &column.sql_type) else {
                    continue;
                };
                let name = sql.split(['(', '<']).next().unwrap_or_default().trim();
                let checkable = matches!(
                    name,
                    "TEXT" | "VARCHAR" | "SMALLINT" | "INT" | "BIGINT" | "TINYINT" | "MEDIUMINT"
                );
                if checkable && values::parse(python).is_some() {
                    column.python_type = typed(sql.clone(), None, dialect).0;
                }
            }
        }
    }

    /// Set (or, with `None`, remove) one relation, keeping the resolved catalog current
    /// without building it again.
    pub(super) fn set_relation(&mut self, key: String, rel: Option<Rel>) {
        let dialect = self.dialect;
        let resolved = match &self.cache {
            Some(cache) if !self.setof_functions => {
                Some(rel.as_ref().map(|rel| resolve_rel(cache, dialect, rel)))
            }
            _ => None,
        };
        match rel {
            Some(rel) => self.relations.insert(key.clone(), rel),
            None => self.relations.remove(&key),
        };
        let Some(resolved) = resolved else {
            self.cache = None;
            return;
        };
        let Some(cache) = self.cache.as_mut() else {
            return;
        };
        let user = !BUILTIN_SCHEMAS
            .iter()
            .any(|schema| key.starts_with(&format!("{schema}.")));
        match resolved {
            Some(relation) => {
                if cache.relations.insert(key, relation).is_none() && user {
                    cache.user_relations += 1;
                }
            }
            None => {
                if cache.relations.remove(&key).is_some() && user {
                    cache.user_relations -= 1;
                }
            }
        }
    }

    /// Tables that inherit from or are partitions of `key`, nested ones included.
    pub(super) fn descendants(&self, key: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let mut queue = vec![key.to_string()];
        while let Some(parent) = queue.pop() {
            for (name, rel) in &self.relations {
                if let Rel::Table(table) = rel {
                    if table.parents.contains(&parent) && !found.contains(name) {
                        found.push(name.clone());
                        queue.push(name.clone());
                    }
                }
            }
        }
        found
    }

    pub(super) fn taint(&mut self, what: Taint, name: &[String], path: &[String]) -> bool {
        match what {
            Taint::Relation => match self.lookup(&self.relations, name, path) {
                Some(key) => {
                    let parents = match self.relations.get(&key) {
                        Some(Rel::Table(table)) => table.parents.clone(),
                        _ => Vec::new(),
                    };
                    self.skipped.push(("table", name.join(".")));
                    for name in std::iter::once(key.clone()).chain(self.descendants(&key)) {
                        self.set_relation(
                            name,
                            Some(Rel::Table(RawTable {
                                unknown: true,
                                parents: parents.clone(),
                                ..RawTable::default()
                            })),
                        );
                    }
                    true
                }
                None => false,
            },
            Taint::Type => match self.lookup(&self.types, name, path) {
                Some(key) => {
                    self.cache = None;
                    self.skipped.push(("type", name.join(".")));
                    self.types.insert(key, TypeDef::Unknown);
                    true
                }
                None => false,
            },
            Taint::Function => {
                let key = self.lookup(&self.functions, name, path).or_else(|| {
                    name.last()
                        .map(|last| format!("{}.{last}", self.default_schema(path)))
                });
                if let Some(key) = key {
                    self.skipped.push(("function", name.join(".")));
                    self.cache = None;
                    self.functions.entry(key.clone()).or_default();
                    self.unknown_functions.insert(key);
                }
                true
            }
        }
    }

    // ---- DROP -----------------------------------------------------------------------

    pub(super) fn drop_objects(&mut self, stmt: &Statement, path: &[String]) {
        if !matches!(
            stmt,
            Statement::Drop {
                object_type: ObjectType::Table | ObjectType::View | ObjectType::MaterializedView,
                ..
            }
        ) {
            self.cache = None;
        }
        match stmt {
            Statement::Drop {
                object_type, names, ..
            } => {
                for name in names {
                    let name_parts = parts(name, self.dialect);
                    match object_type {
                        ObjectType::Table | ObjectType::View | ObjectType::MaterializedView => {
                            if let Some(key) = self.lookup(&self.relations, &name_parts, path) {
                                self.set_relation(key, None);
                            }
                        }
                        ObjectType::Type => {
                            if let Some(key) = self.lookup(&self.types, &name_parts, path) {
                                self.types.remove(&key);
                            }
                        }
                        ObjectType::Schema => {
                            if let Some(schema) = name_parts.last() {
                                let prefix = format!("{schema}.");
                                self.relations.retain(|key, _| !key.starts_with(&prefix));
                                self.types.retain(|key, _| !key.starts_with(&prefix));
                                self.functions.retain(|key, _| !key.starts_with(&prefix));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Statement::DropDomain(drop) => {
                if let Some(key) = self.lookup(&self.types, &parts(&drop.name, self.dialect), path)
                {
                    self.types.remove(&key);
                }
            }
            Statement::DropFunction(drop) => {
                for desc in &drop.func_desc {
                    let Some(key) =
                        self.lookup(&self.functions, &parts(&desc.name, self.dialect), path)
                    else {
                        continue;
                    };
                    let wanted: Option<Vec<String>> = desc.args.as_ref().map(|args| {
                        args.iter()
                            .filter(|a| !matches!(a.mode, Some(ArgMode::Out)))
                            .map(|a| canonical_type(&a.data_type, self.dialect))
                            .collect()
                    });
                    let dialect = self.dialect;
                    if let Some(overloads) = self.functions.get_mut(&key) {
                        overloads.retain(|f| match &wanted {
                            Some(wanted) => {
                                f.args
                                    .iter()
                                    .map(|dt| canonical_type(dt, dialect))
                                    .collect::<Vec<_>>()
                                    != *wanted
                            }
                            None => false,
                        });
                        if overloads.is_empty() {
                            self.functions.remove(&key);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ---- CREATE FUNCTION ------------------------------------------------------------

    pub(super) fn create_function(&mut self, create: &CreateFunction, path: &[String]) {
        let key = self.new_key(&create.name, path);
        let args = create.args.as_deref().unwrap_or(&[]);
        let inputs: Vec<_> = args
            .iter()
            .filter(|arg| !matches!(arg.mode, Some(ArgMode::Out)))
            .collect();
        let outs: Vec<(String, DataType)> = args
            .iter()
            .filter(|arg| matches!(arg.mode, Some(ArgMode::Out | ArgMode::InOut)))
            .enumerate()
            .map(|(i, arg)| {
                (
                    arg.name
                        .as_ref()
                        .map_or(format!("column{}", i + 1), |n| fold(n, self.dialect)),
                    arg.data_type.clone(),
                )
            })
            .collect();
        let returns = match &create.return_type {
            Some(FunctionReturnType::DataType(DataType::Table(Some(columns)))) => RawReturn::Rows(
                columns
                    .iter()
                    .map(|c| (fold(&c.name, self.dialect), c.data_type.clone()))
                    .collect(),
            ),
            Some(FunctionReturnType::DataType(dt)) if outs.len() < 2 => {
                RawReturn::Scalar(dt.clone())
            }
            Some(FunctionReturnType::SetOf(dt)) => {
                self.setof_functions = true;
                RawReturn::SetOf(dt.clone())
            }
            _ if outs.len() >= 2 => RawReturn::Rows(outs),
            _ if outs.len() == 1 => RawReturn::Scalar(outs[0].1.clone()),
            _ => {
                self.functions.entry(key.clone()).or_default();
                self.unknown_functions.insert(key);
                return;
            }
        };
        let function = RawFunction {
            args: inputs.iter().map(|a| a.data_type.clone()).collect(),
            required: inputs
                .iter()
                .filter(|a| a.default_expr.is_none() && !matches!(a.mode, Some(ArgMode::Variadic)))
                .count(),
            variadic: inputs
                .iter()
                .any(|a| matches!(a.mode, Some(ArgMode::Variadic))),
            returns,
        };
        self.unknown_functions.remove(&key);
        let dialect = self.dialect;
        let signature = |f: &RawFunction| -> Vec<String> {
            f.args
                .iter()
                .map(|dt| canonical_type(dt, dialect))
                .collect()
        };
        let overloads = self.functions.entry(key).or_default();
        let wanted = signature(&function);
        overloads.retain(|f| signature(f) != wanted);
        overloads.push(function);
        self.cache = None;
    }
}
