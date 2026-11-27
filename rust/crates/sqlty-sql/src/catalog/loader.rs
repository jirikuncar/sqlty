//! The loader: schema statements as ordered operations over raw tables, views, types and
//! functions.
//!
//! [`Loader::run`] applies operations until nothing changes, so statements may appear in
//! any order. It keeps the resolved [`super::Catalog`] that views read, updating only the
//! relations that changed.

use rustc_hash::{FxHashMap, FxHashSet};
use std::ops::{ControlFlow, Range};
use std::rc::Rc;
use std::sync::Arc;

use sqlparser::ast::{
    AlterTable, AlterType, ArgMode, ColumnOption, CreateFunction, CreateTable, CreateTableLikeKind,
    CreateTableOptions, CreateView, DataType, Expr, ForeignKeyConstraint, FunctionCalledOnNull,
    FunctionReturnType, Ident, ObjectName, ObjectType, Query, SqlOption, Statement,
    TableConstraint, Visit, Visitor,
};

use crate::analyzer::{
    canonical_type, typed, view_columns, Column, Nullability, SkippedObject, SqlDialect,
};
use crate::ddl::{Like, RowEvents, RowTrigger};
use crate::values::{self, Value};

use super::columns::{
    apply_check, column_named, discriminating, is_rowid_type, mark_loose_primary_key,
    mark_primary_key, merge_columns, nonnull_groups, raw_column, unique_key, RawColumn,
};
use super::names::{fold, key_of, lookup_key, parts, schema_for_new, type_key};
use super::policies::{self, Policy};
use super::{Catalog, ForeignKey, TypeOverrides};
use crate::MAX_COLUMNS;

#[derive(Clone, Default, PartialEq)]
pub(super) struct RawTable {
    pub(super) columns: Vec<RawColumn>,
    /// Tables it inherits from or is a partition of.
    pub(super) parents: Vec<String>,
    /// The shape is not known: nothing may be inferred from it.
    pub(super) unknown: bool,
    /// Groups of columns a `CHECK` keeps at least one of non-null.
    pub(super) nonnull_groups: Vec<Vec<String>>,
    /// The conditions of its `CHECK` constraints.
    pub(super) checks: Vec<Expr>,
    /// `BEFORE` row triggers by name, and the statements they fire for.
    pub(super) row_triggers: Vec<(String, RowEvents)>,
    pub(super) foreign_keys: Vec<RawForeignKey>,
    /// `UNIQUE` constraints and unique indexes over plain columns.
    pub(super) unique: Vec<RawUnique>,
    /// `ENABLE ROW LEVEL SECURITY`: a query may not see every row.
    pub(super) row_security: bool,
    /// `FORCE ROW LEVEL SECURITY`: the policies hold for the table's owner too.
    pub(super) force_row_security: bool,
    /// The bytes of the latest statement that enabled, disabled or forced row-level security.
    pub(super) row_security_at: Range<usize>,
    pub(super) policies: Vec<Policy>,
}

/// Columns no two rows share a value of (NULLs aside), and the name of the constraint or
/// index that says so.
#[derive(Clone, PartialEq)]
pub(super) struct RawUnique {
    pub(super) name: Option<String>,
    pub(super) columns: Vec<String>,
}

/// A foreign key as declared, which holds for every row only when `enforced`.
#[derive(Clone, PartialEq)]
pub(super) struct RawForeignKey {
    pub(super) name: Option<String>,
    pub(super) key: ForeignKey,
    /// Neither `DEFERRABLE` nor `NOT ENFORCED`.
    pub(super) immediate: bool,
    /// Not added `NOT VALID`, or validated since.
    pub(super) validated: bool,
    /// The name and search path of a referenced table not defined yet, which
    /// [`Loader::resolve_foreign_keys`] looks up once every table is.
    pub(super) waiting: Option<Box<(Vec<String>, Vec<String>)>>,
}

#[derive(Clone)]
pub(super) enum Rel {
    Table(RawTable),
    View(Vec<Column>),
}

impl Rel {
    fn width(&self) -> usize {
        match self {
            Rel::Table(table) => table.columns.len(),
            Rel::View(columns) => columns.len(),
        }
    }

    pub(super) fn table(&self) -> Option<&RawTable> {
        match self {
            Rel::Table(table) => Some(table),
            Rel::View(_) => None,
        }
    }
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

#[derive(Clone)]
pub(super) struct RawFunction {
    pub(super) args: Vec<DataType>,
    pub(super) required: usize,
    pub(super) variadic: bool,
    pub(super) returns: RawReturn,
    pub(super) strict: bool,
}

#[derive(Clone)]
pub(super) enum RawReturn {
    Scalar(DataType),
    SetOf(DataType),
    Rows(Vec<(String, DataType)>),
}

pub(super) enum OpKind {
    /// With the `LIKE` sources, each by the placeholder column that stands for it.
    Table(Box<CreateTable>, Vec<Like>),
    /// A view, or (`true`) a `CREATE TABLE ... AS` table, whose columns are a query's.
    View(Box<CreateView>, bool),
    Domain(ObjectName, DataType, Option<Vec<Value>>),
    Type(ObjectName, TypeDef),
    AlterTable(Box<AlterTable>),
    AlterType(AlterType),
    Drop(Box<Statement>),
    Function(Box<CreateFunction>),
    /// A `BEFORE` row trigger, which may change any column of the rows it fires for.
    RowTrigger(RowTrigger),
    /// `CREATE UNIQUE INDEX` over plain columns, without a `WHERE`.
    UniqueIndex(Vec<String>, RawUnique),
    /// `DROP TRIGGER name [ON table]`.
    DropTrigger(Vec<String>, Option<Vec<String>>),
    /// `CREATE`, `ALTER` or `DROP POLICY` on the table it names.
    Policy(Vec<String>, Box<policies::Change>),
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
    /// Bytes of the statement, which applying it takes time in proportion to.
    pub(super) size: usize,
    /// Where the statement is in the schema.
    pub(super) at: Range<usize>,
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
    pub(super) relations: FxHashMap<String, Rel>,
    pub(super) types: FxHashMap<String, TypeDef>,
    /// The labels of enums past [`LARGE_ENUM`] labels, so `ADD VALUE` finds a duplicate
    /// without a scan. Every writer of a type forgets its entry.
    pub(super) enum_labels: FxHashMap<String, FxHashSet<String>>,
    pub(super) functions: FxHashMap<String, Vec<RawFunction>>,
    /// Functions whose latest definition could not be read.
    pub(super) unknown_functions: FxHashSet<String>,
    pub(super) search_path: Vec<String>,
    /// The resolved catalog; a change to one relation marks it stale, any other change
    /// drops it to be built again.
    pub(super) cache: Option<Catalog>,
    /// Functions of the cached catalog that are stale; `snapshot` resolves them again.
    pub(super) stale_functions: FxHashSet<String>,
    /// Relations of the cached catalog that are stale; `snapshot` resolves them again, so a
    /// run of statements on one table resolves it once.
    pub(super) stale_relations: FxHashSet<String>,
    /// Old name -> current name of a renamed type: columns declared with the old name
    /// have the type under its new one.
    pub(super) type_aliases: FxHashMap<String, String>,
    /// Objects whose columns or types the loader could not follow.
    pub(super) skipped: Vec<SkippedObject>,
    /// The Python types the project sets for SQL types and columns.
    pub(super) overrides: Arc<TypeOverrides>,
    /// Type name -> the functions that return it: a function returning a table has its rows
    /// follow the table, so a change to a relation of that name refreshes them.
    pub(super) returned_by: FxHashMap<String, FxHashSet<String>>,
    /// Tables some table inherits from or is a partition of; no other has descendants.
    pub(super) inherited: FxHashSet<String>,
    /// Columns of all relations, at most [`MAX_SCHEMA_COLUMNS`] plus `allowance`.
    held: usize,
    /// The columns the schema could spell out, which no limit counts.
    allowance: usize,
    /// Columns of the relations set so far, each time one is set.
    written: usize,
    /// Where the statement being applied is in the schema.
    pub(super) at: Range<usize>,
}

/// The most columns all relations hold besides the ones a schema could spell out (see
/// [`Loader::allow`]): about 1 GB. A table built `LIKE` a wide one copies its columns, so
/// a short schema could otherwise fill memory.
pub(super) const MAX_SCHEMA_COLUMNS: usize = 1 << 21;

/// Enums with more labels than this are indexed for `ADD VALUE` (see [`Loader::enum_labels`]).
pub(super) const LARGE_ENUM: usize = 64;

/// The most bytes of statements [`Loader::run`] tries again, in all passes, besides the
/// schema's own length.
pub(super) const MAX_RETRIED: usize = 1 << 24;

/// The most columns the loader writes, counting a table's width for each `ALTER` or index
/// on it: the statements that copy the table could otherwise take a short schema minutes.
/// Real ones write a few hundred thousand.
pub(super) const MAX_WRITTEN: usize = 1 << 22;

impl Loader {
    pub(super) fn new(dialect: SqlDialect) -> Self {
        Loader {
            dialect,
            postgres: dialect == SqlDialect::Postgres,
            relations: FxHashMap::default(),
            types: FxHashMap::default(),
            enum_labels: FxHashMap::default(),
            functions: FxHashMap::default(),
            unknown_functions: FxHashSet::default(),
            search_path: vec!["public".to_string()],
            cache: None,
            stale_functions: FxHashSet::default(),
            stale_relations: FxHashSet::default(),
            skipped: Vec::new(),
            type_aliases: FxHashMap::default(),
            returned_by: FxHashMap::default(),
            overrides: Arc::default(),
            inherited: FxHashSet::default(),
            held: 0,
            allowance: 0,
            written: 0,
            at: 0..0,
        }
    }

    /// A copy to read more statements into, without the resolved catalog.
    pub(super) fn fork(&self) -> Self {
        Loader {
            dialect: self.dialect,
            postgres: self.postgres,
            relations: self.relations.clone(),
            types: self.types.clone(),
            enum_labels: FxHashMap::default(),
            functions: self.functions.clone(),
            unknown_functions: self.unknown_functions.clone(),
            search_path: self.search_path.clone(),
            cache: None,
            stale_functions: self.stale_functions.clone(),
            stale_relations: FxHashSet::default(),
            type_aliases: self.type_aliases.clone(),
            skipped: self.skipped.clone(),
            overrides: Arc::clone(&self.overrides),
            returned_by: self.returned_by.clone(),
            inherited: self.inherited.clone(),
            held: self.held,
            allowance: self.allowance,
            written: 0,
            at: 0..0,
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
        map: &FxHashMap<String, T>,
        parts: &[String],
        path: &[String],
    ) -> Option<String> {
        if parts.is_empty() {
            return None;
        }
        lookup_key(map, path, &key_of(parts, self.dialect)).map(str::to_string)
    }

    pub(super) fn table_key(&self, parts: &[String], path: &[String]) -> Option<String> {
        self.table_of(parts, path).map(|(key, _)| key)
    }

    /// The table `parts` names, and its key.
    pub(super) fn table_of(
        &self,
        parts: &[String],
        path: &[String],
    ) -> Option<(String, &RawTable)> {
        let key = self.lookup(&self.relations, parts, path)?;
        let table = self.relations.get(&key)?.table()?;
        Some((key, table))
    }

    /// Change the table at `key` where it is, as `set_relation` with a changed copy would:
    /// a statement per index, trigger or column would otherwise copy the table each time.
    pub(super) fn change_table(&mut self, key: String, change: impl FnOnce(&mut RawTable)) {
        if let Some(Rel::Table(table)) = self.relations.get_mut(&key) {
            let mut table = std::mem::take(table);
            self.held = self.held.saturating_sub(table.columns.len());
            change(&mut table);
            self.set_relation(key, Some(Rel::Table(table)));
        }
    }

    /// Apply every op, retrying the ones whose dependencies are missing until a pass
    /// changes nothing; then give up on them, views last.
    pub(super) fn run(&mut self, pending: Vec<Op>) {
        let last = self.search_path.clone();
        self.run_pending(pending);
        if self.search_path != last {
            self.set_search_path(last);
        }
    }

    fn run_pending(&mut self, mut pending: Vec<Op>) {
        let mut retried = 0;
        loop {
            if self.exhausted() {
                if !pending.is_empty() {
                    let waiting = format!("{} statements waiting on others", pending.len());
                    self.skipped.push(("limit", waiting));
                }
                return;
            }
            let mut progress = false;
            let mut next = Vec::new();
            retried += pending.iter().map(|op| op.size).sum::<usize>();
            for op in pending {
                if !self.exhausted() && self.apply(&op, false) {
                    progress = true;
                } else {
                    next.push(op);
                }
            }
            pending = next;
            if pending.is_empty() {
                return;
            }
            // A pass may take one statement of a chain written backwards: past
            // `MAX_RETRIED`, the rest count as stuck rather than take quadratic time.
            if progress && retried <= MAX_RETRIED + 2 * self.allowance {
                continue;
            }
            // Stuck: nothing waits on anything that will appear. Give up on tables first
            // (an unknown table lets the statements on it through), then on the rest, and
            // only then read the views against what is there. Each step takes every
            // statement of its kind at once, so a pass is never repeated per statement.
            let is_table = |op: &Op| matches!(op.kind, OpKind::Table(..));
            if pending.iter().any(is_table) {
                pending.retain(|op| match &op.kind {
                    OpKind::Table(create, _) => {
                        self.give_up(create, &op.path);
                        false
                    }
                    _ => true,
                });
            } else if pending
                .iter()
                .any(|op| !matches!(op.kind, OpKind::View(..)))
            {
                pending.retain(|op| matches!(op.kind, OpKind::View(..)));
            } else {
                pending.retain(|op| {
                    if self.exhausted() {
                        return true;
                    }
                    if !self.apply(op, false) {
                        self.apply(op, true);
                    }
                    false
                });
                if pending.is_empty() {
                    return;
                }
            }
        }
    }

    /// Apply `op`; `false` when what it needs is not defined yet. `force` applies a view
    /// whatever it reads.
    pub(super) fn apply(&mut self, op: &Op, force: bool) -> bool {
        // A statement waiting on a later one reads names under its own `search_path`;
        // [`Loader::run`] restores the last one.
        if *op.path != self.search_path {
            self.set_search_path(op.path.to_vec());
        }
        let path = op.path.clone();
        self.at = op.at.clone();
        let blocked = match &op.kind {
            OpKind::Table(create, likes) => !self.create_table(create, likes, &path),
            OpKind::View(view, table) => !self.create_view(view, *table, &path, force),
            OpKind::Domain(name, base, values) => {
                self.cache = None;
                let key = self.new_key(name, &path);
                self.type_aliases.remove(&key);
                self.enum_labels.remove(&key);
                self.types
                    .insert(key, TypeDef::Domain(base.clone(), values.clone()));
                false
            }
            OpKind::Type(name, def) => {
                self.cache = None;
                let key = self.new_key(name, &path);
                self.type_aliases.remove(&key);
                self.enum_labels.remove(&key);
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
            OpKind::UniqueIndex(name, unique) => {
                let Some(key) = self.table_key(name, &path) else {
                    return false;
                };
                self.change_table(key, |table| table.unique.push(unique.clone()));
                false
            }
            OpKind::RowTrigger(trigger) => {
                let Some(key) = self.table_key(&trigger.table, &path) else {
                    return false;
                };
                let name = trigger.name.last().map(|name| name.to_lowercase());
                let name = name.unwrap_or_default();
                self.change_table(key, |table| {
                    if trigger.events.insert {
                        for column in &mut table.columns {
                            column.has_default = true;
                        }
                    }
                    table.row_triggers.retain(|(other, _)| *other != name);
                    table.row_triggers.push((name, trigger.events));
                });
                false
            }
            OpKind::Policy(table, stmt) => {
                let Some(key) = self.table_key(table, &path) else {
                    return false;
                };
                let (dialect, at) = (self.dialect, self.at.clone());
                self.change_table(key, |table| {
                    policies::apply(&mut table.policies, stmt, dialect, at);
                });
                false
            }
            OpKind::DropTrigger(name, table) => {
                let name = name.last().cloned().unwrap_or_default();
                let fires =
                    |table: &RawTable| table.row_triggers.iter().any(|(other, _)| *other == name);
                let keys: Vec<String> = match table {
                    Some(table) => self
                        .table_of(table, &path)
                        .filter(|(_, table)| fires(table))
                        .map(|(key, _)| key)
                        .into_iter()
                        .collect(),
                    // MySQL names a trigger without its table.
                    None => self
                        .relations
                        .iter()
                        .filter(|(_, rel)| rel.table().is_some_and(fires))
                        .map(|(key, _)| key.clone())
                        .collect(),
                };
                for key in keys {
                    self.change_table(key, |table| {
                        table.row_triggers.retain(|(other, _)| *other != name);
                    });
                }
                false
            }
        };
        !blocked
    }

    /// Stop waiting for what `create` needs: a table built from one that never appears
    /// exists, its shape unknown.
    pub(super) fn give_up(&mut self, create: &CreateTable, path: &[String]) {
        self.give_up_as("table", create, path);
    }

    fn give_up_as(&mut self, kind: &'static str, create: &CreateTable, path: &[String]) {
        let key = self.new_key(&create.name, path);
        self.skipped.push((kind, create.name.to_string()));
        self.set_relation(
            key,
            Some(Rel::Table(RawTable {
                unknown: true,
                ..RawTable::default()
            })),
        );
    }

    // ---- CREATE TABLE ---------------------------------------------------------------

    /// Copies the columns of the `LIKE` table `source` into `table`. `Some(created)` ends
    /// [`Self::create_table`]: `false` waits for a table it builds on, `true` gave up.
    fn copy_like(
        &mut self,
        table: &mut RawTable,
        source: &[String],
        create: &CreateTable,
        path: &[String],
    ) -> Option<bool> {
        let Some((_, from_table)) = self.table_of(source, path) else {
            return Some(false);
        };
        if self.too_wide(table.columns.len() + from_table.columns.len()) {
            self.give_up_as("limit", create, path);
            return Some(true);
        }
        table.unknown |= from_table.unknown;
        merge_columns(&mut table.columns, from_table.columns.clone());
        None
    }

    /// Returns whether the table was created; `false` waits for a table it builds on.
    pub(super) fn create_table(
        &mut self,
        create: &CreateTable,
        likes: &[Like],
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
                let Some((parent, parent_table)) = self.table_of(source, path) else {
                    return false;
                };
                if self.too_wide(table.columns.len() + parent_table.columns.len()) {
                    self.give_up_as("limit", create, path);
                    return true;
                }
                table.unknown |= parent_table.unknown;
                merge_columns(&mut table.columns, parent_table.columns.clone());
                self.inherited.insert(parent.clone());
                table.parents.push(parent);
            }
        }
        if let (
            true,
            Some(CreateTableLikeKind::Parenthesized(like) | CreateTableLikeKind::Plain(like)),
        ) = (
            self.postgres || self.dialect == SqlDialect::MySql,
            &create.like,
        ) {
            let source = parts(&like.name, self.dialect);
            if let Some(done) = self.copy_like(&mut table, &source, create, path) {
                return done;
            }
        }
        // A PostgreSQL `PARTITION OF` column without a type only adds constraints; a SQLite
        // column may have no type at all. A `LIKE` item is a placeholder column (see
        // `Statement::without_like`): the columns it copies go where it stands.
        for column in &create.columns {
            let name = fold(&column.name, self.dialect);
            let like = likes
                .iter()
                .find(|(placeholder, _)| *placeholder == name)
                .map(|(_, source)| source);
            if let Some(source) = like {
                if let Some(done) = self.copy_like(&mut table, source, create, path) {
                    return done;
                }
            } else if column.data_type != DataType::Unspecified
                || self.dialect == SqlDialect::Sqlite
            {
                merge_columns(&mut table.columns, [raw_column(column, self.dialect)]);
            }
        }
        // SQLite lets a PRIMARY KEY hold NULL, except in a WITHOUT ROWID or STRICT table or
        // as the sole `INTEGER` column, which is the rowid.
        let loose_keys =
            self.dialect == SqlDialect::Sqlite && create.sqlite_table_options.options.is_empty();
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
                let loose = keyed && !declared && !is_rowid_type(&def.data_type);
                if let Some(column) = table
                    .columns
                    .iter_mut()
                    .find(|c| c.name == name)
                    .filter(|_| loose)
                {
                    column.not_null = false;
                }
            }
        }
        for def in &create.columns {
            for option in &def.options {
                if let ColumnOption::Check(check) = &option.option {
                    table
                        .nonnull_groups
                        .extend(nonnull_groups(&check.expr, self.dialect));
                    table.checks.extend(discriminating(&check.expr));
                }
            }
        }
        for constraint in &create.constraints {
            if let TableConstraint::Check(check) = constraint {
                apply_check(&mut table.columns, &check.expr, self.dialect);
                table
                    .nonnull_groups
                    .extend(nonnull_groups(&check.expr, self.dialect));
                table.checks.extend(discriminating(&check.expr));
            }
            if let TableConstraint::PrimaryKey(pk) = constraint {
                // SQLite fills a sole `INTEGER PRIMARY KEY`, the rowid, on INSERT.
                if let ([key], SqlDialect::Sqlite) = (pk.columns.as_slice(), self.dialect) {
                    if let Expr::Identifier(ident) = &key.column.expr {
                        let name = fold(ident, self.dialect);
                        if let Some(column) = table.columns.iter_mut().find(|c| c.name == name) {
                            column.has_default |= is_rowid_type(&column.ty);
                        }
                    }
                }
                if loose_keys
                    && !(pk.columns.len() == 1
                        && column_named(&table.columns, &pk.columns[0].column.expr, self.dialect)
                            .is_some_and(|c| is_rowid_type(&c.ty)))
                {
                    // Still the key, as a sole column's: SQLite's NULLs are distinct.
                    for (position, index_column) in pk.columns.iter().enumerate() {
                        mark_loose_primary_key(
                            &mut table.columns,
                            &index_column.column.expr,
                            self.dialect,
                            position,
                        );
                    }
                    continue;
                }
                for (position, index_column) in pk.columns.iter().enumerate() {
                    let expr = &index_column.column.expr;
                    mark_primary_key(&mut table.columns, expr, self.dialect, position);
                }
            }
        }
        for def in &create.columns {
            for option in &def.options {
                if let ColumnOption::Unique(_) = option.option {
                    table.unique.push(RawUnique {
                        name: option.name.as_ref().map(|name| fold(name, self.dialect)),
                        columns: vec![fold(&def.name, self.dialect)],
                    });
                }
            }
        }
        for constraint in &create.constraints {
            if let TableConstraint::Unique(unique) = constraint {
                let name = unique.name.as_ref().or(unique.index_name.as_ref());
                table
                    .unique
                    .extend(unique_key(name, &unique.columns, self.dialect));
            }
        }
        let column_keys = create.columns.iter().flat_map(|def| {
            def.options
                .iter()
                .filter_map(move |option| match &option.option {
                    ColumnOption::ForeignKey(fk) => Some((fk, vec![def.name.clone()])),
                    _ => None,
                })
        });
        let table_keys = create
            .constraints
            .iter()
            .filter_map(|constraint| match constraint {
                TableConstraint::ForeignKey(fk) => Some((fk, fk.columns.clone())),
                _ => None,
            });
        // MySQL parses the foreign keys of a table of an engine other than InnoDB, and
        // ignores them.
        let other_engine = matches!(&create.table_options, CreateTableOptions::Plain(options)
            if options.iter().any(|option| matches!(option,
                SqlOption::NamedParenthesizedList(list)
                    if list.key.value.eq_ignore_ascii_case("engine")
                        && list.name.as_ref().is_some_and(|name| !name.value.eq_ignore_ascii_case("innodb")))));
        if !other_engine {
            let declared: Vec<RawForeignKey> = column_keys
                .chain(table_keys)
                .map(|(fk, columns)| self.foreign_key(fk, &columns, &key, &table, path, true))
                .collect();
            table.foreign_keys = declared;
        }
        if create.query.is_none() && !table.columns.is_empty() {
            self.set_relation(key, Some(Rel::Table(table)));
        }
        true
    }

    /// A foreign key of the table `key`, which `table` is, over `columns`. One to a table
    /// not defined yet waits for [`Loader::resolve_foreign_keys`].
    pub(super) fn foreign_key(
        &self,
        fk: &ForeignKeyConstraint,
        columns: &[Ident],
        key: &str,
        table: &RawTable,
        path: &[String],
        validated: bool,
    ) -> RawForeignKey {
        let parts = parts(&fk.foreign_table, self.dialect);
        let found = if self.new_key(&fk.foreign_table, path) == key {
            Some((key.to_string(), table))
        } else {
            self.table_of(&parts, path)
        };
        let referred: Vec<String> = fk
            .referred_columns
            .iter()
            .map(|ident| fold(ident, self.dialect))
            .collect();
        let (target, referred, waiting) = match found {
            Some((target, columns_of)) if referred.is_empty() => {
                (target, primary_key(columns_of), None)
            }
            Some((target, _)) => (target, referred, None),
            None => (
                String::new(),
                referred,
                Some(Box::new((parts, path.to_vec()))),
            ),
        };
        let characteristics = fk.characteristics.as_ref();
        RawForeignKey {
            name: fk.name.as_ref().map(|name| fold(name, self.dialect)),
            key: ForeignKey {
                columns: columns
                    .iter()
                    .map(|ident| fold(ident, self.dialect))
                    .collect(),
                table: target,
                referred,
            },
            immediate: characteristics.and_then(|c| c.deferrable) != Some(true)
                && characteristics.and_then(|c| c.enforced) != Some(false),
            validated,
            waiting,
        }
    }

    /// Point the foreign keys that name a table defined after them (`mysqldump` orders
    /// tables by name) at it, and drop those whose table never is.
    pub(super) fn resolve_foreign_keys(&mut self) {
        let mut found = FxHashMap::default();
        for (key, rel) in &self.relations {
            let Some(table) = rel.table() else { continue };
            let targets: Vec<_> = table
                .foreign_keys
                .iter()
                .filter_map(|fk| {
                    let (parts, path) = fk.waiting.as_deref()?;
                    Some(self.table_of(parts, path).map(|(target, columns_of)| {
                        let referred = fk.key.referred.is_empty().then(|| primary_key(columns_of));
                        (target, referred)
                    }))
                })
                .collect();
            if !targets.is_empty() {
                found.insert(key.clone(), targets);
            }
        }
        if found.is_empty() {
            return;
        }
        self.cache = None;
        for (key, rel) in &mut self.relations {
            let (Rel::Table(table), Some(targets)) = (rel, found.remove(key)) else {
                continue;
            };
            let mut targets = targets.into_iter();
            table.foreign_keys.retain_mut(|fk| {
                if fk.waiting.take().is_none() {
                    return true;
                }
                let Some((target, referred)) = targets.next().flatten() else {
                    return false;
                };
                fk.key.table = target;
                if let Some(referred) = referred {
                    fk.key.referred = referred;
                }
                true
            });
        }
    }

    // ---- CREATE VIEW ----------------------------------------------------------------

    /// A `table` made by `CREATE TABLE ... AS` has no constraints: its columns may hold
    /// any value of their type, and NULL outside MySQL, which keeps `NOT NULL`.
    pub(super) fn create_view(
        &mut self,
        view: &CreateView,
        table: bool,
        path: &[String],
        force: bool,
    ) -> bool {
        if view.if_not_exists && self.relations.contains_key(&self.new_key(&view.name, path)) {
            return true;
        }
        let (relations_defined, undefined_calls) = self.view_sources(&view.query, path);
        if !force && !relations_defined {
            return false;
        }
        let key = self.new_key(&view.name, path);
        let dialect = self.dialect;
        let mut columns = view_columns(self.snapshot(), dialect, view);
        if table {
            for column in &mut columns {
                forget_values(column, dialect);
                let constant = column.python_type.as_deref().and_then(values::parse);
                if let (None, Some(first)) = (
                    &column.sql_type,
                    constant.and_then(|v| v.into_iter().next()),
                ) {
                    let sql = match first {
                        Value::Str(_) => "TEXT",
                        Value::Int(_) => "INT",
                    };
                    (column.python_type, column.sql_type) = typed(sql.to_string(), None, dialect);
                }
                if dialect != SqlDialect::MySql {
                    column.nullability = Nullability::Nullable;
                }
            }
        }
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
            ctes: FxHashSet<String>,
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
            ctes: FxHashSet::default(),
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
                forget_values(column, dialect);
            }
        }
    }

    /// Set (or, with `None`, remove) one relation; the next `snapshot` resolves it into the
    /// cached catalog without building it again.
    pub(super) fn set_relation(&mut self, key: String, rel: Option<Rel>) {
        match rel {
            Some(rel) => {
                let rel = self.within_limits(&key, rel);
                self.relations.insert(key.clone(), rel);
            }
            None => {
                self.held = self
                    .held
                    .saturating_sub(self.relations.get(&key).map_or(0, Rel::width));
                self.relations.remove(&key);
            }
        }
        if self.cache.is_none() {
            return;
        }
        let bare = key.split_once('.').map_or(key.as_str(), |(_, name)| name);
        let readers: Vec<String> = [key.as_str(), bare]
            .iter()
            .filter_map(|name| self.returned_by.get(*name))
            .flatten()
            .cloned()
            .collect();
        for reader in readers {
            self.refresh_function(reader);
        }
        self.stale_relations.insert(key);
    }

    /// Move the relation at `from`, if any, to `to`, replacing what is there.
    pub(super) fn move_relation(&mut self, from: &str, to: String) {
        let replaced = self
            .relations
            .remove(from)
            .and_then(|moved| self.relations.insert(to, moved));
        self.held = self
            .held
            .saturating_sub(replaced.map_or(0, |rel| rel.width()));
    }

    /// `rel`, or one of unknown shape when it is wider than [`MAX_COLUMNS`] or would take
    /// the schema past [`MAX_SCHEMA_COLUMNS`] and its allowance.
    fn within_limits(&mut self, key: &str, rel: Rel) -> Rel {
        self.held = self
            .held
            .saturating_sub(self.relations.get(key).map_or(0, Rel::width));
        let rel = if self.too_wide(rel.width()) {
            self.skipped.push(("limit", key.to_string()));
            match rel {
                Rel::Table(_) => Rel::Table(RawTable {
                    unknown: true,
                    ..RawTable::default()
                }),
                Rel::View(_) => Rel::View(vec![Column::unknown("*")]),
            }
        } else {
            rel
        };
        self.held += rel.width();
        self.written += rel.width();
        rel
    }

    /// Raise the limits by the columns `schema` could spell out, two bytes each as in
    /// SQLite's `a,`: those take time and memory in proportion to the schema.
    pub(super) fn allow(&mut self, schema: &str) {
        self.allowance += schema.len() / 2;
    }

    /// Read the statements that follow under `path`.
    pub(super) fn set_search_path(&mut self, path: Vec<String>) {
        self.search_path = path;
        self.cache = None;
    }

    /// Whether the loader wrote [`MAX_WRITTEN`] columns: it reads no more statements.
    pub(super) fn exhausted(&self) -> bool {
        self.written > MAX_WRITTEN + self.allowance
    }

    fn too_wide(&self, width: usize) -> bool {
        width > MAX_COLUMNS || self.held + width > MAX_SCHEMA_COLUMNS + self.allowance
    }

    /// Tables that inherit from or are partitions of `key`, nested ones included.
    pub(super) fn descendants(&self, key: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let mut queue = vec![key.to_string()];
        while let Some(parent) = queue.pop() {
            if !self.inherited.contains(&parent) {
                continue;
            }
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
                    self.enum_labels.remove(&key);
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
                    self.functions.entry(key.clone()).or_default();
                    self.unknown_functions.insert(key.clone());
                    self.refresh_function(key);
                }
                true
            }
        }
    }

    // ---- DROP -----------------------------------------------------------------------

    pub(super) fn drop_objects(&mut self, stmt: &Statement, path: &[String]) {
        // Dropping a relation or a function updates the cached catalog in place; anything
        // else (types, schemas) may change what columns resolve to.
        if !matches!(
            stmt,
            Statement::Drop {
                object_type: ObjectType::Table | ObjectType::View | ObjectType::MaterializedView,
                ..
            } | Statement::DropFunction(_)
        ) {
            self.cache = None;
        }
        if let Statement::Drop {
            object_type, names, ..
        } = stmt
        {
            for name in names {
                let name_parts = parts(name, self.dialect);
                match object_type {
                    ObjectType::Table | ObjectType::View | ObjectType::MaterializedView => {
                        if let Some(key) = self.lookup(&self.relations, &name_parts, path) {
                            self.set_relation(key, None);
                        }
                    }
                    ObjectType::Index => {
                        let index = name_parts.last().cloned().unwrap_or_default();
                        for table in self.relations.values_mut() {
                            if let Rel::Table(table) = table {
                                table.unique.retain(|key| key.name.as_ref() != Some(&index));
                            }
                        }
                    }
                    ObjectType::Type => {
                        if let Some(key) = self.lookup(&self.types, &name_parts, path) {
                            self.enum_labels.remove(&key);
                            self.types.remove(&key);
                        }
                    }
                    ObjectType::Schema => {
                        // A name of no parts, `IDENTIFIER(...)`, is the prefix `.` of no key.
                        let schema = name_parts.last().map_or("", String::as_str);
                        let prefix = format!("{schema}.");
                        self.relations.retain(|key, _| !key.starts_with(&prefix));
                        self.held = self.relations.values().map(Rel::width).sum();
                        self.types.retain(|key, _| !key.starts_with(&prefix));
                        self.enum_labels.retain(|key, _| !key.starts_with(&prefix));
                        self.functions.retain(|key, _| !key.starts_with(&prefix));
                    }
                    _ => {}
                }
            }
        }
        if let Statement::DropDomain(drop) = stmt {
            if let Some(key) = self.lookup(&self.types, &parts(&drop.name, self.dialect), path) {
                self.enum_labels.remove(&key);
                self.types.remove(&key);
            }
        }
        if let Statement::DropFunction(drop) = stmt {
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
                let overloads = self.functions.entry(key.clone()).or_default();
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
                self.refresh_function(key);
            }
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
            Some(FunctionReturnType::DataType(DataType::Table(Some(columns)))) => {
                Some(RawReturn::Rows(
                    columns
                        .iter()
                        .map(|c| (fold(&c.name, self.dialect), c.data_type.clone()))
                        .collect(),
                ))
            }
            Some(FunctionReturnType::DataType(dt)) if outs.len() < 2 => {
                Some(RawReturn::Scalar(dt.clone()))
            }
            Some(FunctionReturnType::SetOf(dt)) => Some(RawReturn::SetOf(dt.clone())),
            _ if outs.len() >= 2 => Some(RawReturn::Rows(outs)),
            _ if outs.len() == 1 => Some(RawReturn::Scalar(outs[0].1.clone())),
            _ => None,
        };
        let Some(returns) = returns
            .filter(|returns| !matches!(returns, RawReturn::Rows(c) if c.len() > MAX_COLUMNS))
        else {
            self.functions.entry(key.clone()).or_default();
            self.unknown_functions.insert(key.clone());
            self.refresh_function(key);
            return;
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
            strict: matches!(
                create.called_on_null,
                Some(FunctionCalledOnNull::Strict | FunctionCalledOnNull::ReturnsNullOnNullInput)
            ),
        };
        self.unknown_functions.remove(&key);
        let dialect = self.dialect;
        let signature = |f: &RawFunction| -> Vec<String> {
            f.args
                .iter()
                .map(|dt| canonical_type(dt, dialect))
                .collect()
        };
        if let RawReturn::Scalar(ty) | RawReturn::SetOf(ty) = &function.returns {
            if let Some(name) = type_key(ty, dialect) {
                self.returned_by
                    .entry(name)
                    .or_default()
                    .insert(key.clone());
            }
        }
        let overloads = self.functions.entry(key.clone()).or_default();
        let wanted = signature(&function);
        overloads.retain(|f| signature(f) != wanted);
        overloads.push(function);
        self.refresh_function(key);
    }
}

/// The primary key columns of `table`, in key order.
fn primary_key(table: &RawTable) -> Vec<String> {
    let mut key: Vec<_> = table.columns.iter().filter(|c| c.primary_key).collect();
    key.sort_by_key(|column| column.key_position);
    key.into_iter().map(|column| column.name.clone()).collect()
}

/// Widen a `Literal` `column` taken from a text or integer column to `str` / `int`.
fn forget_values(column: &mut Column, dialect: SqlDialect) {
    let (Some(python), Some(sql)) = (&column.python_type, &column.sql_type) else {
        return;
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
