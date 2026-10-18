//! Name resolution: table scopes, CTE and view sources, `*` expansion and column lookup.

use std::collections::HashMap;
use std::rc::Rc;

use sqlparser::ast::{
    DataType, Expr, Join, JoinConstraint, JoinOperator, JsonTableColumn, ObjectName, TableFactor,
    TableWithJoins,
};

use crate::catalog::{fold, FunctionReturn};

use super::dialect::SqlDialect;
use super::memo::SubqueryMemo;
use super::names::{alias_name, object_name, table_key};
use super::nullability::Nullability;
use super::query::{rename_columns, Analyzer, Column};
use super::table_functions::table_args;
use super::typed::Typed;
use super::{Inferred, SchemaColumn};

/// What a name in the query can refer to: a FROM item under the name it is read by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Source {
    /// The alias, or the table name when there is none, lower-cased; empty for a source that
    /// cannot be named.
    pub(super) reference: String,
    pub(super) table: Table,
}

/// The relation behind a [`Source`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Table {
    /// A schema table or view, a CTE (by its name), or a derived table or table function
    /// (by `(alias)`): anything whose columns can be looked up.
    Named(String),
    /// A source whose columns the analyzer does not read: a table function, `UNNEST`, a
    /// system catalog, an unaliased subquery. Holds what it is, for messages.
    Opaque(String),
    /// The tables of a DML statement's `FROM`/`USING`: they hide columns from RETURNING
    /// names, but `RETURNING *` lists the target table only.
    ExtraSources,
}

impl Table {
    /// The key the columns of a [`Table::Named`] source are looked up by.
    pub(super) fn named(&self) -> Option<&str> {
        match self {
            Table::Named(key) => Some(key),
            _ => None,
        }
    }

    /// What a source is whose columns are not read, for messages.
    pub(super) fn opaque_description(&self) -> Option<&str> {
        match self {
            Table::Named(_) => None,
            Table::Opaque(description) => Some(description),
            Table::ExtraSources => Some("tables joined to the target with FROM or USING"),
        }
    }
}

impl Source {
    pub(super) fn named(reference: String, key: String) -> Self {
        Source {
            reference,
            table: Table::Named(key),
        }
    }
}

/// The sources a statement reads, in query order.
pub(super) type Scope = Vec<Source>;

pub(super) fn table_scope(factor: &TableFactor, dialect: SqlDialect) -> Scope {
    match factor {
        TableFactor::Table {
            name,
            alias,
            args: None,
            ..
        } => {
            let reference = alias.as_ref().map_or_else(
                || object_name(name),
                |alias| alias.name.value.to_lowercase(),
            );
            vec![Source::named(reference, table_key(name, dialect))]
        }
        other => vec![opaque_source(other)],
    }
}

/// sqlparser reads PostgreSQL's `FROM ONLY a` as a table `only` aliased `a`; this is the
/// table `a` it means.
fn only_table(factor: &TableFactor) -> Option<TableFactor> {
    let TableFactor::Table {
        name,
        alias: Some(alias),
        ..
    } = factor
    else {
        return None;
    };
    let unquoted_only = matches!(name.0.as_slice(), [part]
        if part.as_ident().is_some_and(|i| i.quote_style.is_none() && i.value.eq_ignore_ascii_case("only")));
    if !unquoted_only || !alias.columns.is_empty() {
        return None;
    }
    let mut table = factor.clone();
    if let TableFactor::Table { name, alias, .. } = &mut table {
        *name = ObjectName::from(vec![alias.take()?.name]);
    }
    Some(table)
}

/// The virtual table a derived table's columns are kept under: `(alias)`, or one named
/// after the subquery's place in the statement when it has no alias.
fn derived_table(factor: &TableFactor) -> String {
    match factor {
        TableFactor::Derived {
            alias: Some(alias), ..
        } => format!("({})", alias.name.value.to_lowercase()),
        _ => format!("(#{factor:p})"),
    }
}

/// A FROM item the analyzer cannot read the columns of.
pub(super) fn opaque_source(factor: &TableFactor) -> Source {
    let (alias, description) = match factor {
        TableFactor::Table {
            name,
            alias,
            args: Some(_),
            ..
        } => (alias, format!("table function {}", object_name(name))),
        TableFactor::Table { name, alias, .. } => {
            (alias, format!("system catalog {}", object_name(name)))
        }
        TableFactor::Derived { alias, .. } => (alias, "subquery without an alias".to_string()),
        TableFactor::UNNEST { alias, .. } => (alias, "UNNEST".to_string()),
        TableFactor::TableFunction { alias, .. } | TableFactor::Function { alias, .. } => {
            (alias, "table function".to_string())
        }
        TableFactor::NestedJoin { alias, .. } => (alias, "aliased join".to_string()),
        other => (&None, format!("table expression `{other}`")),
    };
    let reference = alias
        .as_ref()
        .map(|alias| alias.name.value.to_lowercase())
        .unwrap_or_default();
    Source {
        reference,
        table: Table::Opaque(description),
    }
}

/// Sources of a SELECT: each FROM relation followed by its joins (`FROM a, b` counts two),
/// looking into unaliased parentheses such as `FROM (a JOIN b ON ...)`.
pub(super) fn relations(from: &[TableWithJoins]) -> std::vec::IntoIter<&TableFactor> {
    fn push<'a>(table: &'a TableWithJoins, into: &mut Vec<&'a TableFactor>) {
        for factor in
            std::iter::once(&table.relation).chain(table.joins.iter().map(|j| &j.relation))
        {
            match factor {
                TableFactor::NestedJoin {
                    table_with_joins,
                    alias: None,
                } => push(table_with_joins, into),
                other => into.push(other),
            }
        }
    }
    let mut factors = Vec::new();
    for table in from {
        push(table, &mut factors);
    }
    factors.into_iter()
}

/// Whether any join in `from`, nested ones included, merges columns.
pub(super) fn any_merging_join(from: &[TableWithJoins]) -> bool {
    from.iter().any(|table| {
        table.joins.iter().any(merges_columns)
            || match &table.relation {
                TableFactor::NestedJoin {
                    table_with_joins,
                    alias: None,
                } => any_merging_join(std::slice::from_ref(table_with_joins)),
                _ => false,
            }
    })
}

/// The name a relation is referred to by: its alias, else its table or function name.
pub(super) fn relation_name(factor: &TableFactor) -> Option<String> {
    let alias = match factor {
        TableFactor::Table { name, alias, .. } => {
            return Some(alias.as_ref().map_or_else(
                || object_name(name),
                |alias| alias.name.value.to_lowercase(),
            ))
        }
        TableFactor::Function { name, alias, .. } => {
            return Some(alias.as_ref().map_or_else(
                || function_name_of(name),
                |alias| alias.name.value.to_lowercase(),
            ))
        }
        TableFactor::UNNEST { alias, .. } => {
            return Some(alias.as_ref().map_or_else(
                || "unnest".to_string(),
                |alias| alias.name.value.to_lowercase(),
            ))
        }
        TableFactor::Derived { alias, .. } | TableFactor::JsonTable { alias, .. } => alias.as_ref(),
        _ => None,
    };
    alias.map(|alias| alias.name.value.to_lowercase())
}

/// Lowercased names of the columns joins merge, nested joins included; `*` for `NATURAL`.
/// A merged column is not ambiguous, whichever side it is read from.
pub(super) fn merged_columns(from: &[TableWithJoins], dialect: SqlDialect) -> Vec<String> {
    let mut merged = Vec::new();
    for table in from {
        for join in &table.joins {
            match join_constraint(join) {
                Some(JoinConstraint::Natural) => merged.push("*".to_string()),
                Some(JoinConstraint::Using(names)) => merged.extend(
                    names
                        .iter()
                        .filter_map(|name| name.0.last()?.as_ident())
                        .map(|ident| fold(ident, dialect)),
                ),
                _ => {}
            }
        }
        if let TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } = &table.relation
        {
            merged.extend(merged_columns(
                std::slice::from_ref(table_with_joins),
                dialect,
            ));
        }
    }
    merged
}

/// Whether a join merges same-named columns (`USING`, `NATURAL`), so `*` does not list them all.
pub(super) fn merges_columns(join: &Join) -> bool {
    matches!(
        join_constraint(join),
        Some(JoinConstraint::Using(_) | JoinConstraint::Natural)
    )
}

/// The constraint of a join, when its operator has one.
fn join_constraint(join: &Join) -> Option<&JoinConstraint> {
    use JoinOperator::*;
    match &join.join_operator {
        Join(c)
        | Inner(c)
        | Left(c)
        | LeftOuter(c)
        | Right(c)
        | RightOuter(c)
        | FullOuter(c)
        | CrossJoin(c)
        | Semi(c)
        | LeftSemi(c)
        | RightSemi(c)
        | Anti(c)
        | LeftAnti(c)
        | RightAnti(c)
        | StraightJoin(c)
        | AsOf { constraint: c, .. } => Some(c),
        _ => None,
    }
}

/// The table an alias (or table name) refers to.
pub(super) fn resolve(reference: &str, scope: &Scope) -> Table {
    let reference = reference.to_lowercase();
    scope
        .iter()
        .find(|source| source.reference == reference)
        .map_or(Table::Named(reference), |source| source.table.clone())
}

/// Output columns of the CTEs and derived tables in scope, by table key: a CTE's
/// name, or `(alias)` for a derived table.
///
/// Cloned for every SELECT, subquery and CTE the analyzer descends into, so the map and its
/// column lists are shared; an insert copies the map (pointers only) when it is shared.
#[derive(Clone, Default)]
pub(super) struct Virtuals {
    tables: Rc<HashMap<String, Rc<[Column]>>>,
    /// Subqueries analyzed against exactly these tables; any insert starts a fresh memo.
    pub(super) memo: SubqueryMemo,
}

impl Virtuals {
    pub(super) fn get(&self, table: &str) -> Option<&[Column]> {
        self.tables.get(table).map(|columns| &**columns)
    }

    pub(super) fn contains_key(&self, table: &str) -> bool {
        self.tables.contains_key(table)
    }

    pub(super) fn insert(&mut self, table: String, columns: Vec<Column>) {
        Rc::make_mut(&mut self.tables).insert(table, columns.into());
        self.memo = SubqueryMemo::default();
    }
}

/// The lower-case name of a function called in `FROM`, without its schema.
fn function_name_of(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map(|ident| ident.value.to_lowercase())
        .unwrap_or_default()
}

impl<'a> Analyzer<'a> {
    /// Columns of a CTE, derived table or schema view; a CTE hides a view of its name.
    pub(super) fn virtual_columns(&self, table: &str) -> Option<&[Column]> {
        self.virtuals.get(table).or_else(|| self.schema.view(table))
    }

    /// Rows of a call to the function `name` with `args` arguments in `FROM`, when the
    /// schema defines it.
    pub(super) fn function_rows(&self, name: &ObjectName, args: usize) -> Option<Vec<Column>> {
        let column =
            |name: String, typed: Typed| Column::from_typed(name, typed, Nullability::Unknown);
        Some(
            match self.schema.function(&table_key(name, self.dialect), args)? {
                FunctionReturn::Rows(columns) | FunctionReturn::Row(_, columns) => columns
                    .iter()
                    .map(|col| {
                        Column::from_typed(
                            col.name.clone(),
                            col.typed(self.dialect),
                            Nullability::declared(col.not_null),
                        )
                    })
                    .collect(),
                FunctionReturn::Scalar(typed) | FunctionReturn::SetOf(typed) => {
                    vec![column(object_name(name), typed.clone().into())]
                }
            },
        )
    }

    /// Whether a table reference reads a virtual source: a CTE or view by its bare
    /// name, or a view by its schema-qualified name.
    pub(super) fn is_virtual(&self, name: &ObjectName) -> bool {
        let table = table_key(name, self.dialect);
        if name.0.len() == 1 {
            self.virtual_columns(&table).is_some()
        } else {
            self.schema.view(&table).is_some()
        }
    }

    /// Whether `name` is a PostgreSQL system relation the catalog does not model: the
    /// modelled ones (`pg_class`, `information_schema.columns`, ...) are tables with columns.
    pub(super) fn is_system_catalog(&self, name: &ObjectName) -> bool {
        self.schema
            .is_unmodelled_system_relation(&table_key(name, self.dialect))
    }

    /// An analyzer for a subquery of an expression: its names may refer to `scope`.
    pub(super) fn correlated(&self, scope: &Scope) -> Analyzer<'a> {
        let mut inner = self.clone();
        inner.outer = Rc::new([scope.as_slice(), self.outer.as_slice()].concat());
        inner
    }

    /// Scope of a FROM clause, and an analyzer that knows its derived tables. A
    /// `LATERAL` subquery is analyzed with the relations before it.
    pub(super) fn scope_of(&self, from: &[TableWithJoins]) -> Inferred<(Analyzer<'a>, Scope)> {
        let mut analyzer = self.clone();
        analyzer.merged = merged_columns(from, analyzer.dialect).into();
        let mut scope = Scope::new();
        for factor in relations(from) {
            let only = only_table(factor);
            let factor = only.as_ref().unwrap_or(factor);
            if let Some((reference, columns)) = analyzer.defined_columns(factor) {
                let table = format!("({reference})");
                analyzer.virtuals.insert(table.clone(), columns);
                scope.push(Source::named(reference, table));
                continue;
            }
            // A built-in set-returning function, which may read the relations before it.
            let builtin = match factor {
                TableFactor::Table {
                    name,
                    alias,
                    args: Some(args),
                    with_ordinality,
                    ..
                } if analyzer.function_rows(name, args.args.len()).is_none() => {
                    let function = function_name_of(name);
                    analyzer
                        .builtin_rows(&function, &table_args(&args.args), &scope)?
                        .map(|rows| (function, alias, rows, *with_ordinality))
                }
                TableFactor::Function {
                    name,
                    args,
                    alias,
                    with_ordinality,
                    ..
                } => {
                    let function = function_name_of(name);
                    analyzer
                        .builtin_rows(&function, &table_args(args), &scope)?
                        .map(|rows| (function, alias, rows, *with_ordinality))
                }
                TableFactor::UNNEST {
                    alias,
                    array_exprs,
                    with_ordinality,
                    ..
                } => {
                    let arrays: Vec<&Expr> = array_exprs.iter().collect();
                    analyzer
                        .builtin_rows("unnest", &arrays, &scope)?
                        .map(|rows| ("unnest".to_string(), alias, rows, *with_ordinality))
                }
                _ => None,
            };
            if let Some((function, alias, rows, ordinality)) = builtin {
                let columns = rows.named(alias.as_ref(), ordinality, analyzer.dialect);
                let reference = alias
                    .as_ref()
                    .map_or(function, |alias| alias.name.value.to_lowercase());
                let table = format!("({reference})");
                analyzer.virtuals.insert(table.clone(), columns);
                scope.push(Source::named(reference, table));
                continue;
            }
            match factor {
                TableFactor::Table {
                    name,
                    alias,
                    args: Some(args),
                    ..
                } if analyzer.function_rows(name, args.args.len()).is_some() => {
                    let mut columns = analyzer
                        .function_rows(name, args.args.len())
                        .unwrap_or_default();
                    if let Some(alias) = alias {
                        rename_columns(&mut columns, alias, analyzer.dialect);
                    }
                    let reference = alias.as_ref().map_or_else(
                        || object_name(name),
                        |alias| alias.name.value.to_lowercase(),
                    );
                    let table = format!("({reference})");
                    analyzer.virtuals.insert(table.clone(), columns);
                    scope.push(Source::named(reference, table));
                }
                TableFactor::Table { name, alias, .. } if analyzer.is_virtual(name) => {
                    let table = table_key(name, self.dialect);
                    let reference = alias.as_ref().map_or_else(
                        || object_name(name),
                        |alias| alias.name.value.to_lowercase(),
                    );
                    scope.push(Source::named(reference, table));
                }
                TableFactor::Derived {
                    subquery, alias, ..
                } => {
                    // A LATERAL subquery reads the relations before it.
                    let mut inner = analyzer.clone();
                    inner.outer = Rc::new([scope.as_slice(), analyzer.outer.as_slice()].concat());
                    let mut columns = inner
                        .query_columns(subquery)?
                        .unwrap_or_else(|| vec![Column::unknown("*")]);
                    if let Some(alias) = alias {
                        rename_columns(&mut columns, alias, analyzer.dialect);
                    }
                    let table = derived_table(factor);
                    analyzer.virtuals.insert(table.clone(), columns);
                    // Without an alias (SQLite, PostgreSQL 16) its columns are read unqualified.
                    let reference = alias
                        .as_ref()
                        .map(|alias| alias.name.value.to_lowercase())
                        .unwrap_or_default();
                    scope.push(Source::named(reference, table));
                }
                other => {
                    let entries = match other {
                        TableFactor::Table {
                            name, args: None, ..
                        } if analyzer.is_system_catalog(name) => vec![opaque_source(other)],
                        _ => table_scope(other, analyzer.dialect),
                    };
                    for source in &entries {
                        analyzer.check_table(&source.table);
                    }
                    scope.extend(entries);
                }
            }
        }
        Ok((analyzer, scope))
    }

    /// The columns a function in `FROM` declares in its column definition list,
    /// `json_to_recordset(x) AS t(a int, b text)`, or a `JSON_TABLE` in its `COLUMNS`
    /// clause, and its reference. A column can be NULL: a record or document that lacks the
    /// member has a NULL there.
    fn defined_columns(&self, factor: &TableFactor) -> Option<(String, Vec<Column>)> {
        if let TableFactor::JsonTable {
            columns,
            alias: Some(alias),
            ..
        } = factor
        {
            let columns = columns
                .iter()
                .map(|column| match column {
                    JsonTableColumn::Named(column) => Some(Column::from_typed(
                        alias_name(&column.name, self.dialect),
                        self.cast_typed(&column.r#type),
                        // `EXISTS PATH` is 1 or 0.
                        if column.exists {
                            Nullability::NonNull
                        } else {
                            Nullability::Nullable
                        },
                    )),
                    JsonTableColumn::ForOrdinality(name) => Some(Column::from_typed(
                        alias_name(name, self.dialect),
                        self.cast_typed(&DataType::Int(None)),
                        Nullability::NonNull,
                    )),
                    // A nested path repeats the outer columns per element.
                    JsonTableColumn::Nested(_) => None,
                })
                .collect::<Option<Vec<Column>>>()?;
            return Some((alias.name.value.to_lowercase(), columns));
        }
        let (TableFactor::Table {
            alias: Some(alias),
            args: Some(_),
            ..
        }
        | TableFactor::Function {
            alias: Some(alias), ..
        }) = factor
        else {
            return None;
        };
        if alias.columns.is_empty() {
            return None;
        }
        let columns = alias
            .columns
            .iter()
            .map(|column| {
                let typed = self.cast_typed(column.data_type.as_ref()?);
                Some(Column::from_typed(
                    alias_name(&column.name, self.dialect),
                    typed,
                    Nullability::Nullable,
                ))
            })
            .collect::<Option<Vec<Column>>>()?;
        Some((alias.name.value.to_lowercase(), columns))
    }

    /// Whether `*` (or `t.*`) reads only sources with known columns: schema tables, and
    /// CTEs and derived tables whose own columns are known.
    ///
    /// `USING`/`NATURAL` joins merge columns, so their stars are unresolved.
    pub(super) fn star_resolvable(&self, from: &[TableWithJoins], reference: Option<&str>) -> bool {
        if reference.is_none() && any_merging_join(from) {
            return false;
        }
        let reference = reference.map(str::to_lowercase);
        let mut matched = relations(from)
            .filter(|factor| reference.is_none() || relation_name(factor) == reference)
            .peekable();
        matched.peek().is_some()
            && matched.all(|factor| match factor {
                TableFactor::Table { name, .. } => self
                    .virtual_columns(&table_key(name, self.dialect))
                    .is_none_or(|columns| !columns.iter().any(Column::is_unknown_star)),
                TableFactor::Derived { .. } => self
                    .virtuals
                    .get(&derived_table(factor))
                    .is_some_and(|columns| !columns.iter().any(Column::is_unknown_star)),
                TableFactor::UNNEST { .. }
                | TableFactor::Function { .. }
                | TableFactor::JsonTable { .. } => relation_name(factor)
                    .is_some_and(|name| self.virtuals.contains_key(&format!("({name})"))),
                _ => false,
            })
    }

    /// Sources a star reads: the one `reference` names, or every scope table.
    pub(super) fn star_sources(reference: Option<&str>, scope: &Scope) -> Vec<Source> {
        match reference {
            Some(reference) => vec![Source {
                reference: reference.to_lowercase(),
                table: resolve(reference, scope),
            }],
            None => scope
                .iter()
                .filter(|source| source.table != Table::ExtraSources)
                .cloned()
                .collect(),
        }
    }

    /// Tables a star reads: the one `reference` names, or every scope table.
    pub(super) fn star_tables(reference: Option<&str>, scope: &Scope) -> Vec<Table> {
        Self::star_sources(reference, scope)
            .into_iter()
            .map(|source| source.table)
            .collect()
    }

    /// Expand `*` (all scope tables) or `t.*` from the sources, or `None` if unknown.
    pub(super) fn star_columns(
        &self,
        reference: Option<&str>,
        scope: &Scope,
    ) -> Option<Vec<Column>> {
        let sources = Self::star_sources(reference, scope);
        if sources.is_empty() {
            return None;
        }
        let columns = sources
            .iter()
            .map(|Source { reference, table }| {
                let table = table.named()?;
                let mut columns = self
                    .virtual_columns(table)
                    .map(<[Column]>::to_vec)
                    .or_else(|| {
                        self.schema.table(table).map(|columns| {
                            columns
                                .iter()
                                .map(|col| {
                                    Column::from_typed(
                                        col.name.clone(),
                                        col.typed(self.dialect),
                                        Nullability::declared(col.not_null),
                                    )
                                })
                                .collect()
                        })
                    })?;
                for column in &mut columns {
                    let key = self.column_key(reference, &column.name, scope);
                    column.nullability = self.facts.column(reference, &key, column.nullability);
                }
                Some(columns)
            })
            .collect::<Option<Vec<Vec<Column>>>>()?;
        Some(columns.into_iter().flatten().collect())
    }

    /// Expand `*` or `t.*` into schema columns, as `SELECT *` rewriting writes them.
    pub(super) fn schema_star_columns(
        &self,
        reference: Option<&str>,
        scope: &Scope,
    ) -> Option<Vec<SchemaColumn>> {
        let tables = Self::star_tables(reference, scope);
        if tables.is_empty() {
            return None;
        }
        let columns = tables
            .iter()
            .map(|table| self.schema.table(table.named()?))
            .collect::<Option<Vec<_>>>()?;
        Some(columns.into_iter().flatten().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::ast::{SetExpr, Statement};

    use super::super::test_support::{reports, statement};
    use super::*;

    fn from_of(sql: &str) -> Vec<TableWithJoins> {
        let Statement::Query(query) = statement(sql, SqlDialect::Postgres) else {
            panic!("not a query");
        };
        let SetExpr::Select(select) = *query.body else {
            panic!("not a select");
        };
        select.from
    }

    fn scope_of(sql: &str) -> Vec<String> {
        let from = from_of(sql);
        relations(&from)
            .flat_map(|factor| table_scope(factor, SqlDialect::Postgres))
            .map(|source| format!("{}={:?}", source.reference, source.table))
            .collect()
    }

    #[test]
    fn named_tables_are_read_by_alias_or_name() {
        assert_eq!(scope_of("SELECT 1 FROM Users"), ["users=Named(\"users\")"]);
        assert_eq!(scope_of("SELECT 1 FROM users AS U"), ["u=Named(\"users\")"]);
        assert_eq!(
            scope_of("SELECT 1 FROM public.users"),
            ["users=Named(\"public.users\")"]
        );
    }

    #[test]
    fn sources_that_cannot_be_read_are_opaque() {
        assert_eq!(
            scope_of("SELECT 1 FROM generate_series(1, 3) AS g"),
            ["g=Opaque(\"table function generate_series\")"]
        );
        assert_eq!(
            scope_of("SELECT 1 FROM (SELECT 1)"),
            ["=Opaque(\"subquery without an alias\")"]
        );
        assert_eq!(
            scope_of("SELECT 1 FROM unnest(ARRAY[1]) AS u"),
            ["u=Opaque(\"UNNEST\")"]
        );
    }

    #[test]
    fn only_is_a_modifier_not_a_table() {
        let from = from_of("SELECT 1 FROM ONLY a");
        let TableFactor::Table { name, alias, .. } = &from[0].relation else {
            panic!("not a table");
        };
        assert_eq!(object_name(name), "only");
        assert!(alias.is_some());
        let table = only_table(&from[0].relation).expect("rewritten to the table a");
        assert_eq!(relation_name(&table).as_deref(), Some("a"));
        assert!(only_table(&from_of("SELECT 1 FROM a")[0].relation).is_none());
    }

    #[test]
    fn relations_lists_joins_and_looks_inside_unaliased_parentheses() {
        let names = |sql: &str| {
            let from = from_of(sql);
            relations(&from)
                .filter_map(relation_name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("SELECT 1 FROM a, b"), ["a", "b"]);
        assert_eq!(
            names("SELECT 1 FROM a JOIN b ON true LEFT JOIN c ON true"),
            ["a", "b", "c"]
        );
        assert_eq!(
            names("SELECT 1 FROM (a JOIN b ON true) JOIN c ON true"),
            ["a", "b", "c"]
        );
        assert_eq!(names("SELECT 1 FROM (SELECT 1) AS s, a"), ["s", "a"]);
    }

    #[test]
    fn relation_name_is_the_alias_or_table_name() {
        let name = |sql: &str| relation_name(&from_of(sql)[0].relation);
        assert_eq!(name("SELECT 1 FROM Public.T").as_deref(), Some("t"));
        assert_eq!(name("SELECT 1 FROM t AS X").as_deref(), Some("x"));
        assert_eq!(name("SELECT 1 FROM (SELECT 1) AS Q").as_deref(), Some("q"));
        assert_eq!(name("SELECT 1 FROM (SELECT 1)"), None);
        assert_eq!(
            name("SELECT 1 FROM generate_series(1, 2)"),
            Some("generate_series".into())
        );
        assert_eq!(
            name("SELECT 1 FROM unnest(ARRAY[1])").as_deref(),
            Some("unnest")
        );
        assert_eq!(
            name("SELECT 1 FROM JSON_TABLE('[]', '$' COLUMNS (a int PATH '$')) AS J").as_deref(),
            Some("j")
        );
    }

    #[test]
    fn merging_joins_are_using_and_natural() {
        let merging = |sql: &str| any_merging_join(&from_of(sql));
        assert!(merging("SELECT 1 FROM a JOIN b USING (id)"));
        assert!(merging("SELECT 1 FROM a NATURAL JOIN b"));
        assert!(merging(
            "SELECT 1 FROM (a JOIN b USING (id)) JOIN c ON true"
        ));
        assert!(!merging("SELECT 1 FROM a JOIN b ON a.id = b.id"));
        assert!(!merging("SELECT 1 FROM a, b"));
        assert!(!merging("SELECT 1 FROM a CROSS JOIN b"));
    }

    #[test]
    fn merged_columns_lists_using_columns_and_marks_natural() {
        let merged = |sql: &str| merged_columns(&from_of(sql), SqlDialect::Postgres);
        assert_eq!(
            merged("SELECT 1 FROM a JOIN b USING (Id, kind)"),
            ["id", "kind"]
        );
        assert_eq!(merged("SELECT 1 FROM a NATURAL JOIN b"), ["*"]);
        assert_eq!(
            merged("SELECT 1 FROM (a JOIN b USING (x)) JOIN c USING (y)"),
            ["y", "x"]
        );
        assert!(merged("SELECT 1 FROM a JOIN b ON true").is_empty());
    }

    #[test]
    fn resolve_finds_an_alias_or_falls_back_to_the_name() {
        let scope = vec![
            Source::named("u".into(), "users".into()),
            Source {
                reference: "g".into(),
                table: Table::Opaque("table function".into()),
            },
        ];
        assert_eq!(resolve("U", &scope), Table::Named("users".into()));
        assert_eq!(resolve("g", &scope), Table::Opaque("table function".into()));
        assert_eq!(resolve("Other", &scope), Table::Named("other".into()));
    }

    #[test]
    fn table_describes_what_it_cannot_read() {
        assert_eq!(Table::Named("t".into()).named(), Some("t"));
        assert_eq!(Table::Named("t".into()).opaque_description(), None);
        assert_eq!(Table::Opaque("UNNEST".into()).named(), None);
        assert_eq!(
            Table::Opaque("UNNEST".into()).opaque_description(),
            Some("UNNEST")
        );
        assert_eq!(
            Table::ExtraSources.opaque_description(),
            Some("tables joined to the target with FROM or USING")
        );
    }

    #[test]
    fn virtuals_share_until_inserted_into() {
        let mut first = Virtuals::default();
        assert!(!first.contains_key("c"));
        first.insert("c".into(), vec![Column::unknown("a")]);
        let second = first.clone();
        first.insert("d".into(), Vec::new());
        assert!(second.contains_key("c"));
        assert!(!second.contains_key("d"));
        assert_eq!(first.get("c").map(<[Column]>::len), Some(1));
        assert!(first.get("d").is_some_and(<[Column]>::is_empty));
    }

    const DDL: &str = "CREATE TABLE a (id INT NOT NULL, x TEXT NOT NULL, shared TEXT); \
                       CREATE TABLE b (id INT NOT NULL, y TEXT, shared TEXT NOT NULL); \
                       CREATE TABLE c (id INT, z TEXT NOT NULL); \
                       CREATE VIEW v AS SELECT id, x FROM a";

    #[test]
    fn star_expansion_over_joins() {
        let queries = [
            "SELECT * FROM a",
            "SELECT * FROM a JOIN b ON a.id = b.id",
            "SELECT * FROM a JOIN b USING (id)",
            "SELECT * FROM a NATURAL JOIN b",
            "SELECT a.*, b.y FROM a, b",
            "SELECT b.* FROM a JOIN b ON true",
            "SELECT * FROM a, b, c",
            "SELECT * FROM (a JOIN b USING (id)) JOIN c USING (id)",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn nullability_follows_the_join_kind() {
        let queries = [
            "SELECT a.id, b.id AS bid FROM a LEFT JOIN b ON a.id = b.id",
            "SELECT a.id, b.id AS bid FROM a RIGHT JOIN b ON a.id = b.id",
            "SELECT a.id, b.id AS bid FROM a FULL JOIN b ON a.id = b.id",
            "SELECT a.id, b.id AS bid FROM a INNER JOIN b ON a.id = b.id",
            "SELECT id FROM a LEFT JOIN b USING (id)",
            "SELECT id FROM a RIGHT JOIN b USING (id)",
            "SELECT b.* FROM a LEFT JOIN b ON true",
            "SELECT c.z FROM a LEFT JOIN b ON true LEFT JOIN c ON true",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn derived_tables_ctes_and_views() {
        let queries = [
            "SELECT * FROM (SELECT id, x FROM a) AS d",
            "SELECT d.n FROM (SELECT id AS n FROM a) AS d",
            "SELECT * FROM (SELECT id, x FROM a) AS d(k, v)",
            "WITH w AS (SELECT id FROM a) SELECT * FROM w",
            "WITH w(k) AS (SELECT id FROM a) SELECT k FROM w",
            "WITH a AS (SELECT 1 AS only_here) SELECT * FROM a",
            "SELECT * FROM v",
            "WITH v AS (SELECT 'shadow'::TEXT AS s) SELECT * FROM v",
            "SELECT * FROM (SELECT 1)",
            "SELECT s.* FROM (SELECT 1 AS one) AS s",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn opaque_sources_explain_themselves() {
        let queries = [
            "SELECT * FROM generate_series(1, 3) AS g",
            "SELECT g FROM generate_series(1, 3) AS g",
            "SELECT * FROM unnest(ARRAY[1, 2]) AS u(n)",
            "SELECT * FROM pg_catalog.pg_tables",
            "SELECT tablename FROM pg_tables",
            "SELECT a.id, g FROM a, generate_series(1, 3) AS g",
            "SELECT * FROM pg_get_keywords() AS k",
            "SELECT k.word FROM pg_get_keywords() AS k",
            "SELECT * FROM a, LATERAL (SELECT a.id + 1 AS next) AS l",
            "SELECT * FROM missing",
            "SELECT * FROM missing AS m JOIN a ON true",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn correlated_subqueries_see_the_outer_scope() {
        let queries = [
            "SELECT (SELECT max(b.id) FROM b WHERE b.id = a.id) AS m FROM a",
            "SELECT (SELECT x) AS m FROM a",
            "SELECT EXISTS (SELECT 1 FROM b WHERE b.id = a.id) AS e FROM a",
            "SELECT * FROM a WHERE id IN (SELECT id FROM b WHERE b.shared = a.shared)",
            "SELECT (SELECT y FROM b WHERE b.id = a.nope) AS m FROM a",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn table_names_fold_by_dialect() {
        let ddl_postgres = "CREATE TABLE \"Mixed\" (id INT NOT NULL); CREATE TABLE lower (id INT)";
        let queries = [
            "SELECT id FROM \"Mixed\"",
            "SELECT id FROM Mixed",
            "SELECT id FROM LOWER",
            "SELECT id FROM public.lower",
            "SELECT id FROM other.lower",
        ];
        let postgres = reports(&queries, "postgres", Some(ddl_postgres));
        let ddl_mysql = "CREATE TABLE Mixed (id INT NOT NULL)";
        let mysql = reports(&queries[..2], "mysql", Some(ddl_mysql));
        insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
    }
}
