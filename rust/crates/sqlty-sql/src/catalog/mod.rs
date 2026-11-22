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
//! case. A table the schema changes in a way this module cannot follow (unparsable
//! `ALTER TABLE`, an unknown `LIKE` source) has an unknown shape: it resolves to nothing,
//! so no column of it gets a type.
//!
//! Submodules:
//!
//! - `names`: identifier folding and lookup keys
//! - `columns`: raw columns, `CHECK` and nullability handling
//! - `loader`: the operation queue, tables, views, types, functions and `DROP`
//! - `alter`: `ALTER TABLE` / `ALTER TYPE` semantics
//! - `resolve`: raw definitions into the resolved [`Catalog`]
//! - `index`: sorted names for prefix lookups ([`Catalog::relations_with_prefix`])
//! - `joins`: the tables a foreign key links, either way ([`Catalog::joins_with_prefix`])
//! - `read`: statements into operations, and [`load`]

mod alter;
mod columns;
mod index;
mod joins;
mod loader;
mod lookup;
mod names;
mod read;
mod resolve;

#[cfg(test)]
pub(crate) use index::visited;
pub use index::{Columns, NameRef, RelationKind, RelationRef};
pub use joins::Join;
pub(crate) use lookup::Lookup;
pub(crate) use names::fold;
pub(crate) use read::{load, load_with};

use rustc_hash::FxHashMap;
use std::sync::{Arc, LazyLock};

use sqlparser::ast::{DataType, Expr};

use crate::analyzer::{Column, SqlDialect};
use crate::ddl::RowEvents;

use index::NameIndex;
use names::lookup_key;
use resolve::Resolver;

/// `(python_type, sql_type)`; `None` is unknown.
pub(crate) type Typed = (Option<String>, Option<String>);

/// A column of a table or composite type.
#[derive(Clone, Debug)]
pub struct SchemaColumn {
    /// Lowercased name, for lookups and result column names.
    pub name: String,
    /// The name as SQL spells it, quotes kept, for `SELECT *` expansion. MySQL and SQLite
    /// keep the case of an unquoted name as declared, which their result columns show.
    pub ident: String,
    /// The canonical SQL type of the column.
    pub sql_type: String,
    /// The Python type when it is not the one `sql_type` maps to (an enum's `Literal`).
    pub python_type: Option<String>,
    /// Declared `NOT NULL`, `PRIMARY KEY` or an identity / `serial` column. Not used for
    /// inference yet.
    pub not_null: bool,
    /// Whether an INSERT that leaves the column out still gives it a value: a `DEFAULT`,
    /// `serial`, identity, generated or auto-increment column, or SQLite's rowid.
    pub has_default: bool,
    /// The `DEFAULT` an INSERT that leaves the column out writes.
    pub default: Option<Box<Expr>>,
    /// The expression a generated column is computed from, when it reads no other
    /// generated column.
    pub generated: Option<Box<Expr>>,
    /// Part of the table's primary key, which PostgreSQL lets a grouped query select
    /// the table's other columns by.
    pub primary_key: bool,
    /// Part of a foreign key the table declares.
    pub(crate) foreign_key: bool,
    /// The labels of the enum the column has, `None` for other types.
    pub allowed_values: Option<Arc<[String]>>,
    /// The values of a `Literal` `python_type`, as data.
    pub(crate) values: Option<Arc<[crate::values::Value]>>,
    /// MySQL's `INVISIBLE`: read only by name, left out of `SELECT *` and of an `INSERT`
    /// without a column list.
    pub invisible: bool,
}

/// A domain, enum or composite type as its columns see it: the SQL type they show (a
/// domain's base type, an enum's own name) and the Python type when that is a `Literal`
/// of the enum's labels.
#[derive(Clone, Debug)]
pub(crate) struct UserType {
    pub sql_type: String,
    pub python_type: Option<String>,
    /// An enum's labels.
    pub allowed_values: Option<Arc<[String]>>,
    /// The values of a `Literal` `python_type`, as data.
    pub values: Option<Arc<[crate::values::Value]>>,
}

/// What a function returns.
#[derive(Clone, Debug)]
pub(crate) enum FunctionReturn {
    /// One value of a type.
    Scalar(Typed),
    /// `SETOF type`: one value per row.
    SetOf(Typed),
    /// `RETURNS TABLE(...)`, `SETOF table` or several `OUT` parameters: rows of columns.
    Rows(Vec<SchemaColumn>),
    /// A table's row type: one value, or in `FROM` one row of the table's columns. Each is
    /// nullable, as a function that returns `NULL` gives a row of NULLs.
    Row(Typed, Vec<SchemaColumn>),
}

/// A function overload.
#[derive(Clone, Debug)]
pub(crate) struct Function {
    /// Arguments a call must give.
    pub required: usize,
    /// Arguments a call may give; `usize::MAX` for a `VARIADIC` function.
    pub max: usize,
    /// The types of the arguments before any `VARIADIC` one.
    pub args: Vec<Typed>,
    pub returns: FunctionReturn,
    /// `STRICT`: NULL exactly when an argument is. Any other function may return NULL.
    pub strict: bool,
}

/// A foreign key that holds for every row: `columns` of a table equal `referred` of
/// the table `table` (a catalog key), or one of them is NULL.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ForeignKey {
    pub columns: Vec<String>,
    pub table: String,
    pub referred: Vec<String>,
}

#[derive(Clone, Debug)]
struct TableRelation {
    columns: Vec<SchemaColumn>,
    /// Groups of columns a `CHECK` keeps at least one of non-null.
    nonnull_groups: Vec<Vec<String>>,
    /// The statements a `BEFORE` row trigger fires for.
    row_triggers: RowEvents,
    foreign_keys: Vec<ForeignKey>,
    /// Every foreign key declared, `NOT VALID`, deferrable and `NOT ENFORCED` ones included.
    declared_foreign_keys: Vec<ForeignKey>,
    /// Columns of each `UNIQUE` constraint or unique index.
    unique: Vec<Vec<String>>,
    row_security: bool,
    /// Positions in `columns`, by name and then position, so that a column is found in
    /// O(log width).
    by_name: Vec<u32>,
}

impl TableRelation {
    fn indexed(mut self) -> Self {
        let mut by_name: Vec<u32> = (0..self.columns.len() as u32).collect();
        by_name.sort_by(|&a, &b| {
            self.columns[a as usize]
                .name
                .cmp(&self.columns[b as usize].name)
        });
        self.by_name = by_name;
        self
    }

    fn column(&self, name: &str) -> Option<&SchemaColumn> {
        let at = self
            .by_name
            .partition_point(|&i| self.columns[i as usize].name.as_str() < name);
        let column = &self.columns[*self.by_name.get(at)? as usize];
        (column.name == name).then_some(column)
    }
}

#[derive(Clone, Debug)]
enum Relation {
    Table(TableRelation),
    View(Vec<Column>),
    /// Defined, but its shape is not known.
    Unknown,
}

/// The resolved schema: what the analyzer reads. Built by `load`; shared, so caching it
/// is cheap.
#[derive(Default)]
pub struct Catalog {
    /// `schema.name` -> relation.
    relations: FxHashMap<Arc<str>, Relation>,
    /// `schema.name` -> domain and enum types.
    types: FxHashMap<String, UserType>,
    /// `schema.name` -> composite type fields in declaration order.
    composites: FxHashMap<String, Vec<SchemaColumn>>,
    /// `schema.name` -> overloads; a function of unknown shape is absent.
    functions: FxHashMap<Arc<str>, Vec<Function>>,
    /// `schema.name` of the functions the schema defines but whose shape is unknown.
    unknown_functions: FxHashMap<Arc<str>, ()>,
    /// `schema.name` of every domain, enum and composite type the schema defines, those
    /// of unknown shape included.
    type_names: FxHashMap<Arc<str>, ()>,
    /// Schemas a bare name is looked up in, in order.
    search_path: Vec<String>,
    /// Relations the schema itself defines, built-ins not counted.
    user_relations: usize,
    postgres: bool,
    /// The Python types the project sets for SQL types and columns.
    overrides: Arc<TypeOverrides>,
    /// Sorted names for prefix lookups, sharing the keys above; built once by `load`.
    index: NameIndex,
    /// The named databases queries are routed to, when the schema was loaded with them.
    pub(crate) databases: Option<Arc<crate::analyzer::Databases>>,
}

/// Python types a project sets for SQL types or columns (`[tool.sqlty.types]`), for
/// values a converter above the driver changes: `DATE = "datetime.date"` when sqlite3
/// parses declared types, `"events.duration" = "datetime.timedelta"` for a duration
/// stored as an integer.
#[derive(Clone, Debug, Default)]
pub struct TypeOverrides {
    /// Canonical SQL type name without parameters (`DATE`, `DECIMAL`) -> Python type.
    types: FxHashMap<String, String>,
    /// `table.column`, folded -> Python type.
    columns: FxHashMap<String, String>,
}

impl TypeOverrides {
    /// Overrides from `(key, python type)` pairs: a key with a dot is `table.column`, any
    /// other a SQL type name, read in `dialect` (`NUMERIC` sets `DECIMAL`). An error names
    /// a key that is no type.
    pub fn new(entries: &[(String, String)], dialect: &str) -> Result<Self, String> {
        let dialect = SqlDialect::from_name(dialect);
        let mut overrides = TypeOverrides::default();
        for (key, python) in entries {
            if let Some((table, column)) = key.rsplit_once('.') {
                let table = table.rsplit('.').next().unwrap_or(table);
                let key = format!("{}.{}", table.to_lowercase(), column.to_lowercase());
                overrides.columns.insert(key, python.clone());
                continue;
            }
            let parsed = sqlparser::parser::Parser::new(dialect.parser_dialect().as_ref())
                .try_with_sql(&crate::analyzer::iso_types(key, dialect))
                .and_then(|mut parser| {
                    let parsed = parser.parse_data_type()?;
                    parser.expect_token(&sqlparser::tokenizer::Token::EOF)?;
                    Ok(parsed)
                })
                .map_err(|_| format!("`{key}` is not a SQL type"))?;
            let canonical = crate::analyzer::canonical_type(&parsed, dialect);
            overrides
                .types
                .insert(base_name(&canonical).to_string(), python.clone());
        }
        Ok(overrides)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.types.is_empty() && self.columns.is_empty()
    }

    /// The modules the Python types name, to import where a row type spells one:
    /// `app.models` for `app.models.Status`.
    fn modules(&self) -> Vec<String> {
        static DOTTED: LazyLock<regex::Regex> =
            LazyLock::new(|| crate::regex(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+"));
        let mut modules: Vec<String> = self
            .types
            .values()
            .chain(self.columns.values())
            .flat_map(|python| DOTTED.find_iter(python))
            .filter_map(|name| {
                name.as_str()
                    .rsplit_once('.')
                    .map(|(module, _)| module.to_string())
            })
            .collect();
        modules.sort();
        modules.dedup();
        modules
    }

    fn for_type(&self, sql_type: &str) -> Option<&str> {
        self.types.get(base_name(sql_type)).map(String::as_str)
    }

    fn for_column(&self, relation: &str, column: &str) -> Option<&str> {
        let table = relation.rsplit('.').next().unwrap_or(relation);
        self.columns
            .get(&format!("{table}.{column}"))
            .map(String::as_str)
    }
}

/// A canonical type's name without its parameters: `DECIMAL(10, 2)` is `DECIMAL`.
fn base_name(sql_type: &str) -> &str {
    sql_type.split(['(', '<']).next().unwrap_or_default().trim()
}

const BUILTIN_SCHEMAS: [&str; 2] = ["pg_catalog", "information_schema"];

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
        #[cfg(test)]
        index::visit();
        lookup_key(&self.relations, &self.search_path, self.norm(name))
            .and_then(|key| self.relations.get(key))
    }

    /// The statements a `BEFORE ... FOR EACH ROW` trigger of the table `name` fires for:
    /// such a trigger may change any column of the row a `RETURNING` clause returns.
    pub(crate) fn row_triggers(&self, name: &str) -> RowEvents {
        match self.find(name) {
            Some(Relation::Table(table)) => table.row_triggers,
            _ => RowEvents::default(),
        }
    }

    /// The catalog key of the table `name`, the same for every spelling of it.
    pub(crate) fn table_id(&self, name: &str) -> Option<&str> {
        self.table_entry(name).map(|(key, _)| key)
    }

    fn table_entry(&self, name: &str) -> Option<(&str, &TableRelation)> {
        let key = lookup_key(&self.relations, &self.search_path, self.norm(name))?;
        Some((key, self.table_at(key)?))
    }

    fn table_at(&self, key: &str) -> Option<&TableRelation> {
        match self.relations.get(key)? {
            Relation::Table(table) => Some(table),
            _ => None,
        }
    }

    /// The foreign keys of the table `name` that hold for every row and that a query sees
    /// hold: those referencing a table without row-level security.
    pub(crate) fn foreign_keys(&self, name: &str) -> impl Iterator<Item = &ForeignKey> {
        let keys = match self.find(name) {
            Some(Relation::Table(table)) => table.foreign_keys.as_slice(),
            _ => &[],
        };
        keys.iter().filter(|fk| {
            matches!(self.relations.get(fk.table.as_str()), Some(Relation::Table(target)) if !target.row_security)
        })
    }

    /// The catalog key of the table `name` and every foreign key it declares, whether or
    /// not it holds for every row, whose referenced table still has the referenced columns.
    pub(crate) fn declared_foreign_keys(&self, name: &str) -> Option<(&str, Vec<&ForeignKey>)> {
        let (key, table) = self.table_entry(name)?;
        let current = |fk: &&ForeignKey| {
            matches!(self.relations.get(fk.table.as_str()), Some(Relation::Table(target))
                if fk.referred.iter().all(|name| target.columns.iter().any(|c| c.name == *name)))
        };
        Some((
            key,
            table.declared_foreign_keys.iter().filter(current).collect(),
        ))
    }

    /// The keys of the table `name`: its primary key, then each `UNIQUE` constraint or
    /// unique index. Two rows never share a non-null value of a key. `None` when `name` is
    /// not a table whose shape is known.
    pub(crate) fn unique_keys(&self, name: &str) -> Option<Vec<Vec<&str>>> {
        let Some(Relation::Table(table)) = self.find(name) else {
            return None;
        };
        let primary: Vec<&str> = table
            .columns
            .iter()
            .filter(|column| column.primary_key)
            .map(|column| column.name.as_str())
            .collect();
        let unique = table
            .unique
            .iter()
            .map(|key| key.iter().map(String::as_str).collect());
        Some(
            (!primary.is_empty())
                .then_some(primary)
                .into_iter()
                .chain(unique)
                .collect(),
        )
    }

    /// The entity a column of the table `name` holds a key of: the primary or unique key
    /// column its foreign keys lead to, as `(table key, column)`. `None` when the column
    /// is no key and references none.
    pub(crate) fn entity(&self, name: &str, column: &str) -> Option<(&str, String)> {
        let mut table = self.table_id(name)?;
        let mut column = column.to_string();
        // A foreign key cycle has no root; stop after any chain a schema could hold.
        for _ in 0..32 {
            let next = self.table_at(table)?.foreign_keys.iter().find_map(|fk| {
                let at = fk.columns.iter().position(|c| *c == column)?;
                let (parent, _) = self.relations.get_key_value(fk.table.as_str())?;
                Some((&**parent, fk.referred.get(at)?.clone()))
            });
            match next {
                Some((parent, referred)) if (parent, &referred) != (table, &column) => {
                    (table, column) = (parent, referred);
                }
                _ => {
                    let keys = self.unique_keys(table)?;
                    let keyed = keys.iter().flatten().any(|c| *c == column);
                    return keyed.then_some((table, column));
                }
            }
        }
        None
    }

    /// Whether `name` is a PostgreSQL system relation (`pg_catalog.x`,
    /// `information_schema.x`, a bare `pg_*`) that the catalog does not model, so its
    /// columns are unknown. A relation the schema or the built-ins define is never one.
    pub(crate) fn is_unmodelled_system_relation(&self, name: &str) -> bool {
        self.postgres
            && self.find(name).is_none()
            && match name.split_once('.') {
                Some((schema, _)) => BUILTIN_SCHEMAS.contains(&schema),
                None => name.starts_with("pg_"),
            }
    }

    /// The domain or enum `name`; for a domain, its base type.
    pub(crate) fn user_type(&self, name: &str) -> Option<&UserType> {
        lookup_key(&self.types, &self.search_path, self.norm(name))
            .and_then(|key| self.types.get(key))
    }

    /// Fields of the composite type `name`.
    pub(crate) fn composite(&self, name: &str) -> Option<&[SchemaColumn]> {
        lookup_key(&self.composites, &self.search_path, self.norm(name))
            .and_then(|key| self.composites.get(key))
            .map(Vec::as_slice)
    }

    /// What the function `name` returns when called with `args` arguments, if exactly one
    /// overload (or only one return type) fits.
    pub(crate) fn function(&self, name: &str, args: usize) -> Option<&FunctionReturn> {
        let key = lookup_key(&self.functions, &self.search_path, self.norm(name))?;
        let mut fits = self.functions[key]
            .iter()
            .filter(|f| f.required <= args && args <= f.max);
        let first = fits.next()?;
        fits.all(|f| same_return(&f.returns, &first.returns))
            .then_some(&first.returns)
    }

    /// The argument types of the overloads of `name` a call with `args` arguments fits,
    /// when they agree: what a placeholder passed to it is bound as.
    pub(crate) fn function_args(&self, name: &str, args: usize) -> Option<&[Typed]> {
        let key = lookup_key(&self.functions, &self.search_path, self.norm(name))?;
        let mut fits = self.functions[key]
            .iter()
            .filter(|f| f.required <= args && args <= f.max);
        let first = fits.next()?;
        fits.all(|f| f.args == first.args)
            .then_some(first.args.as_slice())
    }

    /// Whether every overload of `name` a call with `args` arguments fits is `STRICT`;
    /// `None` when none fits.
    pub(crate) fn function_strict(&self, name: &str, args: usize) -> Option<bool> {
        let key = lookup_key(&self.functions, &self.search_path, self.norm(name))?;
        let mut fits = self.functions[key]
            .iter()
            .filter(|f| f.required <= args && args <= f.max)
            .peekable();
        fits.peek()?;
        Some(fits.all(|f| f.strict))
    }

    /// Whether the schema defines a function `name`, whatever is known of what it returns.
    pub(crate) fn defines_function(&self, name: &str) -> bool {
        let name = self.norm(name);
        lookup_key(&self.functions, &self.search_path, name).is_some()
            || lookup_key(&self.unknown_functions, &self.search_path, name).is_some()
    }

    /// Whether the schema defines a domain, enum or composite type `name`.
    pub(crate) fn defines_type(&self, name: &str) -> bool {
        lookup_key(&self.type_names, &self.search_path, self.norm(name)).is_some()
    }

    /// The modules the project's `[tool.sqlty.types]` name, beyond the standard ones a row
    /// type may need.
    pub fn type_modules(&self) -> Vec<String> {
        self.overrides.modules()
    }

    /// Whether the schema defines no table or view of its own.
    pub fn is_empty(&self) -> bool {
        self.user_relations == 0
    }

    /// Resolves declared types against this catalog's user types.
    fn resolver(&self, dialect: SqlDialect) -> Resolver<'_> {
        Resolver {
            types: &self.types,
            path: &self.search_path,
            dialect,
            overrides: &self.overrides,
        }
    }

    /// The SQL type of a column declared `dt`, and its Python type when a domain or enum
    /// sets it. Domains and enums resolve through the catalog.
    pub(crate) fn resolve_type(
        &self,
        dt: &DataType,
        dialect: SqlDialect,
    ) -> (String, Option<String>, Option<Arc<[crate::values::Value]>>) {
        let resolved = self.resolver(dialect).resolve(dt);
        (resolved.sql_type, resolved.python_type, resolved.values)
    }
}

fn same_return(a: &FunctionReturn, b: &FunctionReturn) -> bool {
    match (a, b) {
        (FunctionReturn::Scalar(a), FunctionReturn::Scalar(b))
        | (FunctionReturn::SetOf(a), FunctionReturn::SetOf(b))
        | (FunctionReturn::Row(a, _), FunctionReturn::Row(b, _)) => a == b,
        _ => false,
    }
}

/// An empty catalog, with the built-in relations in PostgreSQL: what queries read
/// when there is no schema.
pub(crate) fn empty(dialect: SqlDialect) -> &'static Catalog {
    static POSTGRES: LazyLock<Catalog> = LazyLock::new(|| load("", "postgres").0);
    static OTHER: LazyLock<Catalog> = LazyLock::new(Catalog::default);
    if dialect == SqlDialect::Postgres {
        &POSTGRES
    } else {
        &OTHER
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    impl Catalog {
        fn table(&self, name: &str) -> Option<&[SchemaColumn]> {
            Lookup::new(self).table(name)
        }

        fn view(&self, name: &str) -> Option<&[Column]> {
            Lookup::new(self).view(name)
        }
    }

    fn pg(sql: &str) -> Catalog {
        load(sql, "postgres").0
    }

    /// A statement on a table costs the same however much the table holds: copying or
    /// resolving all of it each time made `n` indexes, keys, triggers or added columns on
    /// one table take O(n²). Counts heap allocations, which a copy of the table makes in
    /// proportion to its size.
    #[test]
    fn statements_on_one_table_do_not_copy_it() {
        let head = "CREATE TABLE t (id int PRIMARY KEY, a int, b int);
            CREATE VIEW v AS SELECT id FROM t;\n";
        let each = |n: usize, statement: &dyn Fn(usize) -> String| -> String {
            (0..n).map(statement).collect()
        };
        let trigger =
            |i| format!("CREATE TRIGGER g{i} BEFORE INSERT ON t FOR EACH ROW SET NEW.a = 1;\n");
        let cases: [(&str, &dyn Fn(usize) -> String); 9] = [
            ("postgres", &|n| {
                each(n, &|i| format!("CREATE UNIQUE INDEX i{i} ON t (a, b);\n"))
            }),
            ("postgres", &|n| {
                each(n, &|i| format!("ALTER TABLE t ADD COLUMN c{i} int;\n"))
            }),
            ("postgres", &|n| {
                each(n, &|i| {
                    format!("ALTER TABLE t ADD CONSTRAINT u{i} UNIQUE (a, b);\n")
                })
            }),
            ("postgres", &|n| {
                each(n, &|i| {
                    format!(
                        "ALTER TABLE t ADD CONSTRAINT f{i} FOREIGN KEY (a) REFERENCES t (id);\n"
                    )
                })
            }),
            ("postgres", &|n| {
                each(n, &|i| {
                    format!("ALTER TABLE t ADD CONSTRAINT k{i} CHECK (a IS NOT NULL OR b IS NOT NULL);\n")
                })
            }),
            ("postgres", &|n| {
                each(n, &|i| {
                    format!(
                        "ALTER TABLE t ADD COLUMN c{i} int;
                        ALTER TABLE t ADD CONSTRAINT k{i} CHECK (t.c{i} IN (1, 2));
                        ALTER TABLE t ADD CONSTRAINT n{i} CHECK (c{i} > 0);\n"
                    )
                })
            }),
            ("postgres", &|n| {
                each(n, &trigger) + &each(n, &|i| format!("DROP TRIGGER g{i} ON t;\n"))
            }),
            ("mysql", &|n| {
                each(n, &|i| {
                    format!("ALTER TABLE t ADD COLUMN c{i} int AFTER id;\n")
                })
            }),
            ("mysql", &|n| {
                each(n, &trigger) + &each(n, &|i| format!("DROP TRIGGER g{i};\n"))
            }),
        ];
        for (dialect, statements) in cases {
            let allocations = |n| {
                let sql = format!("{head}{}", statements(n));
                crate::analyzer::allocations(|| drop(load(&sql, dialect)))
            };
            // The first load also reads what every load shares.
            allocations(0);
            let (base, small, large) = (allocations(0), allocations(100), allocations(400));
            // Linear: four times the statements, four times the work. A copy per statement
            // made it more than ten.
            assert!(
                (large - base) < 5 * (small - base),
                "{dialect} {:?}: {} then {} allocations",
                statements(1),
                small - base,
                large - base
            );
        }
    }

    /// A synthetic schema of `tables` tables, 12 columns each, a view over every third
    /// table, one enum and one domain. `head` goes first; `every_ten` follows each tenth table, with
    /// `{n}` replaced by the table number.
    fn synthetic(tables: usize, every_ten: &str, head: &str) -> String {
        let mut sql = String::from(
            "CREATE TYPE mood AS ENUM ('a', 'b', 'c');\nCREATE DOMAIN pct AS integer CHECK (VALUE >= 0);\n",
        );
        sql.push_str(head);
        for t in 0..tables {
            sql.push_str(&format!("CREATE TABLE t{t} (\n  id serial PRIMARY KEY,\n"));
            sql.push_str("  name text NOT NULL,\n  mood mood,\n  share pct,\n  born date,\n");
            sql.push_str("  seen timestamptz,\n  score numeric(10, 2),\n  tags text[],\n");
            sql.push_str("  meta jsonb,\n  active boolean NOT NULL DEFAULT true,\n");
            sql.push_str("  ref_id integer,\n  note varchar(40)\n);\n");
            if t % 3 == 0 {
                sql.push_str(&format!(
                    "CREATE VIEW v{t} AS SELECT id, name, mood, share FROM t{t};\n"
                ));
            }
            if t % 10 == 9 {
                sql.push_str(&every_ten.replace("{n}", &t.to_string()));
            }
        }
        sql
    }

    /// Timing of loading a 270-table schema. Run with
    /// `cargo test --release -p sqlty-sql -- --ignored --nocapture perf_`.
    #[test]
    #[ignore = "timing, run by hand in release mode"]
    fn perf_synthetic_schema_load() {
        let function =
            "CREATE FUNCTION f{n}(a integer) RETURNS integer LANGUAGE sql AS 'SELECT 1';\n";
        let cases = [
            ("no functions", synthetic(270, "", "")),
            ("function every 10 tables", synthetic(270, function, "")),
            (
                "plus one SETOF function",
                synthetic(
                    270,
                    function,
                    "CREATE FUNCTION rows() RETURNS SETOF integer LANGUAGE sql AS 'SELECT 1';\n",
                ),
            ),
        ];
        for (name, sql) in &cases {
            let mut best = std::time::Duration::MAX;
            let mut builds = 0;
            for _ in 0..15 {
                resolve::BUILDS.with(|count| count.set(0));
                let start = std::time::Instant::now();
                let catalog = pg(sql);
                best = best.min(start.elapsed());
                builds = resolve::BUILDS.with(std::cell::Cell::get);
                assert!(catalog.table("t0").is_some());
            }
            println!("perf {name:28} best of 15: {best:>10.2?}  build() calls: {builds}");
        }
    }

    fn view_cols(catalog: &Catalog, view: &str) -> Vec<String> {
        catalog
            .view(view)
            .unwrap_or_else(|| panic!("no view {view}"))
            .iter()
            .map(|c| format!("{}:{}", c.name, c.sql_type.as_deref().unwrap_or("?")))
            .collect()
    }

    /// A view after each function resolves only the functions that changed since the last
    /// one, not all of them.
    #[test]
    fn test_functions_between_views_resolve_once() {
        let mut sql = String::new();
        for i in 0..200 {
            sql += &format!("CREATE TABLE t{i} (id int);\n");
            sql +=
                &format!("CREATE FUNCTION f{i}() RETURNS SETOF t{i} LANGUAGE sql AS 'select 1';\n");
            sql += &format!("CREATE VIEW v{i} AS SELECT * FROM f{i}();\n");
        }
        let _ = pg("");
        resolve::FUNCTIONS_RESOLVED.with(|count| count.set(0));
        let _ = pg("");
        let builtin = resolve::FUNCTIONS_RESOLVED.with(|count| count.replace(0));
        let c = pg(&sql);
        assert_eq!(view_cols(&c, "v199"), ["id:INT"]);
        // Once before its view, once in the final build.
        assert_eq!(
            resolve::FUNCTIONS_RESOLVED.with(std::cell::Cell::get),
            2 * (200 + builtin)
        );
    }

    /// Functions do not force the catalog to be built again.
    #[test]
    fn test_functions_and_noop_alters_do_not_rebuild_the_catalog() {
        resolve::BUILDS.with(|count| count.set(0));
        let _ = pg("CREATE TABLE t (id int);
            CREATE VIEW v1 AS SELECT id FROM t;
            CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
            CREATE FUNCTION g() RETURNS SETOF t LANGUAGE sql AS 'select 1';
            ALTER TABLE t OWNER TO app;
            ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (id) REFERENCES t (id);
            CREATE VIEW v2 AS SELECT id FROM t;
            DROP FUNCTION f();");
        // One build for the first view, one at the end of the load.
        assert_eq!(resolve::BUILDS.with(std::cell::Cell::get), 2);
    }
}
