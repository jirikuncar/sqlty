//! The schema catalog: what a schema defines, and how names resolve against it.
//!
//! [`load`] reads schema DDL (split by [`crate::ddl`]) into a [`Catalog`]. Statements
//! are applied in file order; one whose dependencies are not defined yet (an `ALTER` of
//! a table created further down, a view over a later view, `LIKE` / `INHERITS` /
//! `PARTITION OF` a later table) is retried until nothing changes, so migrations load in
//! any order. The catalog holds:
//!
//! - relations: tables, with each column's type, nullability and allowed values, and views
//! - enum, domain and composite types, and function return types
//! - the `search_path`, and the built-in `pg_catalog` / `information_schema` relations
//!
//! The analyzer asks a [`Catalog`] by name: [`Catalog::table`], [`Catalog::view`],
//! [`Catalog::user_type`], [`Catalog::composite`] and [`Catalog::function`]. A name is
//! `table`, or `schema.table` in PostgreSQL; a bare name tries `pg_catalog`, then each
//! schema of the `search_path` (`public` unless the schema sets it), and no other schema.
//! PostgreSQL names keep the case of a quoted identifier and fold an unquoted one to lower
//! case. A table the schema changes in a way this
//! module cannot follow (unparsable `ALTER TABLE`, an unknown `LIKE` source) has an
//! unknown shape: it resolves to nothing, so no column of it gets a type.
//!
//! Example:
//! ```
//! use sqlty_sql::catalog::load;
//!
//! let (catalog, _) = load(
//!     "CREATE TABLE users (id int PRIMARY KEY, name text); \
//!      ALTER TABLE users ADD COLUMN age int NOT NULL;",
//!     "postgres",
//! );
//! let columns = catalog.table("users").unwrap();
//! assert_eq!(columns.len(), 3);
//! assert!(columns[0].not_null && !columns[1].not_null && columns[2].not_null);
//! ```

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::rc::Rc;

use once_cell::sync::Lazy;
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, AlterTypeOperation, ArgMode, ArrayElemTypeDef,
    ColumnDef, ColumnOption, CreateFunction, CreateTable, CreateTableLikeKind, CreateView,
    DataType, Expr, FunctionReturnType, GeneratedAs, Ident, ObjectName, ObjectType, Query,
    RenameTableNameKind, Statement, TableConstraint, UserDefinedTypeRepresentation, Visit, Visitor,
};
use sqlparser::parser::Parser;

use crate::analyzer::{
    canonical_type, parse_ddl, typed, view_columns, Column, SkippedObject, SqlDialect,
};
use crate::ddl;

/// `(python_type, sql_type)`; `None` is unknown.
pub type Typed = (Option<String>, Option<String>);

/// A column of a table or composite type.
#[derive(Clone, Debug)]
pub struct SchemaColumn {
    /// Lowercased name, for lookups and result column names.
    pub name: String,
    /// The name as SQL spells it, quotes kept, for `SELECT *` expansion.
    pub ident: String,
    pub sql_type: String,
    /// The Python type when it is not the one `sql_type` maps to (an enum's `Literal`).
    pub(crate) python_type: Option<String>,
    /// Declared `NOT NULL`, `PRIMARY KEY` or an identity / `serial` column. Not used for
    /// inference yet.
    pub not_null: bool,
    /// The labels of the enum the column has, `None` for other types.
    pub allowed_values: Option<Vec<String>>,
}

/// A domain, enum or composite type as its columns see it: the SQL type they show (a
/// domain's base type, an enum's own name) and the Python type when that is a `Literal`
/// of the enum's labels.
#[derive(Clone, Debug)]
pub struct UserType {
    pub sql_type: String,
    pub python_type: Option<String>,
    /// An enum's labels.
    pub allowed_values: Option<Vec<String>>,
}

/// What a function returns.
#[derive(Clone, Debug)]
pub enum FunctionReturn {
    /// One value of a type.
    Scalar(Typed),
    /// `SETOF type`: one value per row.
    SetOf(Typed),
    /// `RETURNS TABLE(...)`, `SETOF table` or several `OUT` parameters: rows of columns.
    Rows(Vec<SchemaColumn>),
}

/// A function overload.
#[derive(Clone, Debug)]
pub struct Function {
    /// Arguments a call must give.
    pub required: usize,
    /// Arguments a call may give; `usize::MAX` for a `VARIADIC` function.
    pub max: usize,
    pub returns: FunctionReturn,
}

#[derive(Clone, Debug)]
enum Relation {
    Table(Vec<SchemaColumn>),
    View(Vec<Column>),
    /// Defined, but its shape is not known.
    Unknown,
}

/// The resolved schema: what the analyzer reads. Built by [`load`]; shared, so caching it
/// is cheap.
#[derive(Default)]
pub struct Catalog {
    /// `schema.name` -> relation.
    relations: HashMap<String, Relation>,
    /// `schema.name` -> domain and enum types.
    types: HashMap<String, UserType>,
    /// `schema.name` -> composite type fields in declaration order.
    composites: HashMap<String, Vec<SchemaColumn>>,
    /// `schema.name` -> overloads; a function of unknown shape is absent.
    functions: HashMap<String, Vec<Function>>,
    /// `schema.name` of the functions the schema defines but whose shape is unknown.
    unknown_functions: HashMap<String, ()>,
    /// `schema.name` of every domain, enum and composite type the schema defines, those
    /// of unknown shape included.
    type_names: HashMap<String, ()>,
    /// Schemas a bare name is looked up in, in order.
    search_path: Vec<String>,
    /// Relations the schema itself defines, built-ins not counted.
    user_relations: usize,
    postgres: bool,
}

const BUILTIN_SCHEMAS: [&str; 2] = ["pg_catalog", "information_schema"];

/// The key a bare name is tried under in each schema of `path`; PostgreSQL always
/// searches `pg_catalog` first.
fn lookup_key<'a, T>(map: &'a HashMap<String, T>, path: &[String], name: &str) -> Option<&'a str> {
    if name.contains('.') {
        return map.get_key_value(name).map(|(key, _)| key.as_str());
    }
    for schema in std::iter::once("pg_catalog").chain(path.iter().map(String::as_str)) {
        if let Some((key, _)) = map.get_key_value(&format!("{schema}.{name}")) {
            return Some(key);
        }
    }
    None
}

impl Catalog {
    /// `name` as this dialect keys it: other dialects have no schemas, so `db.t` is `t`.
    fn norm<'a>(&self, name: &'a str) -> &'a str {
        if self.postgres {
            name
        } else {
            name.rsplit('.').next().unwrap_or(name)
        }
    }

    fn find(&self, name: &str) -> Option<&Relation> {
        lookup_key(&self.relations, &self.search_path, self.norm(name))
            .and_then(|key| self.relations.get(key))
    }

    /// Columns of the table `name`, or `None` for a view, an unknown name or a table of
    /// unknown shape.
    pub fn table(&self, name: &str) -> Option<&Vec<SchemaColumn>> {
        match self.find(name)? {
            Relation::Table(columns) => Some(columns),
            _ => None,
        }
    }

    /// Whether the schema defines a table or view `name`, whatever is known of its shape.
    pub fn defines(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Whether `name` is a PostgreSQL system relation (`pg_catalog.x`,
    /// `information_schema.x`, a bare `pg_*`) that the catalog does not model, so its
    /// columns are unknown. A relation the schema or the built-ins define is never one.
    pub fn is_unmodelled_system_relation(&self, name: &str) -> bool {
        self.postgres
            && self.find(name).is_none()
            && match name.split_once('.') {
                Some((schema, _)) => BUILTIN_SCHEMAS.contains(&schema),
                None => name.starts_with("pg_"),
            }
    }

    /// Output columns of the view `name`.
    pub fn view(&self, name: &str) -> Option<&Vec<Column>> {
        match self.find(name)? {
            Relation::View(columns) => Some(columns),
            _ => None,
        }
    }

    /// The domain or enum `name`; for a domain, its base type.
    pub fn user_type(&self, name: &str) -> Option<&UserType> {
        lookup_key(&self.types, &self.search_path, self.norm(name))
            .and_then(|key| self.types.get(key))
    }

    /// Fields of the composite type `name`.
    pub fn composite(&self, name: &str) -> Option<&Vec<SchemaColumn>> {
        lookup_key(&self.composites, &self.search_path, self.norm(name))
            .and_then(|key| self.composites.get(key))
    }

    /// What the function `name` returns when called with `args` arguments, if exactly one
    /// overload (or only one return type) fits.
    pub fn function(&self, name: &str, args: usize) -> Option<&FunctionReturn> {
        let key = lookup_key(&self.functions, &self.search_path, self.norm(name))?;
        let mut fits = self.functions[key]
            .iter()
            .filter(|f| f.required <= args && args <= f.max);
        let first = fits.next()?;
        fits.all(|f| same_return(&f.returns, &first.returns))
            .then_some(&first.returns)
    }

    /// Whether the schema defines a function `name`, whatever is known of what it returns.
    pub fn defines_function(&self, name: &str) -> bool {
        let name = self.norm(name);
        lookup_key(&self.functions, &self.search_path, name).is_some()
            || lookup_key(&self.unknown_functions, &self.search_path, name).is_some()
    }

    /// Whether the schema defines a domain, enum or composite type `name`.
    pub fn defines_type(&self, name: &str) -> bool {
        lookup_key(&self.type_names, &self.search_path, self.norm(name)).is_some()
    }

    /// Whether the schema defines no table or view of its own.
    pub fn is_empty(&self) -> bool {
        self.user_relations == 0
    }

    /// The SQL type of a column declared `dt`, and its Python type when a domain or enum
    /// sets it. Domains and enums resolve through the catalog.
    pub fn resolve_type(&self, dt: &DataType, dialect: SqlDialect) -> (String, Option<String>) {
        resolve_with(dt, &|name| self.user_type(name).cloned(), dialect)
    }
}

fn same_return(a: &FunctionReturn, b: &FunctionReturn) -> bool {
    match (a, b) {
        (FunctionReturn::Scalar(a), FunctionReturn::Scalar(b))
        | (FunctionReturn::SetOf(a), FunctionReturn::SetOf(b)) => a == b,
        _ => false,
    }
}

/// `dt` as `(sql_type, python_type)`, user types looked up by `user`.
fn resolve_with(
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
    match type_key(dt, dialect).and_then(|name| user(&name)) {
        Some(found) => (found.sql_type, found.python_type),
        None => (canonical_type(dt, dialect), None),
    }
}

/// The lookup name of a user-defined type such as `app.percent`.
fn type_key(dt: &DataType, dialect: SqlDialect) -> Option<String> {
    match dt {
        DataType::Custom(name, modifiers) if modifiers.is_empty() => Some(name_key(name, dialect)),
        _ => None,
    }
}

/// An identifier as the dialect keys it. PostgreSQL folds an unquoted name to lower case and
/// keeps a quoted one exactly; MySQL and SQLite compare names case-insensitively.
pub(crate) fn fold(ident: &Ident, dialect: SqlDialect) -> String {
    if dialect == SqlDialect::Postgres && ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_lowercase()
    }
}

fn parts(name: &ObjectName, dialect: SqlDialect) -> Vec<String> {
    name.0
        .iter()
        .filter_map(|part| part.as_ident())
        .map(|ident| fold(ident, dialect))
        .collect()
}

fn key_of(parts: &[String], dialect: SqlDialect) -> String {
    let keep = if dialect == SqlDialect::Postgres {
        2
    } else {
        1
    };
    parts[parts.len().saturating_sub(keep)..].join(".")
}

/// The name a lookup uses: `schema.name` or `name` in PostgreSQL, `name` elsewhere.
fn name_key(name: &ObjectName, dialect: SqlDialect) -> String {
    key_of(&parts(name, dialect), dialect)
}

/// An empty catalog, with the built-in relations in PostgreSQL: what queries read
/// when there is no schema.
pub fn empty(dialect: SqlDialect) -> &'static Catalog {
    static POSTGRES: Lazy<Catalog> = Lazy::new(|| load("", "postgres").0);
    static OTHER: Lazy<Catalog> = Lazy::new(Catalog::default);
    if dialect == SqlDialect::Postgres {
        &POSTGRES
    } else {
        &OTHER
    }
}

// ---------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------

#[derive(Clone)]
struct RawColumn {
    name: String,
    ident: String,
    ty: DataType,
    not_null: bool,
}

#[derive(Clone, Default)]
struct RawTable {
    columns: Vec<RawColumn>,
    /// Tables it inherits from or is a partition of.
    parents: Vec<String>,
    /// The shape is not known: nothing may be inferred from it.
    unknown: bool,
}

enum Rel {
    Table(RawTable),
    View(Vec<Column>),
}

#[derive(Clone)]
enum TypeDef {
    Domain(DataType),
    Enum(Vec<String>),
    Composite(Vec<(String, DataType)>),
    /// Altered in a way this module cannot follow.
    Unknown,
}

struct RawFunction {
    args: Vec<DataType>,
    required: usize,
    variadic: bool,
    returns: RawReturn,
}

enum RawReturn {
    Scalar(DataType),
    SetOf(DataType),
    Rows(Vec<(String, DataType)>),
}

enum OpKind {
    Table(Box<CreateTable>, Vec<Vec<String>>),
    View(Box<CreateView>),
    Domain(ObjectName, DataType),
    Type(ObjectName, TypeDef),
    AlterTable(Box<sqlparser::ast::AlterTable>),
    AlterType(sqlparser::ast::AlterType),
    Drop(Box<Statement>),
    Function(Box<CreateFunction>),
    /// A statement that changes the named relation, type or function in a way this
    /// module cannot follow.
    Taint(Taint, Vec<String>),
}

#[derive(Clone, Copy)]
enum Taint {
    Relation,
    Type,
    Function,
}

/// A statement and the `search_path` it ran under.
struct Op {
    path: Rc<Vec<String>>,
    kind: OpKind,
}

/// Why an `ALTER TABLE` operation did not apply.
enum AlterFail {
    /// A column it names does not exist (yet).
    Missing,
    /// The operation may change columns in a way this module does not follow.
    Unsupported,
}

struct Loader {
    dialect: SqlDialect,
    postgres: bool,
    relations: HashMap<String, Rel>,
    types: HashMap<String, TypeDef>,
    functions: HashMap<String, Vec<RawFunction>>,
    /// Functions whose latest definition could not be read.
    unknown_functions: HashSet<String>,
    search_path: Vec<String>,
    /// The resolved catalog; a change to one relation updates it in place, any other
    /// change drops it to be built again.
    cache: Option<Catalog>,
    /// Old name -> current name of a renamed type: columns declared with the old name
    /// have the type under its new one.
    type_aliases: HashMap<String, String>,
    /// Objects whose columns or types the loader could not follow.
    skipped: Vec<SkippedObject>,
    /// Some function returns `SETOF table`, so its rows follow the table.
    setof_functions: bool,
}

/// Table-level key of a name seen under `path`.
fn schema_for_new(path: &[String]) -> &str {
    path.iter()
        .map(String::as_str)
        .find(|schema| !matches!(*schema, "$user" | "pg_catalog" | "pg_temp"))
        .unwrap_or("public")
}

impl Loader {
    fn new(dialect: SqlDialect) -> Self {
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
    fn new_key(&self, name: &ObjectName, path: &[String]) -> String {
        let parts = parts(name, self.dialect);
        match parts.as_slice() {
            [] => String::new(),
            [only] => format!("{}.{only}", self.default_schema(path)),
            _ if self.postgres => key_of(&parts, self.dialect),
            [.., last] => format!("public.{last}"),
        }
    }

    fn default_schema<'a>(&self, path: &'a [String]) -> &'a str {
        if self.postgres {
            schema_for_new(path)
        } else {
            "public"
        }
    }

    fn lookup<T>(
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

    fn table_key(&self, parts: &[String], path: &[String]) -> Option<String> {
        let key = self.lookup(&self.relations, parts, path)?;
        matches!(self.relations.get(&key), Some(Rel::Table(_))).then_some(key)
    }

    /// Apply every op, retrying the ones whose dependencies are missing until a pass
    /// changes nothing; then give up on them, views last.
    fn run(&mut self, mut pending: Vec<Op>) {
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
    fn apply(&mut self, op: &Op, force: bool) -> bool {
        let path = op.path.clone();
        let blocked = match &op.kind {
            OpKind::Table(create, likes) => !self.create_table(create, likes, &path),
            OpKind::View(view) => !self.create_view(view, &path, force),
            OpKind::Domain(name, base) => {
                self.cache = None;
                let key = self.new_key(name, &path);
                self.type_aliases.remove(&key);
                self.types.insert(key, TypeDef::Domain(base.clone()));
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
    fn give_up(&mut self, op: &Op) {
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
    fn create_table(
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
        for constraint in &create.constraints {
            if let TableConstraint::PrimaryKey(pk) = constraint {
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

    fn create_view(&mut self, view: &CreateView, path: &[String], force: bool) -> bool {
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
    fn view_sources(&self, query: &Query, path: &[String]) -> (bool, bool) {
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

    // ---- ALTER ----------------------------------------------------------------------

    /// Returns `false` when the table (or a column it changes) is not defined yet.
    fn alter_table(&mut self, alter: &sqlparser::ast::AlterTable, path: &[String]) -> bool {
        let Some(key) = self.table_key(&parts(&alter.name, self.dialect), path) else {
            return alter.if_exists;
        };
        let Some(Rel::Table(root)) = self.relations.get(&key) else {
            return true;
        };
        let mut root = root.clone();
        let mut rename = None;
        let mut failed = None;
        for op in &alter.operations {
            if let Err(fail) = alter_op(&mut root, op, true, &mut rename, self.dialect) {
                failed = Some(fail);
                break;
            }
        }
        if matches!(failed, Some(AlterFail::Missing)) {
            return false;
        }
        let unsupported = failed.is_some();
        if unsupported {
            self.skipped.push(("table", alter.name.to_string()));
        }
        let mut changed = vec![(key.clone(), root)];
        if !alter.only {
            for child in self.descendants(&key) {
                let Some(Rel::Table(table)) = self.relations.get(&child) else {
                    continue;
                };
                let mut table = table.clone();
                for op in &alter.operations {
                    let _ = alter_op(&mut table, op, false, &mut None, self.dialect);
                }
                changed.push((child, table));
            }
        }
        for (name, mut table) in changed {
            table.unknown |= unsupported;
            self.set_relation(name, Some(Rel::Table(table)));
        }
        if let Some(new_name) = rename {
            let (schema, _) = key.split_once('.').unwrap_or(("public", ""));
            let new_key = match parts(&new_name, self.dialect).as_slice() {
                [only] => format!("{schema}.{only}"),
                other => key_of(other, self.dialect),
            };
            self.cache = None;
            if let Some(table) = self.relations.remove(&key) {
                self.relations.insert(new_key.clone(), table);
            }
            for rel in self.relations.values_mut() {
                if let Rel::Table(table) = rel {
                    for parent in &mut table.parents {
                        if *parent == key {
                            parent.clone_from(&new_key);
                        }
                    }
                }
            }
        }
        true
    }

    /// Set (or, with `None`, remove) one relation, keeping the resolved catalog current
    /// without building it again.
    fn set_relation(&mut self, key: String, rel: Option<Rel>) {
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
    fn descendants(&self, key: &str) -> Vec<String> {
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

    fn alter_type(&mut self, alter: &sqlparser::ast::AlterType, path: &[String]) -> bool {
        let Some(key) = self.lookup(&self.types, &parts(&alter.name, self.dialect), path) else {
            return false;
        };
        self.cache = None;
        match (&alter.operation, self.types.get_mut(&key)) {
            (AlterTypeOperation::AddValue(add), Some(TypeDef::Enum(labels))) => {
                let value = add.value.value.clone();
                if !labels.contains(&value) {
                    let at = add.position.as_ref().and_then(|position| {
                        let (anchor, after) = match position {
                            sqlparser::ast::AlterTypeAddValuePosition::Before(anchor) => {
                                (anchor, 0)
                            }
                            sqlparser::ast::AlterTypeAddValuePosition::After(anchor) => (anchor, 1),
                        };
                        labels
                            .iter()
                            .position(|label| *label == anchor.value)
                            .map(|at| at + after)
                    });
                    labels.insert(at.unwrap_or(labels.len()), value);
                }
            }
            (AlterTypeOperation::RenameValue(rename), Some(TypeDef::Enum(labels))) => {
                match labels.iter_mut().find(|label| **label == rename.from.value) {
                    Some(label) => label.clone_from(&rename.to.value),
                    None => return false,
                }
            }
            (AlterTypeOperation::Rename(rename), Some(_)) => {
                let schema = key.split_once('.').map_or("public", |(schema, _)| schema);
                let new_key = format!("{schema}.{}", fold(&rename.new_name, self.dialect));
                if let Some(def) = self.types.remove(&key) {
                    for target in self.type_aliases.values_mut() {
                        if *target == key {
                            target.clone_from(&new_key);
                        }
                    }
                    self.type_aliases.remove(&new_key);
                    self.type_aliases.insert(key, new_key.clone());
                    self.types.insert(new_key, def);
                }
            }
            (_, Some(def)) => {
                *def = TypeDef::Unknown;
                self.skipped.push(("type", alter.name.to_string()));
            }
            _ => {}
        }
        true
    }

    fn taint(&mut self, what: Taint, name: &[String], path: &[String]) -> bool {
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

    fn drop_objects(&mut self, stmt: &Statement, path: &[String]) {
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

    fn create_function(&mut self, create: &CreateFunction, path: &[String]) {
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

    // ---- Resolution -----------------------------------------------------------------

    /// The catalog as it stands now.
    fn snapshot(&mut self) -> &Catalog {
        if self.cache.is_none() {
            self.cache = Some(self.build());
        }
        self.cache.as_ref().unwrap()
    }

    fn build(&self) -> Catalog {
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
        let column = |name: &str, ident: &str, ty: &DataType, not_null: bool| {
            let (sql_type, python_type) = resolve_with(ty, &lookup, dialect);
            let allowed_values = type_key(ty, dialect)
                .and_then(|name| lookup(&name))
                .and_then(|user| user.allowed_values);
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
                            .map(|c| column(&c.name, &c.ident, &c.ty, c.not_null))
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
                        .map(|(name, ty)| column(name, name, ty, false))
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
                            .map(|(name, ty)| column(name, name, ty, false))
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
    fn resolve_user_type(&self, key: &str, path: &[String], depth: usize) -> Option<UserType> {
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
            TypeDef::Domain(base) => {
                let inner = type_key(base, self.dialect)
                    .filter(|_| depth < 16)
                    .and_then(|name| lookup_key(&self.types, path, &name))
                    .and_then(|inner| self.resolve_user_type(inner, path, depth + 1));
                inner.unwrap_or_else(|| UserType {
                    sql_type: canonical_type(base, self.dialect),
                    python_type: None,
                    allowed_values: None,
                })
            }
        })
    }
}

/// A column declared `ty`, resolved against `catalog`.
fn schema_column(
    catalog: &Catalog,
    dialect: SqlDialect,
    name: &str,
    ident: &str,
    ty: &DataType,
    not_null: bool,
) -> SchemaColumn {
    let (sql_type, python_type) = catalog.resolve_type(ty, dialect);
    let allowed_values = type_key(ty, dialect)
        .and_then(|name| catalog.user_type(&name))
        .and_then(|user| user.allowed_values.clone());
    SchemaColumn {
        name: name.to_string(),
        ident: ident.to_string(),
        sql_type,
        python_type,
        not_null,
        allowed_values,
    }
}

fn resolve_rel(catalog: &Catalog, dialect: SqlDialect, rel: &Rel) -> Relation {
    match rel {
        Rel::Table(table) if table.unknown => Relation::Unknown,
        Rel::Table(table) => Relation::Table(
            table
                .columns
                .iter()
                .map(|c| schema_column(catalog, dialect, &c.name, &c.ident, &c.ty, c.not_null))
                .collect(),
        ),
        Rel::View(columns) => Relation::View(columns.clone()),
    }
}

/// The labels of an enum as a `typing.Literal`.
fn literal_of(labels: &[String]) -> String {
    let labels: Vec<String> = labels
        .iter()
        .map(|label| format!("'{}'", label.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("typing.Literal[{}]", labels.join(", "))
}

fn raw_column(column: &ColumnDef, dialect: SqlDialect) -> RawColumn {
    let serial = matches!(&column.data_type, DataType::Custom(name, _)
        if parts(name, dialect).last().is_some_and(|n| matches!(n.as_str(), "serial" | "bigserial" | "smallserial" | "serial2" | "serial4" | "serial8")));
    let not_null = serial
        || column.options.iter().any(|option| {
            matches!(
                option.option,
                ColumnOption::NotNull | ColumnOption::PrimaryKey(_)
            ) || matches!(
                &option.option,
                ColumnOption::Generated {
                    generated_as: GeneratedAs::Always | GeneratedAs::ByDefault,
                    generation_expr: None,
                    ..
                }
            )
        });
    RawColumn {
        name: fold(&column.name, dialect),
        ident: match column.name.quote_style {
            Some(_) => column.name.to_string(),
            None => column.name.value.to_lowercase(),
        },
        ty: column.data_type.clone(),
        not_null,
    }
}

/// Add `column`, or replace the same-named one in place (inherited columns merge).
fn merge_column(columns: &mut Vec<RawColumn>, column: RawColumn) {
    match columns.iter_mut().find(|c| c.name == column.name) {
        Some(existing) => {
            let not_null = existing.not_null || column.not_null;
            *existing = column;
            existing.not_null = not_null;
        }
        None => columns.push(column),
    }
}

fn mark_not_null(columns: &mut [RawColumn], expr: &Expr, dialect: SqlDialect) {
    if let Expr::Identifier(ident) = expr {
        let name = fold(ident, dialect);
        if let Some(column) = columns.iter_mut().find(|c| c.name == name) {
            column.not_null = true;
        }
    }
}

/// Apply one `ALTER TABLE` operation to `table`. `strict` fails on a missing column
/// (the table the statement names); the tables below it skip what does not apply.
fn alter_op(
    table: &mut RawTable,
    op: &AlterTableOperation,
    strict: bool,
    rename: &mut Option<ObjectName>,
    dialect: SqlDialect,
) -> Result<(), AlterFail> {
    let missing = || {
        if strict {
            Err(AlterFail::Missing)
        } else {
            Ok(())
        }
    };
    let position = |table: &RawTable, name: &Ident| {
        let name = fold(name, dialect);
        table.columns.iter().position(|c| c.name == name)
    };
    match op {
        AlterTableOperation::AddColumn { column_def, .. } => {
            if column_def.data_type == DataType::Unspecified {
                return Err(AlterFail::Unsupported);
            }
            if position(table, &column_def.name).is_none() {
                table.columns.push(raw_column(column_def, dialect));
            }
        }
        AlterTableOperation::DropColumn {
            column_names,
            if_exists,
            ..
        } => {
            for name in column_names {
                match position(table, name) {
                    Some(at) => {
                        table.columns.remove(at);
                    }
                    None if !if_exists => missing()?,
                    None => {}
                }
            }
        }
        AlterTableOperation::RenameColumn {
            old_column_name,
            new_column_name,
        } => match position(table, old_column_name) {
            Some(at) => {
                let column = &mut table.columns[at];
                column.name = fold(new_column_name, dialect);
                column.ident = match new_column_name.quote_style {
                    Some(_) => new_column_name.to_string(),
                    None => column.name.clone(),
                };
            }
            None => missing()?,
        },
        AlterTableOperation::RenameTable { table_name } => {
            if strict {
                let (RenameTableNameKind::As(name) | RenameTableNameKind::To(name)) = table_name;
                *rename = Some(name.clone());
            }
        }
        AlterTableOperation::AlterColumn { column_name, op } => {
            let Some(at) = position(table, column_name) else {
                return missing();
            };
            match op {
                AlterColumnOperation::SetNotNull => table.columns[at].not_null = true,
                AlterColumnOperation::DropNotNull => table.columns[at].not_null = false,
                AlterColumnOperation::SetDataType { data_type, .. } => {
                    table.columns[at].ty = data_type.clone();
                }
                AlterColumnOperation::SetDefault { .. }
                | AlterColumnOperation::DropDefault
                | AlterColumnOperation::AddGenerated { .. } => {}
            }
        }
        AlterTableOperation::AddConstraint {
            constraint: TableConstraint::PrimaryKey(pk),
            ..
        } => {
            for index_column in &pk.columns {
                mark_not_null(&mut table.columns, &index_column.column.expr, dialect);
            }
        }
        AlterTableOperation::ChangeColumn { .. }
        | AlterTableOperation::ModifyColumn { .. }
        | AlterTableOperation::SwapWith { .. } => return Err(AlterFail::Unsupported),
        // Storage, options, tablespace, triggers, RLS, partitions, comments and the like
        // never change a column.
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Reading statements
// ---------------------------------------------------------------------------------------

/// Whether an `ALTER TABLE` that sqlparser cannot read may change the table's columns,
/// by the words after the table name. Constraints, storage, ownership, triggers, RLS,
/// partitions, tablespaces and comments cannot.
fn changes_columns(action: &[String], mentions_type: bool) -> bool {
    let word = |i: usize| action.get(i).map_or("", String::as_str);
    match word(0) {
        "add" | "drop" => !matches!(
            word(1),
            "constraint" | "primary" | "unique" | "check" | "foreign" | "exclude"
        ),
        "rename" => word(1) != "constraint",
        "alter" => mentions_type,
        "set" => word(1) == "schema",
        "of" => true,
        _ => false,
    }
}

static COLLATE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"(?i)\s+COLLATE\s+(?:"[^"]*"|[\w.]+)"#).unwrap());

fn read_statements(
    statements: &[ddl::Statement],
    dialect: SqlDialect,
    loader: &mut Loader,
    skipped: &mut Vec<SkippedObject>,
) -> Vec<Op> {
    let postgres = dialect == SqlDialect::Postgres;
    let mut path = Rc::new(vec!["public".to_string()]);
    let mut ops = Vec::new();
    let parser_dialect = dialect.parser_dialect();
    let parse_part = |text: &str| Parser::new(parser_dialect.as_ref()).try_with_sql(text).ok();
    for stmt in statements {
        if postgres {
            if let Some(schemas) = stmt.search_path() {
                loader.search_path = schemas.clone();
                path = Rc::new(schemas);
                continue;
            }
        }
        let mut push = |kind: OpKind| {
            ops.push(Op {
                path: path.clone(),
                kind,
            })
        };
        if stmt.is_create_table() || (postgres && stmt.is_create_table_any()) {
            let (likes, text) = match postgres.then(|| stmt.without_like()).flatten() {
                Some((text, likes)) => (likes, Some(text)),
                None => (Vec::new(), None),
            };
            let parsed = match &text {
                None => parse_ddl(&stmt.sql(), dialect)
                    .or_else(|| parse_ddl(&stmt.without_constraints()?, dialect)),
                Some(text) => parse_ddl(text, dialect).or_else(|| {
                    let reduced = ddl::split_statements(text, false);
                    parse_ddl(&reduced.first()?.without_constraints()?, dialect)
                }),
            };
            match parsed.and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateTable(create)) => {
                    if !create.columns.is_empty() || postgres {
                        push(OpKind::Table(Box::new(create), likes));
                    }
                }
                Some(_) => {}
                None => skipped.push(("table", stmt.table_name())),
            }
        } else if stmt.creates("domain") {
            let parsed = stmt.domain().and_then(|(name, base)| {
                let name = parse_part(&name)?.parse_object_name(false).ok()?;
                let base = parse_part(&base)?.parse_data_type().ok()?;
                Some((name, base))
            });
            if let Some((name, base)) = parsed {
                push(OpKind::Domain(name, base));
            }
        } else if stmt.creates("type") {
            if let Some(Statement::CreateType {
                name,
                representation: Some(representation),
            }) = parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next())
            {
                let def = match representation {
                    UserDefinedTypeRepresentation::Enum { labels } => {
                        TypeDef::Enum(labels.into_iter().map(|label| label.value).collect())
                    }
                    UserDefinedTypeRepresentation::Composite { attributes } => TypeDef::Composite(
                        attributes
                            .into_iter()
                            .map(|attr| (fold(&attr.name, dialect), attr.data_type))
                            .collect(),
                    ),
                    _ => continue,
                };
                push(OpKind::Type(name, def));
            }
        } else if stmt.creates("view") {
            match parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateView(view)) => push(OpKind::View(Box::new(view))),
                _ => {
                    skipped.push(("view", stmt.view_name()));
                    if postgres {
                        push(OpKind::Taint(
                            Taint::Relation,
                            stmt.name_parts_after("view"),
                        ));
                    }
                }
            }
        } else if postgres && stmt.creates("function") {
            match parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateFunction(create)) => push(OpKind::Function(Box::new(create))),
                _ => push(OpKind::Taint(
                    Taint::Function,
                    stmt.name_parts_after("function"),
                )),
            }
        } else if postgres {
            read_alter_or_drop(stmt, dialect, &mut push);
        }
    }
    ops
}

fn read_alter_or_drop(stmt: &ddl::Statement, dialect: SqlDialect, push: &mut impl FnMut(OpKind)) {
    // sqlparser rejects `COLLATE` after an ALTER COLUMN type; the collation does not
    // change the type.
    let parsed = || {
        let sql = stmt.sql();
        parse_ddl(&sql, dialect)
            .or_else(|| parse_ddl(&COLLATE.replace_all(&sql, ""), dialect))
            .and_then(|stmts| stmts.into_iter().next())
    };
    if let Some((kind, name, action)) = stmt.target_and_action("alter") {
        match (kind.as_str(), parsed()) {
            ("table", Some(Statement::AlterTable(alter))) => {
                push(OpKind::AlterTable(Box::new(alter)))
            }
            ("table", _) if !changes_columns(&action, stmt.has_word("type")) => {}
            ("table", _) => push(OpKind::Taint(Taint::Relation, name)),
            ("type", Some(Statement::AlterType(alter))) => push(OpKind::AlterType(alter)),
            ("type", _) if action.first().is_some_and(|w| w == "owner") => {}
            ("type", _) => push(OpKind::Taint(Taint::Type, name)),
            ("view", _) if action.first().is_some_and(|w| w == "rename") => {
                push(OpKind::Taint(Taint::Relation, name));
            }
            ("function", _) if action.first().is_some_and(|w| w == "rename") => {
                push(OpKind::Taint(Taint::Function, name));
            }
            _ => {}
        }
    } else if let Some((kind, name)) = stmt.target("drop") {
        match parsed() {
            Some(
                drop @ (Statement::Drop { .. }
                | Statement::DropFunction(_)
                | Statement::DropDomain(_)),
            ) => {
                push(OpKind::Drop(Box::new(drop)));
            }
            _ if matches!(kind.as_str(), "table" | "view" | "materialized") => {
                push(OpKind::Taint(Taint::Relation, name));
            }
            _ if kind == "type" => push(OpKind::Taint(Taint::Type, name)),
            _ => {}
        }
    }
}

/// Relations PostgreSQL provides, the common columns of each.
const BUILTIN_SQL: &str = "
CREATE TABLE pg_catalog.pg_namespace (
    oid INTEGER NOT NULL, nspname TEXT NOT NULL, nspowner INTEGER NOT NULL, nspacl TEXT[]);
CREATE TABLE pg_catalog.pg_class (
    oid INTEGER NOT NULL, relname TEXT NOT NULL, relnamespace INTEGER NOT NULL,
    reltype INTEGER NOT NULL, reloftype INTEGER NOT NULL, relowner INTEGER NOT NULL,
    relam INTEGER NOT NULL, relfilenode INTEGER NOT NULL, reltablespace INTEGER NOT NULL,
    relpages INTEGER NOT NULL, reltuples REAL NOT NULL, relallvisible INTEGER NOT NULL,
    reltoastrelid INTEGER NOT NULL, relhasindex BOOLEAN NOT NULL, relisshared BOOLEAN NOT NULL,
    relpersistence TEXT NOT NULL, relkind TEXT NOT NULL, relnatts SMALLINT NOT NULL,
    relchecks SMALLINT NOT NULL, relhasrules BOOLEAN NOT NULL, relhastriggers BOOLEAN NOT NULL,
    relhassubclass BOOLEAN NOT NULL, relrowsecurity BOOLEAN NOT NULL,
    relispopulated BOOLEAN NOT NULL, relispartition BOOLEAN NOT NULL,
    relrewrite INTEGER NOT NULL, relacl TEXT[], reloptions TEXT[]);
CREATE TABLE pg_catalog.pg_attribute (
    attrelid INTEGER NOT NULL, attname TEXT NOT NULL, atttypid INTEGER NOT NULL,
    attlen SMALLINT NOT NULL, attnum SMALLINT NOT NULL, atttypmod INTEGER NOT NULL,
    attndims SMALLINT NOT NULL, attnotnull BOOLEAN NOT NULL, atthasdef BOOLEAN NOT NULL,
    attidentity TEXT NOT NULL, attgenerated TEXT NOT NULL, attisdropped BOOLEAN NOT NULL,
    attislocal BOOLEAN NOT NULL, attinhcount SMALLINT NOT NULL, attcollation INTEGER NOT NULL);
CREATE TABLE pg_catalog.pg_type (
    oid INTEGER NOT NULL, typname TEXT NOT NULL, typnamespace INTEGER NOT NULL,
    typowner INTEGER NOT NULL, typlen SMALLINT NOT NULL, typbyval BOOLEAN NOT NULL,
    typtype TEXT NOT NULL, typcategory TEXT NOT NULL, typispreferred BOOLEAN NOT NULL,
    typisdefined BOOLEAN NOT NULL, typdelim TEXT NOT NULL, typrelid INTEGER NOT NULL,
    typelem INTEGER NOT NULL, typarray INTEGER NOT NULL, typnotnull BOOLEAN NOT NULL,
    typbasetype INTEGER NOT NULL, typtypmod INTEGER NOT NULL, typndims INTEGER NOT NULL,
    typcollation INTEGER NOT NULL, typdefault TEXT);
CREATE TABLE information_schema.tables (
    table_catalog TEXT, table_schema TEXT, table_name TEXT, table_type TEXT,
    self_referencing_column_name TEXT, reference_generation TEXT,
    user_defined_type_catalog TEXT, user_defined_type_schema TEXT, user_defined_type_name TEXT,
    is_insertable_into TEXT, is_typed TEXT, commit_action TEXT);
CREATE TABLE information_schema.columns (
    table_catalog TEXT, table_schema TEXT, table_name TEXT, column_name TEXT,
    ordinal_position INTEGER, column_default TEXT, is_nullable TEXT, data_type TEXT,
    character_maximum_length INTEGER, character_octet_length INTEGER,
    numeric_precision INTEGER, numeric_precision_radix INTEGER, numeric_scale INTEGER,
    datetime_precision INTEGER, interval_type TEXT, udt_catalog TEXT, udt_schema TEXT,
    udt_name TEXT, is_identity TEXT, identity_generation TEXT, is_generated TEXT,
    generation_expression TEXT, is_updatable TEXT);
";

/// Read schema DDL into a [`Catalog`], and the `(kind, name)` of the tables and views
/// that could not be parsed. In PostgreSQL the catalog also holds the built-in
/// relations and follows `ALTER`, `DROP`, `search_path` and functions; other dialects
/// read `CREATE TABLE`, `VIEW`, `DOMAIN` and `TYPE` only.
pub fn load(schema_sql: &str, dialect: &str) -> (Catalog, Vec<SkippedObject>) {
    let dialect = SqlDialect::from_name(dialect);
    let mut loader = Loader::new(dialect);
    let mut skipped = Vec::new();
    if dialect == SqlDialect::Postgres {
        let builtin = ddl::split_statements(BUILTIN_SQL, false);
        let ops = read_statements(&builtin, dialect, &mut loader, &mut Vec::new());
        loader.run(ops);
        loader.search_path = vec!["public".to_string()];
    }
    let statements = ddl::split_statements(schema_sql, dialect == SqlDialect::MySql);
    let ops = read_statements(&statements, dialect, &mut loader, &mut skipped);
    loader.run(ops);
    skipped.append(&mut loader.skipped);
    let mut catalog = loader.build();
    if catalog.search_path.is_empty() {
        catalog.search_path.push("public".to_string());
    }
    (catalog, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg(sql: &str) -> Catalog {
        load(sql, "postgres").0
    }

    fn cols(catalog: &Catalog, table: &str) -> Vec<String> {
        catalog
            .table(table)
            .unwrap_or_else(|| panic!("no table {table}"))
            .iter()
            .map(|c| format!("{}:{}", c.name, c.sql_type))
            .collect()
    }

    #[test]
    fn test_alter_table_columns() {
        let c = pg("CREATE TABLE t (a int, b text, c int);
            ALTER TABLE t ADD COLUMN d bool NOT NULL;
            ALTER TABLE t DROP COLUMN b;
            ALTER TABLE t RENAME COLUMN c TO cc;
            ALTER TABLE t ALTER COLUMN a TYPE bigint;
            ALTER TABLE t ALTER COLUMN a SET NOT NULL;");
        assert_eq!(cols(&c, "t"), ["a:BIGINT", "cc:INT", "d:BOOLEAN"]);
        let t = c.table("t").unwrap();
        assert!(t[0].not_null && !t[1].not_null && t[2].not_null);
    }

    #[test]
    fn test_order_independent() {
        let c = pg("ALTER TABLE t ADD COLUMN b text;
            CREATE VIEW v AS SELECT a, b FROM t;
            CREATE TABLE t (a int);");
        assert_eq!(cols(&c, "t"), ["a:INT", "b:TEXT"]);
        assert_eq!(c.view("v").unwrap().len(), 2);
    }

    #[test]
    fn test_search_path_and_qualified_names() {
        let c = pg("CREATE SCHEMA app;
            SET search_path TO app, public;
            CREATE TABLE users (id int);
            CREATE TABLE public.users (id text);");
        assert_eq!(cols(&c, "users"), ["id:INT"]);
        assert_eq!(cols(&c, "public.users"), ["id:TEXT"]);
        assert_eq!(cols(&c, "app.users"), ["id:INT"]);
    }

    #[test]
    fn test_ambiguous_bare_name_is_unknown() {
        let c = pg("CREATE TABLE a.t (x int); CREATE TABLE b.t (x text);");
        assert!(c.table("t").is_none());
        assert_eq!(cols(&c, "a.t"), ["x:INT"]);
    }

    #[test]
    fn test_inherits_partition_and_like() {
        let c = pg("CREATE TABLE child (extra int) INHERITS (base);
            CREATE TABLE base (id int NOT NULL);
            CREATE TABLE part PARTITION OF parent FOR VALUES FROM (1) TO (10);
            CREATE TABLE parent (k int) PARTITION BY RANGE (k);
            CREATE TABLE copy (LIKE base INCLUDING ALL, more text);
            ALTER TABLE parent ADD COLUMN v text;");
        assert_eq!(cols(&c, "child"), ["id:INT", "extra:INT"]);
        assert_eq!(cols(&c, "part"), ["k:INT", "v:TEXT"]);
        assert_eq!(cols(&c, "copy"), ["id:INT", "more:TEXT"]);
        assert!(c.table("copy").unwrap()[0].not_null);
    }

    #[test]
    fn test_unknown_like_source_leaves_unknown_shape() {
        let c = pg("CREATE TABLE copy (LIKE missing);");
        assert!(c.table("copy").is_none());
        assert!(c.find("copy").is_some());
    }

    #[test]
    fn test_unparsable_alter_leaves_unknown_shape() {
        let c = pg("CREATE TABLE t (a int);
            ALTER TABLE t SET SCHEMA other;
            CREATE TABLE u (a int);
            ALTER TABLE u OWNER TO me;");
        assert!(c.table("t").is_none());
        assert_eq!(cols(&c, "u"), ["a:INT"]);
    }

    #[test]
    fn test_enum_alter_and_allowed_values() {
        let c = pg("CREATE TABLE t (m mood);
            ALTER TYPE mood ADD VALUE 'meh' BEFORE 'sad';
            CREATE TYPE mood AS ENUM ('happy', 'sad');");
        let column = &c.table("t").unwrap()[0];
        assert_eq!(
            column.allowed_values.as_deref().unwrap(),
            ["happy", "meh", "sad"]
        );
        assert_eq!(
            column.python_type.as_deref(),
            Some("typing.Literal['happy', 'meh', 'sad']")
        );
    }

    #[test]
    fn test_drop() {
        let c = pg(
            "CREATE TABLE a (x int); CREATE TABLE b (x int); CREATE VIEW v AS SELECT x FROM b;
            DROP TABLE a; DROP VIEW v; DROP TABLE IF EXISTS missing;
            CREATE TYPE e AS ENUM ('x'); DROP TYPE e;",
        );
        assert!(c.table("a").is_none() && c.view("v").is_none());
        assert!(c.table("b").is_some() && c.user_type("e").is_none());
    }

    #[test]
    fn test_drop_then_create_keeps_the_new_table() {
        let c = pg("DROP TABLE t; CREATE TABLE t (a int);");
        assert_eq!(cols(&c, "t"), ["a:INT"]);
    }

    #[test]
    fn test_functions() {
        let c = pg("CREATE FUNCTION one() RETURNS int LANGUAGE sql AS 'select 1';
            CREATE FUNCTION many(a int, b text DEFAULT 'x') RETURNS SETOF text LANGUAGE sql AS 'select 1';
            CREATE FUNCTION rows(a int) RETURNS TABLE(id int, name text) LANGUAGE sql AS 'select 1, 2';
            CREATE TABLE t (id int);
            CREATE FUNCTION of_t() RETURNS SETOF t LANGUAGE sql AS 'select 1';");
        assert!(
            matches!(c.function("one", 0), Some(FunctionReturn::Scalar((Some(p), _))) if p == "int")
        );
        assert!(c.function("one", 1).is_none());
        assert!(
            matches!(c.function("many", 1), Some(FunctionReturn::SetOf((Some(p), _))) if p == "str")
        );
        assert!(matches!(c.function("rows", 1), Some(FunctionReturn::Rows(r)) if r.len() == 2));
        assert!(matches!(c.function("of_t", 0), Some(FunctionReturn::Rows(r)) if r.len() == 1));
    }

    #[test]
    fn test_redefined_function_with_unreadable_body_is_unknown() {
        let c = pg("CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
            CREATE OR REPLACE FUNCTION f() RETURNS int LANGUAGE sql AS $$ select 1 $$ SECURITY NONSENSE;");
        assert!(c.function("f", 0).is_none());
    }

    #[test]
    fn test_builtin_relations() {
        let c = pg("");
        assert!(c.table("pg_class").is_some());
        assert!(c.table("pg_catalog.pg_namespace").is_some());
        assert!(c.table("information_schema.columns").is_some());
        assert!(c.table("columns").is_none());
        assert!(c.is_empty());
    }

    #[test]
    fn test_defined_names_and_unmodelled_system_relations() {
        let c = pg(
            "CREATE TABLE pg_orders (id int); CREATE TYPE mood AS ENUM ('a'); \
                    CREATE TYPE pt AS (x int); CREATE DOMAIN pct AS int; \
                    CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'SELECT 1'; \
                    CREATE FUNCTION g() LANGUAGE sql AS 'SELECT 1';",
        );
        assert!(["mood", "pt", "pct"].iter().all(|t| c.defines_type(t)));
        assert!(!c.defines_type("nope"));
        assert!(c.defines_function("f") && c.defines_function("g"));
        assert!(!c.defines_function("h"));
        assert!(c.defines("pg_orders") && !c.is_unmodelled_system_relation("pg_orders"));
        assert!(!c.is_unmodelled_system_relation("pg_class"));
        assert!(c.is_unmodelled_system_relation("pg_index"));
        assert!(c.is_unmodelled_system_relation("information_schema.views"));
        assert!(!c.is_unmodelled_system_relation("users"));
    }

    #[test]
    fn test_primary_key_and_serial_are_not_null() {
        let c =
            pg("CREATE TABLE t (a int PRIMARY KEY, b serial, c int, d int, PRIMARY KEY (a, d));");
        let t = c.table("t").unwrap();
        assert_eq!(
            t.iter().map(|c| c.not_null).collect::<Vec<_>>(),
            [true, true, false, true]
        );
    }

    #[test]
    fn test_other_dialects_ignore_postgres_statements() {
        let (c, _) = load(
            "CREATE TABLE db.t (a INT); ALTER TABLE t ADD COLUMN b INT; DROP TABLE t;",
            "mysql",
        );
        assert_eq!(c.table("t").map(Vec::len), Some(1));
        assert!(c.table("pg_class").is_none());
    }

    fn assert_fast(sql: &str) -> Catalog {
        let start = std::time::Instant::now();
        let catalog = pg(sql);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "load took {:?}",
            start.elapsed()
        );
        catalog
    }

    #[test]
    fn test_statements_that_never_resolve_load_fast() {
        let mut sql = String::new();
        for i in 0..2000 {
            sql += &format!("CREATE TABLE t{i} (id int);\n");
            sql += &format!("ALTER TABLE ext{i} ADD COLUMN x int;\n");
        }
        let catalog = assert_fast(&sql);
        assert_eq!(cols(&catalog, "t1999"), ["id:INT"]);
    }

    #[test]
    fn test_many_views_over_altered_tables_load_fast() {
        let mut sql = String::new();
        for i in 0..2000 {
            sql += &format!("CREATE TABLE t{i} (id int);\n");
            sql += &format!("ALTER TABLE t{i} ADD COLUMN c int;\n");
            sql += &format!("CREATE VIEW v{i} AS SELECT * FROM t{i};\n");
        }
        let catalog = assert_fast(&sql);
        assert_eq!(catalog.view("v1999").unwrap().len(), 2);
    }

    #[test]
    fn test_view_chain_loads_fast() {
        let mut sql = String::from("CREATE TABLE v0 (id int);\n");
        for i in 1..1500 {
            sql += &format!("CREATE VIEW v{i} AS SELECT id FROM v{};\n", i - 1);
        }
        let catalog = assert_fast(&sql);
        assert_eq!(catalog.view("v1499").unwrap().len(), 1);
    }

    #[test]
    fn test_degraded_objects_are_reported() {
        let (c, skipped) = load(
            "CREATE TABLE a (x int); ALTER TABLE a SET SCHEMA other;
             CREATE TABLE b (x int); ALTER TABLE b SET (fillfactor = 70);
             CREATE TABLE c (LIKE nothing);
             CREATE TYPE e AS ENUM ('x'); ALTER TYPE e SET SCHEMA other;",
            "postgres",
        );
        assert!(c.table("a").is_none() && c.table("b").is_some());
        let names: Vec<_> = skipped
            .iter()
            .map(|(kind, name)| format!("{kind} {name}"))
            .collect();
        assert_eq!(names, ["table a", "type e", "table c"]);
    }

    #[test]
    fn test_if_not_exists_keeps_the_existing_relation() {
        let c = pg("CREATE TABLE t (a int);
            CREATE TABLE IF NOT EXISTS t (a text, b text);
            CREATE VIEW v AS SELECT a FROM t;
            CREATE VIEW IF NOT EXISTS v AS SELECT a, a AS b FROM t;
            CREATE TABLE IF NOT EXISTS fresh (c int);");
        assert_eq!(cols(&c, "t"), ["a:INT"]);
        assert_eq!(c.view("v").unwrap().len(), 1);
        assert_eq!(cols(&c, "fresh"), ["c:INT"]);
    }

    #[test]
    fn test_renamed_enum_keeps_its_columns_literal() {
        let c = pg("CREATE TYPE mood AS ENUM ('a', 'b');
            CREATE TABLE t (m mood);
            ALTER TYPE mood RENAME TO feel;
            ALTER TYPE feel ADD VALUE 'c';
            CREATE TABLE u (f feel, m2 mood);");
        let t = c.table("t").unwrap();
        assert_eq!(t[0].sql_type, "FEEL");
        assert_eq!(
            t[0].python_type.as_deref(),
            Some("typing.Literal['a', 'b', 'c']")
        );
        assert_eq!(c.table("u").unwrap()[0].sql_type, "FEEL");
    }
}
