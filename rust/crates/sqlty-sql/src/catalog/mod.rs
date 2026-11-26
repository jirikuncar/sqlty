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
mod policies;
mod read;
mod resolve;

#[cfg(test)]
pub(crate) use index::visited;
pub use index::{Columns, NameRef, RelationKind, RelationRef};
pub use joins::Join;
pub(crate) use lookup::Lookup;
pub(crate) use names::{fold, folded, lowercase};
pub(crate) use read::{load, load_with};

use rustc_hash::FxHashMap;
use std::ops::Range;
use std::sync::{Arc, LazyLock, OnceLock};

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
    /// Its `CHECK` constraints that tie columns to a discriminator.
    checks: Vec<Expr>,
    /// The statements a `BEFORE` row trigger fires for.
    row_triggers: RowEvents,
    foreign_keys: Vec<ForeignKey>,
    /// Every foreign key declared, `NOT VALID`, deferrable and `NOT ENFORCED` ones included.
    declared_foreign_keys: Vec<ForeignKey>,
    /// Columns of each `UNIQUE` constraint or unique index.
    unique: Vec<Vec<String>>,
    row_security: bool,
    force_row_security: bool,
    /// The bytes of the latest statement that changed `row_security` or `force_row_security`.
    row_security_at: Range<usize>,
    policies: Vec<policies::Policy>,
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
/// is cheap. Each named database shares the data of a schema it loads with another.
#[derive(Clone, Default)]
pub struct Catalog {
    data: Arc<CatalogData>,
    /// The named databases queries are routed to, when the schema was loaded with them.
    pub(crate) databases: Option<Arc<crate::analyzer::Databases>>,
}

impl Catalog {
    /// This catalog, routing queries to `databases`.
    pub(crate) fn routed(&self, databases: crate::analyzer::Databases) -> Self {
        Self {
            data: Arc::clone(&self.data),
            databases: Some(Arc::new(databases)),
        }
    }

    /// Whether `other` shares this catalog's data.
    pub fn shares_data(&self, other: &Catalog) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }
}

impl std::ops::Deref for Catalog {
    type Target = CatalogData;

    fn deref(&self) -> &CatalogData {
        &self.data
    }
}

impl std::ops::DerefMut for Catalog {
    /// Unique while the loader builds it, so this never copies.
    fn deref_mut(&mut self) -> &mut CatalogData {
        Arc::make_mut(&mut self.data)
    }
}

/// What a [`Catalog`] reads, shared between the databases that load one schema.
#[derive(Clone, Default)]
pub struct CatalogData {
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
    /// The tenant keys of each table, built on first use; see [`Catalog::tenant_keys`].
    tenants: OnceLock<FxHashMap<Arc<str>, Vec<Vec<String>>>>,
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
    /// `table.column`, folded -> the deprecation `[tool.sqlty.deprecated-columns]` gives it.
    deprecated: FxHashMap<String, Deprecation>,
    /// The tenant table and the tenant keys `[tool.sqlty.tenancy]` excludes or adds.
    tenancy: Option<Tenancy>,
}

/// `[tool.sqlty.tenancy]`: the table whose rows are the tenants, as written (empty for
/// none), the `table.column` keys it excludes or adds, folded and without a schema, and
/// the functions whose result is the session's tenant, folded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tenancy {
    table: String,
    exclude: Vec<String>,
    columns: Vec<String>,
    session_functions: Vec<String>,
}

/// The key of the tenancy among the `types` pairs of [`TypeOverrides::new`]; the value is
/// `table\texclude\tcolumns\tsession-functions`, each list comma-separated.
const TENANCY: &str = "&tenancy";

/// The `types` pair [`TypeOverrides::new`] reads as `[tool.sqlty.tenancy]`: the tenant
/// table (`""` for none), the `table.column` tenant keys it excludes and adds, and the
/// functions a row-level security policy reads the session's tenant from.
///
/// ```
/// use sqlty_sql::analyzer::tenancy_type;
///
/// let pair = tenancy_type("orgs", &["audit_log.org_id"], &["events.org"], &["auth.uid"]);
/// assert_eq!(pair, ("&tenancy".to_string(), "orgs\taudit_log.org_id\tevents.org\tauth.uid".to_string()));
/// ```
pub fn tenancy_type(
    table: &str,
    exclude: &[&str],
    columns: &[&str],
    session_functions: &[&str],
) -> (String, String) {
    let value = format!(
        "{table}\t{}\t{}\t{}",
        exclude.join(","),
        columns.join(","),
        session_functions.join(",")
    );
    (TENANCY.to_string(), value)
}

/// `table.column` folded, without the table's schema.
fn column_key(key: &str) -> String {
    let (table, column) = key.rsplit_once('.').unwrap_or(("", key));
    format!("{}.{}", short_name(table), column.to_lowercase())
}

/// A table name folded, without its schema.
fn short_name(table: &str) -> String {
    table.rsplit('.').next().unwrap_or(table).to_lowercase()
}

/// A column scheduled for removal: what replaces it and when it goes, as configured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deprecation {
    /// The column to read instead.
    pub replacement: Option<String>,
    /// The date after which it is removed.
    pub remove_after: Option<String>,
}

/// The key prefix of a deprecated column among the `types` pairs of
/// [`TypeOverrides::new`]; the value is `replacement\tremove-after`, either part empty.
const DEPRECATED: &str = "!";

/// The `types` pair [`TypeOverrides::new`] reads as the deprecation of `table.column`: the
/// column to use instead and the date it is removed after, as
/// `[tool.sqlty.deprecated-columns]` gives them.
///
/// ```
/// use sqlty_sql::analyzer::deprecated_type;
///
/// let pair = deprecated_type("users.legacy_email", Some("email"), None);
/// assert_eq!(pair, ("!users.legacy_email".to_string(), "email\t".to_string()));
/// ```
pub fn deprecated_type(
    column: &str,
    replacement: Option<&str>,
    remove_after: Option<&str>,
) -> (String, String) {
    let value = format!(
        "{}\t{}",
        replacement.unwrap_or_default(),
        remove_after.unwrap_or_default()
    );
    (format!("{DEPRECATED}{column}"), value)
}

impl TypeOverrides {
    /// Overrides from `(key, python type)` pairs: a key with a dot is `table.column`, any
    /// other a SQL type name, read in `dialect` (`NUMERIC` sets `DECIMAL`). An error names
    /// a key that is no type.
    pub fn new(entries: &[(String, String)], dialect: &str) -> Result<Self, String> {
        let dialect = SqlDialect::from_name(dialect);
        let mut overrides = TypeOverrides::default();
        for (key, python) in entries {
            if key == TENANCY {
                let mut parts = python.split('\t');
                let table = parts.next().unwrap_or_default().to_lowercase();
                let mut list = || {
                    let part = parts.next().unwrap_or_default();
                    part.split(',')
                        .filter(|key| !key.is_empty())
                        .map(column_key)
                        .collect()
                };
                let (exclude, columns) = (list(), list());
                let functions = parts.next().unwrap_or_default().to_lowercase();
                overrides.tenancy = Some(Tenancy {
                    table,
                    exclude,
                    columns,
                    session_functions: functions
                        .split(',')
                        .filter(|name| !name.is_empty())
                        .map(String::from)
                        .collect(),
                });
                continue;
            }
            if let Some((table, column)) = key
                .strip_prefix(DEPRECATED)
                .and_then(|key| key.rsplit_once('.'))
            {
                let table = table.rsplit('.').next().unwrap_or(table);
                let key = format!("{}.{}", table.to_lowercase(), column.to_lowercase());
                let (replacement, remove_after) = python.split_once('\t').unwrap_or((python, ""));
                let given = |part: &str| (!part.is_empty()).then(|| part.to_string());
                let deprecation = Deprecation {
                    replacement: given(replacement),
                    remove_after: given(remove_after),
                };
                overrides.deprecated.insert(key, deprecation);
                continue;
            }
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
        self.types.is_empty()
            && self.columns.is_empty()
            && self.deprecated.is_empty()
            && self.tenancy.is_none()
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

    fn deprecation(&self, relation: &str, column: &str) -> Option<&Deprecation> {
        if self.deprecated.is_empty() {
            return None;
        }
        let table = relation.rsplit('.').next().unwrap_or(relation);
        self.deprecated.get(&format!("{table}.{column}"))
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

    /// The tenant keys of the table `name`: the primary key of the tenant table of
    /// `[tool.sqlty.tenancy]` and the columns of each foreign key to it, and each column a
    /// PostgreSQL policy pins to a session value, less the keys `exclude` names, plus each
    /// column of `columns`. Pinning every column of one key scopes a row. `None` for a table
    /// that has none, and for one whose forced policies pin one for every command, which
    /// the database scopes itself.
    pub(crate) fn tenant_keys(&self, name: &str) -> Option<&[Vec<String>]> {
        let key = self.table_id(name)?;
        self.tenants().get(key).map(Vec::as_slice)
    }

    fn tenants(&self) -> &FxHashMap<Arc<str>, Vec<Vec<String>>> {
        self.tenants.get_or_init(|| self.build_tenant_keys())
    }

    /// The tenant keys of every table, once per schema.
    fn build_tenant_keys(&self) -> FxHashMap<Arc<str>, Vec<Vec<String>>> {
        let mut found = FxHashMap::default();
        let none = Tenancy::default();
        let tenancy = self.overrides.tenancy.as_ref().unwrap_or(&none);
        let tenant = self.table_id(&tenancy.table);
        let functions = &tenancy.session_functions;
        for (key, relation) in &self.relations {
            let Relation::Table(table) = relation else {
                continue;
            };
            if tenant.is_none() && table.policies.is_empty() && tenancy.columns.is_empty() {
                continue;
            }
            let own = short_name(key);
            let listed = |list: &[String], column: &str| {
                let column = column.to_lowercase();
                list.iter()
                    .any(|listed| listed.split_once('.') == Some((own.as_str(), column.as_str())))
            };
            let primary: Vec<String> = table
                .columns
                .iter()
                .filter(|column| column.primary_key)
                .map(|column| column.name.clone())
                .collect();
            let referencing = table
                .declared_foreign_keys
                .iter()
                .filter(|fk| Some(fk.table.as_str()) == tenant)
                .map(|fk| fk.columns.clone());
            let is_tenant = Some(&**key) == tenant;
            // The tenant table's own keys to itself (`parent_id`) name another tenant.
            let referencing = referencing.filter(|_| !is_tenant);
            let own_key = (is_tenant && !primary.is_empty()).then_some(primary);
            let names: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
            let policed = (table.policies.iter())
                .flat_map(|policy| policy.session_columns(&names, functions))
                .map(|column| vec![column.clone()]);
            let mut keys: Vec<Vec<String>> = own_key
                .into_iter()
                .chain(referencing)
                .chain(policed)
                .filter(|columns| !columns.iter().any(|c| listed(&tenancy.exclude, c)))
                .collect();
            let added = table
                .columns
                .iter()
                .filter(|c| listed(&tenancy.columns, &c.name));
            keys.extend(added.map(|column| vec![column.name.clone()]));
            keys.sort();
            keys.dedup();
            let enforced = table.row_security
                && table.force_row_security
                && policies::enforce(&table.policies, &keys, functions);
            if !keys.is_empty() && !enforced {
                found.insert(Arc::clone(key), keys);
            }
        }
        found
    }

    /// Why each tenant table with row-level security is not scoped by the database, at the
    /// statement that leaves it so: `tenant-policy` findings, sorted by position. The table's
    /// queries are judged by `tenant-scope` meanwhile.
    pub fn tenant_policies(&self) -> Vec<crate::migrations::Finding> {
        use crate::migrations::Finding;
        let functions = self.overrides.tenancy.as_ref();
        let functions = functions.map_or(&[][..], |tenancy| &tenancy.session_functions);
        let mut found = Vec::new();
        for (key, relation) in &self.relations {
            let (Relation::Table(table), Some(keys)) = (relation, self.tenants().get(key)) else {
                continue;
            };
            if !table.row_security {
                continue;
            }
            let name = short_name(key);
            let finding = |span, message| Finding {
                rule: sqlty_rules::Rule::TenantPolicy,
                message,
                span,
            };
            if !table.force_row_security {
                found.push(finding(
                    table.row_security_at.clone(),
                    format!("`{name}` has row-level security without `FORCE ROW LEVEL SECURITY`, so its owner sees every tenant"),
                ));
            }
            let named: Vec<String> = keys
                .iter()
                .map(|key| format!("`{}`", key.join("`, `")))
                .collect();
            let named = named.join(" or ");
            let gaps = policies::gaps(&table.policies, keys, functions);
            let mut policies: Vec<&policies::Policy> = gaps.iter().map(|gap| gap.policy).collect();
            policies.sort_by_key(|policy| policy.at().start);
            policies.dedup_by(|a, b| std::ptr::eq(*a, *b));
            for policy in policies {
                let commands = |written: bool| {
                    let listed: Vec<String> = gaps
                        .iter()
                        .filter(|gap| std::ptr::eq(gap.policy, policy) && gap.written == written)
                        .map(|gap| gap.command.to_string())
                        .collect();
                    listed.join(", ")
                };
                let (read, written) = (commands(false), commands(true));
                let message = match (policy.name(), read.is_empty(), written.is_empty()) {
                    ("", ..) => format!("sqlty can't read this policy on `{name}`, so it counts as letting every tenant's rows through"),
                    (own, false, true) => format!(
                        "policy `{own}` on `{name}` does not filter {read} by {named}; compare it to the session in USING"
                    ),
                    (own, true, _) => format!(
                        "policy `{own}` on `{name}` does not check {named} on {written}; compare it to the session in WITH CHECK"
                    ),
                    (own, false, false) => {
                        let clauses = match policy.checks() {
                            true => "USING and WITH CHECK",
                            false => "USING",
                        };
                        format!("policy `{own}` on `{name}` does not filter {read} or check {written} by {named}; compare it to the session in {clauses}")
                    }
                };
                found.push(finding(policy.at(), message));
            }
        }
        found.sort_by_key(|finding| (finding.span.start, finding.message.clone()));
        found
    }

    /// Whether `name` is the tenant table of `[tool.sqlty.tenancy]`.
    pub(crate) fn is_tenant_table(&self, name: &str) -> bool {
        let tenancy = self.overrides.tenancy.as_ref();
        tenancy.is_some_and(|t| {
            self.table_id(name)
                .is_some_and(|key| Some(key) == self.table_id(&t.table))
        })
    }

    /// Whether a table has a tenant key, configured or read from its policies.
    pub(crate) fn has_tenancy(&self) -> bool {
        !self.tenants().is_empty()
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

    /// The deprecation of `column` of the table `relation` names, when it has one.
    pub(crate) fn deprecation(&self, relation: &str, column: &str) -> Option<&Deprecation> {
        self.overrides.deprecation(relation, column)
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

        fn column(&self, name: &str, column: &str) -> Option<Option<&SchemaColumn>> {
            Lookup::new(self).column(name, column)
        }

        fn view(&self, name: &str) -> Option<&[Column]> {
            Lookup::new(self).view(name)
        }

        fn defines(&self, name: &str) -> bool {
            Lookup::new(self).defines(name)
        }

        fn nonnull_groups(&self, name: &str) -> &[Vec<String>] {
            Lookup::new(self).nonnull_groups(name)
        }
    }
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

    #[test]
    fn a_column_is_found_by_name_in_a_wide_table() {
        let columns: Vec<String> = (0..300).rev().map(|i| format!("c{i} int")).collect();
        let c = pg(&format!(
            "CREATE TABLE t ({}); CREATE VIEW v AS SELECT 1 AS x;",
            columns.join(", ")
        ));
        let names: Vec<&str> = c
            .table("t")
            .unwrap()
            .iter()
            .map(|col| col.name.as_str())
            .collect();
        for name in names {
            assert_eq!(c.column("t", name).unwrap().unwrap().name, name);
        }
        assert!(c.column("t", "c300").unwrap().is_none());
        assert!(c.column("t", "").unwrap().is_none());
        assert!(c.column("v", "x").is_none());
        assert!(c.column("nope", "c1").is_none());
    }

    #[test]
    fn lookups_of_a_view_or_an_unknown_name_find_nothing() {
        let c = pg("CREATE TABLE t (a int PRIMARY KEY);
            CREATE VIEW v AS SELECT a FROM t;
            CREATE TABLE u (b int REFERENCES v (a));
            CREATE FUNCTION f(x int) RETURNS int LANGUAGE sql AS 'SELECT 1';
            CREATE FUNCTION f(x text) RETURNS SETOF int LANGUAGE sql AS 'SELECT 1';");
        assert!(c.nonnull_groups("v").is_empty());
        assert_eq!(c.foreign_keys("nope").count(), 0);
        assert_eq!(c.entity("u", "b"), None);
        assert!(c.function("f", 1).is_none());
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
    fn test_renamed_parent_still_alters_its_children() {
        let c = pg("CREATE TABLE base (id int);
            CREATE TABLE child (extra int) INHERITS (base);
            ALTER TABLE base RENAME TO root;
            ALTER TABLE root ADD COLUMN v text;");
        assert_eq!(cols(&c, "child"), ["id:INT", "extra:INT", "v:TEXT"]);
    }

    fn wide(name: &str, width: usize) -> String {
        let columns: Vec<String> = (0..width).map(|i| format!("c{i} int")).collect();
        format!("CREATE TABLE {name} ({});\n", columns.join(", "))
    }

    #[test]
    fn a_table_wider_than_max_columns_is_unknown() {
        let sql = wide("ok", crate::MAX_COLUMNS) + &wide("wide", crate::MAX_COLUMNS + 1);
        let (c, skipped) = load(&sql, "postgres");
        assert_eq!(c.table("ok").map(<[_]>::len), Some(crate::MAX_COLUMNS));
        assert!(c.table("wide").is_none() && c.find("wide").is_some());
        assert_eq!(skipped, [("limit", "public.wide".to_string())]);
    }

    #[test]
    fn relations_past_the_schema_columns_are_unknown() {
        // The built-in relations hold some columns too: the last copies may not fit.
        // The schema's own text allows fewer than 16 more.
        let copies = loader::MAX_SCHEMA_COLUMNS / crate::MAX_COLUMNS + 16;
        let mut sql = wide("big", crate::MAX_COLUMNS) + "CREATE SCHEMA s;\n";
        for i in 0..copies {
            sql += &format!("CREATE TABLE s.t{i} (LIKE big);\n");
        }
        sql += "CREATE TABLE copy (LIKE big);\nCREATE TABLE child () INHERITS (big);\n";
        sql += &wide("one", crate::MAX_COLUMNS);
        sql += "CREATE VIEW v AS SELECT * FROM big;
            DROP SCHEMA s CASCADE;
            CREATE TABLE two (a int);
            CREATE TABLE copy2 (LIKE big);";
        let (c, skipped) = load(&sql, "postgres");
        for name in ["copy", "child", "one"] {
            assert!(c.table(name).is_none() && c.find(name).is_some(), "{name}");
        }
        assert!(
            matches!(c.find("v"), Some(Relation::View(columns)) if columns.len() == 1 && columns[0].name == "*")
        );
        assert_eq!(cols(&c, "two"), ["a:INT"]);
        assert_eq!(c.table("copy2").map(<[_]>::len), Some(crate::MAX_COLUMNS));
        let (copied, rest) = skipped.split_at(skipped.len() - 4);
        assert!(copied.iter().all(|(_, name)| name.starts_with("s.t")));
        assert_eq!(
            rest,
            [
                ("limit", "copy".to_string()),
                ("limit", "child".to_string()),
                ("limit", "public.one".to_string()),
                ("limit", "public.v".to_string()),
            ]
        );
    }

    #[test]
    fn a_long_schema_may_hold_as_many_columns_as_it_spells_out() {
        let copies = loader::MAX_SCHEMA_COLUMNS / crate::MAX_COLUMNS + 16;
        let mut sql = wide("big", crate::MAX_COLUMNS);
        for i in 0..copies {
            sql += &format!("CREATE TABLE t{i} (LIKE big);\n");
        }
        let (c, _) = load(&sql, "postgres");
        assert!(c.table(&format!("t{}", copies - 1)).is_none());
        sql += &" ".repeat(2 * loader::MAX_SCHEMA_COLUMNS);
        let (c, skipped) = load(&sql, "postgres");
        assert!(c.table(&format!("t{}", copies - 1)).is_some() && skipped.is_empty());
    }

    #[test]
    fn a_view_past_the_limits_counts_as_it_is_held() {
        let copies = loader::MAX_SCHEMA_COLUMNS / crate::MAX_COLUMNS + 32;
        let mut sql = wide("big", crate::MAX_COLUMNS);
        for i in 0..copies {
            sql += &format!("CREATE VIEW f{i} AS SELECT * FROM big;\n");
        }
        sql += "CREATE VIEW limited AS SELECT * FROM big;\n";
        for i in 0..copies {
            sql += &format!("DROP VIEW f{i};\n");
        }
        // An unknown table holds no columns: renamed over the view, it frees the view's one.
        sql += "DROP TABLE big;\n";
        sql += &wide("e", crate::MAX_COLUMNS + 1);
        sql += "ALTER TABLE e RENAME TO limited;";
        let (c, skipped) = load(&sql, "sqlite");
        assert!(skipped.contains(&("limit", "public.limited".to_string())));
        assert!(c.table("limited").is_none() && c.find("limited").is_some());
    }

    #[test]
    fn a_table_renamed_over_another_replaces_it() {
        let c = pg("CREATE TABLE a (x int); CREATE TABLE b (y int); ALTER TABLE a RENAME TO b;");
        assert_eq!(cols(&c, "b"), ["x:INT"]);
        assert!(c.find("a").is_none());
    }

    #[test]
    fn a_chain_written_backwards_stops_after_max_retried() {
        // Each pass creates one more table: n passes try about n * n / 2 statements of
        // some 30 bytes.
        let n = 2 * (loader::MAX_RETRIED as f64 / 16.0).sqrt() as usize;
        let mut sql: String = (0..n)
            .map(|i| format!("CREATE TABLE t{i} (LIKE t{});\n", i + 1))
            .collect();
        sql += &format!("CREATE TABLE t{n} (a int);");
        let (c, skipped) = load(&sql, "postgres");
        assert_eq!(cols(&c, &format!("t{}", n - 1)), ["a:INT"]);
        assert!(c.table("t0").is_none() && c.find("t0").is_some());
        assert!(skipped.contains(&("table", "t0".to_string())));
    }

    fn indexes(n: usize) -> String {
        (0..n)
            .map(|i| {
                format!(
                    "CREATE UNIQUE INDEX i{i} ON big (c{});\n",
                    i % crate::MAX_COLUMNS
                )
            })
            .collect()
    }

    /// The fewest writes of a [`crate::MAX_COLUMNS`] wide table that pass
    /// [`loader::MAX_WRITTEN`] and the allowance of the schema `sql` gives for that many.
    fn writes_past_the_limit(sql: impl Fn(usize) -> String) -> (usize, String) {
        (loader::MAX_WRITTEN / crate::MAX_COLUMNS..)
            .map(|n| (n, sql(n)))
            .find(|(n, sql)| n * crate::MAX_COLUMNS > loader::MAX_WRITTEN + sql.len() / 2)
            .unwrap()
    }

    #[test]
    fn loading_stops_after_max_written_columns() {
        let (_, sql) = writes_past_the_limit(|n| {
            "CREATE TABLE waits (LIKE later);\n".to_string()
                + &wide("big", crate::MAX_COLUMNS)
                + &indexes(n - 1)
                + "CREATE TABLE later (a int);\nCREATE TABLE ignored (a int);"
        });
        let (c, skipped) = load(&sql, "postgres");
        assert!(c.find("later").is_none() && c.find("waits").is_none());
        assert_eq!(
            skipped,
            [
                ("limit", "CREATE TABLE later (a int) and later".to_string()),
                ("limit", "1 statements waiting on others".to_string()),
            ]
        );
        // With nothing waiting, only what is left unread is named.
        let sql = sql.replacen("CREATE TABLE waits (LIKE later);\n", "", 1);
        assert_eq!(
            load(&sql, "postgres").1,
            [("limit", "CREATE TABLE later (a int) and later".to_string())]
        );
    }

    #[test]
    fn statements_waiting_on_others_stop_after_max_written_columns() {
        // Defined after the indexes on it, so they wait for it.
        let (_, sql) = writes_past_the_limit(|n| indexes(n) + &wide("big", crate::MAX_COLUMNS));
        let (c, skipped) = load(&sql, "postgres");
        assert_eq!(c.table("big").map(<[_]>::len), Some(crate::MAX_COLUMNS));
        assert_eq!(
            skipped,
            [("limit", "1 statements waiting on others".to_string())]
        );
        // Views that read a table never defined are read last, against what is there.
        let (_, sql) = writes_past_the_limit(|n| {
            wide("big", crate::MAX_COLUMNS)
                + &indexes(n - 2)
                + "CREATE VIEW v1 AS SELECT big.* FROM big, missing;
                   CREATE VIEW v2 AS SELECT big.* FROM big, missing;"
        });
        let (c, skipped) = load(&sql, "postgres");
        assert!(c.find("v1").is_some() && c.find("v2").is_none());
        assert_eq!(
            skipped,
            [("limit", "1 statements waiting on others".to_string())]
        );
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
    fn unique_keys_follow_constraints_indexes_and_drops() {
        let c = pg(
            "CREATE TABLE t (a int PRIMARY KEY, b int UNIQUE, c int, d int, e int, \
             CONSTRAINT t_cd UNIQUE (c, d)); \
             CREATE UNIQUE INDEX t_e ON t (e); CREATE UNIQUE INDEX t_lower ON t (lower(e::text)); \
             CREATE UNIQUE INDEX t_partial ON t (c) WHERE d > 0; \
             ALTER TABLE t DROP CONSTRAINT t_cd; ALTER TABLE t ADD UNIQUE (d); \
             ALTER TABLE t RENAME COLUMN e TO f;",
        );
        assert_eq!(
            c.unique_keys("t").unwrap(),
            [vec!["a"], vec!["b"], vec!["d"]]
        );
        let c = pg("CREATE TABLE t (a int); CREATE UNIQUE INDEX t_a ON t (a); DROP INDEX t_a;");
        assert!(c.unique_keys("t").unwrap().is_empty());
        let (c, _) = load(
            "CREATE TABLE t (a TEXT, b TEXT, PRIMARY KEY (a, b));",
            "sqlite",
        );
        assert_eq!(c.unique_keys("t").unwrap(), [vec!["a", "b"]]);
        let (c, _) = load("CREATE TABLE t (a INT, UNIQUE KEY t_a (a));", "mysql");
        assert_eq!(c.unique_keys("t").unwrap(), [vec!["a"]]);
    }

    #[test]
    fn entities_follow_foreign_keys_to_a_key() {
        let c = pg("CREATE TABLE a (id int PRIMARY KEY, note int); \
             CREATE TABLE b (id int PRIMARY KEY REFERENCES a, a_id int REFERENCES a (id)); \
             CREATE TABLE c (b_id int REFERENCES b); \
             CREATE TABLE x (id int PRIMARY KEY, y_id int UNIQUE); \
             CREATE TABLE y (id int PRIMARY KEY, x_id int UNIQUE REFERENCES x (y_id)); \
             ALTER TABLE x ADD FOREIGN KEY (y_id) REFERENCES y (x_id);");
        let root = |table: &str, column: &str| c.entity(table, column);
        assert_eq!(root("c", "b_id"), Some(("public.a", "id".to_string())));
        assert_eq!(root("b", "a_id"), root("a", "id"));
        assert_eq!(root("a", "note"), None);
        assert_eq!(root("x", "y_id"), None, "a cycle has no root");
    }

    #[test]
    fn foreign_keys_reach_tables_defined_later() {
        let c = pg("SET search_path = app; \
             CREATE TABLE a (b_id int REFERENCES b, c_id int REFERENCES c (code), gone int REFERENCES gone); \
             CREATE TABLE app.b (id int PRIMARY KEY); \
             CREATE TABLE app.c (code text UNIQUE);");
        let keys: Vec<_> = c
            .foreign_keys("app.a")
            .map(|fk| {
                (
                    fk.columns.join(","),
                    fk.table.as_str(),
                    fk.referred.join(","),
                )
            })
            .collect();
        assert_eq!(
            keys,
            [
                ("b_id".to_string(), "app.b", "id".to_string()),
                ("c_id".to_string(), "app.c", "code".to_string()),
            ]
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

    /// Catches loading that grows quadratically, which takes far longer at these sizes; a
    /// debug build on a shared CI runner takes up to a second.
    fn assert_fast(sql: &str) -> Catalog {
        let start = std::time::Instant::now();
        let catalog = pg(sql);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
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

    /// The statements a table takes in place change it as a copy would.
    #[test]
    fn statements_on_one_table_change_it_as_before() {
        let c = pg("CREATE TABLE t (id int PRIMARY KEY, a int, b int);
            CREATE TABLE p (id int); CREATE TABLE ch () INHERITS (p);
            CREATE VIEW v0 AS SELECT * FROM t;
            CREATE VIEW w AS SELECT id FROM t;
            CREATE UNIQUE INDEX on_view ON w (id);
            CREATE TRIGGER on_view BEFORE INSERT ON w FOR EACH ROW EXECUTE FUNCTION f();
            ALTER TABLE t ADD COLUMN c int;
            ALTER TABLE t ADD COLUMN c text;
            ALTER TABLE t ADD COLUMN r int REFERENCES t;
            ALTER TABLE t ADD CONSTRAINT u UNIQUE (a, b);
            ALTER TABLE t ADD CONSTRAINT x UNIQUE (lower(b::text));
            ALTER TABLE t ADD CONSTRAINT f FOREIGN KEY (a) REFERENCES t (id);
            ALTER TABLE t ADD CONSTRAINT g FOREIGN KEY (b) REFERENCES missing (id);
            ALTER TABLE t ADD COLUMN m int, ADD CONSTRAINT h FOREIGN KEY (m) REFERENCES t (id);
            ALTER TABLE t ADD COLUMN n int, ADD CONSTRAINT i FOREIGN KEY (n) REFERENCES missing (id);
            ALTER TABLE t ADD CONSTRAINT k1 CHECK (b IS NOT NULL);
            ALTER TABLE t ADD CONSTRAINT k2 CHECK (a > 0);
            ALTER TABLE t ADD CONSTRAINT k3 CHECK (a IS NOT NULL OR c IS NOT NULL);
            CREATE TRIGGER g1 BEFORE INSERT ON t FOR EACH ROW EXECUTE FUNCTION f();
            DROP TRIGGER g1 ON t;
            DROP TRIGGER g1 ON t;
            ALTER TABLE p ADD COLUMN b int;
            ALTER TABLE ONLY p ADD COLUMN only_p int;
            ALTER TABLE w ADD COLUMN z int;
            DROP TABLE ch;
            CREATE VIEW v1 AS SELECT * FROM t;
            CREATE VIEW v2 AS SELECT * FROM p;");
        assert_eq!(
            cols(&c, "t"),
            ["id:INT", "a:INT", "b:INT", "c:INT", "r:INT", "m:INT", "n:INT"]
        );
        assert_eq!(c.view("v0").unwrap().len(), 3);
        assert_eq!(c.view("v1").unwrap().len(), 7);
        assert_eq!(c.unique_keys("t").unwrap(), [vec!["id"], vec!["a", "b"]]);
        let keys: Vec<_> = c.foreign_keys("t").map(|fk| fk.columns.clone()).collect();
        assert_eq!(keys, [vec!["r"], vec!["a"], vec!["m"]]);
        assert!(!c.row_triggers("t").insert);
        let not_null: Vec<_> = c.table("t").unwrap().iter().map(|c| c.not_null).collect();
        assert_eq!(not_null, [true, false, false, false, false, false, false]);
        assert_eq!(c.nonnull_groups("t"), [vec!["b"], vec!["c", "a"]]);
        assert_eq!(cols(&c, "p"), ["id:INT", "b:INT", "only_p:INT"]);
        assert_eq!(c.view("v2").unwrap().len(), 3);
        assert!(c.table("w").is_none() && c.find("ch").is_none());

        let (c, _) = load(
            "CREATE TABLE t (id INT, a INT);
            CREATE TRIGGER g1 BEFORE INSERT ON t FOR EACH ROW SET NEW.a = 1;
            CREATE TRIGGER g2 BEFORE UPDATE ON t FOR EACH ROW SET NEW.a = 1;
            ALTER TABLE t ADD COLUMN b INT AFTER id;
            ALTER TABLE t ADD COLUMN c INT AFTER missing;
            ALTER TABLE t ADD COLUMN d INT FIRST;
            DROP TRIGGER g1;",
            "mysql",
        );
        assert_eq!(cols(&c, "t"), ["d:INT", "id:INT", "b:INT", "a:INT"]);
        let events = c.row_triggers("t");
        assert!(!events.insert && events.update);
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

    /// A function returning a table's row follows a later change to the table.
    #[test]
    fn test_function_returning_a_row_follows_its_table() {
        let c = pg("CREATE TABLE t (id int);
            CREATE VIEW v1 AS SELECT id FROM t;
            CREATE FUNCTION f() RETURNS t LANGUAGE sql AS 'select 1';
            ALTER TABLE t ADD COLUMN name text;
            CREATE VIEW v2 AS SELECT * FROM f();");
        assert_eq!(view_cols(&c, "v2"), ["id:INT", "name:TEXT"]);
    }

    #[test]
    fn test_a_function_changed_before_a_rebuild_resolves_with_it() {
        let c = pg("CREATE TABLE t (id int);
            CREATE VIEW v1 AS SELECT id FROM t;
            CREATE FUNCTION f() RETURNS SETOF u LANGUAGE sql AS 'select 1';
            CREATE TABLE u (name text);
            CREATE VIEW v2 AS SELECT * FROM f();");
        assert_eq!(view_cols(&c, "v2"), ["name:TEXT"]);
    }

    #[test]
    fn test_a_function_changed_before_a_type_is_resolved_in_the_rebuild() {
        let c = pg("CREATE TABLE t (id int);
            CREATE VIEW v1 AS SELECT id FROM t;
            CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
            CREATE TYPE e AS ENUM ('a');
            CREATE VIEW v2 AS SELECT f() AS n;");
        assert_eq!(view_cols(&c, "v2"), ["n:INT"]);
    }

    #[test]
    fn test_a_view_waiting_with_a_table_is_read_after_it() {
        let c = pg("CREATE TABLE a (LIKE nope); CREATE VIEW v AS SELECT 1 AS x FROM nope2;");
        assert_eq!(view_cols(&c, "v"), ["x:?"]);
    }

    #[test]
    fn test_a_dropped_named_key_leaves_the_others() {
        let c = pg("CREATE TABLE p (id int PRIMARY KEY);
            CREATE TABLE c (pid int,
                CONSTRAINT k FOREIGN KEY (pid) REFERENCES p (id),
                CONSTRAINT k2 FOREIGN KEY (pid) REFERENCES p (id));
            ALTER TABLE c DROP CONSTRAINT k;
            CREATE VIEW v AS SELECT id FROM p;
            CREATE TABLE r (vid int REFERENCES v (id));");
        let tables: Vec<_> = c
            .joins_with_prefix("p", "", 5)
            .iter()
            .map(|j| j.table.name)
            .collect();
        assert_eq!(tables, ["c"]);
        assert_eq!(c.entity("v", "id"), None);
        assert_eq!(c.entity("r", "vid"), None);
    }

    #[test]
    fn test_joins_of_views_unknown_tables_and_dangling_keys() {
        let c = pg("CREATE TABLE p (id int PRIMARY KEY);
            CREATE TABLE c (pid int REFERENCES p (id), x int REFERENCES nope (id));
            CREATE VIEW v AS SELECT id FROM p;");
        assert!(c.joins_with_prefix("nope", "", 5).is_empty());
        assert!(c.joins_with_prefix("v", "", 5).is_empty());
        let tables: Vec<_> = c
            .joins_with_prefix("p", "", 5)
            .iter()
            .map(|j| j.table.name)
            .collect();
        assert_eq!(tables, ["c"]);
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
