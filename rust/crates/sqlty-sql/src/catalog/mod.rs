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
//! - `read`: statements into operations, and [`load`]

mod alter;
mod columns;
mod loader;
mod names;
mod read;
mod resolve;

pub(crate) use names::fold;
pub(crate) use read::{load, load_with};

use rustc_hash::FxHashMap;
use std::sync::{Arc, LazyLock};

use sqlparser::ast::DataType;

use crate::analyzer::{Column, SqlDialect};

use names::lookup_key;
use resolve::Resolver;

/// `(python_type, sql_type)`; `None` is unknown.
pub(crate) type Typed = (Option<String>, Option<String>);

/// A column of a table or composite type.
#[derive(Clone, Debug)]
pub struct SchemaColumn {
    /// Lowercased name, for lookups and result column names.
    pub name: String,
    /// The name as SQL spells it, quotes kept, for `SELECT *` expansion.
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
    /// Part of the table's primary key, which PostgreSQL lets a grouped query select
    /// the table's other columns by.
    pub primary_key: bool,
    /// The labels of the enum the column has, `None` for other types.
    pub allowed_values: Option<Arc<[String]>>,
    /// The values of a `Literal` `python_type`, as data.
    pub(crate) values: Option<Arc<[crate::values::Value]>>,
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
}

#[derive(Clone, Debug)]
enum Relation {
    /// Columns, and the groups of them a `CHECK` keeps at least one of non-null.
    Table(Vec<SchemaColumn>, Vec<Vec<String>>),
    View(Vec<Column>),
    /// Defined, but its shape is not known.
    Unknown,
}

/// The resolved schema: what the analyzer reads. Built by `load`; shared, so caching it
/// is cheap.
#[derive(Default)]
pub struct Catalog {
    /// `schema.name` -> relation.
    relations: FxHashMap<String, Relation>,
    /// `schema.name` -> domain and enum types.
    types: FxHashMap<String, UserType>,
    /// `schema.name` -> composite type fields in declaration order.
    composites: FxHashMap<String, Vec<SchemaColumn>>,
    /// `schema.name` -> overloads; a function of unknown shape is absent.
    functions: FxHashMap<String, Vec<Function>>,
    /// `schema.name` of the functions the schema defines but whose shape is unknown.
    unknown_functions: FxHashMap<String, ()>,
    /// `schema.name` of every domain, enum and composite type the schema defines, those
    /// of unknown shape included.
    type_names: FxHashMap<String, ()>,
    /// Schemas a bare name is looked up in, in order.
    search_path: Vec<String>,
    /// Relations the schema itself defines, built-ins not counted.
    user_relations: usize,
    postgres: bool,
    /// The Python types the project sets for SQL types and columns.
    overrides: Arc<TypeOverrides>,
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
                .try_with_sql(key)
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
        static DOTTED: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+").expect("valid pattern")
        });
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
        lookup_key(&self.relations, &self.search_path, self.norm(name))
            .and_then(|key| self.relations.get(key))
    }

    /// Columns of the table `name`, or `None` for a view, an unknown name or a table of
    /// unknown shape.
    pub(crate) fn table(&self, name: &str) -> Option<&[SchemaColumn]> {
        match self.find(name)? {
            Relation::Table(columns, _) => Some(columns),
            _ => None,
        }
    }

    /// Groups of columns of the table `name` that a `CHECK` keeps at least one of
    /// non-null, such as `CHECK (num_nonnulls(a, b) = 1)`.
    pub(crate) fn nonnull_groups(&self, name: &str) -> &[Vec<String>] {
        match self.find(name) {
            Some(Relation::Table(_, groups)) => groups,
            _ => &[],
        }
    }

    /// Whether the schema defines a table or view `name`, whatever is known of its shape.
    pub(crate) fn defines(&self, name: &str) -> bool {
        self.find(name).is_some()
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

    /// Output columns of the view `name`.
    pub(crate) fn view(&self, name: &str) -> Option<&[Column]> {
        match self.find(name)? {
            Relation::View(columns) => Some(columns),
            _ => None,
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
    pub(crate) fn is_empty(&self) -> bool {
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
mod tests {
    use super::*;
    use crate::analyzer::canonical_type;
    use sqlparser::ast::Statement;
    use sqlparser::parser::Parser;

    #[test]
    fn module_example_alter_table_adds_a_column() {
        let (catalog, _) = load(
            "CREATE TABLE users (id int PRIMARY KEY, name text); \
             ALTER TABLE users ADD COLUMN age int NOT NULL;",
            "postgres",
        );
        let columns = catalog.table("users").unwrap();
        assert_eq!(columns.len(), 3);
        assert!(columns[0].not_null && !columns[1].not_null && columns[2].not_null);
    }

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
        assert!(c.is_unmodelled_system_relation("pg_trigger"));
        assert!(c.is_unmodelled_system_relation("information_schema.sequences"));
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
            "duckdb",
        );
        assert_eq!(c.table("t").map(<[SchemaColumn]>::len), Some(1));
        assert!(c.table("pg_class").is_none());
    }

    #[test]
    fn test_mysql_and_sqlite_follow_alter_and_drop() {
        for dialect in ["mysql", "sqlite"] {
            let (c, _) = load(
                "CREATE TABLE t (a INT); ALTER TABLE t ADD COLUMN b INT; \
                 CREATE TABLE u (a INT); DROP TABLE u;",
                dialect,
            );
            assert_eq!(
                c.table("t").map(<[SchemaColumn]>::len),
                Some(2),
                "{dialect}"
            );
            assert!(c.table("u").is_none(), "{dialect}");
        }
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

    /// A view read first caches the catalog; the functions after it must not each rebuild
    /// the function table. Time should grow about linearly with the count.
    #[test]
    #[ignore = "timing, run by hand in release mode"]
    fn perf_view_then_many_functions() {
        for n in [500, 1000, 2000, 4000] {
            let mut sql =
                String::from("CREATE TABLE t (id integer);\nCREATE VIEW v AS SELECT id FROM t;\n");
            for i in 0..n {
                sql.push_str(&format!(
                    "CREATE FUNCTION f{i}(a integer) RETURNS integer LANGUAGE sql AS 'SELECT 1';\n"
                ));
            }
            let start = std::time::Instant::now();
            let catalog = pg(&sql);
            let elapsed = start.elapsed();
            assert!(catalog.table("t").is_some());
            println!("perf view + {n:5} functions: {elapsed:>10.2?}");
        }
    }

    /// The common column types `canonical_type` answers without formatting the AST must
    /// spell exactly what the general path does, in every dialect.
    #[test]
    fn canonical_type_of_common_types_is_the_same_in_every_dialect() {
        use sqlparser::dialect::GenericDialect;
        let cases = [
            ("INTEGER", "INT"),
            ("INT", "INT"),
            ("INT4", "INT"),
            ("BIGINT", "BIGINT"),
            ("SMALLINT", "SMALLINT"),
            ("TEXT", "TEXT"),
            ("BOOLEAN", "BOOLEAN"),
            ("BOOL", "BOOLEAN"),
            ("UUID", "UUID"),
            ("DATE", "DATE"),
            ("BYTEA", "VARBINARY"),
            ("JSON", "JSON"),
            ("JSONB", "JSONB"),
            ("REAL", "FLOAT"),
            ("DOUBLE PRECISION", "DOUBLE"),
            ("VARCHAR", "VARCHAR"),
            ("CHAR", "CHAR"),
            ("VARCHAR(20)", "VARCHAR(20)"),
            ("TIMESTAMP", "TIMESTAMP"),
            ("TEXT[]", "ARRAY<TEXT>"),
        ];
        let dialects = [
            SqlDialect::Postgres,
            SqlDialect::MySql,
            SqlDialect::Sqlite,
            SqlDialect::ClickHouse,
            SqlDialect::Other("bigquery"),
        ];
        for (sql, expected) in cases {
            let parsed =
                Parser::parse_sql(&GenericDialect {}, &format!("CREATE TABLE t (a {sql})"))
                    .unwrap();
            let Statement::CreateTable(create) = &parsed[0] else {
                panic!("not a table: {sql}");
            };
            for (n, dialect) in dialects.into_iter().enumerate() {
                let got = canonical_type(&create.columns[0].data_type, dialect);
                let want = match (dialect, sql) {
                    (SqlDialect::MySql, "TIMESTAMP") => "TIMESTAMPTZ",
                    (SqlDialect::MySql, "REAL") => "DOUBLE",
                    _ => expected,
                };
                assert_eq!(got, want, "{sql} in dialect #{n}");
            }
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

    /// A view created after the catalog was cached sees what the statements before it
    /// did to functions, exactly as if the catalog had been built from scratch.
    #[test]
    fn test_views_see_functions_changed_after_the_cache_was_built() {
        let c = pg("CREATE TABLE t (id int);
            CREATE VIEW v1 AS SELECT id FROM t;
            CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
            CREATE VIEW v2 AS SELECT f() AS x;
            CREATE OR REPLACE FUNCTION f() RETURNS text LANGUAGE sql AS 'select 1';
            CREATE VIEW v3 AS SELECT f() AS x;
            DROP FUNCTION f();
            CREATE VIEW v4 AS SELECT f() AS x;");
        assert_eq!(view_cols(&c, "v2"), ["x:INT"]);
        assert_eq!(view_cols(&c, "v3"), ["x:TEXT"]);
        assert_eq!(view_cols(&c, "v4"), ["x:?"]);
    }

    /// `SETOF table` functions follow later changes to the table without a rebuild.
    #[test]
    fn test_setof_table_function_follows_alter_after_the_cache_was_built() {
        let c = pg("CREATE TABLE t (id int);
            CREATE FUNCTION rows() RETURNS SETOF t LANGUAGE sql AS 'select 1';
            CREATE VIEW v1 AS SELECT * FROM rows();
            ALTER TABLE t ADD COLUMN name text;
            CREATE VIEW v2 AS SELECT * FROM rows();
            ALTER TABLE t OWNER TO app;
            CREATE VIEW v3 AS SELECT * FROM rows();");
        assert_eq!(view_cols(&c, "v1"), ["id:INT"]);
        assert_eq!(view_cols(&c, "v2"), ["id:INT", "name:TEXT"]);
        assert_eq!(view_cols(&c, "v3"), ["id:INT", "name:TEXT"]);
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
